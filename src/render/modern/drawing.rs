//! Zeichenbefehle: Pipelines, Materialien und Modelle binden, direkt oder indirekt zeichnen.

use super::culling::{Culling, COMMAND};
use super::prepare::{Batch, Kind};
use super::{Inputs, Modern};
use ash::vk;

const GFX: vk::PipelineBindPoint = vk::PipelineBindPoint::GRAPHICS;

/// Welche Pipelines ein Durchgang benutzt.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    Shadow,
    Pre,
    /// wie `Pre`, mit Bildern fuers Entrauschen
    Guides,
    Opaque,
    Blend,
}

impl Modern {
    /// Zeichenbefehle aufnehmen, Zustand nur bei Wechsel setzen.
    pub(super) unsafe fn draw(&self, i: &Inputs, stage: Stage, batches: &[Batch]) {
        self.draw_from(i, stage, batches, None);
    }

    /// Sicht der Kamera, ggf. auf der Grafikkarte aussortiert; `lists` = Durchgaenge des
    /// Aussortierens.
    pub(super) unsafe fn draw_camera(
        &self,
        i: &Inputs,
        stage: Stage,
        lists: std::ops::Range<usize>,
    ) {
        let batches = self.batches(&self.views.camera);
        match self.culling.as_ref().filter(|_| self.gpu_cull) {
            None => self.draw(i, stage, batches),
            Some(c) => {
                for k in lists {
                    self.draw_from(i, stage, batches, Some((c, k)));
                }
            }
        }
    }

    /// `culled` = Befehle aus einem Durchgang des Aussortierens.
    pub(super) unsafe fn draw_from(
        &self,
        i: &Inputs,
        stage: Stage,
        batches: &[Batch],
        culled: Option<(&Culling, usize)>,
    ) {
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let mut pipe = vk::Pipeline::null();
        let mut material = None;
        let mut model: Option<(Kind, u32)> = None;
        let mut cull = None;
        for (n, b) in batches.iter().enumerate() {
            let k = self.pipes.kind(b.kind);
            let want = match stage {
                Stage::Shadow if b.masked => k.shadow_masked,
                Stage::Shadow => k.shadow,
                Stage::Pre => k.pre,
                Stage::Guides => k.guides.unwrap_or(k.pre),
                Stage::Opaque => k.opaque,
                Stage::Blend => k.blend,
            };
            if want != pipe {
                d.cmd_bind_pipeline(cmd, GFX, want);
                pipe = want;
            }
            let two_sided = b.double_sided;
            if cull != Some(two_sided) {
                d.cmd_set_cull_mode(
                    cmd,
                    if two_sided {
                        vk::CullModeFlags::NONE
                    } else {
                        vk::CullModeFlags::BACK
                    },
                );
                cull = Some(two_sided);
            }
            if material != Some(b.material) {
                if let Some(m) = self.store.material(super::MaterialId(b.material)) {
                    let sets = [m.set];
                    d.cmd_bind_descriptor_sets(cmd, GFX, self.pipes.layout, 1, &sets, &[]);
                }
                material = Some(b.material);
            }
            let bind = model != Some((b.kind, b.model));
            model = Some((b.kind, b.model));
            let indirect = culled.map(|(c, list)| c.command(i.fi, list, n));
            self.draw_batch(i, b, bind, indirect);
        }
    }

    /// Ein Zeichenbefehl; `bind` = das Modell hat gewechselt.
    pub(super) unsafe fn draw_batch(
        &self,
        i: &Inputs,
        b: &Batch,
        bind: bool,
        indirect: Option<(vk::Buffer, u64)>,
    ) {
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let (vertices, ranges) = match b.kind {
            Kind::Mesh => (i.mesh_vertices, i.mesh_ranges),
            Kind::Skin => (i.skin_vertices, i.skin_ranges),
            Kind::Object | Kind::Skinned => {
                let Some(m) = self.store.model(super::ModelId(b.model)) else {
                    return;
                };
                if bind {
                    d.cmd_bind_vertex_buffers(cmd, 0, &[m.vertices.buffer], &[0]);
                    if let Some(j) = &m.joints {
                        d.cmd_bind_vertex_buffers(cmd, 1, &[j.buffer], &[0]);
                    }
                    d.cmd_bind_index_buffer(cmd, m.indices.buffer, 0, vk::IndexType::UINT32);
                }
                match indirect {
                    Some((buffer, at)) => {
                        d.cmd_draw_indexed_indirect(cmd, buffer, at, 1, COMMAND as u32);
                    }
                    None => d.cmd_draw_indexed(cmd, m.index_count, b.count, 0, 0, b.first),
                }
                return;
            }
        };
        if bind {
            d.cmd_bind_vertex_buffers(cmd, 0, &[vertices], &[0]);
        }
        if let Some((buffer, at)) = indirect {
            d.cmd_draw_indirect(cmd, buffer, at, 1, COMMAND as u32);
        } else if let Some(&(first, count)) = ranges.get(b.model as usize) {
            d.cmd_draw(cmd, count, b.count, first, b.first);
        }
    }

    pub(super) unsafe fn bind_frame(&self, i: &Inputs, set: usize) {
        let sets = [self.sets.frames[i.fi][set]];
        let layout = self.pipes.layout;
        i.ctx
            .device
            .cmd_bind_descriptor_sets(i.cmd, GFX, layout, 0, &sets, &[]);
    }
}
