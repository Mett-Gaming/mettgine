//! Schatten von Lampen: alle teilen sich ein Tiefenbild mit gleich grossen Feldern. Kegel
//! brauchen ein Feld, Punktlampen sechs (je Wuerfelseite eins).

use super::gpu::{GpuTile, TILES};
use super::types::{Light, LightKind};
use ash::vk;
use glam::{Mat4, Vec3};

/// Kantenlaenge des Atlas (Texel)
pub const ATLAS_SIZE: u32 = 4096;
/// Felder je Kante
const PER_SIDE: u32 = 8;
/// Nahe Grenze (Meter)
const NEAR: f32 = 0.05;

/// Ein Feld, das in diesem Bild gezeichnet wird.
#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub view_proj: Mat4,
    pub pos: Vec3,
    pub range: f32,
    pub area: vk::Rect2D,
}

impl Tile {
    pub fn gpu(&self) -> GpuTile {
        let k = 1.0 / ATLAS_SIZE as f32;
        GpuTile {
            view_proj: self.view_proj.to_cols_array_2d(),
            rect: [
                self.area.offset.x as f32 * k,
                self.area.offset.y as f32 * k,
                self.area.extent.width as f32 * k,
                self.area.extent.height as f32 * k,
            ],
        }
    }
}

/// Blickrichtung und Oben der sechs Seiten: +x, -x, +y, -y, +z, -z (wie im Shader).
const FACES: [(Vec3, Vec3); 6] = [
    (Vec3::X, Vec3::Y),
    (Vec3::NEG_X, Vec3::Y),
    (Vec3::Y, Vec3::Z),
    (Vec3::NEG_Y, Vec3::Z),
    (Vec3::Z, Vec3::Y),
    (Vec3::NEG_Z, Vec3::Y),
];

fn area(i: usize) -> vk::Rect2D {
    let size = ATLAS_SIZE / PER_SIDE;
    let i = i as u32;
    vk::Rect2D {
        offset: vk::Offset2D {
            x: (i % PER_SIDE * size) as i32,
            y: (i / PER_SIDE * size) as i32,
        },
        extent: vk::Extent2D {
            width: size,
            height: size,
        },
    }
}

/// Felder vergeben: nahe Lampen zuerst. Liefert je Lampe ihr erstes Feld.
pub fn assign(lights: &[Light], eye: Vec3, tiles: &mut Vec<Tile>) -> Vec<Option<usize>> {
    tiles.clear();
    let mut out = vec![None; lights.len()];
    let mut order: Vec<usize> = (0..lights.len()).filter(|&i| lights[i].shadow).collect();
    let importance = |l: &Light| l.pos.distance(eye) - l.range;
    order.sort_by(|&a, &b| importance(&lights[a]).total_cmp(&importance(&lights[b])));
    for i in order {
        let l = &lights[i];
        let views = views(l);
        if tiles.len() + views.len() > TILES {
            continue;
        }
        out[i] = Some(tiles.len());
        for view_proj in views {
            tiles.push(Tile {
                view_proj,
                pos: l.pos,
                range: l.range,
                area: area(tiles.len()),
            });
        }
    }
    out
}

fn views(l: &Light) -> Vec<Mat4> {
    let far = l.range.max(NEAR * 2.0);
    match l.kind {
        LightKind::Spot { dir, outer, .. } => {
            let dir = dir.normalize_or(Vec3::NEG_Y);
            let up = if dir.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
            let fov = (outer * 2.0 + 0.05).clamp(0.1, 2.9);
            let view = Mat4::look_at_rh(l.pos, l.pos + dir, up);
            vec![Mat4::perspective_rh(fov, 1.0, NEAR, far) * view]
        }
        LightKind::Point => {
            let proj = Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, NEAR, far);
            FACES
                .iter()
                .map(|(dir, up)| proj * Mat4::look_at_rh(l.pos, l.pos + *dir, *up))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamp(x: f32, kind: LightKind, shadow: bool) -> Light {
        Light {
            pos: Vec3::new(x, 0.0, 0.0),
            color: Vec3::ONE,
            range: 5.0,
            kind,
            shadow,
        }
    }

    #[test]
    fn near_lamps_get_tiles_first_and_the_atlas_never_overflows() {
        let mut lights: Vec<Light> = (0..20)
            .map(|i| lamp(i as f32, LightKind::Point, true))
            .collect();
        lights.push(lamp(0.0, LightKind::Point, false));
        let mut tiles = Vec::new();
        let first = assign(&lights, Vec3::new(19.0, 0.0, 0.0), &mut tiles);
        assert_eq!(tiles.len(), 60, "zehn Punktlampen zu je sechs Feldern");
        assert!(first[19].is_some() && first[0].is_none() && first[20].is_none());
        let mut seen = std::collections::HashSet::new();
        for t in &tiles {
            assert!(seen.insert((t.area.offset.x, t.area.offset.y)));
            assert!(t.area.offset.x as u32 + t.area.extent.width <= ATLAS_SIZE);
        }
    }

    #[test]
    fn cube_faces_see_their_own_direction() {
        let l = lamp(0.0, LightKind::Point, true);
        for ((dir, _), vp) in FACES.iter().zip(views(&l)) {
            let q = vp.project_point3(*dir * 2.0);
            assert!(q.x.abs() < 1e-4 && q.y.abs() < 1e-4 && (0.0..1.0).contains(&q.z));
        }
    }
}
