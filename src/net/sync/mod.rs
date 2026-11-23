//! Bausteine fuer fluessigen Mehrspieler. Was abgeglichen wird, bestimmt das Spiel.
//!
//! - [`wire`]: Zahlen platzsparend in Nachrichten schreiben und sicher lesen.
//! - [`clock`]: fester Takt und die geschaetzte Uhr des Hosts.
//! - [`interp`]: Zustaende anderer Spieler zwischen zwei Nachrichten fluessig anzeigen.
//! - [`predict`]: eigene Eingaben sofort anwenden und mit dem Host abgleichen.
//!
//! Ueblicher Ablauf: Mitspieler senden im festen Takt nummerierte Eingaben
//! ([`predict::Predictor::recent`]). Der Host rechnet sie der Reihe nach
//! ([`predict::InputQueue`]) und sendet Zustand + Nummer der zuletzt gerechneten Eingabe.

pub mod clock;
pub mod interp;
pub mod predict;
#[cfg(test)]
mod tests;
pub mod wire;

/// Ist `a` neuer als `b`? Laufende Nummern duerfen ueberlaufen.
pub fn newer(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) > 0
}
