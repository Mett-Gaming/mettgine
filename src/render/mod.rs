//! Vulkan-Renderer.
//!
//! Pass 1 (480x270 Offscreen): statische Welt-Boxen, Meshes (Modelle + dynamische Boxen), UI.
//! Pass 2 (Swapchain): VHS-Post-Pass. Dynamic Rendering (Vulkan 1.3).

pub(crate) mod context;
pub(crate) mod pipeline;
pub(crate) mod present;
pub(crate) mod swapchain;
pub(crate) mod timing;
pub(crate) mod transfer;
pub(crate) mod util;

use crate::cull::{Frustum, SkinBounds, Sphere};
use crate::gpu::{GpuInstance, GpuLight, MeshData};
use crate::profile::FrameStats;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use context::Context;
use glam::{Mat4, Vec3, Vec4};
use pipeline::{create_pipeline, Blend, PipelineDesc};
use present::{Capture, Frame};
use std::mem::size_of;
use swapchain::Swapchain;
use timing::GpuTimer;
use transfer::{set_viewport, upload_r8};
use util::{image_barrier, Buffer, Image};
use winit::window::Window;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub const RENDER_WIDTH: u32 = 480;
pub const RENDER_HEIGHT: u32 = 270;
const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;
const FRAMES_IN_FLIGHT: usize = 2;
const MAX_MESH_DRAWS: usize = 4096;
const MAX_UI_VERTS: usize = 60_000;
/// Knochen-Matrizen pro Frame (animierte Figuren)
const MAX_BONES: usize = 16_384;
/// Aufloesung des Ueberwachungsmonitors (zweite Kamera)
pub const MONITOR_WIDTH: u32 = 160;
pub const MONITOR_HEIGHT: u32 = 90;

macro_rules! spv {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".spv"))
    };
}

// Teile des Renderers (nach dem spv!-Makro, das sie benutzen)
mod draw;
mod frame;
mod init;
mod monitor_pass;
mod pipelines;
mod post_pass;
mod upload;

pub use pipelines::WorldShaders;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WorldPush {
    view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    params: [f32; 4],
    fog_color: [f32; 4],
    /// Live-Verbiegen: seitlich, hoch, Gangbeginn z, Gangende z
    sway: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PostPush {
    params: [f32; 4],
    res: [f32; 4],
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuMeshInst {
    model: [[f32; 4]; 4],
    tint: [f32; 4],
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct UiVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// Ein Modell (oder eine dynamische Box) fuer diesen Frame.
#[derive(Clone, Copy)]
pub struct MeshDraw {
    pub mesh: usize,
    pub model: Mat4,
    /// rgb = Farbe (multipliziert), a = Eigenleuchten
    pub tint: Vec4,
}

/// Eine animierte Figur: Mesh `skin` (Index in den Figuren), Knochen ab `first_bone` im Knochen-Puffer.
#[derive(Clone, Copy, Debug)]
pub struct SkinDraw {
    pub skin: usize,
    pub model: Mat4,
    /// rgb = Farbmultiplikator, w = Eigenleuchten
    pub tint: Vec4,
    pub first_bone: u32,
    /// halbtransparent (Hologramm)
    pub ghost: bool,
    /// Zerreissen im Shader (0..1)
    pub distort: f32,
}

pub struct FrameParams {
    pub view_proj: Mat4,
    pub cam_pos: Vec3,
    pub time: f32,
    pub tipped: bool,
    pub fog_color: Vec3,
    pub fog_density: f32,
    pub glitch: f32,
    pub vhs: f32,
    pub fade: f32,
    pub red: f32,
    /// Verfall der Werbeposter (0..1)
    pub decay: f32,
    /// Live-Verbiegen der Gaenge (siehe world::sway)
    pub sway: Vec4,
    /// Farbumkehr (0..1) und Ueberbelichtung (0..1) beim Schreck
    pub invert: f32,
    pub flash: f32,
    /// Kamera des Ueberwachungsmonitors (view_proj, Position); None = Bild bleibt schwarz
    pub monitor: Option<(Mat4, Vec3)>,
}

pub struct Renderer {
    swapchain: Swapchain,
    color: Image,
    depth: Image,
    font: Image,
    ui_target: Image,
    /// Bild der Ueberwachungskamera und seine Tiefe
    monitor: Image,
    monitor_depth: Image,
    sampler: vk::Sampler,
    /// Schild-Atlas (Schriftzuege im Markt), linear gefiltert
    signs: Image,
    sign_sampler: vk::Sampler,
    scene_set_layout: vk::DescriptorSetLayout,
    tex_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    world_set: vk::DescriptorSet,
    /// wie world_set, aber ohne Monitor-Bild (fuer den Kamera-Pass, der es gerade beschreibt)
    world_set_cam: vk::DescriptorSet,
    mesh_sets: Vec<vk::DescriptorSet>,
    /// statische Mesh-Instanzen (Waren in den Regalen), nach Modell sortiert
    static_set: vk::DescriptorSet,
    static_buf: Buffer,
    static_ranges: Vec<(u32, u32, u32)>,
    post_set: vk::DescriptorSet,
    ui_set: vk::DescriptorSet,
    scene_layout: vk::PipelineLayout,
    post_layout: vk::PipelineLayout,
    ui_layout: vk::PipelineLayout,
    world_pipeline: vk::Pipeline,
    mesh_pipeline: vk::Pipeline,
    ghost_pipeline: vk::Pipeline,
    skin_pipeline: vk::Pipeline,
    skin_ghost_pipeline: vk::Pipeline,
    skin_vertex_buf: Buffer,
    skin_ranges: Vec<(u32, u32)>,
    /// Knochen-Matrizen je Frame im Flug
    bone_bufs: Vec<Buffer>,
    ui_pipeline: vk::Pipeline,
    post_pipeline: vk::Pipeline,
    command_pool: vk::CommandPool,
    frames: Vec<Frame>,
    frame_index: usize,
    instance_buf: Buffer,
    light_buf: Buffer,
    instance_count: u32,
    light_count: u32,
    mesh_vertex_buf: Buffer,
    mesh_ranges: Vec<(u32, u32)>,
    mesh_inst_bufs: Vec<Buffer>,
    ui_bufs: Vec<Buffer>,
    window_size: (u32, u32),
    needs_recreate: bool,
    capture: Capture,
    timer: GpuTimer,
    stats: FrameStats,
    mesh_bounds: Vec<Sphere>,
    skin_bounds: Vec<SkinBounds>,
    /// im Kamerabild: Modelle, dann Hologramme, dann feste und halbtransparente Figuren
    visible: Vec<bool>,
    // Muss zuletzt freigegeben werden (Drop-Reihenfolge der Felder).
    ctx: Context,
}

fn binding(
    b: u32,
    ty: vk::DescriptorType,
    stages: vk::ShaderStageFlags,
) -> vk::DescriptorSetLayoutBinding<'static> {
    vk::DescriptorSetLayoutBinding::default()
        .binding(b)
        .descriptor_type(ty)
        .descriptor_count(1)
        .stage_flags(stages)
}

impl Renderer {
    /// `font` ist ein 128x128-Graustufen-Atlas (16x16 Zeichen a 8x8 Pixel).
    /// `signs` = (R8-Pixel, Breite, Hoehe) des Schild-Atlas.
    pub fn new(
        window: &Window,
        meshes: &[MeshData],
        skins: &[crate::gpu::SkinData],
        font: &[u8],
        signs: (&[u8], u32, u32),
    ) -> Result<Self> {
        let world = WorldShaders::default();
        Self::with_world_shaders(window, meshes, skins, font, signs, &world)
    }

    /// Name der Grafikkarte samt Treiber.
    pub fn gpu_name(&self) -> &str {
        &self.ctx.gpu_name
    }

    /// Speichert das naechste fertige Bild (nach VHS-Pass) als BMP.
    pub fn capture_next(&mut self, path: std::path::PathBuf) {
        self.capture.request(path);
    }

    /// Messwerte des letzten Bildes (Auswertung: `profile::Meter`).
    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.window_size = (width, height);
        self.needs_recreate = true;
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let d = &self.ctx.device;
            let _ = d.device_wait_idle();
            present::destroy_frames(d, &self.frames);
            d.destroy_command_pool(self.command_pool, None);
            self.instance_buf.destroy(d);
            self.static_buf.destroy(d);
            self.light_buf.destroy(d);
            self.mesh_vertex_buf.destroy(d);
            self.skin_vertex_buf.destroy(d);
            for b in &self.bone_bufs {
                b.destroy(d);
            }
            self.capture.destroy(d);
            self.timer.destroy(d);
            for b in self.mesh_inst_bufs.iter().chain(self.ui_bufs.iter()) {
                b.destroy(d);
            }
            for p in [
                self.world_pipeline,
                self.mesh_pipeline,
                self.ghost_pipeline,
                self.skin_pipeline,
                self.skin_ghost_pipeline,
                self.ui_pipeline,
                self.post_pipeline,
            ] {
                d.destroy_pipeline(p, None);
            }
            for l in [self.scene_layout, self.post_layout, self.ui_layout] {
                d.destroy_pipeline_layout(l, None);
            }
            d.destroy_descriptor_pool(self.descriptor_pool, None);
            d.destroy_descriptor_set_layout(self.scene_set_layout, None);
            d.destroy_descriptor_set_layout(self.tex_set_layout, None);
            d.destroy_sampler(self.sampler, None);
            d.destroy_sampler(self.sign_sampler, None);
            self.signs.destroy(d);
            self.color.destroy(d);
            self.depth.destroy(d);
            self.font.destroy(d);
            self.ui_target.destroy(d);
            self.monitor.destroy(d);
            self.monitor_depth.destroy(d);
            self.swapchain.destroy(&self.ctx);
        }
    }
}
