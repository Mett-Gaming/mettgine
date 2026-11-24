//! Viele Lampen: Das Sichtfeld ist in Zellen geteilt, jede kennt nur die Lampen, die sie
//! erreichen. So rechnet jeder Bildpunkt mit wenigen Lampen, egal wie viele es gibt.

use super::types::{Camera, Light};
use glam::Vec3;

/// Zellen quer, hoch, in die Tiefe
pub const GRID: [usize; 3] = [16, 9, 24];
pub const CELLS: usize = GRID[0] * GRID[1] * GRID[2];
/// Lampen je Bild
pub const MAX_LIGHTS: usize = 1024;
/// Eintraege aller Zellen zusammen
const MAX_ENTRIES: usize = 256 * 1024;
/// Lampen je Zelle
const MAX_PER_CELL: usize = 64;

/// Wiederverwendete Listen.
#[derive(Default)]
pub struct Clusters {
    /// je Zelle: Anfang und Anzahl in `entries`
    pub cells: Vec<[u32; 2]>,
    pub entries: Vec<u32>,
    lists: Vec<Vec<u16>>,
}

/// Tiefe (Meter) der Grenze `k` der Tiefenscheiben.
fn slice_depth(cam: &Camera, k: usize) -> f32 {
    let near = cam.near.max(0.05);
    near * (cam.far / near).powf(k as f32 / GRID[2] as f32)
}

/// Faktor und Versatz, mit denen der Shader aus log2(Tiefe) die Scheibe berechnet.
pub fn slice_params(cam: &Camera) -> (f32, f32) {
    let near = cam.near.max(0.05);
    let scale = GRID[2] as f32 / (cam.far / near).log2();
    (scale, -near.log2() * scale)
}

/// Spanne der Zellen, die `mid` +- `r` in einer Bildrichtung trifft. `depth` ist die kleinste
/// Tiefe der Kugel (> 0), `tan` die halbe Bildbreite je Meter Tiefe.
fn span(mid: f32, r: f32, near: f32, far: f32, tan: f32, n: usize) -> (usize, usize) {
    // aeusserste Werte: links durch die nahe oder ferne Grenze, rechts ebenso
    let lo = ((mid - r) / (near * tan)).min((mid - r) / (far * tan));
    let hi = ((mid + r) / (near * tan)).max((mid + r) / (far * tan));
    let cell = |v: f32| ((v * 0.5 + 0.5) * n as f32).clamp(0.0, n as f32 - 1.0) as usize;
    (cell(lo), cell(hi))
}

impl Clusters {
    /// Lampen auf die Zellen verteilen; `lights` hat hoechstens [`MAX_LIGHTS`] Eintraege.
    pub fn build(&mut self, cam: &Camera, lights: &[Light]) {
        self.lists.resize_with(CELLS, Vec::new);
        for l in &mut self.lists {
            l.clear();
        }
        let tan_y = (cam.fov_y * 0.5).tan();
        let tan_x = tan_y * cam.aspect;
        for (i, l) in lights.iter().take(MAX_LIGHTS).enumerate() {
            let c = cam.view.transform_point3(l.pos);
            let (depth, r) = (-c.z, l.range);
            if depth + r < cam.near || depth - r > cam.far {
                continue;
            }
            for z in 0..GRID[2] {
                let (a, b) = (slice_depth(cam, z), slice_depth(cam, z + 1));
                if depth + r < a || depth - r > b {
                    continue;
                }
                let (x0, x1) = span(c.x, r, a, b, tan_x, GRID[0]);
                // Zeile 0 ist oben, Kamera-y zeigt nach oben
                let (y0, y1) = span(-c.y, r, a, b, tan_y, GRID[1]);
                self.fill(i as u16, z, (x0, x1), (y0, y1), |x, y| {
                    touches(c, r, cell_box(x, y, (a, b), tan_x, tan_y))
                });
            }
        }
        self.flatten();
    }

    fn fill(
        &mut self,
        light: u16,
        z: usize,
        (x0, x1): (usize, usize),
        (y0, y1): (usize, usize),
        hit: impl Fn(usize, usize) -> bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                let list = &mut self.lists[(z * GRID[1] + y) * GRID[0] + x];
                if list.len() < MAX_PER_CELL && hit(x, y) {
                    list.push(light);
                }
            }
        }
    }

    fn flatten(&mut self) {
        self.cells.clear();
        self.entries.clear();
        for l in &self.lists {
            let n = l.len().min(MAX_ENTRIES - self.entries.len());
            self.cells.push([self.entries.len() as u32, n as u32]);
            self.entries.extend(l[..n].iter().map(|&i| i as u32));
        }
    }
}

/// Kiste einer Zelle in Kamera-Koordinaten.
fn cell_box(x: usize, y: usize, (a, b): (f32, f32), tan_x: f32, tan_y: f32) -> (Vec3, Vec3) {
    let edge = |k: usize, n: usize| k as f32 / n as f32 * 2.0 - 1.0;
    let (xl, xr) = (edge(x, GRID[0]) * tan_x, edge(x + 1, GRID[0]) * tan_x);
    let (yt, yb) = (-edge(y, GRID[1]) * tan_y, -edge(y + 1, GRID[1]) * tan_y);
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for d in [a, b] {
        for (kx, ky) in [(xl, yt), (xr, yt), (xl, yb), (xr, yb)] {
            let p = Vec3::new(kx * d, ky * d, -d);
            min = min.min(p);
            max = max.max(p);
        }
    }
    (min, max)
}

fn touches(c: Vec3, r: f32, (min, max): (Vec3, Vec3)) -> bool {
    c.clamp(min, max).distance_squared(c) <= r * r
}

#[cfg(test)]
mod tests {
    use super::super::types::LightKind;
    use super::*;

    fn light(pos: Vec3, range: f32) -> Light {
        Light {
            pos,
            color: Vec3::ONE,
            range,
            kind: LightKind::Point,
            shadow: false,
        }
    }

    /// Zelle eines Punktes, gerechnet wie im Shader.
    fn cell_of(cam: &Camera, p: Vec3) -> Option<usize> {
        let q = cam.view_proj().project_point3(p);
        let depth = -cam.view.transform_point3(p).z;
        if q.x.abs() >= 1.0 || q.y.abs() >= 1.0 || depth < cam.near || depth > cam.far {
            return None;
        }
        let (s, o) = slice_params(cam);
        let x = ((q.x * 0.5 + 0.5) * GRID[0] as f32) as usize;
        let y = ((0.5 - q.y * 0.5) * GRID[1] as f32) as usize;
        let z = (depth.log2() * s + o) as usize;
        Some((z * GRID[1] + y) * GRID[0] + x)
    }

    #[test]
    fn every_lit_point_finds_its_lights() {
        let cam = Camera::looking(Vec3::new(0.0, 2.0, 0.0), Vec3::new(3.0, 1.0, 10.0), 1.6);
        let mut seed = 7u32;
        let mut rnd = |a: f32, b: f32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            a + (b - a) * (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let lights: Vec<Light> = (0..40)
            .map(|_| {
                let p = Vec3::new(rnd(-20.0, 20.0), rnd(-4.0, 8.0), rnd(-5.0, 40.0));
                light(p, rnd(0.5, 6.0))
            })
            .collect();
        let mut c = Clusters::default();
        c.build(&cam, &lights);
        let mut checked = 0;
        for _ in 0..4000 {
            let p = Vec3::new(rnd(-20.0, 20.0), rnd(-4.0, 8.0), rnd(0.0, 40.0));
            let Some(cell) = cell_of(&cam, p) else {
                continue;
            };
            let [first, n] = c.cells[cell];
            let list = &c.entries[first as usize..(first + n) as usize];
            for (i, l) in lights.iter().enumerate() {
                if l.pos.distance(p) < l.range * 0.999 {
                    assert!(list.contains(&(i as u32)), "Lampe {i} fehlt bei {p}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 100);
    }

    #[test]
    fn lights_behind_the_camera_cost_nothing() {
        let cam = Camera::looking(Vec3::ZERO, Vec3::Z, 1.6);
        let mut c = Clusters::default();
        c.build(&cam, &[light(Vec3::new(0.0, 0.0, -9.0), 3.0)]);
        assert!(c.entries.is_empty());
        assert_eq!(c.cells.len(), CELLS);
    }
}
