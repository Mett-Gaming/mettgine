//! Aufnahme: Schatten, Tiefe, Beleuchtung, Halbtransparentes.

use super::super::frame::{area, depth_clear};
use super::super::transfer::set_viewport;
use super::super::util::image_barrier;
use super::prepare::{Batch, Kind};
use super::{Inputs, Modern, SceneInput};
use ash::vk;
use glam::Mat4;

const GFX: vk::PipelineBindPoint = vk::PipelineBindPoint::GRAPHICS;
const DEPTH_STAGES: vk::PipelineStageFlags = vk::PipelineStageFlags::from_raw(
    vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS.as_raw()
        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS.as_raw(),
);

/// Welche Pipelines ein Durchgang benutzt.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Shadow,
    Pre,
    Opaque,
    Blend,
}

/// Barriere fuer ein Bild.
pub(super) fn barrier(
    d: &ash::Device,
    cmd: vk::CommandBuffer,
    (from, to): (vk::PipelineStageFlags, vk::PipelineStageFlags),
    images: &[vk::ImageMemoryBarrier],
) {
    // SAFETY: `cmd` wird aufgenommen, die Bilder gehoeren diesem Geraet.
    unsafe {
        d.cmd_pipeline_barrier(cmd, from, to, Default::default(), &[], &[], images);
    }
}

/// Tiefenbild (alle Ebenen) neu beschreiben.
fn depth_target(image: vk::Image) -> vk::ImageMemoryBarrier<'static> {
    all_layers(image_barrier(
        image,
        vk::ImageAspectFlags::DEPTH,
        vk::ImageLayout::UNDEFINED,
        vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
        vk::AccessFlags::SHADER_READ,
        vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
    ))
}

/// Fertiges Tiefenbild lesen.
fn depth_read(image: vk::Image, to: vk::ImageLayout) -> vk::ImageMemoryBarrier<'static> {
    all_layers(image_barrier(
        image,
        vk::ImageAspectFlags::DEPTH,
        vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
        to,
        vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
        vk::AccessFlags::SHADER_READ | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ,
    ))
}

fn all_layers(mut b: vk::ImageMemoryBarrier<'static>) -> vk::ImageMemoryBarrier<'static> {
    b.subresource_range.layer_count = vk::REMAINING_ARRAY_LAYERS;
    b
}

/// Farbbild neu beschreiben.
pub(super) fn color_target(image: vk::Image) -> vk::ImageMemoryBarrier<'static> {
    image_barrier(
        image,
        vk::ImageAspectFlags::COLOR,
        vk::ImageLayout::UNDEFINED,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        vk::AccessFlags::SHADER_READ,
        vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
    )
}

/// Fertiges Farbbild lesen.
pub(super) fn color_read(image: vk::Image) -> vk::ImageMemoryBarrier<'static> {
    image_barrier(
        image,
        vk::ImageAspectFlags::COLOR,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        vk::AccessFlags::SHADER_READ,
    )
}

fn color_att(
    view: vk::ImageView,
    load: vk::AttachmentLoadOp,
) -> vk::RenderingAttachmentInfo<'static> {
    vk::RenderingAttachmentInfo::default()
        .image_view(view)
        .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        .load_op(load)
        .store_op(vk::AttachmentStoreOp::STORE)
}

fn depth_att(
    view: vk::ImageView,
    layout: vk::ImageLayout,
    load: vk::AttachmentLoadOp,
) -> vk::RenderingAttachmentInfo<'static> {
    vk::RenderingAttachmentInfo::default()
        .image_view(view)
        .image_layout(layout)
        .load_op(load)
        .store_op(vk::AttachmentStoreOp::STORE)
        .clear_value(depth_clear())
}

impl Modern {
    /// Zeichenbefehle aufnehmen, Zustand nur bei Wechsel setzen.
    unsafe fn draw(&self, i: &Inputs, stage: Stage, batches: &[Batch]) {
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let mut pipe = vk::Pipeline::null();
        let mut material = None;
        let mut model: Option<(Kind, u32)> = None;
        let mut cull = None;
        for b in batches {
            let k = self.pipes.kind(b.kind);
            let want = match stage {
                Stage::Shadow if b.masked => k.shadow_masked,
                Stage::Shadow => k.shadow,
                Stage::Pre => k.pre,
                Stage::Opaque => k.opaque,
                Stage::Blend => k.blend,
            };
            if want != pipe {
                d.cmd_bind_pipeline(cmd, GFX, want);
                pipe = want;
            }
            let two_sided = b.double_sided;
            if cull != Some(two_sided) {
                d.cmd_set_cull_mode(
                    cmd,
                    if two_sided {
                        vk::CullModeFlags::NONE
                    } else {
                        vk::CullModeFlags::BACK
                    },
                );
                cull = Some(two_sided);
            }
            if material != Some(b.material) {
                if let Some(m) = self.store.material(super::MaterialId(b.material)) {
                    let sets = [m.set];
                    d.cmd_bind_descriptor_sets(cmd, GFX, self.pipes.layout, 1, &sets, &[]);
                }
                material = Some(b.material);
            }
            let bind = model != Some((b.kind, b.model));
            model = Some((b.kind, b.model));
            self.draw_batch(i, b, bind);
        }
    }

    /// Ein Zeichenbefehl; `bind` = das Modell hat gewechselt.
    unsafe fn draw_batch(&self, i: &Inputs, b: &Batch, bind: bool) {
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let (vertices, ranges) = match b.kind {
            Kind::Mesh => (i.mesh_vertices, i.mesh_ranges),
            Kind::Skin => (i.skin_vertices, i.skin_ranges),
            Kind::Object | Kind::Skinned => {
                let Some(m) = self.store.model(super::ModelId(b.model)) else {
                    return;
                };
                if bind {
                    d.cmd_bind_vertex_buffers(cmd, 0, &[m.vertices.buffer], &[0]);
                    if let Some(j) = &m.joints {
                        d.cmd_bind_vertex_buffers(cmd, 1, &[j.buffer], &[0]);
                    }
                    d.cmd_bind_index_buffer(cmd, m.indices.buffer, 0, vk::IndexType::UINT32);
                }
                d.cmd_draw_indexed(cmd, m.index_count, b.count, 0, 0, b.first);
                return;
            }
        };
        if bind {
            d.cmd_bind_vertex_buffers(cmd, 0, &[vertices], &[0]);
        }
        if let Some(&(first, count)) = ranges.get(b.model as usize) {
            d.cmd_draw(cmd, count, b.count, first, b.first);
        }
    }

    unsafe fn bind_frame(&self, i: &Inputs, set: usize) {
        let sets = [self.sets.frames[i.fi][set]];
        let layout = self.pipes.layout;
        i.ctx
            .device
            .cmd_bind_descriptor_sets(i.cmd, GFX, layout, 0, &sets, &[]);
    }

    unsafe fn shadow_view(&self, i: &Inputs, vp: &Mat4, rect: vk::Rect2D, batches: &[Batch]) {
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let viewport = vk::Viewport {
            x: rect.offset.x as f32,
            y: rect.offset.y as f32,
            width: rect.extent.width as f32,
            height: rect.extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        d.cmd_set_viewport(cmd, 0, &[viewport]);
        d.cmd_set_scissor(cmd, 0, &[rect]);
        let stages = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
        let m = vp.to_cols_array();
        d.cmd_push_constants(cmd, self.pipes.layout, stages, 0, bytemuck::cast_slice(&m));
        self.draw(i, Stage::Shadow, batches);
    }

    /// Schattenkarten von Sonne und Lampen.
    pub(super) unsafe fn record_shadows(&self, i: &Inputs) {
        let Some(t) = &self.targets else {
            return;
        };
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let maps: Vec<_> = t.sun.iter().chain(&t.lamps).collect();
        if maps.is_empty() {
            return;
        }
        let frag = vk::PipelineStageFlags::FRAGMENT_SHADER;
        let images: Vec<_> = maps.iter().map(|m| depth_target(m.image)).collect();
        barrier(d, cmd, (frag, DEPTH_STAGES), &images);
        self.bind_frame(i, 0);
        let begin = |view, size: u32| {
            let extent = vk::Extent2D {
                width: size,
                height: size,
            };
            let att = depth_att(
                view,
                vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                vk::AttachmentLoadOp::CLEAR,
            );
            d.cmd_begin_rendering(
                cmd,
                &vk::RenderingInfo::default()
                    .render_area(area(extent))
                    .layer_count(1)
                    .depth_attachment(&att),
            );
            area(extent)
        };
        if let Some(sun) = &t.sun {
            for (layer, view) in sun.layers.iter().enumerate() {
                let rect = begin(*view, t.wants.shadow);
                if let Some((vp, r)) = self.views.sun.get(layer) {
                    self.shadow_view(i, vp, rect, self.batches(r));
                }
                d.cmd_end_rendering(cmd);
            }
        }
        if let Some(lamps) = &t.lamps {
            let _ = begin(lamps.view, super::atlas::ATLAS_SIZE);
            for (tile, r) in &self.views.lamps {
                self.shadow_view(i, &tile.view_proj, tile.area, self.batches(r));
            }
            d.cmd_end_rendering(cmd);
        }
        let read = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        let images: Vec<_> = maps.iter().map(|m| depth_read(m.image, read)).collect();
        barrier(d, cmd, (DEPTH_STAGES, frag), &images);
    }

    /// Tiefe, Normale und Bewegung aller deckenden Objekte.
    pub(super) unsafe fn record_prepass(&self, i: &Inputs) {
        let Some(t) = &self.targets else {
            return;
        };
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let frag = vk::PipelineStageFlags::FRAGMENT_SHADER;
        let out = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
        let images = [
            depth_target(t.depth.image),
            color_target(t.normal.image),
            color_target(t.motion.image),
        ];
        barrier(
            d,
            cmd,
            (frag | DEPTH_STAGES | out, DEPTH_STAGES | out),
            &images,
        );
        let clear = vk::AttachmentLoadOp::CLEAR;
        let colors = [
            color_att(t.normal.view, clear),
            color_att(t.motion.view, clear),
        ];
        let layout = vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL;
        let depth = depth_att(t.depth.view, layout, clear);
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area(i.extent))
                .layer_count(1)
                .color_attachments(&colors)
                .depth_attachment(&depth),
        );
        set_viewport(d, cmd, i.extent);
        self.bind_frame(i, 0);
        self.draw(i, Stage::Pre, self.batches(&self.views.camera));
        d.cmd_end_rendering(cmd);
        let images = [
            depth_read(
                t.depth.image,
                vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL,
            ),
            color_read(t.normal.image),
            color_read(t.motion.image),
        ];
        barrier(d, cmd, (DEPTH_STAGES | out, frag | DEPTH_STAGES), &images);
    }

    /// Beleuchtung ins HDR-Bild, danach Halbtransparentes und Partikel.
    pub(super) unsafe fn record_lit(&self, i: &Inputs, s: &SceneInput) {
        let Some(t) = &self.targets else {
            return;
        };
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let frag = vk::PipelineStageFlags::FRAGMENT_SHADER;
        let out = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
        barrier(d, cmd, (frag | out, out), &[color_target(t.hdr.image)]);
        let colors = [color_att(t.hdr.view, vk::AttachmentLoadOp::CLEAR)];
        let layout = vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL;
        let depth = depth_att(t.depth.view, layout, vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::NONE);
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area(i.extent))
                .layer_count(1)
                .color_attachments(&colors)
                .depth_attachment(&depth),
        );
        set_viewport(d, cmd, i.extent);
        self.bind_frame(i, 1);
        self.mark_blended(i, false);
        self.draw(i, Stage::Opaque, self.batches(&self.views.camera));
        d.cmd_bind_pipeline(cmd, GFX, self.pipes.sky);
        self.bind_any_material(i);
        d.cmd_draw(cmd, 3, 1, 0, 0);
        self.bind_frame(i, 2);
        self.mark_blended(i, true);
        self.draw(i, Stage::Blend, self.batches(&self.views.blend));
        let (particles, count) = i.particles;
        if count > 0 {
            d.cmd_bind_pipeline(cmd, GFX, self.pipes.particle);
            self.bind_any_material(i);
            let mut grid = [0.0f32; 16];
            if let Some((id, columns, rows)) = s.particle_atlas {
                if let Some(m) = self.store.material(id) {
                    (grid[0], grid[1]) = (columns as f32, rows as f32);
                    let sets = [m.set];
                    d.cmd_bind_descriptor_sets(cmd, GFX, self.pipes.layout, 1, &sets, &[]);
                }
            }
            let stages = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
            let bytes = bytemuck::cast_slice(&grid);
            d.cmd_push_constants(cmd, self.pipes.layout, stages, 0, bytes);
            d.cmd_bind_vertex_buffers(cmd, 0, &[particles], &[0]);
            d.cmd_draw(cmd, 6, count, 0, 0);
        }
        d.cmd_end_rendering(cmd);
        barrier(d, cmd, (out, frag), &[color_read(t.hdr.image)]);
    }

    /// Sagt den Shadern, ob Werte aus Bildschirm-Durchgaengen gelten.
    unsafe fn mark_blended(&self, i: &Inputs, blended: bool) {
        let mut push = [0.0f32; 16];
        push[0] = blended as u8 as f32;
        let stages = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
        let bytes = bytemuck::cast_slice(&push);
        let layout = self.pipes.layout;
        i.ctx
            .device
            .cmd_push_constants(i.cmd, layout, stages, 0, bytes);
    }

    /// Himmel und Partikel lesen kein Material, das Set muss trotzdem gebunden sein.
    unsafe fn bind_any_material(&self, i: &Inputs) {
        if let Some(m) = self.store.material(self.plain_material) {
            let sets = [m.set];
            let layout = self.pipes.layout;
            i.ctx
                .device
                .cmd_bind_descriptor_sets(i.cmd, GFX, layout, 1, &sets, &[]);
        }
    }
}
