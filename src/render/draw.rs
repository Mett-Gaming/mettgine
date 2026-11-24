//! Ein Bild zeichnen: Listen aufbauen (lists.rs), Puffer fuellen, aufnehmen,
//! abschicken, anzeigen. Dazu Neuaufbau nach Groessenwechsel.

use super::frame::Counts;
use super::lists::{skin_sphere, Views};
use super::*;
use glam::Vec3;
use std::mem::size_of;
use std::time::Instant;

impl Renderer {
    fn recreate(&mut self) -> Result<()> {
        let (w, h) = self.window_size;
        if w == 0 || h == 0 {
            return Ok(());
        }
        if let Some(g) = &self.generator {
            g.gen.apply(false); // waehrend des Umbaus aus
        }
        // SAFETY: Neuaufbau; danach nutzt die GPU keine der alten Ressourcen mehr.
        unsafe { self.ctx.device.device_wait_idle()? };
        let hdr = self.wants_hdr();
        let new = Swapchain::new(&self.ctx, (w, h), self.vsync, hdr, self.swapchain.handle)?;
        std::mem::replace(&mut self.swapchain, new).destroy(&self.ctx);
        self.latency.swapchain_changed();
        if let Some(g) = &mut self.generator {
            // SAFETY: die GPU ist untaetig (oben gewartet)
            unsafe { g.rebuild(&self.ctx, &self.swapchain)? };
            g.gen.apply(true);
        }
        if let Some(h) = &mut self.hdr {
            // SAFETY: die GPU ist untaetig (oben gewartet)
            unsafe {
                h.rebuild(
                    &self.ctx,
                    &self.desc,
                    self.pipes.tex_layout,
                    &self.swapchain,
                )?
            };
        }
        let ext = offscreen_extent(self.window_size, self.resolution);
        if ext != self.offscreen || self.wanted_samples != self.samples {
            let (c, d, m) =
                Self::make_targets(&self.ctx, ext, self.wanted_samples, self.setup.color)?;
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
        // die Tiefe der Szene steht auch in den Szenen-Sets
        for fi in 0..FRAMES_IN_FLIGHT {
            self.write_scene_descriptors(fi);
        }
        self.needs_recreate = false;
        Ok(())
    }

    /// Uniform fuer eine Kamera; `second` = zweite Kamera.
    fn globals(&self, f: &FrameInput, view: &View, second: bool, lamps: usize) -> Globals {
        let v4 = |v: Vec3, w: f32| v.extend(w).to_array();
        let rooms = matches!(f.lighting, Lighting::Rooms(_));
        let mut g = Globals {
            view_proj: view.view_proj.to_cols_array_2d(),
            inv_view_proj: view.view_proj.inverse().to_cols_array_2d(),
            cam_pos: v4(view.pos, f.time),
            params: [
                self.light_count as f32,
                0.0,
                rooms as u8 as f32,
                second as u8 as f32,
            ],
            ..Globals::zeroed()
        };
        match &f.lighting {
            Lighting::Sun(s) => {
                let light = light_view_proj(s.dir, s.shadow_center, s.shadow_radius);
                g.light_vp = light.to_cols_array_2d();
                g.sun_dir = v4(s.dir.normalize_or_zero(), s.shadow_strength);
                g.sun_color = v4(s.color, SHADOW_SIZE as f32);
                g.sky_top = v4(s.sky_top, 1.0);
                g.horizon = v4(s.horizon, s.fog_density);
                g.ground = v4(s.ground, 1.0);
            }
            Lighting::Rooms(r) => {
                g.horizon = v4(r.fog_color, r.fog_density);
                g.params[1] = r.dark as u8 as f32;
                g.sway = r.sway.to_array();
                g.extra = [r.decay, lamps as f32, self.lamp.cfg.contact, 0.0];
            }
        }
        g
    }

    /// Was ist zu sehen? Fuellt `lists`; liefert die erste Figuren-Instanz.
    fn build_lists(&mut self, f: &FrameInput) -> usize {
        let lamps = self.lamp_spheres(f);
        let views = Views::new(f, self.second.enabled, self.culling, lamps);
        // Schwanken bewegt feste Objekte hoechstens so weit
        let grow = match &f.lighting {
            Lighting::Rooms(r) => r.sway.x.abs() + r.sway.y.abs(),
            Lighting::Sun(_) => 0.0,
        };
        let runs = &mut self.visible_runs;
        for (k, frustum) in views.frustums().into_iter().enumerate() {
            let boxes = frustum.filter(|_| self.cull_boxes);
            self.static_runs
                .visible(frustum, grow, &mut runs.statics[k]);
            self.box_runs.visible(boxes, grow, &mut runs.boxes[k]);
        }
        let (bounds, skin_bounds) = (&self.mesh_bounds, &self.skin_bounds);
        let known = self.mesh_ranges.len();
        let mesh = |m: &MeshDraw| views.sees(Some(bounds[m.mesh].transformed(&m.model)));
        let l = &mut self.lists;
        l.build_meshes(f.meshes, known, HARD_INSTANCES, mesh);
        let room = HARD_INSTANCES - l.order.len();
        l.build_ghosts(f.ghosts, known, room, mesh);
        let skin_base = l.order.len() + l.ghosts.len();
        l.build_skins(f.skins, f.camera_skins, HARD_INSTANCES - skin_base, |s| {
            views.sees(skin_sphere(s, skin_bounds, f.bones))
        });
        skin_base
    }

    /// Listen aufbauen und alle Puffer des Bildes `fi` fuellen (die GPU ist mit ihnen fertig).
    fn fill_frame(&mut self, fi: usize, f: &FrameInput) -> Result<Counts> {
        let lamps = self.prepare_lamps(fi, f);
        let skin_base = self.build_lists(f);
        let l = &self.lists;
        self.scratch_insts.clear();
        let meshes = l.order.iter().map(|&(_, i)| &f.meshes[i as usize]);
        let ghosts = l.ghosts.iter().map(|&i| &f.ghosts[i as usize]);
        self.scratch_insts
            .extend(meshes.chain(ghosts).map(|m| GpuInst::mesh(m, false)));
        self.scratch_insts
            .extend(l.skins.iter().map(|s| GpuInst::skin(&s.draw)));
        self.scratch_bones.clear();
        let bone_count = f.bones.len().min(HARD_BONES);
        self.scratch_bones
            .extend(f.bones[..bone_count].iter().map(|m| m.to_cols_array_2d()));
        if let (Some(m), Some(_)) = (&mut self.modern, f.scene) {
            self.scratch_bones
                .extend_from_slice(m.old_bones(bone_count));
        }
        let ui_count = f.ui.len().min(HARD_UI_VERTS) / 3 * 3;
        let particles = self.particles.len().min(HARD_PARTICLES);
        self.grow_buffers(fi, ui_count, particles)?;

        self.inst_bufs[fi].write(bytemuck::cast_slice(&self.scratch_insts));
        self.bone_bufs[fi].write(bytemuck::cast_slice(&self.scratch_bones));
        let main = self.globals(f, &f.view, false, lamps);
        self.globals_bufs[fi].write(bytemuck::bytes_of(&main));
        if let Some(view) = &f.second_camera {
            let second = self.globals(f, view, true, lamps);
            self.second_globals[fi].write(bytemuck::bytes_of(&second));
        }
        self.ui_bufs[fi].write(bytemuck::cast_slice(&f.ui[..ui_count]));
        self.particle_bufs[fi].write(bytemuck::cast_slice(&self.particles[..particles]));
        let lost = (f.bones.len() - bone_count) + (self.particles.len() - particles);
        self.particles.clear();

        let l = &self.lists;
        self.stats.drawn = l.drawn;
        self.stats.culled = l.culled;
        self.stats.dropped = l.dropped + lost as u32;
        // dazu Himmel bzw. Boxen, Nachbearbeitung, feste Modelle, ggf. UI und Partikel
        self.stats.draw_calls = l.draw_calls()
            + self.visible_runs.statics[0].len() as u32
            + (self.visible_runs.boxes[0].len() as u32).saturating_sub(1)
            + 2
            + (ui_count > 0) as u32
            + (particles > 0) as u32;
        Ok(Counts {
            skin_base: skin_base as u32,
            ui: ui_count as u32,
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

    /// Grafikspeicher hin und wieder nachsehen.
    fn measure_memory(&mut self) {
        if self.memory_in > 0 {
            self.memory_in -= 1;
            return;
        }
        self.memory_in = MEMORY_EVERY;
        let (instance, pd) = (&self.ctx.instance, self.ctx.physical);
        if let Some((used, budget)) = self.ctx.diagnostics.memory(instance, pd) {
            self.stats.memory_mb = (used >> 20) as u32;
            self.stats.memory_budget_mb = (budget >> 20) as u32;
        }
    }

    /// Zeichnet ein Bild. Solange das Startbild laeuft, erscheint stattdessen dieses.
    pub fn draw(&mut self, f: &FrameInput) -> Result<()> {
        let r = self.draw_frame(f);
        self.latency.end(false);
        if r.as_ref().is_err_and(|e| is_device_lost(e.as_ref())) {
            let why = self.ctx.diagnostics.fault_report();
            let why = why.unwrap_or_else(|| "kein Grund bekannt".into());
            crate::crash::context("Grafikkarte ausgefallen", &why);
            crate::crash::log(&format!("Grafikkarte ausgefallen: {why}"));
        }
        r
    }

    fn draw_frame(&mut self, f: &FrameInput) -> Result<()> {
        if self.window_size.0 == 0 || self.window_size.1 == 0 {
            return Ok(()); // minimiert
        }
        if self.needs_recreate {
            self.recreate()?;
        }
        self.latency.ensure_begun();
        for m in [
            vk::LatencyMarkerNV::SIMULATION_END,
            vk::LatencyMarkerNV::RENDERSUBMIT_START,
        ] {
            self.latency.mark(&self.ctx, &self.swapchain, m);
        }
        let fi = self.frame_index;
        let frame = self.frames[fi];
        // SAFETY: Der Fence garantiert, dass die GPU mit den Puffern dieses Bildes fertig ist.
        unsafe {
            let Some(image_index) = present::acquire(&self.ctx, &self.swapchain, &frame)? else {
                self.needs_recreate = true;
                return Ok(());
            };
            if let Err(e) = self.draw_to(f, fi, &frame, image_index) {
                // Bild verwerfen; gelingt das, kann das Spiel weiterzeichnen
                present::reset_frame(&self.ctx, &mut self.frames[fi])?;
                self.needs_recreate = true;
                return Err(e);
            }
        }
        self.frame_index = (fi + 1) % FRAMES_IN_FLIGHT;
        Ok(())
    }

    /// Das geholte Swapchain-Bild fuellen, abschicken und anzeigen.
    ///
    /// # Safety
    /// Die GPU muss mit den Puffern des Bildes `fi` fertig sein.
    unsafe fn draw_to(
        &mut self,
        f: &FrameInput,
        fi: usize,
        frame: &Frame,
        image_index: u32,
    ) -> Result<()> {
        let image = image_index as usize;
        let cap = self.capture.prepare(&self.ctx, &self.swapchain)?;
        if self.splash.active() {
            let (target, view) = self.screen_target(image);
            let to = (target, view, self.swapchain.extent);
            let d = &self.ctx.device;
            self.splash.record(d, frame.cmd, fi, to)?;
            self.finish_commands(frame.cmd, image, cap)?;
            self.particles.clear();
        } else {
            self.timer.collect(&self.ctx.device, fi, &mut self.stats);
            self.measure_memory();
            let start = Instant::now();
            if f.scene.is_some() && self.modern.is_none() {
                let m = self.new_modern()?;
                self.modern = Some(Box::new(m));
            }
            // moderne Grafik: Modelle und Figuren laufen ueber ihre eigenen Listen
            let simple = FrameInput {
                view: f.scene.map_or(f.view, |s| s.camera.to_view()),
                ..*f
            };
            let empty = FrameInput {
                meshes: &[],
                ghosts: &[],
                skins: &[],
                camera_skins: &[],
                ..simple
            };
            let counts = self.fill_frame(fi, if f.scene.is_some() { &empty } else { f })?;
            self.record(frame.cmd, fi, image, &simple, counts, cap)?;
            self.stats.cpu_ms = start.elapsed().as_secs_f32() * 1000.0;
        }
        let (ctx, sc) = (&self.ctx, &self.swapchain);
        if present::submit_and_present(ctx, sc, frame, image_index, &self.latency)? {
            self.needs_recreate = true;
        }
        self.latency.end(true);
        if let Some(g) = &self.generator {
            g.gen.presented();
        }
        if cap.is_some() {
            let paper = self.paper_white();
            self.capture.finish(&self.ctx, &self.swapchain, paper)?;
        }
        Ok(())
    }
}
