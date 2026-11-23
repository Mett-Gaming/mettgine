//! Einstellungsdatei als `schluessel=wert`-Zeilen in `%APPDATA%\<spiel>\<datei>`.
//!
//! Welche Einstellungen es gibt, legt das Spiel fest; hier liegen nur Lesen, Schreiben und
//! Werte holen mit Grenzen. Nie neben die exe (der Launcher ersetzt den Installordner).

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Schluessel-Wert-Paare einer Einstellungsdatei.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyValues(pub BTreeMap<String, String>);

impl KeyValues {
    /// Liest `schluessel=wert`-Zeilen (Leerzeichen am Rand egal, unbekannte Zeilen ignoriert).
    pub fn parse(text: &str) -> Self {
        let mut m = BTreeMap::new();
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                m.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        KeyValues(m)
    }

    /// Als Text, eine Zeile pro Eintrag (sortiert).
    pub fn to_text(&self) -> String {
        self.0.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
    }

    pub fn set(&mut self, key: &str, value: impl ToString) {
        self.0.insert(key.to_string(), value.to_string());
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
    }

    /// Zahl im Bereich `min..=max`, sonst `default`.
    pub fn f32(&self, key: &str, default: f32, min: f32, max: f32) -> f32 {
        self.str(key)
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .map_or(default, |v| v.clamp(min, max))
    }

    /// Ganzzahl im Bereich `min..=max`, sonst `default`.
    pub fn u32(&self, key: &str, default: u32, min: u32, max: u32) -> u32 {
        self.str(key)
            .and_then(|v| v.parse::<u32>().ok())
            .map_or(default, |v| v.clamp(min, max))
    }

    /// `true`/`false`, sonst `default`.
    pub fn bool(&self, key: &str, default: bool) -> bool {
        match self.str(key) {
            Some("true") => true,
            Some("false") => false,
            _ => default,
        }
    }
}

/// Pfad der Einstellungsdatei `file` des Spiels `app`.
pub fn path(app: &str, file: &str) -> Option<PathBuf> {
    crate::crash::data_dir(app).map(|d| d.join(file))
}

/// Laedt die Datei (fehlt sie, gibt es leere Werte).
pub fn load(app: &str, file: &str) -> KeyValues {
    path(app, file)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| KeyValues::parse(&t))
        .unwrap_or_default()
}

/// Speichert die Datei; Fehler landen im Protokoll.
pub fn save(app: &str, file: &str, kv: &KeyValues) {
    let Some(p) = path(app, file) else {
        return;
    };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&p, kv.to_text()) {
        crate::crash::log(&format!("Einstellungen nicht gespeichert: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_roundtrip_and_are_clamped() {
        let mut kv = KeyValues::default();
        kv.set("volume", 0.5);
        kv.set("fullscreen", true);
        kv.set("name", "Mett");
        let back = KeyValues::parse(&kv.to_text());
        assert_eq!(back, kv);
        assert_eq!(back.f32("volume", 1.0, 0.0, 1.0), 0.5);
        assert!(back.bool("fullscreen", false));
        let bad = KeyValues::parse("volume=9\nbots=abc\n kaputt \nfov=NaN");
        assert_eq!(bad.f32("volume", 0.8, 0.0, 1.0), 1.0);
        assert_eq!(bad.u32("bots", 3, 1, 7), 3);
        assert_eq!(bad.f32("fov", 70.0, 50.0, 100.0), 70.0);
    }
}
