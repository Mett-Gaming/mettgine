//! Sonnenschatten in Stufen: nahe am Betrachter fein, weiter weg grob.

use super::types::Camera;
use glam::{Mat4, Vec3, Vec4};

/// Anzahl Stufen
pub const CASCADES: usize = 4;
/// Anteil logarithmischer Aufteilung (Rest gleichmaessig)
const LOG_SHARE: f32 = 0.7;
/// So weit (Meter) vor der Stufe werfen Dinge noch Schatten hinein
const CASTER_DISTANCE: f32 = 120.0;

/// Eine Stufe.
#[derive(Clone, Copy, Debug)]
pub struct Cascade {
    pub view_proj: Mat4,
    /// gilt bis zu dieser Entfernung (Meter, Blickrichtung)
    pub end: f32,
    /// Groesse eines Texels (Meter)
    pub texel: f32,
}

/// Grenzen der Stufen von `near` bis `far`.
fn splits(near: f32, far: f32) -> [f32; CASCADES + 1] {
    std::array::from_fn(|i| {
        let t = i as f32 / CASCADES as f32;
        let log = near * (far / near).powf(t);
        let even = near + (far - near) * t;
        log * LOG_SHARE + even * (1.0 - LOG_SHARE)
    })
}

/// Umkugel eines Sichtfeld-Abschnitts: Entfernung des Mittelpunkts auf der Blickachse, Radius.
/// Haengt nicht von der Drehung ab, darum flimmern die Schatten beim Umsehen nicht.
fn sphere(cam: &Camera, near: f32, far: f32) -> (f32, f32) {
    let tan = (cam.fov_y * 0.5).tan();
    // Abstand der Ecke von der Achse je Meter Tiefe
    let k2 = tan * tan * (1.0 + cam.aspect * cam.aspect);
    let mid = ((near + far) * 0.5 * (1.0 + k2)).min(far);
    let r_far = ((far - mid).powi(2) + k2 * far * far).sqrt();
    let r_near = ((near - mid).powi(2) + k2 * near * near).sqrt();
    (mid, r_far.max(r_near))
}

/// Stufen fuer die Kamera; `size` = Kantenlaenge der Schattenkarte (Texel).
pub fn cascades(cam: &Camera, sun_dir: Vec3, distance: f32, size: u32) -> [Cascade; CASCADES] {
    let far = distance.min(cam.far).max(cam.near + 1.0);
    let s = splits(cam.near.max(0.05), far);
    let inv = cam.view.inverse();
    let forward = -inv.z_axis.truncate();
    let dir = sun_dir.normalize_or(Vec3::Y);
    let up = if dir.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
    std::array::from_fn(|i| {
        let (mid, radius) = sphere(cam, s[i], s[i + 1]);
        // auf glatte Werte runden, damit sich der Massstab nicht staendig aendert
        let radius = (radius * 16.0).ceil() / 16.0;
        let center = cam.pos + forward * mid;
        let texel = 2.0 * radius / size as f32;
        // Mitte aufs Texelraster der Sonne einrasten
        let rot = Mat4::look_at_rh(Vec3::ZERO, -dir, up);
        let c = rot.transform_point3(center);
        let snapped = Vec3::new(
            (c.x / texel).floor() * texel,
            (c.y / texel).floor() * texel,
            c.z,
        );
        let center = rot.inverse().transform_point3(snapped);
        let depth = radius + CASTER_DISTANCE;
        let view = Mat4::look_at_rh(center + dir * depth, center, up);
        let proj = Mat4::orthographic_rh(-radius, radius, -radius, radius, 0.0, depth + radius);
        Cascade {
            view_proj: proj * view,
            end: s[i + 1],
            texel,
        }
    })
}

/// Vier Grenzen als Vektor.
pub fn ends(c: &[Cascade; CASCADES]) -> Vec4 {
    Vec4::new(c[0].end, c[1].end, c[2].end, c[3].end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascades_cover_the_whole_view_up_to_the_distance() {
        let cam = Camera::looking(
            Vec3::new(3.0, 2.0, -5.0),
            Vec3::new(20.0, 0.0, 9.0),
            16.0 / 9.0,
        );
        let c = cascades(&cam, Vec3::new(0.3, 1.0, 0.2), 60.0, 2048);
        assert!((c[3].end - 60.0).abs() < 1e-3);
        let inv = cam.view.inverse();
        let tan = (cam.fov_y * 0.5).tan();
        let mut start = cam.near;
        for k in c {
            assert!(k.end > start);
            for depth in [start, k.end] {
                for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                    let p = Vec3::new(x * tan * cam.aspect * depth, y * tan * depth, -depth);
                    let q = k.view_proj.project_point3(inv.transform_point3(p));
                    assert!(q.x.abs() <= 1.01 && q.y.abs() <= 1.01, "{q}");
                    assert!((0.0..=1.0).contains(&q.z), "{q}");
                }
            }
            start = k.end;
        }
    }

    #[test]
    fn small_camera_steps_move_the_map_by_whole_texels() {
        let at = |x: f32| {
            let eye = Vec3::new(x, 2.0, 0.0);
            let cam = Camera::looking(eye, eye + Vec3::new(0.0, -0.2, 1.0), 1.5);
            cascades(&cam, Vec3::new(0.2, 1.0, 0.1), 50.0, 1024)[0]
        };
        let (a, b) = (at(0.0), at(0.013));
        let p = Vec3::new(0.5, 0.0, 3.0);
        let d = (a.view_proj.project_point3(p) - b.view_proj.project_point3(p)) * 512.0;
        assert!(
            (d.x - d.x.round()).abs() < 0.02 && (d.y - d.y.round()).abs() < 0.02,
            "{d}"
        );
    }
}
