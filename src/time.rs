//! Zeit im Spiel: Bildzeit mit Obergrenze und fester Takt.

use std::time::Instant;

/// Misst die Zeit zwischen zwei Bildern. Nach einem Haenger (Laden, Fenster verschoben) zaehlt
/// hoechstens `max_dt`, damit nichts durch Waende springt.
#[derive(Clone, Copy, Debug)]
pub struct FrameClock {
    last: Instant,
    max_dt: f32,
}

impl FrameClock {
    /// `max_dt` in Sekunden.
    pub fn new(max_dt: f32) -> Self {
        Self {
            last: Instant::now(),
            max_dt,
        }
    }

    /// Sekunden seit dem letzten Aufruf, hoechstens `max_dt`.
    pub fn tick(&mut self) -> f32 {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(self.max_dt);
        self.last = now;
        dt
    }

    /// Ab jetzt neu messen (nach langem Laden).
    pub fn restart(&mut self) {
        self.last = Instant::now();
    }
}

/// Fester Takt: Regeln laufen immer mit derselben Schrittweite, egal wie schnell gezeichnet wird.
///
/// ```
/// let mut takt = mettgine::time::FixedStep::new(1.0 / 60.0);
/// takt.add(0.05);
/// while takt.step() {
///     // Spielregeln mit takt.dt() rechnen
/// }
/// // Zeichnen: zwischen letztem und vorletztem Stand mit takt.alpha() mischen
/// ```
#[derive(Clone, Copy, Debug)]
pub struct FixedStep {
    dt: f32,
    rest: f32,
}

impl FixedStep {
    /// `dt` in Sekunden.
    pub fn new(dt: f32) -> Self {
        Self { dt, rest: 0.0 }
    }

    pub fn dt(&self) -> f32 {
        self.dt
    }

    /// Vergangene Zeit dazulegen.
    pub fn add(&mut self, seconds: f32) {
        self.rest += seconds;
    }

    /// `true` = ein Schritt ist faellig (und damit verbraucht).
    pub fn step(&mut self) -> bool {
        if self.rest < self.dt {
            return false;
        }
        self.rest -= self.dt;
        true
    }

    /// Wie weit der naechste Schritt schon ist (0..1), zum Mischen zwischen zwei Staenden.
    pub fn alpha(&self) -> f32 {
        (self.rest / self.dt).clamp(0.0, 1.0)
    }

    /// Angesparte Zeit vergessen (neue Runde, Pause).
    pub fn reset(&mut self) {
        self.rest = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_steps_do_not_lose_time() {
        let mut f = FixedStep::new(0.25);
        let mut steps = 0;
        for _ in 0..10 {
            f.add(0.1);
            while f.step() {
                steps += 1;
            }
        }
        // 1,0 s angesammelt; die letzte Rundung darf einen Schritt offen lassen
        assert!((3..=4).contains(&steps));
        assert!((0.0..=1.0).contains(&f.alpha()));
        f.reset();
        assert_eq!(f.alpha(), 0.0);
        assert!(!f.step());
    }

    #[test]
    fn long_pauses_are_cut_off() {
        let mut c = FrameClock::new(0.02);
        std::thread::sleep(std::time::Duration::from_millis(40));
        assert_eq!(c.tick(), 0.02);
    }
}
