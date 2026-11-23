//! Status fuer den Mett Launcher (Discord-Anzeige): Das Spiel sagt, was gerade passiert, der
//! Launcher zeigt es an. Ohne Launcher oder bei Fehlern passiert nichts; das Spiel wartet nie.
//!
//! Der Launcher uebergibt `METT_PRESENCE_ADDR` (nur localhost) und `METT_PRESENCE_TOKEN`.
//! Gesendet wird je Aenderung eine JSON-Zeile von einem eigenen Faden aus.

use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

/// Wartezeit zwischen zwei Meldungen
const INTERVAL: Duration = Duration::from_secs(1);
/// Wartezeit fuer Verbinden und Senden
const TIMEOUT: Duration = Duration::from_secs(2);

/// Was angezeigt wird. Keine privaten Angaben: Der Text ist oeffentlich sichtbar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// was gerade passiert, z. B. "Im Markt" (hoechstens 128 Zeichen)
    pub details: String,
    /// Zusatz, z. B. "Stufe: unruhig" (hoechstens 128 Zeichen)
    pub state: String,
    /// Mitspieler jetzt und hoechstens
    pub party: Option<(u32, u32)>,
}

impl Status {
    pub fn new(details: impl Into<String>, state: impl Into<String>) -> Self {
        Self {
            details: details.into(),
            state: state.into(),
            party: None,
        }
    }

    pub fn party(mut self, now: usize, max: usize) -> Self {
        self.party = Some((now as u32, max as u32));
        self
    }

    /// Eine Zeile fuer den Launcher; die erste traegt das Token.
    fn line(&self, token: Option<&str>) -> String {
        let mut out = String::from("{");
        if let Some(t) = token {
            out += &format!("\"token\":{},", quote(t));
        }
        out += &format!("\"details\":{},", quote(&self.details));
        out += &format!("\"state\":{}", quote(&self.state));
        if let Some((now, max)) = self.party {
            out += &format!(",\"party\":[{now},{max}]");
        }
        out + "}\n"
    }
}

/// JSON-Zeichenkette, auf 128 Zeichen gekuerzt.
fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars().take(128) {
        match c {
            '"' | '\\' => out.extend(['\\', c]),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out + "\""
}

/// Verbindung zum Launcher.
#[derive(Debug, Default)]
pub struct Presence {
    tx: Option<Sender<Status>>,
    last: Option<Status>,
}

impl Presence {
    /// Aus den Angaben des Launchers; ohne Launcher bleibt alles still.
    pub fn from_launcher() -> Self {
        let var = |k| std::env::var(k).ok();
        match (var("METT_PRESENCE_ADDR"), var("METT_PRESENCE_TOKEN")) {
            (Some(addr), Some(token)) => Self::connect(&addr, token),
            _ => Self::default(),
        }
    }

    fn connect(addr: &str, token: String) -> Self {
        let Some(addr) = addr
            .parse::<SocketAddr>()
            .ok()
            .filter(|a| a.ip().is_loopback())
        else {
            return Self::default();
        };
        let (tx, rx) = channel();
        let started = std::thread::Builder::new()
            .name("presence".into())
            .spawn(move || run(addr, &token, &rx));
        Self {
            tx: started.is_ok().then_some(tx),
            last: None,
        }
    }

    /// Status melden. Darf jedes Bild aufgerufen werden: Gesendet wird nur bei Aenderungen.
    pub fn set(&mut self, s: Status) {
        if self.tx.is_none() || self.last.as_ref() == Some(&s) {
            return;
        }
        if let Some(tx) = &self.tx {
            if tx.send(s.clone()).is_err() {
                self.tx = None;
            }
        }
        self.last = Some(s);
    }
}

/// Faden: verbindet sich einmal und sendet immer nur den neuesten Status.
fn run(addr: SocketAddr, token: &str, rx: &Receiver<Status>) {
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, TIMEOUT) else {
        return;
    };
    let _ = stream.set_write_timeout(Some(TIMEOUT));
    let mut token = Some(token);
    while let Ok(mut s) = rx.recv() {
        while let Ok(newer) = rx.try_recv() {
            s = newer;
        }
        if stream.write_all(s.line(token.take()).as_bytes()).is_err() {
            return;
        }
        std::thread::sleep(INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    #[test]
    fn lines_are_valid_json_and_short() {
        let s = Status::new("Sagt \"Hallo\\\"", "Zeile\numbruch").party(2, 8);
        let l = s.line(Some("abc"));
        assert_eq!(
            l,
            "{\"token\":\"abc\",\"details\":\"Sagt \\\"Hallo\\\\\\\"\",\"state\":\"Zeile umbruch\",\"party\":[2,8]}\n"
        );
        assert_eq!(quote(&"ä".repeat(500)).chars().count(), 130);
    }

    #[test]
    fn newest_status_arrives_and_nothing_happens_without_launcher() {
        let mut off = Presence::connect("kein-ort", "t".into());
        off.set(Status::new("a", ""));
        assert!(Presence::connect("8.8.8.8:80", "t".into()).tx.is_none());

        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut p = Presence::connect(&l.local_addr().unwrap().to_string(), "geheim".into());
        let mut r = BufReader::new(l.accept().unwrap().0);
        let mut next = || {
            let mut s = String::new();
            r.read_line(&mut s).unwrap();
            s
        };
        p.set(Status::new("eins", ""));
        assert!(next().starts_with("{\"token\":\"geheim\",\"details\":\"eins\""));
        p.set(Status::new("eins", ""));
        p.set(Status::new("zwei", "x"));
        assert!(next().starts_with("{\"details\":\"zwei\""));
    }
}
