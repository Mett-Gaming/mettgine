//! Paketdateien fuer Spieldaten (`.mpak`): viele kleine Dateien in einer, jede einzeln gepackt
//! und mit Pruefsumme. Gleicher Inhalt ergibt immer dieselbe Datei.
//!
//! Aufbau (alle Zahlen LE): `MPK1`, Anzahl (u32), Laenge des Verzeichnisses (u64), sha256 des
//! Verzeichnisses, Verzeichnis, Inhalte. Je Eintrag im Verzeichnis: Laenge des Namens (u16),
//! Name, Ort ab Dateianfang (u64), gespeicherte Groesse (u64), echte Groesse (u64), gepackt
//! (u8: 0 roh, 1 zlib), sha256 der gespeicherten Bytes. Sortiert nach Namen.

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

const MAGIC: &[u8; 4] = b"MPK1";
/// Kennung, Anzahl, Laenge und Pruefsumme des Verzeichnisses
const HEADER: usize = 4 + 4 + 8 + 32;
/// Laengstes Verzeichnis
const MAX_INDEX: u64 = 64 << 20;
/// Groesste Datei in einem Paket
const MAX_ENTRY: u64 = 1 << 30;
/// Laengster Name in Bytes
const MAX_NAME: usize = 512;

pub type Sha = [u8; 32];

/// Ein Stueck der Paketdatei (fuer Aktualisierungen, die nur Geaendertes laden).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub offset: u64,
    pub size: u64,
    pub sha256: Sha,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    part: Part,
    raw_size: u64,
    packed: bool,
}

pub fn sha256(bytes: &[u8]) -> Sha {
    Sha256::digest(bytes).into()
}

/// Erlaubter Name: Teile mit `/`, kein Weg nach oben, keine Laufwerke.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.contains(['\\', ':', '\0'])
        && name
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

/// Paket bauen. Doppelte oder unerlaubte Namen sind Fehler.
pub fn write(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut sorted = BTreeMap::new();
    for (name, bytes) in files {
        if !valid_name(name) {
            return Err(format!("unerlaubter Name „{name}“"));
        }
        if sorted.insert(name.as_str(), bytes).is_some() {
            return Err(format!("{name} ist doppelt"));
        }
    }
    let index_len: usize = sorted.keys().map(|n| 2 + n.len() + 8 * 3 + 1 + 32).sum();
    let (mut index, mut data) = (Vec::with_capacity(index_len), Vec::<u8>::new());
    for (name, bytes) in &sorted {
        let packed = pack(bytes)?;
        let (stored, flag) = match &packed {
            Some(p) => (p.as_slice(), 1u8),
            None => (bytes.as_slice(), 0),
        };
        index.extend((name.len() as u16).to_le_bytes());
        index.extend(name.as_bytes());
        index.extend(((HEADER + index_len + data.len()) as u64).to_le_bytes());
        index.extend((stored.len() as u64).to_le_bytes());
        index.extend((bytes.len() as u64).to_le_bytes());
        index.push(flag);
        index.extend(sha256(stored));
        data.extend(stored);
    }
    let mut out = MAGIC.to_vec();
    out.extend((sorted.len() as u32).to_le_bytes());
    out.extend((index.len() as u64).to_le_bytes());
    out.extend(sha256(&index));
    out.extend(index);
    out.extend(data);
    Ok(out)
}

/// `None` = Packen lohnt nicht.
fn pack(bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::best());
    z.write_all(bytes).map_err(|e| e.to_string())?;
    let out = z.finish().map_err(|e| e.to_string())?;
    Ok((out.len() < bytes.len()).then_some(out))
}

/// Geoeffnetes Paket. Beim Oeffnen wird das Verzeichnis geprueft, jeder Eintrag beim Lesen.
pub struct Pack {
    file: Mutex<File>,
    entries: BTreeMap<String, Entry>,
    head: Part,
}

impl Pack {
    pub fn open(path: &Path) -> Result<Self, String> {
        let text = |e: std::io::Error| format!("{}: {e}", path.display());
        let bad = |what: &str| format!("{}: {what}", path.display());
        let mut file = File::open(path).map_err(text)?;
        let total = file.metadata().map_err(text)?.len();
        let mut head = [0; HEADER];
        file.read_exact(&mut head).map_err(text)?;
        if &head[..4] != MAGIC {
            return Err(bad("keine Paketdatei"));
        }
        let count = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
        let mut len = [0; 8];
        len.copy_from_slice(&head[8..16]);
        let len = u64::from_le_bytes(len);
        if len > MAX_INDEX || HEADER as u64 + len > total {
            return Err(bad("abgeschnitten"));
        }
        let mut index = vec![0; len as usize];
        file.read_exact(&mut index).map_err(text)?;
        if sha256(&index) != head[16..HEADER] {
            return Err(bad("Verzeichnis beschädigt"));
        }
        let entries = parse(&index, count, total).ok_or_else(|| bad("Verzeichnis ungültig"))?;
        let whole = [&head[..], &index].concat();
        Ok(Self {
            head: Part {
                offset: 0,
                size: whole.len() as u64,
                sha256: sha256(&whole),
            },
            file: Mutex::new(file),
            entries,
        })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        let e = self
            .entries
            .get(name)
            .ok_or_else(|| format!("{name} fehlt im Paket"))?;
        let mut stored = vec![0; e.part.size as usize];
        {
            let mut f = self.file.lock().unwrap_or_else(|p| p.into_inner());
            f.seek(SeekFrom::Start(e.part.offset))
                .and_then(|_| f.read_exact(&mut stored))
                .map_err(|err| format!("{name}: {err}"))?;
        }
        if sha256(&stored) != e.part.sha256 {
            return Err(format!("{name} ist beschädigt"));
        }
        if !e.packed {
            return Ok(stored);
        }
        let mut out = Vec::with_capacity(e.raw_size as usize);
        ZlibDecoder::new(stored.as_slice())
            .take(e.raw_size + 1)
            .read_to_end(&mut out)
            .map_err(|err| format!("{name}: {err}"))?;
        if out.len() as u64 != e.raw_size {
            return Err(format!("{name} hat die falsche Größe"));
        }
        Ok(out)
    }

    /// Alle Stuecke in Dateireihenfolge, luckenlos: erst Kopf und Verzeichnis, dann die Inhalte.
    pub fn parts(&self) -> Vec<Part> {
        let mut out: Vec<Part> = self.entries.values().map(|e| e.part).collect();
        out.retain(|p| p.size > 0);
        out.sort_by_key(|p| p.offset);
        out.insert(0, self.head);
        out
    }
}

/// Verzeichnis lesen; `None` = passt nicht zur Datei.
fn parse(index: &[u8], count: u32, total: u64) -> Option<BTreeMap<String, Entry>> {
    let mut rest = index;
    let mut out = BTreeMap::new();
    for _ in 0..count {
        let n = u16::from_le_bytes(take(&mut rest)?) as usize;
        let name = std::str::from_utf8(rest.get(..n)?).ok()?.to_string();
        rest = &rest[n..];
        let offset = u64::from_le_bytes(take(&mut rest)?);
        let size = u64::from_le_bytes(take(&mut rest)?);
        let raw_size = u64::from_le_bytes(take(&mut rest)?);
        let packed = match take::<1>(&mut rest)? {
            [0] => false,
            [1] => true,
            _ => return None,
        };
        let sha256 = take(&mut rest)?;
        let fits = offset.checked_add(size)? <= total && size <= MAX_ENTRY && raw_size <= MAX_ENTRY;
        if !valid_name(&name) || !fits || (!packed && size != raw_size) {
            return None;
        }
        let part = Part {
            offset,
            size,
            sha256,
        };
        let entry = Entry {
            part,
            raw_size,
            packed,
        };
        if out.insert(name, entry).is_some() {
            return None;
        }
    }
    rest.is_empty().then_some(out)
}

fn take<const N: usize>(rest: &mut &[u8]) -> Option<[u8; N]> {
    let (a, b) = rest.split_first_chunk::<N>()?;
    *rest = b;
    Some(*a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<(String, Vec<u8>)> {
        let many = (0..=255).cycle().take(9000).collect();
        vec![
            ("b/zwei.bin".into(), many),
            ("a.txt".into(), vec![b'a'; 60]),
            ("leer".into(), vec![]),
            ("roh".into(), vec![7]),
        ]
    }

    fn temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("mpak-{}-{name}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn everything_comes_back_and_parts_cover_the_file() {
        let bytes = write(&files()).unwrap();
        let mut shuffled = files();
        shuffled.reverse();
        assert_eq!(bytes, write(&shuffled).unwrap(), "Reihenfolge egal");
        let path = temp("ok", &bytes);
        let p = Pack::open(&path).unwrap();
        for (name, content) in files() {
            assert_eq!(p.read(&name).unwrap(), content, "{name}");
        }
        assert!(p.read("fehlt").is_err() && !p.contains("fehlt") && p.names().count() == 4);
        let mut at = 0;
        for part in p.parts() {
            assert_eq!(part.offset, at);
            let end = at + part.size;
            assert_eq!(sha256(&bytes[at as usize..end as usize]), part.sha256);
            at = end;
        }
        assert_eq!(at, bytes.len() as u64);
        drop(p);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn changing_one_file_keeps_the_other_parts() {
        let a = temp("a", &write(&files()).unwrap());
        let mut changed = files();
        changed[0].1[5] ^= 1;
        let b = temp("b", &write(&changed).unwrap());
        let (a, b) = (Pack::open(&a).unwrap(), Pack::open(&b).unwrap());
        let known: Vec<Sha> = a.parts().iter().map(|p| p.sha256).collect();
        let new = b.parts().into_iter().filter(|p| !known.contains(&p.sha256));
        assert_eq!(new.count(), 2, "Verzeichnis und die eine Datei");
    }

    #[test]
    fn damage_is_noticed() {
        let good = write(&files()).unwrap();
        for cut in [0, 3, HEADER - 1, HEADER + 10, good.len() - 1] {
            assert!(Pack::open(&temp("cut", &good[..cut])).is_err(), "{cut}");
        }
        for at in [0, HEADER + 4] {
            let mut wrong = good.clone();
            wrong[at] ^= 1;
            assert!(Pack::open(&temp("wrong", &wrong)).is_err(), "{at}");
        }
        let mut data = good;
        *data.last_mut().unwrap() ^= 1;
        let p = Pack::open(&temp("data", &data)).unwrap();
        assert!(p.read("roh").is_err() && p.read("a.txt").is_ok());
        assert!(Pack::open(Path::new("gibt/es/nicht")).is_err());
    }

    #[test]
    fn bad_names_are_refused() {
        for n in ["", "../x", "a//b", "/a", "C:/x", "a\\b", "a/./b"] {
            assert!(write(&[(n.into(), vec![])]).is_err(), "{n}");
        }
        let twice = [("a".into(), vec![]), ("a".into(), vec![])];
        assert!(write(&twice).is_err());
    }
}
