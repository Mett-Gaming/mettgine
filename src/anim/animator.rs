//! Clip-Wechsel mit Ueberblenden pro Figur.

use super::ClipId;

/// Clip-Wechsel mit Ueberblenden, pro Figur. Die Zeit liefert der Aufrufer (bei Laufclips an
/// die Strecke gekoppelt, sonst normale Zeit); Einmal-Clips zaehlen ab ihrem Start.
#[derive(Clone, Copy, Debug, Default)]
pub struct Animator<C> {
    pub clip: C,
    /// Zeit beim Start des aktuellen Clips
    start: f32,
    /// zuletzt gemeldete Zeit
    now: f32,
    prev: Option<(C, f32)>,
    fade: f32,
    fade_len: f32,
}

impl<C: ClipId> Animator<C> {
    /// Gewuenschten Clip melden. `time` laeuft stetig (Sekunden), `fade` = Ueberblendzeit.
    pub fn update(&mut self, clip: C, time: f32, dt: f32, fade: f32) {
        if clip != self.clip {
            self.prev = Some((self.clip, self.clip_time()));
            self.clip = clip;
            self.start = time;
            self.fade = 0.0;
            self.fade_len = fade.max(1e-3);
        } else if let Some((_, t)) = &mut self.prev {
            *t += dt;
        }
        self.now = time;
        self.fade += dt;
        if self.fade >= self.fade_len {
            self.prev = None;
        }
    }

    /// Zeit im aktuellen Clip (Schleifen: stetige Zeit, Einmal-Clips: ab Start).
    pub fn clip_time(&self) -> f32 {
        if self.clip.locomotion() || self.clip.looping() {
            self.now
        } else {
            self.now - self.start
        }
    }

    /// Ueberblend-Angabe fuer PoseFx.
    pub fn blend(&self) -> Option<(C, f32, f32)> {
        self.prev
            .map(|(c, t)| (c, t, (1.0 - self.fade / self.fade_len).clamp(0.0, 1.0)))
            .filter(|b| b.2 > 0.0)
    }

    /// Einmal-Clip von vorn (z. B. dieselbe Ware immer wieder nehmen).
    pub fn restart(&mut self) {
        self.start = self.now;
    }

    /// Ist ein Einmal-Clip durchgelaufen?
    pub fn finished(&self, duration: f32) -> bool {
        !self.clip.looping() && !self.clip.locomotion() && self.now - self.start >= duration
    }
}
