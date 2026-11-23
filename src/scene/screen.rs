//! Letzter Pass: Nachbearbeitung, UI, Bildschirmfoto, Anzeigen.

use super::frame::area;
use super::*;
use crate::render::transfer::set_viewport;
use crate::render::util::image_barrier;

impl SceneRenderer {
    /// Pass 2: Nachbearbeitung und UI auf die Swapchain.
    pub(super) unsafe fn record_screen(
        &self,
        cmd: vk::CommandBuffer,
        image_index: usize,
        p: &SceneParams,
        fi: usize,
        ui_count: u32,
    ) {
        let d = &self.ctx.device;
        let gfx = vk::PipelineBindPoint::GRAPHICS;
        let sc_image = self.swapchain.images[image_index];
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::PipelineStageFlags::FRAGMENT_SHADER
                | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[
                image_barrier(
                    self.color.image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                    vk::AccessFlags::SHADER_READ,
                ),
                image_barrier(
                    sc_image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::AccessFlags::empty(),
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                ),
            ],
        );
        let extent = self.swapchain.extent;
        let att = vk::RenderingAttachmentInfo::default()
            .image_view(self.swapchain.views[image_index])
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::DONT_CARE)
            .store_op(vk::AttachmentStoreOp::STORE);
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area(extent))
                .layer_count(1)
                .color_attachments(std::slice::from_ref(&att)),
        );
        set_viewport(d, cmd, extent);
        d.cmd_bind_pipeline(cmd, gfx, self.pipes.post);
        d.cmd_bind_descriptor_sets(cmd, gfx, self.pipes.post_layout, 0, &[self.post_set], &[]);
        let post = [p.fade, p.red, p.saturation, 0.0f32];
        d.cmd_push_constants(
            cmd,
            self.pipes.post_layout,
            vk::ShaderStageFlags::FRAGMENT,
            0,
            bytemuck::bytes_of(&post),
        );
        d.cmd_draw(cmd, 3, 1, 0, 0);
        if ui_count > 0 {
            d.cmd_bind_pipeline(cmd, gfx, self.pipes.ui);
            d.cmd_bind_descriptor_sets(cmd, gfx, self.pipes.ui_layout, 0, &[self.ui_set], &[]);
            let screen = [self.ui_width(), UI_HEIGHT, 0.0, 0.0];
            d.cmd_push_constants(
                cmd,
                self.pipes.ui_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                bytemuck::bytes_of(&screen),
            );
            d.cmd_bind_vertex_buffers(cmd, 0, &[self.ui_bufs[fi].buffer], &[0]);
            d.cmd_draw(cmd, ui_count, 1, 0, 0);
        }
        d.cmd_end_rendering(cmd);
    }

    /// Swapchain-Bild zum Anzeigen freigeben, vorher ggf. in den Foto-Puffer kopieren.
    pub(super) unsafe fn record_present(
        &self,
        cmd: vk::CommandBuffer,
        sc_image: vk::Image,
        capture: Option<vk::Buffer>,
    ) {
        let d = &self.ctx.device;
        let mut layout = vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL;
        let mut src_stage = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
        let mut src_access = vk::AccessFlags::COLOR_ATTACHMENT_WRITE;
        if let Some(buf) = capture {
            let extent = self.swapchain.extent;
            d.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[image_barrier(
                    sc_image,
                    vk::ImageAspectFlags::COLOR,
                    layout,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                    vk::AccessFlags::TRANSFER_READ,
                )],
            );
            d.cmd_copy_image_to_buffer(
                cmd,
                sc_image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                buf,
                &[vk::BufferImageCopy {
                    buffer_offset: 0,
                    buffer_row_length: 0,
                    buffer_image_height: 0,
                    image_subresource: vk::ImageSubresourceLayers {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        mip_level: 0,
                        base_array_layer: 0,
                        layer_count: 1,
                    },
                    image_offset: vk::Offset3D::default(),
                    image_extent: vk::Extent3D {
                        width: extent.width,
                        height: extent.height,
                        depth: 1,
                    },
                }],
            );
            layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
            src_stage = vk::PipelineStageFlags::TRANSFER;
            src_access = vk::AccessFlags::TRANSFER_READ;
        }
        d.cmd_pipeline_barrier(
            cmd,
            src_stage,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier(
                sc_image,
                vk::ImageAspectFlags::COLOR,
                layout,
                vk::ImageLayout::PRESENT_SRC_KHR,
                src_access,
                vk::AccessFlags::empty(),
            )],
        );
    }
}
