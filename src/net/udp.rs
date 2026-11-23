//! Lokales UDP zum Testen mehrerer Instanzen auf einem PC (der Launcher erlaubt pro Rechner nur
//! einen Host). Gedacht fuer 127.0.0.1: es gibt keine Wiederholung verlorener Pakete, daher ist
//! "zuverlaessig" hier nur so gut wie die lokale Verbindung.
//!
//! Paket: u8 Art | u16 Peer (LE) | Nutzdaten. Arten: 1 = Hallo (Name), 2 = Willkommen
//! (Peer-Nummer, Host-Name), 3 = Daten, 4 = Tschuess.

use super::{Delivery, NetEvent, Transport, BROADCAST, HOST};
use std::net::{SocketAddr, UdpSocket};

const HELLO: u8 = 1;
const WELCOME: u8 = 2;
const DATA: u8 = 3;
const BYE: u8 = 4;
/// Groesstes Paket (Byte)
const MAX_PACKET: usize = 65_000;

pub struct UdpTransport {
    socket: UdpSocket,
    host: bool,
    /// Host: Adressen der Mitspieler (Index = Peer - 1); Mitspieler: Adresse des Hosts
    peers: Vec<Option<SocketAddr>>,
    host_addr: Option<SocketAddr>,
    pending: Vec<NetEvent>,
    name: String,
}

fn packet(kind: u8, peer: u16, data: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(3 + data.len());
    p.push(kind);
    p.extend_from_slice(&peer.to_le_bytes());
    p.extend_from_slice(data);
    p
}

impl UdpTransport {
    /// Host auf `port` (alle lokalen Adressen).
    pub fn host(port: u16, name: &str) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(("0.0.0.0", port))?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            host: true,
            peers: Vec::new(),
            host_addr: None,
            pending: vec![NetEvent::Hosted {
                code: format!("127.0.0.1:{port}"),
            }],
            name: name.into(),
        })
    }

    /// Einem Host unter `addr` (z. B. "127.0.0.1:47800") beitreten.
    pub fn join(addr: &str, name: &str) -> std::io::Result<Self> {
        let host_addr: SocketAddr = addr
            .parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let socket = UdpSocket::bind(("0.0.0.0", 0))?;
        socket.set_nonblocking(true)?;
        socket.send_to(&packet(HELLO, 0, name.as_bytes()), host_addr)?;
        Ok(Self {
            socket,
            host: false,
            peers: Vec::new(),
            host_addr: Some(host_addr),
            pending: Vec::new(),
            name: name.into(),
        })
    }

    fn send_raw(&mut self, p: &[u8], to: SocketAddr) {
        if let Err(e) = self.socket.send_to(p, to) {
            self.pending.push(NetEvent::Error {
                message: e.to_string(),
            });
        }
    }

    fn handle(&mut self, buf: &[u8], from: SocketAddr, out: &mut Vec<NetEvent>) {
        if buf.len() < 3 {
            return;
        }
        let (kind, data) = (buf[0], &buf[3..]);
        if self.host {
            let known = self.peers.iter().position(|p| *p == Some(from));
            match (kind, known) {
                (HELLO, None) => {
                    self.peers.push(Some(from));
                    let peer = self.peers.len() as u16;
                    let w = packet(WELCOME, peer, self.name.as_bytes());
                    self.send_raw(&w, from);
                    out.push(NetEvent::PeerJoined {
                        peer,
                        name: String::from_utf8_lossy(data).into_owned(),
                    });
                }
                (DATA, Some(i)) => out.push(NetEvent::Message {
                    from: i as u16 + 1,
                    data: data.to_vec(),
                }),
                (BYE, Some(i)) => {
                    self.peers[i] = None;
                    out.push(NetEvent::PeerLeft { peer: i as u16 + 1 });
                }
                _ => {}
            }
        } else if Some(from) == self.host_addr {
            match kind {
                WELCOME => out.push(NetEvent::Joined {
                    host_name: String::from_utf8_lossy(data).into_owned(),
                }),
                DATA => out.push(NetEvent::Message {
                    from: HOST,
                    data: data.to_vec(),
                }),
                BYE => out.push(NetEvent::Ended {
                    reason: "Host hat verlassen".into(),
                }),
                _ => {}
            }
        }
    }
}

impl Transport for UdpTransport {
    fn poll(&mut self) -> Vec<NetEvent> {
        let mut out = std::mem::take(&mut self.pending);
        let mut buf = vec![0u8; MAX_PACKET];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((n, from)) => {
                    let data = buf[..n].to_vec();
                    self.handle(&data, from, &mut out);
                }
                // WouldBlock = nichts mehr da; ConnectionReset meldet Windows nach ICMP-Fehlern
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::ConnectionReset =>
                {
                    break
                }
                Err(e) => {
                    out.push(NetEvent::Error {
                        message: e.to_string(),
                    });
                    break;
                }
            }
        }
        out
    }

    fn send(&mut self, to: u16, data: &[u8], _delivery: Delivery) {
        let p = packet(DATA, 0, data);
        if !self.host {
            if let Some(h) = self.host_addr {
                self.send_raw(&p, h);
            }
            return;
        }
        let targets: Vec<SocketAddr> = if to == BROADCAST {
            self.peers.iter().flatten().copied().collect()
        } else {
            self.peers
                .get((to as usize).wrapping_sub(1))
                .copied()
                .flatten()
                .into_iter()
                .collect()
        };
        for t in targets {
            self.send_raw(&p, t);
        }
    }

    fn is_host(&self) -> bool {
        self.host
    }

    fn leave(&mut self) {
        let bye = packet(BYE, 0, &[]);
        let targets: Vec<SocketAddr> = if self.host {
            self.peers.iter().flatten().copied().collect()
        } else {
            self.host_addr.into_iter().collect()
        };
        for t in targets {
            self.send_raw(&bye, t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poll_until(t: &mut UdpTransport, want: usize) -> Vec<NetEvent> {
        let mut all = Vec::new();
        for _ in 0..200 {
            all.extend(t.poll());
            if all.len() >= want {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        all
    }

    #[test]
    fn local_host_and_peer_talk() {
        let mut host = UdpTransport::host(0, "H").unwrap();
        let port = host.socket.local_addr().unwrap().port();
        let mut peer = UdpTransport::join(&format!("127.0.0.1:{port}"), "P").unwrap();
        let ev = poll_until(&mut host, 2);
        assert!(ev.contains(&NetEvent::PeerJoined {
            peer: 1,
            name: "P".into()
        }));
        assert!(poll_until(&mut peer, 1).contains(&NetEvent::Joined {
            host_name: "H".into()
        }));
        peer.send(HOST, b"pos", Delivery::Unreliable);
        host.send(BROADCAST, b"snap", Delivery::Unreliable);
        assert!(poll_until(&mut host, 1).contains(&NetEvent::Message {
            from: 1,
            data: b"pos".to_vec()
        }));
        assert!(poll_until(&mut peer, 1).contains(&NetEvent::Message {
            from: HOST,
            data: b"snap".to_vec()
        }));
    }
}
