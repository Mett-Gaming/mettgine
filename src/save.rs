//! Spielstaende in `%APPDATA%\.mettlauncher\<spiel>\saves`: mit Versionsnummer und Pruefsumme,
//! unteilbar geschrieben, der vorige Stand bleibt als Sicherung. Den Inhalt (Bytes) bestimmt das
//! Spiel, z. B. mit `net::sync::wire` oder serde.

use crate::settings::write_atomic;
use std::path::{Path, PathBuf};

/// Kennung am Anfang jeder Datei
const MAGIC: &[u8; 4] = b"MSAV";
const HEADER: usize = 24;

/// Warum ein Stand nicht geladen werden konnte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// gibt es nicht
    Missing,
    /// beschaedigt (auch die Sicherung)
    Broken,
    /// von einer neueren Spielversion
    TooNew(u32),
    /// Datei nicht lesbar
    Io(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LoadError::Missing => write!(f, "Spielstand nicht vorhanden"),
            LoadError::Broken => write!(f, "Spielstand beschädigt"),
            LoadError::TooNew(v) => write!(f, "Spielstand ist von einer neueren Version ({v})"),
            LoadError::Io(e) => write!(f, "Spielstand nicht lesbar: {e}"),
        }
    }
}
impl std::error::Error for LoadError {}

/// Ein geladener Stand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    /// Version, mit der er geschrieben wurde (das Spiel wandelt alte selbst um)
    pub version: u32,
    pub data: Vec<u8>,
    /// die Hauptdatei war kaputt, das hier ist die Sicherung
    pub from_backup: bool,
}

/// Spielstaende eines Spiels.
#[derive(Clone, Debug)]
pub struct Saves {
    dir: PathBuf,
    version: u32,
}

impl Saves {
    /// `app` = Kurzname des Spiels, `version` = Format, das dieses Spiel schreibt. `None` = kein
    /// Datenordner vorhanden.
    pub fn new(app: &str, version: u32) -> Option<Self> {
        let dir = crate::crash::data_dir(app)?.join("saves");
        Some(Self::in_dir(dir, version))
    }

    /// Anderer Ordner (Tests).
    pub fn in_dir(dir: impl Into<PathBuf>, version: u32) -> Self {
        Self {
            dir: dir.into(),
            version,
        }
    }

    /// Nur Buchstaben, Ziffern, `-` und `_`, damit der Name ein Dateiname bleibt.
    fn file(&self, slot: &str) -> Result<PathBuf, String> {
        let ok = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
        if slot.is_empty() || slot.len() > 64 || !slot.chars().all(ok) {
            return Err(format!("ungueltiger Spielstand-Name: {slot}"));
        }
        Ok(self.dir.join(format!("{slot}.sav")))
    }

    /// Stand schreiben; der vorige wird zur Sicherung.
    pub fn save(&self, slot: &str, data: &[u8]) -> Result<(), String> {
        let path = self.file(slot)?;
        if read(&path, u32::MAX).is_ok() {
            // nur heile Staende sichern
            let _ = std::fs::copy(&path, backup(&path));
        }
        let mut out = Vec::with_capacity(HEADER + data.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(&checksum(data).to_le_bytes());
        out.extend_from_slice(data);
        write_atomic(&path, &out).map_err(|e| e.to_string())
    }

    /// Stand laden; ist er beschaedigt, die Sicherung.
    pub fn load(&self, slot: &str) -> Result<Loaded, LoadError> {
        let path = self.file(slot).map_err(LoadError::Io)?;
        match read(&path, self.version) {
            Ok((version, data)) => Ok(Loaded {
                version,
                data,
                from_backup: false,
            }),
            Err(LoadError::TooNew(v)) => Err(LoadError::TooNew(v)),
            Err(first) => match read(&backup(&path), self.version) {
                Ok((version, data)) => {
                    crate::crash::log(&format!("{slot}: {first}, Sicherung geladen"));
                    Ok(Loaded {
                        version,
                        data,
                        from_backup: true,
                    })
                }
                Err(_) => Err(first),
            },
        }
    }

    /// Namen aller Staende, zuletzt geschriebene zuerst.
    pub fn list(&self) -> Vec<String> {
        let Ok(dir) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut found: Vec<_> = dir
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "sav"))
            .filter_map(|e| {
                let name = e.path().file_stem()?.to_str()?.to_string();
                Some((e.metadata().ok()?.modified().ok()?, name))
            })
            .collect();
        found.sort_by(|a, b| b.cmp(a));
        found.into_iter().map(|f| f.1).collect()
    }

    /// Stand samt Sicherung loeschen.
    pub fn delete(&self, slot: &str) -> Result<(), String> {
        let path = self.file(slot)?;
        let _ = std::fs::remove_file(backup(&path));
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

fn backup(p: &Path) -> PathBuf {
    p.with_extension("bak")
}

/// FNV-1a: erkennt beschaedigte Dateien (kein Schutz vor absichtlichem Aendern).
fn checksum(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

fn read(path: &Path, newest: u32) -> Result<(u32, Vec<u8>), LoadError> {
    let b = std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => LoadError::Missing,
        _ => LoadError::Io(e.to_string()),
    })?;
    let Some((head, data)) = b.split_at_checked(HEADER) else {
        return Err(LoadError::Broken);
    };
    let u64_at = |o: usize| {
        let mut x = [0; 8];
        x.copy_from_slice(&head[o..o + 8]);
        u64::from_le_bytes(x)
    };
    let version = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
    if &head[..4] != MAGIC || u64_at(8) != data.len() as u64 || u64_at(16) != checksum(data) {
        return Err(LoadError::Broken);
    }
    if version > newest {
        return Err(LoadError::TooNew(version));
    }
    Ok((version, data.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saves(name: &str, version: u32) -> Saves {
        let dir = std::env::temp_dir().join(format!("mettgine-save-{name}-{}", std::process::id()));
        Saves::in_dir(dir, version)
    }

    #[test]
    fn saves_survive_damage_through_their_backup() {
        let s = saves("a", 2);
        assert_eq!(s.load("eins"), Err(LoadError::Missing));
        s.save("eins", b"alt").unwrap();
        s.save("eins", b"neu").unwrap();
        assert_eq!(s.load("eins").unwrap().data, b"neu");
        let path = s.file("eins").unwrap();
        let mut b = std::fs::read(&path).unwrap();
        *b.last_mut().unwrap() ^= 1;
        std::fs::write(&path, &b).unwrap();
        let l = s.load("eins").unwrap();
        assert!(l.from_backup && l.data == b"alt");
        // ein kaputter Stand ersetzt nie die heile Sicherung
        s.save("eins", b"drei").unwrap();
        std::fs::write(&path, b"MSAV").unwrap();
        assert_eq!(s.load("eins").unwrap().data, b"alt");
        assert_eq!(s.list(), ["eins"]);
        s.delete("eins").unwrap();
        assert_eq!(s.load("eins"), Err(LoadError::Missing));
        std::fs::remove_dir_all(&s.dir).unwrap();
    }

    #[test]
    fn newer_saves_and_bad_names_are_refused() {
        let s = saves("b", 5);
        s.save("x", b"1").unwrap();
        let old = Saves::in_dir(&s.dir, 4);
        assert_eq!(old.load("x"), Err(LoadError::TooNew(5)));
        assert!(s.save("../x", b"").is_err() && s.save("", b"").is_err());
        std::fs::remove_dir_all(&s.dir).unwrap();
    }
}
