//! Aufzeichnung eines Frames: Welt, statische Waren, Meshes, Geister, UI und Nachbearbeitung.

use super::*;

impl Renderer {
    /// Zweite Kamera: Welt, Waren, Meshes und Figuren in das kleine Monitor-Bild.
    /// cam_pos.w = 2 sagt den Shadern: das hier ist die Kamera (der Monitor selbst bleibt
    /// dann dunkel, sonst gaebe es eine Rueckkopplung).
    #[allow(clippy::too_many_arguments)]
    unsafe fn record_monitor(
        &self,
        cmd: vk::CommandBuffer,
        fi: usize,
        p: &FrameParams,
        meshes: &[MeshDraw],
        ghost_count: usize,
        skins: &[SkinDraw],
        skin_ghosts: &[SkinDraw],
        monitor_skins: &[SkinDraw],
    ) {
        let d = &self.ctx.device;
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::FRAGMENT_SHADER | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[
                image_barrier(
                    self.monitor.image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::AccessFlags::SHADER_READ,
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                ),
                image_barrier(
                    self.monitor_depth.image,
                    vk::ImageAspectFlags::DEPTH,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                    vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                    vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                ),
            ],
        );
        let extent = vk::Extent2D {
            width: MONITOR_WIDTH,
            height: MONITOR_HEIGHT,
        };
        let fc = p.fog_color;
        let color_att = vk::RenderingAttachmentInfo::default()
            .image_view(self.monitor.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [fc.x, fc.y, fc.z, 1.0],
                },
            });
        let depth_att = vk::RenderingAttachmentInfo::default()
            .image_view(self.monitor_depth.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::DONT_CARE)
            .clear_value(vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0,
                },
            });
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent,
                })
                .layer_count(1)
                .color_attachments(std::slice::from_ref(&color_att))
                .depth_attachment(&depth_att),
        );
        if let Some((view_proj, cam)) = p.monitor {
            set_viewport(d, cmd, extent);
            let push = WorldPush {
                view_proj: view_proj.to_cols_array_2d(),
                cam_pos: cam.extend(2.0).to_array(),
                params: [
                    p.time,
                    self.light_count as f32,
                    if p.tipped { 1.0 } else { 0.0 },
                    p.fog_density,
                ],
                fog_color: fc.extend(p.decay).to_array(),
                sway: p.sway.to_array(),
            };
            let stages = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
            let bind = |pipe: vk::Pipeline, set: vk::DescriptorSet| {
                d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipe);
                d.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.scene_layout,
                    0,
                    &[set],
                    &[],
                );
                d.cmd_push_constants(cmd, self.scene_layout, stages, 0, bytemuck::bytes_of(&push));
            };
            bind(self.world_pipeline, self.world_set_cam);
            if self.instance_count > 0 {
                d.cmd_draw(cmd, 36, self.instance_count, 0, 0);
            }
            if !self.static_ranges.is_empty() {
                bind(self.mesh_pipeline, self.static_set);
                d.cmd_bind_vertex_buffers(cmd, 0, &[self.mesh_vertex_buf.buffer], &[0]);
                for &(mesh, first_inst, n) in &self.static_ranges {
                    let (first, count) = self.mesh_ranges[mesh as usize];
                    d.cmd_draw(cmd, count, n, first, first_inst);
                }
            }
            if !meshes.is_empty() {
                bind(self.mesh_pipeline, self.mesh_sets[fi]);
                d.cmd_bind_vertex_buffers(cmd, 0, &[self.mesh_vertex_buf.buffer], &[0]);
                for (i, m) in meshes.iter().enumerate() {
                    let (first, count) = self.mesh_ranges[m.mesh];
                    d.cmd_draw(cmd, count, 1, first, i as u32);
                }
            }
            // Figuren: die normalen und die, die es nur auf dem Monitor gibt
            let skin_base = (meshes.len() + ghost_count) as u32;
            let mon_base = skin_base + (skins.len() + skin_ghosts.len()) as u32;
            if !skins.is_empty() || !monitor_skins.is_empty() {
                bind(self.skin_pipeline, self.mesh_sets[fi]);
                d.cmd_bind_vertex_buffers(cmd, 0, &[self.skin_vertex_buf.buffer], &[0]);
                let all = skins
                    .iter()
                    .enumerate()
                    .map(|(k, s)| (s, skin_base + k as u32))
                    .chain(
                        monitor_skins
                            .iter()
                            .enumerate()
                            .map(|(k, s)| (s, mon_base + k as u32)),
                    );
                for (s, inst) in all {
                    let (first, count) = self.skin_ranges[s.skin % self.skin_ranges.len()];
                    d.cmd_draw(cmd, count, 1, first, inst);
                }
            }
        }
        d.cmd_end_rendering(cmd);
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier(
                self.monitor.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                vk::AccessFlags::SHADER_READ,
            )],
        );
    }

    pub(super) unsafe fn record(
        &self,
        cmd: vk::CommandBuffer,
        fi: usize,
        image_index: usize,
        p: &FrameParams,
        meshes: &[MeshDraw],
        ghosts: &[MeshDraw],
        skins: &[SkinDraw],
        skin_ghosts: &[SkinDraw],
        monitor_skins: &[SkinDraw],
        ui_count: u32,
        capture: Option<vk::Buffer>,
    ) -> Result<()> {
        let d = &self.ctx.device;
        d.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
        d.begin_command_buffer(
            cmd,
            &vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )?;

        // --- Pass 0: Ueberwachungskamera -> Monitor-Bild ---
        self.record_monitor(
            cmd,
            fi,
            p,
            meshes,
            ghosts.len(),
            skins,
            skin_ghosts,
            monitor_skins,
        );

        // --- Pass 1: Szene -> Offscreen (480x270) ---
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::FRAGMENT_SHADER
                | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[
                image_barrier(
                    self.color.image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                        | vk::AccessFlags::COLOR_ATTACHMENT_READ,
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

        let off_extent = vk::Extent2D {
            width: RENDER_WIDTH,
            height: RENDER_HEIGHT,
        };
        let fc = p.fog_color;
        let color_att = vk::RenderingAttachmentInfo::default()
            .image_view(self.color.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [fc.x, fc.y, fc.z, 1.0],
                },
            });
        let depth_att = vk::RenderingAttachmentInfo::default()
            .image_view(self.depth.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::DONT_CARE)
            .clear_value(vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0,
                },
            });
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent: off_extent,
                })
                .layer_count(1)
                .color_attachments(std::slice::from_ref(&color_att))
                .depth_attachment(&depth_att),
        );
        set_viewport(d, cmd, off_extent);

        let push = WorldPush {
            view_proj: p.view_proj.to_cols_array_2d(),
            cam_pos: p.cam_pos.extend(1.0).to_array(),
            params: [
                p.time,
                self.light_count as f32,
                if p.tipped { 1.0 } else { 0.0 },
                p.fog_density,
            ],
            fog_color: fc.extend(p.decay).to_array(),
            sway: p.sway.to_array(),
        };
        let scene_stages = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;

        // Statische Welt
        d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.world_pipeline);
        d.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            self.scene_layout,
            0,
            &[self.world_set],
            &[],
        );
        d.cmd_push_constants(
            cmd,
            self.scene_layout,
            scene_stages,
            0,
            bytemuck::bytes_of(&push),
        );
        if self.instance_count > 0 {
            d.cmd_draw(cmd, 36, self.instance_count, 0, 0);
        }

        // Statische Waren: ein instanzierter Draw-Call pro Modell
        if !self.static_ranges.is_empty() {
            d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.mesh_pipeline);
            d.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.scene_layout,
                0,
                &[self.static_set],
                &[],
            );
            d.cmd_push_constants(
                cmd,
                self.scene_layout,
                scene_stages,
                0,
                bytemuck::bytes_of(&push),
            );
            d.cmd_bind_vertex_buffers(cmd, 0, &[self.mesh_vertex_buf.buffer], &[0]);
            for &(mesh, first_inst, n) in &self.static_ranges {
                let (first, count) = self.mesh_ranges[mesh as usize];
                d.cmd_draw(cmd, count, n, first, first_inst);
            }
        }

        // Meshes
        if !meshes.is_empty() || !ghosts.is_empty() {
            d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.mesh_pipeline);
            d.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.scene_layout,
                0,
                &[self.mesh_sets[fi]],
                &[],
            );
            d.cmd_push_constants(
                cmd,
                self.scene_layout,
                scene_stages,
                0,
                bytemuck::bytes_of(&push),
            );
            d.cmd_bind_vertex_buffers(cmd, 0, &[self.mesh_vertex_buf.buffer], &[0]);
            for (i, m) in meshes.iter().enumerate() {
                let (first, count) = self.mesh_ranges[m.mesh];
                d.cmd_draw(cmd, count, 1, first, i as u32);
            }
        }
        // Animierte Figuren (Instanzen hinter Meshes und Hologrammen)
        let skin_base = (meshes.len() + ghosts.len()) as u32;
        for (pipe, list, offset) in [
            (self.skin_pipeline, skins, 0u32),
            (self.skin_ghost_pipeline, skin_ghosts, skins.len() as u32),
        ] {
            if list.is_empty() {
                continue;
            }
            d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipe);
            d.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.scene_layout,
                0,
                &[self.mesh_sets[fi]],
                &[],
            );
            d.cmd_push_constants(
                cmd,
                self.scene_layout,
                scene_stages,
                0,
                bytemuck::bytes_of(&push),
            );
            d.cmd_bind_vertex_buffers(cmd, 0, &[self.skin_vertex_buf.buffer], &[0]);
            for (k, s) in list.iter().enumerate() {
                let (first, count) = self.skin_ranges[s.skin % self.skin_ranges.len()];
                d.cmd_draw(cmd, count, 1, first, skin_base + offset + k as u32);
            }
        }
        // Hologramme: gleiche Mesh-Daten, halbtransparent, ohne Tiefe zu schreiben
        if !ghosts.is_empty() {
            d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.ghost_pipeline);
            d.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.scene_layout,
                0,
                &[self.mesh_sets[fi]],
                &[],
            );
            d.cmd_push_constants(
                cmd,
                self.scene_layout,
                scene_stages,
                0,
                bytemuck::bytes_of(&push),
            );
            d.cmd_bind_vertex_buffers(cmd, 0, &[self.mesh_vertex_buf.buffer], &[0]);
            for (j, m) in ghosts.iter().enumerate() {
                let (first, count) = self.mesh_ranges[m.mesh];
                d.cmd_draw(cmd, count, 1, first, (meshes.len() + j) as u32);
            }
        }

        d.cmd_end_rendering(cmd);

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
        d.end_command_buffer(cmd)?;
        Ok(())
    }
}
