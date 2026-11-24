//! Kamera-Fuehrung: Ego, Verfolger, Wackeln. Liefert [`render::Camera`](crate::render::Camera)
//! bzw. [`View`] fuer beide Grafikwege.

use crate::render::{perspective, Camera, View};
use glam::{Mat4, Quat, Vec3};

/// Weiter hoch oder runter als das kann man nicht schauen (Radiant)
const PITCH_LIMIT: f32 = 1.54;

/// Gleichmaessig an einen Wert annaehern, unabhaengig von der Bildrate. `half_life` = Sekunden,
/// in denen sich der Abstand halbiert.
pub fn damp(now: f32, to: f32, half_life: f32, dt: f32) -> f32 {
    to + (now - to) * 0.5f32.powf(dt / half_life.max(1e-4))
}

/// Wie [`damp`] fuer Punkte.
pub fn damp3(now: Vec3, to: Vec3, half_life: f32, dt: f32) -> Vec3 {
    to + (now - to) * 0.5f32.powf(dt / half_life.max(1e-4))
}

/// Winkel auf kuerzestem Weg mischen.
pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    a + ((b - a + PI).rem_euclid(TAU) - PI) * t
}

/// Blick aus den Augen: Drehung durch Maus oder Stick.
#[derive(Clone, Copy, Debug, Default)]
pub struct FirstPerson {
    pub pos: Vec3,
    /// um die Hochachse (Radiant), 0 = Blick nach -z
    pub yaw: f32,
    pub pitch: f32,
}

impl FirstPerson {
    /// Um Mausschritte drehen; `sensitivity` = Radiant je Schritt.
    pub fn turn(&mut self, dx: f32, dy: f32, sensitivity: f32) {
        self.yaw -= dx * sensitivity;
        self.pitch = (self.pitch - dy * sensitivity).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch)
    }

    pub fn forward(&self) -> Vec3 {
        self.rotation() * Vec3::NEG_Z
    }

    /// Laufrichtung auf dem Boden aus Eingaben (rechts, vorwaerts), Laenge hoechstens 1.
    pub fn walk(&self, right: f32, forward: f32) -> Vec3 {
        let q = Quat::from_rotation_y(self.yaw);
        (q * Vec3::new(right, 0.0, -forward)).clamp_length_max(1.0)
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_quat(self.rotation().inverse()) * Mat4::from_translation(-self.pos)
    }
}

/// Folgt einem Ziel mit Abstand und Verzoegerung.
#[derive(Clone, Copy, Debug)]
pub struct Follow {
    pub pos: Vec3,
    pub target: Vec3,
    /// Lage relativ zum Ziel (in dessen Blickrichtung gedreht)
    pub offset: Vec3,
    /// auf diese Hoehe ueber dem Ziel schauen
    pub look_up: f32,
    /// Traegheit (Sekunden bis zum halben Weg)
    pub half_life: f32,
}

impl Default for Follow {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            target: Vec3::ZERO,
            offset: Vec3::new(0.0, 2.5, 5.0),
            look_up: 1.2,
            half_life: 0.08,
        }
    }
}

impl Follow {
    /// Nachfuehren. `free` sagt, wie weit (0..1) der Weg vom Ziel zur Kamera frei ist, z. B. aus
    /// einem Strahltest; ohne Pruefung 1 liefern.
    pub fn update(&mut self, at: Vec3, yaw: f32, dt: f32, free: impl Fn(Vec3, Vec3) -> f32) {
        let eye = at + Vec3::Y * self.look_up;
        let want = at + Quat::from_rotation_y(yaw) * self.offset;
        let want = eye + (want - eye) * free(eye, want).clamp(0.05, 1.0);
        self.target = damp3(self.target, eye, self.half_life * 0.5, dt);
        self.pos = damp3(self.pos, want, self.half_life, dt);
    }

    /// Sofort an die richtige Stelle (neue Runde, Teleport).
    pub fn snap(&mut self, at: Vec3, yaw: f32) {
        self.target = at + Vec3::Y * self.look_up;
        self.pos = at + Quat::from_rotation_y(yaw) * self.offset;
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.pos, self.target, Vec3::Y)
    }
}

/// Wackeln nach Treffern: Wucht sammelt sich und klingt ab.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shake {
    trauma: f32,
    time: f32,
}

impl Shake {
    /// Wucht 0..1 dazulegen.
    pub fn hit(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount).clamp(0.0, 1.0);
    }

    pub fn update(&mut self, dt: f32) {
        self.time += dt;
        self.trauma = (self.trauma - dt * 1.4).max(0.0);
    }

    /// Auf die Kamera-Matrix (Welt -> Kamera) anwenden. `angle` = groesster Ausschlag (Radiant).
    pub fn apply(&self, view: Mat4, angle: f32) -> Mat4 {
        if self.trauma <= 0.0 {
            return view;
        }
        let k = self.trauma * self.trauma * angle;
        let n = |seed: f32| {
            let t = self.time * 18.0 + seed * 17.3;
            (t.sin() + (t * 1.7 + seed).sin() * 0.6) / 1.6
        };
        let turn = Quat::from_euler(glam::EulerRot::ZXY, n(1.0) * k, n(2.0) * k, n(3.0) * k);
        Mat4::from_quat(turn) * view
    }
}

/// Kamera-Matrix als Kamera der modernen Grafik.
pub fn camera(pos: Vec3, view: Mat4, fov_y: f32, aspect: f32) -> Camera {
    Camera {
        pos,
        view,
        fov_y,
        aspect,
        near: 0.1,
        far: 500.0,
    }
}

/// Kamera-Matrix als Blick der einfachen Grafik.
pub fn simple_view(pos: Vec3, view: Mat4, fov_y: f32, aspect: f32) -> View {
    View {
        view_proj: perspective(fov_y, aspect, 0.1, 500.0) * view,
        pos,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_person_looks_and_walks_where_it_should() {
        let mut c = FirstPerson::default();
        assert!((c.forward() - Vec3::NEG_Z).length() < 1e-5);
        c.turn(-std::f32::consts::FRAC_PI_2, 0.0, 1.0);
        assert!((c.forward() - Vec3::NEG_X).length() < 1e-5, "links herum");
        assert!((c.walk(0.0, 1.0) - Vec3::NEG_X).length() < 1e-5);
        assert!(c.walk(1.0, 1.0).length() <= 1.0001);
        c.turn(0.0, -100.0, 1.0);
        assert!(c.forward().y > 0.99 && c.pitch < 1.571);
        let p = c.pos + c.forward() * 3.0;
        assert!((c.matrix().transform_point3(p) - Vec3::NEG_Z * 3.0).length() < 1e-4);
    }

    #[test]
    fn damping_does_not_depend_on_the_frame_rate() {
        let one = damp(0.0, 10.0, 0.2, 0.1);
        let two = damp(damp(0.0, 10.0, 0.2, 0.05), 10.0, 0.2, 0.05);
        assert!((one - two).abs() < 1e-4);
        assert!((lerp_angle(3.0, -3.0, 0.5).abs() - std::f32::consts::PI).abs() < 1e-3);
    }

    #[test]
    fn follow_camera_stays_in_front_of_walls_and_shaking_fades() {
        let mut f = Follow::default();
        f.snap(Vec3::ZERO, 0.0);
        for _ in 0..200 {
            f.update(Vec3::ZERO, 0.0, 0.016, |_, _| 0.5);
        }
        assert!(f.pos.z < 2.6 && f.pos.z > 2.3, "{}", f.pos);
        let mut s = Shake::default();
        s.hit(5.0);
        assert!(s.apply(Mat4::IDENTITY, 0.1) != Mat4::IDENTITY);
        for _ in 0..100 {
            s.update(0.016);
        }
        assert_eq!(s.apply(Mat4::IDENTITY, 0.1), Mat4::IDENTITY);
    }
}
