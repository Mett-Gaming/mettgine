//! Audio-Wiedergabe (rodio) und kleine Klang-Bausteine zum Erzeugen von Effekten im Code.
//!
//! - [`Audio`]: Ausgabegeraet, Effekte abspielen, eine Musik-Schleife, Lautstaerken.
//!   Ohne Ausgabegeraet bleibt alles still (kein Fehler).
//! - [`Sound`]: Mono-Samples bei [`RATE`]; [`synth`] baut Toene, Rauschen und Huellkurven.
//!
//! - [`mixer`]: Gruppen, Spuren mit Ueberblenden und Tempo, Klangdateien (Feature `audio-files`).
//! - [`spatial`]: Klaenge mit Ort in der Welt ([`Audio::play_at`], [`Audio::start_loop`]).
//!
//! Welche Klaenge es gibt, legt das Spiel fest.

pub mod mixer;
pub mod spatial;
pub mod synth;

use spatial::{spatialize, Control, Emitter, Listener, SpatialSource};

use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use std::sync::Arc;

/// Abtastrate aller Klaenge (Hz)
pub const RATE: u32 = 44_100;
/// So viele Effekte duerfen gleichzeitig klingen (aelteste werden ersetzt)
const VOICES: usize = 16;

/// So viele Klaenge mit Ort duerfen gleichzeitig klingen
const SPATIAL_VOICES: usize = 32;

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

/// Kennung eines laufenden Klangs mit Ort.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoiceId(u64);

/// Ein laufender Klang mit Ort.
struct SpatialVoice {
    id: VoiceId,
    ctl: Arc<Control>,
    emitter: Emitter,
    looping: bool,
    // haelt die Wiedergabe am Leben
    _sink: Sink,
}

/// Ausgabe mit Effekt-Stimmen und einer Musik-Spur.
pub struct Audio {
    // Der Stream muss leben, solange etwas klingt.
    _stream: Option<OutputStream>,
    handle: Option<OutputStreamHandle>,
    voices: Vec<Sink>,
    next_voice: usize,
    music: Option<Sink>,
    spatial: Vec<SpatialVoice>,
    listener: Listener,
    next_id: u64,
    mixer: mixer::Mixer,
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
            spatial: Vec::new(),
            listener: Listener::default(),
            next_id: 0,
            mixer: mixer::Mixer::default(),
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

    /// Ort und Blickrichtung des Hoerers; einmal pro Bild. Fuehrt alle Klaenge mit Ort nach und
    /// raeumt beendete auf.
    pub fn set_listener(&mut self, l: Listener) {
        self.listener = l;
        self.spatial.retain(|v| !v.ctl.done());
        for v in &self.spatial {
            v.ctl.set(&self.mix(&v.emitter));
        }
    }

    fn mix(&self, e: &Emitter) -> spatial::Mix {
        let mut m = spatialize(&self.listener, e);
        m.left *= self.volume;
        m.right *= self.volume;
        m
    }

    fn start(&mut self, sound: &Sound, e: Emitter, looping: bool) -> Option<VoiceId> {
        if sound.0.is_empty() {
            return None;
        }
        let h = self.handle.as_ref()?;
        self.spatial.retain(|v| !v.ctl.done());
        if self.spatial.len() >= SPATIAL_VOICES {
            // aelteste einmalige Stimme weicht; laufen nur Schleifen, bleibt der neue Klang aus
            let i = self.spatial.iter().position(|v| !v.looping)?;
            self.spatial.remove(i);
        }
        let sink = Sink::try_new(h).ok()?;
        let ctl = Control::new(&self.mix(&e));
        sink.append(SpatialSource::new(sound.clone(), ctl.clone(), looping));
        let id = VoiceId(self.next_id);
        self.next_id += 1;
        self.spatial.push(SpatialVoice {
            id,
            ctl,
            emitter: e,
            looping,
            _sink: sink,
        });
        Some(id)
    }

    /// Spielt einen Effekt an einem Ort. `None` = kein Geraet oder keine Stimme frei.
    pub fn play_at(&mut self, sound: &Sound, e: Emitter) -> Option<VoiceId> {
        // ausser Hoerweite: gar nicht erst starten
        let m = spatialize(&self.listener, &e);
        if m.left.max(m.right) <= 0.0 {
            return None;
        }
        self.start(sound, e, false)
    }

    /// Startet eine Schleife an einem Ort (Motor, Summen); laeuft bis [`Audio::stop_voice`].
    pub fn start_loop(&mut self, sound: &Sound, e: Emitter) -> Option<VoiceId> {
        self.start(sound, e, true)
    }

    /// Quelle bewegen oder veraendern; `false` = klingt nicht mehr.
    pub fn move_voice(&mut self, id: VoiceId, e: Emitter) -> bool {
        let mix = self.mix(&e);
        match self.spatial.iter_mut().find(|v| v.id == id) {
            Some(v) if !v.ctl.done() => {
                v.emitter = e;
                v.ctl.set(&mix);
                true
            }
            _ => false,
        }
    }

    /// Klang mit Ort kurz ausblenden und beenden.
    pub fn stop_voice(&mut self, id: VoiceId) {
        if let Some(v) = self.spatial.iter().find(|v| v.id == id) {
            v.ctl.stop();
        }
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
        for v in &self.spatial {
            v.ctl.set(&self.mix(&v.emitter));
        }
        self.update_tracks(0.0);
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
