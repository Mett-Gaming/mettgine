//! Formen und einzelne Schnitt-Tests: Strahl, Kiste, Kugel, stehende Kapsel.

use glam::{Vec2, Vec3};

/// Kleinste Laenge, unter der eine Richtung als null gilt
const EPS: f32 = 1e-6;

/// Ein Strahl; `dir` ist normiert.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Ray {
    pub fn new(origin: Vec3, dir: Vec3) -> Self {
        Self {
            origin,
            dir: dir.normalize_or_zero(),
        }
    }
    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.dir * t
    }
}

/// Treffer eines Strahls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// Entfernung (Meter); 0 = Start liegt in der Form
    pub t: f32,
    pub point: Vec3,
    /// zeigt von der Form weg
    pub normal: Vec3,
}

/// Achsenparallele Kiste.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(a: Vec3, b: Vec3) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }
    /// Aus Mittelpunkt und halben Kantenlaengen.
    pub fn centered(center: Vec3, half: Vec3) -> Self {
        Self::new(center - half.abs(), center + half.abs())
    }
    pub fn overlaps(&self, o: &Aabb) -> bool {
        self.min.cmple(o.max).all() && self.max.cmpge(o.min).all()
    }
    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }
    pub fn closest(&self, p: Vec3) -> Vec3 {
        p.clamp(self.min, self.max)
    }
    pub fn expanded(&self, by: f32) -> Self {
        Self {
            min: self.min - Vec3::splat(by),
            max: self.max + Vec3::splat(by),
        }
    }

    /// Erster Schnitt bis `max_t`.
    pub fn raycast(&self, r: &Ray, max_t: f32) -> Option<Hit> {
        let (mut t0, mut t1) = (0.0f32, max_t);
        let mut normal = Vec3::ZERO;
        for a in 0..3 {
            if r.dir[a].abs() < EPS {
                if r.origin[a] < self.min[a] || r.origin[a] > self.max[a] {
                    return None;
                }
                continue;
            }
            let inv = 1.0 / r.dir[a];
            let (mut near, mut far) = (
                (self.min[a] - r.origin[a]) * inv,
                (self.max[a] - r.origin[a]) * inv,
            );
            if near > far {
                std::mem::swap(&mut near, &mut far);
            }
            if near > t0 {
                t0 = near;
                normal = Vec3::ZERO;
                normal[a] = -r.dir[a].signum();
            }
            t1 = t1.min(far);
            if t0 > t1 {
                return None;
            }
        }
        Some(Hit {
            t: t0,
            point: r.at(t0),
            normal: if normal == Vec3::ZERO { -r.dir } else { normal },
        })
    }
}

/// Feste Kiste, um die Hochachse gedreht.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solid {
    pub center: Vec3,
    /// halbe Kantenlaengen
    pub half: Vec3,
    /// Drehung um die Hochachse (Radiant, wie `Quat::from_rotation_y`)
    pub yaw: f32,
}

impl Solid {
    pub fn new(center: Vec3, half: Vec3, yaw: f32) -> Self {
        Self {
            center,
            half: half.abs(),
            yaw,
        }
    }

    fn rotate(v: Vec3, yaw: f32) -> Vec3 {
        let (s, c) = yaw.sin_cos();
        Vec3::new(v.x * c + v.z * s, v.y, -v.x * s + v.z * c)
    }
    /// Punkt in das ungedrehte System der Kiste (Mitte = Ursprung).
    pub fn to_local(&self, p: Vec3) -> Vec3 {
        Self::rotate(p - self.center, -self.yaw)
    }
    /// Richtung zurueck in die Welt.
    pub fn dir_to_world(&self, v: Vec3) -> Vec3 {
        Self::rotate(v, self.yaw)
    }
    fn local_box(&self) -> Aabb {
        Aabb::centered(Vec3::ZERO, self.half)
    }

    /// Achsenparallele Huelle.
    pub fn aabb(&self) -> Aabb {
        let (s, c) = self.yaw.sin_cos();
        let (s, c) = (s.abs(), c.abs());
        let h = Vec3::new(
            self.half.x * c + self.half.z * s,
            self.half.y,
            self.half.x * s + self.half.z * c,
        );
        Aabb::centered(self.center, h)
    }

    pub fn raycast(&self, r: &Ray, max_t: f32) -> Option<Hit> {
        let local = Ray {
            origin: self.to_local(r.origin),
            dir: Self::rotate(r.dir, -self.yaw),
        };
        self.local_box().raycast(&local, max_t).map(|h| Hit {
            t: h.t,
            point: r.at(h.t),
            normal: self.dir_to_world(h.normal),
        })
    }
}

/// Kugel: erster Schnitt bis `max_t`.
pub fn ray_sphere(r: &Ray, center: Vec3, radius: f32, max_t: f32) -> Option<Hit> {
    let m = r.origin - center;
    let b = m.dot(r.dir);
    let c = m.length_squared() - radius * radius;
    if c > 0.0 && b > 0.0 {
        return None;
    }
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()).max(0.0);
    (t <= max_t).then(|| {
        let point = r.at(t);
        Hit {
            t,
            point,
            normal: (point - center).normalize_or(-r.dir),
        }
    })
}

/// Stehende Kapsel (Spielfigur): Fusspunkt, Radius und Gesamthoehe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capsule {
    /// tiefster Punkt
    pub foot: Vec3,
    pub radius: f32,
    /// Gesamthoehe, mindestens zwei Radien
    pub height: f32,
}

impl Capsule {
    pub fn new(foot: Vec3, radius: f32, height: f32) -> Self {
        Self {
            foot,
            radius,
            height: height.max(radius * 2.0),
        }
    }
    /// Unteres und oberes Ende der Mittellinie (y).
    fn axis(&self) -> (f32, f32) {
        (
            self.foot.y + self.radius,
            self.foot.y + self.height - self.radius,
        )
    }
    pub fn aabb(&self) -> Aabb {
        let r = Vec3::new(self.radius, 0.0, self.radius);
        Aabb::new(self.foot - r, self.foot + r + Vec3::Y * self.height)
    }

    /// Wie weit und wohin die Kapsel geschoben werden muss, um `s` zu verlassen
    /// (Normale von der Kiste weg, Tiefe in Metern).
    pub fn penetration(&self, s: &Solid) -> Option<(Vec3, f32)> {
        let local = s.to_local(self.foot);
        let (lo, hi) = self.axis();
        let (lo, hi) = (lo - s.center.y, hi - s.center.y);
        let h = s.half;
        // naechste Stelle der Mittellinie zur Kiste
        let y = if hi < -h.y {
            hi
        } else if lo > h.y {
            lo
        } else {
            lo.max(-h.y).min(hi)
        };
        let p = Vec3::new(local.x, y, local.z);
        let q = p.clamp(-h, h);
        let d = p - q;
        let dist = d.length();
        if dist > EPS {
            return (dist < self.radius).then(|| (s.dir_to_world(d / dist), self.radius - dist));
        }
        // Mittellinie steckt in der Kiste: kuerzester Weg hinaus
        let up = h.y - lo;
        let down = hi + h.y;
        let exits = [
            (Vec3::X, h.x - p.x),
            (Vec3::NEG_X, h.x + p.x),
            (Vec3::Z, h.z - p.z),
            (Vec3::NEG_Z, h.z + p.z),
            (Vec3::Y, up),
            (Vec3::NEG_Y, down),
        ];
        exits
            .iter()
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, depth)| (s.dir_to_world(*n), depth + self.radius))
    }

    pub fn raycast(&self, r: &Ray, max_t: f32) -> Option<Hit> {
        let (lo, hi) = self.axis();
        let mid = |y: f32| Vec3::new(self.foot.x, y, self.foot.z);
        let mut best: Option<Hit> = None;
        let mut take = |h: Option<Hit>| {
            if let Some(h) = h {
                if best.is_none_or(|b| h.t < b.t) {
                    best = Some(h);
                }
            }
        };
        take(ray_sphere(r, mid(lo), self.radius, max_t));
        take(ray_sphere(r, mid(hi), self.radius, max_t));
        // Mantel des Zylinders
        let o = Vec2::new(r.origin.x - self.foot.x, r.origin.z - self.foot.z);
        let d = Vec2::new(r.dir.x, r.dir.z);
        let (a, b) = (d.length_squared(), o.dot(d));
        let c = o.length_squared() - self.radius * self.radius;
        let disc = b * b - a * c;
        if a > EPS && disc >= 0.0 && (c <= 0.0 || b < 0.0) {
            let t = ((-b - disc.sqrt()) / a).max(0.0);
            let p = r.at(t);
            if t <= max_t && p.y >= lo && p.y <= hi {
                let n = Vec3::new(p.x - self.foot.x, 0.0, p.z - self.foot.z);
                take(Some(Hit {
                    t,
                    point: p,
                    normal: n.normalize_or(-r.dir),
                }));
            }
        }
        best
    }
}
