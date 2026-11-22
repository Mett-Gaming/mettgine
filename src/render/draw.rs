//! Ein Bild zeichnen: Per-Frame-Daten schreiben, Befehle aufzeichnen (frame.rs), abschicken,
//! praesentieren; Swapchain bei Groessenaenderung neu aufbauen.

use super::*;

impl Renderer {
    fn recreate_swapchain(&mut self) -> Result<()> {
        let (w, h) = self.window_size;
        if w == 0 || h == 0 {
            return Ok(());
        }
        unsafe { self.ctx.device.device_wait_idle()? };
        let new = Swapchain::new(&self.ctx, w, h, self.swapchain.handle)?;
        std::mem::replace(&mut self.swapchain, new).destroy(&self.ctx);
        self.needs_recreate = false;
        Ok(())
    }

    /// `ghosts` werden halbtransparent ueber die Szene gelegt (Hologramme).
    /// `monitor_skins` erscheinen nur auf dem Ueberwachungsmonitor.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        p: &FrameParams,
        meshes: &[MeshDraw],
        ghosts: &[MeshDraw],
        skins: &[SkinDraw],
        monitor_skins: &[SkinDraw],
        bones: &[Mat4],
        ui: &[UiVertex],
    ) -> Result<()> {
        if self.window_size.0 == 0 || self.window_size.1 == 0 {
            return Ok(()); // minimiert
        }
        if self.needs_recreate {
            self.recreate_swapchain()?;
        }
        let fi = self.frame_index;
        let frame = self.frames[fi];
        unsafe {
            let d = &self.ctx.device;
            d.wait_for_fences(&[frame.in_flight], true, u64::MAX)?;
            let image_index = match self.ctx.swapchain_loader.acquire_next_image(
                self.swapchain.handle,
                u64::MAX,
                frame.image_available,
                vk::Fence::null(),
            ) {
                Ok((i, _)) => i,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.needs_recreate = true;
                    return Ok(());
                }
                Err(e) => return Err(e.into()),
            };
            d.reset_fences(&[frame.in_flight])?;

            // Per-Frame-Daten schreiben (der Fence garantiert, dass die GPU damit fertig ist)
            let mesh_count = meshes.len().min(MAX_MESH_DRAWS);
            let ghost_count = ghosts.len().min(MAX_MESH_DRAWS - mesh_count);
            let mut insts: Vec<GpuMeshInst> = meshes[..mesh_count]
                .iter()
                .chain(ghosts[..ghost_count].iter())
                .map(|m| GpuMeshInst {
                    model: m.model.to_cols_array_2d(),
                    tint: [m.tint.x, m.tint.y, m.tint.z, 1.0],
                    extra: [m.tint.w, 0.0, 0.0, 0.0],
                })
                .collect();
            // Figuren dahinter: erst undurchsichtige, dann Hologramme; extra.y = erster Knochen
            let bone_count = bones.len().min(MAX_BONES);
            let room = MAX_MESH_DRAWS - insts.len();
            let mut solid_skins: Vec<SkinDraw> =
                skins.iter().filter(|s| !s.ghost).copied().collect();
            let mut ghost_skins: Vec<SkinDraw> =
                skins.iter().filter(|s| s.ghost).copied().collect();
            solid_skins.truncate(room);
            ghost_skins.truncate(room - solid_skins.len());
            let mut mon_skins: Vec<SkinDraw> = monitor_skins.to_vec();
            mon_skins.truncate(room - solid_skins.len() - ghost_skins.len());
            for s in solid_skins
                .iter()
                .chain(ghost_skins.iter())
                .chain(mon_skins.iter())
            {
                insts.push(GpuMeshInst {
                    model: s.model.to_cols_array_2d(),
                    tint: [s.tint.x, s.tint.y, s.tint.z, 1.0],
                    extra: [s.tint.w, s.first_bone as f32, s.distort, 0.0],
                });
            }
            let bone_data: Vec<[[f32; 4]; 4]> = bones[..bone_count]
                .iter()
                .map(|m| m.to_cols_array_2d())
                .collect();
            self.bone_bufs[fi].write(bytemuck::cast_slice(&bone_data));
            self.mesh_inst_bufs[fi].write(bytemuck::cast_slice(&insts));
            let ui_count = ui.len().min(MAX_UI_VERTS) / 3 * 3;
            self.ui_bufs[fi].write(bytemuck::cast_slice(&ui[..ui_count]));

            let ext = self.swapchain.extent;
            let cap_bytes = (ext.width * ext.height * 4) as u64;
            let capturing = self.capture.is_some() && self.swapchain.can_capture;
            if capturing
                && self
                    .capture_buf
                    .as_ref()
                    .map_or(true, |b| b.size < cap_bytes)
            {
                if let Some(old) = self.capture_buf.take() {
                    old.destroy(&self.ctx.device);
                }
                self.capture_buf = Some(Buffer::new_host(
                    &self.ctx,
                    cap_bytes,
                    vk::BufferUsageFlags::TRANSFER_DST,
                )?);
            }
            let cap_target = if capturing {
                self.capture_buf.as_ref().map(|b| b.buffer)
            } else {
                None
            };

            self.record(
                frame.cmd,
                fi,
                image_index as usize,
                p,
                &meshes[..mesh_count],
                &ghosts[..ghost_count],
                &solid_skins,
                &ghost_skins,
                &mon_skins,
                ui_count as u32,
                cap_target,
            )?;

            let wait = [frame.image_available];
            let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            let cmds = [frame.cmd];
            let signal = [self.swapchain.render_finished[image_index as usize]];
            d.queue_submit(
                self.ctx.queue,
                &[vk::SubmitInfo::default()
                    .wait_semaphores(&wait)
                    .wait_dst_stage_mask(&stages)
                    .command_buffers(&cmds)
                    .signal_semaphores(&signal)],
                frame.in_flight,
            )?;

            let swapchains = [self.swapchain.handle];
            let indices = [image_index];
            match self.ctx.swapchain_loader.queue_present(
                self.ctx.queue,
                &vk::PresentInfoKHR::default()
                    .wait_semaphores(&signal)
                    .swapchains(&swapchains)
                    .image_indices(&indices),
            ) {
                Ok(false) => {}
                Ok(true) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.needs_recreate = true,
                Err(e) => return Err(e.into()),
            }
            if capturing {
                self.ctx.device.device_wait_idle()?;
                if let (Some(path), Some(buf)) = (self.capture.take(), &self.capture_buf) {
                    let px = buf.read(cap_bytes as usize);
                    let rgba = self.swapchain.format == vk::Format::R8G8B8A8_UNORM;
                    if let Err(e) = write_bmp(&path, ext.width, ext.height, &px, rgba) {
                        crate::crash::log(&format!("Screenshot fehlgeschlagen: {e}"));
                    }
                }
            }
        }
        self.frame_index = (self.frame_index + 1) % FRAMES_IN_FLIGHT;
        Ok(())
    }
}
