//! Animationssteuerung fuer neue Spiele: Zustaende mit Uebergaengen, Schichten mit Masken,
//! Ereignisse, Zwei-Knochen-IK. Baut Posen ([`Pose`]) und daraus Skinning-Matrizen.
//!
//! [`super::Animator`] und [`super::PoseFx`] bleiben der einfache Weg.

use super::{AnimSet, Local};
use glam::{Mat4, Quat, Vec3};

/// Lokale Lage aller Knochen.
#[derive(Clone, Debug, Default)]
pub struct Pose {
    pub locals: Vec<Local>,
}

impl Pose {
    /// Clip `clip` (Nummer in der Datei) zur Zeit `t`.
    pub fn sample(&mut self, set: &AnimSet, clip: usize, t: f32) {
        set.sample_locals(clip, t, &mut self.locals);
    }

    /// Zu `other` hin mischen; `weight` 0..1, je Knochen mal `mask`.
    pub fn blend(&mut self, other: &Pose, weight: f32, mask: Option<&Mask>) {
        for (i, (a, b)) in self.locals.iter_mut().zip(&other.locals).enumerate() {
            let w = weight * mask.map_or(1.0, |m| m.weight(i));
            if w > 0.0 {
                *a = a.mix(*b, w.min(1.0));
            }
        }
    }

    /// Unterschied zwischen `add` und `base` dazulegen (Atmen, Treffer, Zielen).
    pub fn add(&mut self, add: &Pose, base: &Pose, weight: f32, mask: Option<&Mask>) {
        let pairs = add.locals.iter().zip(&base.locals);
        for (i, (l, (a, b))) in self.locals.iter_mut().zip(pairs).enumerate() {
            let w = weight * mask.map_or(1.0, |m| m.weight(i));
            if w > 0.0 {
                let turn = b.rot.inverse() * a.rot;
                l.rot = (l.rot * Quat::IDENTITY.slerp(turn, w)).normalize();
                l.pos += (a.pos - b.pos) * w;
            }
        }
    }

    /// Lage aller Knochen in Figuren-Koordinaten.
    pub fn globals(&self, set: &AnimSet) -> Vec<Mat4> {
        let mut out: Vec<Mat4> = Vec::with_capacity(self.locals.len());
        for (l, &p) in self.locals.iter().zip(&set.parents) {
            let parent = usize::try_from(p).map_or(Mat4::IDENTITY, |p| out[p]);
            out.push(parent * l.mat());
        }
        out
    }

    /// Skinning-Matrizen an `out` anhaengen.
    pub fn skin(&self, set: &AnimSet, out: &mut Vec<Mat4>) {
        let globals = self.globals(set);
        out.extend(globals.iter().zip(&set.rest_inv).map(|(g, r)| *g * *r));
    }

    /// Arm oder Bein so beugen, dass der Knochen `end` das Ziel (Figuren-Koordinaten) erreicht.
    /// `mid` ist sein Eltern-Knochen, `root` dessen Eltern-Knochen; `pole` gibt an, wohin das
    /// Gelenk zeigt. `false` = diese Knochen haengen nicht zusammen.
    pub fn two_bone_ik(
        &mut self,
        set: &AnimSet,
        [root, mid, end]: [usize; 3],
        target: Vec3,
        pole: Vec3,
        weight: f32,
    ) -> bool {
        let chain =
            |child: usize, parent: usize| set.parents.get(child).copied() == Some(parent as i32);
        if !chain(end, mid) || !chain(mid, root) || end >= self.locals.len() {
            return false;
        }
        let g = self.globals(set);
        let at = |b: usize| g[b].w_axis.truncate();
        let (a, b, c) = (at(root), at(mid), at(end));
        let (ab, bc) = (a.distance(b), b.distance(c));
        let want = target - a;
        let reach = want.length().clamp((ab - bc).abs() + 1e-4, ab + bc - 1e-4);
        let dir = want.normalize_or(Vec3::Y);
        // Gelenk: Abstand entlang der Richtung und seitlich zum Pol
        let along = (ab * ab - bc * bc + reach * reach) / (2.0 * reach);
        let off = (ab * ab - along * along).max(0.0).sqrt();
        let side = (pole - a) - dir * (pole - a).dot(dir);
        let side = side.normalize_or(dir.any_orthonormal_vector());
        let new_mid = a + dir * along + side * off;
        let new_end = a + dir * reach;
        self.turn(set, &g, root, b - a, new_mid - a, weight);
        let g = self.globals(set);
        let (b, c) = (g[mid].w_axis.truncate(), g[end].w_axis.truncate());
        let goal = b + (new_end - new_mid);
        self.turn(set, &g, mid, c - b, goal - b, weight);
        true
    }

    /// Knochen so drehen, dass `from` nach `to` zeigt (Figuren-Koordinaten).
    fn turn(&mut self, set: &AnimSet, g: &[Mat4], bone: usize, from: Vec3, to: Vec3, w: f32) {
        let (from, to) = (from.normalize_or_zero(), to.normalize_or_zero());
        if from == Vec3::ZERO || to == Vec3::ZERO {
            return;
        }
        let q = Quat::IDENTITY.slerp(Quat::from_rotation_arc(from, to), w.clamp(0.0, 1.0));
        let parent = usize::try_from(set.parents[bone])
            .map_or(Quat::IDENTITY, |p| g[p].to_scale_rotation_translation().1);
        let l = &mut self.locals[bone];
        l.rot = (parent.inverse() * q * parent * l.rot).normalize();
    }
}

/// Gewicht je Knochen (z. B. nur Oberkoerper).
#[derive(Clone, Debug, PartialEq)]
pub struct Mask(Vec<f32>);

impl Mask {
    /// Knochen `root` und alles darunter mit 1, der Rest 0.
    pub fn below(set: &AnimSet, root: &str) -> Option<Self> {
        let r = set.bone(root)?;
        let mut w = vec![0.0; set.bones()];
        w[r] = 1.0;
        for (i, &p) in set.parents.iter().enumerate() {
            if usize::try_from(p).is_ok_and(|p| w[p] > 0.0) {
                w[i] = 1.0;
            }
        }
        Some(Self(w))
    }

    fn weight(&self, bone: usize) -> f32 {
        self.0.get(bone).copied().unwrap_or(0.0)
    }
}

/// Ein Zustand: spielt einen Clip.
#[derive(Clone, Debug)]
pub struct AnimState {
    /// Nummer des Clips in der Datei
    pub clip: usize,
    pub speed: f32,
    pub looped: bool,
    /// Zeitpunkte (Sekunden im Clip), die gemeldet werden (Schritt, Schlag)
    pub events: Vec<(f32, u32)>,
    /// Zustand, der nach einem Einmal-Clip folgt
    pub then: Option<usize>,
}

impl AnimState {
    /// Schleife in normaler Geschwindigkeit.
    pub fn looped(clip: usize) -> Self {
        Self {
            clip,
            speed: 1.0,
            looped: true,
            events: Vec::new(),
            then: None,
        }
    }

    /// Einmal abspielen, danach weiter mit Zustand `then`.
    pub fn once(clip: usize, then: usize) -> Self {
        Self {
            looped: false,
            then: Some(then),
            ..Self::looped(clip)
        }
    }

    pub fn event(mut self, time: f32, id: u32) -> Self {
        self.events.push((time, id));
        self
    }
}

/// Zustandsmaschine einer Figur: ein Zustand, beim Wechsel wird ueberblendet.
#[derive(Clone, Debug)]
pub struct StateMachine {
    states: Vec<AnimState>,
    now: usize,
    time: f32,
    /// vorheriger Zustand, seine Zeit, Dauer und Fortschritt der Blende
    prev: Option<(usize, f32, f32, f32)>,
    fired: Vec<u32>,
}

impl StateMachine {
    /// Beginnt mit Zustand 0.
    pub fn new(states: Vec<AnimState>) -> Self {
        Self {
            states,
            now: 0,
            time: 0.0,
            prev: None,
            fired: Vec::new(),
        }
    }

    pub fn state(&self) -> usize {
        self.now
    }

    /// Zustand wechseln (`fade` Sekunden ueberblenden); der laufende laeuft weiter.
    pub fn go(&mut self, state: usize, fade: f32) {
        if state != self.now {
            self.restart(state, fade);
        }
    }

    /// Wie [`StateMachine::go`], beginnt aber auch den laufenden von vorn.
    pub fn restart(&mut self, state: usize, fade: f32) {
        if state < self.states.len() {
            self.prev = (fade > 0.0).then_some((self.now, self.time, fade, 0.0));
            self.now = state;
            self.time = 0.0;
        }
    }

    /// Geschwindigkeit eines Zustands aendern (Laufen nach Tempo).
    pub fn set_speed(&mut self, state: usize, speed: f32) {
        if let Some(s) = self.states.get_mut(state) {
            s.speed = speed;
        }
    }

    /// Zeit vergehen lassen; liefert die Ereignisse dieses Schritts.
    pub fn update(&mut self, set: &AnimSet, dt: f32) -> &[u32] {
        self.fired.clear();
        if let Some((s, t, len, p)) = &mut self.prev {
            *t += dt * self.states.get(*s).map_or(0.0, |s| s.speed);
            *p += dt;
            if *p >= *len {
                self.prev = None;
            }
        }
        let Some(s) = self.states.get(self.now) else {
            return &self.fired;
        };
        let (from, to) = (self.time, self.time + dt * s.speed);
        let len = set.clip_seconds(s.clip).max(1e-3);
        for &(at, id) in &s.events {
            // bei Schleifen zaehlt jeder Durchlauf
            let hit = if s.looped {
                ((to - at) / len).floor() > ((from - at) / len).floor() && to >= at
            } else {
                (from < at || (from == 0.0 && at == 0.0)) && to >= at
            };
            if hit {
                self.fired.push(id);
            }
        }
        self.time = to;
        if !s.looped && to >= len {
            if let Some(n) = s.then {
                self.go(n, 0.15);
            }
        }
        &self.fired
    }

    /// Pose des Augenblicks.
    pub fn pose(&self, set: &AnimSet) -> Pose {
        let sample = |s: usize, t: f32| {
            let mut p = Pose::default();
            let st = &self.states[s];
            let len = set.clip_seconds(st.clip);
            let t = if st.looped {
                t
            } else {
                t.min((len - 1e-3).max(0.0))
            };
            p.sample(set, st.clip, t);
            p
        };
        if self.states.is_empty() {
            return Pose::default();
        }
        let mut pose = sample(self.now, self.time);
        if let Some((s, t, len, p)) = self.prev {
            let w = (1.0 - p / len).clamp(0.0, 1.0);
            // weicher Anfang und Ende
            pose.blend(&sample(s, t), w * w * (3.0 - 2.0 * w), None);
        }
        pose
    }
}

#[cfg(test)]
mod tests {
    use super::super::ClipData;
    use super::*;

    /// Kette aus vier Knochen entlang y; Clip 0 ruht, Clip 1 dreht Knochen 1.
    fn set() -> AnimSet {
        let rest = Local {
            rot: Quat::IDENTITY,
            pos: Vec3::Y,
            scale: Vec3::ONE,
        };
        let clip = |turn: f32| ClipData {
            fps: 10.0,
            frames: 10,
            looped: true,
            locals: (0..40)
                .map(|i| Local {
                    rot: if i % 4 == 1 {
                        Quat::from_rotation_x(turn)
                    } else {
                        Quat::IDENTITY
                    },
                    ..rest
                })
                .collect(),
        };
        AnimSet::new(
            ["A", "B", "C", "D"].map(String::from).to_vec(),
            vec![-1, 0, 1, 2],
            (1..5)
                .map(|i| Mat4::from_translation(Vec3::Y * -(i as f32)))
                .collect(),
            vec![clip(0.0), clip(1.0)],
            vec!["ruhe".into(), "drehen".into()],
        )
    }

    #[test]
    fn masks_limit_layers_to_part_of_the_body() {
        let s = set();
        let (mut a, mut b) = (Pose::default(), Pose::default());
        a.sample(&s, 0, 0.0);
        b.sample(&s, 1, 0.0);
        let upper = Mask::below(&s, "C").unwrap();
        a.blend(&b, 1.0, Some(&upper));
        assert!(a.locals[1].rot.abs_diff_eq(Quat::IDENTITY, 1e-5));
        a.blend(&b, 0.5, None);
        assert!((a.locals[1].rot.to_axis_angle().1 - 0.5).abs() < 1e-3);
        assert!(Mask::below(&s, "fehlt").is_none());
        let mut add = Pose::default();
        add.sample(&s, 0, 0.0);
        let base = add.clone();
        add.add(&b, &base, 1.0, None);
        assert!((add.locals[1].rot.to_axis_angle().1 - 1.0).abs() < 1e-3);
    }

    #[test]
    fn hands_reach_their_target() {
        let s = set();
        let mut p = Pose::default();
        p.sample(&s, 0, 0.0);
        let target = Vec3::new(0.6, 2.9, 0.5);
        assert!(p.two_bone_ik(&s, [1, 2, 3], target, Vec3::new(0.0, 3.0, 5.0), 1.0));
        let g = p.globals(&s);
        assert!(g[3].w_axis.truncate().distance(target) < 0.02);
        assert!(g[2].w_axis.z > 0.0, "Gelenk zeigt zum Pol");
        assert!(!p.two_bone_ik(&s, [0, 2, 3], target, Vec3::Z, 1.0));
        // unerreichbar: streckt sich hin, ohne kaputtzugehen
        assert!(p.two_bone_ik(&s, [1, 2, 3], Vec3::new(50.0, 2.0, 0.0), Vec3::Z, 1.0));
        assert!(p.globals(&s)[3].is_finite());
    }

    #[test]
    fn states_fade_report_events_and_continue() {
        let s = set();
        let mut m = StateMachine::new(vec![
            AnimState::looped(0).event(0.5, 7),
            AnimState::once(1, 0).event(0.2, 9),
        ]);
        let mut hits = 0;
        for _ in 0..30 {
            hits += m.update(&s, 0.1).iter().filter(|e| **e == 7).count();
        }
        assert_eq!(hits, 3, "ein Ereignis je Durchlauf");
        m.go(1, 0.2);
        m.update(&s, 0.1);
        let turn = m.pose(&s).locals[1].rot.to_axis_angle().1;
        assert!(turn > 0.1 && turn < 0.95, "mitten in der Blende: {turn}");
        let mut strike = 0;
        for _ in 0..12 {
            strike += m.update(&s, 0.1).iter().filter(|e| **e == 9).count();
        }
        assert_eq!((strike, m.state()), (1, 0), "danach wieder Zustand 0");
        let mut out = Vec::new();
        m.pose(&s).skin(&s, &mut out);
        assert_eq!(out.len(), 4);
    }
}
