//! Absturzberichte und laufendes Protokoll.
//!
//! - `log(...)`: Zeile mit Zeitstempel ins laufende Protokoll
//!   (`%APPDATA%\<app>\logs\latest.log`, beim Start geleert) und in einen Ringpuffer.
//!   Im Release-Build gibt es kein Konsolenfenster: Ohne das Protokoll waeren Fehler unsichtbar.
//! - `context(...)`: Momentaufnahme des Spiels (Bildschirm, Stufe, Seed, GPU ...), die in
//!   jeden Bericht kommt.
//! - Absturz (Rust-Panic oder ein Absturz im Grafiktreiber): Bericht nach
//!   `%APPDATA%\<app>\crashes\crash-<Datum>.txt` mit Version, Fehler, Ort, Aufrufliste,
//!   Moduladresse (zum spaeteren Aufloesen), Kontext und den letzten Protokollzeilen. Danach
//!   ein Hinweisfenster fuer den Spieler. Es bleiben die letzten 10 Berichte.
//! - `fatal(...)`: geordneter Abbruch mit Bericht und verstaendlicher Meldung (z. B. Vulkan
//!   fehlt).
//!
//! Alles landet in `%APPDATA%`, nie neben der exe (der Launcher ersetzt den Installordner).

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// So viele Protokollzeilen kommen in einen Bericht
const LOG_TAIL: usize = 200;
/// So viele Berichte bleiben liegen
const KEEP_REPORTS: usize = 10;

struct State {
    app: String,
    title: String,
    version: String,
    dir: Option<PathBuf>,
    log_file: Option<File>,
    tail: VecDeque<String>,
    context: BTreeMap<String, String>,
    started: SystemTime,
    dialogs: bool,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn state() -> Option<&'static Mutex<State>> {
    STATE.get()
}

/// Ordner fuer Protokoll und Berichte: `%APPDATA%\<app>` (sonst neben dem Arbeitsordner).
pub fn data_dir(app: &str) -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    Some(base.join(app))
}

/// Einrichten (einmal beim Start). `app` = Ordnername, `title` = Name fuer Meldungen.
pub fn init(app: &str, title: &str, version: &str) {
    init_in(data_dir(app), app, title, version, true);
}

/// Wie `init`, mit eigenem Ordner und ohne Hinweisfenster (Tests).
pub fn init_in(dir: Option<PathBuf>, app: &str, title: &str, version: &str, dialogs: bool) {
    let log_file = dir.as_ref().and_then(|d| {
        let logs = d.join("logs");
        fs::create_dir_all(&logs).ok()?;
        File::create(logs.join("latest.log")).ok()
    });
    let st = State {
        app: app.into(),
        title: title.into(),
        version: version.into(),
        dir,
        log_file,
        tail: VecDeque::with_capacity(LOG_TAIL),
        context: BTreeMap::new(),
        started: SystemTime::now(),
        dialogs,
    };
    if STATE.set(Mutex::new(st)).is_err() {
        return; // schon eingerichtet
    }
    std::panic::set_hook(Box::new(|info| {
        let msg = match info.payload().downcast_ref::<&str>() {
            Some(s) => s.to_string(),
            None => info
                .payload()
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "(unbekannt)".into()),
        };
        let place = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        let bt = std::backtrace::Backtrace::force_capture();
        let text = format!("Panic im Thread '{thread}' bei {place}:\n{msg}\n\nAufrufliste:\n{bt}");
        eprintln!("{text}");
        let path = write_report("Absturz (Panic)", &text);
        show_crash_dialog(path.as_deref());
    }));
    native::install();
    log(&format!("{title} {version} gestartet"));
}

/// Eine Zeile ins Protokoll (und auf die Konsole, falls vorhanden).
pub fn log(line: &str) {
    eprintln!("{line}");
    let Some(m) = state() else { return };
    let Ok(mut st) = m.lock() else { return };
    let secs = st.started.elapsed().map(|d| d.as_secs_f32()).unwrap_or(0.0);
    let entry = format!("[{secs:8.2}] {line}");
    if let Some(f) = &mut st.log_file {
        let _ = writeln!(f, "{entry}");
        let _ = f.flush();
    }
    if st.tail.len() >= LOG_TAIL {
        st.tail.pop_front();
    }
    st.tail.push_back(entry);
}

/// Momentaufnahme setzen (kommt in jeden Bericht), z. B. `context("Stufe", "falsch")`.
pub fn context(key: &str, value: impl Into<String>) {
    let Some(m) = state() else { return };
    if let Ok(mut st) = m.lock() {
        st.context.insert(key.into(), value.into());
    }
}

/// Geordneter Abbruch: Bericht schreiben, dem Spieler `player_msg` zeigen.
pub fn fatal(what: &str, detail: &str, player_msg: &str) {
    log(&format!("FEHLER: {what}: {detail}"));
    let path = write_report(
        what,
        &format!("{detail}\n\nHinweis fuer den Spieler: {player_msg}"),
    );
    let dialogs = state()
        .and_then(|m| m.try_lock().ok().map(|s| s.dialogs))
        .unwrap_or(true);
    if !dialogs {
        return;
    }
    if path
        .as_deref()
        .is_some_and(|p| crate::crash_window::spawn(p, Some(player_msg)))
    {
        return;
    }
    let mut text = player_msg.to_string();
    if let Some(p) = &path {
        text.push_str(&format!("\n\nBericht: {}", p.display()));
    }
    native::message_box(&app_title(), &text);
}

fn app_title() -> String {
    state()
        .and_then(|m| m.try_lock().ok().map(|s| s.title.clone()))
        .unwrap_or_else(|| "Fehler".into())
}

/// Bericht schreiben; liefert den Pfad. Verwendet `try_lock`, damit ein Absturz waehrend
/// eines Protokolleintrags nicht haengen bleibt.
pub fn write_report(kind: &str, detail: &str) -> Option<PathBuf> {
    let m = state()?;
    let st = match m.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
        Err(_) => return None,
    };
    let dir = st.dir.as_ref()?.join("crashes");
    fs::create_dir_all(&dir).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    let path = dir.join(format!("crash-{}.txt", stamp(now)));
    let mut out = String::new();
    out.push_str(&format!("{} {}: {kind}\n", st.title, st.version));
    out.push_str(&format!("Zeit (UTC): {}\n", stamp(now)));
    let up = st.started.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    out.push_str(&format!("Laufzeit: {} min {} s\n", up / 60, up % 60));
    out.push_str(&format!(
        "System: {} {}, Moduladresse 0x{:x}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        native::module_base()
    ));
    out.push_str("\n== Fehler ==\n");
    out.push_str(detail);
    out.push_str("\n\n== Zustand ==\n");
    for (k, v) in &st.context {
        out.push_str(&format!("{k}: {v}\n"));
    }
    out.push_str("\n== Letzte Protokollzeilen ==\n");
    for l in &st.tail {
        out.push_str(l);
        out.push('\n');
    }
    fs::write(&path, out).ok()?;
    prune(&dir);
    Some(path)
}

/// Nur die neuesten Berichte behalten.
fn prune(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("crash-"))
        })
        .collect();
    files.sort();
    while files.len() > KEEP_REPORTS {
        let _ = fs::remove_file(files.remove(0));
    }
}

/// `JJJJ-MM-TT_hh-mm-ss` (UTC) aus Unix-Sekunden, ohne Zusatzbibliothek.
fn stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Kalender aus Tagen seit 1970 (Howard Hinnant, civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

fn show_crash_dialog(path: Option<&Path>) {
    let (title, dialogs) = match state().and_then(|m| m.try_lock().ok()) {
        Some(st) => (st.title.clone(), st.dialogs),
        None => ("Das Spiel".into(), true),
    };
    if !dialogs {
        return;
    }
    // eigenes Absturzfenster in einem frischen Prozess, sonst eine einfache Meldung
    if path.is_some_and(|p| crate::crash_window::spawn(p, None)) {
        return;
    }
    let mut text = format!("{title} ist abgestürzt. Das tut uns leid.");
    if let Some(p) = path {
        text.push_str(&format!(
            "\n\nEin Absturzbericht liegt hier:\n{}\n\nSchick ihn gern den Entwicklern.",
            p.display()
        ));
    }
    native::message_box(&title, &text);
}

#[allow(dead_code)]
fn app_name() -> Option<String> {
    state()?.lock().ok().map(|s| s.app.clone())
}

#[cfg(windows)]
#[path = "native_windows.rs"]
mod native;
#[cfg(not(windows))]
#[path = "native_other.rs"]
mod native;

#[cfg(test)]
mod tests;
