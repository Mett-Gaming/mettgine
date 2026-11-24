//! Letzter Teil eines Bildes: UI-Ebene, Nachbearbeitung, UI aufs Fenster.

use super::frame::{area, to_readable};
use super::transfer::set_viewport;
use super::util::image_barrier;
use super::*;

impl Renderer {
    /// Liegt die UI in der eigenen Ebene (und wird mit nachbearbeitet)?
    fn uses_ui_layer(&self, f: &FrameInput) -> bool {
        self.ui_layer.enabled && matches!(f.post, Post::Vhs(_))
    }

    unsafe fn push(&self, cmd: vk::CommandBuffer, data: &[[f32; 4]]) {
        self.ctx.device.cmd_push_constants(
            cmd,
            self.pipes.tex_layout,
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            0,
            bytemuck::cast_slice(data),
        );
    }

    unsafe fn bind_tex(&self, cmd: vk::CommandBuffer, pipe: vk::Pipeline, set: vk::DescriptorSet) {
        let d = &self.ctx.device;
        let gfx = vk::PipelineBindPoint::GRAPHICS;
        d.cmd_bind_pipeline(cmd, gfx, pipe);
        d.cmd_bind_descriptor_sets(cmd, gfx, self.pipes.tex_layout, 0, &[set], &[]);
    }

    unsafe fn draw_ui(&self, cmd: vk::CommandBuffer, pipe: vk::Pipeline, fi: usize, count: u32) {
        if count == 0 {
            return;
        }
        let d = &self.ctx.device;
        let (w, h) = self.ui_area();
        self.bind_tex(cmd, pipe, self.desc.ui);
        self.push(cmd, &[[w, h, 0.0, 0.0]]);
        d.cmd_bind_vertex_buffers(cmd, 0, &[self.ui_bufs[fi].buffer], &[0]);
        d.cmd_draw(cmd, count, 1, 0, 0);
    }

    /// UI in ihre eigene, transparente Ebene.
    unsafe fn record_ui_layer(&self, cmd: vk::CommandBuffer, fi: usize, count: u32) {
        let d = &self.ctx.device;
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier(
                self.ui_layer.image.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::COLOR_ATTACHMENT_READ,
            )],
        );
        let att = [vk::RenderingAttachmentInfo::default()
            .image_view(self.ui_layer.image.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)];
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area(self.ui_layer.extent))
                .layer_count(1)
                .color_attachments(&att),
        );
        set_viewport(d, cmd, self.ui_layer.extent);
        self.draw_ui(cmd, self.pipes.ui_layer, fi, count);
        d.cmd_end_rendering(cmd);
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[to_readable(self.ui_layer.image.image)],
        );
    }

    /// Pipeline und Werte der Nachbearbeitung (siehe `post_clean.wgsl`, `post_vhs.wgsl`).
    fn post_setup(&self, f: &FrameInput) -> (vk::Pipeline, [[f32; 4]; 3]) {
        match &f.post {
            Post::Clean(c) => {
                // 0 = SDR, sonst so viel heller als Weiss darf es werden
                let bright = if self.hdr_active() {
                    self.headroom()
                } else {
                    0.0
                };
                let params = [c.fade, c.red, c.saturation, bright];
                (self.pipes.post_clean, [params, [0.0; 4], [0.0; 4]])
            }
            Post::Plain(fade) => {
                let params = [*fade, 0.0, 0.0, 0.0];
                (self.pipes.post_plain, [params, [0.0; 4], [0.0; 4]])
            }
            Post::Vhs(v) => {
                let (out, src) = (self.swapchain.extent, self.offscreen);
                let params = [f.time, v.glitch, v.dark as u8 as f32, v.strength];
                let res = [out.width, out.height, src.width, src.height].map(|x| x as f32);
                let extra = [v.fade, v.red, v.invert, v.flash];
                (self.pipes.post_vhs, [params, res, extra])
            }
        }
    }

    /// Nachbearbeitung und UI auf die Swapchain.
    pub(super) unsafe fn record_screen(
        &self,
        cmd: vk::CommandBuffer,
        image: usize,
        f: &FrameInput,
        fi: usize,
        ui_count: u32,
    ) {
        let d = &self.ctx.device;
        let layered = self.uses_ui_layer(f);
        if layered {
            self.record_ui_layer(cmd, fi, ui_count);
        }
        let (target, view) = self.screen_target(image);
        let target = image_barrier(
            target,
            vk::ImageAspectFlags::COLOR,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            vk::AccessFlags::empty(),
            vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        );
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::PipelineStageFlags::FRAGMENT_SHADER
                | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[to_readable(self.color.image), target],
        );
        let extent = self.swapchain.extent;
        let att = [vk::RenderingAttachmentInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::DONT_CARE)
            .store_op(vk::AttachmentStoreOp::STORE)];
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area(extent))
                .layer_count(1)
                .color_attachments(&att),
        );
        set_viewport(d, cmd, extent);
        let (pipe, values) = self.post_setup(f);
        self.bind_tex(cmd, pipe, self.desc.post);
        self.push(cmd, &values);
        d.cmd_draw(cmd, 3, 1, 0, 0);
        if let Some(g) = self.keeps_hudless() {
            d.cmd_end_rendering(cmd);
            g.keep(d, cmd, self.swapchain.images[image]);
            let att = [att[0].load_op(vk::AttachmentLoadOp::LOAD)];
            d.cmd_begin_rendering(
                cmd,
                &vk::RenderingInfo::default()
                    .render_area(area(extent))
                    .layer_count(1)
                    .color_attachments(&att),
            );
            set_viewport(d, cmd, extent);
        }
        if let Some(m) = self.modern.as_deref().filter(|_| !f.images.is_empty()) {
            modern::record_images(m, d, (cmd, fi), self.ui_area(), f.images);
        }
        if !layered {
            self.draw_ui(cmd, self.pipes.ui, fi, ui_count);
        }
        d.cmd_end_rendering(cmd);
    }
}
