//! Vorhersage: eigene Eingaben sofort rechnen, spaeter mit dem Host abgleichen.

use super::newer;
use glam::Vec3;
use std::collections::VecDeque;

/// So viele unbestaetigte Eingaben werden hoechstens aufgehoben (bei 60/s gut 4 s)
const MAX_PENDING: usize = 256;

/// Eigener Zustand `S` mit noch nicht bestaetigten Eingaben `I` (Mitspieler-Seite).
///
/// Die Rechnung eines Schritts muss bei Host und Mitspieler dieselbe sein.
#[derive(Clone, Debug)]
pub struct Predictor<S, I> {
    state: S,
    pending: VecDeque<(u32, I)>,
    seq: u32,
    acked: u32,
}

impl<S: Clone, I: Clone> Predictor<S, I> {
    pub fn new(state: S) -> Self {
        Self {
            state,
            pending: VecDeque::new(),
            seq: 0,
            acked: 0,
        }
    }

    /// Vorhergesagter Zustand.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Eingabe eines Schritts sofort anwenden; liefert ihre Nummer (ab 1).
    pub fn apply(&mut self, input: I, step: impl FnOnce(&mut S, &I)) -> u32 {
        self.seq = self.seq.wrapping_add(1);
        step(&mut self.state, &input);
        self.pending.push_back((self.seq, input));
        if self.pending.len() > MAX_PENDING {
            self.pending.pop_front();
        }
        self.seq
    }

    /// Die letzten `n` unbestaetigten Eingaben mit Nummer, aelteste zuerst. Jedes Mal alle zu
    /// senden macht verlorene Nachrichten unschaedlich.
    pub fn recent(&self, n: usize) -> impl Iterator<Item = &(u32, I)> {
        self.pending
            .iter()
            .skip(self.pending.len().saturating_sub(n))
    }

    /// Zustand vom Host, in dem alle Eingaben bis `ack` stecken. Rechnet die uebrigen neu
    /// darauf. `false` = veraltete Nachricht, ignoriert.
    pub fn reconcile(&mut self, ack: u32, state: S, mut step: impl FnMut(&mut S, &I)) -> bool {
        if newer(self.acked, ack) || newer(ack, self.seq) {
            return false;
        }
        self.acked = ack;
        while self.pending.front().is_some_and(|(s, _)| !newer(*s, ack)) {
            self.pending.pop_front();
        }
        self.state = state;
        for (_, i) in &self.pending {
            step(&mut self.state, i);
        }
        true
    }
}

/// Eingaben eines Mitspielers der Reihe nach (Host-Seite). Doppelte fallen weg.
#[derive(Clone, Debug)]
pub struct InputQueue<I> {
    queue: VecDeque<(u32, I)>,
    last: u32,
}

impl<I> Default for InputQueue<I> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            last: 0,
        }
    }
}

impl<I> InputQueue<I> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Empfangene Eingabe einsortieren.
    pub fn push(&mut self, seq: u32, input: I) {
        if !newer(seq, self.last) || self.queue.len() >= MAX_PENDING {
            return;
        }
        let i = self.queue.partition_point(|(s, _)| newer(seq, *s));
        if self.queue.get(i).is_none_or(|(s, _)| *s != seq) {
            self.queue.insert(i, (seq, input));
        }
    }

    /// Naechste Eingabe. Eine Luecke (verloren) wird uebersprungen.
    pub fn pop(&mut self) -> Option<I> {
        let (seq, input) = self.queue.pop_front()?;
        self.last = seq;
        Some(input)
    }

    /// Nummer der zuletzt gerechneten Eingabe (als Bestaetigung an den Mitspieler).
    pub fn ack(&self) -> u32 {
        self.last
    }

    /// Wartende Eingaben.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

/// Blendet Korrekturen des Hosts weich aus, statt die Figur springen zu lassen.
#[derive(Clone, Copy, Debug)]
pub struct Smoothing {
    offset: Vec3,
    /// Halbwertszeit (s)
    half_life: f32,
    /// ab hier wird gesprungen (m)
    snap: f32,
}

impl Smoothing {
    pub fn new(half_life: f32, snap: f32) -> Self {
        Self {
            offset: Vec3::ZERO,
            half_life: half_life.max(1e-3),
            snap,
        }
    }
    /// Der gerechnete Ort sprang von `before` nach `after`.
    pub fn corrected(&mut self, before: Vec3, after: Vec3) {
        self.offset += before - after;
        if self.offset.length() > self.snap {
            self.offset = Vec3::ZERO;
        }
    }
    /// Angezeigter Ort; einmal pro Bild.
    pub fn update(&mut self, dt: f32, pos: Vec3) -> Vec3 {
        self.offset *= 0.5f32.powf(dt.max(0.0) / self.half_life);
        pos + self.offset
    }
}
