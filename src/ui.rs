//! Bitmap-Text und Rechtecke im 480x270-Raster.

use crate::render::UiVertex;
use font8x8::UnicodeFonts;

pub const W: f32 = 480.0;
pub const H: f32 = 270.0;
const EURO_SLOT: u32 = 128;

pub type Color = [f32; 4];
pub const WHITE: Color = [0.95, 0.95, 0.92, 1.0];
pub const DIM: Color = [0.6, 0.6, 0.58, 1.0];
pub const YELLOW: Color = [0.98, 0.88, 0.3, 1.0];
pub const RED: Color = [0.95, 0.25, 0.2, 1.0];
pub const GREEN: Color = [0.4, 0.95, 0.5, 1.0];
pub const SHADOW: Color = [0.0, 0.0, 0.0, 0.75];

/// 128x128-Atlas: 16x16 Zellen, Zeichen = Latin-1-Code, Slot 128 = Euro.
pub fn font_atlas() -> Vec<u8> {
    let mut px = vec![0u8; 128 * 128];
    let mut put = |slot: u32, rows: [u8; 8]| {
        let (cx, cy) = (slot % 16 * 8, slot / 16 * 8);
        for (y, row) in rows.iter().enumerate() {
            for x in 0..8 {
                if row & (1 << x) != 0 {
                    px[(cy as usize + y) * 128 + cx as usize + x] = 255;
                }
            }
        }
    };
    for code in 32u32..256 {
        if let Some(c) = char::from_u32(code) {
            let glyph = font8x8::BASIC_FONTS
                .get(c)
                .or_else(|| font8x8::LATIN_FONTS.get(c));
            if let Some(g) = glyph {
                put(code, g);
            }
        }
    }
    // Euro-Zeichen (Bit 0 = linkes Pixel)
    put(EURO_SLOT, [0x38, 0x44, 0x0F, 0x04, 0x0F, 0x44, 0x38, 0x00]);
    // Play-Dreieck fuer das VHS-OSD auf Slot 129
    put(129, [0x03, 0x0F, 0x3F, 0xFF, 0x3F, 0x0F, 0x03, 0x00]);
    px
}

fn slot(c: char) -> u32 {
    match c {
        '€' => EURO_SLOT,
        '▶' => 129,
        c if (c as u32) < 256 => c as u32,
        _ => '?' as u32,
    }
}

#[derive(Default)]
pub struct Ui {
    pub verts: Vec<UiVertex>,
}

impl Ui {
    pub fn clear(&mut self) {
        self.verts.clear();
    }

    fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, uv0: [f32; 2], uv1: [f32; 2], color: Color) {
        let v = |px: f32, py: f32, u: f32, vv: f32| UiVertex {
            pos: [px, py],
            uv: [u, vv],
            color,
        };
        let (x1, y1) = (x + w, y + h);
        self.verts.extend_from_slice(&[
            v(x, y, uv0[0], uv0[1]),
            v(x1, y, uv1[0], uv0[1]),
            v(x1, y1, uv1[0], uv1[1]),
            v(x, y, uv0[0], uv0[1]),
            v(x1, y1, uv1[0], uv1[1]),
            v(x, y1, uv0[0], uv1[1]),
        ]);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.quad(x, y, w, h, [-1.0, -1.0], [-1.0, -1.0], color);
    }

    pub fn text_width(s: &str, scale: f32) -> f32 {
        s.chars().count() as f32 * 8.0 * scale
    }

    pub fn text(&mut self, x: f32, y: f32, s: &str, color: Color, scale: f32) {
        let mut cx = x.round();
        let y = y.round();
        let size = 8.0 * scale;
        for c in s.chars() {
            if c != ' ' {
                let sl = slot(c);
                let u = (sl % 16) as f32 * 8.0 / 128.0;
                let v = (sl / 16) as f32 * 8.0 / 128.0;
                self.quad(
                    cx,
                    y,
                    size,
                    size,
                    [u, v],
                    [u + 8.0 / 128.0, v + 8.0 / 128.0],
                    color,
                );
            }
            cx += size;
        }
    }

    /// Text mit 1-Pixel-Schatten (gut lesbar auf hellem Grund).
    pub fn text_s(&mut self, x: f32, y: f32, s: &str, color: Color, scale: f32) {
        self.text(x + scale, y + scale, s, SHADOW, scale);
        self.text(x, y, s, color, scale);
    }

    pub fn text_center(&mut self, y: f32, s: &str, color: Color, scale: f32) {
        let x = (W - Self::text_width(s, scale)) * 0.5;
        self.text_s(x, y, s, color, scale);
    }

    /// Bricht Text an Wortgrenzen auf `max_chars` um.
    pub fn wrap(s: &str, max_chars: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut cur = String::new();
        for word in s.split_whitespace() {
            if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > max_chars {
                lines.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        lines
    }
}

pub fn euro(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let c = cents.abs();
    format!("{sign}{},{:02} €", c / 100, c % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_has_letters_umlauts_and_euro() {
        let a = font_atlas();
        let filled = |slot: u32| {
            let (cx, cy) = ((slot % 16 * 8) as usize, (slot / 16 * 8) as usize);
            (0..8).any(|y| (0..8).any(|x| a[(cy + y) * 128 + cx + x] > 0))
        };
        for c in ['A', 'z', '0', 'ä', 'Ö', 'ü', 'ß', '€', '▶'] {
            assert!(filled(slot(c)), "Glyph fehlt: {c}");
        }
        assert!(!filled(' ' as u32));
    }

    #[test]
    fn euro_formatting() {
        assert_eq!(euro(0), "0,00 €");
        assert_eq!(euro(1999), "19,99 €");
        assert_eq!(euro(-5), "-0,05 €");
    }

    #[test]
    fn wrapping() {
        assert_eq!(
            Ui::wrap("Kunde Nummer 0 bitte zur Kasse 0", 14),
            vec!["Kunde Nummer 0", "bitte zur", "Kasse 0"]
        );
    }
}
