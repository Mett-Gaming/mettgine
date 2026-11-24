//! Offene Erfolgsmeldungen: zusammenfassen, wiederholen, bestaetigen.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Ohne Bestaetigung erneut senden nach
const RETRY: Duration = Duration::from_secs(5);
/// Hoechste Laenge einer Id
const MAX_ID: usize = 64;

/// Stand eines Erfolgs; `value` `None` = freischalten.
#[derive(Clone, Copy, Debug)]
struct Entry {
    value: Option<u32>,
    n: u64,
    sent: Option<Instant>,
    done: bool,
}

#[derive(Debug, Default)]
pub(super) struct Queue {
    entries: BTreeMap<String, Entry>,
    next: u64,
}

impl Queue {
    /// Neue Meldung; `false` = schon bekannt oder ungueltige Id.
    pub fn add(&mut self, id: &str, value: Option<u32>) -> bool {
        if let Some(e) = self.entries.get(id) {
            let newer = match (e.value, value) {
                (None, _) => false,
                (Some(_), None) => true,
                (Some(old), Some(new)) => new > old,
            };
            if !newer {
                return false;
            }
        } else if !valid(id) {
            return false;
        }
        self.next += 1;
        let entry = Entry {
            value,
            n: self.next,
            sent: None,
            done: false,
        };
        self.entries.insert(id.to_string(), entry);
        true
    }

    /// Zeilen, die jetzt zu senden sind.
    pub fn due(&mut self, now: Instant) -> Vec<String> {
        let mut out = Vec::new();
        for (id, e) in &mut self.entries {
            if e.done || e.sent.is_some_and(|t| now - t < RETRY) {
                continue;
            }
            e.sent = Some(now);
            let n = e.n;
            out.push(match e.value {
                None => format!("{{\"unlock\":\"{id}\",\"n\":{n}}}\n"),
                Some(v) => format!("{{\"progress\":\"{id}\",\"value\":{v},\"n\":{n}}}\n"),
            });
        }
        out
    }

    /// Bestaetigung des Launchers; veraltete Nummern zaehlen nicht.
    pub fn ack(&mut self, n: u64) {
        for e in self.entries.values_mut().filter(|e| e.n == n) {
            e.done = true;
        }
    }

    /// Verbindung weg: alles Offene gilt als nicht gesendet.
    pub fn reset(&mut self) {
        for e in self.entries.values_mut() {
            e.sent = None;
        }
    }

    pub fn is_done(&self) -> bool {
        self.entries.values().all(|e| e.done)
    }
}

/// Kleinbuchstaben, Ziffern, Unterstrich (dieselbe Regel wie im Launcher).
fn valid(id: &str) -> bool {
    let ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
    !id.is_empty() && id.len() <= MAX_ID && id.chars().all(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_merged_repeated_and_confirmed() {
        let (mut q, t) = (Queue::default(), Instant::now());
        assert!(q.add("laufen", Some(3)) && q.add("laufen", Some(5)));
        assert!(!q.add("laufen", Some(4)), "sinkt nie");
        assert!(!q.add("Böse", None) && !q.add("", None));
        let first = q.due(t);
        assert_eq!(first, ["{\"progress\":\"laufen\",\"value\":5,\"n\":2}\n"]);
        assert!(q.due(t).is_empty());
        assert_eq!(q.due(t + RETRY), first, "ohne Bestaetigung noch einmal");
        assert!(q.add("laufen", None));
        q.ack(2); // gilt der alten Meldung
        assert!(!q.is_done());
        assert_eq!(q.due(t), ["{\"unlock\":\"laufen\",\"n\":3}\n"]);
        q.reset();
        assert_eq!(q.due(t).len(), 1, "nach neuer Verbindung sofort");
        q.ack(3);
        assert!(q.is_done() && !q.add("laufen", None) && !q.add("laufen", Some(9)));
    }
}
