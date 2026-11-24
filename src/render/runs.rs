//! Feste Objekte aussortieren: Instanzen liegen unveraendert im Puffer, gezeichnet werden nur die
//! Stuecke, deren Huellkugel im Sichtfeld liegt. Die Reihenfolge bleibt, das Bild also auch.

use super::Group;
use crate::cull::{Frustum, Sphere};
use crate::gpu::GpuInstance;
use glam::Vec3;

/// So viele Instanzen teilen sich eine Huellkugel
const RUN: usize = 32;

/// Aufeinanderfolgende Instanzen desselben Modells.
#[derive(Clone, Copy, Debug)]
struct Run {
    group: Group,
    sphere: Sphere,
}

/// Alle Stuecke.
#[derive(Clone, Debug, Default)]
pub(super) struct Runs {
    list: Vec<Run>,
}

impl Runs {
    /// Aus (Modell, Huellkugel) je Instanz, in Puffer-Reihenfolge.
    pub fn new(items: impl Iterator<Item = (u32, Sphere)>) -> Self {
        let mut list: Vec<Run> = Vec::new();
        for (i, (mesh, sphere)) in items.enumerate() {
            match list.last_mut() {
                Some(r) if r.group.mesh == mesh && (r.group.count as usize) < RUN => {
                    r.group.count += 1;
                    r.sphere = r.sphere.union(&sphere);
                }
                _ => list.push(Run {
                    group: Group {
                        mesh,
                        first_inst: i as u32,
                        count: 1,
                    },
                    sphere,
                }),
            }
        }
        Self { list }
    }

    /// Welt-Boxen.
    pub fn of_boxes(boxes: &[GpuInstance]) -> Self {
        Self::new(boxes.iter().map(|b| {
            let [x, y, z, _] = b.center;
            let radius = Vec3::new(b.half[0], b.half[1], b.half[2]).length();
            let center = Vec3::new(x, y, z);
            (0, Sphere { center, radius })
        }))
    }

    /// Sichtbare Stuecke nach `out`; benachbarte werden zu einem Zeichenbefehl. `grow` = so weit
    /// kann sich eine Instanz bewegen (Schwanken), `None` als Sichtfeld = alles.
    pub fn visible(&self, frustum: Option<&Frustum>, grow: f32, out: &mut Vec<Group>) {
        out.clear();
        for r in &self.list {
            let mut s = r.sphere;
            s.radius += grow;
            if frustum.is_some_and(|f| !f.sees(&s)) {
                continue;
            }
            match out.last_mut() {
                Some(g)
                    if g.mesh == r.group.mesh && g.first_inst + g.count == r.group.first_inst =>
                {
                    g.count += r.group.count;
                }
                _ => out.push(r.group),
            }
        }
    }
}

/// Sichtbare Stuecke je Durchgang (Kamera, zweite Kamera, Schatten).
#[derive(Clone, Debug, Default)]
pub(super) struct VisibleRuns {
    pub statics: [Vec<Group>; 3],
    pub boxes: [Vec<Group>; 3],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::perspective;

    fn sphere(x: f32, z: f32) -> Sphere {
        Sphere {
            center: Vec3::new(x, 0.0, z),
            radius: 0.5,
        }
    }

    #[test]
    fn hidden_runs_are_skipped_and_neighbours_merge_in_order() {
        let mut items: Vec<(u32, Sphere)> = (0..100).map(|_| (0, sphere(0.0, -10.0))).collect();
        items.extend((0..40).map(|_| (0, sphere(0.0, 50.0))));
        items.extend((0..5).map(|_| (1, sphere(0.0, -10.0))));
        let runs = Runs::new(items.into_iter());
        let f = Frustum::new(&perspective(1.0, 1.0, 0.1, 100.0));
        let mut out = Vec::new();
        runs.visible(None, 0.0, &mut out);
        let all: Vec<_> = out
            .iter()
            .map(|g| (g.mesh, g.first_inst, g.count))
            .collect();
        assert_eq!(
            all,
            [(0, 0, 140), (1, 140, 5)],
            "ohne Aussortieren wie frueher"
        );
        runs.visible(Some(&f), 0.0, &mut out);
        let seen: u32 = out.iter().map(|g| g.count).sum();
        assert!((105..=137).contains(&seen), "{out:?}");
        assert!(out
            .windows(2)
            .all(|w| w[0].first_inst + w[0].count <= w[1].first_inst));
        runs.visible(Some(&f), 200.0, &mut out);
        assert_eq!(out.iter().map(|g| g.count).sum::<u32>(), 145);
    }
}
