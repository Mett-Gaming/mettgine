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

mod draw;
mod frame;
mod helpers;
mod init;
mod pipelines;

use crate::render::context::Context;
use crate::render::swapchain::Swapchain;
use crate::render::util::{Buffer, Image};
pub use crate::render::{Result, UiVertex};
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};

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

/// Ein Modell fuer dieses Bild.
#[derive(Clone, Copy, Debug)]
pub struct MeshDraw {
    /// Index in die Modelle, die `SceneRenderer::new` bekommen hat
    pub mesh: usize,
    pub model: Mat4,
    /// rgb = Farbmultiplikator, a = Eigenleuchten
    pub tint: Vec4,
    /// pulsierendes Aufleuchten (Warnung), 0..1
    pub blink: f32,
    /// wirft Schatten
    pub shadow: bool,
}

impl MeshDraw {
    pub fn new(mesh: usize, model: Mat4) -> Self {
        Self {
            mesh,
            model,
            tint: Vec4::new(1.0, 1.0, 1.0, 0.0),
            blink: 0.0,
            shadow: true,
        }
    }
    /// Farbmultiplikator setzen (Eigenleuchten bleibt).
    pub fn tint(mut self, rgb: Vec3) -> Self {
        self.tint = rgb.extend(self.tint.w);
        self
    }
    /// Eigenleuchten (0 = aus, 1 = volle Eigenfarbe dazu).
    pub fn glow(mut self, g: f32) -> Self {
        self.tint.w = g;
        self
    }
    pub fn blink(mut self, b: f32) -> Self {
        self.blink = b;
        self
    }
    pub fn no_shadow(mut self) -> Self {
        self.shadow = false;
        self
    }
}

/// Eine animierte Figur: ihre Knochen stehen ab `first_bone` in den Knochen des Bildes.
#[derive(Clone, Copy, Debug)]
pub struct SkinDraw {
    /// Index in die Figuren, die `SceneRenderer::new` bekommen hat
    pub skin: usize,
    pub model: Mat4,
    /// rgb = Farbmultiplikator, a = Eigenleuchten
    pub tint: Vec4,
    pub first_bone: u32,
    /// pulsierendes Aufleuchten, 0..1
    pub blink: f32,
}

/// Licht, Kamera und Blenden eines Bildes. Farben in Anzeigewerten (0..1).
#[derive(Clone, Copy, Debug)]
pub struct SceneParams {
    /// Kamera (Projektion mit nach oben zeigendem Bild, siehe [`perspective`])
    pub view_proj: Mat4,
    pub cam_pos: Vec3,
    /// laufende Zeit in Sekunden (fuer Blinken)
    pub time: f32,
    /// Richtung zur Sonne (wird normiert)
    pub sun_dir: Vec3,
    pub sun_color: Vec3,
    /// Bereich der Schattenkarte: Mittelpunkt und halbe Kantenlaenge (Meter)
    pub shadow_center: Vec3,
    pub shadow_radius: f32,
    /// 0 = keine Schatten (Pass faellt weg), 1 = volle Schatten
    pub shadow_strength: f32,
    pub sky_top: Vec3,
    /// Horizont- und Nebelfarbe
    pub horizon: Vec3,
    /// Nebeldichte pro Meter (ab 25 m Abstand)
    pub fog_density: f32,
    /// Umgebungslicht von unten
    pub ground: Vec3,
    /// Schwarzblende 0..1
    pub fade: f32,
    /// Rotblitz 0..1
    pub red: f32,
    /// Saettigung (1 = normal)
    pub saturation: f32,
}

/// Perspektive (`fov_y` in Radiant). naga uebersetzt die Shader mit der y-Drehung von WebGPU:
/// NDC y = +1 ist oben im Bild, wie bei OpenGL. Darum genuegt `Mat4::perspective_rh` ohne
/// Spiegeln; Nachbearbeitung, UI und Schattenkarte rechnen mit derselben Regel.
pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    Mat4::perspective_rh(fov_y, aspect, near, far)
}

/// Ortho-Kamera der Sonne, die einen Wuerfel um `center` (halbe Kante `radius`) abdeckt.
/// Auf das Texelraster eingerastet, damit Schattenkanten beim Bewegen nicht flimmern.
pub fn light_view_proj(sun_dir: Vec3, center: Vec3, radius: f32) -> Mat4 {
    let dir = sun_dir.normalize();
    let up = if dir.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
    let eye = center + dir * radius * 3.0;
    let view = Mat4::look_at_rh(eye, center, up);
    let texel = 2.0 * radius / SHADOW_SIZE as f32;
    let c = view.transform_point3(center);
    let snap = Vec3::new(
        (c.x / texel).round() * texel - c.x,
        (c.y / texel).round() * texel - c.y,
        0.0,
    );
    let view = Mat4::from_translation(snap) * view;
    let proj = Mat4::orthographic_rh(-radius, radius, -radius, radius, 0.1, radius * 6.0);
    proj * view
}

#[derive(Clone, Copy)]
struct Frame {
    cmd: vk::CommandBuffer,
    image_available: vk::Semaphore,
    in_flight: vk::Fence,
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
    capture: Option<std::path::PathBuf>,
    capture_buf: Option<Buffer>,
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
        self.capture = Some(path);
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
            for f in &self.frames {
                d.destroy_semaphore(f.image_available, None);
                d.destroy_fence(f.in_flight, None);
            }
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
            if let Some(b) = &self.capture_buf {
                b.destroy(d);
            }
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
