//! Moderne Grafik: Modelle mit Materialien (Metall-Rauheit), HDR, Sonne mit Schattenstufen,
//! viele Lampen, Verdeckung, Ueberstrahlen, TAA. Kein eigener Renderer, sondern zusaetzliche
//! Durchgaenge von [`super::Renderer`], die statt des einfachen Szenen-Durchgangs laufen, wenn
//! `FrameInput::scene` gesetzt ist. Bis dahin entsteht nichts davon.
//!
//! Ablauf: Schatten (Sonne, Lampen) -> Tiefe, Normale, Bewegung -> Verdeckung -> Beleuchtung ->
//! Halbtransparentes -> TAA -> Belichtung, Ueberstrahlen -> Bildschirmwerte ins Szenenbild.
//! Danach laufen Nachbearbeitung und UI wie immer.
//!
//! Dateien: `types`/`model` (Eingaben), `store`/`memory`/`copy` (Grafikspeicher), `prepare` (was ist zu sehen),
//! `cascades`/`clusters`/`atlas` (Rechnungen), `gpu` (Shader-Daten), `sets`/`pipes`/`targets`
//! (Vulkan-Aufbau), `scene`/`post` (Aufnahme).

mod atlas;
mod cascades;
mod clusters;
mod copy;
mod gpu;
mod memory;
mod model;
mod pipes;
mod post;
mod prepare;
mod rays;
mod rt;
mod scene;
mod sets;
mod store;
mod targets;
mod types;

pub(crate) use post::record_images;

/// Bilder der Oberflaeche je Bild
pub const MAX_UI_IMAGES: usize = sets::IMAGE_SETS;

pub use model::*;
pub use types::*;

use super::context::Context;
use super::util::Buffer;
use super::{FrameInput, MeshDraw, Result, FRAMES_IN_FLIGHT};
use crate::cull::{SkinBounds, Sphere};
use crate::gpu::GpuInstance;
use ash::vk;
use cascades::Cascade;
use glam::{Mat4, Vec2};
use gpu::{GpuLamp, GpuScene, SceneExtras};
use prepare::{Batch, Lists, Plain};
use std::ops::Range;
use store::Store;
use targets::{Fixed, Targets, Wants};

/// Puffer eines Bildes im Flug.
struct FrameBuffers {
    objs: Buffer,
    order: Buffer,
    scene: Buffer,
    lamps: Buffer,
    cells: Buffer,
    entries: Buffer,
    materials: Buffer,
}

impl FrameBuffers {
    fn new(ctx: &Context) -> Result<Self> {
        let storage =
            |size: usize| Buffer::new_host(ctx, size as u64, vk::BufferUsageFlags::STORAGE_BUFFER);
        Ok(Self {
            objs: storage(64 * 1024)?,
            order: storage(16 * 1024)?,
            scene: Buffer::new_host(
                ctx,
                size_of::<GpuScene>() as u64,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
            )?,
            lamps: storage(16 * 1024)?,
            cells: storage(clusters::CELLS * 8)?,
            entries: storage(64 * 1024)?,
            materials: storage(16 * 1024)?,
        })
    }

    fn all(&self) -> [&Buffer; 7] {
        [
            &self.objs,
            &self.order,
            &self.scene,
            &self.lamps,
            &self.cells,
            &self.entries,
            &self.materials,
        ]
    }
}

/// Sichten eines Bildes: ihre Zeichenbefehle.
#[derive(Default)]
struct Views {
    camera: Range<usize>,
    blend: Range<usize>,
    sun: Vec<(Mat4, Range<usize>)>,
    lamps: Vec<(atlas::Tile, Range<usize>)>,
}

/// Alles fuer die moderne Grafik.
pub(crate) struct Modern {
    pub store: Store,
    sets: sets::Sets,
    pipes: pipes::Pipes,
    fixed: Fixed,
    targets: Option<Targets>,
    frames: Vec<FrameBuffers>,
    lists: Lists,
    clusters: clusters::Clusters,
    views: Views,
    tiles: Vec<atlas::Tile>,
    rt: Option<rt::Rt>,
    /// einfache Modelle, fuer Strahlen umgewandelt
    mesh_models: Vec<Option<ModelId>>,
    plain_material: MaterialId,
    /// Einheitswuerfel fuer die Welt-Boxen
    cube: ModelId,
    /// laufende Bildnummer
    number: u64,
    prev_view_proj: Option<Mat4>,
    /// Verlauf (TAA, Belichtung) ist brauchbar
    history_valid: bool,
    /// welches der beiden Verlaufs- und Belichtungsbilder zuletzt beschrieben wurde
    ping: usize,
    /// wiederverwendet
    lamp_data: Vec<GpuLamp>,
}

/// Was die Aufnahme aus dem Renderer braucht.
pub(crate) struct Inputs<'a> {
    pub ctx: &'a Context,
    pub cmd: vk::CommandBuffer,
    pub fi: usize,
    /// Aufloesung der Szene (setzt die Aufnahme aus `out` und der Einstellung)
    pub extent: vk::Extent2D,
    /// Aufloesung des Ziels
    pub out: vk::Extent2D,
    /// Ziel: das Szenenbild des Renderers
    pub color: vk::Image,
    pub color_view: vk::ImageView,
    pub bones: &'a Buffer,
    pub particles: (vk::Buffer, u32),
    pub mesh_vertices: vk::Buffer,
    /// dieselben Daten, vom Prozessor lesbar
    pub mesh_buffer: &'a Buffer,
    pub mesh_ranges: &'a [(u32, u32)],
    pub skin_vertices: vk::Buffer,
    pub skin_ranges: &'a [(u32, u32)],
    pub mesh_bounds: &'a [Sphere],
    pub skin_bounds: &'a [SkinBounds],
    pub boxes: &'a [GpuInstance],
    pub statics: &'a [MeshDraw],
}

impl Inputs<'_> {
    /// Eckpunkte aller einfachen Modelle (langsam, nur einmal beim Umwandeln).
    fn mesh_vertices_data(&self) -> Vec<crate::gpu::MeshVertex> {
        let size = size_of::<crate::gpu::MeshVertex>();
        let ends = self.mesh_ranges.iter().map(|(f, c)| (f + c) as usize);
        let count = ends
            .max()
            .unwrap_or(0)
            .min(self.mesh_buffer.size as usize / size);
        let bytes = self.mesh_buffer.read(count * size);
        let chunks = bytes.chunks(size);
        chunks.map(bytemuck::pod_read_unaligned).collect()
    }
}

impl Modern {
    pub fn new(ctx: &Context, pool: vk::CommandPool, swapchain: vk::Format) -> Result<Self> {
        let mut store = Store::new(ctx)?;
        let plain_material = store.add_material(ctx, &Material::default())?;
        let mut rt = rt::Rt::new(ctx)?;
        let cube_data = ModelData::from_mesh(&crate::shapes::cube());
        let cube = store.add_model(ctx, &cube_data)?;
        if let Some(rt) = &mut rt {
            rt.add(ctx, &mut store.arena, cube.0, &cube_data)?;
        }
        let sets = sets::Sets::new(ctx, FRAMES_IN_FLIGHT)?;
        let pipes = pipes::Pipes::new(
            ctx,
            sets.frame_layout,
            store.sets.layout,
            sets.post_layout,
            rt.as_ref().map(|r| r.layout),
            swapchain,
        )?;
        let fixed = Fixed::new(ctx, pool, &mut store.arena)?;
        Ok(Self {
            frames: (0..FRAMES_IN_FLIGHT)
                .map(|_| FrameBuffers::new(ctx))
                .collect::<Result<_>>()?,
            store,
            sets,
            pipes,
            fixed,
            targets: None,
            lists: Lists::default(),
            clusters: clusters::Clusters::default(),
            views: Views::default(),
            tiles: Vec::new(),
            rt,
            mesh_models: Vec::new(),
            plain_material,
            cube,
            number: 0,
            prev_view_proj: None,
            history_valid: false,
            ping: 0,
            lamp_data: Vec::new(),
        })
    }

    /// Gezeichnete, aussortierte Objekte und Zeichenbefehle des letzten Bildes.
    pub fn counts(&self) -> (u32, u32, u32) {
        let batches = self.lists.batches.len() as u32;
        (self.lists.drawn, self.lists.culled, batches)
    }

    fn ensure_targets(&mut self, ctx: &Context, wants: Wants) -> Result<()> {
        if self.targets.as_ref().is_some_and(|t| t.wants == wants) {
            return Ok(());
        }
        // SAFETY: danach benutzt die GPU keins der alten Bilder mehr.
        unsafe {
            ctx.device.device_wait_idle()?;
            if let Some(old) = self.targets.take() {
                old.destroy(&mut self.store.arena, &ctx.device);
            }
        }
        self.targets = Some(Targets::new(ctx, &mut self.store.arena, wants)?);
        self.history_valid = false;
        Ok(())
    }

    /// Sichten berechnen, Objekte sortieren, Puffer fuellen, Sets schreiben.
    fn prepare(&mut self, i: &Inputs, f: &FrameInput, s: &SceneInput) -> Result<()> {
        self.convert_meshes(i)?;
        self.ensure_targets(i.ctx, Wants::new(i.out, &s.features))?;
        if s.cut {
            self.history_valid = false;
        }
        let cam = &s.camera;
        let plain = Plain {
            mesh_bounds: i.mesh_bounds,
            skin_bounds: i.skin_bounds,
            material: self.plain_material.0,
            boxes: i.boxes,
            cube: self.cube.0,
            statics: i.statics,
            mesh_models: &self.mesh_models,
        };
        self.lists.collect(f, s, &self.store, &plain);
        let cascades = self.sun_views(s);
        let lights = &s.lights[..s.lights.len().min(clusters::MAX_LIGHTS)];
        self.lamp_views(s, lights);
        let vp = cam.view_proj();
        self.views.camera = self.lists.view(&vp, false);
        self.views.blend = self.lists.blended(&vp, cam.pos);
        self.clusters.build(cam, lights);
        let jitter = if s.features.anti_alias == AntiAlias::Taa {
            gpu::jitter(self.number)
        } else {
            Vec2::ZERO
        };
        let tiles: Vec<gpu::GpuTile> = self.tiles.iter().map(atlas::Tile::gpu).collect();
        let scene = GpuScene::new(
            s,
            &SceneExtras {
                extent: i.extent,
                prev_view_proj: self.prev_view_proj.filter(|_| !s.cut).unwrap_or(vp),
                jitter,
                cascades: cascades.as_ref(),
                tiles: &tiles,
                atlas: atlas::ATLAS_SIZE,
                sky_mips: self.sky_texture(s).1,
                time: f.time,
                number: self.number,
                lamps: lights.len(),
            },
        );
        self.fill(i, s, &scene)?;
        self.write_sets(i, s);
        self.prev_view_proj = Some(vp);
        Ok(())
    }

    fn sun_views(&mut self, s: &SceneInput) -> Option<[Cascade; cascades::CASCADES]> {
        self.views.sun.clear();
        let sun = s.sun.filter(|l| l.shadow_distance > 0.0)?;
        let size = self.targets.as_ref()?.wants.shadow;
        if size == 0 {
            return None;
        }
        let c = cascades::cascades(&s.camera, sun.dir, sun.shadow_distance, size);
        for c in &c {
            let r = if s.features.ray_tracing.sun_shadows {
                self.lists.view_without_rays(&c.view_proj)
            } else {
                self.lists.view(&c.view_proj, true)
            };
            self.views.sun.push((c.view_proj, r));
        }
        Some(c)
    }

    fn lamp_views(&mut self, s: &SceneInput, lights: &[Light]) {
        self.views.lamps.clear();
        self.lamp_data.clear();
        let first = if s.features.lamp_shadows {
            atlas::assign(lights, s.camera.pos, &mut self.tiles)
        } else {
            self.tiles.clear();
            vec![None; lights.len()]
        };
        for t in &self.tiles {
            let r = self.lists.lamp_view(&t.view_proj, t.pos, t.range);
            self.views.lamps.push((*t, r));
        }
        let data = lights.iter().zip(first).map(|(l, t)| GpuLamp::new(l, t));
        self.lamp_data.extend(data);
    }

    fn fill(&mut self, i: &Inputs, s: &SceneInput, scene: &GpuScene) -> Result<()> {
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let mut materials = self.store.material_values();
        if let Some(m) = materials.get_mut(self.plain_material.0 as usize) {
            m.set_roughness(s.plain_roughness);
        }
        let b = &mut self.frames[i.fi];
        let put = |buf: &mut Buffer, bytes: &[u8]| -> Result<()> {
            buf.ensure(i.ctx, bytes.len() as u64, storage)?;
            buf.write(bytes);
            Ok(())
        };
        put(&mut b.objs, bytemuck::cast_slice(&self.lists.objs))?;
        put(&mut b.order, bytemuck::cast_slice(&self.lists.order))?;
        put(&mut b.lamps, bytemuck::cast_slice(&self.lamp_data))?;
        put(&mut b.cells, bytemuck::cast_slice(&self.clusters.cells))?;
        put(&mut b.entries, bytemuck::cast_slice(&self.clusters.entries))?;
        put(&mut b.materials, bytemuck::cast_slice(&materials))?;
        b.scene.write(bytemuck::bytes_of(scene));
        Ok(())
    }

    /// Ansicht und Stufen des Himmelsbilds (ohne Bild: weiss).
    fn sky_texture(&self, s: &SceneInput) -> (vk::ImageView, u32) {
        let id = match s.sky {
            Sky::Image { texture, .. } => texture,
            Sky::Gradient { .. } => self.store.white,
        };
        let image = self
            .store
            .texture(id)
            .or(self.store.texture(self.store.white));
        image.map_or((vk::ImageView::null(), 1), |i| (i.view, i.mips))
    }

    fn write_sets(&self, i: &Inputs, s: &SceneInput) {
        let Some(t) = &self.targets else {
            return;
        };
        let b = &self.frames[i.fi];
        let buffers = [
            &b.objs,
            i.bones,
            &b.scene,
            &b.lamps,
            &b.cells,
            &b.entries,
            &b.materials,
            &b.order,
        ];
        let white = self.store.texture(self.store.white);
        let white = white.map_or(vk::ImageView::null(), |w| w.view);
        let sky = self.sky_texture(s).0;
        let dummy = self.fixed.dummy_depth.view;
        let read = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        let sun = t.sun.as_ref().unwrap_or(&self.fixed.dummy_array).view;
        let lamps = t.lamps.as_ref().unwrap_or(&self.fixed.dummy_depth).view;
        let ao = t.ao.as_ref().map_or(white, |a| a[0].view);
        let sets = &self.sets.frames[i.fi];
        let array = self.fixed.dummy_array.view;
        let w = |set, images, layout| self.sets.write_frame(i.ctx, set, buffers, images, layout);
        let rays = t.rays.as_ref().map(|r| [r.now[0].view, r.now[1].view]);
        let [gi, mirror] = rays.unwrap_or([white; 2]);
        w(
            sets[0],
            [array, dummy, white, sky, dummy, white, white],
            read,
        );
        w(sets[1], [sun, lamps, ao, sky, dummy, gi, mirror], read);
        let depth = vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL;
        w(
            sets[2],
            [sun, lamps, ao, sky, t.depth.view, gi, mirror],
            depth,
        );
    }

    /// Ein Bild aufnehmen; das Ergebnis liegt danach als Farbziel in `i.color`.
    ///
    /// # Safety
    /// `i.cmd` muss aufgenommen werden, die GPU mit Bild `i.fi` fertig sein.
    pub unsafe fn record(
        &mut self,
        i: &mut Inputs,
        f: &FrameInput,
        s: &SceneInput,
        mark: &mut dyn FnMut(&'static str),
    ) -> Result<()> {
        // Strahlen nur, wenn die Karte sie kann
        let mut limited = *s;
        if self.rt.is_none() {
            limited.features.ray_tracing = RayTracing::default();
            limited.features.path_tracing = None;
        }
        let s = &limited;
        i.extent = Wants::new(i.out, &s.features).scene();
        let i = &*i;
        let same_camera = self.prev_view_proj == Some(s.camera.view_proj());
        self.prepare(i, f, s)?;
        self.store.begin_frame(i.ctx, i.cmd);
        self.record_shadows(i);
        mark("Schatten");
        self.record_prepass(i);
        mark("Tiefe");
        self.build_rays(i, s)?;
        let mut post = post::Post::new(self, i, s);
        post.occlusion();
        post.rays(same_camera);
        mark("Verdeckung, Strahlen");
        if s.features.path_tracing.is_none() {
            self.record_lit(i, s);
        }
        mark("Beleuchtung");
        let used = post.used;
        let mut post = post::Post::new(self, i, s);
        post.used = used;
        post.finish();
        mark("Nachbearbeitung HDR");
        self.history_valid = true;
        self.ping ^= 1;
        self.number += 1;
        Ok(())
    }

    /// Anfang eines Bildes ohne moderne Szene: nur Nachgeladenes kopieren.
    ///
    /// # Safety
    /// `cmd` muss aufgenommen werden, ausserhalb eines Durchgangs.
    pub unsafe fn upload_only(&mut self, ctx: &Context, cmd: vk::CommandBuffer) {
        self.store.begin_frame(ctx, cmd);
        // die naechste moderne Szene beginnt ohne Verlauf
        self.history_valid = false;
        self.prev_view_proj = None;
    }

    fn batches(&self, r: &Range<usize>) -> &[Batch] {
        &self.lists.batches[r.clone()]
    }

    /// # Safety
    /// Die GPU ist untaetig.
    pub unsafe fn destroy(&mut self, ctx: &Context) {
        let d = &ctx.device;
        for b in &self.frames {
            for b in b.all() {
                b.destroy(d);
            }
        }
        if let Some(t) = self.targets.take() {
            t.destroy(&mut self.store.arena, d);
        }
        self.fixed.destroy(&mut self.store.arena, d);
        if let Some(rt) = &mut self.rt {
            rt.destroy(ctx, &mut self.store.arena);
        }
        self.pipes.destroy(d);
        self.sets.destroy(d);
        self.store.destroy(ctx);
        self.store.arena.destroy(d);
    }
}
