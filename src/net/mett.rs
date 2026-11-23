//! Verbindung ueber den Mett Launcher (`mett-net`): der Launcher betreibt die Peer-to-Peer-
//! Verbindungen, das Spiel spricht nur lokal mit ihm. Nur verfuegbar, wenn das Spiel ueber den
//! Launcher gestartet wurde.

use super::{Delivery, NetEvent, Transport};
use mett_net::{Event, LaunchAction, MettNet, Reliability};

/// Was der Spieler im Launcher gewaehlt hat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// Spiel hosten
    Host,
    /// Mit Code beitreten
    Join(String),
    /// Nichts gewaehlt (das Spiel kann ein eigenes Menue anbieten)
    None,
}

pub struct MettTransport {
    net: MettNet,
    host: bool,
    pending: Vec<NetEvent>,
}

impl MettTransport {
    /// Verbindung zum Netzdienst des Launchers. `Ok(None)`: nicht ueber den Launcher gestartet.
    pub fn from_launcher() -> std::io::Result<Option<(Self, Launch)>> {
        let Some(net) = MettNet::from_env()? else {
            return Ok(None);
        };
        let launch = match net.launch_action() {
            LaunchAction::Host => Launch::Host,
            LaunchAction::Join(code) => Launch::Join(code),
            LaunchAction::None => Launch::None,
        };
        Ok(Some((
            Self {
                net,
                host: false,
                pending: Vec::new(),
            },
            launch,
        )))
    }

    /// Fuer Mitspieler oeffnen (`max_peers` ohne den Host, 0 = unbegrenzt).
    pub fn host(&mut self, max_peers: u16) {
        self.host = true;
        if let Err(e) = self.net.host(max_peers) {
            self.pending.push(NetEvent::Error {
                message: e.to_string(),
            });
        }
    }

    /// Einem Host per Code beitreten.
    pub fn join(&mut self, code: &str) {
        self.host = false;
        if let Err(e) = self.net.join(code) {
            self.pending.push(NetEvent::Error {
                message: e.to_string(),
            });
        }
    }
}

impl Transport for MettTransport {
    fn poll(&mut self) -> Vec<NetEvent> {
        let mut out = std::mem::take(&mut self.pending);
        for ev in self.net.poll() {
            out.push(match ev {
                Event::Hosted { code } => NetEvent::Hosted { code },
                Event::Joined { host_name, .. } => NetEvent::Joined { host_name },
                Event::PeerJoined { peer, name, .. } => NetEvent::PeerJoined { peer, name },
                Event::PeerLeft { peer, .. } => NetEvent::PeerLeft { peer },
                Event::Message { from, data, .. } => NetEvent::Message { from, data },
                Event::SessionEnded { reason } => NetEvent::Ended { reason },
                Event::Disconnected => NetEvent::Ended {
                    reason: "Verbindung zum Launcher verloren".into(),
                },
                Event::Error { message } => NetEvent::Error { message },
                _ => continue,
            });
        }
        out
    }

    fn send(&mut self, to: u16, data: &[u8], delivery: Delivery) {
        let r = match delivery {
            Delivery::Reliable => Reliability::Reliable,
            Delivery::Unreliable => Reliability::Unreliable,
        };
        if let Err(e) = self.net.send(to, data, r) {
            self.pending.push(NetEvent::Error {
                message: e.to_string(),
            });
        }
    }

    fn is_host(&self) -> bool {
        self.host
    }

    fn leave(&mut self) {
        let _ = self.net.leave();
    }
}
