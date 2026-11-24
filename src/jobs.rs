//! Arbeit auf mehrere Prozessorkerne verteilen.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

type Job = Box<dyn FnOnce() + Send>;

/// Auftraege, gemeinsam fuer alle Faeden.
#[derive(Default)]
struct Queue {
    jobs: Mutex<(std::collections::VecDeque<Job>, bool)>,
    wake: Condvar,
}

/// Feste Zahl Faeden, die Auftraege abarbeiten. Beim Freigeben werden angefangene Auftraege
/// fertig, wartende verfallen.
pub struct Pool {
    queue: Arc<Queue>,
    threads: Vec<JoinHandle<()>>,
}

/// Ergebnis eines Auftrags.
pub struct Task<T> {
    rx: Receiver<T>,
}

impl<T> Task<T> {
    /// Ergebnis, sobald es da ist; wartet nie. Danach wieder `None`.
    pub fn poll(&self) -> Option<T> {
        self.rx.try_recv().ok()
    }

    /// Wartet auf das Ergebnis. `None` = der Auftrag ist abgestuerzt oder verfallen.
    pub fn wait(self) -> Option<T> {
        self.rx.recv().ok()
    }
}

/// So viele Faeden lohnen sich neben dem Spielfaden.
pub fn worker_count() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1).clamp(1, 8))
}

impl Pool {
    /// `threads` Faeden (mindestens einer).
    pub fn new(threads: usize) -> Self {
        let queue = Arc::new(Queue::default());
        let threads = (0..threads.max(1))
            .filter_map(|i| {
                let q = queue.clone();
                std::thread::Builder::new()
                    .name(format!("arbeit-{i}"))
                    .spawn(move || work(&q))
                    .ok()
            })
            .collect();
        Self { queue, threads }
    }

    /// Auftrag starten.
    pub fn spawn<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Task<T> {
        let (tx, rx): (Sender<T>, _) = channel();
        let job: Job = Box::new(move || {
            let _ = tx.send(f());
        });
        if let Ok(mut q) = self.queue.jobs.lock() {
            q.0.push_back(job);
        }
        self.queue.wake.notify_one();
        Task { rx }
    }
}

fn work(q: &Queue) {
    loop {
        let job = {
            let Ok(mut state) = q.jobs.lock() else {
                return;
            };
            loop {
                if state.1 {
                    return;
                }
                if let Some(j) = state.0.pop_front() {
                    break j;
                }
                state = match q.wake.wait(state) {
                    Ok(s) => s,
                    Err(_) => return,
                };
            }
        };
        // ein abgestuerzter Auftrag soll den Faden nicht mitnehmen
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        if let Ok(mut q) = self.queue.jobs.lock() {
            q.1 = true;
            q.0.clear();
        }
        self.queue.wake.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Wendet `f` auf alle Eintraege an, auf mehrere Kerne verteilt, und wartet. Lohnt sich ab
/// einigen hundert Mikrosekunden Arbeit.
pub fn for_each<T: Send>(items: &mut [T], f: impl Fn(&mut T) + Sync) {
    let n = worker_count() + 1;
    let chunk = items.len().div_ceil(n).max(1);
    std::thread::scope(|s| {
        let mut parts = items.chunks_mut(chunk);
        let first = parts.next();
        for part in parts {
            let f = &f;
            s.spawn(move || part.iter_mut().for_each(f));
        }
        if let Some(p) = first {
            p.iter_mut().for_each(&f);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_run_and_crashes_do_not_stop_the_pool() {
        let p = Pool::new(2);
        let tasks: Vec<_> = (0..50).map(|i| p.spawn(move || i * 2)).collect();
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        assert!(p.spawn(|| -> i32 { panic!("Absicht") }).wait().is_none());
        std::panic::set_hook(quiet);
        for (i, t) in tasks.into_iter().enumerate() {
            assert_eq!(t.wait(), Some(i as i32 * 2));
        }
        assert_eq!(p.spawn(|| 7).wait(), Some(7));
    }

    #[test]
    fn for_each_touches_everything_once() {
        let mut v: Vec<u32> = (0..1000).collect();
        for_each(&mut v, |x| *x += 1);
        assert!(v.iter().enumerate().all(|(i, x)| *x == i as u32 + 1));
        for_each(&mut Vec::<u32>::new(), |_| {});
    }
}
