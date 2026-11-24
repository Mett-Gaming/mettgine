//! Knochen-Animation mit Hierarchie (.k0a v2, Knochennamen wie bei den Quaternius-Figuren): lokale Transformationen je Bild, zwischen
//! Bildern und Clips per slerp gemischt, danach Eingriffe pro Knochen (Kopf dreht sich zu
//! einem Ziel, Zucken, gestreckte Glieder, Stop-Motion). Ergebnis: Skinning-Matrizen.

use glam::{Mat4, Quat, Vec3};

/// Kennung eines Clips. Das Spiel legt seine Clips als Aufzaehlung fest (Reihenfolge wie in
/// der .k0a-Datei) und sagt, welche davon Fortbewegung sind und welche im Stand weiterlaufen.
pub trait ClipId: Copy + PartialEq + Default + std::fmt::Debug {
    /// Position in der .k0a-Datei
    fn index(self) -> usize;
    /// Fortbewegung: die Zeit ist an die zurueckgelegte Strecke gekoppelt
    fn locomotion(self) -> bool {
        false
    }
    /// laeuft im Stand als Schleife (sonst Einmal-Clip, zaehlt ab seinem Start)
    fn looping(self) -> bool {
        true
    }
}

/// Lokale Transformation eines Knochens relativ zu seinem Eltern-Knochen.
#[derive(Clone, Copy, Debug)]
pub struct Local {
    pub rot: Quat,
    pub pos: Vec3,
    pub scale: Vec3,
}

impl Local {
    fn mix(self, o: Local, w: f32) -> Local {
        Local {
            rot: self.rot.slerp(o.rot, w),
            pos: self.pos.lerp(o.pos, w),
            scale: self.scale.lerp(o.scale, w),
        }
    }
    fn mat(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rot, self.pos)
    }
}

pub(crate) struct ClipData {
    pub fps: f32,
    pub frames: usize,
    pub looped: bool,
    /// frames * bones
    pub locals: Vec<Local>,
}

/// Eingriffe in die Pose (alles optional, Standard = nichts).
#[derive(Clone, Copy, Debug, Default)]
pub struct PoseFx<C> {
    /// Ueberblenden: vorheriger Clip, dessen Zeit, Gewicht (1 = nur der alte, 0 = nur der neue)
    pub blend: Option<(C, f32, f32)>,
    /// Blickziel des Kopfes in Figuren-Koordinaten (vorne = +z, Fuesse bei y = 0)
    pub look: Option<Vec3>,
    /// wie weit sich der Kopf zur Seite drehen darf (Radiant; ueber 1.6 = unmenschlich)
    pub look_limit: f32,
    /// Zucken 0..1: kurze Rucke in Kopf, Hals und Armen
    pub twitch: f32,
    /// Strecken 0..1: Hals und Arme werden laenger
    pub stretch: f32,
    /// Stop-Motion: Bilder pro Sekunde (0 = fluessig)
    pub step_fps: f32,
    /// Zufallswert der Figur (fuer Zucken)
    pub seed: f32,
    /// Kopf um die Blickachse gedreht (Radiant; PI = kopfueber)
    pub head_roll: f32,
    /// Zerreissen der Figur im Shader (0..1)
    pub distort: f32,
}

pub struct AnimSet {
    names: Vec<String>,
    parents: Vec<i32>,
    rest_inv: Vec<Mat4>,
    clips: Vec<ClipData>,
    clip_names: Vec<String>,
    head: Option<usize>,
    neck: Option<usize>,
    arms: Vec<usize>,
    twitchy: Vec<usize>,
}

fn hash(x: f32) -> f32 {
    ((x * 12.9898).sin() * 43_758.547).fract()
}

impl AnimSet {
    pub(crate) fn new(
        names: Vec<String>,
        parents: Vec<i32>,
        rest_inv: Vec<Mat4>,
        clips: Vec<ClipData>,
        clip_names: Vec<String>,
    ) -> Self {
        let find = |n: &str| names.iter().position(|x| x == n);
        let all = |list: &[&str]| list.iter().filter_map(|n| find(n)).collect();
        Self {
            head: find("Head"),
            neck: find("Neck"),
            arms: all(&["UpperArm.L", "UpperArm.R", "LowerArm.L", "LowerArm.R"]),
            twitchy: all(&["Neck", "Head", "UpperArm.L", "UpperArm.R", "Chest"]),
            names,
            parents,
            rest_inv,
            clips,
            clip_names,
        }
    }

    pub fn bones(&self) -> usize {
        self.parents.len()
    }

    pub fn bone(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|x| x == name)
    }

    pub fn has_clip<C: ClipId>(&self, clip: C) -> bool {
        clip.index() < self.clips.len()
    }

    /// Name eines Clips in der Datei (zum Pruefen der Reihenfolge).
    pub fn clip_name<C: ClipId>(&self, clip: C) -> &str {
        self.clip_names.get(clip.index()).map_or("", |n| n.as_str())
    }

    /// Clip in der Datei; unbekannte Nummern meinen den letzten. Nie leer (siehe `file`).
    fn clip(&self, index: usize) -> Option<&ClipData> {
        self.clips.get(index).or(self.clips.last())
    }

    /// Dauer eines Clips in Sekunden (0 = die Datei hat keine Clips).
    pub fn duration<C: ClipId>(&self, clip: C) -> f32 {
        self.clip(clip.index())
            .map_or(0.0, |c| c.frames as f32 / c.fps)
    }

    /// Lokale Pose eines Clips zur Zeit `t` (Schleife oder am Ende stehen bleiben).
    fn sample_locals(&self, clip: usize, t: f32, out: &mut Vec<Local>) {
        let n = self.bones();
        out.clear();
        let Some(c) = self.clip(clip) else {
            // Datei ohne Clips: Ruhehaltung
            let rest = self.rest_inv.iter().map(|m| m.inverse());
            let globals: Vec<Mat4> = rest.collect();
            out.extend(globals.iter().zip(&self.parents).map(|(g, &p)| {
                let parent = usize::try_from(p).map_or(Mat4::IDENTITY, |p| globals[p]);
                let (scale, rot, pos) = (parent.inverse() * *g).to_scale_rotation_translation();
                Local { rot, pos, scale }
            }));
            return;
        };
        let pos = if c.looped {
            (t * c.fps).rem_euclid(c.frames as f32)
        } else {
            (t * c.fps).clamp(0.0, (c.frames - 1) as f32)
        };
        let f0 = pos.floor() as usize % c.frames;
        let f1 = if c.looped {
            (f0 + 1) % c.frames
        } else {
            (f0 + 1).min(c.frames - 1)
        };
        let a = pos.fract();
        for b in 0..n {
            out.push(c.locals[f0 * n + b].mix(c.locals[f1 * n + b], a));
        }
    }

    /// Skinning-Matrizen ohne Eingriffe (fuer Tests und einfache Figuren).
    pub fn sample<C: ClipId>(&self, clip: C, t: f32, out: &mut Vec<Mat4>) {
        self.pose(clip, t, &PoseFx::default(), out);
    }

    /// Skinning-Matrizen fuer Clip `clip` zur Zeit `t` mit allen Eingriffen aus `fx`.
    pub fn pose<C: ClipId>(&self, clip: C, t: f32, fx: &PoseFx<C>, out: &mut Vec<Mat4>) {
        let n = self.bones();
        let step = |t: f32| {
            if fx.step_fps > 0.0 {
                (t * fx.step_fps).floor() / fx.step_fps
            } else {
                t
            }
        };
        let mut locals = Vec::with_capacity(n);
        self.sample_locals(clip.index(), step(t), &mut locals);
        if let Some((prev, pt, w)) = fx.blend {
            if w > 0.0 {
                let mut other = Vec::with_capacity(n);
                self.sample_locals(prev.index(), step(pt), &mut other);
                for (l, o) in locals.iter_mut().zip(&other) {
                    *l = l.mix(*o, w.min(1.0));
                }
            }
        }
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
        // Hierarchie durchrechnen; Kopf und Hals drehen sich zum Blickziel
        let mut global: Vec<Mat4> = Vec::with_capacity(n);
        for b in 0..n {
            let mut g = match self.parents[b] {
                p if p >= 0 => global[p as usize] * locals[b].mat(),
                _ => locals[b].mat(),
            };
            if let Some(target) = fx.look {
                let share = if Some(b) == self.neck {
                    0.35
                } else if Some(b) == self.head {
                    1.0
                } else {
                    0.0
                };
                if share > 0.0 {
                    g = look_towards(g, &self.rest_inv[b], target, fx.look_limit, share);
                }
            }
            if fx.head_roll != 0.0 && Some(b) == self.head {
                // Kopf um die eigene Blickachse drehen (bis kopfueber)
                let pivot = g.w_axis.truncate();
                let axis = (g * self.rest_inv[b])
                    .transform_vector3(Vec3::Z)
                    .normalize_or_zero();
                if axis != Vec3::ZERO {
                    g = Mat4::from_translation(pivot)
                        * Mat4::from_quat(Quat::from_axis_angle(axis, fx.head_roll))
                        * Mat4::from_translation(-pivot)
                        * g;
                }
            }
            global.push(g);
        }
        out.extend(global.iter().zip(&self.rest_inv).map(|(g, r)| *g * *r));
    }
}

mod animator;
mod file;
mod look;
pub use animator::Animator;
pub use file::parse_anim;
use look::look_towards;

#[cfg(test)]
mod tests;
