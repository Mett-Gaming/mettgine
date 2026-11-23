//! Raumklang: Richtung, Entfernung, Dumpfheit und Doppler fuer Klaenge in der Spielwelt.
//!
//! [`spatialize`] rechnet aus Hoerer und Quelle die [`Mix`]-Werte; [`SpatialSource`] spielt den
//! Klang damit in Stereo. Die Werte duerfen sich waehrend des Abspielens aendern (Atomics) und
//! werden geglaettet, damit nichts knackt.

use super::{Sound, RATE};
use glam::Vec3;
use rodio::Source;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Duration;

/// Schallgeschwindigkeit (m/s)
const SPEED_OF_SOUND: f32 = 343.0;
/// Offener Tiefpass (Hz): darueber hoert man keinen Unterschied
const OPEN_HZ: f32 = 18_000.0;
/// Dumpfster Tiefpass (Hz)
const MUFFLED_HZ: f32 = 700.0;
/// Glaettung der Werte (s)
const SMOOTH: f32 = 0.02;
/// Samples zwischen zwei Blicken auf die Zielwerte (Zweierpotenz)
const REFRESH: u32 = 64;

/// Wo und wie das Spiel hoert.
#[derive(Clone, Copy, Debug)]
pub struct Listener {
    pub pos: Vec3,
    /// Blickrichtung
    pub forward: Vec3,
    pub up: Vec3,
    /// Bewegung (m/s), fuer Doppler
    pub velocity: Vec3,
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            forward: Vec3::NEG_Z,
            up: Vec3::Y,
            velocity: Vec3::ZERO,
        }
    }
}

/// Eine Klangquelle in der Welt.
#[derive(Clone, Copy, Debug)]
pub struct Emitter {
    pub pos: Vec3,
    /// Bewegung (m/s), fuer Doppler
    pub velocity: Vec3,
    /// Lautstaerke aus der Naehe (0..1)
    pub gain: f32,
    /// Tonhoehe (1 = normal)
    pub pitch: f32,
    /// bis zu dieser Entfernung volle Lautstaerke (m)
    pub near: f32,
    /// ab hier unhoerbar (m)
    pub far: f32,
    /// Verdeckung: 0 = frei, 1 = hinter einer Wand (leiser und dumpf)
    pub muffled: f32,
    /// Staerke des Doppler-Effekts (0 = aus)
    pub doppler: f32,
}

impl Emitter {
    /// Ruhende Quelle mit ueblichen Werten (hoerbar bis 40 m).
    pub fn at(pos: Vec3) -> Self {
        Self {
            pos,
            velocity: Vec3::ZERO,
            gain: 1.0,
            pitch: 1.0,
            near: 1.5,
            far: 40.0,
            muffled: 0.0,
            doppler: 1.0,
        }
    }
}

/// Was beim Hoerer ankommt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    pub left: f32,
    pub right: f32,
    /// Abspielgeschwindigkeit
    pub pitch: f32,
    /// Tiefpass (Hz)
    pub cutoff: f32,
}

/// Lautstaerke nach Entfernung: 1 bis `near`, umgekehrt zur Entfernung, 0 ab `far`.
pub fn falloff(dist: f32, near: f32, far: f32) -> f32 {
    let near = near.max(0.01);
    let far = far.max(near + 0.01);
    if dist <= near {
        return 1.0;
    }
    let fade = 1.0 - ((dist - near) / (far - near)).clamp(0.0, 1.0);
    (near / dist) * fade.min(1.0) + 0.25 * fade * fade * (1.0 - near / dist)
}

/// Rechnet aus, wie `e` fuer `l` klingt.
pub fn spatialize(l: &Listener, e: &Emitter) -> Mix {
    let d = e.pos - l.pos;
    let dist = d.length();
    let dir = d.normalize_or_zero();
    let right = l.forward.cross(l.up).normalize_or_zero();
    let muffled = e.muffled.clamp(0.0, 1.0);
    let gain = e.gain.max(0.0) * falloff(dist, e.near, e.far) * (1.0 - 0.6 * muffled);
    // Ganz nah verschwimmt die Richtung, sonst spraenge der Klang beim Durchlaufen
    let width = (dist / e.near.max(0.1)).min(1.0);
    let pan = dir.dot(right).clamp(-1.0, 1.0) * width;
    // gleiche Gesamtenergie in jeder Richtung; nie ganz stumm auf einem Ohr
    let angle = (pan * 0.85 + 1.0) * std::f32::consts::FRAC_PI_4;
    let (r, lft) = angle.sin_cos();
    // hinter dem Kopf etwas dumpfer
    let behind = (-dir.dot(l.forward.normalize_or_zero())).max(0.0) * width;
    let dull = muffled.max(behind * 0.25);
    let cutoff = OPEN_HZ * (MUFFLED_HZ / OPEN_HZ).powf(dull);
    // Doppler: Annaeherung hebt, Entfernung senkt
    let towards = |v: Vec3| v.dot(dir);
    let k = e.doppler.max(0.0);
    let num = SPEED_OF_SOUND + towards(l.velocity) * k;
    let den = SPEED_OF_SOUND + towards(e.velocity) * k;
    let doppler = (num.max(1.0) / den.max(1.0)).clamp(0.5, 2.0);
    Mix {
        left: gain * lft,
        right: gain * r,
        pitch: (e.pitch * doppler).clamp(0.1, 4.0),
        cutoff,
    }
}

/// Zielwerte einer laufenden Stimme, vom Spiel geschrieben und vom Audio-Faden gelesen.
#[derive(Debug)]
pub(super) struct Control {
    left: AtomicU32,
    right: AtomicU32,
    pitch: AtomicU32,
    cutoff: AtomicU32,
    /// ausblenden und beenden
    stop: AtomicBool,
    done: AtomicBool,
}

impl Control {
    pub fn new(m: &Mix) -> Arc<Self> {
        let f = |v: f32| AtomicU32::new(v.to_bits());
        Arc::new(Self {
            left: f(m.left),
            right: f(m.right),
            pitch: f(m.pitch),
            cutoff: f(m.cutoff),
            stop: AtomicBool::new(false),
            done: AtomicBool::new(false),
        })
    }
    pub fn set(&self, m: &Mix) {
        self.left.store(m.left.to_bits(), Relaxed);
        self.right.store(m.right.to_bits(), Relaxed);
        self.pitch.store(m.pitch.to_bits(), Relaxed);
        self.cutoff.store(m.cutoff.to_bits(), Relaxed);
    }
    fn get(&self) -> Mix {
        let f = |v: &AtomicU32| f32::from_bits(v.load(Relaxed));
        Mix {
            left: f(&self.left),
            right: f(&self.right),
            pitch: f(&self.pitch),
            cutoff: f(&self.cutoff),
        }
    }
    pub fn stop(&self) {
        self.stop.store(true, Relaxed);
    }
    pub fn done(&self) -> bool {
        self.done.load(Relaxed)
    }
}

/// Stereo-Wiedergabe eines Mono-Klangs mit laufend nachgefuehrten Werten.
pub(super) struct SpatialSource {
    sound: Sound,
    ctl: Arc<Control>,
    looping: bool,
    pos: f64,
    now: Mix,
    target: Mix,
    /// Tiefpass-Anteil pro Sample und sein Gedaechtnis
    k: f32,
    low: f32,
    tick: u32,
    /// rechtes Ohr des angebrochenen Samples
    pending: Option<f32>,
    stopping: bool,
}

impl SpatialSource {
    pub fn new(sound: Sound, ctl: Arc<Control>, looping: bool) -> Self {
        let m = ctl.get();
        Self {
            sound,
            ctl,
            looping,
            pos: 0.0,
            now: m,
            target: m,
            k: lowpass_k(m.cutoff),
            low: 0.0,
            tick: 0,
            pending: None,
            stopping: false,
        }
    }

    fn refresh(&mut self) {
        self.target = self.ctl.get();
        self.stopping = self.ctl.stop.load(Relaxed);
        if self.stopping {
            self.target.left = 0.0;
            self.target.right = 0.0;
        }
        self.k = lowpass_k(self.now.cutoff);
    }

    /// Sample an der aktuellen (gebrochenen) Stelle; `None` = zu Ende.
    fn read(&mut self) -> Option<f32> {
        let n = self.sound.0.len();
        if n == 0 {
            return None;
        }
        if self.pos >= n as f64 {
            if !self.looping {
                return None;
            }
            self.pos %= n as f64;
        }
        let i = self.pos as usize;
        let next = match i + 1 {
            j if j < n => self.sound.0[j],
            _ if self.looping => self.sound.0[0],
            _ => 0.0,
        };
        let a = self.sound.0[i];
        Some(a + (next - a) * self.pos.fract() as f32)
    }
}

fn lowpass_k(cutoff: f32) -> f32 {
    1.0 - (-std::f32::consts::TAU * cutoff / RATE as f32).exp()
}

impl Iterator for SpatialSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(r) = self.pending.take() {
            return Some(r);
        }
        if self.tick & (REFRESH - 1) == 0 {
            self.refresh();
        }
        self.tick = self.tick.wrapping_add(1);
        let silent = self.now.left.max(self.now.right) < 1e-3;
        let sample = if self.stopping && silent {
            None
        } else {
            self.read()
        };
        let Some(s) = sample else {
            self.ctl.done.store(true, Relaxed);
            return None;
        };
        let a = 1.0 / (SMOOTH * RATE as f32);
        let t = self.target;
        self.now.left += (t.left - self.now.left) * a;
        self.now.right += (t.right - self.now.right) * a;
        self.now.pitch += (t.pitch - self.now.pitch) * a;
        self.now.cutoff += (t.cutoff - self.now.cutoff) * a;
        self.pos += self.now.pitch.max(0.01) as f64;
        self.low += (s - self.low) * self.k;
        // ganz offen: unveraendert durchreichen
        let s = if self.now.cutoff > OPEN_HZ * 0.98 {
            s
        } else {
            self.low
        };
        self.pending = Some(s * self.now.right);
        Some(s * self.now.left)
    }
}

impl Source for SpatialSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mix(x: f32, y: f32, z: f32) -> Mix {
        // Hoerer im Ursprung, blickt nach -z; rechts ist +x
        spatialize(&Listener::default(), &Emitter::at(Vec3::new(x, y, z)))
    }

    #[test]
    fn direction_distance_and_energy() {
        let (l, r, c) = (mix(-5.0, 0.0, 0.0), mix(5.0, 0.0, 0.0), mix(0.0, 0.0, -5.0));
        assert!(l.left > 2.0 * l.right && r.right > 2.0 * r.left);
        assert!((c.left - c.right).abs() < 1e-5);
        let energy = |m: Mix| m.left * m.left + m.right * m.right;
        assert!(
            (energy(l) - energy(c)).abs() < 1e-4,
            "gleich laut in jeder Richtung"
        );
        assert_eq!(mix(0.0, 0.0, -50.0).left, 0.0, "ausser Hoerweite");
        let mut last = f32::MAX;
        for i in 0..100 {
            let v = falloff(i as f32 * 0.5, 1.5, 40.0);
            assert!(v <= last + 1e-6 && (0.0..=1.0).contains(&v));
            last = v;
        }
        let here = mix(0.0, 0.0, 0.0);
        assert!((here.left - here.right).abs() < 1e-5 && here.left.is_finite());
    }

    #[test]
    fn walls_muffle_and_behind_is_duller() {
        let mut e = Emitter::at(Vec3::new(0.0, 0.0, -5.0));
        let free = spatialize(&Listener::default(), &e);
        e.muffled = 1.0;
        let wall = spatialize(&Listener::default(), &e);
        assert!(wall.cutoff < 1000.0 && wall.left < free.left * 0.5);
        assert!(mix(0.0, 0.0, 5.0).cutoff < mix(0.0, 0.0, -5.0).cutoff);
    }

    #[test]
    fn doppler_raises_approaching_and_lowers_leaving() {
        let mut e = Emitter::at(Vec3::new(0.0, 0.0, -10.0));
        e.velocity = Vec3::new(0.0, 0.0, 30.0); // auf den Hoerer zu
        assert!(spatialize(&Listener::default(), &e).pitch > 1.05);
        e.velocity = -e.velocity;
        assert!(spatialize(&Listener::default(), &e).pitch < 0.95);
        e.doppler = 0.0;
        assert_eq!(spatialize(&Listener::default(), &e).pitch, 1.0);
    }

    fn source(len: usize, looping: bool, m: Mix) -> (SpatialSource, Arc<Control>) {
        let ctl = Control::new(&m);
        let s = SpatialSource::new(Sound::new(vec![1.0; len]), ctl.clone(), looping);
        (s, ctl)
    }

    const OPEN: Mix = Mix {
        left: 0.25,
        right: 0.5,
        pitch: 1.0,
        cutoff: OPEN_HZ,
    };

    #[test]
    fn source_plays_stereo_with_pitch_and_ends() {
        let (s, ctl) = source(1000, false, OPEN);
        let out: Vec<f32> = s.collect();
        assert_eq!(out.len(), 2000);
        assert_eq!((out[0], out[1]), (0.25, 0.5));
        assert!(ctl.done());
        let (s, _) = source(1000, false, Mix { pitch: 2.0, ..OPEN });
        assert_eq!(s.count(), 1000, "doppelt so schnell");
    }

    #[test]
    fn loops_follow_changes_smoothly_and_fade_out() {
        let (mut s, ctl) = source(100, true, OPEN);
        let mut frame = || s.next().zip(s.next());
        for _ in 0..500 {
            assert!(frame().is_some(), "Schleife laeuft weiter");
        }
        ctl.set(&Mix { left: 1.0, ..OPEN });
        let (mut last, mut biggest) = (0.25, 0.0f32);
        for _ in 0..RATE / 5 {
            let (l, _) = frame().unwrap();
            biggest = biggest.max((l - last).abs());
            last = l;
        }
        assert!((last - 1.0).abs() < 0.01 && biggest < 0.01, "ohne Knacken");
        ctl.stop();
        let mut n = 0;
        while frame().is_some() {
            n += 1;
            assert!(n < RATE, "endet nicht");
        }
        assert!(n > 100 && ctl.done(), "blendet aus");
    }
}
