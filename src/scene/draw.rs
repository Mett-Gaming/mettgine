//! Ein Bild zeichnen: Listen aufbauen (lists.rs), Puffer fuellen, aufnehmen,
//! abschicken, anzeigen. Dazu Neuaufbau nach Groessenwechsel.

use super::lists::{skin_sphere, Views};
use super::*;
use std::mem::size_of;
use std::time::Instant;

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
        if ext != self.offscreen || self.wanted_samples != self.samples {
            let (c, d, m) = Self::make_targets(&self.ctx, ext, self.wanted_samples)?;
            let dev = &self.ctx.device;
            std::mem::replace(&mut self.color, c).destroy(dev);
            std::mem::replace(&mut self.depth, d).destroy(dev);
            if let Some(old) = std::mem::replace(&mut self.ms_color, m) {
                old.destroy(dev);
            }
            self.offscreen = ext;
        }
        self.apply_samples()?;
        self.write_tex_descriptors();
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

    /// Listen aufbauen und alle Puffer des Bildes `fi` fuellen (die GPU ist mit ihnen fertig).
    fn fill_frame(
        &mut self,
        fi: usize,
        p: &SceneParams,
        meshes: &[MeshDraw],
        skins: &[SkinDraw],
        bones: &[Mat4],
        ui: &[UiVertex],
    ) -> Result<super::frame::Counts> {
        let views = Views::new(p, self.culling);
        let (bounds, skin_bounds) = (&self.mesh_bounds, &self.skin_bounds);
        self.lists
            .build_meshes(meshes, self.mesh_ranges.len(), HARD_INSTANCES, |m| {
                views.sees(Some(bounds[m.mesh].transformed(&m.model)))
            });
        let skin_base = self.lists.order.len();
        self.lists
            .build_skins(skins, HARD_INSTANCES - skin_base, |s| {
                views.sees(skin_sphere(s, skin_bounds, bones))
            });
        self.scratch_insts.clear();
        self.scratch_insts
            .extend(self.lists.order.iter().map(|&(_, i)| {
                let m = &meshes[i as usize];
                GpuInst {
                    model: m.model.to_cols_array_2d(),
                    tint: m.tint.to_array(),
                    extra: [0.0, m.blink, 0.0, 0.0],
                }
            }));
        let shown = &skins[..self.lists.skins.len()];
        self.scratch_insts.extend(shown.iter().map(|s| GpuInst {
            model: s.model.to_cols_array_2d(),
            tint: s.tint.to_array(),
            extra: [s.first_bone as f32, s.blink, 0.0, 0.0],
        }));
        self.scratch_bones.clear();
        let bone_count = bones.len().min(HARD_BONES);
        self.scratch_bones
            .extend(bones[..bone_count].iter().map(|m| m.to_cols_array_2d()));
        let ui_count = ui.len().min(HARD_UI_VERTS) / 3 * 3;
        let particles = self.particles.len().min(HARD_PARTICLES);
        self.grow_buffers(fi, ui_count, particles)?;

        self.inst_bufs[fi].write(bytemuck::cast_slice(&self.scratch_insts));
        self.bone_bufs[fi].write(bytemuck::cast_slice(&self.scratch_bones));
        self.globals_bufs[fi].write(bytemuck::bytes_of(&Self::globals(p)));
        self.ui_bufs[fi].write(bytemuck::cast_slice(&ui[..ui_count]));
        self.particle_bufs[fi].write(bytemuck::cast_slice(&self.particles[..particles]));
        let lost = (bones.len() - bone_count) + (self.particles.len() - particles);
        self.particles.clear();

        let l = &self.lists;
        let skin_calls = l
            .skins
            .iter()
            .map(|v| v.camera as u32 + v.shadow as u32)
            .sum::<u32>();
        self.stats.drawn = l.drawn;
        self.stats.culled = l.culled;
        self.stats.dropped = l.dropped + lost as u32;
        // dazu Himmel und Nachbearbeitung, ggf. UI und Partikel
        self.stats.draw_calls = (l.groups.len() + l.shadow_groups.len()) as u32
            + skin_calls
            + 2
            + (ui_count > 0) as u32
            + (particles > 0) as u32;
        Ok(super::frame::Counts {
            skin_base: skin_base as u32,
            ui_count: ui_count as u32,
            particles: particles as u32,
        })
    }

    /// Puffer des Bildes `fi` vergroessern, falls noetig.
    fn grow_buffers(&mut self, fi: usize, ui_verts: usize, particles: usize) -> Result<()> {
        let bytes = |n: usize, size: usize| (n * size) as u64;
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let vertex = vk::BufferUsageFlags::VERTEX_BUFFER;
        let insts = bytes(self.scratch_insts.len(), size_of::<GpuInst>());
        let bones = bytes(self.scratch_bones.len(), 64);
        let mut rewrite = self.inst_bufs[fi].ensure(&self.ctx, insts, storage)?;
        rewrite |= self.bone_bufs[fi].ensure(&self.ctx, bones, storage)?;
        if rewrite {
            self.write_scene_descriptors(fi);
        }
        let ui = bytes(ui_verts, size_of::<UiVertex>());
        self.ui_bufs[fi].ensure(&self.ctx, ui, vertex)?;
        let parts = bytes(particles, size_of::<ParticleInst>());
        self.particle_bufs[fi].ensure(&self.ctx, parts, vertex)?;
        Ok(())
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
            let Some(image_index) = present::acquire(&self.ctx, &self.swapchain, &frame)? else {
                self.needs_recreate = true;
                return Ok(());
            };

            self.timer.collect(&self.ctx.device, fi, &mut self.stats);
            let start = Instant::now();
            let counts = self.fill_frame(fi, p, meshes, skins, bones, ui)?;
            let cap_target = self.capture.prepare(&self.ctx, &self.swapchain)?;
            self.record(
                frame.cmd,
                fi,
                image_index as usize,
                p,
                skins,
                counts,
                cap_target,
            )?;
            self.stats.cpu_ms = start.elapsed().as_secs_f32() * 1000.0;
            if present::submit_and_present(&self.ctx, &self.swapchain, &frame, image_index)? {
                self.needs_recreate = true;
            }
            if cap_target.is_some() {
                self.capture.finish(&self.ctx, &self.swapchain)?;
            }
        }
        self.frame_index = (self.frame_index + 1) % FRAMES_IN_FLIGHT;
        Ok(())
    }
}
