//! Faden mit der Verbindung zum Launcher.

use super::queue::Queue;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Wartezeit fuer Verbinden und Senden
const TIMEOUT: Duration = Duration::from_secs(2);
/// So lange wartet der Faden auf Antworten, bevor er wieder sendet
const TURN: Duration = Duration::from_millis(15);
/// Pause vor einer neuen Verbindung
const RECONNECT: Duration = Duration::from_secs(5);
/// So lange darf das Beenden auf offene Erfolge warten
const FAREWELL: Duration = Duration::from_millis(300);
/// Laengste Zeile des Launchers
const MAX_LINE: usize = 4096;
/// Hoechstzahl wartender Overlay-Meldungen
const MAX_UI: usize = 256;

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    ui: Mutex<Vec<String>>,
    overlay: AtomicBool,
    online: AtomicBool,
    stop: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Griff auf die Verbindung; mit dem letzten endet der Faden.
#[derive(Clone)]
pub(crate) struct Link {
    shared: Arc<Shared>,
    _end: Arc<End>,
}

struct End(Arc<Shared>);

impl Drop for End {
    fn drop(&mut self) {
        let until = Instant::now() + FAREWELL;
        while self.0.online.load(Ordering::Relaxed)
            && !lock(&self.0.queue).is_done()
            && Instant::now() < until
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.0.stop.store(true, Ordering::Relaxed);
    }
}

impl Link {
    pub fn connect(addr: &str, token: &str) -> Option<Self> {
        let addr: SocketAddr = addr.parse().ok()?;
        let clean = !token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric());
        if !addr.ip().is_loopback() || !clean {
            return None;
        }
        let hello = format!("{{\"token\":\"{token}\",\"link\":2}}\n");
        let shared = Arc::new(Shared::default());
        let state = shared.clone();
        std::thread::Builder::new()
            .name("launcher".into())
            .spawn(move || run(addr, &hello, &state))
            .ok()?;
        let end = Arc::new(End(shared.clone()));
        Some(Self { shared, _end: end })
    }

    pub fn report(&self, id: &str, value: Option<u32>) {
        lock(&self.shared.queue).add(id, value);
    }

    /// Overlay-Meldung (eine JSON-Zeile ohne Umbruch).
    pub fn ui(&self, line: String) {
        let mut ui = lock(&self.shared.ui);
        if ui.len() < MAX_UI && self.shared.online.load(Ordering::Relaxed) {
            ui.push(line);
        }
    }

    /// Ist das Overlay-Feld offen?
    pub fn overlay(&self) -> bool {
        self.shared.overlay.load(Ordering::Relaxed)
    }
}

fn run(addr: SocketAddr, hello: &str, s: &Shared) {
    while !s.stop.load(Ordering::Relaxed) {
        if let Ok(stream) = TcpStream::connect_timeout(&addr, TIMEOUT) {
            let _ = session(stream, hello, s);
        }
        s.online.store(false, Ordering::Relaxed);
        s.overlay.store(false, Ordering::Relaxed);
        lock(&s.ui).clear();
        lock(&s.queue).reset();
        let until = Instant::now() + RECONNECT;
        while Instant::now() < until && !s.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Bis zum Abbruch der Verbindung oder zum Spielende.
fn session(mut stream: TcpStream, hello: &str, s: &Shared) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.set_read_timeout(Some(TURN))?;
    stream.write_all(hello.as_bytes())?;
    s.online.store(true, Ordering::Relaxed);
    let mut input = Vec::new();
    let mut buffer = [0u8; 512];
    while !s.stop.load(Ordering::Relaxed) {
        let mut out = lock(&s.queue).due(Instant::now()).concat();
        for line in lock(&s.ui).drain(..) {
            out += &line;
            out.push('\n');
        }
        stream.write_all(out.as_bytes())?;
        match stream.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(n) => input.extend_from_slice(&buffer[..n]),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) => return Err(e),
        }
        while let Some(end) = input.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = input.drain(..=end).collect();
            handle(&String::from_utf8_lossy(&line), s);
        }
        if input.len() > MAX_LINE {
            return Ok(());
        }
    }
    Ok(())
}

/// Eine Zeile des Launchers; Unbekanntes faellt weg.
fn handle(line: &str, s: &Shared) {
    let inner = line.trim().trim_start_matches('{').trim_end_matches('}');
    let Some((key, value)) = inner.split_once(':') else {
        return;
    };
    let value = value.trim();
    match key.trim().trim_matches('"') {
        "ack" => {
            if let Ok(n) = value.parse() {
                lock(&s.queue).ack(n);
            }
        }
        "overlay" => s.overlay.store(value == "true", Ordering::Relaxed),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    fn until(what: &str, ok: impl Fn() -> bool) {
        let start = Instant::now();
        while !ok() {
            assert!(start.elapsed() < Duration::from_secs(5), "{what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn unlocks_arrive_and_the_launcher_opens_the_overlay() {
        assert!(Link::connect("8.8.8.8:80", "t").is_none());
        assert!(Link::connect("127.0.0.1:1", "t\"}").is_none());
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let link = Link::connect(&l.local_addr().unwrap().to_string(), "geheim").unwrap();
        link.report("erste", None);
        let (mut w, _) = l.accept().unwrap();
        let mut r = BufReader::new(w.try_clone().unwrap());
        let mut next = || {
            let mut s = String::new();
            r.read_line(&mut s).unwrap();
            s
        };
        assert_eq!(next(), "{\"token\":\"geheim\",\"link\":2}\n");
        assert_eq!(next(), "{\"unlock\":\"erste\",\"n\":1}\n");
        w.write_all(b"M\xfcll\n{\"ack\":1}\n{\"overlay\":true}\n")
            .unwrap();
        until("Overlay", || link.overlay());
        assert!(lock(&link.shared.queue).is_done());
        link.ui("{\"ui\":[\"ok\"]}".into());
        assert_eq!(next(), "{\"ui\":[\"ok\"]}\n");
        drop((w, r));
        until("getrennt", || !link.overlay());
    }

    #[test]
    fn leaving_waits_shortly_for_open_reports() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let link = Link::connect(&l.local_addr().unwrap().to_string(), "t").unwrap();
        let (w, _) = l.accept().unwrap();
        until("verbunden", || link.shared.online.load(Ordering::Relaxed));
        link.report("letzte", None);
        let t = Instant::now();
        drop(link);
        assert!(t.elapsed() < Duration::from_secs(2));
        let lines: Vec<String> = BufReader::new(w).lines().map_while(Result::ok).collect();
        assert_eq!(lines[1], "{\"unlock\":\"letzte\",\"n\":1}");
    }
}
