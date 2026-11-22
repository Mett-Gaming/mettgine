//! Zweite Kamera (Ueberwachungsmonitor): rendert die Szene klein in ein eigenes Bild, das
//! die Welt-Shader auf dem Monitor-Material zeigen.

use super::*;

impl Renderer {
    /// Zweite Kamera: Welt, Waren, Meshes und Figuren in das kleine Monitor-Bild.
    /// cam_pos.w = 2 sagt den Shadern: das hier ist die Kamera (der Monitor selbst bleibt
    /// dann dunkel, sonst gaebe es eine Rueckkopplung).
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn record_monitor(
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
}
