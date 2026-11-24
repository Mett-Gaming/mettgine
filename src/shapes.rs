//! Einfache Formen als Modelle, weiss (Farbe kommt ueber `MeshDraw::tint`). Fuer Versuche,
//! Hilfsanzeigen und Dinge aus Kisten.

use crate::gpu::{MeshData, MeshVertex};
use glam::Vec3;

/// Modell aus Dreiecken (Ecken gegen den Uhrzeigersinn, je Dreieck eine Normale).
pub fn from_triangles(triangles: &[[Vec3; 3]]) -> MeshData {
    let mut vertices = Vec::with_capacity(triangles.len() * 3);
    for [a, b, c] in triangles {
        let n = (*b - *a).cross(*c - *a).normalize_or_zero();
        vertices.extend([a, b, c].map(|p| vertex(*p, n)));
    }
    mesh(vertices)
}

fn vertex(pos: Vec3, normal: Vec3) -> MeshVertex {
    MeshVertex {
        pos: pos.to_array(),
        normal: normal.to_array(),
        color: [1.0; 3],
    }
}

/// Modell mit ausgerechneter Huelle.
pub fn mesh(vertices: Vec<MeshVertex>) -> MeshData {
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for v in &vertices {
        min = min.min(v.pos.into());
        max = max.max(v.pos.into());
    }
    if vertices.is_empty() {
        (min, max) = (Vec3::ZERO, Vec3::ZERO);
    }
    MeshData { vertices, min, max }
}

/// Viereck aus zwei Dreiecken, Ecken gegen den Uhrzeigersinn.
fn quad(out: &mut Vec<[Vec3; 3]>, [a, b, c, d]: [Vec3; 4]) {
    out.push([a, b, c]);
    out.push([a, c, d]);
}

/// Einheitswuerfel um den Ursprung (Kante 1).
pub fn cube() -> MeshData {
    boxed(Vec3::splat(0.5))
}

/// Kiste um den Ursprung mit halben Kanten `half`.
pub fn boxed(half: Vec3) -> MeshData {
    let mut t = Vec::new();
    for axis in 0..3 {
        for side in [-1.0f32, 1.0] {
            let mut n = Vec3::ZERO;
            n[axis] = side;
            let mut u = Vec3::ZERO;
            u[(axis + 1) % 3] = 1.0;
            let v = n.cross(u);
            let p = |a: f32, b: f32| (n + u * a + v * b) * half;
            quad(
                &mut t,
                [p(-1.0, -1.0), p(1.0, -1.0), p(1.0, 1.0), p(-1.0, 1.0)],
            );
        }
    }
    from_triangles(&t)
}

/// Waagerechte Flaeche bei y = 0, Kante `size`, Oberseite sichtbar.
pub fn plane(size: f32) -> MeshData {
    let h = size * 0.5;
    let p = |x: f32, z: f32| Vec3::new(x * h, 0.0, z * h);
    let mut t = Vec::new();
    quad(
        &mut t,
        [p(-1.0, 1.0), p(1.0, 1.0), p(1.0, -1.0), p(-1.0, -1.0)],
    );
    from_triangles(&t)
}

/// Kugel mit Radius 0,5 aus `rings` Ringen (mindestens 3), weich beleuchtet.
pub fn sphere(rings: u32) -> MeshData {
    let rings = rings.max(3);
    let around = rings * 2;
    let at = |r: u32, a: u32| {
        let lat = std::f32::consts::PI * r as f32 / rings as f32;
        let lon = std::f32::consts::TAU * a as f32 / around as f32;
        Vec3::new(lat.sin() * lon.sin(), lat.cos(), lat.sin() * lon.cos())
    };
    let mut vertices = Vec::new();
    for r in 0..rings {
        for a in 0..around {
            let (p00, p01) = (at(r, a), at(r, a + 1));
            let (p10, p11) = (at(r + 1, a), at(r + 1, a + 1));
            for n in [p00, p10, p11, p00, p11, p01] {
                vertices.push(vertex(n * 0.5, n));
            }
        }
    }
    mesh(vertices)
}

/// Zylinder um die Hochachse: Radius 0,5, Hoehe 1, `sides` Seiten (mindestens 3).
pub fn cylinder(sides: u32) -> MeshData {
    let sides = sides.max(3);
    let at = |i: u32, y: f32| {
        let a = std::f32::consts::TAU * i as f32 / sides as f32;
        Vec3::new(a.sin() * 0.5, y, a.cos() * 0.5)
    };
    let mut t = Vec::new();
    for i in 0..sides {
        let (a, b) = (at(i, -0.5), at(i + 1, -0.5));
        let (c, d) = (at(i + 1, 0.5), at(i, 0.5));
        quad(&mut t, [a, b, c, d]);
        t.push([Vec3::Y * 0.5, d, c]);
        t.push([Vec3::Y * -0.5, b, a]);
    }
    from_triangles(&t)
}

/// Keil (Rampe): Einheitswuerfel, dessen Oberseite von -z (unten) nach +z (oben) ansteigt.
pub fn wedge() -> MeshData {
    let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) * 0.5;
    let (l, r) = (-1.0, 1.0);
    let mut t = Vec::new();
    quad(
        &mut t,
        [
            p(l, -1.0, -1.0),
            p(l, 1.0, 1.0),
            p(r, 1.0, 1.0),
            p(r, -1.0, -1.0),
        ],
    );
    quad(
        &mut t,
        [
            p(l, -1.0, 1.0),
            p(r, -1.0, 1.0),
            p(r, 1.0, 1.0),
            p(l, 1.0, 1.0),
        ],
    );
    quad(
        &mut t,
        [
            p(l, -1.0, -1.0),
            p(r, -1.0, -1.0),
            p(r, -1.0, 1.0),
            p(l, -1.0, 1.0),
        ],
    );
    t.push([p(r, -1.0, -1.0), p(r, 1.0, 1.0), p(r, -1.0, 1.0)]);
    t.push([p(l, -1.0, -1.0), p(l, -1.0, 1.0), p(l, 1.0, 1.0)]);
    from_triangles(&t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Alle Normalen zeigen von der Mitte weg und passen zur Drehrichtung der Dreiecke.
    fn check_outward(m: &MeshData) {
        for t in m.vertices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(t[i].pos));
            let face = (b - a).cross(c - a);
            let n = Vec3::from(t[0].normal);
            if face.length() < 1e-6 {
                continue;
            }
            assert!(face.dot(n) > 0.0, "Drehrichtung passt nicht zur Normale");
            assert!(face.dot(a + b + c) > -1e-4, "Flaeche zeigt nach innen");
        }
    }

    #[test]
    fn closed_shapes_face_outward() {
        for m in [cube(), sphere(8), cylinder(12), wedge()] {
            check_outward(&m);
            assert!(m.max.max_element() <= 0.5001 && m.min.min_element() >= -0.5001);
        }
        assert_eq!(cube().vertices.len(), 36);
        assert!(plane(4.0).vertices.iter().all(|v| v.normal[1] > 0.99));
        assert_eq!(plane(4.0).max.x, 2.0);
    }
}
