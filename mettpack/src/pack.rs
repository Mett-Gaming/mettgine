//! `dist/` fuellen: Ordner zum Ausprobieren, Zip und Dateiliste fuer den Launcher.
//!
//! Dateiliste (`<Zip ohne .zip>.dateien.json`): je Datei Groesse, sha256 und Ort im Zip; bei
//! Paketen (`.mpak`, im Zip unveraendert) zusaetzlich die Stuecke.

use crate::config::{files, Config, ASSETS};
use crate::zip::{Method, Zip};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Groesser darf keine Datei werden
const MAX_FILE: usize = 100 << 20;

#[derive(Serialize)]
struct Listing {
    schema: u32,
    zip: String,
    zip_size: u64,
    /// Oberordner im Zip
    top: String,
    files: Vec<Listed>,
}

#[derive(Serialize)]
struct Listed {
    path: String,
    size: u64,
    sha256: String,
    /// Ort im Zip
    offset: u64,
    /// Bytes im Zip
    stored: u64,
    /// im Zip gepackt (deflate)?
    packed: bool,
    /// Stuecke: (Ort in der Datei, Groesse, sha256), luckenlos
    #[serde(skip_serializing_if = "Vec::is_empty")]
    parts: Vec<(u64, u64, String)>,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Alles, was ausgeliefert wird: Ziel -> Inhalt.
fn collect(root: &Path, c: &Config) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let read = |p: &Path| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
    let mut out = BTreeMap::new();
    let built = target_dir(root).join("release");
    let suffix = std::env::consts::EXE_SUFFIX;
    out.insert(
        c.exe.clone(),
        read(&built.join(format!("{}{suffix}", c.package)))?,
    );
    // Symbole: Absturzberichte zeigen damit Funktionsnamen und Zeilen
    let pdb = built.join(format!("{}.pdb", c.package.replace('-', "_")));
    if pdb.is_file() {
        out.insert(format!("{}.pdb", c.package), read(&pdb)?);
    }
    let assets = files(&root.join(ASSETS))?;
    for pack in c.daten.keys() {
        let mut list = Vec::new();
        for (name, path) in &assets {
            if !c.skipped(name) && c.packs_for(name) == [pack] {
                list.push((name.clone(), read(path)?));
            }
        }
        out.insert(format!("daten/{pack}.mpak"), mpak::write(&list)?);
    }
    for (target, source) in &c.beilagen {
        let path = root.join(source);
        if path.is_dir() {
            for (name, file) in files(&path)? {
                out.insert(format!("{target}/{name}"), read(&file)?);
            }
        } else {
            out.insert(target.clone(), read(&path)?);
        }
    }
    match out.iter().find(|(_, b)| b.len() > MAX_FILE) {
        Some((n, b)) => Err(format!("{n} ist mit {} MB zu groß", b.len() >> 20)),
        None => Ok(out),
    }
}

fn target_dir(root: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(d) => d.into(),
        None => root.join("target"),
    }
}

pub fn run(root: &Path, c: &Config, version: &str) -> Result<(), String> {
    let all = collect(root, c)?;
    let dist = root.join("dist");
    let folder = dist.join(&c.name);
    if folder.is_dir() {
        std::fs::remove_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    for (name, bytes) in &all {
        save(&folder.join(name), bytes)?;
    }
    let base = format!("{}-{version}", c.name);
    let (zip, list) = archive(c, &format!("{base}.zip"), &all, &folder)?;
    save(&dist.join(format!("{base}.zip")), &zip)?;
    save(&dist.join(format!("{base}.dateien.json")), list.as_bytes())?;
    println!("dist/{base}.zip: {:.1} MB", zip.len() as f64 / 1e6);
    for (name, bytes) in all
        .iter()
        .filter(|(n, _)| !n.contains('/') || n.ends_with(".mpak"))
    {
        println!("  {name}: {:.1} MB", bytes.len() as f64 / 1e6);
    }
    Ok(())
}

fn archive(
    c: &Config,
    zip_name: &str,
    all: &BTreeMap<String, Vec<u8>>,
    folder: &Path,
) -> Result<(Vec<u8>, String), String> {
    let mut zip = Zip::default();
    let mut list = Vec::new();
    for (name, bytes) in all {
        let is_pack = name.ends_with(".mpak");
        let method = if is_pack {
            Method::Store
        } else {
            Method::Deflate
        };
        let placed = zip.add(&format!("{}/{name}", c.name), bytes, method)?;
        let mut parts = Vec::new();
        if is_pack {
            // Zurueckgelesen: Was ausgeliefert wird, laesst sich auch oeffnen
            let pack = mpak::Pack::open(&folder.join(name))?;
            for n in pack.names() {
                pack.read(n)?;
            }
            parts = pack
                .parts()
                .iter()
                .map(|p| (p.offset, p.size, hex(&p.sha256)))
                .collect();
        }
        list.push(Listed {
            path: name.clone(),
            size: bytes.len() as u64,
            sha256: hex(&mpak::sha256(bytes)),
            offset: placed.offset,
            stored: placed.stored,
            packed: !is_pack,
            parts,
        });
    }
    let zip = zip.finish()?;
    let listing = Listing {
        schema: 1,
        zip: zip_name.into(),
        zip_size: zip.len() as u64,
        top: c.name.clone(),
        files: list,
    };
    let text = serde_json::to_string(&listing).map_err(|e| e.to_string())?;
    Ok((zip, text))
}

fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let text = |e: std::io::Error| format!("{}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(text)?;
    }
    std::fs::write(path, bytes).map_err(text)
}
