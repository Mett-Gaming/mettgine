//! Wegfindung auf einem Raster: Wege (A*), Abstandsfelder (viele Figuren, ein Ziel) und Glaetten.
//! Waagerechte Ebene x/z, eine Etage. Welche Zellen begehbar sind, sagt das Spiel.

use glam::{Vec2, Vec3};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Kosten eines geraden Schritts; schraeg kostet 14 (etwa Wurzel 2)
const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 14;
/// Zelle nicht begehbar
pub const BLOCKED: u8 = 0;

/// Eine Zelle (Spalte, Zeile).
pub type Cell = (usize, usize);

/// Begehbares Raster.
#[derive(Clone, Debug)]
pub struct Grid {
    origin: Vec2,
    cell: f32,
    w: usize,
    h: usize,
    /// 0 = gesperrt, 1 = normal, groesser = teurer
    cost: Vec<u8>,
}

impl Grid {
    /// Raster ab der Ecke `origin` (x, z), `cell` Meter je Zelle, zunaechst alles frei.
    pub fn new(origin: Vec2, cell: f32, w: usize, h: usize) -> Self {
        Self {
            origin,
            cell: cell.max(0.01),
            w,
            h,
            cost: vec![1; w * h],
        }
    }

    /// Raster fuer den Bereich `min`..`max`; `walkable` entscheidet je Zellmitte.
    pub fn build(min: Vec2, max: Vec2, cell: f32, walkable: impl Fn(Vec3) -> bool) -> Self {
        let size = ((max - min) / cell).ceil().max(Vec2::ONE);
        let mut g = Self::new(min, cell, size.x as usize, size.y as usize);
        for z in 0..g.h {
            for x in 0..g.w {
                if !walkable(g.center((x, z))) {
                    g.set_cost((x, z), BLOCKED);
                }
            }
        }
        g
    }

    pub fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }

    /// Zelle, in der ein Punkt liegt.
    pub fn cell_at(&self, p: Vec3) -> Option<Cell> {
        let q = (Vec2::new(p.x, p.z) - self.origin) / self.cell;
        if q.x < 0.0 || q.y < 0.0 {
            return None;
        }
        let c = (q.x as usize, q.y as usize);
        (c.0 < self.w && c.1 < self.h).then_some(c)
    }

    /// Mitte einer Zelle (y = 0).
    pub fn center(&self, c: Cell) -> Vec3 {
        let x = self.origin.x + (c.0 as f32 + 0.5) * self.cell;
        let z = self.origin.y + (c.1 as f32 + 0.5) * self.cell;
        Vec3::new(x, 0.0, z)
    }

    /// [`BLOCKED`] = gesperrt, 1 = normal, mehr = wird gemieden.
    pub fn set_cost(&mut self, c: Cell, cost: u8) {
        if c.0 < self.w && c.1 < self.h {
            self.cost[c.1 * self.w + c.0] = cost;
        }
    }

    pub fn free(&self, c: Cell) -> bool {
        c.0 < self.w && c.1 < self.h && self.cost[c.1 * self.w + c.0] != BLOCKED
    }

    /// Kisten sperren, um `margin` (Koerperradius) vergroessert.
    pub fn block_rect(&mut self, min: Vec2, max: Vec2, margin: f32) {
        let a = ((min - margin - self.origin) / self.cell).floor().max(Vec2::ZERO);
        let b = ((max + margin - self.origin) / self.cell).ceil().max(Vec2::ZERO);
        for z in a.y as usize..(b.y as usize).min(self.h) {
            for x in a.x as usize..(b.x as usize).min(self.w) {
                self.set_cost((x, z), BLOCKED);
            }
        }
    }

    /// Begehbare Nachbarn mit Schrittkosten. Schraeg nur, wenn beide Ecken frei sind.
    fn neighbors(&self, i: usize) -> impl Iterator<Item = (usize, u32)> + '_ {
        let (x, z) = ((i % self.w) as i32, (i / self.w) as i32);
        const DIRS: [(i32, i32); 8] = [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ];
        let free = move |x: i32, z: i32| x >= 0 && z >= 0 && self.free((x as usize, z as usize));
        DIRS.into_iter().filter_map(move |(dx, dz)| {
            let (nx, nz) = (x + dx, z + dz);
            if !free(nx, nz) {
                return None;
            }
            let diagonal = dx != 0 && dz != 0;
            if diagonal && !(free(x + dx, z) && free(x, z + dz)) {
                return None;
            }
            let n = nz as usize * self.w + nx as usize;
            let step = if diagonal { DIAGONAL } else { STRAIGHT };
            Some((n, step * self.cost[n] as u32))
        })
    }

    /// Naechste freie Zelle um `c` herum (hoechstens `reach` Zellen entfernt).
    pub fn nearest_free(&self, c: Cell, reach: usize) -> Option<Cell> {
        for r in 0..=reach as i32 {
            let mut best: Option<(i32, Cell)> = None;
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dz.abs()) != r {
                        continue;
                    }
                    let (x, z) = (c.0 as i32 + dx, c.1 as i32 + dz);
                    if x < 0 || z < 0 || !self.free((x as usize, z as usize)) {
                        continue;
                    }
                    let d = dx * dx + dz * dz;
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, (x as usize, z as usize)));
                    }
                }
            }
            if let Some((_, c)) = best {
                return Some(c);
            }
        }
        None
    }

    /// Kuerzester Weg von `from` nach `to`, geglaettet, ohne den Startpunkt. Start und Ziel
    /// duerfen knapp neben dem Begehbaren liegen. `None` = kein Weg.
    pub fn path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let start = self.nearest_free(self.cell_at(from)?, 3)?;
        let goal = self.nearest_free(self.cell_at(to)?, 3)?;
        let cells = self.search(start, goal)?;
        let mut points: Vec<Vec3> = cells.iter().map(|&c| self.center(c)).collect();
        if self.cell_at(to) == Some(goal) {
            if let Some(l) = points.last_mut() {
                *l = Vec3::new(to.x, 0.0, to.z);
            }
        }
        if self.cell_at(from) == Some(start) {
            points[0] = Vec3::new(from.x, 0.0, from.z);
        }
        let mut smooth = self.smooth(&points);
        if smooth.len() > 1 {
            smooth.remove(0);
        }
        Some(smooth)
    }

    /// A* von Zelle zu Zelle.
    fn search(&self, start: Cell, goal: Cell) -> Option<Vec<Cell>> {
        let id = |c: Cell| c.1 * self.w + c.0;
        let (s, g) = (id(start), id(goal));
        let guess = |i: usize| {
            let dx = (i % self.w).abs_diff(goal.0) as u32;
            let dz = (i / self.w).abs_diff(goal.1) as u32;
            STRAIGHT * dx.max(dz) + (DIAGONAL - STRAIGHT) * dx.min(dz)
        };
        let mut best = vec![u32::MAX; self.w * self.h];
        let mut from = vec![u32::MAX; self.w * self.h];
        let mut open = BinaryHeap::new();
        best[s] = 0;
        open.push(Reverse((guess(s), s)));
        while let Some(Reverse((f, i))) = open.pop() {
            if i == g {
                let mut cells = vec![goal];
                let mut at = g;
                while at != s {
                    at = from[at] as usize;
                    cells.push((at % self.w, at / self.w));
                }
                cells.reverse();
                return Some(cells);
            }
            if f > best[i].saturating_add(guess(i)) {
                continue; // veraltet
            }
            for (n, step) in self.neighbors(i) {
                let d = best[i] + step;
                if d < best[n] {
                    best[n] = d;
                    from[n] = i as u32;
                    open.push(Reverse((d + guess(n), n)));
                }
            }
        }
        None
    }

    /// Kommt man auf gerader Linie von `a` nach `b`, ohne Gesperrtes oder Teureres zu betreten?
    pub fn line_free(&self, a: Vec3, b: Vec3) -> bool {
        let d = Vec2::new(b.x - a.x, b.z - a.z);
        let steps = (d.length() / (self.cell * 0.25)).ceil().max(1.0) as usize;
        let side = Vec2::new(-d.y, d.x).normalize_or_zero() * self.cell * 0.3;
        let plain = |p: Vec2| {
            self.cell_at(Vec3::new(p.x, 0.0, p.y))
                .is_some_and(|c| self.cost[c.1 * self.w + c.0] == 1)
        };
        (0..=steps).all(|i| {
            let p = Vec2::new(a.x, a.z) + d * (i as f32 / steps as f32);
            plain(p) && plain(p + side) && plain(p - side)
        })
    }

    /// Unnoetige Zwischenpunkte weglassen.
    pub fn smooth(&self, points: &[Vec3]) -> Vec<Vec3> {
        let Some(&first) = points.first() else {
            return Vec::new();
        };
        let mut out = vec![first];
        let mut i = 0;
        while i + 1 < points.len() {
            let mut far = i + 1;
            for k in (i + 2..points.len()).rev() {
                if self.line_free(points[i], points[k]) {
                    far = k;
                    break;
                }
            }
            out.push(points[far]);
            i = far;
        }
        out
    }

    /// Abstaende aller Zellen zu den Zielen: einmal rechnen, beliebig viele Figuren folgen.
    pub fn distance_field(&self, goals: &[Cell]) -> DistanceField {
        let mut dist = vec![u32::MAX; self.w * self.h];
        let mut open = BinaryHeap::new();
        for &c in goals.iter().filter(|c| self.free(**c)) {
            let i = c.1 * self.w + c.0;
            dist[i] = 0;
            open.push(Reverse((0, i)));
        }
        while let Some(Reverse((d, i))) = open.pop() {
            if d > dist[i] {
                continue;
            }
            for (n, step) in self.neighbors(i) {
                if d + step < dist[n] {
                    dist[n] = d + step;
                    open.push(Reverse((d + step, n)));
                }
            }
        }
        DistanceField { dist }
    }
}

/// Abstaende zu einem oder mehreren Zielen.
#[derive(Clone, Debug)]
pub struct DistanceField {
    dist: Vec<u32>,
}

impl DistanceField {
    /// Weglaenge in Metern; `None` = unerreichbar.
    pub fn distance(&self, g: &Grid, p: Vec3) -> Option<f32> {
        let c = g.cell_at(p)?;
        let d = *self.dist.get(c.1 * g.w + c.0)?;
        (d != u32::MAX).then(|| d as f32 / STRAIGHT as f32 * g.cell)
    }

    /// Richtung (waagerecht, Laenge 1) zum Ziel; `None` = da oder unerreichbar.
    pub fn direction(&self, g: &Grid, p: Vec3) -> Option<Vec3> {
        let c = g.cell_at(p)?;
        let i = c.1 * g.w + c.0;
        let (n, _) = g.neighbors(i).min_by_key(|(n, _)| self.dist[*n])?;
        if self.dist[n] >= self.dist[i] {
            return None;
        }
        let to = g.center((n % g.w, n / g.w));
        let d = Vec3::new(to.x - p.x, 0.0, to.z - p.z);
        Some(d.normalize_or_zero()).filter(|d| *d != Vec3::ZERO)
    }
}

/// Folgt einem Weg Punkt fuer Punkt.
#[derive(Clone, Debug, Default)]
pub struct Follower {
    path: Vec<Vec3>,
    next: usize,
    /// so lange kommt die Figur schon nicht voran (Sekunden)
    stuck: f32,
    last: Vec3,
}

impl Follower {
    pub fn new(path: Vec<Vec3>) -> Self {
        Self {
            path,
            ..Default::default()
        }
    }

    pub fn done(&self) -> bool {
        self.next >= self.path.len()
    }

    /// Richtung zum naechsten Punkt (waagerecht, Laenge 1). `reach` = so nah gilt ein Punkt als
    /// erreicht. `None` = am Ziel.
    pub fn steer(&mut self, pos: Vec3, reach: f32, dt: f32) -> Option<Vec3> {
        let moved = Vec2::new(pos.x - self.last.x, pos.z - self.last.z).length();
        self.stuck = if moved < reach * 0.02 { self.stuck + dt } else { 0.0 };
        self.last = pos;
        while let Some(p) = self.path.get(self.next) {
            let d = Vec3::new(p.x - pos.x, 0.0, p.z - pos.z);
            if d.length() > reach {
                return Some(d.normalize());
            }
            self.next += 1;
        }
        None
    }

    /// Steckt die Figur seit `seconds` fest? Dann neuen Weg suchen.
    pub fn stuck(&self, seconds: f32) -> bool {
        self.stuck >= seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10 x 10 m, Wand bei x = 5 mit einer Luecke bei z = 9.
    fn grid() -> Grid {
        Grid::build(Vec2::ZERO, Vec2::splat(10.0), 0.5, |p| {
            !(p.x > 4.5 && p.x < 5.5 && p.z < 8.5)
        })
    }

    #[test]
    fn paths_go_around_walls_and_are_short() {
        let g = grid();
        let (a, b) = (Vec3::new(2.0, 0.0, 2.0), Vec3::new(8.0, 0.0, 2.0));
        assert!(!g.line_free(a, b));
        let path = g.path(a, b).unwrap();
        assert!(path.len() <= 5, "nicht geglaettet: {path:?}");
        assert_eq!(*path.last().unwrap(), b);
        let mut at = a;
        for p in &path {
            assert!(g.line_free(at, *p), "{at} -> {p} geht durch die Wand");
            at = *p;
        }
        let len: f32 = std::iter::once(a)
            .chain(path.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .sum();
        assert!(len < 17.5, "{len}");
    }

    #[test]
    fn unreachable_goals_have_no_path() {
        let mut g = grid();
        g.block_rect(Vec2::new(4.5, 8.0), Vec2::new(5.5, 10.0), 0.0);
        assert!(g.path(Vec3::new(2.0, 0.0, 2.0), Vec3::new(8.0, 0.0, 2.0)).is_none());
        let f = g.distance_field(&[g.cell_at(Vec3::new(8.0, 0.0, 2.0)).unwrap()]);
        assert!(f.distance(&g, Vec3::new(2.0, 0.0, 2.0)).is_none());
    }

    #[test]
    fn followers_and_fields_reach_the_goal() {
        let g = grid();
        let goal = Vec3::new(8.0, 0.0, 2.0);
        let field = g.distance_field(&[g.cell_at(goal).unwrap()]);
        let mut p = Vec3::new(2.0, 0.0, 2.0);
        for _ in 0..400 {
            let Some(d) = field.direction(&g, p) else {
                break;
            };
            p += d * 0.1;
        }
        assert!(p.distance(goal) < 1.0, "{p}");
        let mut f = Follower::new(g.path(Vec3::new(2.0, 0.0, 2.0), goal).unwrap());
        let mut p = Vec3::new(2.0, 0.0, 2.0);
        for _ in 0..400 {
            match f.steer(p, 0.15, 0.016) {
                Some(d) => p += d * 0.1,
                None => break,
            }
        }
        assert!(f.done() && p.distance(goal) < 0.2);
        let mut blocked = Follower::new(vec![goal]);
        for _ in 0..10 {
            blocked.steer(Vec3::X, 0.2, 0.2);
        }
        assert!(blocked.stuck(1.0));
    }

    #[test]
    fn expensive_ground_is_avoided() {
        let mut g = Grid::new(Vec2::ZERO, 1.0, 9, 9);
        for z in 0..8 {
            g.set_cost((4, z), 20);
        }
        let path = g.path(Vec3::new(0.5, 0.0, 4.5), Vec3::new(8.5, 0.0, 4.5)).unwrap();
        assert!(path.iter().any(|p| p.z > 7.5), "{path:?}");
    }
}
