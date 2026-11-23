//! Aufnahme eines Bildes: Schattenkarte, Szene, Nachbearbeitung + UI, ggf. Bildschirmfoto.

use super::helpers::set_viewport;
use super::*;
use crate::render::util::image_barrier;

/// Zahlen, die die Aufnahme aus der Vorbereitung braucht.
#[derive(Clone, Copy)]
pub(super) struct Counts {
    /// erste Figuren-Instanz (hinter den Modellen)
    pub skin_base: u32,
    pub skin_count: usize,
    pub ui_count: u32,
}

fn depth_clear() -> vk::ClearValue {
    vk::ClearValue {
        depth_stencil: vk::ClearDepthStencilValue {
            depth: 1.0,
            stencil: 0,
        },
    }
}

fn area(extent: vk::Extent2D) -> vk::Rect2D {
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
        groups: &[Group],
        shadow_groups: &[Group],
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
        self.record_shadow(cmd, fi, p, shadow_groups, skins, n);
        self.record_scene(cmd, fi, p, groups, skins, n);
        let sc_image = self.swapchain.images[image_index];
        self.record_screen(cmd, image_index, p, fi, n.ui_count);
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

    unsafe fn draw_skins(&self, cmd: vk::CommandBuffer, skins: &[SkinDraw], n: Counts) {
        if n.skin_count == 0 || self.skin_ranges.is_empty() {
            return;
        }
        let d = &self.ctx.device;
        d.cmd_bind_vertex_buffers(cmd, 0, &[self.skin_vertex_buf.buffer], &[0]);
        for (k, s) in skins[..n.skin_count].iter().enumerate() {
            let (first, count) = self.skin_ranges[s.skin % self.skin_ranges.len()];
            d.cmd_draw(cmd, count, 1, first, n.skin_base + k as u32);
        }
    }

    /// Pass 0: Tiefe aus Sicht der Sonne.
    unsafe fn record_shadow(
        &self,
        cmd: vk::CommandBuffer,
        fi: usize,
        p: &SceneParams,
        groups: &[Group],
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
            self.draw_groups(cmd, groups);
            self.bind_scene(cmd, self.pipes.shadow_skin, self.shadow_sets[fi]);
            self.draw_skins(cmd, skins, n);
        }
        d.cmd_end_rendering(cmd);
    }

    /// Pass 1: Himmel, Modelle, Figuren ins Offscreen-Bild.
    unsafe fn record_scene(
        &self,
        cmd: vk::CommandBuffer,
        fi: usize,
        p: &SceneParams,
        groups: &[Group],
        skins: &[SkinDraw],
        n: Counts,
    ) {
        let d = &self.ctx.device;
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
            &[
                image_barrier(
                    self.shadow.image,
                    vk::ImageAspectFlags::DEPTH,
                    vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                    vk::AccessFlags::SHADER_READ,
                ),
                image_barrier(
                    self.color.image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::AccessFlags::SHADER_READ,
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                ),
                image_barrier(
                    self.depth.image,
                    vk::ImageAspectFlags::DEPTH,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                    vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                    vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                ),
            ],
        );
        let hz = p.horizon;
        let color_att = vk::RenderingAttachmentInfo::default()
            .image_view(self.color.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [hz.x, hz.y, hz.z, 1.0],
                },
            });
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
        self.bind_scene(cmd, self.pipes.sky, self.main_sets[fi]);
        d.cmd_draw(cmd, 3, 1, 0, 0);
        self.bind_scene(cmd, self.pipes.mesh, self.main_sets[fi]);
        self.draw_groups(cmd, groups);
        self.bind_scene(cmd, self.pipes.skin, self.main_sets[fi]);
        self.draw_skins(cmd, skins, n);
        d.cmd_end_rendering(cmd);
    }

    /// Pass 2: Nachbearbeitung und UI auf die Swapchain.
    unsafe fn record_screen(
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
    unsafe fn record_present(
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
