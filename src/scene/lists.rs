//! Was in diesem Bild gezeichnet wird: aussortieren, nach Modell gruppieren.

use super::*;
use crate::cull::{Frustum, SkinBounds, Sphere};

/// Sichtbarkeit in Kamera und Schattenkarte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Visible {
    pub camera: bool,
    pub shadow: bool,
}

impl Visible {
    /// Sortierstufe: erst nur Schatten, dann beides, dann nur Kamera. So bleiben die Instanzen
    /// beider Durchgaenge je Modell zusammenhaengend. `None` = nirgends zu sehen.
    fn class(self) -> Option<u64> {
        match (self.camera, self.shadow) {
            (false, true) => Some(0),
            (true, true) => Some(1),
            (true, false) => Some(2),
            (false, false) => None,
        }
    }
}

/// Wiederverwendete Listen eines Bildes.
#[derive(Default)]
pub(super) struct DrawLists {
    /// (Modell und Sortierstufe, Index in den uebergebenen Modellen)
    pub order: Vec<(u64, u32)>,
    pub groups: Vec<Group>,
    pub shadow_groups: Vec<Group>,
    /// je Figur
    pub skins: Vec<Visible>,
    pub drawn: u32,
    pub culled: u32,
    pub dropped: u32,
}

fn extend(gs: &mut Vec<Group>, mesh: u32, slot: u32) {
    match gs.last_mut() {
        Some(g) if g.mesh == mesh && g.first_inst + g.count == slot => g.count += 1,
        _ => gs.push(Group {
            mesh,
            first_inst: slot,
            count: 1,
        }),
    }
}

impl DrawLists {
    /// Modelle sortieren und gruppieren. Unbekannte Modelle fallen weg, `visible` entscheidet,
    /// was zu sehen ist, hoechstens `max` bleiben uebrig.
    pub fn build_meshes(
        &mut self,
        meshes: &[MeshDraw],
        mesh_count: usize,
        max: usize,
        visible: impl Fn(&MeshDraw) -> Visible,
    ) {
        self.order.clear();
        self.groups.clear();
        self.shadow_groups.clear();
        (self.drawn, self.culled, self.dropped) = (0, 0, 0);
        for (i, m) in meshes.iter().enumerate() {
            if m.mesh >= mesh_count {
                continue;
            }
            let mut v = visible(m);
            v.shadow &= m.shadow;
            self.count(v);
            if let Some(class) = v.class() {
                self.order.push(((m.mesh as u64) << 2 | class, i as u32));
            }
        }
        self.order.sort_unstable();
        self.dropped += self.order.len().saturating_sub(max) as u32;
        self.order.truncate(max);
        for (slot, &(key, _)) in self.order.iter().enumerate() {
            let (mesh, class, slot) = ((key >> 2) as u32, key & 3, slot as u32);
            if class > 0 {
                extend(&mut self.groups, mesh, slot);
            }
            if class < 2 {
                extend(&mut self.shadow_groups, mesh, slot);
            }
        }
    }

    /// Figuren bewerten (Reihenfolge bleibt); hoechstens `max`.
    pub fn build_skins(
        &mut self,
        skins: &[SkinDraw],
        max: usize,
        visible: impl Fn(&SkinDraw) -> Visible,
    ) {
        self.skins.clear();
        let n = skins.len().min(max);
        self.dropped += (skins.len() - n) as u32;
        for s in &skins[..n] {
            let v = visible(s);
            self.count(v);
            self.skins.push(v);
        }
    }

    fn count(&mut self, v: Visible) {
        if v.camera {
            self.drawn += 1;
        } else {
            self.culled += 1;
        }
    }
}

/// Sichtfelder von Kamera und Sonne eines Bildes.
pub(super) struct Views {
    /// None = alles gilt als sichtbar (Aussortieren aus)
    camera: Option<Frustum>,
    sun: Option<Frustum>,
    shadows: bool,
}

impl Views {
    pub fn new(p: &SceneParams, culling: bool) -> Self {
        let sun = || light_view_proj(p.sun_dir, p.shadow_center, p.shadow_radius);
        Self {
            camera: culling.then(|| Frustum::new(&p.view_proj)),
            sun: culling.then(|| Frustum::new(&sun())),
            shadows: p.shadow_strength > 0.0,
        }
    }

    /// `None` = Ausdehnung unbekannt, also sichtbar.
    pub fn sees(&self, s: Option<Sphere>) -> Visible {
        let in_view = |f: &Option<Frustum>| match (f, &s) {
            (Some(f), Some(s)) => f.sees(s),
            _ => true,
        };
        Visible {
            camera: in_view(&self.camera),
            shadow: self.shadows && in_view(&self.sun),
        }
    }
}

/// Huell-Kugel einer Figur in ihrer Pose.
pub(super) fn skin_sphere(s: &SkinDraw, bounds: &[SkinBounds], bones: &[Mat4]) -> Option<Sphere> {
    if bounds.is_empty() {
        return None;
    }
    let pose = bones.get(s.first_bone as usize..)?;
    bounds[s.skin % bounds.len()].posed(&s.model, pose)
}

/// Gruppierung ohne Aussortieren (Tests).
#[cfg(test)]
pub(super) fn build_groups(
    meshes: &[MeshDraw],
    mesh_count: usize,
    max: usize,
) -> (Vec<usize>, Vec<Group>, Vec<Group>) {
    let mut l = DrawLists::default();
    l.build_meshes(meshes, mesh_count, max, |_| Visible {
        camera: true,
        shadow: true,
    });
    let order = l.order.iter().map(|o| o.1 as usize).collect();
    (order, l.groups, l.shadow_groups)
}
