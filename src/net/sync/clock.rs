//! Fester Takt und geschaetzte Uhr des Hosts.

/// Zerlegt die Bildzeit in feste Schritte.
#[derive(Clone, Debug)]
pub struct Ticker {
    step: f64,
    rest: f64,
    tick: u32,
    max_steps: u32,
}

impl Ticker {
    /// `rate` Schritte pro Sekunde.
    pub fn new(rate: f64) -> Self {
        Self {
            step: 1.0 / rate.max(1.0),
            rest: 0.0,
            tick: 0,
            max_steps: 8,
        }
    }
    /// Laenge eines Schritts (s).
    pub fn step(&self) -> f32 {
        self.step as f32
    }
    /// Nummer des letzten Schritts.
    pub fn tick(&self) -> u32 {
        self.tick
    }
    /// So viele Schritte sind nach `dt` Sekunden faellig. Nach einem langen Haenger werden
    /// hoechstens 8 nachgeholt, der Rest verfaellt.
    pub fn advance(&mut self, dt: f64) -> u32 {
        self.rest += dt.max(0.0);
        // kleine Reserve, damit Rundung keinen faelligen Schritt verschluckt
        let mut n = (self.rest / self.step + 1e-9) as u32;
        self.rest = (self.rest - n as f64 * self.step).max(0.0);
        if n > self.max_steps {
            n = self.max_steps;
            self.rest = 0.0;
        }
        self.tick = self.tick.wrapping_add(n);
        n
    }
    /// Anteil bis zum naechsten Schritt (0..1), zum Zeichnen zwischen zwei Schritten.
    pub fn fraction(&self) -> f32 {
        (self.rest / self.step) as f32
    }
}

/// Wie schnell die Uhr neue Messungen uebernimmt
const FOLLOW: f64 = 0.1;
/// Abweichung, ab der die Uhr springt statt gleitet (s)
const JUMP: f64 = 0.5;

/// Uhr des Hosts aus Sicht eines Mitspielers, geschaetzt aus Zeitstempeln in Nachrichten.
#[derive(Clone, Debug, Default)]
pub struct RemoteClock {
    offset: Option<f64>,
    jitter: f64,
}

impl RemoteClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Nachricht mit Host-Zeit `remote` ist zur eigenen Zeit `local` angekommen (s).
    pub fn observe(&mut self, local: f64, remote: f64) {
        let new = remote - local;
        match self.offset {
            Some(o) if (new - o).abs() < JUMP => {
                self.jitter += ((new - o).abs() - self.jitter) * FOLLOW;
                self.offset = Some(o + (new - o) * FOLLOW);
            }
            _ => {
                self.offset = Some(new);
                self.jitter = 0.0;
            }
        }
    }

    /// Host-Zeit zur eigenen Zeit `local` (um die Laufzeit der Nachrichten verzoegert).
    pub fn now(&self, local: f64) -> Option<f64> {
        self.offset.map(|o| local + o)
    }

    /// Mittlere Schwankung der Ankunft (s).
    pub fn jitter(&self) -> f64 {
        self.jitter
    }

    /// Anzeigezeit fuer [`super::interp::Snapshots`]: so weit zurueck, dass bei `interval`
    /// Sekunden Sendeabstand fast immer schon die naechste Nachricht da ist.
    pub fn display_time(&self, local: f64, interval: f64) -> Option<f64> {
        let delay = interval * 1.5 + self.jitter * 3.0;
        self.now(local).map(|t| t - delay)
    }
}
