//! Zwischenbilder: Zustaende mit Zeitstempel sammeln und dazwischen fluessig anzeigen.

use glam::{Quat, Vec2, Vec3};
use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

/// Etwas, das sich zwischen zwei Zustaenden mischen laesst (`t` = 0..1, beim Weiterrechnen
/// etwas darueber).
pub trait Blend {
    fn blend(&self, to: &Self, t: f32) -> Self;
}

impl Blend for f32 {
    fn blend(&self, to: &Self, t: f32) -> Self {
        self + (to - self) * t
    }
}
impl Blend for Vec2 {
    fn blend(&self, to: &Self, t: f32) -> Self {
        *self + (*to - *self) * t
    }
}
impl Blend for Vec3 {
    fn blend(&self, to: &Self, t: f32) -> Self {
        *self + (*to - *self) * t
    }
}
impl Blend for Quat {
    fn blend(&self, to: &Self, t: f32) -> Self {
        self.slerp(*to, t.clamp(0.0, 1.0)).normalize()
    }
}
impl<A: Blend, B: Blend> Blend for (A, B) {
    fn blend(&self, to: &Self, t: f32) -> Self {
        (self.0.blend(&to.0, t), self.1.blend(&to.1, t))
    }
}

/// Winkel (Radiant): mischt auf dem kuerzeren Weg herum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Angle(pub f32);

impl Blend for Angle {
    fn blend(&self, to: &Self, t: f32) -> Self {
        let d = (to.0 - self.0 + PI).rem_euclid(TAU) - PI;
        Angle(self.0 + d * t)
    }
}

/// So viele Zustaende werden hoechstens aufgehoben
const KEEP: usize = 64;

/// Empfangene Zustaende eines Dings, nach Zeit geordnet.
#[derive(Clone, Debug)]
pub struct Snapshots<T> {
    list: VecDeque<(f64, T)>,
    /// so lange wird hoechstens weitergerechnet, wenn nichts Neues kommt (s)
    max_ahead: f64,
}

impl<T: Blend + Clone> Snapshots<T> {
    pub fn new(max_ahead: f64) -> Self {
        Self {
            list: VecDeque::new(),
            max_ahead: max_ahead.max(0.0),
        }
    }

    /// Zustand zur Host-Zeit `time`. Verspaetete werden einsortiert, doppelte ersetzt.
    pub fn push(&mut self, time: f64, state: T) {
        let i = self.list.partition_point(|(t, _)| *t < time);
        match self.list.get_mut(i) {
            Some(e) if e.0 == time => e.1 = state,
            _ => self.list.insert(i, (time, state)),
        }
        if self.list.len() > KEEP {
            self.list.pop_front();
        }
    }

    /// Alles vergessen (z. B. nach einem Sprung an einen anderen Ort).
    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// Neuester Zustand.
    pub fn latest(&self) -> Option<&T> {
        self.list.back().map(|e| &e.1)
    }

    /// Zustand zur Anzeigezeit `time`; vor dem ersten Zustand der erste, nach dem letzten kurz
    /// weitergerechnet, dann stehen geblieben.
    pub fn sample(&self, time: f64) -> Option<T> {
        let i = self.list.partition_point(|(t, _)| *t <= time);
        if i == 0 {
            return self.list.front().map(|e| e.1.clone());
        }
        let n = self.list.len();
        if i < n {
            let ((t0, a), (t1, b)) = (&self.list[i - 1], &self.list[i]);
            return Some(a.blend(b, ((time - t0) / (t1 - t0)) as f32));
        }
        if n == 1 {
            return self.latest().cloned();
        }
        let ((t0, a), (t1, b)) = (&self.list[n - 2], &self.list[n - 1]);
        let ahead = (time - t1).min(self.max_ahead);
        Some(a.blend(b, (1.0 + ahead / (t1 - t0)) as f32))
    }
}
