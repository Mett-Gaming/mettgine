//! Sichtbarkeit: Huell-Kugeln und das Sichtfeld der Kamera.
//!
//! Der Szenen-Renderer sortiert damit selbst aus, was nicht im Bild ist; Spiele koennen die
//! Bausteine auch direkt benutzen (z. B. um Figuren ausserhalb des Bildes nicht zu animieren).

use crate::gpu::{MeshVertex, SkinVertex};
use glam::{Mat4, Vec3, Vec4};

/// Eine Kugel, die etwas vollstaendig umschliesst.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sphere {
    pub center: Vec3,
    pub radius: f32,
}

impl Sphere {
    /// Kugel um alle Punkte (Mitte des Quaders, nicht die kleinstmoegliche). Leer = Radius 0.
    pub fn around(points: impl Iterator<Item = Vec3> + Clone) -> Self {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        let mut any = false;
        for p in points.clone() {
            min = min.min(p);
            max = max.max(p);
            any = true;
        }
        if !any {
            return Self {
                center: Vec3::ZERO,
                radius: 0.0,
            };
        }
        let center = (min + max) * 0.5;
        let radius = points
            .map(|p| p.distance_squared(center))
            .fold(0.0f32, f32::max)
            .sqrt();
        Self { center, radius }
    }

    /// Kugel um ein Modell.
    pub fn of_mesh(vertices: &[MeshVertex]) -> Self {
        Self::around(vertices.iter().map(|v| Vec3::from(v.pos)))
    }

    /// Kugel nach einer Transformation; die staerkste Achsen-Skalierung zaehlt.
    pub fn transformed(&self, m: &Mat4) -> Self {
        Self {
            center: m.transform_point3(self.center),
            radius: self.radius * max_scale(m),
        }
    }

    /// Kleinste Kugel, die beide umschliesst.
    pub fn union(&self, o: &Sphere) -> Sphere {
        let d = o.center - self.center;
        let dist = d.length();
        if dist + o.radius <= self.radius {
            return *self;
        }
        if dist + self.radius <= o.radius {
            return *o;
        }
        let radius = (dist + self.radius + o.radius) * 0.5;
        Sphere {
            center: self.center + d * ((radius - self.radius) / dist),
            radius,
        }
    }
}

/// Obergrenze dafuer, wie stark `m` Laengen streckt (auch bei Scherung gueltig).
fn max_scale(m: &Mat4) -> f32 {
    let (x, y, z) = (
        m.x_axis.truncate(),
        m.y_axis.truncate(),
        m.z_axis.truncate(),
    );
    let orthogonal = x.dot(y).abs() + y.dot(z).abs() + x.dot(z).abs() < 1e-4;
    let (a, b, c) = (x.length_squared(), y.length_squared(), z.length_squared());
    if orthogonal {
        a.max(b).max(c).sqrt()
    } else {
        // Frobenius-Norm: immer mindestens so gross wie die staerkste Streckung
        (a + b + c).sqrt()
    }
}

/// Huell-Kugeln einer Figur je Knochen. Weil jeder Vertex eine Mischung seiner Knochen ist,
/// liegt die Figur in jeder Pose innerhalb der bewegten Kugeln.
#[derive(Clone, Debug, Default)]
pub struct SkinBounds {
    bones: Vec<(u32, Sphere)>,
}

impl SkinBounds {
    pub fn new(vertices: &[SkinVertex]) -> Self {
        let mut per_bone: Vec<Vec<Vec3>> = Vec::new();
        for v in vertices {
            for (j, w) in v.joints.iter().zip(v.weights) {
                if w <= 0.0 {
                    continue;
                }
                let j = *j as usize;
                if per_bone.len() <= j {
                    per_bone.resize(j + 1, Vec::new());
                }
                per_bone[j].push(Vec3::from(v.pos));
            }
        }
        let bones = per_bone
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.is_empty())
            .map(|(i, p)| (i as u32, Sphere::around(p.iter().copied())))
            .collect();
        Self { bones }
    }

    /// Kugel um die Figur in ihrer Pose. `None` = unbekannt (Knochen fehlen): nicht aussortieren.
    pub fn posed(&self, model: &Mat4, bones: &[Mat4]) -> Option<Sphere> {
        let mut total: Option<Sphere> = None;
        for (i, s) in &self.bones {
            let b = bones.get(*i as usize)?;
            let s = s.transformed(b);
            total = Some(total.map_or(s, |t| t.union(&s)));
        }
        total.map(|t| t.transformed(model))
    }
}

/// Sichtfeld einer Kamera als Ebenen (Normalen zeigen nach innen).
#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    /// Aus Projektion x Ansicht (Tiefe 0..1 wie in Vulkan).
    pub fn new(view_proj: &Mat4) -> Self {
        let r = [
            view_proj.row(0),
            view_proj.row(1),
            view_proj.row(2),
            view_proj.row(3),
        ];
        let planes = [
            r[3] + r[0],
            r[3] - r[0],
            r[3] + r[1],
            r[3] - r[1],
            r[2],
            r[3] - r[2],
        ];
        Self {
            planes: planes.map(|p| {
                let l = p.truncate().length();
                if l > 0.0 {
                    p / l
                } else {
                    p
                }
            }),
        }
    }

    /// Beruehrt die Kugel das Sichtfeld? Im Zweifel ja (nie faelschlich nein).
    pub fn sees(&self, s: &Sphere) -> bool {
        // umgekehrt gefragt, damit ungueltige Zahlen (NaN) als sichtbar gelten
        !self
            .planes
            .iter()
            .any(|p| p.truncate().dot(s.center) + p.w < -s.radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::perspective;

    fn cam() -> Frustum {
        let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::Z, Vec3::Y);
        Frustum::new(&(perspective(1.2, 16.0 / 9.0, 0.1, 100.0) * view))
    }

    fn ball(x: f32, y: f32, z: f32, radius: f32) -> Sphere {
        Sphere {
            center: Vec3::new(x, y, z),
            radius,
        }
    }

    #[test]
    fn frustum_keeps_visible_and_drops_hidden() {
        let f = cam();
        assert!(f.sees(&ball(0.0, 0.0, 10.0, 1.0)), "vor der Kamera");
        assert!(!f.sees(&ball(0.0, 0.0, -10.0, 1.0)), "hinter der Kamera");
        assert!(!f.sees(&ball(60.0, 0.0, 10.0, 1.0)), "weit links");
        assert!(!f.sees(&ball(0.0, 0.0, 150.0, 1.0)), "zu weit weg");
        assert!(f.sees(&ball(0.0, 0.0, -0.5, 1.0)), "Kamera steckt darin");
        assert!(f.sees(&ball(60.0, 0.0, 10.0, 60.0)), "gross genug");
    }

    #[test]
    fn frustum_never_drops_a_point_that_lands_in_the_image() {
        let view = Mat4::look_at_rh(
            Vec3::new(3.0, 5.0, -2.0),
            Vec3::new(-4.0, 1.0, 20.0),
            Vec3::Y,
        );
        let vp = perspective(1.1, 1.6, 0.1, 80.0) * view;
        let f = Frustum::new(&vp);
        for i in 0..4000 {
            let h = |k: i32| ((i * 7919 + k * 104_729) % 1000) as f32 / 1000.0 - 0.5;
            let p = Vec3::new(h(1) * 90.0, h(2) * 60.0, h(3) * 160.0);
            let c = vp * p.extend(1.0);
            let inside = c.w > 0.0 && c.x.abs() < c.w && c.y.abs() < c.w && c.z > 0.0 && c.z < c.w;
            assert!(!inside || f.sees(&ball(p.x, p.y, p.z, 0.0)), "{p}");
        }
    }

    #[test]
    fn spheres_follow_scale_shear_and_union() {
        let s = ball(1.0, 0.0, 0.0, 1.0);
        let t = s.transformed(
            &(Mat4::from_translation(Vec3::Y) * Mat4::from_scale(Vec3::new(1.0, 4.0, 2.0))),
        );
        assert_eq!(t.center, Vec3::new(1.0, 1.0, 0.0));
        assert!((t.radius - 4.0).abs() < 1e-5);
        let shear = Mat4::from_cols(Vec4::X, Vec4::new(3.0, 1.0, 0.0, 0.0), Vec4::Z, Vec4::W);
        let d = Vec3::new(0.3, 0.95, 0.0).normalize();
        assert!(shear.transform_vector3(d).length() <= max_scale(&shear));
        let u = ball(-5.0, 0.0, 0.0, 1.0).union(&ball(5.0, 0.0, 0.0, 2.0));
        assert!((u.radius - 6.5).abs() < 1e-5 && (u.center.x - 0.5).abs() < 1e-5);
        assert_eq!(ball(0.0, 0.0, 0.0, 9.0).union(&s).radius, 9.0);
        assert_eq!(Sphere::around(std::iter::empty()).radius, 0.0);
    }

    #[test]
    fn skin_bounds_contain_every_posed_vertex() {
        let v = |x: f32, y: f32, j: [f32; 4], w: [f32; 4]| SkinVertex {
            pos: [x, y, 0.0],
            normal: [0.0; 3],
            color: [1.0; 3],
            joints: j,
            weights: w,
        };
        let verts = [
            v(0.0, 0.0, [0.0; 4], [1.0, 0.0, 0.0, 0.0]),
            v(0.2, 1.0, [0.0, 1.0, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0]),
            v(0.0, 2.0, [1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]),
        ];
        let b = SkinBounds::new(&verts);
        let bones = [
            Mat4::IDENTITY,
            Mat4::from_rotation_z(1.2) * Mat4::from_translation(Vec3::new(0.0, 3.0, 0.0)),
        ];
        let model =
            Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)) * Mat4::from_scale(Vec3::splat(2.0));
        let s = b.posed(&model, &bones).unwrap();
        for v in &verts {
            let mut p = Vec3::ZERO;
            for (j, w) in v.joints.iter().zip(v.weights) {
                p += bones[*j as usize].transform_point3(Vec3::from(v.pos)) * w;
            }
            let p = model.transform_point3(p);
            assert!(
                p.distance(s.center) <= s.radius + 1e-3,
                "{p} ausserhalb {s:?}"
            );
        }
        assert!(
            b.posed(&model, &bones[..1]).is_none(),
            "Knochen fehlt: nicht aussortieren"
        );
    }
}
