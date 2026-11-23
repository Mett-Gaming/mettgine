//! Aufnahme eines Bildes: Schattenkarte und Szene (der Rest steht in screen.rs).

use super::lists::Visible;
use super::*;
use crate::render::transfer::set_viewport;
use crate::render::util::image_barrier;

/// Zahlen, die die Aufnahme aus der Vorbereitung braucht.
#[derive(Clone, Copy)]
pub(super) struct Counts {
    /// erste Figuren-Instanz (hinter den Modellen)
    pub skin_base: u32,
    pub ui_count: u32,
    pub particles: u32,
}

fn depth_clear() -> vk::ClearValue {
    vk::ClearValue {
        depth_stencil: vk::ClearDepthStencilValue {
            depth: 1.0,
            stencil: 0,
        },
    }
}

pub(super) fn area(extent: vk::Extent2D) -> vk::Rect2D {
    vk::Rect2D {
        offset: vk::Offset2D::default(),
        extent,
    }
}

impl SceneRenderer {
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn record(
        &self,
        cmd: vk::CommandBuffer,
        fi: usize,
        image_index: usize,
        p: &SceneParams,
        skins: &[SkinDraw],
        n: Counts,
        capture: Option<vk::Buffer>,
    ) -> Result<()> {
        let d = &self.ctx.device;
        d.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
        d.begin_command_buffer(
            cmd,
            &vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )?;
        self.timer.begin(d, cmd, fi);
        self.record_shadow(cmd, fi, p, skins, n);
        self.timer.mark(d, cmd, fi, "Schatten");
        self.record_scene(cmd, fi, p, skins, n);
        self.timer.mark(d, cmd, fi, "Szene");
        let sc_image = self.swapchain.images[image_index];
        self.record_screen(cmd, image_index, p, fi, n.ui_count);
        self.timer.mark(d, cmd, fi, "Nachbearbeitung");
        self.record_present(cmd, sc_image, capture);
        d.end_command_buffer(cmd)?;
        Ok(())
    }

    unsafe fn bind_scene(
        &self,
        cmd: vk::CommandBuffer,
        pipe: vk::Pipeline,
        set: vk::DescriptorSet,
    ) {
        let d = &self.ctx.device;
        let gfx = vk::PipelineBindPoint::GRAPHICS;
        d.cmd_bind_pipeline(cmd, gfx, pipe);
        d.cmd_bind_descriptor_sets(cmd, gfx, self.pipes.scene_layout, 0, &[set], &[]);
    }

    unsafe fn draw_groups(&self, cmd: vk::CommandBuffer, gs: &[Group]) {
        let d = &self.ctx.device;
        d.cmd_bind_vertex_buffers(cmd, 0, &[self.mesh_vertex_buf.buffer], &[0]);
        for g in gs {
            let (first, count) = self.mesh_ranges[g.mesh as usize];
            d.cmd_draw(cmd, count, g.count, first, g.first_inst);
        }
    }

    unsafe fn draw_skins(
        &self,
        cmd: vk::CommandBuffer,
        skins: &[SkinDraw],
        n: Counts,
        shown: impl Fn(&Visible) -> bool,
    ) {
        if self.lists.skins.is_empty() || self.skin_ranges.is_empty() {
            return;
        }
        let d = &self.ctx.device;
        d.cmd_bind_vertex_buffers(cmd, 0, &[self.skin_vertex_buf.buffer], &[0]);
        for (k, (s, v)) in skins.iter().zip(&self.lists.skins).enumerate() {
            if shown(v) {
                let (first, count) = self.skin_ranges[s.skin % self.skin_ranges.len()];
                d.cmd_draw(cmd, count, 1, first, n.skin_base + k as u32);
            }
        }
    }

    unsafe fn draw_particles(&self, cmd: vk::CommandBuffer, fi: usize, count: u32) {
        if count == 0 {
            return;
        }
        let d = &self.ctx.device;
        self.bind_scene(cmd, self.pipes.scene.particle, self.main_sets[fi]);
        d.cmd_bind_vertex_buffers(cmd, 0, &[self.particle_bufs[fi].buffer], &[0]);
        d.cmd_draw(cmd, 6, count, 0, 0);
    }

    /// Pass 0: Tiefe aus Sicht der Sonne.
    unsafe fn record_shadow(
        &self,
        cmd: vk::CommandBuffer,
        fi: usize,
        p: &SceneParams,
        skins: &[SkinDraw],
        n: Counts,
    ) {
        let d = &self.ctx.device;
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier(
                self.shadow.image,
                vk::ImageAspectFlags::DEPTH,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                vk::AccessFlags::SHADER_READ,
                vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                    | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
            )],
        );
        let extent = vk::Extent2D {
            width: SHADOW_SIZE,
            height: SHADOW_SIZE,
        };
        let att = vk::RenderingAttachmentInfo::default()
            .image_view(self.shadow.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(depth_clear());
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area(extent))
                .layer_count(1)
                .depth_attachment(&att),
        );
        if p.shadow_strength > 0.0 {
            set_viewport(d, cmd, extent);
            self.bind_scene(cmd, self.pipes.shadow, self.shadow_sets[fi]);
            self.draw_groups(cmd, &self.lists.shadow_groups);
            self.bind_scene(cmd, self.pipes.shadow_skin, self.shadow_sets[fi]);
            self.draw_skins(cmd, skins, n, |v| v.shadow);
        }
        d.cmd_end_rendering(cmd);
    }

    /// Pass 1: Himmel, Modelle, Figuren ins Offscreen-Bild.
    unsafe fn record_scene(
        &self,
        cmd: vk::CommandBuffer,
        fi: usize,
        p: &SceneParams,
        skins: &[SkinDraw],
        n: Counts,
    ) {
        let d = &self.ctx.device;
        let color_barrier = |image| {
            image_barrier(
                image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::AccessFlags::SHADER_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            )
        };
        // der letzte Eintrag gilt nur mit Kantenglaettung
        let ms = self.ms_color.as_ref();
        let barriers = [
            image_barrier(
                self.shadow.image,
                vk::ImageAspectFlags::DEPTH,
                vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                vk::AccessFlags::SHADER_READ,
            ),
            color_barrier(self.color.image),
            image_barrier(
                self.depth.image,
                vk::ImageAspectFlags::DEPTH,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                    | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
            ),
            color_barrier(ms.unwrap_or(&self.color).image),
        ];
        let used = if ms.is_some() { 4 } else { 3 };
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::LATE_FRAGMENT_TESTS
                | vk::PipelineStageFlags::FRAGMENT_SHADER
                | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::PipelineStageFlags::FRAGMENT_SHADER
                | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &barriers[..used],
        );
        let hz = p.horizon;
        let mut color_att = vk::RenderingAttachmentInfo::default()
            .image_view(ms.unwrap_or(&self.color).view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [hz.x, hz.y, hz.z, 1.0],
                },
            });
        if ms.is_some() {
            color_att = color_att
                .store_op(vk::AttachmentStoreOp::DONT_CARE)
                .resolve_mode(vk::ResolveModeFlags::AVERAGE)
                .resolve_image_view(self.color.view)
                .resolve_image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
        }
        let depth_att = vk::RenderingAttachmentInfo::default()
            .image_view(self.depth.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::DONT_CARE)
            .clear_value(depth_clear());
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area(self.offscreen))
                .layer_count(1)
                .color_attachments(std::slice::from_ref(&color_att))
                .depth_attachment(&depth_att),
        );
        set_viewport(d, cmd, self.offscreen);
        self.bind_scene(cmd, self.pipes.scene.sky, self.main_sets[fi]);
        d.cmd_draw(cmd, 3, 1, 0, 0);
        self.bind_scene(cmd, self.pipes.scene.mesh, self.main_sets[fi]);
        self.draw_groups(cmd, &self.lists.groups);
        self.bind_scene(cmd, self.pipes.scene.skin, self.main_sets[fi]);
        self.draw_skins(cmd, skins, n, |v| v.camera);
        self.draw_particles(cmd, fi, n.particles);
        d.cmd_end_rendering(cmd);
    }
}
