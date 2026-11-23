//! Zeichenflaeche fuer Menues und HUD in virtuellen Einheiten (Hoehe `scene::UI_HEIGHT`,
//! Breite nach Seitenverhaeltnis, y nach unten): Rechtecke, Rahmen, Balken und Text aus einem
//! [`FontAtlas`]. Ergebnis sind Dreiecke (`render::UiVertex`) fuer `Renderer::draw`.
//!
//! Farben und Layout gehoeren dem Spiel; hier gibt es nur neutrale Grundfarben.

use crate::render::UiVertex;
use crate::text::{FontAtlas, BAKE_PX};

/// RGBA, 0..1
pub type Color = [f32; 4];
pub const WHITE: Color = [0.97, 0.96, 0.92, 1.0];
pub const BLACK: Color = [0.05, 0.04, 0.04, 1.0];
/// Schattenfarbe fuer Text ueber der Szene
pub const SHADOW: Color = [0.0, 0.0, 0.0, 0.6];

/// Farbe mit zusaetzlicher Deckkraft `a` (0..1).
pub fn alpha(c: Color, a: f32) -> Color {
    [c[0], c[1], c[2], c[3] * a]
}

/// Ausrichtung eines Textes zur x-Position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Sammelt die UI-Dreiecke eines Bildes.
pub struct Canvas {
    pub verts: Vec<UiVertex>,
    pub font: FontAtlas,
    /// virtuelle Breite
    pub w: f32,
    /// virtuelle Hoehe
    pub h: f32,
}

impl Canvas {
    pub fn new(font: FontAtlas) -> Self {
        Self {
            verts: Vec::with_capacity(4096),
            font,
            w: 1280.0,
            h: 720.0,
        }
    }

    /// Neues Bild beginnen (leert die Dreiecke, setzt die virtuelle Groesse).
    pub fn begin(&mut self, w: f32, h: f32) {
        self.verts.clear();
        self.w = w;
        self.h = h;
    }

    fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], color: Color) {
        let v = |px: f32, py: f32, u: f32, vv: f32| UiVertex {
            pos: [px, py],
            uv: [u, vv],
            color,
        };
        let (x1, y1) = (x + w, y + h);
        self.verts.extend_from_slice(&[
            v(x, y, uv[0], uv[1]),
            v(x1, y, uv[2], uv[1]),
            v(x1, y1, uv[2], uv[3]),
            v(x, y, uv[0], uv[1]),
            v(x1, y1, uv[2], uv[3]),
            v(x, y1, uv[0], uv[3]),
        ]);
    }

    /// Gefuelltes Rechteck (uv < 0 sagt dem Shader: einfarbig).
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.quad(x, y, w, h, [-1.0; 4], color);
    }

    /// Rahmen der Dicke `t`.
    pub fn frame(&mut self, x: f32, y: f32, w: f32, h: f32, t: f32, color: Color) {
        self.rect(x, y, w, t, color);
        self.rect(x, y + h - t, w, t, color);
        self.rect(x, y, t, h, color);
        self.rect(x + w - t, y, t, h, color);
    }

    /// Fortschrittsbalken, `v` = 0..1.
    pub fn bar(&mut self, x: f32, y: f32, w: f32, h: f32, v: f32, fg: Color, bg: Color) {
        self.rect(x, y, w, h, bg);
        self.rect(x, y, w * v.clamp(0.0, 1.0), h, fg);
    }

    pub fn text_width(&self, s: &str, size: f32) -> f32 {
        self.font.width(s, size)
    }

    /// Text mit Oberkante bei `y`; `size` = Zeichenhoehe in UI-Einheiten.
    pub fn text(&mut self, x: f32, y: f32, s: &str, size: f32, color: Color, align: Align) {
        let k = size / BAKE_PX;
        let w = self.font.width(s, size);
        let mut cx = match align {
            Align::Left => x,
            Align::Center => x - w * 0.5,
            Align::Right => x - w,
        };
        let base = y + self.font.ascent * k;
        for c in s.chars() {
            let g = *self.font.glyph(c);
            if g.size[0] > 0.0 {
                self.quad(
                    cx + g.offset[0] * k,
                    base + g.offset[1] * k,
                    g.size[0] * k,
                    g.size[1] * k,
                    g.uv,
                    color,
                );
            }
            cx += g.advance * k;
        }
    }

    /// Text mit weichem Schatten (lesbar ueber der Szene).
    pub fn text_s(&mut self, x: f32, y: f32, s: &str, size: f32, color: Color, align: Align) {
        let o = (size * 0.06).max(1.5);
        self.text(x + o, y + o, s, size, alpha(SHADOW, color[3]), align);
        self.text(x, y, s, size, color, align);
    }

    /// Bricht Text an Wortgrenzen auf die Breite `max_w` um.
    pub fn wrap(&self, s: &str, size: f32, max_w: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut cur = String::new();
        for word in s.split_whitespace() {
            let test = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if !cur.is_empty() && self.text_width(&test, size) > max_w {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            } else {
                cur = test;
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        lines
    }
}
