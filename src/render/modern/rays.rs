//! Aufnahme der Strahlen: Verdeckung und Sonnenschatten, Streulicht, Spiegelungen, Pathtracing und
//! das Entrauschen dazu (ueber mehrere Bilder mitteln, dann kantentreu glaetten).

use super::super::frame::area;
use super::super::transfer::set_viewport;
use super::super::Result;
use super::memory::GpuImage;
use super::post::{Post, Push, FRAG, OUT};
use super::scene::{barrier, color_read, color_target};
use super::targets::RayTargets;
use super::types::*;
use super::{Inputs, Modern};
use ash::vk;

/// So viele Bilder werden bei Bewegung gemittelt
const MOVING: f32 = 24.0;
/// … und hoechstens bei stehender Kamera
const STILL: f32 = 2048.0;

impl Modern {
    /// Einfache Modelle einmal fuer die Strahlen umwandeln.
    pub(super) fn convert_meshes(&mut self, i: &Inputs) -> Result<()> {
        if self.rt.is_none() || self.mesh_models.len() == i.mesh_ranges.len() {
            return Ok(());
        }
        let vertices: Vec<crate::gpu::MeshVertex> = i.mesh_vertices_data();
        self.mesh_models.clear();
        for &(first, count) in i.mesh_ranges {
            let part = vertices.get(first as usize..(first + count) as usize);
            let data = crate::shapes::mesh(part.unwrap_or_default().to_vec());
            let id = if count >= 3 {
                Some(self.add_model(i.ctx, &super::ModelData::from_mesh(&data))?)
            } else {
                None
            };
            self.mesh_models.push(id);
        }
        Ok(())
    }

    /// Modell laden, auch fuer die Strahlen.
    pub fn add_model(
        &mut self,
        ctx: &super::Context,
        m: &super::ModelData,
    ) -> Result<super::ModelId> {
        let id = self.store.add_model(ctx, m)?;
        if let Some(rt) = &mut self.rt {
            rt.add(ctx, &mut self.store.arena, id.0, m)?;
        }
        Ok(id)
    }

    pub fn remove_model(&mut self, id: super::ModelId) {
        self.store.remove_model(id);
        if let Some(rt) = &mut self.rt {
            rt.remove(id.0);
        }
    }

    /// Beschleunigungsstrukturen des Bildes bauen, wenn Strahlen gebraucht werden.
    pub(super) unsafe fn build_rays(&mut self, i: &Inputs, s: &SceneInput) -> Result<()> {
        let f = &s.features;
        if f.ray_tracing == RayTracing::default() && f.path_tracing.is_none() {
            return Ok(());
        }
        let (Some(rt), Some(t)) = (&mut self.rt, &self.targets) else {
            return Ok(());
        };
        let views = (t.depth.view, t.normal.view);
        let source = super::rt::Source {
            bones: i.bones.buffer,
            skins: i.skin_vertices,
            ranges: i.skin_ranges,
        };
        rt.record(
            i.ctx,
            (i.cmd, i.fi),
            &mut self.store,
            &self.lists,
            source,
            views,
        )
    }
}

impl Post<'_> {
    /// Ein Durchgang mit Strahlen in ein oder zwei Bilder, die danach gelesen werden.
    unsafe fn ray_pass(&mut self, pipe: vk::Pipeline, out: &[&GpuImage], push: &Push) {
        let (Some(rt), Some(pipes)) = (&self.m.rt, &self.m.pipes.rays) else {
            return;
        };
        let (d, cmd) = (&self.i.ctx.device, self.i.cmd);
        let before: Vec<_> = out.iter().map(|o| color_target(o.image)).collect();
        barrier(d, cmd, (FRAG | OUT, OUT), &before);
        let atts: Vec<_> = out
            .iter()
            .map(|o| {
                vk::RenderingAttachmentInfo::default()
                    .image_view(o.view)
                    .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .store_op(vk::AttachmentStoreOp::STORE)
            })
            .collect();
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area(self.i.extent))
                .layer_count(1)
                .color_attachments(&atts),
        );
        set_viewport(d, cmd, self.i.extent);
        let gfx = vk::PipelineBindPoint::GRAPHICS;
        let sets = [self.m.sets.frames[self.i.fi][0], rt.sets[self.i.fi]];
        d.cmd_bind_pipeline(cmd, gfx, pipe);
        d.cmd_bind_descriptor_sets(cmd, gfx, pipes.layout, 0, &sets, &[]);
        d.cmd_push_constants(
            cmd,
            pipes.layout,
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            0,
            bytemuck::bytes_of(push),
        );
        d.cmd_draw(cmd, 3, 1, 0, 0);
        d.cmd_end_rendering(cmd);
        let after: Vec<_> = out.iter().map(|o| color_read(o.image)).collect();
        barrier(d, cmd, (OUT, FRAG), &after);
    }

    /// Verdeckung und Sonnenschatten.
    pub(super) unsafe fn ray_visibility(&mut self, out: &GpuImage) {
        if let Some(p) = &self.m.pipes.rays {
            self.ray_pass(p.vis, &[out], &Push::a([0.0; 4]));
        }
    }

    /// Streulicht, Spiegelungen oder das ganze Bild.
    pub(super) unsafe fn rays(&mut self, same_camera: bool) {
        let m = self.m;
        let (Some(t), Some(p)) = (&m.targets, &m.pipes.rays) else {
            return;
        };
        let Some(r) = &t.rays else {
            return;
        };
        let f = &self.s.features;
        let [first, second] = &r.now;
        if let Some(cfg) = f.path_tracing {
            let push = Push::a([
                cfg.bounces.min(16) as f32,
                cfg.samples.clamp(1, 64) as f32,
                1.0,
                0.0,
            ]);
            self.ray_pass(p.path, &[first, second], &push);
            let still = cfg.accumulate && same_camera;
            self.denoise(r, 0, still, true);
            // Farbe der ersten Flaeche: bei stehender Kamera ebenfalls mitteln (glatte Kanten)
            let color = if still {
                self.accumulate(r, 1, true, 0.0).view
            } else {
                second.view
            };
            let views = [first.view, color];
            let push = Push::a([0.0; 4]);
            self.draw_into(m.pipes.post.compose, (&t.hdr, self.i.extent), &views, &push);
            return;
        }
        if !(f.ray_tracing.indirect || f.ray_tracing.reflections) {
            return;
        }
        self.ray_pass(p.gi, &[first, second], &Push::a([0.0; 4]));
        if f.ray_tracing.indirect {
            self.denoise(r, 0, false, true);
        }
        if f.ray_tracing.reflections {
            self.denoise(r, 1, false, false);
        }
    }

    /// `now[k]` in den Verlauf mischen; liefert das Ergebnis.
    unsafe fn accumulate<'r>(
        &mut self,
        r: &'r RayTargets,
        k: usize,
        still: bool,
        limit: f32,
    ) -> &'r GpuImage {
        let m = self.m;
        let e = self.i.extent;
        let (new, old) = (&r.history[k][m.ping ^ 1], &r.history[k][m.ping]);
        let now = &r.now[k];
        let Some(t) = &m.targets else {
            return now;
        };
        let old_view = if m.history_valid { old.view } else { now.view };
        let valid = m.history_valid as u8 as f32;
        let most = if still { STILL } else { MOVING };
        let mut push = Push::a([e.width as f32, e.height as f32, valid, most]);
        push.b[0] = limit;
        let views = [now.view, old_view, t.motion.view, t.depth.view];
        self.draw_into(m.pipes.post.accum, (new, e), &views, &push);
        new
    }

    /// `now[k]` entrauschen; das Ergebnis liegt wieder dort.
    unsafe fn denoise(&mut self, r: &RayTargets, k: usize, still: bool, soft: bool) {
        let m = self.m;
        let Some(t) = &m.targets else {
            return;
        };
        let e = self.i.extent;
        let now = &r.now[k];
        // Spiegelungen wandern anders als ihre Flaeche: Verlauf enger begrenzen
        let limit = match (still, soft) {
            (true, _) => 0.0,
            (_, true) => 3.0,
            (_, false) => 1.5,
        };
        let new = self.accumulate(r, k, still, limit);
        let cam = &self.s.camera;
        let strict = if soft { 1.2 } else { 0.6 };
        let [a, b] = &r.temp;
        let steps = [(new, a, 1.0), (a, b, 2.0), (b, now, 4.0)];
        for (from, to, step) in steps {
            let push = Push::a([step, cam.near, cam.far, strict]);
            let views = [from.view, t.depth.view, t.normal.view];
            self.draw_into(m.pipes.post.atrous, (to, e), &views, &push);
        }
    }
}
