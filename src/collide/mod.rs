//! Kollision ohne Physik-Bibliothek: feste Kisten, stehende Kapseln (Figuren), Strahlen.
//!
//! - [`World`]: feste Kisten mit Raster, damit nur die Umgebung geprueft wird.
//! - [`World::raycast`], [`World::line_of_sight`]: Strahlen und Sichtlinien.
//! - [`World::move_capsule`]: Figur bewegen, an Waenden entlanggleiten, Stufen steigen.
//!
//! Einheiten: Meter. Hochachse ist y. Keine Kraefte: Schwerkraft und Tempo rechnet das Spiel.

mod shape;
#[cfg(test)]
mod tests;

pub use shape::{ray_sphere, Aabb, Capsule, Hit, Ray, Solid};

use glam::Vec3;
use std::cell::RefCell;
use std::collections::HashMap;

/// Ab dieser senkrechten Normale gilt eine Flaeche als Boden (etwa 45 Grad)
const GROUND_NORMAL: f32 = 0.7;
/// Durchgaenge, um die Figur aus allen Kisten zu schieben
const PUSH_STEPS: usize = 6;
/// Hoechstzahl Teilschritte einer Bewegung
const MAX_SUBSTEPS: f32 = 64.0;
/// Kisten ueber mehr Rasterfelder als das werden immer geprueft statt eingetragen
const MAX_CELLS: i64 = 1024;
/// Spalt, der als Bodenkontakt zaehlt (Meter)
const GROUND_GAP: f32 = 0.02;

/// Kennung einer Kiste in der Welt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SolidId(usize);

/// Ergebnis einer Bewegung.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moved {
    /// neuer Fusspunkt
    pub foot: Vec3,
    /// steht auf etwas
    pub grounded: bool,
    /// an eine Wand gestossen (Normale zur Figur hin)
    pub wall: Option<Vec3>,
    /// an die Decke gestossen
    pub ceiling: bool,
}

type Cell = (i32, i32);

/// Feste Kisten der Spielwelt. Das Raster liegt in der x/z-Ebene.
#[derive(Debug)]
pub struct World {
    cell: f32,
    solids: Vec<Option<Solid>>,
    free: Vec<usize>,
    grid: HashMap<Cell, Vec<usize>>,
    /// sehr grosse Kisten (Boden), immer geprueft
    big: Vec<usize>,
    scratch: RefCell<Vec<usize>>,
}

impl Default for World {
    fn default() -> Self {
        Self::new(4.0)
    }
}

impl World {
    /// `cell` = Kantenlaenge eines Rasterfelds (etwa die uebliche Kistengroesse).
    pub fn new(cell: f32) -> Self {
        Self {
            cell: cell.max(0.25),
            solids: Vec::new(),
            free: Vec::new(),
            grid: HashMap::new(),
            big: Vec::new(),
            scratch: RefCell::new(Vec::new()),
        }
    }

    fn cells(&self, b: &Aabb) -> (Cell, Cell) {
        let f = |v: f32| (v / self.cell).floor() as i32;
        ((f(b.min.x), f(b.min.z)), (f(b.max.x), f(b.max.z)))
    }

    fn is_big(&self, b: &Aabb) -> bool {
        let (a, z) = self.cells(b);
        (z.0 as i64 - a.0 as i64 + 1) * (z.1 as i64 - a.1 as i64 + 1) > MAX_CELLS
    }

    /// Kiste hinzufuegen.
    pub fn add(&mut self, s: Solid) -> SolidId {
        let i = self.free.pop().unwrap_or_else(|| {
            self.solids.push(None);
            self.solids.len() - 1
        });
        self.solids[i] = Some(s);
        let b = s.aabb();
        if self.is_big(&b) {
            self.big.push(i);
        } else {
            let (a, z) = self.cells(&b);
            for x in a.0..=z.0 {
                for y in a.1..=z.1 {
                    self.grid.entry((x, y)).or_default().push(i);
                }
            }
        }
        SolidId(i)
    }

    /// Kiste entfernen; liefert sie zurueck, falls es sie gab.
    pub fn remove(&mut self, id: SolidId) -> Option<Solid> {
        let s = self.solids.get_mut(id.0)?.take()?;
        let b = s.aabb();
        if self.is_big(&b) {
            self.big.retain(|i| *i != id.0);
        } else {
            let (a, z) = self.cells(&b);
            for x in a.0..=z.0 {
                for y in a.1..=z.1 {
                    if let Some(v) = self.grid.get_mut(&(x, y)) {
                        v.retain(|i| *i != id.0);
                    }
                }
            }
        }
        self.free.push(id.0);
        Some(s)
    }

    pub fn get(&self, id: SolidId) -> Option<&Solid> {
        self.solids.get(id.0)?.as_ref()
    }

    /// Anzahl Kisten.
    pub fn len(&self) -> usize {
        self.solids.len() - self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Alle Kisten, deren Huelle `b` beruehrt (jede hoechstens einmal).
    pub fn for_each_in(&self, b: &Aabb, mut f: impl FnMut(SolidId, &Solid)) {
        let mut found = self.scratch.take();
        found.clear();
        found.extend_from_slice(&self.big);
        let (a, z) = self.cells(b);
        for x in a.0..=z.0 {
            for y in a.1..=z.1 {
                if let Some(v) = self.grid.get(&(x, y)) {
                    found.extend_from_slice(v);
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        for &i in &found {
            if let Some(s) = &self.solids[i] {
                if s.aabb().overlaps(b) {
                    f(SolidId(i), s);
                }
            }
        }
        self.scratch.replace(found);
    }

    /// Naechster Treffer bis `max_t` Meter. Laeuft die Rasterfelder entlang des Strahls ab.
    pub fn raycast(&self, r: &Ray, max_t: f32) -> Option<(SolidId, Hit)> {
        if r.dir == Vec3::ZERO || max_t <= 0.0 {
            return None;
        }
        let mut best: Option<(SolidId, Hit)> = None;
        let check = |i: usize, best: &mut Option<(SolidId, Hit)>| {
            let limit = best.map_or(max_t, |b| b.1.t);
            if let Some(h) = self.solids[i].as_ref().and_then(|s| s.raycast(r, limit)) {
                if best.is_none_or(|b| h.t < b.1.t) {
                    *best = Some((SolidId(i), h));
                }
            }
        };
        for &i in &self.big {
            check(i, &mut best);
        }
        let axis = |o: f32, d: f32| {
            let c = (o / self.cell).floor();
            let step = if d > 0.0 { 1 } else { -1 };
            if d.abs() < 1e-9 {
                return (c as i32, 0, f32::INFINITY, f32::INFINITY);
            }
            let edge = (c + if d > 0.0 { 1.0 } else { 0.0 }) * self.cell;
            (c as i32, step, (edge - o) / d, self.cell / d.abs())
        };
        let (mut x, sx, mut tx, dx) = axis(r.origin.x, r.dir.x);
        let (mut z, sz, mut tz, dz) = axis(r.origin.z, r.dir.z);
        let mut entered = 0.0;
        while entered <= best.map_or(max_t, |b| b.1.t) {
            for &i in self.grid.get(&(x, z)).map_or(&[][..], |v| v) {
                check(i, &mut best);
            }
            if tx.is_infinite() && tz.is_infinite() {
                break; // senkrecht: nur dieses Feld
            }
            if tx < tz {
                entered = tx;
                x += sx;
                tx += dx;
            } else {
                entered = tz;
                z += sz;
                tz += dz;
            }
        }
        best
    }

    /// Freie Sicht zwischen zwei Punkten?
    pub fn line_of_sight(&self, a: Vec3, b: Vec3) -> bool {
        self.raycast(&Ray::new(a, b - a), a.distance(b)).is_none()
    }

    /// Beruehrt die Kapsel eine Kiste?
    pub fn blocked(&self, c: &Capsule) -> bool {
        let mut hit = false;
        self.for_each_in(&c.aabb(), |_, s| hit |= c.penetration(s).is_some());
        hit
    }

    /// Schiebt die Kapsel aus allen Kisten; merkt sich, woran sie stoesst.
    fn push_out(&self, c: &mut Capsule, m: &mut Moved) {
        for _ in 0..PUSH_STEPS {
            let mut deepest: Option<(Vec3, f32)> = None;
            self.for_each_in(&c.aabb(), |_, s| {
                if let Some((n, d)) = c.penetration(s) {
                    if d > 1e-5 && deepest.is_none_or(|b| d > b.1) {
                        deepest = Some((n, d));
                    }
                }
            });
            let Some((n, d)) = deepest else {
                return;
            };
            c.foot += n * d;
            if n.y > GROUND_NORMAL {
                m.grounded = true;
            } else if n.y < -GROUND_NORMAL {
                m.ceiling = true;
            } else {
                m.wall = Some(n);
            }
        }
    }

    fn slide(&self, c: &Capsule, delta: Vec3) -> Moved {
        let mut c = *c;
        let mut m = Moved {
            foot: c.foot,
            grounded: false,
            wall: None,
            ceiling: false,
        };
        // Teilschritte kuerzer als der Radius: duenne Waende werden nicht uebersprungen
        let steps = (delta.length() / (c.radius * 0.5).max(0.01)).ceil();
        let steps = steps.clamp(1.0, MAX_SUBSTEPS);
        for _ in 0..steps as usize {
            c.foot += delta / steps;
            self.push_out(&mut c, &mut m);
        }
        m.foot = c.foot;
        m
    }

    /// Bewegt eine Figur um `delta`. Gleitet an Waenden entlang und steigt ohne Abbremsen auf
    /// Hindernisse bis `step` Meter Hoehe (0 = keine Stufen).
    pub fn move_capsule(&self, c: &Capsule, delta: Vec3, step: f32) -> Moved {
        let direct = self.slide(c, delta);
        let flat = Vec3::new(delta.x, 0.0, delta.z);
        let mut out = direct;
        if step > 0.0 && direct.wall.is_some() && flat.length_squared() > 0.0 && delta.y <= 0.0 {
            if let Some(up) = self.try_step(c, flat, step) {
                let along = flat.normalize();
                let gain = (up.foot - direct.foot).dot(along);
                if gain > 1e-3 {
                    out = up;
                }
            }
        }
        if !out.grounded {
            let probe = Capsule {
                foot: out.foot - Vec3::Y * GROUND_GAP,
                ..*c
            };
            self.for_each_in(&probe.aabb(), |_, s| {
                if probe
                    .penetration(s)
                    .is_some_and(|(n, _)| n.y > GROUND_NORMAL)
                {
                    out.grounded = true;
                }
            });
        }
        out
    }

    /// Hochheben, vorwaerts gehen, absetzen; nur gueltig, wenn hinter der Kante Boden ist.
    /// Die runde Unterseite laege sonst schraeg auf der Kante, darum zaehlt die Hoehe, die die
    /// Figur einen Radius weiter vorn haette.
    fn try_step(&self, c: &Capsule, flat: Vec3, step: f32) -> Option<Moved> {
        let raised = Capsule {
            foot: c.foot + Vec3::Y * step,
            ..*c
        };
        if self.blocked(&raised) {
            return None;
        }
        let fwd = self.slide(&raised, flat);
        let ahead = self.slide(&raised, flat + flat.normalize() * c.radius);
        let probe = Capsule {
            foot: ahead.foot,
            ..*c
        };
        let down = self.slide(&probe, Vec3::NEG_Y * (step + GROUND_GAP));
        if !down.grounded || down.foot.y < c.foot.y + GROUND_GAP {
            return None;
        }
        Some(Moved {
            foot: Vec3::new(fwd.foot.x, down.foot.y, fwd.foot.z),
            grounded: true,
            wall: fwd.wall,
            ceiling: false,
        })
    }
}
