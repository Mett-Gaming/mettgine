//! Aufnahme: Schatten, Tiefe, Beleuchtung, Halbtransparentes.

use super::super::frame::{area, depth_clear};
use super::super::transfer::set_viewport;
use super::super::util::image_barrier;
use super::drawing::Stage;
use super::prepare::Batch;
use super::{Inputs, Modern, SceneInput};
use ash::vk;
use glam::Mat4;

const COMPUTE: vk::PipelineStageFlags = vk::PipelineStageFlags::COMPUTE_SHADER;
const GFX: vk::PipelineBindPoint = vk::PipelineBindPoint::GRAPHICS;
const DEPTH_STAGES: vk::PipelineStageFlags = vk::PipelineStageFlags::from_raw(
    vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS.as_raw()
        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS.as_raw(),
);

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

/// Szenenbild (gerade Farbziel) in ein zweites Bild kopieren, das danach gelesen wird.
unsafe fn copy_scene(
    d: &ash::Device,
    cmd: vk::CommandBuffer,
    from: vk::Image,
    to: vk::Image,
    extent: vk::Extent2D,
) {
    use vk::{AccessFlags as A, ImageLayout as L, PipelineStageFlags as S};
    let color = vk::ImageAspectFlags::COLOR;
    let (out, copy) = (S::COLOR_ATTACHMENT_OUTPUT, S::TRANSFER);
    let (target, src, dst) = (
        L::COLOR_ATTACHMENT_OPTIMAL,
        L::TRANSFER_SRC_OPTIMAL,
        L::TRANSFER_DST_OPTIMAL,
    );
    let draw = A::COLOR_ATTACHMENT_READ | A::COLOR_ATTACHMENT_WRITE;
    let before = [
        image_barrier(from, color, target, src, draw, A::TRANSFER_READ),
        image_barrier(to, color, L::UNDEFINED, dst, A::empty(), A::TRANSFER_WRITE),
    ];
    barrier(d, cmd, (out | S::FRAGMENT_SHADER, copy), &before);
    let layers = vk::ImageSubresourceLayers {
        aspect_mask: color,
        mip_level: 0,
        base_array_layer: 0,
        layer_count: 1,
    };
    let region = vk::ImageCopy::default()
        .src_subresource(layers)
        .dst_subresource(layers)
        .extent(extent.into());
    d.cmd_copy_image(cmd, from, src, to, dst, &[region]);
    let after = [
        image_barrier(from, color, src, target, A::TRANSFER_READ, draw),
        image_barrier(
            to,
            color,
            dst,
            L::SHADER_READ_ONLY_OPTIMAL,
            A::TRANSFER_WRITE,
            A::SHADER_READ,
        ),
    ];
    barrier(d, cmd, (copy, out | S::FRAGMENT_SHADER), &after);
}

impl Modern {
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
    ///
    /// `old` = Kamera des letzten Bildes (fuers Aussortieren).
    pub(super) unsafe fn record_prepass(&mut self, i: &Inputs, old: &Mat4) {
        let at = (i.cmd, i.fi);
        let mut culling = self.culling.take().filter(|_| self.gpu_cull);
        if let Some(c) = &culling {
            c.record_cull(i.ctx, at, 1, old);
        }
        self.culling = culling.take();
        self.depth_pass(i, 0);
        if !self.gpu_cull {
            return;
        }
        let now = self.view_proj_now;
        if let Some(c) = &mut self.culling {
            c.record_pyramid(i.ctx, at);
            c.record_cull(i.ctx, at, 2, &now);
        }
        self.depth_pass(i, 1);
    }

    /// Tiefen-Durchgang; `list` = 0 beginnt neu, 1 ergaenzt das zunaechst Aussortierte.
    unsafe fn depth_pass(&self, i: &Inputs, list: usize) {
        let Some(t) = &self.targets else {
            return;
        };
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let frag = vk::PipelineStageFlags::FRAGMENT_SHADER;
        let out = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
        let guides = t.guides.iter().flatten();
        let mut images = vec![
            depth_target(t.depth.image),
            color_target(t.normal.image),
            color_target(t.motion.image),
        ];
        images.extend(guides.clone().map(|g| color_target(g.image)));
        if list == 1 {
            // Inhalt behalten
            for (k, b) in images.iter_mut().enumerate() {
                b.old_layout = match k {
                    0 => vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL,
                    _ => vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                };
            }
        }
        barrier(
            d,
            cmd,
            (frag | DEPTH_STAGES | out | COMPUTE, DEPTH_STAGES | out),
            &images,
        );
        let clear = match list {
            0 => vk::AttachmentLoadOp::CLEAR,
            _ => vk::AttachmentLoadOp::LOAD,
        };
        let mut colors = vec![
            color_att(t.normal.view, clear),
            color_att(t.motion.view, clear),
        ];
        colors.extend(guides.clone().map(|g| color_att(g.view, clear)));
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
        let stage = if t.wants.guides {
            Stage::Guides
        } else {
            Stage::Pre
        };
        self.draw_camera(i, stage, list..list + 1);
        // nur wer den Verlauf nutzt, braucht die Bewegung des Himmels
        let last = list == 1 || !self.gpu_cull;
        if last && t.wants.temporal() {
            let sky = self.pipes.sky_guides.filter(|_| t.wants.guides);
            d.cmd_bind_pipeline(cmd, GFX, sky.unwrap_or(self.pipes.sky_move));
            self.bind_any_material(i);
            d.cmd_draw(cmd, 3, 1, 0, 0);
        }
        d.cmd_end_rendering(cmd);
        let mut images = vec![
            depth_read(
                t.depth.image,
                vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL,
            ),
            color_read(t.normal.image),
            color_read(t.motion.image),
        ];
        images.extend(guides.map(|g| color_read(g.image)));
        barrier(
            d,
            cmd,
            (DEPTH_STAGES | out, frag | DEPTH_STAGES | COMPUTE),
            &images,
        );
    }

    /// Beleuchtung ins HDR-Bild, danach Halbtransparentes und Partikel. `traced` = das Deckende
    /// steht schon im (lesbaren) Bild, nur der Rest kommt dazu.
    pub(super) unsafe fn record_lit(&self, i: &Inputs, s: &SceneInput, traced: bool) {
        let Some(t) = &self.targets else {
            return;
        };
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let frag = vk::PipelineStageFlags::FRAGMENT_SHADER;
        let out = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
        let mut target = color_target(t.hdr.image);
        let mut load = vk::AttachmentLoadOp::CLEAR;
        if traced {
            target.old_layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
            load = vk::AttachmentLoadOp::LOAD;
        }
        barrier(d, cmd, (frag | out, out), &[target]);
        let colors = [color_att(t.hdr.view, load)];
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
        if !traced {
            self.mark_blended(i, false);
            self.draw_camera(i, Stage::Opaque, 0..2);
            d.cmd_bind_pipeline(cmd, GFX, self.pipes.sky);
            self.bind_any_material(i);
            d.cmd_draw(cmd, 3, 1, 0, 0);
        }
        if let Some(opaque) = &t.opaque {
            d.cmd_end_rendering(cmd);
            copy_scene(d, cmd, t.hdr.image, opaque.image, i.extent);
            let colors = [color_att(t.hdr.view, vk::AttachmentLoadOp::LOAD)];
            d.cmd_begin_rendering(
                cmd,
                &vk::RenderingInfo::default()
                    .render_area(area(i.extent))
                    .layer_count(1)
                    .color_attachments(&colors)
                    .depth_attachment(&depth),
            );
            set_viewport(d, cmd, i.extent);
        }
        self.bind_frame(i, 2);
        self.mark_blended(i, true);
        self.record_decals(i);
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

    unsafe fn record_decals(&self, i: &Inputs) {
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let batches = self.batches(&self.views.decals);
        if batches.is_empty() {
            return;
        }
        d.cmd_bind_pipeline(cmd, GFX, self.pipes.decal);
        d.cmd_set_cull_mode(cmd, vk::CullModeFlags::FRONT);
        for (k, b) in batches.iter().enumerate() {
            if let Some(m) = self.store.material(super::MaterialId(b.material)) {
                let sets = [m.set];
                d.cmd_bind_descriptor_sets(cmd, GFX, self.pipes.layout, 1, &sets, &[]);
                self.draw_batch(i, b, k == 0, None);
            }
        }
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
