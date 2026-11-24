//! Pruefungen vor dem Packen. Jede Meldung sagt, was zu tun ist.

use crate::config::{files, Config, ASSETS};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

/// Grenzen des Launchers fuer Erfolge (siehe docs/erfolge.md)
const MAX_ACHIEVEMENTS: usize = 200;
const MAX_ICON: u64 = 256 * 1024;

/// Alle Probleme, leer = in Ordnung.
pub fn run(root: &Path, c: &Config) -> Vec<String> {
    let mut out = Vec::new();
    names(c, &mut out);
    match files(&root.join(ASSETS)) {
        Ok(list) => assets(
            c,
            &list.into_iter().map(|f| f.0).collect::<Vec<_>>(),
            &mut out,
        ),
        Err(e) => out.push(e),
    }
    extras(root, c, &mut out);
    if root.join("erfolge").is_dir() {
        if !c.beilagen.values().any(|s| s == "erfolge") {
            out.push("Der Ordner erfolge/ fehlt in den Beilagen".into());
        }
        achievements(&root.join("erfolge"), &mut out);
    }
    out
}

fn names(c: &Config, out: &mut Vec<String>) {
    let plain = |s: &str| mpak::valid_name(s) && !s.contains('/');
    for n in [&c.name, &c.package, &c.exe]
        .into_iter()
        .chain(c.daten.keys())
    {
        if !plain(n) {
            out.push(format!("Unerlaubter Name „{n}“"));
        }
    }
    let mut seen = HashSet::new();
    for target in c.beilagen.keys() {
        let low = target.to_lowercase();
        let taken = low == c.exe.to_lowercase() || low == "daten" || low.starts_with("daten/");
        if !mpak::valid_name(target) || taken || !seen.insert(low) {
            out.push(format!("Beilage „{target}“: Ziel nicht erlaubt"));
        }
    }
}

/// Jede Datei gehoert in genau ein Paket oder ist ausdruecklich ausgelassen.
fn assets(c: &Config, names: &[String], out: &mut Vec<String>) {
    for n in names {
        let packs = c.packs_for(n);
        match (packs.len(), c.skipped(n)) {
            (0, false) => out.push(format!("assets/{n} liegt in keinem Paket (spiel.toml)")),
            (1, true) => out.push(format!("assets/{n} ist zugleich gepackt und ausgelassen")),
            (2.., _) => out.push(format!("assets/{n} liegt in {}", packs.join(" und "))),
            _ => {}
        }
        let font = n.ends_with(".ttf") || n.ends_with(".otf");
        if font && !c.skipped(n) {
            match c.schriften.get(n) {
                None => out.push(format!("assets/{n}: Lizenz unter [schriften] fehlt")),
                Some(l) if !c.beilagen.contains_key(l) => {
                    out.push(format!("Lizenz {l} fehlt in den Beilagen"));
                }
                Some(_) => {}
            }
        }
    }
    for (pack, list) in &c.daten {
        for m in list {
            if !names.iter().any(|n| crate::config::matches(m, n)) {
                out.push(format!("Paket {pack}: „{m}“ trifft keine Datei"));
            }
        }
    }
}

fn extras(root: &Path, c: &Config, out: &mut Vec<String>) {
    for (target, source) in &c.beilagen {
        if !mpak::valid_name(source) || !root.join(source).exists() {
            out.push(format!("Beilage {source} fehlt"));
            continue;
        }
        let inside = match files(&root.join(source)) {
            Ok(list) => list
                .into_iter()
                .map(|f| format!("{target}/{}", f.0))
                .collect(),
            Err(_) => vec![target.clone()],
        };
        for f in inside {
            if f.to_lowercase().ends_with(".dll") && !c.freigegebene_dlls.contains(&f) {
                out.push(format!(
                    "{f}: Weitergabe ist nicht freigegeben (freigegebene_dlls)"
                ));
            }
        }
    }
}

#[derive(Deserialize)]
struct Achievements {
    schema: u32,
    achievements: Vec<Achievement>,
}

#[derive(Deserialize)]
struct Achievement {
    id: String,
    name: String,
    #[serde(default)]
    text: String,
    icon: Option<String>,
    goal: Option<i64>,
}

fn achievements(dir: &Path, out: &mut Vec<String>) {
    let read = std::fs::read_to_string(dir.join("erfolge.json")).map_err(|e| e.to_string());
    let parsed =
        read.and_then(|t| serde_json::from_str::<Achievements>(&t).map_err(|e| e.to_string()));
    let list = match parsed {
        Ok(a) if a.schema == 1 && a.achievements.len() <= MAX_ACHIEVEMENTS => a.achievements,
        Ok(_) => return out.push("erfolge.json: Schema nicht 1 oder zu viele Erfolge".into()),
        Err(e) => return out.push(format!("erfolge.json: {e}")),
    };
    let mut ids = HashSet::new();
    for a in &list {
        let id_ok = (1..=64).contains(&a.id.len())
            && a.id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        // Laengen zaehlt der Launcher in UTF-16
        let len = |s: &str| s.encode_utf16().count();
        let ok = id_ok
            && ids.insert(&a.id)
            && !a.name.trim().is_empty()
            && len(&a.name) <= 80
            && len(&a.text) <= 200
            && (1..=1_000_000_000).contains(&a.goal.unwrap_or(1));
        if !ok {
            out.push(format!(
                "erfolge.json: „{}“ ist ungültig oder doppelt",
                a.id
            ));
        }
        if let Some(icon) = &a.icon {
            let size = std::fs::metadata(dir.join(icon)).map(|m| m.len());
            let good = mpak::valid_name(icon)
                && icon.to_lowercase().ends_with(".png")
                && size.is_ok_and(|s| s <= MAX_ICON);
            if !good {
                out.push(format!("erfolge.json: Bild {icon} fehlt oder ist zu groß"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(extra: &str) -> Config {
        Config::parse(&format!(
            "name='A'\npackage='a'\nexe='A.exe'\n{extra}\n[daten]\nx=['x/*']\nf=['*.ttf']"
        ))
        .unwrap()
    }

    fn check(c: &Config, names: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        let names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        assets(c, &names, &mut out);
        out
    }

    #[test]
    fn every_file_needs_exactly_one_pack() {
        let c = config("schriften={'f/a.ttf'='L.txt'}\nbeilagen={'L.txt'='L.txt'}");
        assert_eq!(check(&c, &["x/1", "f/a.ttf"]), Vec::<String>::new());
        assert!(check(&c, &["x/1", "f/a.ttf", "neu"])[0].contains("keinem Paket"));
        assert!(check(&c, &["x/a.ttf"])[0].contains("f und x"));
        assert!(check(&c, &["x/1"])[0].contains("trifft keine Datei"));
    }

    #[test]
    fn fonts_need_a_licence_and_dlls_a_permission() {
        assert!(check(&config(""), &["x/1", "f/a.ttf"])[0].contains("Lizenz"));
        let dir = std::env::temp_dir().join(format!("mettpack-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(dir.join("lib/fremd.dll"), b"").unwrap();
        let mut out = Vec::new();
        extras(&dir, &config("beilagen={'lib'='lib'}"), &mut out);
        assert_eq!(out.len(), 1, "{out:?}");
        out.clear();
        let allowed = config("beilagen={'lib'='lib'}\nfreigegebene_dlls=['lib/fremd.dll']");
        extras(&dir, &allowed, &mut out);
        assert!(out.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn targets_stay_inside_and_unique() {
        for bad in [
            "'../x'='a'",
            "'a.EXE'='a'",
            "'daten/x'='a'",
            "'L'='a','l'='b'",
        ] {
            let mut out = Vec::new();
            names(&config(&format!("beilagen={{{bad}}}")), &mut out);
            assert!(!out.is_empty(), "{bad}");
        }
    }
}
