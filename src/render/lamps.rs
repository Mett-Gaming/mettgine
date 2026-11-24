//! Lampenschatten der einfachen Grafik: welche Lampen Schatten werfen und was davon neu zu
//! zeichnen ist. Gezeichnet wird in lamp_pass.rs.
//!
//! Jede Lampe wirft Schatten mit einem weiten Kegel nach unten (ein Feld im Atlas). Was darueber
//! liegt, bleibt hell: gedacht fuer Deckenlampen. Die stehende Welt liegt in einem eigenen Atlas
//! und wird nur neu gezeichnet, wenn sich Lampe, Welt oder Schwanken aendern.

use crate::cull::Sphere;
use crate::gpu::GpuLight;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};

/// So viele Lampen koennen hoechstens Schatten werfen (Platz im Puffer, siehe `_common.wgsl`)
pub(super) const MAX_LAMPS: usize = 16;
/// Felder nebeneinander im Atlas
pub(super) const COLUMNS: u32 = 4;
/// Oeffnung des Kegels (Radiant)
const CONE: f32 = 2.62;
const NEAR: f32 = 0.15;
/// Ueber diese Entfernung (m) blendet der Schatten einer Lampe aus, bevor sie ihr Feld verliert
const FADE: f32 = 2.0;
/// Hoechstens so viele Felder mit stehender Welt werden je Bild neu gezeichnet
const REDRAW: usize = 3;
/// Ab dieser Verschiebung durch das Schwanken (m) wird neu gezeichnet
const SWAY_STEP: f32 = 0.004;
/// So schnell aendert sich das Schwanken hoechstens je Meter Ausschlag (1/s, siehe `sway` in
/// `_common.wgsl`)
const SWAY_SPEED: f32 = 1.0;

/// Schatten von Lampen und Verdunkelung in Ecken, siehe `Renderer::set_lamp_shadows`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LampShadows {
    /// so viele Lampen nahe der Kamera werfen Schatten (0 = aus, hoechstens 16)
    pub count: u32,
    /// Kantenlaenge eines Schattenfelds (Texel)
    pub resolution: u32,
    /// wie dunkel Schatten sind (0..1)
    pub strength: f32,
    /// Verdunkelung in Ecken und an Kanten (0 = aus, 1 = kraeftig); nur ohne Kantenglaettung
    pub contact: f32,
    /// Lampen mit kleinerer Reichweite (m) werfen keinen Schatten
    pub min_range: f32,
}

impl Default for LampShadows {
    /// Alles aus.
    fn default() -> Self {
        Self {
            count: 0,
            resolution: 512,
            strength: 0.85,
            contact: 0.0,
            min_range: 4.0,
        }
    }
}

impl LampShadows {
    pub(super) fn clamped(self) -> Self {
        Self {
            count: self.count.min(MAX_LAMPS as u32),
            resolution: self.resolution.clamp(128, 1024),
            strength: self.strength.clamp(0.0, 1.0),
            contact: self.contact.clamp(0.0, 1.0),
            min_range: self.min_range.max(0.0),
        }
    }
}

/// Ein Schattenfeld, gleich aufgebaut wie `LampTile` in `_common.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuLampTile {
    pub view_proj: [[f32; 4]; 4],
    /// Feld im Atlas (0..1): x, y, Breite, Hoehe
    pub rect: [f32; 4],
    /// x = Nummer der Lampe, y = Staerke, z = Texelgroesse im Feld (0..1)
    pub info: [f32; 4],
}

/// Wie die Welt gerade schwankt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Swaying {
    /// Ausschlag (m) und Lage, wie `Rooms::sway`
    pub sway: Vec4,
    /// Sekunden
    pub time: f32,
}

impl Swaying {
    /// Noch nie gezeichnet oder veraltet.
    const NEVER: Swaying = Swaying {
        sway: Vec4::INFINITY,
        time: 0.0,
    };

    /// So weit kann sich seit `self` etwas verschoben haben (m).
    fn moved(&self, now: &Swaying) -> f32 {
        let reach = now.sway.x.abs() + now.sway.y.abs();
        let by_time = reach * (now.time - self.time).abs() * SWAY_SPEED;
        let off = (self.sway - now.sway).abs().max_element() + by_time;
        if off.is_finite() {
            off
        } else {
            f32::MAX
        }
    }
}

/// Ein Platz im Atlas.
#[derive(Clone, Copy, Debug)]
pub(super) struct Slot {
    pub lamp: usize,
    pub view_proj: Mat4,
    pub sphere: Sphere,
    pub strength: f32,
    /// stehende Welt liegt im Atlas (wenn auch vielleicht leicht veraltet)
    drawn: bool,
    sway: Swaying,
    /// wird in diesem Bild neu gezeichnet
    pub redraw: bool,
}

/// Auswahl ueber die Bilder hinweg.
#[derive(Debug, Default)]
pub(super) struct Lamps {
    slots: Vec<Option<Slot>>,
    /// Welt seit dem letzten Bild geaendert: alle Felder veraltet
    stale: bool,
}

fn pos(l: &GpuLight) -> Vec3 {
    Vec3::new(l.pos[0], l.pos[1], l.pos[2])
}

fn view_proj(l: &GpuLight) -> Mat4 {
    let p = pos(l);
    Mat4::perspective_rh(CONE, 1.0, NEAR, l.pos[3].max(NEAR * 2.0))
        * Mat4::look_at_rh(p, p - Vec3::Y, Vec3::Z)
}

/// Lampen mit Staerke, nahe zuerst. Die Staerke faellt stetig auf 0, bevor eine Lampe aus der
/// Auswahl faellt; so springt beim Gehen nichts.
pub(super) fn choose(lights: &[GpuLight], eye: Vec3, cfg: &LampShadows) -> Vec<(usize, f32)> {
    let mut near: Vec<(f32, usize)> = lights
        .iter()
        .enumerate()
        .filter(|(_, l)| l.color[3] > 0.0 && l.pos[3] >= cfg.min_range)
        .map(|(i, l)| (pos(l).distance(eye), i))
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let n = cfg.count as usize;
    let limit = near.get(n).map_or(f32::INFINITY, |d| d.0);
    near.iter()
        .take(n)
        .map(|&(d, i)| (i, ((limit - d) / FADE).clamp(0.0, 1.0)))
        .collect()
}

impl Lamps {
    /// Neue Lampen oder Boxen: Nummern gelten nicht mehr.
    pub fn world_changed(&mut self) {
        self.slots.clear();
    }

    /// Feste Modelle geaendert: Lampen bleiben, ihre Felder sind veraltet.
    pub fn statics_changed(&mut self) {
        self.stale = true;
    }

    /// Plaetze fuer dieses Bild vergeben.
    pub fn update(&mut self, lights: &[GpuLight], eye: Vec3, now: Swaying, cfg: &LampShadows) {
        let chosen = choose(lights, eye, cfg);
        self.slots.resize(cfg.count as usize, None);
        if std::mem::take(&mut self.stale) {
            for s in self.slots.iter_mut().flatten() {
                // weiter zeigen, aber neu zeichnen
                s.sway = Swaying::NEVER;
            }
        }
        for s in &mut self.slots {
            if s.is_some_and(|s| chosen.iter().all(|c| c.0 != s.lamp)) {
                *s = None;
            }
        }
        for &(lamp, strength) in &chosen {
            if let Some(s) = self.slots.iter_mut().flatten().find(|s| s.lamp == lamp) {
                s.strength = strength;
                continue;
            }
            let l = &lights[lamp];
            if let Some(free) = self.slots.iter_mut().find(|s| s.is_none()) {
                *free = Some(Slot {
                    lamp,
                    view_proj: view_proj(l),
                    sphere: Sphere {
                        center: pos(l),
                        radius: l.pos[3],
                    },
                    strength,
                    drawn: false,
                    sway: Swaying::NEVER,
                    redraw: false,
                });
            }
        }
        // zuerst, was noch nie gezeichnet wurde, dann das am staerksten Veraltete
        let mut order: Vec<(bool, f32, usize)> = Vec::new();
        for (i, s) in self.slots.iter_mut().enumerate() {
            let Some(s) = s else { continue };
            s.redraw = false;
            let off = s.sway.moved(&now);
            if !s.drawn || off > SWAY_STEP {
                order.push((s.drawn, off, i));
            }
        }
        order.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)).then(a.2.cmp(&b.2)));
        // schwankt alles nur ein wenig, genuegt ein Feld je Bild
        let urgent = order.iter().any(|o| !o.0 || o.1 == f32::MAX);
        let budget = if urgent { REDRAW } else { 1 };
        for &(_, _, i) in order.iter().take(budget) {
            if let Some(s) = &mut self.slots[i] {
                (s.redraw, s.drawn, s.sway) = (true, true, now);
            }
        }
    }

    /// Belegte Plaetze, deren Schatten schon im Atlas liegt: (Platz, Inhalt).
    pub fn active(&self) -> impl Iterator<Item = (usize, &Slot)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().filter(|s| s.drawn).map(|s| (i, s)))
    }

    /// Daten fuer den Shader.
    pub fn tiles(&self, cfg: &LampShadows) -> Vec<GpuLampTile> {
        let rows = rows(cfg.count);
        self.active()
            .filter(|(_, s)| s.strength > 0.0)
            .map(|(i, s)| GpuLampTile {
                view_proj: s.view_proj.to_cols_array_2d(),
                rect: [
                    (i as u32 % COLUMNS) as f32 / COLUMNS as f32,
                    (i as u32 / COLUMNS) as f32 / rows as f32,
                    1.0 / COLUMNS as f32,
                    1.0 / rows as f32,
                ],
                info: [
                    s.lamp as f32,
                    s.strength * cfg.strength,
                    1.0 / cfg.resolution as f32,
                    0.0,
                ],
            })
            .collect()
    }
}

/// Zeilen im Atlas.
pub(super) fn rows(count: u32) -> u32 {
    count.div_ceil(COLUMNS).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamp(x: f32, range: f32) -> GpuLight {
        GpuLight {
            pos: [x, 3.0, 0.0, range],
            color: [1.0, 1.0, 1.0, 0.6],
            extra: [0.0; 4],
        }
    }

    fn still(x: f32) -> Swaying {
        Swaying {
            sway: Vec4::splat(x),
            time: 0.0,
        }
    }

    fn cfg(count: u32) -> LampShadows {
        LampShadows {
            count,
            ..Default::default()
        }
    }

    #[test]
    fn strengths_change_smoothly_while_walking() {
        let lights: Vec<_> = (0..40).map(|i| lamp(i as f32 * 1.7, 7.5)).collect();
        let at = |x: f32| {
            let mut v = vec![0.0f32; lights.len()];
            for (i, s) in choose(&lights, Vec3::new(x, 1.6, 0.0), &cfg(6)) {
                v[i] = s;
            }
            v
        };
        let mut before = at(0.0);
        for k in 1..3000 {
            let now = at(k as f32 * 0.02);
            for (a, b) in before.iter().zip(&now) {
                assert!((a - b).abs() <= 0.04, "Schatten springt: {a} -> {b}");
            }
            before = now;
        }
    }

    #[test]
    fn weak_lamps_and_lamps_that_are_off_are_left_out() {
        let mut lights = vec![lamp(0.0, 3.0), lamp(1.0, 7.5), lamp(2.0, 7.5)];
        lights[2].color[3] = 0.0;
        let c = choose(&lights, Vec3::ZERO, &cfg(8));
        assert_eq!(c, vec![(1, 1.0)], "wenige Lampen: volle Staerke");
        assert!(choose(&lights, Vec3::ZERO, &cfg(0)).is_empty());
    }

    #[test]
    fn slots_stay_put_and_only_changed_ones_are_redrawn() {
        let lights: Vec<_> = (0..3).map(|i| lamp(i as f32, 7.5)).collect();
        let mut l = Lamps::default();
        let sway = still(0.0);
        l.update(&lights, Vec3::ZERO, sway, &cfg(4));
        assert_eq!(l.active().filter(|(_, s)| s.redraw).count(), 3);
        let first: Vec<_> = l.active().map(|(i, s)| (i, s.lamp)).collect();
        // Kamera am anderen Ende: andere Reihenfolge, gleiche Plaetze, nichts neu
        l.update(&lights, Vec3::new(9.0, 0.0, 0.0), sway, &cfg(4));
        assert_eq!(
            first,
            l.active().map(|(i, s)| (i, s.lamp)).collect::<Vec<_>>()
        );
        assert!(l.active().all(|(_, s)| !s.redraw));
        l.statics_changed();
        l.update(&lights, Vec3::ZERO, sway, &cfg(4));
        assert_eq!(l.active().filter(|(_, s)| s.redraw).count(), 3);
        // Schwanken: erst ab einer sichtbaren Aenderung
        l.update(&lights, Vec3::ZERO, still(0.001), &cfg(4));
        assert!(l.active().all(|(_, s)| !s.redraw));
        l.update(&lights, Vec3::ZERO, still(0.1), &cfg(4));
        assert!(l.active().any(|(_, s)| s.redraw));
        // auch die Zeit bewegt, aber nur, wenn ueberhaupt etwas schwankt
        let later = |x: f32| Swaying {
            time: 5.0,
            ..still(x)
        };
        assert!(still(0.0).moved(&later(0.0)) == 0.0);
        assert!(still(0.1).moved(&later(0.1)) > SWAY_STEP);
    }

    #[test]
    fn a_changed_world_is_caught_up_within_a_few_frames() {
        let lights: Vec<_> = (0..16).map(|i| lamp(i as f32, 7.5)).collect();
        let mut l = Lamps::default();
        for _ in 0..6 {
            l.update(&lights, Vec3::ZERO, still(0.0), &cfg(16));
            assert!(l.active().filter(|(_, s)| s.redraw).count() <= REDRAW);
        }
        assert_eq!(l.active().count(), 16);
        l.update(&lights, Vec3::ZERO, still(0.0), &cfg(16));
        assert!(l.active().all(|(_, s)| !s.redraw));
        l.world_changed();
        assert_eq!(l.active().count(), 0);
    }

    #[test]
    fn tiles_lie_inside_the_atlas_and_point_down() {
        let lights: Vec<_> = (0..5).map(|i| lamp(i as f32, 7.5)).collect();
        let mut l = Lamps::default();
        for _ in 0..3 {
            l.update(&lights, Vec3::ZERO, still(0.0), &cfg(5));
        }
        let tiles = l.tiles(&cfg(5));
        assert_eq!(tiles.len(), 5);
        for t in &tiles {
            let [x, y, w, h] = t.rect;
            assert!(x + w <= 1.0001 && y + h <= 1.0001);
            let below = pos(&lights[t.info[0] as usize]) - Vec3::Y * 2.0;
            let q = Mat4::from_cols_array_2d(&t.view_proj).project_point3(below);
            assert!(q.x.abs() < 1e-4 && q.y.abs() < 1e-4 && (0.0..1.0).contains(&q.z));
        }
    }
}
