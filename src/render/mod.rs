//! Der Renderer der Mettgine. Es gibt genau diesen einen: Neue Faehigkeiten kommen hier dazu
//! und stehen damit allen Spielen offen.
//!
//! Ablauf pro Bild (Vulkan 1.3, Dynamic Rendering), nicht Gebrauchtes faellt weg:
//! 1. Schattenkarte aus Sicht der Sonne
//! 2. zweite Kamera (Monitor, Spiegel) in ein eigenes kleines Bild
//! 3. Szene in ein Offscreen-Bild: Himmel, Welt-Boxen, feste und bewegliche Modelle, Figuren,
//!    Hologramme, Partikel
//! 4. UI-Ebene (nur bei festem UI-Raster mit Videoband)
//! 5. Nachbearbeitung aufs Fenster, UI obendrauf
//!
//! Mit `FrameInput::scene` laufen statt 1 bis 3 die Durchgaenge der modernen Grafik (`modern`).
//!
//! Dateien: `params` (Eingaben), `init`/`descriptors`/`pipelines` (Aufbau), `world` (selten
//! geaenderte Daten), `lists` (was ist zu sehen), `draw` (ein Bild vorbereiten),
//! `frame`/`screen` (aufnehmen), `quality` (Zuschaltbares), `modern`/`models` (moderne Grafik). Darunter die Vulkan-Grundlagen.
//! Shader: `shaders/`, Schnittstelle in `_common.wgsl` und `descriptors`.

pub(crate) mod cache;
pub(crate) mod context;
pub(crate) mod pipeline;
pub(crate) mod present;
pub(crate) mod swapchain;
pub(crate) mod timing;
pub(crate) mod transfer;
pub(crate) mod util;

mod descriptors;
mod draw;
mod frame;
mod init;
mod lists;
mod models;
mod modern;
mod params;
mod pipelines;
mod quality;
mod runs;
mod screen;
#[cfg(test)]
mod tests;
mod world;

pub use modern::{MAX_UI_IMAGES, *};
pub use params::*;
pub use pipelines::WorldShaders;

use crate::cull::{SkinBounds, Sphere};
use crate::particles::ParticleInst;
use crate::profile::FrameStats;
use crate::splash::Splash;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use context::Context;
use descriptors::Descriptors;
use glam::Mat4;
use lists::DrawLists;
use pipelines::{Pipelines, SceneSetup};
use present::{Capture, Frame};
use runs::{Runs, VisibleRuns};
use swapchain::Swapchain;
use timing::GpuTimer;
use util::{Buffer, Image};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Ist die Grafikkarte ausgefallen (Treiberabsturz, Karte abgezogen)? Dann hilft nur ein neuer
/// [`Renderer`] oder ein Neustart; andere Zeichenfehler darf das Spiel noch einmal versuchen.
pub fn is_device_lost(e: &(dyn std::error::Error + 'static)) -> bool {
    matches!(
        e.downcast_ref::<vk::Result>(),
        Some(&vk::Result::ERROR_DEVICE_LOST | &vk::Result::ERROR_SURFACE_LOST_KHR)
    )
}

const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;
const FRAMES_IN_FLIGHT: usize = 2;
/// Modell- plus Figuren-Instanzen, fuer die am Anfang Platz ist (waechst bei Bedarf)
pub const MAX_INSTANCES: usize = 8192;
/// UI-Vertices, fuer die am Anfang Platz ist (waechst bei Bedarf)
pub const MAX_UI_VERTS: usize = 120_000;
/// Knochen-Matrizen (alle Figuren zusammen), fuer die am Anfang Platz ist (waechst bei Bedarf)
pub const MAX_BONES: usize = 8192;
/// Feste Obergrenzen pro Bild; was darueber liegt, fehlt und zaehlt in `FrameStats::dropped`
const HARD_INSTANCES: usize = 131_072;
const HARD_UI_VERTS: usize = 1_200_000;
const HARD_BONES: usize = 131_072;
const HARD_PARTICLES: usize = 131_072;
/// Partikel, fuer die am Anfang Platz ist
const START_PARTICLES: usize = 1024;
/// Kantenlaenge der Schattenkarte (Texel)
pub const SHADOW_SIZE: u32 = 2048;
/// Uebliche virtuelle UI-Hoehe; die Breite folgt dem Seitenverhaeltnis des Fensters
pub const UI_HEIGHT: f32 = 720.0;

/// Ein Eckpunkt der Oberflaeche.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct UiVertex {
    pub pos: [f32; 2],
    /// Stelle im Schrift-Atlas; x < 0 = einfarbig
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// Szenen-Daten als Uniform (binding 2), gleich aufgebaut wie `Globals` in `_common.wgsl`.
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
    params: [f32; 4],
    sway: [f32; 4],
    extra: [f32; 4],
}

/// Eine Instanz (binding 0), gleich aufgebaut wie `Inst` in `_common.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuInst {
    model: [[f32; 4]; 4],
    tint: [f32; 4],
    /// x = erster Knochen (Figuren), y = Blinken, z = Zerreissen, w = 1 schwankt mit
    extra: [f32; 4],
}

impl GpuInst {
    fn mesh(m: &MeshDraw, sways: bool) -> Self {
        Self {
            model: m.model.to_cols_array_2d(),
            tint: m.tint.to_array(),
            extra: [0.0, m.blink, 0.0, sways as u8 as f32],
        }
    }

    fn skin(s: &SkinDraw) -> Self {
        Self {
            model: s.model.to_cols_array_2d(),
            tint: s.tint.to_array(),
            extra: [s.first_bone as f32, s.blink, s.distort, 0.0],
        }
    }
}

/// Zusammenhaengende Instanzen eines Modells (ein instanzierter Draw-Call).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Group {
    mesh: u32,
    first_inst: u32,
    count: u32,
}

/// Bild der zweiten Kamera (ohne Kamera: 1x1-Platzhalter).
struct SecondCamera {
    color: Image,
    depth: Image,
    extent: vk::Extent2D,
    enabled: bool,
}

/// Eigene UI-Ebene (ohne festes Raster: 1x1-Platzhalter).
struct UiLayer {
    image: Image,
    extent: vk::Extent2D,
    enabled: bool,
}

/// Der Renderer. Aufbau mit [`Renderer::new`], ein Bild mit [`Renderer::draw`].
pub struct Renderer {
    swapchain: Swapchain,
    /// Offscreen-Ziel der Szene und seine Tiefe
    color: Image,
    depth: Image,
    /// mehrfach abgetastete Farbe, nur mit Kantenglaettung (wird nach `color` aufgeloest)
    ms_color: Option<Image>,
    samples: vk::SampleCountFlags,
    wanted_samples: vk::SampleCountFlags,
    offscreen: vk::Extent2D,
    resolution: Resolution,
    ui_size: UiSize,
    second: SecondCamera,
    ui_layer: UiLayer,
    shadow: Image,
    /// 1x1-Tiefe als Platzhalter an binding 3 im Schatten-Pass (dort wird die echte Karte beschrieben)
    dummy_shadow: Image,
    font: Image,
    signs: Image,
    smooth: bool,
    font_mipmaps: bool,
    desc: Descriptors,
    setup: SceneSetup,
    pipes: Pipelines,
    command_pool: vk::CommandPool,
    frames: Vec<Frame>,
    frame_index: usize,
    mesh_vertex_buf: Buffer,
    mesh_ranges: Vec<(u32, u32)>,
    skin_vertex_buf: Buffer,
    skin_ranges: Vec<(u32, u32)>,
    /// selten geaendert: Welt-Boxen, Lampen, feste Modelle
    box_buf: Buffer,
    box_count: u32,
    light_buf: Buffer,
    light_count: u32,
    static_buf: Buffer,
    static_runs: Runs,
    box_runs: Runs,
    visible_runs: VisibleRuns,
    /// eigener Vertex-Shader der Boxen: Ausdehnung unbekannt, nicht aussortieren
    cull_boxes: bool,
    /// dieselben Daten fuer die moderne Grafik
    boxes: Vec<crate::gpu::GpuInstance>,
    statics: Vec<MeshDraw>,
    inst_bufs: Vec<Buffer>,
    bone_bufs: Vec<Buffer>,
    globals_bufs: Vec<Buffer>,
    second_globals: Vec<Buffer>,
    ui_bufs: Vec<Buffer>,
    particle_bufs: Vec<Buffer>,
    particles: Vec<ParticleInst>,
    mesh_bounds: Vec<Sphere>,
    skin_bounds: Vec<SkinBounds>,
    culling: bool,
    lists: DrawLists,
    timer: GpuTimer,
    /// moderne Grafik, entsteht beim ersten Gebrauch
    modern: Option<Box<modern::Modern>>,
    splash: Splash,
    stats: FrameStats,
    /// wiederverwendete Puffer fuer den Bild-Takt
    scratch_insts: Vec<GpuInst>,
    scratch_bones: Vec<[[f32; 4]; 4]>,
    window_size: (u32, u32),
    vsync: bool,
    needs_recreate: bool,
    capture: Capture,
    // Muss zuletzt freigegeben werden (Drop-Reihenfolge der Felder).
    ctx: Context,
}

fn offscreen_extent(window: (u32, u32), r: Resolution) -> vk::Extent2D {
    let (width, height) = match r {
        Resolution::Fixed(w, h) => (w, h),
        Resolution::Scale(s) => (
            (window.0 as f32 * s.clamp(0.25, 2.0)) as u32,
            (window.1 as f32 * s.clamp(0.25, 2.0)) as u32,
        ),
    };
    vk::Extent2D {
        width: width.max(16),
        height: height.max(16),
    }
}

impl Renderer {
    /// Name der Grafikkarte samt Treiber.
    pub fn gpu_name(&self) -> &str {
        &self.ctx.gpu_name
    }

    /// Ist Raytracing eingeschaltet und von der Karte unterstuetzt?
    pub fn ray_tracing(&self) -> bool {
        self.ctx.rt.is_some()
    }

    /// Laeuft noch das Startbild der Engine? Dann keine Eingaben annehmen.
    pub fn splash_active(&self) -> bool {
        self.splash.active()
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
        let new = Resolution::Scale(scale.clamp(0.25, 2.0));
        if new != self.resolution {
            self.resolution = new;
            self.needs_recreate = true;
        }
    }

    /// Bilder an den Bildschirm binden (Vorgabe: an). Aus = so schnell wie moeglich.
    pub fn set_vsync(&mut self, on: bool) {
        if on != self.vsync {
            self.vsync = on;
            self.needs_recreate = true;
        }
    }

    /// Virtuelle UI-Flaeche (Breite, Hoehe), siehe [`UiSize`].
    pub fn ui_area(&self) -> (f32, f32) {
        match self.ui_size {
            UiSize::Height(h) => (h * self.aspect(), h),
            UiSize::Fixed(w, h) => (w as f32, h as f32),
        }
    }

    /// Breite der virtuellen UI-Flaeche.
    pub fn ui_width(&self) -> f32 {
        self.ui_area().0
    }

    /// Seitenverhaeltnis des Fensters (Breite / Hoehe).
    pub fn aspect(&self) -> f32 {
        let (w, h) = self.window_size;
        w.max(1) as f32 / h.max(1) as f32
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // SAFETY: Warten auf das Geraet, danach nutzt die GPU keine der Ressourcen mehr;
        // jede wird hier genau einmal freigegeben.
        unsafe {
            let _ = self.ctx.device.device_wait_idle();
            if let Some(mut m) = self.modern.take() {
                m.destroy(&self.ctx);
            }
            let d = &self.ctx.device;
            present::destroy_frames(d, &self.frames);
            d.destroy_command_pool(self.command_pool, None);
            let single = [
                &self.mesh_vertex_buf,
                &self.skin_vertex_buf,
                &self.box_buf,
                &self.light_buf,
                &self.static_buf,
            ];
            for b in self
                .inst_bufs
                .iter()
                .chain(&self.bone_bufs)
                .chain(&self.globals_bufs)
                .chain(&self.second_globals)
                .chain(&self.ui_bufs)
                .chain(&self.particle_bufs)
                .chain(single)
            {
                b.destroy(d);
            }
            self.capture.destroy(d);
            self.timer.destroy(d);
            self.splash.destroy(d);
            self.pipes.destroy(d);
            self.desc.destroy(d);
            for i in [
                &self.color,
                &self.depth,
                &self.second.color,
                &self.second.depth,
                &self.ui_layer.image,
                &self.shadow,
                &self.dummy_shadow,
                &self.font,
                &self.signs,
            ]
            .into_iter()
            .chain(&self.ms_color)
            {
                i.destroy(d);
            }
            self.swapchain.destroy(&self.ctx);
        }
    }
}
