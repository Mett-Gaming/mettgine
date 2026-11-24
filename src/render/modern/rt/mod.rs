//! Raytracing (Vulkan-Strahlabfragen): Beschleunigungsstrukturen und Daten fuer Treffer. Gibt es
//! nur mit `Config::ray_tracing` und einer Karte, die es kann.
//!
//! Set 1 der Strahlen-Shader: 0 = Szene, 1 = Dreiecke, 2 = Objekte, 3 = Tiefe, 4 = Normale und
//! Rauheit, 5 = Farbtexturen (Feld), 6 = Sampler. Animierte Figuren sind (noch) nicht dabei.

mod accel;

use super::super::context::Context;
use super::super::util::Buffer;
use super::super::{Result, FRAMES_IN_FLIGHT};
use super::memory::{Arena, GpuBuffer};
use super::model::ModelData;
use super::prepare::{Kind, Lists, RtItem};
use super::store::Store;
use accel::Accel;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;

/// Farbtexturen, die Strahlen sehen
pub const MAX_TEXTURES: usize = 512;

/// Ein Dreieck (wie `Tri` in `_rt_common.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuTri {
    n: [[f32; 4]; 3],
    v: [f32; 4],
    color: [f32; 4],
}

/// Ein Objekt in der Szene (wie `Info`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuInfo {
    tint: [f32; 4],
    first: u32,
    material: u32,
    glow: f32,
    pad: u32,
}

/// Ein Modell.
struct Blas {
    accel: Accel,
    first_tri: u32,
}

/// Szene eines Bildes im Flug.
#[derive(Default)]
struct Tlas {
    accel: Option<Accel>,
    capacity: u32,
    scratch: Option<GpuBuffer>,
    instances: Option<Buffer>,
    infos: Option<Buffer>,
}

/// Was spaeter freigegeben wird.
enum Trash {
    Accel(Accel),
    Buffer(GpuBuffer),
    Host(Buffer),
}

pub struct Rt {
    blas: HashMap<u32, Blas>,
    /// noch zu bauen
    pending: Vec<u32>,
    tris: Vec<GpuTri>,
    tri_buf: Buffer,
    tris_changed: bool,
    tlas: Vec<Tlas>,
    trash: Vec<(u64, Trash)>,
    frame: u64,
    pub layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    pub sets: Vec<vk::DescriptorSet>,
    sampler: vk::Sampler,
    /// Objekte in der Szene des letzten Bildes
    pub count: u32,
}

const ACCEL: vk::DescriptorType = vk::DescriptorType::ACCELERATION_STRUCTURE_KHR;
const STORAGE: vk::DescriptorType = vk::DescriptorType::STORAGE_BUFFER;
const IMAGE: vk::DescriptorType = vk::DescriptorType::SAMPLED_IMAGE;
const SAMPLER: vk::DescriptorType = vk::DescriptorType::SAMPLER;
const BINDINGS: [(vk::DescriptorType, u32); 7] = [
    (ACCEL, 1),
    (STORAGE, 1),
    (STORAGE, 1),
    (IMAGE, 1),
    (IMAGE, 1),
    (IMAGE, MAX_TEXTURES as u32),
    (SAMPLER, 1),
];

impl Rt {
    /// `None`, wenn Raytracing nicht eingeschaltet ist.
    pub fn new(ctx: &Context) -> Result<Option<Self>> {
        if ctx.rt.is_none() {
            return Ok(None);
        }
        let d = &ctx.device;
        // SAFETY: gueltiges Geraet; alles wird in `destroy` freigegeben.
        unsafe {
            let bindings: Vec<_> = BINDINGS
                .iter()
                .enumerate()
                .map(|(b, (ty, n))| {
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(b as u32)
                        .descriptor_type(*ty)
                        .descriptor_count(*n)
                        .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                })
                .collect();
            // im Texturfeld duerfen Plaetze leer bleiben
            let flags: Vec<_> = BINDINGS
                .iter()
                .map(|(_, n)| {
                    if *n > 1 {
                        vk::DescriptorBindingFlags::PARTIALLY_BOUND
                    } else {
                        vk::DescriptorBindingFlags::empty()
                    }
                })
                .collect();
            let mut extra =
                vk::DescriptorSetLayoutBindingFlagsCreateInfo::default().binding_flags(&flags);
            let layout = d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default()
                    .bindings(&bindings)
                    .push_next(&mut extra),
                None,
            )?;
            let n = FRAMES_IN_FLIGHT as u32;
            let sizes = BINDINGS.map(|(ty, count)| vk::DescriptorPoolSize {
                ty,
                descriptor_count: count * n,
            });
            let pool = d.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(n)
                    .pool_sizes(&sizes),
                None,
            )?;
            let layouts = vec![layout; FRAMES_IN_FLIGHT];
            let sets = d.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&layouts),
            )?;
            let sampler = d.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                    .max_lod(vk::LOD_CLAMP_NONE),
                None,
            )?;
            Ok(Some(Self {
                blas: HashMap::new(),
                pending: Vec::new(),
                tris: Vec::new(),
                tri_buf: Buffer::new_host(ctx, 1024, vk::BufferUsageFlags::STORAGE_BUFFER)?,
                tris_changed: false,
                tlas: (0..FRAMES_IN_FLIGHT).map(|_| Tlas::default()).collect(),
                trash: Vec::new(),
                frame: 0,
                layout,
                pool,
                sets,
                sampler,
                count: 0,
            }))
        }
    }

    /// Modell aufnehmen (ohne Knochen). Gebaut wird beim naechsten Bild.
    pub fn add(&mut self, ctx: &Context, arena: &mut Arena, id: u32, m: &ModelData) -> Result<()> {
        if !m.joints.is_empty() {
            return Ok(());
        }
        self.remove(id);
        let first_tri = self.tris.len() as u32;
        for t in m.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| m.vertices[i as usize]);
            let n = |v: super::model::Vertex| [v.normal[0], v.normal[1], v.normal[2], v.uv[0]];
            let color = std::array::from_fn(|k| (a.color[k] + b.color[k] + c.color[k]) / 3.0);
            self.tris.push(GpuTri {
                n: [n(a), n(b), n(c)],
                v: [a.uv[1], b.uv[1], c.uv[1], 0.0],
                color,
            });
        }
        self.tris_changed = true;
        let triangles = m.indices.len() as u32 / 3;
        let accel = Accel::for_triangles(ctx, arena, m.vertices.len() as u32, triangles)?;
        self.blas.insert(id, Blas { accel, first_tri });
        self.pending.push(id);
        Ok(())
    }

    /// Modell entfernen (seine Dreiecksdaten bleiben liegen, bis das Spiel endet).
    pub fn remove(&mut self, id: u32) {
        self.pending.retain(|p| *p != id);
        if let Some(b) = self.blas.remove(&id) {
            self.trash.push((self.frame, Trash::Accel(b.accel)));
        }
    }

    /// Vor den Strahlen: Neues bauen, Szene des Bildes bauen, Set schreiben.
    ///
    /// # Safety
    /// `cmd` muss aufgenommen werden, ausserhalb eines Durchgangs; die GPU ist mit Bild `fi`
    /// fertig; Modelle aus `store` sind schon kopiert.
    pub unsafe fn record(
        &mut self,
        ctx: &Context,
        (cmd, fi): (vk::CommandBuffer, usize),
        store: &mut Store,
        lists: &Lists,
        (depth, normal): (vk::ImageView, vk::ImageView),
    ) -> Result<()> {
        self.begin_frame(ctx, &mut store.arena);
        self.build_models(ctx, cmd, store)?;
        self.update_tris(ctx)?;
        let items = lists.rt_items();
        self.fill(ctx, fi, lists, &items)?;
        self.build_scene(ctx, cmd, fi, &mut store.arena)?;
        self.write_set(ctx, fi, store, depth, normal);
        Ok(())
    }

    unsafe fn begin_frame(&mut self, ctx: &Context, arena: &mut Arena) {
        self.frame += 1;
        let done = self.frame.saturating_sub(FRAMES_IN_FLIGHT as u64 + 1);
        let (old, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.trash)
            .into_iter()
            .partition(|(f, _)| *f < done);
        self.trash = keep;
        for (_, t) in old {
            release(ctx, arena, t);
        }
    }

    unsafe fn build_models(
        &mut self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        store: &mut Store,
    ) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        accel::after_copy(ctx, cmd);
        for id in std::mem::take(&mut self.pending) {
            let Some(b) = self.blas.get(&id) else {
                continue;
            };
            let Some(m) = store.model(super::ModelId(id)) else {
                continue;
            };
            let (vertices, indices) = (m.vertices.buffer, m.indices.buffer);
            let scratch = b
                .accel
                .build_triangles(ctx, cmd, &mut store.arena, vertices, indices)?;
            self.trash.push((self.frame, Trash::Buffer(scratch)));
        }
        accel::after_build(ctx, cmd);
        Ok(())
    }

    fn update_tris(&mut self, ctx: &Context) -> Result<()> {
        if !self.tris_changed || self.tris.is_empty() {
            return Ok(());
        }
        // neuer Puffer, damit Bilder im Flug den alten behalten
        let bytes: &[u8] = bytemuck::cast_slice(&self.tris);
        let usage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let new = Buffer::new_host(ctx, bytes.len() as u64, usage)?;
        new.write(bytes);
        let old = std::mem::replace(&mut self.tri_buf, new);
        self.trash.push((self.frame, Trash::Host(old)));
        self.tris_changed = false;
        Ok(())
    }

    /// Objekte des Bildes in die Puffer schreiben.
    fn fill(&mut self, ctx: &Context, fi: usize, lists: &Lists, items: &[RtItem]) -> Result<()> {
        let mut instances = Vec::with_capacity(items.len());
        let mut infos = Vec::with_capacity(items.len());
        for it in items {
            debug_assert!(it.kind == Kind::Object);
            let Some(b) = self.blas.get(&it.model) else {
                continue;
            };
            let o = &lists.objs[it.obj as usize];
            let m = &o.model;
            let mut rows = [0.0; 12];
            for r in 0..3 {
                for c in 0..4 {
                    rows[r * 4 + c] = m[c][r];
                }
            }
            instances.push(accel::instance(
                rows,
                instances.len() as u32,
                b.accel.address,
            ));
            infos.push(GpuInfo {
                tint: o.tint,
                first: b.first_tri,
                material: it.material,
                glow: o.extra[3],
                pad: 0,
            });
        }
        self.count = instances.len() as u32;
        let t = &mut self.tlas[fi];
        let input = vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;
        put(ctx, &mut t.instances, accel::bytes(&instances), input)?;
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
        put(ctx, &mut t.infos, bytemuck::cast_slice(&infos), storage)
    }

    unsafe fn build_scene(
        &mut self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        fi: usize,
        arena: &mut Arena,
    ) -> Result<()> {
        let count = self.count;
        let t = &mut self.tlas[fi];
        if t.accel.is_none() || t.capacity < count {
            let capacity = count.next_power_of_two().max(64);
            let new = Accel::for_instances(ctx, arena, capacity)?;
            let scratch = arena.buffer(ctx, new.scratch_size(ctx), accel::SCRATCH)?;
            if let Some(old) = t.accel.replace(new) {
                self.trash.push((self.frame, Trash::Accel(old)));
            }
            if let Some(old) = t.scratch.replace(scratch) {
                self.trash.push((self.frame, Trash::Buffer(old)));
            }
            t.capacity = capacity;
        }
        if let (Some(a), Some(s), Some(i)) = (&t.accel, &t.scratch, &t.instances) {
            a.build_instances(ctx, cmd, s.buffer, i.buffer, count);
        }
        accel::after_build(ctx, cmd);
        Ok(())
    }

    fn write_set(
        &self,
        ctx: &Context,
        fi: usize,
        store: &Store,
        depth: vk::ImageView,
        normal: vk::ImageView,
    ) {
        let t = &self.tlas[fi];
        let (Some(accel), Some(infos)) = (&t.accel, &t.infos) else {
            return;
        };
        let set = self.sets[fi];
        let w = |b: u32| {
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(b)
                .descriptor_type(BINDINGS[b as usize].0)
        };
        let handles = [accel.handle];
        let mut structures = vk::WriteDescriptorSetAccelerationStructureKHR::default()
            .acceleration_structures(&handles);
        let buffer = |b: vk::Buffer| {
            [vk::DescriptorBufferInfo::default()
                .buffer(b)
                .range(vk::WHOLE_SIZE)]
        };
        let (tris, infos) = (buffer(self.tri_buf.buffer), buffer(infos.buffer));
        let image = |view, layout| {
            [vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(layout)]
        };
        let depth = image(depth, vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL);
        let normal = image(normal, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        let sampler = [vk::DescriptorImageInfo::default().sampler(self.sampler)];
        let textures: Vec<_> = store
            .ray_textures()
            .map(|view| image(view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)[0])
            .collect();
        let mut writes = vec![
            w(0).descriptor_count(1).push_next(&mut structures),
            w(1).buffer_info(&tris),
            w(2).buffer_info(&infos),
            w(3).image_info(&depth),
            w(4).image_info(&normal),
            w(6).image_info(&sampler),
        ];
        if !textures.is_empty() {
            writes.push(w(5).image_info(&textures));
        }
        // SAFETY: Set des Bildes, mit dem die GPU fertig ist.
        unsafe { ctx.device.update_descriptor_sets(&writes, &[]) };
    }

    /// # Safety
    /// Die GPU ist untaetig.
    pub unsafe fn destroy(&mut self, ctx: &Context, arena: &mut Arena) {
        for (_, b) in self.blas.drain() {
            release(ctx, arena, Trash::Accel(b.accel));
        }
        for t in self.tlas.drain(..) {
            let host = [t.instances, t.infos].into_iter().flatten();
            for b in host {
                release(ctx, arena, Trash::Host(b));
            }
            if let Some(a) = t.accel {
                release(ctx, arena, Trash::Accel(a));
            }
            if let Some(b) = t.scratch {
                release(ctx, arena, Trash::Buffer(b));
            }
        }
        for (_, t) in std::mem::take(&mut self.trash) {
            release(ctx, arena, t);
        }
        let d = &ctx.device;
        self.tri_buf.destroy(d);
        d.destroy_sampler(self.sampler, None);
        d.destroy_descriptor_pool(self.pool, None);
        d.destroy_descriptor_set_layout(self.layout, None);
    }
}

unsafe fn release(ctx: &Context, arena: &mut Arena, t: Trash) {
    match t {
        Trash::Accel(a) => a.destroy(ctx, arena),
        Trash::Buffer(b) => arena.drop_buffer(&ctx.device, &b),
        Trash::Host(b) => b.destroy(&ctx.device),
    }
}

/// Puffer anlegen oder vergroessern und fuellen.
fn put(
    ctx: &Context,
    buf: &mut Option<Buffer>,
    bytes: &[u8],
    usage: vk::BufferUsageFlags,
) -> Result<()> {
    if buf.is_none() {
        *buf = Some(Buffer::new_host(ctx, 4096, usage)?);
    }
    if let Some(b) = buf {
        b.ensure(ctx, bytes.len() as u64, usage)?;
        b.write(bytes);
    }
    Ok(())
}
