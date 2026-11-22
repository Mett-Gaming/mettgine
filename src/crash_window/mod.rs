//! Absturzfenster: eigenes kleines Windows-Fenster, das nach einem Absturz in einem frischen
//! Prozess laeuft (`<exe> --crash-report <Bericht> [--hint <Text>]`). So funktioniert es auch,
//! wenn der Absturz im Grafiktreiber war. Zeigt Ueberschrift, Erklaerung und den ganzen
//! Bericht; Knoepfe: Bericht kopieren, Ordner oeffnen, Neu starten, Schliessen.
//! Reine Win32-Aufrufe, keine Zusatzbibliothek.

use std::path::PathBuf;

/// Argument, mit dem die exe als Absturzfenster startet
pub const ARG_REPORT: &str = "--crash-report";
pub const ARG_HINT: &str = "--hint";

/// Was das Absturzfenster zeigen soll (aus den Startargumenten).
#[derive(Debug, Clone, PartialEq)]
pub struct CrashArgs {
    pub report: PathBuf,
    pub hint: Option<String>,
}

/// Startargumente lesen: None = normales Spiel.
pub fn parse_args(args: &[String]) -> Option<CrashArgs> {
    let i = args.iter().position(|a| a == ARG_REPORT)?;
    let report = PathBuf::from(args.get(i + 1)?);
    let hint = args
        .iter()
        .position(|a| a == ARG_HINT)
        .and_then(|h| args.get(h + 1).cloned());
    Some(CrashArgs { report, hint })
}

/// Absturzfenster als eigenen Prozess starten. false = ging nicht (dann einfache Meldung).
pub fn spawn(report: &std::path::Path, hint: Option<&str>) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(ARG_REPORT).arg(report);
    if let Some(h) = hint {
        cmd.arg(ARG_HINT).arg(h);
    }
    cmd.spawn().is_ok()
}

/// Lage der Bedienelemente bei Fenstergroesse `w` x `h` (Pixel), Skalierung `s`:
/// (Ueberschrift, Erklaerung, Berichtsfeld, vier Knoepfe) jeweils (x, y, Breite, Hoehe).
pub fn layout(w: i32, h: i32, s: f32) -> [(i32, i32, i32, i32); 7] {
    let px = |v: f32| (v * s).round() as i32;
    let m = px(16.0);
    let bh = px(30.0);
    let bw = ((w - m * 5) / 4).max(px(60.0));
    let by = h - m - bh;
    let title = (m, m, w - 2 * m, px(30.0));
    let text = (m, m + px(36.0), w - 2 * m, px(40.0));
    let ry = m + px(84.0);
    let report = (m, ry, w - 2 * m, (by - m - ry).max(px(40.0)));
    let b = |k: i32| (m + k * (bw + m), by, bw, bh);
    [title, text, report, b(0), b(1), b(2), b(3)]
}

/// Absturzfenster anzeigen und warten, bis es geschlossen wird.
pub fn run(args: &CrashArgs, title: &str) {
    let report = std::fs::read_to_string(&args.report)
        .unwrap_or_else(|e| format!("Bericht konnte nicht gelesen werden: {e}"));
    let headline = if args.hint.is_some() {
        format!("{title} konnte nicht weiterlaufen.")
    } else {
        format!("{title} ist abgestürzt.")
    };
    let mut text = match &args.hint {
        Some(h) => h.clone(),
        None => "Das tut uns leid. Ein Absturzbericht wurde gespeichert. Schick ihn gern den \
                 Entwicklern: kopieren und einfügen genügt."
            .into(),
    };
    text.push_str(&format!("\nBericht: {}", args.report.display()));
    win::show(title, &headline, &text, &report, &args.report);
}

#[cfg(windows)]
#[path = "win_windows.rs"]
mod win;
#[cfg(not(windows))]
#[path = "win_other.rs"]
mod win;

#[cfg(test)]
mod tests;
