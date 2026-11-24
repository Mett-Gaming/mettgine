//! Leistung messen: Zeiten pro Bild von Prozessor und Grafikkarte, gleitende Auswertung und
//! eine kleine Anzeige.
//!
//! Die Renderer liefern [`FrameStats`] (`stats()`), [`Meter`] fasst viele Bilder zusammen,
//! [`Meter::lines`] macht daraus Text und [`draw_overlay`] zeichnet ihn.

use crate::canvas::{alpha, Align, Canvas, BLACK, WHITE};

/// Hoechstzahl gemessener Abschnitte der Grafikkarte pro Bild
pub const MAX_PASSES: usize = 8;

/// Zeit eines Abschnitts auf der Grafikkarte.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PassTime {
    pub name: &'static str,
    /// Millisekunden
    pub ms: f32,
}

/// Messwerte eines Bildes. GPU-Zeiten gehoeren zu einem um wenige Bilder aelteren Bild.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    /// Prozessorzeit im Renderer ohne Warten auf die Grafikkarte (ms)
    pub cpu_ms: f32,
    /// Grafikkarte gesamt (ms); 0 = nicht messbar
    pub gpu_ms: f32,
    /// Abschnitte; unbenutzte haben einen leeren Namen
    pub passes: [PassTime; MAX_PASSES],
    /// gezeichnete Objekte (Modelle + Figuren) im Kamerabild
    pub drawn: u32,
    /// aussortierte Objekte (nicht im Bild)
    pub culled: u32,
    /// Zeichenbefehle
    pub draw_calls: u32,
    /// Objekte, die wegen einer Obergrenze fehlen (sollte 0 sein)
    pub dropped: u32,
}

/// Wie viele Bilder die Auswertung umfasst
const WINDOW: usize = 240;

/// Gleitende Auswertung der letzten Bilder.
#[derive(Clone, Debug, Default)]
pub struct Meter {
    frame_ms: Vec<f32>,
    next: usize,
    last: FrameStats,
    cpu: f32,
    gpu: f32,
    passes: [PassTime; MAX_PASSES],
    scratch: Vec<f32>,
}

impl Meter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ein Bild eintragen: Dauer `dt` (s, Bild zu Bild) und die Werte des Renderers.
    pub fn push(&mut self, dt: f32, s: &FrameStats) {
        let ms = dt.max(0.0) * 1000.0;
        if self.frame_ms.len() < WINDOW {
            self.frame_ms.push(ms);
        } else {
            self.frame_ms[self.next] = ms;
        }
        self.next = (self.next + 1) % WINDOW;
        let k = if self.frame_ms.len() == 1 { 1.0 } else { 0.05 };
        self.cpu += (s.cpu_ms - self.cpu) * k;
        self.gpu += (s.gpu_ms - self.gpu) * k;
        for (avg, new) in self.passes.iter_mut().zip(&s.passes) {
            let k = if avg.name == new.name { k } else { 1.0 };
            avg.name = new.name;
            avg.ms += (new.ms - avg.ms) * k;
        }
        self.last = *s;
    }

    /// Mittlere Bilddauer (ms).
    pub fn frame_avg(&self) -> f32 {
        let n = self.frame_ms.len().max(1) as f32;
        self.frame_ms.iter().sum::<f32>() / n
    }

    /// Bilder pro Sekunde im Mittel.
    pub fn fps(&self) -> f32 {
        let a = self.frame_avg();
        if a > 0.0 {
            1000.0 / a
        } else {
            0.0
        }
    }

    /// Bilddauer, die 99 % der Bilder nicht ueberschreiten (ms): zeigt Ruckler.
    pub fn frame_p99(&mut self) -> f32 {
        self.scratch.clear();
        self.scratch.extend_from_slice(&self.frame_ms);
        self.scratch.sort_by(f32::total_cmp);
        let n = self.scratch.len();
        if n == 0 {
            return 0.0;
        }
        let i = (n as f32 * 0.99).ceil() as usize;
        self.scratch[i.clamp(1, n) - 1]
    }

    /// Geglaettete Zeiten: Prozessor, Grafikkarte (ms).
    pub fn times(&self) -> (f32, f32) {
        (self.cpu, self.gpu)
    }

    /// Textzeilen fuer die Anzeige.
    pub fn lines(&mut self) -> Vec<String> {
        let mut out = vec![
            format!(
                "{:.0} B/s  {:.1} ms  (99 %: {:.1} ms)",
                self.fps(),
                self.frame_avg(),
                self.frame_p99()
            ),
            format!("CPU {:.2} ms  GPU {:.2} ms", self.cpu, self.gpu),
        ];
        for p in self.passes.iter().filter(|p| !p.name.is_empty()) {
            out.push(format!("  {}: {:.2} ms", p.name, p.ms));
        }
        let s = &self.last;
        out.push(format!(
            "{} gezeichnet, {} aussortiert, {} Befehle",
            s.drawn, s.culled, s.draw_calls
        ));
        if s.dropped > 0 {
            out.push(format!("{} Objekte fehlen (Obergrenze)", s.dropped));
        }
        out
    }
}

/// Zeichnet die Messanzeige mit der linken oberen Ecke bei (`x`, `y`), Schrifthoehe `size`.
pub fn draw_overlay(c: &mut Canvas, x: f32, y: f32, size: f32, lines: &[String]) {
    let pad = size * 0.5;
    let step = size * 1.3;
    let w = lines
        .iter()
        .map(|l| c.text_width(l, size))
        .fold(0.0f32, f32::max);
    let h = lines.len() as f32 * step;
    c.rect(x, y, w + 2.0 * pad, h + 2.0 * pad, alpha(BLACK, 0.7));
    for (i, l) in lines.iter().enumerate() {
        c.text(
            x + pad,
            y + pad + i as f32 * step,
            l,
            size,
            WHITE,
            Align::Left,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_averages_and_finds_stutter() {
        let mut m = Meter::new();
        assert_eq!((m.fps(), m.frame_p99()), (0.0, 0.0), "leer ist kein Fehler");
        for i in 0..200 {
            let dt = if i % 50 == 0 { 0.050 } else { 0.010 };
            m.push(dt, &FrameStats::default());
        }
        assert!((m.frame_avg() - 10.8).abs() < 0.01);
        assert!(
            (m.frame_p99() - 50.0).abs() < 0.01,
            "Ruckler bleiben sichtbar"
        );
        for _ in 0..WINDOW {
            m.push(0.010, &FrameStats::default());
        }
        assert!(
            (m.frame_p99() - 10.0).abs() < 0.01,
            "alte Bilder fallen heraus"
        );
    }

    #[test]
    fn lines_name_the_passes_and_missing_objects() {
        let mut s = FrameStats {
            gpu_ms: 3.0,
            drawn: 12,
            culled: 5,
            ..Default::default()
        };
        s.passes[0] = PassTime {
            name: "Schatten",
            ms: 1.25,
        };
        let mut m = Meter::new();
        m.push(0.016, &s);
        let text = m.lines().join("\n");
        assert!(text.contains("Schatten: 1.25 ms"), "{text}");
        assert!(text.contains("12 gezeichnet, 5 aussortiert"), "{text}");
        assert!(!text.contains("fehlen"));
        s.dropped = 2;
        m.push(0.016, &s);
        assert!(m.lines().join("\n").contains("2 Objekte fehlen"));
    }
}
