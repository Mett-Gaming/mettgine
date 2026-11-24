//! Mehrspieler-Verbindung, unabhaengig davon, wie die Spieler verbunden sind.
//!
//! Ein Spieler hostet (Peer [`HOST`] = 0), die anderen treten bei. Mitspieler senden immer an
//! den Host, der Host an einzelne Peers oder mit [`BROADCAST`] an alle. Was in den Nachrichten
//! steht (Spielzustand, Eingaben), entscheidet das Spiel.
//!
//! [`sync`]: Bausteine fuer laufenden Abgleich (Nachrichten packen, Uhr, Zwischenbilder, Vorhersage).
//!
//! Verbindungsarten ([`Transport`]):
//! - [`mett::MettTransport`]: ueber den Mett Launcher (`mett-net`, Peer-to-Peer ohne Server)
//! - [`udp::UdpTransport`]: lokales UDP zum Testen auf einem PC (mehrere Instanzen)
//! - [`loopback::Loopback`]: im Speicher, fuer Tests ohne Netzwerk

pub mod loopback;
#[cfg(feature = "mett-net")]
pub mod mett;
pub mod sync;
pub mod udp;

/// Peer-Nummer des Hosts (aus Sicht eines Mitspielers)
pub const HOST: u16 = 0;
/// Empfaenger "alle Mitspieler" (nur Host)
pub const BROADCAST: u16 = 65535;
/// Groesste unzuverlaessige Nachricht (Byte); groessere zuverlaessig senden
pub const MAX_UNRELIABLE: usize = 1000;

/// Zustellgarantie einer Nachricht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// kommt sicher und in Reihenfolge an (Ereignisse, Zustandswechsel)
    Reliable,
    /// kann verloren gehen, ist schneller (Positionen, staendig neu gesendet)
    Unreliable,
}

/// Was beim Abfragen passiert ist.
#[derive(Clone, Debug, PartialEq)]
pub enum NetEvent {
    /// Hosten laeuft; `code` geben Mitspieler zum Beitreten ein
    Hosted {
        code: String,
    },
    /// Beitritt geklappt
    Joined {
        host_name: String,
    },
    PeerJoined {
        peer: u16,
        name: String,
    },
    PeerLeft {
        peer: u16,
    },
    Message {
        from: u16,
        data: Vec<u8>,
    },
    /// Sitzung vorbei (Host weg, Verbindung verloren, selbst verlassen)
    Ended {
        reason: String,
    },
    Error {
        message: String,
    },
}

/// Eine Verbindungsart.
pub trait Transport {
    /// Neue Ereignisse (blockiert nie).
    fn poll(&mut self) -> Vec<NetEvent>;
    /// Nachricht senden; Fehler landen als `NetEvent::Error` beim naechsten `poll`.
    fn send(&mut self, to: u16, data: &[u8], delivery: Delivery);
    /// Sind wir der Host?
    fn is_host(&self) -> bool;
    /// Sitzung verlassen.
    fn leave(&mut self) {}
}
