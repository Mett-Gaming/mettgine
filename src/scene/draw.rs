//! Ein Bild zeichnen: Instanzen sortieren und gruppieren, Puffer fuellen, aufnehmen,
//! abschicken, anzeigen. Dazu Neuaufbau nach Groessenwechsel.

use super::helpers::write_bmp;
use super::*;

/// Sortiert Modelle nach Mesh (Schattenwerfer zuerst) und bildet instanzierte Gruppen fuer die
/// Szene und fuer den Schatten-Pass. Liefert die Reihenfolge als Indizes in `meshes`.
pub(super) fn build_groups(
    meshes: &[MeshDraw],
    mesh_count: usize,
    max: usize,
) -> (Vec<usize>, Vec<Group>, Vec<Group>) {
    let mut order: Vec<usize> = (0..meshes.len())
        .filter(|&i| meshes[i].mesh < mesh_count)
        .collect();
    order.sort_by_key(|&i| (meshes[i].mesh, !meshes[i].shadow));
    order.truncate(max);
    let mut groups: Vec<Group> = Vec::new();
    let mut shadow_groups: Vec<Group> = Vec::new();
    for (slot, &i) in order.iter().enumerate() {
        let (mesh, slot) = (meshes[i].mesh as u32, slot as u32);
        let extend = |gs: &mut Vec<Group>| match gs.last_mut() {
            Some(g) if g.mesh == mesh && g.first_inst + g.count == slot => g.count += 1,
            _ => gs.push(Group {
                mesh,
                first_inst: slot,
                count: 1,
            }),
        };
        extend(&mut groups);
        if meshes[i].shadow {
            extend(&mut shadow_groups);
        }
    }
    (order, groups, shadow_groups)
}

impl SceneRenderer {
    fn recreate(&mut self) -> Result<()> {
        let (w, h) = self.window_size;
        if w == 0 || h == 0 {
            return Ok(());
        }
        // SAFETY: Neuaufbau; danach nutzt die GPU keine der alten Ressourcen mehr.
        unsafe { self.ctx.device.device_wait_idle()? };
        let new = Swapchain::new(&self.ctx, w, h, self.swapchain.handle)?;
        std::mem::replace(&mut self.swapchain, new).destroy(&self.ctx);
        let ext = offscreen_extent(self.window_size, self.render_scale);
        if ext != self.offscreen {
            let (c, d) = Self::make_targets(&self.ctx, ext)?;
            std::mem::replace(&mut self.color, c).destroy(&self.ctx.device);
            std::mem::replace(&mut self.depth, d).destroy(&self.ctx.device);
            self.offscreen = ext;
            self.write_tex_descriptors();
        }
        self.needs_recreate = false;
        Ok(())
    }

    fn globals(p: &SceneParams) -> Globals {
        let light_vp = light_view_proj(p.sun_dir, p.shadow_center, p.shadow_radius);
        Globals {
            view_proj: p.view_proj.to_cols_array_2d(),
            light_vp: light_vp.to_cols_array_2d(),
            inv_view_proj: p.view_proj.inverse().to_cols_array_2d(),
            cam_pos: p.cam_pos.extend(p.time).to_array(),
            sun_dir: p
                .sun_dir
                .normalize_or_zero()
                .extend(p.shadow_strength)
                .to_array(),
            sun_color: p.sun_color.extend(SHADOW_SIZE as f32).to_array(),
            sky_top: p.sky_top.extend(1.0).to_array(),
            horizon: p.horizon.extend(p.fog_density).to_array(),
            ground: p.ground.extend(1.0).to_array(),
        }
    }

    /// Zeichnet ein Bild. `bones` = alle Knochen-Matrizen (`SkinDraw::first_bone` zeigt hinein),
    /// `ui` = Dreiecke in UI-Einheiten (siehe [`UI_HEIGHT`]).
    pub fn draw(
        &mut self,
        p: &SceneParams,
        meshes: &[MeshDraw],
        skins: &[SkinDraw],
        bones: &[Mat4],
        ui: &[UiVertex],
    ) -> Result<()> {
        if self.window_size.0 == 0 || self.window_size.1 == 0 {
            return Ok(()); // minimiert
        }
        if self.needs_recreate {
            self.recreate()?;
        }
        let fi = self.frame_index;
        let frame = self.frames[fi];
        // SAFETY: Der Fence garantiert, dass die GPU mit den Puffern dieses Bildes fertig ist.
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

            let (order, groups, shadow_groups) =
                build_groups(meshes, self.mesh_ranges.len(), MAX_INSTANCES);
            self.scratch_insts.clear();
            self.scratch_insts.extend(order.iter().map(|&i| {
                let m = &meshes[i];
                GpuInst {
                    model: m.model.to_cols_array_2d(),
                    tint: m.tint.to_array(),
                    extra: [0.0, m.blink, 0.0, 0.0],
                }
            }));
            let skin_base = self.scratch_insts.len() as u32;
            let skin_count = skins.len().min(MAX_INSTANCES - self.scratch_insts.len());
            self.scratch_insts
                .extend(skins[..skin_count].iter().map(|s| GpuInst {
                    model: s.model.to_cols_array_2d(),
                    tint: s.tint.to_array(),
                    extra: [s.first_bone as f32, s.blink, 0.0, 0.0],
                }));
            self.inst_bufs[fi].write(bytemuck::cast_slice(&self.scratch_insts));
            self.scratch_bones.clear();
            self.scratch_bones.extend(
                bones[..bones.len().min(MAX_BONES)]
                    .iter()
                    .map(|m| m.to_cols_array_2d()),
            );
            self.bone_bufs[fi].write(bytemuck::cast_slice(&self.scratch_bones));
            self.globals_bufs[fi].write(bytemuck::bytes_of(&Self::globals(p)));
            let ui_count = ui.len().min(MAX_UI_VERTS) / 3 * 3;
            self.ui_bufs[fi].write(bytemuck::cast_slice(&ui[..ui_count]));

            let cap_target = self.prepare_capture()?;
            let counts = super::frame::Counts {
                skin_base,
                skin_count,
                ui_count: ui_count as u32,
            };
            self.record(
                frame.cmd,
                fi,
                image_index as usize,
                p,
                &groups,
                &shadow_groups,
                skins,
                counts,
                cap_target,
            )?;
            self.submit_and_present(frame, image_index)?;
            if cap_target.is_some() {
                self.finish_capture()?;
            }
        }
        self.frame_index = (self.frame_index + 1) % FRAMES_IN_FLIGHT;
        Ok(())
    }

    unsafe fn submit_and_present(&mut self, frame: Frame, image_index: u32) -> Result<()> {
        let wait = [frame.image_available];
        let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let cmds = [frame.cmd];
        let signal = [self.swapchain.render_finished[image_index as usize]];
        self.ctx.device.queue_submit(
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
            Ok(false) => Ok(()),
            Ok(true) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.needs_recreate = true;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Bildschirmfoto vorbereiten: Zielpuffer fuer die Kopie (nur wenn angefordert).
    fn prepare_capture(&mut self) -> Result<Option<vk::Buffer>> {
        if self.capture.is_none() || !self.swapchain.can_capture {
            return Ok(None);
        }
        let ext = self.swapchain.extent;
        let bytes = (ext.width * ext.height * 4) as u64;
        if self.capture_buf.as_ref().map_or(true, |b| b.size < bytes) {
            if let Some(old) = self.capture_buf.take() {
                old.destroy(&self.ctx.device);
            }
            self.capture_buf = Some(Buffer::new_host(
                &self.ctx,
                bytes,
                vk::BufferUsageFlags::TRANSFER_DST,
            )?);
        }
        Ok(self.capture_buf.as_ref().map(|b| b.buffer))
    }

    /// Bildschirmfoto auslesen (wartet einmal auf die GPU, nur bei Fotos).
    fn finish_capture(&mut self) -> Result<()> {
        // SAFETY: Warten, damit die Kopie fertig im Puffer liegt.
        unsafe { self.ctx.device.device_wait_idle()? };
        let ext = self.swapchain.extent;
        if let (Some(path), Some(buf)) = (self.capture.take(), &self.capture_buf) {
            let px = buf.read((ext.width * ext.height * 4) as usize);
            let rgba = self.swapchain.format == vk::Format::R8G8B8A8_UNORM;
            if let Err(e) = write_bmp(&path, ext.width, ext.height, &px, rgba) {
                crate::crash::log(&format!("Bildschirmfoto fehlgeschlagen: {e}"));
            }
        }
        Ok(())
    }
}
