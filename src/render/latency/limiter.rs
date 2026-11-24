//! Einfache Bildbegrenzung auf dem Prozessor.

use std::time::{Duration, Instant};

/// So kurz vor dem Ziel wird nicht mehr geschlafen, sondern gewartet (Schlafen ist ungenau)
const SPIN: Duration = Duration::from_micros(1500);

/// Haelt einen gleichmaessigen Abstand zwischen Bildern.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Limiter {
    next: Option<Instant>,
}

impl Limiter {
    /// Wann das naechste Bild beginnen darf; `now` = jetzt.
    fn target(&mut self, now: Instant, step: Duration) -> Instant {
        // Zu spaet dran: Takt ab jetzt, nicht aufholen
        let at = self.next.filter(|t| *t + step > now).unwrap_or(now);
        self.next = Some(at + step);
        at
    }

    /// Wartet bis zum naechsten Bild; `fps` 0 = gar nicht.
    pub fn wait(&mut self, fps: f32) {
        if fps <= 0.0 {
            self.next = None;
            return;
        }
        let now = Instant::now();
        let step = Duration::from_secs_f32(1.0 / fps.max(1.0));
        let at = self.target(now, step);
        if let Some(rest) = at.checked_duration_since(now + SPIN) {
            std::thread::sleep(rest);
        }
        while Instant::now() < at {
            std::thread::yield_now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_beat_and_does_not_catch_up() {
        let step = Duration::from_millis(10);
        let mut l = Limiter::default();
        let t = Instant::now();
        assert_eq!(l.target(t, step), t);
        assert_eq!(l.target(t + Duration::from_millis(3), step), t + step);
        let late = t + Duration::from_millis(100);
        assert_eq!(
            l.target(late, step),
            late,
            "nach einem Haenger neu anfangen"
        );
        assert_eq!(l.target(late, step), late + step);
    }

    #[test]
    fn waits_about_as_long_as_asked() {
        let mut l = Limiter::default();
        let t = Instant::now();
        for _ in 0..5 {
            l.wait(200.0);
        }
        assert!(t.elapsed() >= Duration::from_millis(19));
    }
}
