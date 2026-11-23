//! Bausteine fuer Klaenge im Code.

use super::RATE;

/// Wellenform eines Tons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wave {
    Sine,
    Square,
    Saw,
    Triangle,
}

fn wave(w: Wave, phase: f32) -> f32 {
    let p = phase.fract();
    match w {
        Wave::Sine => (p * std::f32::consts::TAU).sin(),
        Wave::Square => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Wave::Saw => 2.0 * p - 1.0,
        Wave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
    }
}

/// Anzahl Samples fuer `secs` Sekunden.
pub fn len(secs: f32) -> usize {
    (secs.max(0.0) * RATE as f32) as usize
}

/// Ton, dessen Frequenz linear von `f0` nach `f1` (Hz) gleitet.
pub fn tone(w: Wave, f0: f32, f1: f32, secs: f32) -> Vec<f32> {
    let n = len(secs);
    let mut phase = 0.0;
    (0..n)
        .map(|i| {
            let t = i as f32 / n.max(1) as f32;
            phase += (f0 + (f1 - f0) * t) / RATE as f32;
            wave(w, phase)
        })
        .collect()
}

/// Weisses Rauschen (deterministisch aus `seed`).
pub fn noise(secs: f32, seed: u32) -> Vec<f32> {
    let mut s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453) | 1;
    (0..len(secs))
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}

/// Huellkurve: Anstieg `attack` und exponentielles Ausklingen (`decay` = Zeitkonstante, s).
pub fn envelope(samples: &mut [f32], attack: f32, decay: f32) {
    let a = len(attack).max(1) as f32;
    for (i, s) in samples.iter_mut().enumerate() {
        let t = i as f32 / RATE as f32;
        let rise = (i as f32 / a).min(1.0);
        *s *= rise * (-t / decay.max(1e-4)).exp();
    }
}

/// Einfacher Tiefpass (Glaettung, `k` = 0..1, kleiner = dumpfer).
pub fn lowpass(samples: &mut [f32], k: f32) {
    let mut y = 0.0;
    for s in samples.iter_mut() {
        y += (*s - y) * k.clamp(0.0, 1.0);
        *s = y;
    }
}

/// Mischt `src` mit Lautstaerke `gain` ab Sekunde `at` in `dst` (verlaengert bei Bedarf).
pub fn mix_into(dst: &mut Vec<f32>, src: &[f32], at: f32, gain: f32) {
    let start = len(at);
    if dst.len() < start + src.len() {
        dst.resize(start + src.len(), 0.0);
    }
    for (i, s) in src.iter().enumerate() {
        dst[start + i] += s * gain;
    }
}

/// Auf hoechstens `peak` begrenzen (gegen Uebersteuern).
pub fn normalize(samples: &mut [f32], peak: f32) {
    let m = samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    if m > 1e-6 {
        let k = peak / m;
        samples.iter_mut().for_each(|s| *s *= k);
    }
}
