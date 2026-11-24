//! Verdecktes auf der Grafikkarte aussortieren (wahlfrei): Tiefenpyramide und indirekte
//! Zeichenbefehle in zwei Durchgaengen (`c_hiz.wgsl`, `c_cull.wgsl`).

use super::super::context::Context;
use super::super::pipeline::create_compute;
use super::super::pipelines::spv;
use super::super::util::Buffer;
use super::super::{Result, FRAMES_IN_FLIGHT};
use super::memory::{Arena, GpuBuffer, GpuImage, TexDesc};
use super::prepare::{Batch, Kind};
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use std::ops::Range;

const FORMAT: vk::Format = vk::Format::R32_SFLOAT;
/// groesste Anzahl Stufen der Pyramide
const MAX_MIPS: usize = 13;
/// Bytes je Zeichenbefehl (`VkDrawIndexedIndirectCommand`)
pub const COMMAND: u64 = 20;
const COMPUTE: vk::PipelineStageFlags = vk::PipelineStageFlags::COMPUTE_SHADER;

/// Ein Ding (wie `Item` im Shader).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Item {
    pub sphere: [f32; 4],
    pub obj: u32,
    pub batch: u32,
    pub first: u32,
    pub second: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Push {
    view_proj: [[f32; 4]; 4],
    count: u32,
    pass_no: u32,
    commands: u32,
    valid: u32,
}

/// Puffer eines Bildes im Flug.
struct Frame {
    items: Buffer,
    commands: Buffer,
    hidden: Option<GpuBuffer>,
    hidden_count: u32,
    hiz_sets: Vec<vk::DescriptorSet>,
    cull_set: vk::DescriptorSet,
}

/// Pyramide einer Aufloesung.
struct Pyramid {
    image: GpuImage,
    /// je Stufe eine Ansicht
    mips: Vec<vk::ImageView>,
    size: (u32, u32),
    source: vk::Extent2D,
}

/// Was dieses Bild gezeichnet wird.
#[derive(Clone, Copy, Default)]
struct Plan {
    items: u32,
    batches: u32,
}

pub struct Culling {
    hiz_layout: vk::DescriptorSetLayout,
    cull_layout: vk::DescriptorSetLayout,
    hiz_pipe_layout: vk::PipelineLayout,
    cull_pipe_layout: vk::PipelineLayout,
    hiz: vk::Pipeline,
    cull: vk::Pipeline,
    pool: vk::DescriptorPool,
    frames: Vec<Frame>,
    pyramid: Option<Pyramid>,
    /// Pyramide gehoert zum letzten Bild
    valid: bool,
    plan: Plan,
}

unsafe fn set_layout(
    d: &ash::Device,
    types: &[vk::DescriptorType],
) -> Result<vk::DescriptorSetLayout> {
    let bindings: Vec<_> = types
        .iter()
        .enumerate()
        .map(|(b, ty)| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(b as u32)
                .descriptor_type(*ty)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
        })
        .collect();
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    Ok(d.create_descriptor_set_layout(&info, None)?)
}

unsafe fn pipe_layout(
    d: &ash::Device,
    set: vk::DescriptorSetLayout,
    push: u32,
) -> Result<vk::PipelineLayout> {
    let sets = [set];
    let range = [vk::PushConstantRange::default()
        .stage_flags(vk::ShaderStageFlags::COMPUTE)
        .size(push)];
    let mut info = vk::PipelineLayoutCreateInfo::default().set_layouts(&sets);
    if push > 0 {
        info = info.push_constant_ranges(&range);
    }
    Ok(d.create_pipeline_layout(&info, None)?)
}

const STORAGE: vk::DescriptorType = vk::DescriptorType::STORAGE_BUFFER;
const IMAGE: vk::DescriptorType = vk::DescriptorType::SAMPLED_IMAGE;
const STORE: vk::DescriptorType = vk::DescriptorType::STORAGE_IMAGE;

impl Culling {
    pub fn new(ctx: &Context) -> Result<Self> {
        let d = &ctx.device;
        // SAFETY: gueltiges Geraet; freigegeben in `destroy`.
        unsafe {
            let hiz_layout = set_layout(d, &[IMAGE, STORE])?;
            let cull_layout = set_layout(d, &[STORAGE, STORAGE, STORAGE, STORAGE, IMAGE])?;
            let hiz_pipe_layout = pipe_layout(d, hiz_layout, 0)?;
            let cull_pipe_layout = pipe_layout(d, cull_layout, size_of::<Push>() as u32)?;
            let n = FRAMES_IN_FLIGHT as u32;
            let size = |ty, count| vk::DescriptorPoolSize {
                ty,
                descriptor_count: count * n,
            };
            let mips = MAX_MIPS as u32;
            let sizes = [size(STORAGE, 4), size(IMAGE, mips + 1), size(STORE, mips)];
            let info = vk::DescriptorPoolCreateInfo::default()
                .max_sets((mips + 1) * n)
                .pool_sizes(&sizes);
            let pool = d.create_descriptor_pool(&info, None)?;
            let mut frames = Vec::new();
            for _ in 0..FRAMES_IN_FLIGHT {
                let mut layouts = vec![hiz_layout; MAX_MIPS];
                layouts.push(cull_layout);
                let info = vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&layouts);
                let mut hiz_sets = d.allocate_descriptor_sets(&info)?;
                let cull_set = hiz_sets.pop().ok_or("Set fehlt")?;
                let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
                let indirect = storage | vk::BufferUsageFlags::INDIRECT_BUFFER;
                frames.push(Frame {
                    items: Buffer::new_host(ctx, 16 * 1024, storage)?,
                    commands: Buffer::new_host(ctx, 16 * 1024, indirect)?,
                    hidden: None,
                    hidden_count: 0,
                    hiz_sets,
                    cull_set,
                });
            }
            Ok(Self {
                hiz: create_compute(&ctx.gpu(), hiz_pipe_layout, spv!("c_hiz.comp"))?,
                cull: create_compute(&ctx.gpu(), cull_pipe_layout, spv!("c_cull.comp"))?,
                hiz_layout,
                cull_layout,
                hiz_pipe_layout,
                cull_pipe_layout,
                pool,
                frames,
                pyramid: None,
                valid: false,
                plan: Plan::default(),
            })
        }
    }

    pub fn forget(&mut self) {
        self.valid = false;
    }

    /// Zeichenbefehle und Dinge des Bildes vorbereiten. `order` waechst um die Plaetze, in die der
    /// Shader schreibt. `counts` = (Eckpunkte bzw. Index-Anzahl, erster Eckpunkt) je Befehl.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        ctx: &Context,
        fi: usize,
        arena: &mut Arena,
        (batches, camera): (&[Batch], Range<usize>),
        order: &mut Vec<u32>,
        spheres: &[[f32; 4]],
        counts: impl Fn(&Batch) -> Option<(u32, u32)>,
    ) -> Result<()> {
        let list = &batches[camera];
        let n: u32 = list.iter().map(|b| b.count).sum();
        let (base_a, base_b) = (order.len() as u32, order.len() as u32 + n);
        let mut items = Vec::with_capacity(n as usize);
        let mut commands = vec![[0u32; 5]; list.len() * 2];
        let mut at = 0;
        for (k, b) in list.iter().enumerate() {
            let (count, first) = counts(b).unwrap_or((0, 0));
            for (pass, base) in [(0, base_a), (1, base_b)] {
                commands[pass * list.len() + k] = match b.kind {
                    Kind::Object | Kind::Skinned => [count, 0, 0, 0, base + at],
                    Kind::Mesh | Kind::Skin => [count, 0, first, base + at, 0],
                };
            }
            for i in 0..b.count {
                let obj = order[(b.first + i) as usize];
                items.push(Item {
                    sphere: spheres
                        .get(obj as usize)
                        .copied()
                        .unwrap_or([0.0, 0.0, 0.0, -1.0]),
                    obj,
                    batch: k as u32,
                    first: base_a + at,
                    second: base_b + at,
                });
            }
            at += b.count;
        }
        order.resize(order.len() + 2 * n as usize, 0);
        let f = &mut self.frames[fi];
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let bytes: &[u8] = bytemuck::cast_slice(&items);
        f.items.ensure(ctx, bytes.len() as u64, storage)?;
        f.items.write(bytes);
        let bytes: &[u8] = bytemuck::cast_slice(&commands);
        let indirect = storage | vk::BufferUsageFlags::INDIRECT_BUFFER;
        f.commands.ensure(ctx, bytes.len() as u64, indirect)?;
        f.commands.write(bytes);
        if f.hidden_count < n || f.hidden.is_none() {
            let count = n.next_power_of_two().max(256);
            let new = arena.buffer(ctx, count as u64 * 4, storage)?;
            if let Some(old) = f.hidden.replace(new) {
                // SAFETY: Die GPU ist mit diesem Bild im Flug fertig.
                unsafe { arena.drop_buffer(&ctx.device, &old) };
            }
            f.hidden_count = count;
        }
        self.plan = Plan {
            items: n,
            batches: list.len() as u32,
        };
        Ok(())
    }

    /// Pyramide zur Aufloesung anlegen.
    pub fn ensure(&mut self, ctx: &Context, arena: &mut Arena, e: vk::Extent2D) -> Result<()> {
        if self.pyramid.as_ref().is_some_and(|p| p.source == e) {
            return Ok(());
        }
        // SAFETY: danach benutzt die GPU die alte Pyramide nicht mehr.
        unsafe {
            ctx.device.device_wait_idle()?;
            self.drop_pyramid(ctx, arena);
        }
        let half = |v: u32| (v.max(2).next_power_of_two() / 2).max(1);
        let size = (half(e.width), half(e.height));
        let mips = (size.0.max(size.1).ilog2() + 1).min(MAX_MIPS as u32);
        let desc = TexDesc {
            usage: vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::SAMPLED,
            mips,
            ..TexDesc::target(size.0, size.1, FORMAT)
        };
        let image = arena.image(ctx, &desc)?;
        let mut views = Vec::new();
        for level in 0..mips {
            let range = vk::ImageSubresourceRange {
                base_mip_level: level,
                level_count: 1,
                ..super::super::util::full_range(vk::ImageAspectFlags::COLOR)
            };
            let info = vk::ImageViewCreateInfo::default()
                .image(image.image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(FORMAT)
                .subresource_range(range);
            // SAFETY: frisches Bild; freigegeben in `drop_pyramid`.
            views.push(unsafe { ctx.device.create_image_view(&info, None)? });
        }
        self.pyramid = Some(Pyramid {
            image,
            mips: views,
            size,
            source: e,
        });
        self.valid = false;
        Ok(())
    }

    /// Sets des Bildes schreiben; `depth` = Tiefenbild in `DEPTH_STENCIL_READ_ONLY_OPTIMAL`.
    pub fn write_sets(&self, ctx: &Context, fi: usize, order: vk::Buffer, depth: vk::ImageView) {
        let f = &self.frames[fi];
        let (Some(p), Some(hidden)) = (&self.pyramid, &f.hidden) else {
            return;
        };
        let d = &ctx.device;
        let image = |view, layout| {
            [vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(layout)]
        };
        let general = vk::ImageLayout::GENERAL;
        for (k, view) in p.mips.iter().enumerate() {
            let src = match k {
                0 => image(depth, vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL),
                _ => image(p.mips[k - 1], general),
            };
            let dst = image(*view, general);
            let w = |b, ty| {
                vk::WriteDescriptorSet::default()
                    .dst_set(f.hiz_sets[k])
                    .dst_binding(b)
                    .descriptor_type(ty)
            };
            let writes = [w(0, IMAGE).image_info(&src), w(1, STORE).image_info(&dst)];
            // SAFETY: Die GPU ist mit diesem Bild im Flug fertig.
            unsafe { d.update_descriptor_sets(&writes, &[]) };
        }
        let list = [f.items.buffer, f.commands.buffer, order, hidden.buffer];
        let infos = list.map(|b| {
            [vk::DescriptorBufferInfo::default()
                .buffer(b)
                .range(vk::WHOLE_SIZE)]
        });
        let all = image(p.image.view, general);
        let w = |b: usize| {
            vk::WriteDescriptorSet::default()
                .dst_set(f.cull_set)
                .dst_binding(b as u32)
        };
        let mut writes: Vec<_> = infos
            .iter()
            .enumerate()
            .map(|(b, i)| w(b).descriptor_type(STORAGE).buffer_info(i))
            .collect();
        writes.push(w(4).descriptor_type(IMAGE).image_info(&all));
        // SAFETY: wie oben.
        unsafe { d.update_descriptor_sets(&writes, &[]) };
    }

    /// Aussortieren: Durchgang 1 mit der alten, 2 mit der neuen Kamera.
    ///
    /// # Safety
    /// `cmd` wird ausserhalb eines Durchgangs aufgenommen.
    pub unsafe fn record_cull(
        &self,
        ctx: &Context,
        (cmd, fi): (vk::CommandBuffer, usize),
        pass_no: u32,
        view_proj: &Mat4,
    ) {
        let d = &ctx.device;
        let Some(p) = &self.pyramid else {
            return;
        };
        if pass_no == 1 && !self.valid {
            // noch nie beschrieben
            let b = super::super::util::image_barrier(
                p.image.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::GENERAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE,
            );
            super::scene::barrier(d, cmd, (COMPUTE, COMPUTE), &[b]);
        }
        self.memory(
            ctx,
            cmd,
            vk::PipelineStageFlags::DRAW_INDIRECT | COMPUTE,
            COMPUTE,
        );
        let point = vk::PipelineBindPoint::COMPUTE;
        let f = &self.frames[fi];
        d.cmd_bind_pipeline(cmd, point, self.cull);
        d.cmd_bind_descriptor_sets(cmd, point, self.cull_pipe_layout, 0, &[f.cull_set], &[]);
        let push = Push {
            view_proj: view_proj.to_cols_array_2d(),
            count: self.plan.items,
            pass_no,
            commands: if pass_no == 1 { 0 } else { self.plan.batches },
            valid: (pass_no == 2 || self.valid) as u32,
        };
        let stage = vk::ShaderStageFlags::COMPUTE;
        let bytes = bytemuck::bytes_of(&push);
        d.cmd_push_constants(cmd, self.cull_pipe_layout, stage, 0, bytes);
        d.cmd_dispatch(cmd, self.plan.items.div_ceil(64), 1, 1);
        let readers =
            vk::PipelineStageFlags::DRAW_INDIRECT | vk::PipelineStageFlags::VERTEX_SHADER | COMPUTE;
        self.memory(ctx, cmd, COMPUTE, readers);
    }

    unsafe fn memory(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        from: vk::PipelineStageFlags,
        to: vk::PipelineStageFlags,
    ) {
        let b = vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::SHADER_WRITE)
            .dst_access_mask(
                vk::AccessFlags::SHADER_READ
                    | vk::AccessFlags::SHADER_WRITE
                    | if to.contains(vk::PipelineStageFlags::DRAW_INDIRECT) {
                        vk::AccessFlags::INDIRECT_COMMAND_READ
                    } else {
                        vk::AccessFlags::empty()
                    },
            );
        let none = vk::DependencyFlags::empty();
        ctx.device
            .cmd_pipeline_barrier(cmd, from, to, none, &[b], &[], &[]);
    }

    /// Pyramide aus der (lesbaren) Tiefe bauen.
    ///
    /// # Safety
    /// Wie [`Culling::record_cull`].
    pub unsafe fn record_pyramid(&mut self, ctx: &Context, (cmd, fi): (vk::CommandBuffer, usize)) {
        let d = &ctx.device;
        let Some(p) = &self.pyramid else {
            return;
        };
        let point = vk::PipelineBindPoint::COMPUTE;
        d.cmd_bind_pipeline(cmd, point, self.hiz);
        for k in 0..p.mips.len() {
            let set = [self.frames[fi].hiz_sets[k]];
            d.cmd_bind_descriptor_sets(cmd, point, self.hiz_pipe_layout, 0, &set, &[]);
            let size = |v: u32| (v >> k).max(1).div_ceil(8);
            d.cmd_dispatch(cmd, size(p.size.0), size(p.size.1), 1);
            self.memory(ctx, cmd, COMPUTE, COMPUTE);
        }
        self.valid = true;
    }

    /// Puffer der Zeichenbefehle und Stelle eines Befehls; `list` = Durchgang (0, 1), `k` = Befehl.
    pub fn command(&self, fi: usize, list: usize, k: usize) -> (vk::Buffer, u64) {
        let at = list as u64 * self.plan.batches as u64 + k as u64;
        (self.frames[fi].commands.buffer, at * COMMAND)
    }

    unsafe fn drop_pyramid(&mut self, ctx: &Context, arena: &mut Arena) {
        if let Some(p) = self.pyramid.take() {
            for v in p.mips {
                ctx.device.destroy_image_view(v, None);
            }
            arena.drop_image(&ctx.device, &p.image);
        }
    }

    /// # Safety
    /// Die GPU ist untaetig.
    pub unsafe fn destroy(&mut self, ctx: &Context, arena: &mut Arena) {
        let d = &ctx.device;
        self.drop_pyramid(ctx, arena);
        for f in self.frames.drain(..) {
            f.items.destroy(d);
            f.commands.destroy(d);
            if let Some(b) = f.hidden {
                arena.drop_buffer(d, &b);
            }
        }
        d.destroy_descriptor_pool(self.pool, None);
        for p in [self.hiz, self.cull] {
            d.destroy_pipeline(p, None);
        }
        for l in [self.hiz_pipe_layout, self.cull_pipe_layout] {
            d.destroy_pipeline_layout(l, None);
        }
        for l in [self.hiz_layout, self.cull_layout] {
            d.destroy_descriptor_set_layout(l, None);
        }
    }
}
