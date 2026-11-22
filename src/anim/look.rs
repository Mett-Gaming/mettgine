//! Kopf und Hals zu einem Blickziel drehen (begrenzter Winkel).

use glam::{Mat4, Quat, Vec3};

/// Dreht die globale Knochen-Matrix `g` um ihren Ursprung, sodass die Blickrichtung
/// (Vorwaerts der Ruhepose) zum Ziel zeigt, seitlich begrenzt auf `limit`, nach oben/unten
/// auf 0.7 rad. `share` = Anteil der Drehung (Hals nur teilweise, Kopf den Rest).
pub(super) fn look_towards(g: Mat4, rest_inv: &Mat4, target: Vec3, limit: f32, share: f32) -> Mat4 {
    let pivot = g.w_axis.truncate();
    // aktuelle Blickrichtung: Ruhepose schaut nach +z; g * rest_inv bildet Ruhe -> jetzt ab
    let now = (g * *rest_inv)
        .transform_vector3(Vec3::Z)
        .normalize_or_zero();
    let want = (target - pivot).normalize_or_zero();
    if now == Vec3::ZERO || want == Vec3::ZERO {
        return g;
    }
    use std::f32::consts::{PI, TAU};
    let wrap = |a: f32| (a + PI).rem_euclid(TAU) - PI;
    let yaw_now = now.x.atan2(now.z);
    // seitlich relativ zum Koerper (vorne = +z) begrenzen, dann den fehlenden Anteil drehen
    let desired = want.x.atan2(want.z).clamp(-limit, limit);
    let dyaw = wrap(desired - yaw_now);
    let pitch_now = now.y.clamp(-1.0, 1.0).asin();
    let dpitch = (want.y.clamp(-1.0, 1.0).asin().clamp(-0.7, 0.7) - pitch_now) * share;
    let yaw_q = Quat::from_rotation_y(dyaw * share);
    let right = (yaw_q * now).cross(Vec3::Y).normalize_or_zero();
    let pitch_q = if right == Vec3::ZERO {
        Quat::IDENTITY
    } else {
        Quat::from_axis_angle(right, -dpitch)
    };
    let r = pitch_q * yaw_q;
    Mat4::from_translation(pivot) * Mat4::from_quat(r) * Mat4::from_translation(-pivot) * g
}
