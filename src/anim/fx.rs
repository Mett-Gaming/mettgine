//! Unheimliche Eingriffe in die Pose (aus [`PoseFx`]): Zucken, Strecken, gedrehter Kopf. Ohne
//! Werte in `PoseFx` kosten sie nichts.

use super::{AnimSet, Local, PoseFx};
use glam::{Mat4, Quat, Vec3};

fn hash(x: f32) -> f32 {
    ((x * 12.9898).sin() * 43_758.547).fract()
}

impl AnimSet {
    /// Zucken und Strecken.
    pub(super) fn distort<C>(&self, t: f32, fx: &PoseFx<C>, locals: &mut [Local]) {
        // Zucken: kurze, harte Rucke (wechseln mehrmals pro Sekunde)
        if fx.twitch > 0.0 {
            let slot = (t * 7.0).floor();
            for (k, &b) in self.twitchy.iter().enumerate() {
                let h = hash(slot + fx.seed * 3.1 + k as f32 * 1.7);
                if h > 1.0 - 0.35 * fx.twitch {
                    let ax = Vec3::new(
                        hash(h + 1.0) - 0.5,
                        hash(h + 2.0) - 0.5,
                        hash(h + 3.0) - 0.5,
                    )
                    .normalize_or_zero();
                    locals[b].rot *= Quat::from_axis_angle(ax, 0.6 * fx.twitch);
                }
            }
        }
        // Strecken: Hals und Arme laenger (Knochen-Achse = lokales y)
        if fx.stretch > 0.0 {
            if let Some(nk) = self.neck {
                locals[nk].scale.y *= 1.0 + 1.2 * fx.stretch;
            }
            for &a in &self.arms {
                locals[a].scale.y *= 1.0 + 0.35 * fx.stretch;
            }
        }
    }
}

/// Kopf um die eigene Blickachse drehen (bis kopfueber).
pub(super) fn roll_head(g: Mat4, rest_inv: &Mat4, angle: f32) -> Mat4 {
    let pivot = g.w_axis.truncate();
    let axis = (g * *rest_inv)
        .transform_vector3(Vec3::Z)
        .normalize_or_zero();
    if axis == Vec3::ZERO {
        return g;
    }
    Mat4::from_translation(pivot)
        * Mat4::from_quat(Quat::from_axis_angle(axis, angle))
        * Mat4::from_translation(-pivot)
        * g
}
