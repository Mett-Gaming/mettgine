//! Mischpult: Gruppen mit eigener Lautstaerke (Musik, Effekte, Stimmen …) und Spuren (Schleifen
//! ohne Ort) mit Ueberblenden und Tempo. Ergaenzt `play` und `play_music`.

use super::spatial::{Control, Mix, SpatialSource};
use super::{Audio, Sound};
use rodio::Sink;
use std::sync::Arc;

/// Tiefpass offen (Hz)
const OPEN_HZ: f32 = 20_000.0;

/// Gruppe mit eigener Lautstaerke. Die Nummern legt das Spiel fest (0..8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Bus(pub u8);

/// Anzahl Gruppen
const BUSES: usize = 8;

/// Kennung einer Spur.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackId(u64);

/// Wie eine Spur klingt.
#[derive(Clone, Copy, Debug)]
pub struct TrackOptions {
    pub bus: Bus,
    pub gain: f32,
    /// Tempo und Tonhoehe (1 = normal)
    pub speed: f32,
    /// Sekunden bis zur vollen Lautstaerke
    pub fade_in: f32,
    /// -1 links … 1 rechts
    pub pan: f32,
    pub looped: bool,
}

impl Default for TrackOptions {
    fn default() -> Self {
        Self {
            bus: Bus(0),
            gain: 1.0,
            speed: 1.0,
            fade_in: 0.0,
            pan: 0.0,
            looped: true,
        }
    }
}

struct Track {
    id: TrackId,
    ctl: Arc<Control>,
    opt: TrackOptions,
    /// Blende 0..1 und ihre Aenderung pro Sekunde
    fade: f32,
    rate: f32,
    _sink: Sink,
}

/// Zustand des Mischpults.
pub(super) struct Mixer {
    buses: [f32; BUSES],
    tracks: Vec<Track>,
    next: u64,
    /// Tiefpass ueber allen Spuren (Hz), z. B. in der Pause
    cutoff: f32,
}

impl Default for Mixer {
    fn default() -> Self {
        Self {
            buses: [1.0; BUSES],
            tracks: Vec::new(),
            next: 0,
            cutoff: OPEN_HZ,
        }
    }
}

impl Mixer {
    fn mix(&self, t: &TrackOptions, fade: f32, volume: f32) -> Mix {
        let bus = self.buses.get(t.bus.0 as usize).copied().unwrap_or(1.0);
        // gleiche Energie in jeder Richtung; Mitte = volle Lautstaerke auf beiden
        let angle = (t.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let (r, l) = angle.sin_cos();
        let g = t.gain.max(0.0) * bus * volume * fade * std::f32::consts::SQRT_2;
        Mix {
            left: g * l,
            right: g * r,
            pitch: t.speed.clamp(0.1, 4.0),
            cutoff: self.cutoff,
        }
    }
}

impl Audio {
    /// Lautstaerke einer Gruppe (0..1).
    pub fn set_bus(&mut self, bus: Bus, gain: f32) {
        if let Some(b) = self.mixer.buses.get_mut(bus.0 as usize) {
            *b = gain.max(0.0);
        }
        self.update_tracks(0.0);
    }

    pub fn bus(&self, bus: Bus) -> f32 {
        let b = self.mixer.buses.get(bus.0 as usize);
        b.copied().unwrap_or(1.0)
    }

    /// Alle Spuren dumpf klingen lassen (Pause, unter Wasser): 0 = klar, 1 = sehr dumpf.
    pub fn set_muffle(&mut self, amount: f32) {
        let k = amount.clamp(0.0, 1.0);
        self.mixer.cutoff = OPEN_HZ * (400.0f32 / OPEN_HZ).powf(k);
        self.update_tracks(0.0);
    }

    /// Effekt in einer Gruppe spielen.
    pub fn play_on(&mut self, bus: Bus, sound: &Sound, gain: f32, pitch: f32) {
        let g = self.bus(bus);
        self.play(sound, gain * g, pitch);
    }

    /// Spur starten. `None` = kein Geraet oder leerer Klang.
    pub fn start_track(&mut self, sound: &Sound, opt: TrackOptions) -> Option<TrackId> {
        if sound.0.is_empty() {
            return None;
        }
        let sink = Sink::try_new(self.handle.as_ref()?).ok()?;
        let fade = if opt.fade_in > 0.0 { 0.0 } else { 1.0 };
        let ctl = Control::new(&self.mixer.mix(&opt, fade, self.volume));
        sink.append(SpatialSource::new(sound.clone(), ctl.clone(), opt.looped));
        let id = TrackId(self.mixer.next);
        self.mixer.next += 1;
        self.mixer.tracks.push(Track {
            id,
            ctl,
            opt,
            fade,
            rate: if opt.fade_in > 0.0 {
                1.0 / opt.fade_in
            } else {
                0.0
            },
            _sink: sink,
        });
        Some(id)
    }

    /// Lautstaerke und Tempo einer laufenden Spur; `false` = laeuft nicht mehr.
    pub fn set_track(&mut self, id: TrackId, gain: f32, speed: f32) -> bool {
        let found = self.mixer.tracks.iter_mut().find(|t| t.id == id);
        let Some(t) = found else {
            return false;
        };
        t.opt.gain = gain;
        t.opt.speed = speed;
        self.update_tracks(0.0);
        true
    }

    /// Spur ausblenden (Sekunden) und beenden.
    pub fn stop_track(&mut self, id: TrackId, fade_out: f32) {
        if let Some(t) = self.mixer.tracks.iter_mut().find(|t| t.id == id) {
            if fade_out > 0.0 {
                t.rate = -1.0 / fade_out;
            } else {
                t.ctl.stop();
            }
        }
    }

    /// Von einer Spur zur naechsten ueberblenden.
    pub fn crossfade(
        &mut self,
        from: Option<TrackId>,
        sound: &Sound,
        opt: TrackOptions,
        seconds: f32,
    ) -> Option<TrackId> {
        if let Some(f) = from {
            self.stop_track(f, seconds);
        }
        let opt = TrackOptions {
            fade_in: seconds,
            ..opt
        };
        self.start_track(sound, opt)
    }

    /// Einmal pro Bild: Blenden weiterfuehren, Beendetes aufraeumen.
    pub fn update_tracks(&mut self, dt: f32) {
        let volume = self.volume;
        self.mixer.tracks.retain(|t| !t.ctl.done());
        for t in &mut self.mixer.tracks {
            t.fade = (t.fade + t.rate * dt).clamp(0.0, 1.0);
            if t.rate < 0.0 && t.fade <= 0.0 {
                t.ctl.stop();
            }
        }
        let m = &self.mixer;
        for t in &m.tracks {
            t.ctl.set(&m.mix(&t.opt, t.fade, volume));
        }
    }
}

/// Klangdatei entpacken (WAV, OGG Vorbis) und auf Mono bei [`super::RATE`] bringen.
#[cfg(feature = "audio-files")]
pub fn decode(bytes: &[u8]) -> Result<Sound, String> {
    use rodio::Source;
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let d = rodio::Decoder::new(cursor).map_err(|e| e.to_string())?;
    let (channels, rate) = (d.channels().max(1) as usize, d.sample_rate().max(1));
    let samples: Vec<f32> = d.convert_samples::<f32>().collect();
    let mono: Vec<f32> = samples
        .chunks(channels)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect();
    Ok(Sound::new(resample(&mono, rate)))
}

#[cfg(feature = "audio-files")]
impl crate::asset::Asset for Sound {
    fn parse(bytes: Vec<u8>, name: &str) -> Result<Self, String> {
        decode(&bytes).map_err(|e| format!("{name}: {e}"))
    }
}

#[cfg(any(feature = "audio-files", test))]
fn resample(s: &[f32], rate: u32) -> Vec<f32> {
    if rate == super::RATE || s.is_empty() {
        return s.to_vec();
    }
    let step = rate as f64 / super::RATE as f64;
    let n = (s.len() as f64 / step) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * step;
            let (k, f) = (p as usize, p.fract() as f32);
            let (a, b) = (s[k], s[(k + 1).min(s.len() - 1)]);
            a + (b - a) * f
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buses_pan_and_fades_shape_the_mix() {
        let mut m = Mixer::default();
        let t = TrackOptions {
            gain: 0.5,
            ..Default::default()
        };
        let full = m.mix(&t, 1.0, 1.0);
        assert!((full.left - 0.5).abs() < 1e-5 && (full.right - 0.5).abs() < 1e-5);
        m.buses[0] = 0.5;
        assert!((m.mix(&t, 0.5, 1.0).left - 0.125).abs() < 1e-5);
        let left = m.mix(&TrackOptions { pan: -1.0, ..t }, 1.0, 1.0);
        assert!(left.right.abs() < 1e-5 && left.left > 0.3);
        // Nummern ausserhalb zaehlen als volle Gruppe
        let t = TrackOptions { bus: Bus(200), ..t };
        assert!((m.mix(&t, 1.0, 1.0).left - 0.5).abs() < 1e-5);
    }

    #[test]
    fn resampling_keeps_length_in_seconds() {
        let s: Vec<f32> = (0..22_050).map(|i| (i as f32 * 0.05).sin()).collect();
        let r = resample(&s, 22_050);
        assert!((r.len() as i32 - 44_100).abs() < 3);
        assert!(r.iter().all(|v| v.abs() <= 1.0));
    }
}
