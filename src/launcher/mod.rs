//! Verbindung zum Mett Launcher: Erfolge melden ([`Achievements`]) und das Overlay bedienen.
//! Ohne Launcher oder bei Fehlern passiert nichts; das Spiel wartet nie.
//!
//! Der Launcher uebergibt Adresse (nur localhost) und Token wie bei `presence` und zeigt mit
//! `METT_LAUNCHER_LINK` an, dass er diese Verbindung versteht. Ein eigener Faden schickt
//! JSON-Zeilen; Erfolge werden wiederholt, bis der Launcher sie bestaetigt.

mod link;
mod overlay;
mod queue;

pub use overlay::OverlayKey;

use link::Link;
pub(crate) use overlay::{Overlay, Route};

/// Erfolge des Spiels, siehe `docs/erfolge.md`. Beides darf jedes Bild aufgerufen werden:
/// Gesendet wird nur, was neu ist.
#[derive(Clone, Default)]
pub struct Achievements {
    link: Option<Link>,
}

impl Achievements {
    /// Erfolg freischalten (Id aus `erfolge/erfolge.json`).
    pub fn unlock(&self, id: &str) {
        if let Some(l) = &self.link {
            l.report(id, None);
        }
    }

    /// Fortschritt melden: der ganze Stand (nicht der Zuwachs). Sinkt nie; beim Ziel aus
    /// `erfolge/erfolge.json` wird freigeschaltet.
    pub fn progress(&self, id: &str, value: u32) {
        if let Some(l) = &self.link {
            l.report(id, Some(value));
        }
    }

    /// Nimmt ein Launcher Erfolge an?
    pub fn available(&self) -> bool {
        self.link.is_some()
    }
}

/// Verbindung aus den Angaben des Launchers; ohne Launcher bleibt alles still.
pub(crate) fn connect() -> (Achievements, Overlay) {
    let var = |k| std::env::var(k).ok();
    let link = match (
        var("METT_LAUNCHER_LINK"),
        var("METT_PRESENCE_ADDR"),
        var("METT_PRESENCE_TOKEN"),
    ) {
        (Some(_), Some(addr), Some(token)) => Link::connect(&addr, &token),
        _ => None,
    };
    let key = var("METT_OVERLAY_KEY").and_then(|k| OverlayKey::parse(&k));
    let overlay = Overlay::new(link.clone(), key);
    (Achievements { link }, overlay)
}
