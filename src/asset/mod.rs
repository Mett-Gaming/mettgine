//! Spieldaten laden: aus Paketen neben der exe ([`data`]), eingebettet oder aus einem Ordner, im Hintergrund, mit Griffen
//! ([`Handle`]) statt Kopien. Im Debug-Bau laedt ein Ordner geaenderte Dateien von selbst neu.
//!
//! Wer alles einbettet und beim Start liest (wie bisher), braucht dieses Modul nicht.

pub mod data;
#[cfg(feature = "gltf")]
pub mod gltf;
#[cfg(feature = "images")]
pub mod image;

use crate::jobs::{Pool, Task};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Abstand, in dem auf geaenderte Dateien geprueft wird
const CHECK_EVERY: Duration = Duration::from_millis(500);

/// Woher die Daten kommen.
#[derive(Clone)]
pub enum Source {
    /// Name und Inhalt, z. B. aus `include_bytes!`
    Embedded(&'static [(&'static str, &'static [u8])]),
    Folder(PathBuf),
    /// Paketdateien (`.mpak`)
    Packs(Arc<data::Packs>),
}

impl Source {
    /// Eingebettet im Release-Bau, aus dem Ordner im Debug-Bau (mit Neuladen).
    pub fn embedded_or_folder(
        list: &'static [(&'static str, &'static [u8])],
        folder: impl Into<PathBuf>,
    ) -> Self {
        let folder = folder.into();
        if cfg!(debug_assertions) && folder.is_dir() {
            Source::Folder(folder)
        } else {
            Source::Embedded(list)
        }
    }

    /// Datei lesen. Namen mit `/`; nach oben aus dem Ordner heraus geht es nicht.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        match self {
            Source::Embedded(list) => list
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, b)| b.to_vec())
                .ok_or_else(|| format!("{name} ist nicht eingebettet")),
            Source::Folder(dir) => {
                let path = safe_join(dir, name)?;
                std::fs::read(path).map_err(|e| format!("{name}: {e}"))
            }
            Source::Packs(p) => p.read(name),
        }
    }

    fn changed_at(&self, name: &str) -> Option<SystemTime> {
        let Source::Folder(dir) = self else {
            return None;
        };
        let path = safe_join(dir, name).ok()?;
        std::fs::metadata(path).ok()?.modified().ok()
    }
}

fn safe_join(dir: &std::path::Path, name: &str) -> Result<PathBuf, String> {
    use std::path::Component;
    let rel = std::path::Path::new(name);
    let ok = rel.components().all(|c| matches!(c, Component::Normal(_)));
    if !ok {
        return Err(format!("{name}: unerlaubter Pfad"));
    }
    Ok(dir.join(rel))
}

/// Was sich laden laesst.
pub trait Asset: Send + Sized + 'static {
    /// Aus dem Inhalt der Datei `name` bauen (laeuft auf einem Hintergrundfaden).
    fn parse(bytes: Vec<u8>, name: &str) -> Result<Self, String>;
}

impl Asset for String {
    fn parse(bytes: Vec<u8>, name: &str) -> Result<Self, String> {
        String::from_utf8(bytes).map_err(|_| format!("{name} ist kein UTF-8-Text"))
    }
}

impl Asset for Vec<u8> {
    fn parse(bytes: Vec<u8>, _: &str) -> Result<Self, String> {
        Ok(bytes)
    }
}

impl Asset for crate::gpu::MeshData {
    fn parse(bytes: Vec<u8>, name: &str) -> Result<Self, String> {
        crate::gpu::parse_mesh(&bytes).map_err(|e| format!("{name}: {e}"))
    }
}

impl Asset for crate::gpu::SkinData {
    fn parse(bytes: Vec<u8>, name: &str) -> Result<Self, String> {
        crate::gpu::parse_skin(&bytes).map_err(|e| format!("{name}: {e}"))
    }
}

impl Asset for crate::anim::AnimSet {
    fn parse(bytes: Vec<u8>, name: &str) -> Result<Self, String> {
        crate::anim::parse_anim(&bytes).map_err(|e| format!("{name}: {e}"))
    }
}

/// Griff auf etwas Geladenes (oder noch Ladendes).
pub struct Handle<T> {
    index: u32,
    kind: PhantomData<fn() -> T>,
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Handle<T> {}
impl<T> PartialEq for Handle<T> {
    fn eq(&self, o: &Self) -> bool {
        self.index == o.index
    }
}
impl<T> Eq for Handle<T> {}
impl<T> std::hash::Hash for Handle<T> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.index.hash(h);
    }
}
impl<T> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Handle({})", self.index)
    }
}

/// Stand eines Eintrags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Loading,
    Ready,
    Failed(String),
}

struct Entry<T> {
    name: String,
    value: Option<T>,
    error: Option<String>,
    task: Option<Task<Result<T, String>>>,
    changed_at: Option<SystemTime>,
}

/// Alle geladenen Dinge einer Art.
pub struct Assets<T> {
    source: Source,
    pool: Arc<Pool>,
    entries: Vec<Entry<T>>,
    by_name: HashMap<String, u32>,
    last_check: Instant,
    watch: bool,
}

impl<T: Asset> Assets<T> {
    /// `pool` darf mit anderen Arten geteilt werden.
    pub fn new(source: Source, pool: Arc<Pool>) -> Self {
        let watch = cfg!(debug_assertions) && matches!(source, Source::Folder(_));
        Self {
            source,
            pool,
            entries: Vec::new(),
            by_name: HashMap::new(),
            last_check: Instant::now(),
            watch,
        }
    }

    /// Im Hintergrund laden. Derselbe Name liefert denselben Griff.
    pub fn load(&mut self, name: &str) -> Handle<T> {
        if let Some(&i) = self.by_name.get(name) {
            return handle(i);
        }
        let i = self.entries.len() as u32;
        self.entries.push(Entry {
            name: name.to_string(),
            value: None,
            error: None,
            task: None,
            changed_at: self.source.changed_at(name),
        });
        self.by_name.insert(name.to_string(), i);
        self.start(i);
        handle(i)
    }

    /// Sofort laden (wartet).
    pub fn load_now(&mut self, name: &str) -> Result<Handle<T>, String> {
        let h = self.load(name);
        let e = &mut self.entries[h.index as usize];
        if let Some(t) = e.task.take() {
            let r = t
                .wait()
                .unwrap_or_else(|| Err(format!("{name}: Laden abgebrochen")));
            finish(e, r);
        }
        match &self.entries[h.index as usize].error {
            Some(err) => Err(err.clone()),
            None => Ok(h),
        }
    }

    /// Etwas selbst Gebautes einreihen.
    pub fn insert(&mut self, name: &str, value: T) -> Handle<T> {
        let i = match self.by_name.get(name) {
            Some(&i) => i,
            None => {
                let i = self.entries.len() as u32;
                self.by_name.insert(name.to_string(), i);
                self.entries.push(Entry {
                    name: name.to_string(),
                    value: None,
                    error: None,
                    task: None,
                    changed_at: None,
                });
                i
            }
        };
        let e = &mut self.entries[i as usize];
        e.value = Some(value);
        e.error = None;
        e.task = None;
        handle(i)
    }

    fn start(&mut self, i: u32) {
        let e = &mut self.entries[i as usize];
        let (source, name) = (self.source.clone(), e.name.clone());
        e.task = Some(
            self.pool
                .spawn(move || T::parse(source.read(&name)?, &name)),
        );
    }

    /// Einmal pro Bild: Fertiges uebernehmen. Liefert, was neu oder neu geladen ist.
    pub fn update(&mut self) -> Vec<Handle<T>> {
        let mut done = Vec::new();
        for (i, e) in self.entries.iter_mut().enumerate() {
            let Some(r) = e.task.as_ref().and_then(Task::poll) else {
                continue;
            };
            e.task = None;
            if r.is_ok() {
                done.push(handle(i as u32));
            }
            finish(e, r);
        }
        self.reload_changed();
        done
    }

    fn reload_changed(&mut self) {
        if !self.watch || self.last_check.elapsed() < CHECK_EVERY {
            return;
        }
        self.last_check = Instant::now();
        for i in 0..self.entries.len() {
            let e = &self.entries[i];
            let now = self.source.changed_at(&e.name);
            if e.task.is_none() && now.is_some() && now != e.changed_at {
                self.entries[i].changed_at = now;
                self.start(i as u32);
            }
        }
    }

    /// Wert, sobald er da ist. Beim Neuladen bleibt der alte bis dahin.
    pub fn get(&self, h: Handle<T>) -> Option<&T> {
        self.entries.get(h.index as usize)?.value.as_ref()
    }

    pub fn state(&self, h: Handle<T>) -> State {
        match self.entries.get(h.index as usize) {
            Some(e) if e.value.is_some() => State::Ready,
            Some(Entry {
                error: Some(err), ..
            }) => State::Failed(err.clone()),
            _ => State::Loading,
        }
    }

    /// Geladene und alle Eintraege (Ladebalken). Fehler zaehlen als geladen.
    pub fn progress(&self) -> (usize, usize) {
        let done = self.entries.iter().filter(|e| e.task.is_none()).count();
        (done, self.entries.len())
    }
}

fn handle<T>(index: u32) -> Handle<T> {
    Handle {
        index,
        kind: PhantomData,
    }
}

fn finish<T>(e: &mut Entry<T>, r: Result<T, String>) {
    match r {
        Ok(v) => {
            e.value = Some(v);
            e.error = None;
        }
        Err(err) => {
            crate::crash::log(&format!("Laden fehlgeschlagen: {err}"));
            e.error = Some(err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static LIST: &[(&str, &[u8])] = &[("a.txt", b"hallo"), ("kaputt.txt", &[0xff, 0xfe])];

    fn pool() -> Arc<Pool> {
        Arc::new(Pool::new(2))
    }

    fn wait<T: Asset>(a: &mut Assets<T>) {
        for _ in 0..200 {
            a.update();
            if a.progress().0 == a.progress().1 {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("laedt nicht fertig");
    }

    #[test]
    fn embedded_data_loads_in_the_background_and_errors_stay_visible() {
        let mut a = Assets::<String>::new(Source::Embedded(LIST), pool());
        let (x, y, z) = (a.load("a.txt"), a.load("kaputt.txt"), a.load("fehlt.txt"));
        assert_eq!(a.load("a.txt"), x, "gleicher Name, gleicher Griff");
        wait(&mut a);
        assert_eq!(a.get(x).map(String::as_str), Some("hallo"));
        assert!(matches!(a.state(y), State::Failed(_)));
        assert!(matches!(a.state(z), State::Failed(_)));
        assert!(a.load_now("fehlt2.txt").is_err());
        let own = a.insert("selbst", "x".into());
        assert_eq!(a.state(own), State::Ready);
    }

    #[test]
    fn changed_files_are_reloaded_and_paths_cannot_escape() {
        let dir = std::env::temp_dir().join(format!("mettgine-asset-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("u")).unwrap();
        std::fs::write(dir.join("u/t.txt"), "eins").unwrap();
        let src = Source::Folder(dir.clone());
        assert!(src.read("../x").is_err() && src.read("C:/x").is_err());
        let mut a = Assets::<String>::new(src, pool());
        let h = a.load_now("u/t.txt").unwrap();
        assert_eq!(a.get(h).unwrap(), "eins");
        if cfg!(debug_assertions) {
            std::thread::sleep(Duration::from_millis(50));
            std::fs::write(dir.join("u/t.txt"), "zwei").unwrap();
            let start = Instant::now();
            while a.get(h).unwrap() != "zwei" && start.elapsed().as_secs() < 10 {
                a.update();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(a.get(h).unwrap(), "zwei");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
