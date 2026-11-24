//! Ecken verdunkeln (einfache Grafik): ein Zwischenschritt zwischen Festem und Halbtransparentem,
//! der die Tiefe der Szene liest (shaders/contact.wgsl).

use super::descriptors::Pass;
use super::frame::area;
use super::transfer::set_viewport;
use super::util::image_barrier;
use super::*;

impl Renderer {
    /// Werden in diesem Bild Ecken verdunkelt?
    pub(super) fn contact_on(&self, f: &FrameInput) -> bool {
        self.lamp.cfg.contact > 0.0
            && self.ms_color.is_none()
            && f.scene.is_none()
            && matches!(f.lighting, Lighting::Rooms(_))
    }

    /// Mitten in der Szene: Zeichnen unterbrechen, Ecken verdunkeln, weiterzeichnen.
    ///
    /// # Safety
    /// Das Zeichnen in `color` und `depth` ist begonnen (ohne Kantenglaettung).
    pub(super) unsafe fn darken_corners(&self, cmd: vk::CommandBuffer, fi: usize) {
        let d = &self.ctx.device;
        d.cmd_end_rendering(cmd);
        self.depth_to_read(cmd, true);
        self.begin_again(cmd, false);
        let gfx = vk::PipelineBindPoint::GRAPHICS;
        let set = self.desc.frames[fi][Pass::Main as usize][0];
        let p = &self.lamp.pipes;
        d.cmd_bind_pipeline(cmd, gfx, p.contact);
        d.cmd_bind_descriptor_sets(cmd, gfx, p.layout, 0, &[set], &[]);
        d.cmd_draw(cmd, 3, 1, 0, 0);
        d.cmd_end_rendering(cmd);
        self.depth_to_read(cmd, false);
        self.begin_again(cmd, true);
    }

    /// Tiefe der Szene lesbar machen bzw. wieder als Ziel benutzen.
    unsafe fn depth_to_read(&self, cmd: vk::CommandBuffer, read: bool) {
        use vk::{AccessFlags as A, ImageLayout as L, PipelineStageFlags as S};
        let target = (
            L::DEPTH_ATTACHMENT_OPTIMAL,
            A::DEPTH_STENCIL_ATTACHMENT_READ | A::DEPTH_STENCIL_ATTACHMENT_WRITE,
            S::EARLY_FRAGMENT_TESTS | S::LATE_FRAGMENT_TESTS | S::COLOR_ATTACHMENT_OUTPUT,
        );
        let texture = (
            L::SHADER_READ_ONLY_OPTIMAL,
            A::SHADER_READ,
            S::FRAGMENT_SHADER | S::COLOR_ATTACHMENT_OUTPUT,
        );
        let (from, to) = if read {
            (target, texture)
        } else {
            (texture, target)
        };
        let color = A::COLOR_ATTACHMENT_READ | A::COLOR_ATTACHMENT_WRITE;
        let keep = L::COLOR_ATTACHMENT_OPTIMAL;
        let barriers = [
            image_barrier(
                self.depth.image,
                vk::ImageAspectFlags::DEPTH,
                from.0,
                to.0,
                from.1,
                to.1,
            ),
            image_barrier(
                self.color.image,
                vk::ImageAspectFlags::COLOR,
                keep,
                keep,
                color,
                color,
            ),
        ];
        let none = vk::DependencyFlags::empty();
        let d = &self.ctx.device;
        d.cmd_pipeline_barrier(cmd, from.2, to.2, none, &[], &[], &barriers);
    }

    /// Weiterzeichnen, wahlweise mit Tiefe.
    unsafe fn begin_again(&self, cmd: vk::CommandBuffer, with_depth: bool) {
        let d = &self.ctx.device;
        let color = vk::RenderingAttachmentInfo::default()
            .image_view(self.color.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE);
        let depth = vk::RenderingAttachmentInfo::default()
            .image_view(self.depth.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::DONT_CARE);
        let mut info = vk::RenderingInfo::default()
            .render_area(area(self.offscreen))
            .layer_count(1)
            .color_attachments(std::slice::from_ref(&color));
        if with_depth {
            info = info.depth_attachment(&depth);
        }
        d.cmd_begin_rendering(cmd, &info);
        set_viewport(d, cmd, self.offscreen);
    }
}
