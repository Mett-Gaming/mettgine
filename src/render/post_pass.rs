//! UI-Ebene und VHS-Nachbearbeitung: die UI wird in eine eigene transparente Ebene gezeichnet,
//! dann setzt der Post-Pass Szene und UI in die Swapchain (optional mit Screenshot).

use super::*;

impl Renderer {
    /// Pass 1b: UI in eine eigene, transparente Ebene.
    pub(super) unsafe fn record_ui_pass(
        &self,
        cmd: vk::CommandBuffer,
        fi: usize,
        ui_count: u32,
        off_extent: vk::Extent2D,
    ) {
        let d = &self.ctx.device;
        // --- Pass 1b: UI -> eigene, transparente Ebene ---
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier(
                self.ui_target.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::COLOR_ATTACHMENT_READ,
            )],
        );
        let ui_att = vk::RenderingAttachmentInfo::default()
            .image_view(self.ui_target.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue { float32: [0.0; 4] },
            });
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent: off_extent,
                })
                .layer_count(1)
                .color_attachments(std::slice::from_ref(&ui_att)),
        );
        if ui_count > 0 {
            set_viewport(d, cmd, off_extent);
            d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.ui_pipeline);
            d.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.ui_layout,
                0,
                &[self.ui_set],
                &[],
            );
            let screen = [RENDER_WIDTH as f32, RENDER_HEIGHT as f32, 0.0, 0.0];
            d.cmd_push_constants(
                cmd,
                self.ui_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                bytemuck::bytes_of(&screen),
            );
            d.cmd_bind_vertex_buffers(cmd, 0, &[self.ui_bufs[fi].buffer], &[0]);
            d.cmd_draw(cmd, ui_count, 1, 0, 0);
        }
        d.cmd_end_rendering(cmd);
    }

    /// Pass 2: VHS-Nachbearbeitung in die Swapchain, danach ggf. Screenshot-Kopie.
    pub(super) unsafe fn record_post_pass(
        &self,
        cmd: vk::CommandBuffer,
        image_index: usize,
        p: &FrameParams,
        capture: Option<vk::Buffer>,
    ) {
        let d = &self.ctx.device;
        // --- Pass 2: VHS-Post -> Swapchain ---
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
                    self.ui_target.image,
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
        let sc_extent = self.swapchain.extent;
        let sc_att = vk::RenderingAttachmentInfo::default()
            .image_view(self.swapchain.views[image_index])
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::DONT_CARE)
            .store_op(vk::AttachmentStoreOp::STORE);
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent: sc_extent,
                })
                .layer_count(1)
                .color_attachments(std::slice::from_ref(&sc_att)),
        );
        set_viewport(d, cmd, sc_extent);
        d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.post_pipeline);
        d.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            self.post_layout,
            0,
            &[self.post_set],
            &[],
        );
        let post = PostPush {
            params: [p.time, p.glitch, if p.tipped { 1.0 } else { 0.0 }, p.vhs],
            res: [
                sc_extent.width as f32,
                sc_extent.height as f32,
                RENDER_WIDTH as f32,
                RENDER_HEIGHT as f32,
            ],
            extra: [p.fade, p.red, p.invert, p.flash],
        };
        d.cmd_push_constants(
            cmd,
            self.post_layout,
            vk::ShaderStageFlags::FRAGMENT,
            0,
            bytemuck::bytes_of(&post),
        );
        d.cmd_draw(cmd, 3, 1, 0, 0);
        d.cmd_end_rendering(cmd);

        let mut layout = vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL;
        let mut src_stage = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
        let mut src_access = vk::AccessFlags::COLOR_ATTACHMENT_WRITE;
        if let Some(buf) = capture {
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
                        width: sc_extent.width,
                        height: sc_extent.height,
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
