//! Was in diesem Bild gezeichnet wird: aussortieren, nach Modell gruppieren.

use super::*;
use crate::cull::{Frustum, SkinBounds, Sphere};

/// Wie weit das Zerreissen im Shader (`skin.wgsl`) einen Vertex hoechstens verschiebt (Meter)
const DISTORT_REACH: f32 = 0.3;

/// Sichtbarkeit je Durchgang.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Visible {
    pub camera: bool,
    pub shadow: bool,
    /// zweite Kamera
    pub second: bool,
}

impl Visible {
    /// Sortierstufe: so geordnet, dass die Instanzen jedes Durchgangs je Modell in hoechstens
    /// zwei zusammenhaengenden Stuecken liegen. `None` = nirgends zu sehen.
    fn class(self) -> Option<u64> {
        Some(match (self.camera, self.shadow, self.second) {
            (false, true, false) => 0,
            (true, true, false) => 1,
            (true, false, false) => 2,
            (true, false, true) => 3,
            (true, true, true) => 4,
            (false, true, true) => 5,
            (false, false, true) => 6,
            (false, false, false) => return None,
        })
    }
}

/// Wie eine Figur gezeichnet wird.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SkinKind {
    Solid,
    Ghost,
    /// nur fuer die zweite Kamera
    SecondOnly,
}

/// Eine Figur in Instanz-Reihenfolge.
#[derive(Clone, Copy, Debug)]
pub(super) struct SkinSlot {
    pub draw: SkinDraw,
    pub kind: SkinKind,
    pub vis: Visible,
}

/// Wiederverwendete Listen eines Bildes.
#[derive(Default)]
pub(super) struct DrawLists {
    /// (Modell und Sortierstufe, Index in den uebergebenen Modellen)
    pub order: Vec<(u64, u32)>,
    pub groups: Vec<Group>,
    pub shadow_groups: Vec<Group>,
    pub second_groups: Vec<Group>,
    /// sichtbare Hologramme in ihrer Reihenfolge
    pub ghosts: Vec<u32>,
    pub ghost_groups: Vec<Group>,
    /// feste Figuren, dann Hologramme, dann die der zweiten Kamera
    pub skins: Vec<SkinSlot>,
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
        self.second_groups.clear();
        (self.drawn, self.culled, self.dropped) = (0, 0, 0);
        for (i, m) in meshes.iter().enumerate() {
            if m.mesh >= mesh_count {
                continue;
            }
            let mut v = visible(m);
            v.shadow &= m.shadow;
            self.count(v);
            if let Some(class) = v.class() {
                self.order.push(((m.mesh as u64) << 3 | class, i as u32));
            }
        }
        self.order.sort_unstable();
        self.dropped += self.order.len().saturating_sub(max) as u32;
        self.order.truncate(max);
        for (slot, &(key, _)) in self.order.iter().enumerate() {
            let (mesh, class, slot) = ((key >> 3) as u32, key & 7, slot as u32);
            if (1..=4).contains(&class) {
                extend(&mut self.groups, mesh, slot);
            }
            if matches!(class, 0 | 1 | 4 | 5) {
                extend(&mut self.shadow_groups, mesh, slot);
            }
            if class >= 3 {
                extend(&mut self.second_groups, mesh, slot);
            }
        }
    }

    /// Hologramme: Reihenfolge bleibt, Instanzen hinter den Modellen.
    pub fn build_ghosts(
        &mut self,
        ghosts: &[MeshDraw],
        mesh_count: usize,
        max: usize,
        visible: impl Fn(&MeshDraw) -> Visible,
    ) {
        self.ghosts.clear();
        self.ghost_groups.clear();
        let base = self.order.len() as u32;
        for (i, m) in ghosts.iter().enumerate() {
            if m.mesh >= mesh_count {
                continue;
            }
            let v = visible(m);
            self.count(v);
            if !v.camera {
                continue;
            }
            if self.ghosts.len() >= max {
                self.dropped += 1;
                continue;
            }
            let slot = base + self.ghosts.len() as u32;
            extend(&mut self.ghost_groups, m.mesh as u32, slot);
            self.ghosts.push(i as u32);
        }
    }

    /// Figuren bewerten; hoechstens `max`.
    pub fn build_skins(
        &mut self,
        skins: &[SkinDraw],
        second_only: &[SkinDraw],
        max: usize,
        visible: impl Fn(&SkinDraw) -> Visible,
    ) {
        self.skins.clear();
        let solid = skins
            .iter()
            .filter(|s| !s.ghost)
            .map(|s| (s, SkinKind::Solid));
        let ghost = skins
            .iter()
            .filter(|s| s.ghost)
            .map(|s| (s, SkinKind::Ghost));
        let extra = second_only.iter().map(|s| (s, SkinKind::SecondOnly));
        for (s, kind) in solid.chain(ghost).chain(extra) {
            let mut vis = visible(s);
            match kind {
                SkinKind::Solid => {}
                SkinKind::Ghost => (vis.shadow, vis.second) = (false, false),
                SkinKind::SecondOnly => (vis.camera, vis.shadow) = (false, false),
            }
            if kind != SkinKind::SecondOnly {
                self.count(vis);
            }
            if self.skins.len() < max {
                self.skins.push(SkinSlot {
                    draw: *s,
                    kind,
                    vis,
                });
            } else {
                self.dropped += 1;
            }
        }
    }

    fn count(&mut self, v: Visible) {
        if v.camera {
            self.drawn += 1;
        } else {
            self.culled += 1;
        }
    }

    /// Zeichenbefehle aller Durchgaenge fuer Modelle und Figuren.
    pub fn draw_calls(&self) -> u32 {
        let groups = self.groups.len()
            + self.shadow_groups.len()
            + self.second_groups.len()
            + self.ghost_groups.len();
        let skins: u32 = self
            .skins
            .iter()
            .map(|s| s.vis.camera as u32 + s.vis.shadow as u32 + s.vis.second as u32)
            .sum();
        groups as u32 + skins
    }
}

/// Sichtfelder eines Bildes.
pub(super) struct Views {
    culling: bool,
    camera: Frustum,
    /// None = keine Schatten
    sun: Option<Frustum>,
    /// None = keine zweite Kamera
    second: Option<Frustum>,
}

impl Views {
    pub fn new(f: &FrameInput, second_camera: bool, culling: bool) -> Self {
        let sun =
            match &f.lighting {
                Lighting::Sun(s) if s.shadow_strength > 0.0 => Some(Frustum::new(
                    &light_view_proj(s.dir, s.shadow_center, s.shadow_radius),
                )),
                _ => None,
            };
        let second = f.second_camera.filter(|_| second_camera);
        Self {
            culling,
            camera: Frustum::new(&f.view.view_proj),
            sun,
            second: second.map(|v| Frustum::new(&v.view_proj)),
        }
    }

    /// Sichtfelder von Kamera, zweiter Kamera und Sonne; `None` = nicht aussortieren.
    pub fn frustums(&self) -> [Option<&Frustum>; 3] {
        if !self.culling {
            return [None; 3];
        }
        [Some(&self.camera), self.second.as_ref(), self.sun.as_ref()]
    }

    /// `None` = Ausdehnung unbekannt, also sichtbar.
    pub fn sees(&self, s: Option<Sphere>) -> Visible {
        let seen = |f: &Frustum| !self.culling || s.is_none_or(|s| f.sees(&s));
        Visible {
            camera: seen(&self.camera),
            shadow: self.sun.as_ref().is_some_and(seen),
            second: self.second.as_ref().is_some_and(seen),
        }
    }
}

/// Huell-Kugel einer Figur in ihrer Pose.
pub(super) fn skin_sphere(s: &SkinDraw, bounds: &[SkinBounds], bones: &[Mat4]) -> Option<Sphere> {
    if bounds.is_empty() {
        return None;
    }
    let pose = bones.get(s.first_bone as usize..)?;
    let mut sphere = bounds[s.skin % bounds.len()].posed(&s.model, pose)?;
    sphere.radius += s.distort.abs() * DISTORT_REACH;
    Some(sphere)
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
        second: false,
    });
    let order = l.order.iter().map(|o| o.1 as usize).collect();
    (order, l.groups, l.shadow_groups)
}
