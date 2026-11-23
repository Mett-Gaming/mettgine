//! Audio-Wiedergabe (rodio) und kleine Klang-Bausteine zum Erzeugen von Effekten im Code.
//!
//! - [`Audio`]: Ausgabegeraet, Effekte abspielen, eine Musik-Schleife, Lautstaerken.
//!   Ohne Ausgabegeraet bleibt alles still (kein Fehler).
//! - [`Sound`]: Mono-Samples bei [`RATE`]; [`synth`] baut Toene, Rauschen und Huellkurven.
//!
//! Welche Klaenge es gibt, legt das Spiel fest.

use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use std::sync::Arc;

/// Abtastrate aller Klaenge (Hz)
pub const RATE: u32 = 44_100;
/// So viele Effekte duerfen gleichzeitig klingen (aelteste werden ersetzt)
const VOICES: usize = 16;

/// Ein fertiger Klang (Mono, -1..1).
#[derive(Clone, Debug, Default)]
pub struct Sound(pub Arc<Vec<f32>>);

impl Sound {
    pub fn new(samples: Vec<f32>) -> Self {
        Sound(Arc::new(samples))
    }
    /// Laenge in Sekunden.
    pub fn duration(&self) -> f32 {
        self.0.len() as f32 / RATE as f32
    }
}

/// Ausgabe mit Effekt-Stimmen und einer Musik-Spur.
pub struct Audio {
    // Der Stream muss leben, solange etwas klingt.
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    voices: Vec<Sink>,
    next_voice: usize,
    music: Option<Sink>,
    /// Gesamtlautstaerke 0..1
    pub volume: f32,
    /// Musiklautstaerke 0..1 (zusaetzlich zur Gesamtlautstaerke)
    pub music_volume: f32,
}

impl Audio {
    /// Oeffnet das Standard-Ausgabegeraet; ohne Geraet bleibt das Spiel stumm.
    pub fn new(volume: f32, music_volume: f32) -> Self {
        let (stream, handle) = match OutputStream::try_default() {
            Ok((s, h)) => (Some(s), Some(h)),
            Err(e) => {
                crate::crash::log(&format!("Kein Audiogeraet: {e}"));
                (None, None)
            }
        };
        let voices = handle
            .as_ref()
            .map(|h| (0..VOICES).filter_map(|_| Sink::try_new(h).ok()).collect())
            .unwrap_or_default();
        Self {
            _stream: stream,
            handle,
            voices,
            next_voice: 0,
            music: None,
            volume,
            music_volume,
        }
    }

    /// Spielt einen Effekt mit Lautstaerke `gain` (0..1) und Tonhoehe `pitch` (1 = normal).
    pub fn play(&mut self, sound: &Sound, gain: f32, pitch: f32) {
        if self.voices.is_empty() || sound.0.is_empty() {
            return;
        }
        // freie Stimme suchen, sonst die naechste der Reihe nach ersetzen
        let idx = self
            .voices
            .iter()
            .position(|s| s.empty())
            .unwrap_or(self.next_voice % self.voices.len());
        self.next_voice = idx + 1;
        let sink = &self.voices[idx];
        sink.stop();
        let rate = (RATE as f32 * pitch.clamp(0.25, 4.0)) as u32;
        sink.set_volume(gain * self.volume);
        sink.append(SamplesBuffer::new(1, rate, sound.0.as_ref().clone()));
        sink.play();
    }

    /// Startet eine Musik-Schleife (ersetzt die vorherige).
    pub fn play_music(&mut self, sound: &Sound) {
        self.stop_music();
        let Some(h) = &self.handle else {
            return;
        };
        if let Ok(sink) = Sink::try_new(h) {
            sink.set_volume(self.volume * self.music_volume);
            sink.append(SamplesBuffer::new(1, RATE, sound.0.as_ref().clone()).repeat_infinite());
            self.music = Some(sink);
        }
    }

    pub fn stop_music(&mut self) {
        if let Some(m) = self.music.take() {
            m.stop();
        }
    }

    /// Lautstaerken uebernehmen (nach Aenderung in den Einstellungen).
    pub fn set_volumes(&mut self, volume: f32, music: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.music_volume = music.clamp(0.0, 1.0);
        if let Some(m) = &self.music {
            m.set_volume(self.volume * self.music_volume);
        }
    }
}

/// Bausteine fuer Klaenge im Code.
pub mod synth {
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
}

#[cfg(test)]
mod tests {
    use super::synth::*;
    use super::*;

    #[test]
    fn synth_builds_sounds_of_the_right_length_and_level() {
        let mut t = tone(Wave::Square, 440.0, 220.0, 0.5);
        assert_eq!(t.len(), (RATE / 2) as usize);
        envelope(&mut t, 0.01, 0.1);
        assert!(t[t.len() - 1].abs() < 0.01, "klingt aus");
        let mut m = Vec::new();
        mix_into(&mut m, &t, 0.25, 1.0);
        mix_into(&mut m, &noise(0.1, 7), 0.0, 0.5);
        assert_eq!(m.len(), len(0.25) + t.len());
        normalize(&mut m, 0.8);
        let peak = m.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!((peak - 0.8).abs() < 1e-4);
        assert_eq!(
            noise(0.01, 3),
            noise(0.01, 3),
            "Rauschen ist reproduzierbar"
        );
        assert!((Sound::new(t).duration() - 0.5).abs() < 1e-3);
    }
}
