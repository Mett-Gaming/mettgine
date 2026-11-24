//! Spieldaten neben der exe: Paketdateien in `daten/`, beim Entwickeln der Ordner `assets/` des
//! Projekts. Anleitung: `docs/pakete.md`.

use super::Source;
use crate::crash;
use mpak::Pack;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// Ordner mit den Paketdateien, neben der exe
pub const DATA_DIR: &str = "daten";
/// Datei, an der ein Projektordner erkannt wird, und der Datenordner darin
const PROJECT_FILE: &str = "spiel.toml";
const PROJECT_DATA: &str = "assets";
/// Andere Quelle zum Ausprobieren: Ordner mit Paketen oder mit einzelnen Dateien
const OVERRIDE: &str = "METTGINE_DATEN";

/// Mehrere Pakete zusammen.
pub struct Packs(Vec<Pack>);

impl Packs {
    /// Alle `.mpak` eines Ordners. Ein kaputtes Paket oder ein doppelter Name ist ein Fehler.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let list = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut paths: Vec<PathBuf> = list
            .filter_map(|e| Some(e.ok()?.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "mpak"))
            .collect();
        paths.sort();
        let mut packs: Vec<Pack> = Vec::new();
        for p in paths {
            let new = Pack::open(&p)?;
            if let Some(n) = new.names().find(|n| packs.iter().any(|o| o.contains(n))) {
                return Err(format!("{n} liegt in mehreren Paketen"));
            }
            packs.push(new);
        }
        if packs.is_empty() {
            return Err(format!("{}: keine Pakete", dir.display()));
        }
        Ok(Self(packs))
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        match self.0.iter().find(|p| p.contains(name)) {
            Some(p) => p.read(name),
            None => Err(format!("{name} fehlt in den Spieldaten")),
        }
    }
}

/// Daten des laufenden Spiels suchen; wird nur einmal geoeffnet.
pub fn game_data() -> Result<&'static Source, String> {
    static DATA: OnceLock<Result<Source, String>> = OnceLock::new();
    let found = DATA.get_or_init(|| {
        let exe = std::env::current_exe().map_err(|e| format!("Programmpfad: {e}"))?;
        find(&exe, std::env::var_os(OVERRIDE).map(PathBuf::from))
    });
    found.as_ref().map_err(Clone::clone)
}

fn find(exe: &Path, other: Option<PathBuf>) -> Result<Source, String> {
    let packs = |dir: &Path| Packs::open(dir).map(|p| Source::Packs(Arc::new(p)));
    if let Some(dir) = other {
        return packs(&dir).or_else(|e| {
            let plain = dir.is_dir() && e.ends_with("keine Pakete");
            plain.then(|| Source::Folder(dir)).ok_or(e)
        });
    }
    let beside = exe.parent().unwrap_or(Path::new(".")).join(DATA_DIR);
    if beside.is_dir() {
        return packs(&beside);
    }
    let project = exe
        .ancestors()
        .skip(1)
        .find(|d| d.join(PROJECT_FILE).is_file())
        .map(|d| d.join(PROJECT_DATA))
        .filter(|d| d.is_dir());
    match project {
        Some(dir) => Ok(Source::Folder(dir)),
        None => Err(format!("Der Ordner {} fehlt", beside.display())),
    }
}

/// Eine Datei der Spieldaten lesen. Geht das nicht, erscheint eine verstaendliche Meldung und das
/// Spiel endet: Ohne seine Daten kann es nicht weiterlaufen.
pub fn read(name: &str) -> Vec<u8> {
    match game_data().and_then(|s| s.read(name)) {
        Ok(b) => b,
        Err(e) => missing(&e),
    }
}

/// Wie [`read`], gleich umgewandelt (Modelle, Figuren, Animationen).
pub fn load<T: super::Asset>(name: &str) -> T {
    match T::parse(read(name), name) {
        Ok(t) => t,
        Err(e) => missing(&e),
    }
}

fn missing(e: &str) -> ! {
    // Ohne eingerichtete Absturzberichte (Tests, Werkzeuge) reicht der Abbruch
    if !crash::active() {
        panic!("Spieldaten: {e}");
    }
    crash::fatal(
        "Spieldaten",
        e,
        "Dateien des Spiels fehlen oder sind beschädigt. Im Mett Launcher hilft das Zahnrad beim \
         Spiel: „Dateien überprüfen“. Wer das Zip benutzt, entpackt es bitte noch einmal ganz.",
    );
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("mettgine-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }

        fn put(&self, name: &str, bytes: &[u8]) {
            let p = self.0.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }

        fn pack(&self, file: &str, name: &str, bytes: &[u8]) {
            let data = mpak::write(&[(name.into(), bytes.to_vec())]).unwrap();
            self.put(file, &data);
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn packs_beside_the_exe_win_and_several_belong_together() {
        let t = Temp::new("packs");
        t.put("spiel.toml", b"");
        t.put("assets/a.txt", b"Ordner");
        let exe = t.0.join("spiel.exe");
        let read = |n: &str| find(&exe, None).and_then(|s| s.read(n));
        assert_eq!(read("a.txt").unwrap(), b"Ordner");
        assert!(read("../spiel.toml").is_err());
        t.pack("daten/eins.mpak", "a.txt", b"Paket");
        t.pack("daten/zwei.mpak", "b/c.txt", b"mehr");
        assert_eq!(read("a.txt").unwrap(), b"Paket");
        assert_eq!(read("b/c.txt").unwrap(), b"mehr");
        assert!(read("fehlt").unwrap_err().contains("fehlt"));
        t.pack("daten/drei.mpak", "a.txt", b"doppelt");
        assert!(read("b/c.txt").unwrap_err().contains("mehreren"));
    }

    #[test]
    fn missing_or_broken_data_is_explained() {
        let t = Temp::new("broken");
        let exe = t.0.join("spiel.exe");
        assert!(find(&exe, None).err().unwrap().contains("fehlt"));
        std::fs::create_dir(t.0.join("daten")).unwrap();
        assert!(find(&exe, None).err().unwrap().contains("keine Pakete"));
        t.put("daten/x.mpak", b"MPK1 kaputt");
        assert!(find(&exe, None).is_err());
    }

    #[test]
    fn another_place_can_be_named() {
        let t = Temp::new("other");
        t.put("lose/a.txt", b"lose");
        t.pack("pakete/p.mpak", "a.txt", b"gepackt");
        let read = |dir: &str| {
            find(Path::new("x"), Some(t.0.join(dir)))
                .unwrap()
                .read("a.txt")
        };
        assert_eq!(read("lose").unwrap(), b"lose");
        assert_eq!(read("pakete").unwrap(), b"gepackt");
        assert!(find(Path::new("x"), Some(t.0.join("nix"))).is_err());
    }
}
