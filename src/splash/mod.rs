//! Startbild "Gemacht mit Mettgine". Beide Renderer zeigen es von selbst, sobald sie das erste
//! Mal zeichnen: immer, nicht ueberspringbar, ohne Nachbearbeitung des Spiels und direkt in
//! Fensteraufloesung. Spiele fragen mit `splash_active()`, ob es noch laeuft (keine Eingaben
//! annehmen).
//!
//! Hier liegt der Ablauf (reine Rechnung), in `gpu` das Zeichnen.

mod gpu;

pub(crate) use gpu::Splash;

use crate::render::UiVertex;

/// Laenge (s)
const DURATION: f32 = 2.6;
/// Laengster gewerteter Schritt (s): ein Haenger beim Laden ueberspringt nichts
const MAX_STEP: f32 = 0.05;
/// Gedachte Bildflaeche; wird mittig eingepasst
const W: f32 = 1280.0;
const H: f32 = 720.0;
/// Hintergrund (Anzeigewerte)
const BACK: [f32; 3] = [18.0 / 255.0, 14.0 / 255.0, 11.0 / 255.0];
/// Groesse von `assets/splash.png`
const ATLAS: (f32, f32) = (2048.0, 1500.0);
/// So viele Vertices entstehen hoechstens
const MAX_VERTS: usize = 8 * 6;

/// Rechteck: links, oben, rechts, unten.
type Rect = [f32; 4];

/// Ein Teil des Bildes: Stelle im Atlas und auf der Bildflaeche.
struct Part {
    atlas: Rect,
    screen: Rect,
}

const DINO: Part = Part {
    atlas: [0.0, 0.0, 1024.0, 1024.0],
    screen: [322.0, 50.0, 762.0, 490.0],
};
/// Das Broetchen steht im Atlas (wie die Schrift) in dreifacher Groesse.
const BUN: Part = Part {
    atlas: [1200.0, 80.0, 1960.0, 490.0],
    screen: [696.0, 178.0, 949.33, 314.67],
};
const BITTEN: Part = Part {
    atlas: [1200.0, 592.0, 1960.0, 1002.0],
    screen: BUN.screen,
};
const NAME: Part = Part {
    atlas: [30.0, 1050.0, 1370.0, 1240.0],
    screen: [416.67, 575.33, 863.33, 638.67],
};
const MADE_WITH: Part = Part {
    atlas: [330.0, 1270.0, 1050.0, 1345.0],
    screen: [516.67, 536.0, 756.67, 561.0],
};
const DOT: Rect = [1550.0, 1190.0, 1650.0, 1290.0];

/// Zeitpunkte (s)
const BUN_IN: f32 = 0.05;
const LUNGE: f32 = 0.45;
const BITE: f32 = 0.8;
const SHAKE: f32 = 0.82;
const MADE_IN: f32 = 1.0;
const NAME_IN: f32 = 1.1;
const FADE_OUT: f32 = 2.13;

/// Anteil 0..1 der Zeit zwischen `start` und `start + len`.
fn part(t: f32, start: f32, len: f32) -> f32 {
    ((t - start) / len).clamp(0.0, 1.0)
}

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

/// Seitlicher Versatz des Dinos: springt herein, schwingt ueber, setzt sich.
fn lunge_x(t: f32) -> f32 {
    let x = part(t, LUNGE, 0.4);
    if x < 0.7 {
        -900.0 + 922.0 * ease_out(x / 0.7)
    } else {
        22.0 * (1.0 - ease_out((x - 0.7) / 0.3))
    }
}

/// Kurzes Wackeln nach dem Biss.
fn shake(t: f32) -> (f32, f32) {
    let x = part(t, SHAKE, 0.22);
    let keys = [
        (0.0, 0.0, 0.0),
        (0.25, -7.0, 4.0),
        (0.6, 5.0, -3.0),
        (1.0, 0.0, 0.0),
    ];
    for k in keys.windows(2) {
        let ((t0, x0, y0), (t1, x1, y1)) = (k[0], k[1]);
        if x <= t1 {
            let f = (x - t0) / (t1 - t0);
            return (x0 + (x1 - x0) * f, y0 + (y1 - y0) * f);
        }
    }
    (0.0, 0.0)
}

/// Helligkeit des ganzen Bildes (blendet am Ende nach Schwarz).
fn brightness(t: f32) -> f32 {
    1.0 - part(t, FADE_OUT, DURATION - FADE_OUT)
}

/// Sammelt Rechtecke und rechnet sie in Bildkoordinaten (-1..1) um.
struct Sheet<'a> {
    out: &'a mut Vec<UiVertex>,
    /// Umrechnung der gedachten Flaeche auf das Fenster
    scale: (f32, f32),
    light: f32,
}

impl Sheet<'_> {
    /// `rgb` faerbt, `alpha` blendet (Farben sind mit Alpha vormultipliziert).
    fn quad(&mut self, atlas: Rect, r: Rect, rgb: [f32; 3], alpha: f32) {
        if alpha <= 0.0 {
            return;
        }
        let k = alpha * self.light;
        let color = [rgb[0] * k, rgb[1] * k, rgb[2] * k, alpha];
        let v = |x: usize, y: usize| UiVertex {
            pos: [
                (r[x] / W * 2.0 - 1.0) * self.scale.0,
                (1.0 - r[y] / H * 2.0) * self.scale.1,
            ],
            uv: [atlas[x] / ATLAS.0, atlas[y] / ATLAS.1],
            color,
        };
        let c = [v(0, 1), v(2, 1), v(2, 3), v(0, 1), v(2, 3), v(0, 3)];
        self.out.extend_from_slice(&c);
    }

    fn part(&mut self, p: &Part, dx: f32, dy: f32, alpha: f32) {
        let s = p.screen;
        let r = [s[0] + dx, s[1] + dy, s[2] + dx, s[3] + dy];
        self.quad(p.atlas, r, [1.0; 3], alpha);
    }

    /// Von unten eingeblendet.
    fn rise(&mut self, p: &Part, t: f32, start: f32, len: f32) {
        let x = ease_out(part(t, start, len));
        self.part(p, 0.0, 14.0 * (1.0 - x), x);
    }
}

/// Alle Rechtecke zum Zeitpunkt `t` fuer ein Fenster mit `aspect` = Breite / Hoehe.
fn build(t: f32, aspect: f32, out: &mut Vec<UiVertex>) {
    out.clear();
    let fit = aspect / (W / H);
    let mut s = Sheet {
        out,
        scale: (1.0 / fit.max(1.0), fit.min(1.0)),
        light: brightness(t),
    };
    let (sx, sy) = shake(t);
    s.part(&DINO, lunge_x(t) + sx, sy, 1.0);
    if t < BITE {
        s.rise(&BUN, t, BUN_IN, 0.2);
    } else {
        s.part(&BITTEN, sx, sy, 1.0);
    }
    let x = part(t, BITE, 0.6);
    if x > 0.0 && x < 1.0 {
        let e = ease_out(x);
        let mett = [0.824, 0.380, 0.353];
        let bread = [0.898, 0.745, 0.510];
        for (dx, dy, rgb) in [
            (-60.0, 90.0, mett),
            (50.0, 110.0, bread),
            (10.0, -80.0, mett),
        ] {
            let (cx, cy, r) = (735.0 + dx * e, 245.0 + dy * e, 7.3);
            s.quad(DOT, [cx - r, cy - r, cx + r, cy + r], rgb, 1.0 - e);
        }
    }
    s.rise(&MADE_WITH, t, MADE_IN, 0.3);
    s.rise(&NAME, t, NAME_IN, 0.3);
}

/// Laufzeit des Startbilds; zaehlt nur gezeichnete Bilder.
#[derive(Debug, Default)]
struct Timeline {
    t: f32,
    last: Option<std::time::Instant>,
}

impl Timeline {
    fn active(&self) -> bool {
        self.t < DURATION
    }

    fn advance(&mut self, dt: f32) {
        self.t += dt.clamp(0.0, MAX_STEP);
    }

    fn tick(&mut self) {
        let now = std::time::Instant::now();
        if let Some(l) = self.last.replace(now) {
            self.advance((now - l).as_secs_f32());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_moment_fits_and_stays_inside_the_atlas() {
        let mut v = Vec::new();
        for i in 0..=260 {
            build(i as f32 * 0.01, 16.0 / 9.0, &mut v);
            assert!(v.len() <= MAX_VERTS && v.len() % 6 == 0);
            for p in &v {
                assert!((0.0..=1.0).contains(&p.uv[0]) && (0.0..=1.0).contains(&p.uv[1]));
                assert!(p.pos.iter().chain(&p.color).all(|x| x.is_finite()));
                assert!((0.0..=1.0).contains(&p.color[3]));
            }
        }
    }

    #[test]
    fn any_window_shape_shows_the_whole_picture() {
        let mut v = Vec::new();
        for aspect in [0.5, 1.0, 16.0 / 9.0, 3.6] {
            build(1.6, aspect, &mut v);
            let max = |k: usize| v.iter().map(|p| p.pos[k].abs()).fold(0.0f32, f32::max);
            assert!(max(0) < 1.0 && max(1) < 1.0, "{aspect}");
            // Dino bleibt quadratisch
            let (w, h) = (v[1].pos[0] - v[0].pos[0], v[0].pos[1] - v[2].pos[1]);
            assert!((w * aspect / h - 1.0).abs() < 1e-3, "{aspect} verzerrt");
        }
    }

    #[test]
    fn runs_its_full_length_even_after_a_hang() {
        let mut l = Timeline::default();
        assert!(l.active());
        l.advance(60.0);
        assert!(l.active(), "ein Haenger ueberspringt es nicht");
        let mut n = 0;
        while l.active() {
            l.advance(1.0 / 60.0);
            n += 1;
        }
        assert!((150..160).contains(&n));
        assert_eq!((brightness(1.0), brightness(DURATION)), (1.0, 0.0));
    }
}
