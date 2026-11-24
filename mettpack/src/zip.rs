//! Schlanker Zip-Schreiber: feste Zeitstempel und Reihenfolge (gleicher Inhalt, gleiche Datei) und
//! bekannte Orte der Inhalte, damit der Launcher Teile einzeln laden kann. Bis 4 GB.

use flate2::write::DeflateEncoder;
use flate2::{Compression, Crc};
use std::io::Write;

/// 1. Januar 1980, 0 Uhr (aeltester Zeitstempel im Zip)
const DATE: u16 = 0x0021;
/// Namen sind UTF-8
const UTF8: u16 = 1 << 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// unveraendert (Stuecke einzeln ladbar)
    Store,
    Deflate,
}

/// Wo ein Inhalt im Zip liegt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub offset: u64,
    pub stored: u64,
}

struct Entry {
    name: String,
    header: u32,
    fixed: Vec<u8>,
}

#[derive(Default)]
pub struct Zip {
    out: Vec<u8>,
    entries: Vec<Entry>,
}

impl Zip {
    pub fn add(&mut self, name: &str, bytes: &[u8], method: Method) -> Result<Placed, String> {
        let small =
            |n: usize| u32::try_from(n).map_err(|_| "Das Zip wird größer als 4 GB".to_string());
        let mut crc = Crc::new();
        crc.update(bytes);
        let (code, data) = match method {
            Method::Store => (0u16, bytes.to_vec()),
            Method::Deflate => {
                let mut z = DeflateEncoder::new(Vec::new(), Compression::best());
                z.write_all(bytes).map_err(|e| e.to_string())?;
                (8, z.finish().map_err(|e| e.to_string())?)
            }
        };
        let header = small(self.out.len())?;
        // gemeinsamer Teil von Datei- und Verzeichniskopf
        let mut fixed = Vec::new();
        for v in [20, UTF8, code, 0, DATE] {
            fixed.extend(v.to_le_bytes());
        }
        for v in [crc.sum(), small(data.len())?, small(bytes.len())?] {
            fixed.extend(v.to_le_bytes());
        }
        fixed.extend((name.len() as u16).to_le_bytes());
        fixed.extend(0u16.to_le_bytes());
        self.out.extend(0x0403_4b50u32.to_le_bytes());
        self.out.extend(&fixed);
        self.out.extend(name.as_bytes());
        let placed = Placed {
            offset: self.out.len() as u64,
            stored: data.len() as u64,
        };
        self.out.extend(data);
        self.entries.push(Entry {
            name: name.into(),
            header,
            fixed,
        });
        Ok(placed)
    }

    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        let start = self.out.len();
        for e in &self.entries {
            self.out.extend(0x0201_4b50u32.to_le_bytes());
            self.out.extend(20u16.to_le_bytes());
            self.out.extend(&e.fixed);
            // Kommentar, Datentraeger, Merkmale
            self.out.extend([0; 10]);
            self.out.extend(e.header.to_le_bytes());
            self.out.extend(e.name.as_bytes());
        }
        let (size, count) = (self.out.len() - start, self.entries.len());
        let tiny = u16::try_from(count).map_err(|_| "Zu viele Dateien im Zip")?;
        let start = u32::try_from(start).map_err(|_| "Das Zip wird größer als 4 GB")?;
        self.out.extend(0x0605_4b50u32.to_le_bytes());
        self.out.extend([0; 4]);
        self.out.extend(tiny.to_le_bytes());
        self.out.extend(tiny.to_le_bytes());
        self.out.extend((size as u32).to_le_bytes());
        self.out.extend(start.to_le_bytes());
        self.out.extend([0; 2]);
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn contents_lie_where_promised_and_output_is_stable() {
        let build = || {
            let mut z = Zip::default();
            let a = z.add("A/ä.bin", b"hallo", Method::Store).unwrap();
            let b = z.add("A/b.txt", &[7; 500], Method::Deflate).unwrap();
            (a, b, z.finish().unwrap())
        };
        let (a, b, bytes) = build();
        assert_eq!(bytes, build().2);
        let at = |p: Placed| &bytes[p.offset as usize..(p.offset + p.stored) as usize];
        assert_eq!(at(a), b"hallo");
        assert!(b.stored < 100);
        let mut text = Vec::new();
        flate2::read::DeflateDecoder::new(at(b))
            .read_to_end(&mut text)
            .unwrap();
        assert_eq!(text, [7; 500]);
    }
}
