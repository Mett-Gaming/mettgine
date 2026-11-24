//! Animierte Figuren fuer Strahlen: Eckpunkte auf der Grafikkarte ausrechnen (`rt_skin.wgsl`) und
//! je Figur eine Beschleunigungsstruktur bauen, jedes Bild neu.

use super::super::super::context::Context;
use super::super::super::pipelines::spv;
use super::super::super::{Result, FRAMES_IN_FLIGHT};
use super::super::memory::{Arena, GpuBuffer};
use super::super::prepare::{Kind, Lists, RtItem};
use super::super::store::Store;
use super::super::ModelId;
use super::accel::{self, Accel};
use super::{GpuTri, Trash};
use ash::vk;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;

/// So viele Figuren sehen die Strahlen je Bild
pub const MAX_FIGURES: usize = 128;
/// Bindings: Eckpunkte, Knochen-Nummern, Index, Knochen, Lagen, Dreiecke
const BINDINGS: u32 = 6;
const GROUP: u32 = 64;

/// Quelle der einfachen Figuren.
#[derive(Clone, Copy)]
pub struct Source<'a> {
    pub bones: vk::Buffer,
    pub skins: vk::Buffer,
    /// (erster Eckpunkt, Anzahl) je Figur
    pub ranges: &'a [(u32, u32)],
}

/// Wie `Push` in `rt_skin.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Push {
    tris: u32,
    out_tri: u32,
    first: u32,
    bones: u32,
    kind: u32,
    pad: [u32; 3],
}

/// Eine Struktur, die sich Figuren von Bild zu Bild teilen.
struct Slot {
    accel: Accel,
    capacity: u32,
    scratch: GpuBuffer,
}

/// Was zu einem Bild im Flug gehoert.
#[derive(Default)]
struct Frame {
    slots: Vec<Slot>,
    positions: Option<GpuBuffer>,
    tris: Option<GpuBuffer>,
    capacity: u32,
    sets: Vec<vk::DescriptorSet>,
}

/// Eine Figur dieses Bildes.
#[derive(Clone, Copy)]
pub struct Figure {
    pub address: u64,
    pub first_tri: u32,
}

pub struct Figures {
    layout: vk::DescriptorSetLayout,
    pipe_layout: vk::PipelineLayout,
    pipe: vk::Pipeline,
    pool: vk::DescriptorPool,
    frames: Vec<Frame>,
    /// nach Objekt-Nummer
    pub found: HashMap<u32, Figure>,
}

/// Eine auszurechnende Figur.
struct Job {
    obj: u32,
    vertices: vk::Buffer,
    joints: vk::Buffer,
    indices: vk::Buffer,
    push: Push,
}

impl Figures {
    /// # Safety
    /// Raytracing ist auf `ctx` eingeschaltet.
    pub unsafe fn new(ctx: &Context) -> Result<Self> {
        let d = &ctx.device;
        let bindings: Vec<_> = (0..BINDINGS)
            .map(|b| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(b)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE)
            })
            .collect();
        let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
        let layout = d.create_descriptor_set_layout(&info, None)?;
        let layouts = [layout];
        let push = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .size(size_of::<Push>() as u32)];
        let info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&layouts)
            .push_constant_ranges(&push);
        let pipe_layout = d.create_pipeline_layout(&info, None)?;
        let pipe = super::super::super::pipeline::create_compute(
            &ctx.gpu(),
            pipe_layout,
            spv!("rt_skin.comp"),
        )?;
        let total = (MAX_FIGURES * FRAMES_IN_FLIGHT) as u32;
        let sizes = [vk::DescriptorPoolSize {
            ty: vk::DescriptorType::STORAGE_BUFFER,
            descriptor_count: total * BINDINGS,
        }];
        let info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(total)
            .pool_sizes(&sizes);
        let pool = d.create_descriptor_pool(&info, None)?;
        let all = vec![layout; MAX_FIGURES];
        let mut frames = Vec::new();
        for _ in 0..FRAMES_IN_FLIGHT {
            let info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(pool)
                .set_layouts(&all);
            frames.push(Frame {
                sets: d.allocate_descriptor_sets(&info)?,
                ..Default::default()
            });
        }
        Ok(Self {
            layout,
            pipe_layout,
            pipe,
            pool,
            frames,
            found: HashMap::new(),
        })
    }

    /// Puffer mit den Dreiecken des Bildes (None = noch keine Figur gesehen).
    pub fn tris(&self, fi: usize) -> Option<vk::Buffer> {
        self.frames[fi].tris.as_ref().map(|b| b.buffer)
    }

    /// Figuren des Bildes ausrechnen und ihre Strukturen bauen.
    ///
    /// # Safety
    /// `cmd` wird ausserhalb eines Durchgangs aufgenommen, die GPU ist mit Bild `fi` fertig.
    pub unsafe fn record(
        &mut self,
        ctx: &Context,
        (cmd, fi): (vk::CommandBuffer, usize),
        store: &mut Store,
        lists: &Lists,
        source: Source,
        trash: &mut Vec<Trash>,
    ) -> Result<()> {
        self.found.clear();
        let (jobs, total) = collect(store, lists, &lists.rt_items(), source);
        if jobs.is_empty() {
            return Ok(());
        }
        self.reserve(ctx, fi, &mut store.arena, total, trash)?;
        accel::before_figures(ctx, cmd);
        self.dispatch(ctx, cmd, fi, source.bones, &jobs);
        accel::after_figures(ctx, cmd);
        self.build(ctx, cmd, fi, &mut store.arena, &jobs, trash)?;
        accel::after_build(ctx, cmd);
        Ok(())
    }

    fn reserve(
        &mut self,
        ctx: &Context,
        fi: usize,
        arena: &mut Arena,
        total: u32,
        trash: &mut Vec<Trash>,
    ) -> Result<()> {
        let f = &mut self.frames[fi];
        if f.capacity >= total {
            return Ok(());
        }
        let capacity = total.next_power_of_two();
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let input = vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;
        let positions = arena.buffer(ctx, capacity as u64 * 48, storage | input)?;
        let size = capacity as u64 * size_of::<GpuTri>() as u64;
        let tris = arena.buffer(ctx, size, storage)?;
        let old = [f.positions.replace(positions), f.tris.replace(tris)];
        trash.extend(old.into_iter().flatten().map(Trash::Buffer));
        f.capacity = capacity;
        Ok(())
    }

    unsafe fn dispatch(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        fi: usize,
        bones: vk::Buffer,
        jobs: &[Job],
    ) {
        let d = &ctx.device;
        let f = &self.frames[fi];
        let (Some(positions), Some(tris)) = (&f.positions, &f.tris) else {
            return;
        };
        let point = vk::PipelineBindPoint::COMPUTE;
        d.cmd_bind_pipeline(cmd, point, self.pipe);
        for (job, &set) in jobs.iter().zip(&f.sets) {
            let buffers = [
                job.vertices,
                job.joints,
                job.indices,
                bones,
                positions.buffer,
                tris.buffer,
            ];
            let infos = buffers.map(|b| {
                [vk::DescriptorBufferInfo::default()
                    .buffer(b)
                    .range(vk::WHOLE_SIZE)]
            });
            let writes: Vec<_> = infos
                .iter()
                .enumerate()
                .map(|(b, info)| {
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(b as u32)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .buffer_info(info)
                })
                .collect();
            d.update_descriptor_sets(&writes, &[]);
            d.cmd_bind_descriptor_sets(cmd, point, self.pipe_layout, 0, &[set], &[]);
            let stage = vk::ShaderStageFlags::COMPUTE;
            let bytes = bytemuck::bytes_of(&job.push);
            d.cmd_push_constants(cmd, self.pipe_layout, stage, 0, bytes);
            d.cmd_dispatch(cmd, job.push.tris.div_ceil(GROUP), 1, 1);
        }
    }

    unsafe fn build(
        &mut self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        fi: usize,
        arena: &mut Arena,
        jobs: &[Job],
        trash: &mut Vec<Trash>,
    ) -> Result<()> {
        let f = &mut self.frames[fi];
        let Some(positions) = f.positions else {
            return Ok(());
        };
        for (k, job) in jobs.iter().enumerate() {
            let tris = job.push.tris;
            if f.slots.get(k).is_none_or(|s| s.capacity < tris) {
                let capacity = tris.next_power_of_two();
                let accel = Accel::for_figure(ctx, arena, capacity)?;
                let scratch = arena.buffer(ctx, accel.scratch_size(ctx), accel::SCRATCH)?;
                let slot = Slot {
                    accel,
                    capacity,
                    scratch,
                };
                if k < f.slots.len() {
                    let old = std::mem::replace(&mut f.slots[k], slot);
                    trash.extend([Trash::Accel(old.accel), Trash::Buffer(old.scratch)]);
                } else {
                    f.slots.push(slot);
                }
            }
            let s = &f.slots[k];
            let first = job.push.out_tri;
            let at = first as u64 * 48;
            s.accel
                .build_figure(ctx, cmd, (positions.buffer, at), s.scratch.buffer, tris);
            self.found.insert(
                job.obj,
                Figure {
                    address: s.accel.address,
                    first_tri: first,
                },
            );
        }
        Ok(())
    }

    /// # Safety
    /// Die GPU ist untaetig.
    pub unsafe fn destroy(&mut self, ctx: &Context, arena: &mut Arena) {
        let d = &ctx.device;
        for f in self.frames.drain(..) {
            for s in f.slots {
                s.accel.destroy(ctx, arena);
                arena.drop_buffer(d, &s.scratch);
            }
            for b in [f.positions, f.tris].into_iter().flatten() {
                arena.drop_buffer(d, &b);
            }
        }
        d.destroy_descriptor_pool(self.pool, None);
        d.destroy_pipeline(self.pipe, None);
        d.destroy_pipeline_layout(self.pipe_layout, None);
        d.destroy_descriptor_set_layout(self.layout, None);
    }
}

/// Figuren des Bildes und die Summe ihrer Dreiecke.
fn collect(store: &Store, lists: &Lists, items: &[RtItem], source: Source) -> (Vec<Job>, u32) {
    let mut total = 0;
    let mut jobs = Vec::new();
    for it in items.iter().filter(|i| i.kind != Kind::Object) {
        if jobs.len() == MAX_FIGURES {
            break;
        }
        let bones = lists.objs[it.obj as usize].extra[0] as u32;
        let (buffers, first, count, kind) = match it.kind {
            Kind::Skin => {
                let Some(&(first, count)) = source.ranges.get(it.model as usize) else {
                    continue;
                };
                ([source.skins; 3], first, count, 1)
            }
            _ => {
                let Some(m) = store.model(ModelId(it.model)) else {
                    continue;
                };
                let Some(j) = &m.joints else {
                    continue;
                };
                let buffers = [m.vertices.buffer, j.buffer, m.indices.buffer];
                (buffers, 0, m.index_count, 0)
            }
        };
        let tris = count / 3;
        if tris == 0 {
            continue;
        }
        let [vertices, joints, indices] = buffers;
        jobs.push(Job {
            obj: it.obj,
            vertices,
            joints,
            indices,
            push: Push {
                tris,
                out_tri: total,
                first,
                bones,
                kind,
                pad: [0; 3],
            },
        });
        total += tris;
    }
    (jobs, total)
}
