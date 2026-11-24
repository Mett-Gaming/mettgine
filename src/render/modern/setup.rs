//! Ein Bild der modernen Grafik vorbereiten: Sichten, Listen, Puffer, Sets.

use super::*;

impl Modern {
    fn ensure_targets(&mut self, ctx: &Context, wants: Wants) -> Result<()> {
        if self.targets.as_ref().is_some_and(|t| t.wants == wants) {
            return Ok(());
        }
        // SAFETY: danach benutzt die GPU keins der alten Bilder mehr.
        unsafe {
            ctx.device.device_wait_idle()?;
            if let Some(old) = self.targets.take() {
                old.destroy(&mut self.store.arena, &ctx.device);
            }
        }
        self.targets = Some(Targets::new(ctx, &mut self.store.arena, wants, self.ldr)?);
        self.history_valid = false;
        Ok(())
    }

    /// Sichten berechnen, Objekte sortieren, Puffer fuellen, Sets schreiben.
    pub(super) fn prepare(
        &mut self,
        i: &Inputs,
        f: &FrameInput,
        s: &SceneInput,
        wants: Wants,
    ) -> Result<()> {
        self.convert_meshes(i)?;
        self.ensure_targets(i.ctx, wants)?;
        if s.cut {
            self.history_valid = false;
            self.motion.forget();
            if let Some(c) = &mut self.culling {
                c.forget();
            }
        }
        let cam = &s.camera;
        let plain = Plain {
            mesh_bounds: i.mesh_bounds,
            skin_bounds: i.skin_bounds,
            material: self.plain_material.0,
            boxes: i.boxes,
            cube: self.cube.0,
            statics: i.statics,
            mesh_models: &self.mesh_models,
        };
        self.lists
            .collect(f, s, &self.store, &plain, &mut self.motion);
        let cascades = self.sun_views(s);
        let lights = &s.lights[..s.lights.len().min(clusters::MAX_LIGHTS)];
        self.lamp_views(s, lights);
        let vp = cam.view_proj();
        self.views.camera = self.lists.view(&vp, false);
        self.views.decals = self.lists.decals(s.decals, self.cube.0);
        self.views.blend = self.lists.blended(&vp, cam.pos);
        self.clusters.build(cam, lights);
        let phases = if wants.vendor {
            super::super::vendor::jitter_phases(wants.width, wants.out_width)
        } else {
            JITTERS
        };
        let jitter = if wants.temporal() {
            gpu::jitter(self.number, phases)
        } else {
            Vec2::ZERO
        };
        self.jitter = jitter;
        let tiles: Vec<gpu::GpuTile> = self.tiles.iter().map(atlas::Tile::gpu).collect();
        let scene = GpuScene::new(
            s,
            &SceneExtras {
                extent: i.extent,
                prev_view_proj: self.prev_view_proj.filter(|_| !s.cut).unwrap_or(vp),
                jitter,
                cascades: cascades.as_ref(),
                tiles: &tiles,
                atlas: atlas::ATLAS_SIZE,
                sky_mips: self.sky_texture(s).1,
                time: f.time,
                number: self.number,
                lamps: lights.len(),
            },
        );
        self.prepare_culling(i, s, wants)?;
        self.fill(i, s, &scene)?;
        self.write_sets(i, s);
        self.prev_view_proj = Some(vp);
        self.view_proj_now = gpu::jittered(cam, jitter, i.extent);
        Ok(())
    }

    /// Zeichenbefehle fuer das Aussortieren auf der Grafikkarte.
    fn prepare_culling(&mut self, i: &Inputs, s: &SceneInput, wants: Wants) -> Result<()> {
        self.gpu_cull = s.features.gpu_culling && i.ctx.gpu_culling;
        if !self.gpu_cull {
            if let Some(c) = &mut self.culling {
                c.forget();
            }
            return Ok(());
        }
        let mut c = match self.culling.take() {
            Some(c) => c,
            None => culling::Culling::new(i.ctx)?,
        };
        let store = &self.store;
        let counts = |b: &prepare::Batch| match b.kind {
            prepare::Kind::Mesh => i.mesh_ranges.get(b.model as usize).map(|r| (r.1, r.0)),
            prepare::Kind::Skin => i.skin_ranges.get(b.model as usize).map(|r| (r.1, r.0)),
            _ => store.model(ModelId(b.model)).map(|m| (m.index_count, 0)),
        };
        let lists = &mut self.lists;
        let all = (&lists.batches[..], self.views.camera.clone());
        // Platz fuer die Befehle braucht den Speicher, die Modelle gehoeren demselben Lager
        let mut arena = std::mem::take(&mut self.arena_for_culling);
        let r = c.prepare(
            i.ctx,
            i.fi,
            &mut arena,
            all,
            &mut lists.order,
            &lists.spheres,
            counts,
        );
        let r = r.and_then(|()| c.ensure(i.ctx, &mut arena, wants.scene()));
        self.arena_for_culling = arena;
        self.culling = Some(c);
        r
    }

    fn sun_views(&mut self, s: &SceneInput) -> Option<[Cascade; cascades::CASCADES]> {
        self.views.sun.clear();
        let sun = s.sun.filter(|l| l.shadow_distance > 0.0)?;
        let size = self.targets.as_ref()?.wants.shadow;
        if size == 0 {
            return None;
        }
        let c = cascades::cascades(&s.camera, sun.dir, sun.shadow_distance, size);
        for c in &c {
            let r = if s.features.ray_tracing.sun_shadows {
                self.lists.view_without_rays(&c.view_proj)
            } else {
                self.lists.view(&c.view_proj, true)
            };
            self.views.sun.push((c.view_proj, r));
        }
        Some(c)
    }

    fn lamp_views(&mut self, s: &SceneInput, lights: &[Light]) {
        self.views.lamps.clear();
        self.lamp_data.clear();
        let first = if s.features.lamp_shadows {
            atlas::assign(lights, s.camera.pos, &mut self.tiles)
        } else {
            self.tiles.clear();
            vec![None; lights.len()]
        };
        for t in &self.tiles {
            let r = self.lists.lamp_view(&t.view_proj, t.pos, t.range);
            self.views.lamps.push((*t, r));
        }
        let data = lights.iter().zip(first).map(|(l, t)| GpuLamp::new(l, t));
        self.lamp_data.extend(data);
    }

    fn fill(&mut self, i: &Inputs, s: &SceneInput, scene: &GpuScene) -> Result<()> {
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let mut materials = self.store.material_values();
        if let Some(m) = materials.get_mut(self.plain_material.0 as usize) {
            m.set_roughness(s.plain_roughness);
        }
        let b = &mut self.frames[i.fi];
        let put = |buf: &mut Buffer, bytes: &[u8]| -> Result<()> {
            buf.ensure(i.ctx, bytes.len() as u64, storage)?;
            buf.write(bytes);
            Ok(())
        };
        put(&mut b.objs, bytemuck::cast_slice(&self.lists.objs))?;
        put(&mut b.order, bytemuck::cast_slice(&self.lists.order))?;
        put(&mut b.lamps, bytemuck::cast_slice(&self.lamp_data))?;
        put(&mut b.cells, bytemuck::cast_slice(&self.clusters.cells))?;
        put(&mut b.entries, bytemuck::cast_slice(&self.clusters.entries))?;
        put(&mut b.materials, bytemuck::cast_slice(&materials))?;
        b.scene.write(bytemuck::bytes_of(scene));
        Ok(())
    }

    /// Ansicht und Stufen des Himmelsbilds (ohne Bild: weiss).
    fn sky_texture(&self, s: &SceneInput) -> (vk::ImageView, u32) {
        let id = match s.sky {
            Sky::Image { texture, .. } => texture,
            Sky::Gradient { .. } => self.store.white,
        };
        let image = self
            .store
            .texture(id)
            .or(self.store.texture(self.store.white));
        image.map_or((vk::ImageView::null(), 1), |i| (i.view, i.mips))
    }

    fn write_sets(&self, i: &Inputs, s: &SceneInput) {
        let Some(t) = &self.targets else {
            return;
        };
        let b = &self.frames[i.fi];
        let buffers = [
            &b.objs,
            i.bones,
            &b.scene,
            &b.lamps,
            &b.cells,
            &b.entries,
            &b.materials,
            &b.order,
        ];
        let white = self.store.texture(self.store.white);
        let white = white.map_or(vk::ImageView::null(), |w| w.view);
        let sky = self.sky_texture(s).0;
        let dummy = self.fixed.dummy_depth.view;
        if let (true, Some(c)) = (self.gpu_cull, &self.culling) {
            c.write_sets(i.ctx, i.fi, b.order.buffer, t.depth.view);
        }
        let read = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        let sun = t.sun.as_ref().unwrap_or(&self.fixed.dummy_array).view;
        let lamps = t.lamps.as_ref().unwrap_or(&self.fixed.dummy_depth).view;
        let ao = t.ao.as_ref().map_or(white, |a| a[0].view);
        let sets = &self.sets.frames[i.fi];
        let array = self.fixed.dummy_array.view;
        let w = |set, images, layout| self.sets.write_frame(i.ctx, set, buffers, images, layout);
        let now = |k: usize| {
            let image = t.rays.as_ref().and_then(|r| r.now.get(k));
            image.map_or(white, |i| i.view)
        };
        let [gi, mirror, lamp, gloss] = [0, 1, 2, 3].map(now);
        w(
            sets[0],
            [array, dummy, white, sky, dummy, white, white, white, white],
            read,
        );
        w(
            sets[1],
            [sun, lamps, ao, sky, dummy, gi, mirror, lamp, gloss],
            read,
        );
        let depth = vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL;
        w(
            sets[2],
            [sun, lamps, ao, sky, t.depth.view, gi, mirror, lamp, gloss],
            depth,
        );
    }
}
