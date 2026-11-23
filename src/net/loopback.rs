//! Verbindung im Speicher: ein Host und beliebig viele Mitspieler im selben Prozess.
//! Fuer Tests von Netzcode ohne Netzwerk. Alles kommt sofort und vollstaendig an.

use super::{Delivery, NetEvent, Transport, BROADCAST, HOST};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

#[derive(Default)]
struct Hub {
    /// Postfach je Teilnehmer (Index 0 = Host, 1.. = Mitspieler)
    inbox: Vec<VecDeque<NetEvent>>,
    names: Vec<String>,
    gone: Vec<bool>,
}

/// Ein Teilnehmer im Speicher-Netz.
pub struct Loopback {
    hub: Rc<RefCell<Hub>>,
    /// eigene Nummer: 0 = Host
    me: u16,
}

impl Loopback {
    /// Host plus Mitspieler mit den Namen `names` (erster Name = Host).
    pub fn network(names: &[&str]) -> Vec<Loopback> {
        let n = names.len();
        let hub = Rc::new(RefCell::new(Hub {
            inbox: vec![VecDeque::new(); n],
            names: names.iter().map(|s| s.to_string()).collect(),
            gone: vec![false; n],
        }));
        {
            let mut h = hub.borrow_mut();
            h.inbox[0].push_back(NetEvent::Hosted {
                code: "LOOPBACK".into(),
            });
            for p in 1..n {
                let name = h.names[p].clone();
                h.inbox[0].push_back(NetEvent::PeerJoined {
                    peer: p as u16,
                    name,
                });
                let host_name = h.names[0].clone();
                h.inbox[p].push_back(NetEvent::Joined { host_name });
            }
        }
        (0..n)
            .map(|i| Loopback {
                hub: hub.clone(),
                me: i as u16,
            })
            .collect()
    }
}

impl Transport for Loopback {
    fn poll(&mut self) -> Vec<NetEvent> {
        self.hub.borrow_mut().inbox[self.me as usize]
            .drain(..)
            .collect()
    }

    fn send(&mut self, to: u16, data: &[u8], _delivery: Delivery) {
        let mut h = self.hub.borrow_mut();
        if h.gone[self.me as usize] {
            return;
        }
        let targets: Vec<usize> = if self.me != HOST {
            vec![0] // Mitspieler senden nur an den Host
        } else if to == BROADCAST {
            (1..h.inbox.len()).collect()
        } else {
            vec![to as usize]
        };
        // Aus Sicht eines Mitspielers ist der Absender immer der Host (0)
        for t in targets {
            if t < h.inbox.len() && !h.gone[t] {
                h.inbox[t].push_back(NetEvent::Message {
                    from: self.me,
                    data: data.to_vec(),
                });
            }
        }
    }

    fn is_host(&self) -> bool {
        self.me == HOST
    }

    fn leave(&mut self) {
        let mut h = self.hub.borrow_mut();
        h.gone[self.me as usize] = true;
        if self.me == HOST {
            for p in 1..h.inbox.len() {
                h.inbox[p].push_back(NetEvent::Ended {
                    reason: "Host hat verlassen".into(),
                });
            }
        } else {
            let peer = self.me;
            h.inbox[0].push_back(NetEvent::PeerLeft { peer });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_and_peers_exchange_messages() {
        let mut net = Loopback::network(&["Host", "A", "B"]);
        let joined: Vec<_> = net[0].poll();
        assert!(joined.contains(&NetEvent::PeerJoined {
            peer: 2,
            name: "B".into()
        }));
        net[1].send(HOST, b"hallo", Delivery::Unreliable);
        net[0].send(BROADCAST, b"alle", Delivery::Reliable);
        net[0].send(2, b"nur B", Delivery::Reliable);
        assert_eq!(
            net[0].poll(),
            vec![NetEvent::Message {
                from: 1,
                data: b"hallo".to_vec()
            }]
        );
        let b: Vec<_> = net[2].poll();
        assert_eq!(b.len(), 3, "Joined + 2 Nachrichten: {b:?}");
        net[0].leave();
        assert!(matches!(net[1].poll().last(), Some(NetEvent::Ended { .. })));
    }
}
