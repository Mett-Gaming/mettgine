//! Szenen-Renderer fuer 3D-Spiele unter freiem Himmel (zuerst fuer HACKEPETER).
//!
//! Unabhaengig vom KASSE-0-Renderer in `render` (der bleibt unveraendert); geteilt werden nur
//! Vulkan-Grundlagen (`render::context`, `render::swapchain`, `render::util`) und die
//! Datenformate aus `gpu`.
//!
//! Ablauf pro Bild (Vulkan 1.3, Dynamic Rendering):
//! - Pass 0: Schattenkarte aus Sicht der Sonne (Modelle + Figuren, nur Tiefe)
//! - Pass 1: Szene in ein Offscreen-Bild (Fenstergroesse x Render-Skalierung): Himmel, Modelle
//!   (nach Modell instanziert), Figuren (Skinning)
//! - Pass 2: Nachbearbeitung auf die Swapchain (weiche Lichter, Vignette, Blenden), danach die
//!   UI obendrauf in virtuellen Einheiten (Hoehe [`UI_HEIGHT`])
//!
//! Shader: `shaders/scene_*.wgsl` und `_scene.wgsl`. Schnittstelle: Set 0 mit binding 0 =
//! Instanzen ([`GpuInst`]), 1 = Knochen, 2 = [`Globals`] (Uniform), 3 = Schattenkarte,
//! 4 = Vergleichs-Sampler.

mod descriptors;
mod draw;
mod frame;
mod init;
mod params;
mod pipelines;
mod screen;

use crate::render::context::Context;
use crate::render::present::{self, Capture, Frame};
use crate::render::swapchain::Swapchain;
use crate::render::util::{Buffer, Image};
pub use crate::render::{Result, UiVertex};
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::Mat4;
pub use params::{light_view_proj, perspective, MeshDraw, SceneParams, SkinDraw};

const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;
const FRAMES_IN_FLIGHT: usize = 2;
/// Hoechstzahl Modell- plus Figuren-Instanzen pro Bild
pub const MAX_INSTANCES: usize = 8192;
/// Hoechstzahl UI-Vertices pro Bild
pub const MAX_UI_VERTS: usize = 120_000;
/// Hoechstzahl Knochen-Matrizen pro Bild (alle Figuren zusammen)
pub const MAX_BONES: usize = 8192;
/// Kantenlaenge der Schattenkarte (Texel)
pub const SHADOW_SIZE: u32 = 2048;
/// Virtuelle UI-Hoehe; die Breite folgt dem Seitenverhaeltnis des Fensters
pub const UI_HEIGHT: f32 = 720.0;

/// Szenen-Daten als Uniform (binding 2), gleich aufgebaut wie `Globals` in `_scene.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    light_vp: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sky_top: [f32; 4],
    horizon: [f32; 4],
    ground: [f32; 4],
}

/// Eine Instanz (binding 0), gleich aufgebaut wie `Inst` in `_scene.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuInst {
    model: [[f32; 4]; 4],
    tint: [f32; 4],
    /// x = erster Knochen (Figuren), y = Blinken
    extra: [f32; 4],
}

/// Zusammenhaengende Instanzen eines Modells (ein instanzierter Draw-Call).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Group {
    mesh: u32,
    first_inst: u32,
    count: u32,
}

/// Alle Pipelines und Layouts des Szenen-Renderers.
struct Pipelines {
    scene_layout: vk::PipelineLayout,
    post_layout: vk::PipelineLayout,
    ui_layout: vk::PipelineLayout,
    sky: vk::Pipeline,
    mesh: vk::Pipeline,
    skin: vk::Pipeline,
    shadow: vk::Pipeline,
    shadow_skin: vk::Pipeline,
    ui: vk::Pipeline,
    post: vk::Pipeline,
}

pub struct SceneRenderer {
    swapchain: Swapchain,
    /// Offscreen-Ziel der Szene und seine Tiefe (Groesse = Fenster x Skalierung)
    color: Image,
    depth: Image,
    offscreen: vk::Extent2D,
    render_scale: f32,
    shadow: Image,
    /// 1x1-Tiefe als Platzhalter an binding 3 im Schatten-Pass (dort wird die echte Karte beschrieben)
    dummy_shadow: Image,
    font: Image,
    linear: vk::Sampler,
    shadow_sampler: vk::Sampler,
    scene_set_layout: vk::DescriptorSetLayout,
    tex_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    main_sets: Vec<vk::DescriptorSet>,
    shadow_sets: Vec<vk::DescriptorSet>,
    post_set: vk::DescriptorSet,
    ui_set: vk::DescriptorSet,
    pipes: Pipelines,
    command_pool: vk::CommandPool,
    frames: Vec<Frame>,
    frame_index: usize,
    mesh_vertex_buf: Buffer,
    mesh_ranges: Vec<(u32, u32)>,
    skin_vertex_buf: Buffer,
    skin_ranges: Vec<(u32, u32)>,
    inst_bufs: Vec<Buffer>,
    bone_bufs: Vec<Buffer>,
    globals_bufs: Vec<Buffer>,
    ui_bufs: Vec<Buffer>,
    /// wiederverwendete Puffer fuer den Bild-Takt
    scratch_insts: Vec<GpuInst>,
    scratch_bones: Vec<[[f32; 4]; 4]>,
    window_size: (u32, u32),
    needs_recreate: bool,
    capture: Capture,
    // Muss zuletzt freigegeben werden (Drop-Reihenfolge der Felder).
    ctx: Context,
}

fn offscreen_extent(window: (u32, u32), scale: f32) -> vk::Extent2D {
    vk::Extent2D {
        width: ((window.0 as f32 * scale) as u32).max(16),
        height: ((window.1 as f32 * scale) as u32).max(16),
    }
}

impl SceneRenderer {
    /// Name der Grafikkarte samt Treiber.
    pub fn gpu_name(&self) -> &str {
        &self.ctx.gpu_name
    }

    /// Speichert das naechste fertige Bild als BMP.
    pub fn capture_next(&mut self, path: std::path::PathBuf) {
        self.capture.request(path);
    }

    /// Fenstergroesse hat sich geaendert (Pixel).
    pub fn resize(&mut self, width: u32, height: u32) {
        self.window_size = (width, height);
        self.needs_recreate = true;
    }

    /// Aufloesung der Szene relativ zum Fenster (0.25..2).
    pub fn set_render_scale(&mut self, scale: f32) {
        let s = scale.clamp(0.25, 2.0);
        if (s - self.render_scale).abs() > 1e-3 {
            self.render_scale = s;
            self.needs_recreate = true;
        }
    }

    /// Breite der virtuellen UI-Flaeche (Hoehe = [`UI_HEIGHT`]).
    pub fn ui_width(&self) -> f32 {
        UI_HEIGHT * self.aspect()
    }

    /// Seitenverhaeltnis des Fensters (Breite / Hoehe).
    pub fn aspect(&self) -> f32 {
        let (w, h) = self.window_size;
        w.max(1) as f32 / h.max(1) as f32
    }
}

impl Drop for SceneRenderer {
    fn drop(&mut self) {
        // SAFETY: Warten auf das Geraet, danach nutzt die GPU keine der Ressourcen mehr;
        // jede wird hier genau einmal freigegeben.
        unsafe {
            let d = &self.ctx.device;
            let _ = d.device_wait_idle();
            present::destroy_frames(d, &self.frames);
            d.destroy_command_pool(self.command_pool, None);
            self.mesh_vertex_buf.destroy(d);
            self.skin_vertex_buf.destroy(d);
            for b in self
                .inst_bufs
                .iter()
                .chain(&self.bone_bufs)
                .chain(&self.globals_bufs)
                .chain(&self.ui_bufs)
            {
                b.destroy(d);
            }
            self.capture.destroy(d);
            let p = &self.pipes;
            for pipe in [p.sky, p.mesh, p.skin, p.shadow, p.shadow_skin, p.ui, p.post] {
                d.destroy_pipeline(pipe, None);
            }
            for l in [p.scene_layout, p.post_layout, p.ui_layout] {
                d.destroy_pipeline_layout(l, None);
            }
            d.destroy_descriptor_pool(self.descriptor_pool, None);
            d.destroy_descriptor_set_layout(self.scene_set_layout, None);
            d.destroy_descriptor_set_layout(self.tex_set_layout, None);
            d.destroy_sampler(self.linear, None);
            d.destroy_sampler(self.shadow_sampler, None);
            self.color.destroy(d);
            self.depth.destroy(d);
            self.shadow.destroy(d);
            self.dummy_shadow.destroy(d);
            self.font.destroy(d);
            self.swapchain.destroy(&self.ctx);
        }
    }
}

#[cfg(test)]
mod tests;
