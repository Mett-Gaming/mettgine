//! Aufzeichnung eines Frames: Welt, statische Waren, Meshes, Geister, UI und Nachbearbeitung.

use super::*;

impl Renderer {
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

        self.timer.begin(d, cmd, fi);
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

        self.timer.mark(d, cmd, fi, "Kamera");
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
                if !self.visible[i] {
                    continue;
                }
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
                if !self.visible[(skin_base + offset) as usize + k] {
                    continue;
                }
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
                if !self.visible[meshes.len() + j] {
                    continue;
                }
                let (first, count) = self.mesh_ranges[m.mesh];
                d.cmd_draw(cmd, count, 1, first, (meshes.len() + j) as u32);
            }
        }

        d.cmd_end_rendering(cmd);

        self.timer.mark(d, cmd, fi, "Szene");
        self.record_ui_pass(cmd, fi, ui_count, off_extent);

        self.timer.mark(d, cmd, fi, "Menue");
        self.record_post_pass(cmd, image_index, p, capture);
        self.timer.mark(d, cmd, fi, "Nachbearbeitung");
        d.end_command_buffer(cmd)?;
        Ok(())
    }
}
