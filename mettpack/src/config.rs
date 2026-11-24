//! `spiel.toml`: was ein Spiel ausliefert.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FILE: &str = "spiel.toml";
/// Ordner mit den Spieldaten im Projekt (wie `asset::data` in der Engine)
pub const ASSETS: &str = "assets";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Ordner im Zip und Anfang des Zip-Namens, z. B. `HACKEPETER`
    pub name: String,
    /// Name in Cargo.toml (so heisst die gebaute exe)
    pub package: String,
    /// Name der exe im Paket
    pub exe: String,
    /// Paket -> Muster unter `assets/` (`*` steht fuer beliebige Zeichen)
    pub daten: BTreeMap<String, Vec<String>>,
    /// Muster unter `assets/`, die nicht ausgeliefert werden
    #[serde(default)]
    pub ausgelassen: Vec<String>,
    /// Ziel im Paket -> Datei oder Ordner im Projekt
    #[serde(default)]
    pub beilagen: BTreeMap<String, String>,
    /// Schrift unter `assets/` -> Beilage mit ihrer Lizenz
    #[serde(default)]
    pub schriften: BTreeMap<String, String>,
    /// Beilagen mit Programmcode (DLL), deren Weitergabe freigegeben ist
    #[serde(default)]
    pub freigegebene_dlls: Vec<String>,
}

impl Config {
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join(FILE);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{FILE}: {e}"))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// Welche Pakete wollen diese Datei (`assets/`-Name)?
    pub fn packs_for(&self, name: &str) -> Vec<&str> {
        let wanted = |list: &Vec<String>| list.iter().any(|m| matches(m, name));
        self.daten
            .iter()
            .filter(|(_, list)| wanted(list))
            .map(|(pack, _)| pack.as_str())
            .collect()
    }

    pub fn skipped(&self, name: &str) -> bool {
        self.ausgelassen.iter().any(|m| matches(m, name))
    }
}

/// `*` steht fuer beliebig viele Zeichen, auch `/`.
pub fn matches(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((before, after)) => name.strip_prefix(before).is_some_and(|rest| {
            rest.char_indices()
                .map(|(i, _)| i)
                .chain([rest.len()])
                .any(|i| matches(after, &rest[i..]))
        }),
    }
}

/// Alle Dateien unter `dir`, sortiert: (Name mit `/`, Pfad).
pub fn files(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    walk(dir, "", &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let text = |e: std::io::Error| format!("{}: {e}", dir.display());
    for e in std::fs::read_dir(dir).map_err(text)? {
        let e = e.map_err(text)?;
        let name = format!("{prefix}{}", e.file_name().to_string_lossy());
        if e.file_type().map_err(text)?.is_dir() {
            walk(&e.path(), &format!("{name}/"), out)?;
        } else {
            out.push((name, e.path()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(matches("people/*", "people/a/b.k0s"));
        assert!(matches("*.ttf", "fonts/x.ttf") && !matches("*.ttf", "fonts/x.txt"));
        assert!(matches("a*c*e", "abcde") && !matches("a*c*f", "abcde"));
        assert!(matches("genau", "genau") && !matches("genau", "genauer"));
        assert!(matches("*", "ü/ö"));
    }

    #[test]
    fn typos_in_the_file_are_errors() {
        let ok = "name='A'\npackage='a'\nexe='A.exe'\n[daten]\nx=['*']";
        assert!(Config::parse(ok).is_ok());
        assert!(Config::parse(&format!("nmae='B'\n{ok}")).is_err());
    }
}
