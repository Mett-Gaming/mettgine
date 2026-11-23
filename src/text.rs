//! Schrift-Atlas aus einer TTF/OTF-Datei: alle Latin-1-Zeichen werden einmal beim Start in eine
//! Graustufen-Textur gebacken. `canvas` zeichnet daraus Text in beliebiger Groesse.
//!
//! Die Schriftdatei gehoert dem Spiel (Lizenz dort); die Engine bekommt nur die Bytes.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

/// Pixelgroesse, in der die Zeichen gebacken werden (Text bis etwa doppelt so gross bleibt scharf)
pub const BAKE_PX: f32 = 56.0;
/// Breite des Atlas in Pixeln (Hoehe ergibt sich, Zweierpotenz)
const ATLAS_W: u32 = 1024;
/// Abstand zwischen den Zeichen im Atlas (gegen Ueberlaufen beim Filtern)
const PAD: u32 = 2;

/// Lage und Masse eines Zeichens.
#[derive(Clone, Copy, Debug, Default)]
pub struct Glyph {
    /// Texturkoordinaten (u0, v0, u1, v1)
    pub uv: [f32; 4],
    /// Versatz der Bitmap zur Grundlinie (x, y), in BAKE_PX-Pixeln
    pub offset: [f32; 2],
    /// Groesse der Bitmap, in BAKE_PX-Pixeln
    pub size: [f32; 2],
    /// Vorschub zum naechsten Zeichen, in BAKE_PX-Pixeln
    pub advance: f32,
}

/// Gebackene Schrift: Pixel (R8) fuer die GPU und Zeichenmasse.
pub struct FontAtlas {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    glyphs: Vec<Glyph>,
    /// Oberlaenge in BAKE_PX-Pixeln (Abstand Oberkante -> Grundlinie)
    pub ascent: f32,
}

impl FontAtlas {
    /// Backt die Schrift aus den Bytes einer TTF/OTF-Datei.
    pub fn bake(ttf: &[u8]) -> Result<FontAtlas, String> {
        let font = FontRef::try_from_slice(ttf).map_err(|e| format!("Schrift defekt: {e}"))?;
        let scale = PxScale::from(BAKE_PX);
        let scaled = font.as_scaled(scale);
        let mut glyphs = vec![Glyph::default(); 256];
        // Zeilenweise packen
        let (mut x, mut y, mut row_h) = (PAD, PAD, 0u32);
        let mut placed = Vec::new();
        for code in 32u32..256 {
            let Some(c) = char::from_u32(code) else {
                continue;
            };
            let id = scaled.glyph_id(c);
            glyphs[code as usize].advance = scaled.h_advance(id);
            let Some(outline) = font.outline_glyph(id.with_scale(scale)) else {
                continue;
            };
            let b = outline.px_bounds();
            let (w, h) = (b.width().ceil() as u32, b.height().ceil() as u32);
            if x + w + PAD > ATLAS_W {
                x = PAD;
                y += row_h + PAD;
                row_h = 0;
            }
            placed.push((code as usize, x, y, outline));
            x += w + PAD;
            row_h = row_h.max(h);
        }
        let height = (y + row_h + PAD).next_power_of_two();
        let mut pixels = vec![0u8; (ATLAS_W * height) as usize];
        for (code, gx, gy, outline) in placed {
            let b = outline.px_bounds();
            outline.draw(|px, py, cov| {
                let (tx, ty) = (gx + px, gy + py);
                if tx < ATLAS_W && ty < height {
                    let i = (ty * ATLAS_W + tx) as usize;
                    pixels[i] = pixels[i].max((cov.clamp(0.0, 1.0) * 255.0) as u8);
                }
            });
            let (w, h) = (b.width().ceil(), b.height().ceil());
            glyphs[code] = Glyph {
                uv: [
                    gx as f32 / ATLAS_W as f32,
                    gy as f32 / height as f32,
                    (gx as f32 + w) / ATLAS_W as f32,
                    (gy as f32 + h) / height as f32,
                ],
                offset: [b.min.x, b.min.y],
                size: [w, h],
                advance: glyphs[code].advance,
            };
        }
        Ok(FontAtlas {
            pixels,
            width: ATLAS_W,
            height,
            glyphs,
            ascent: scaled.ascent(),
        })
    }

    /// Zeichen-Masse; Zeichen ausserhalb von Latin-1 werden zu `?`.
    pub fn glyph(&self, c: char) -> &Glyph {
        let code = c as u32;
        if code < 256 {
            &self.glyphs[code as usize]
        } else {
            &self.glyphs['?' as usize]
        }
    }

    /// Breite eines Textes bei Zeichenhoehe `size` (in denselben Einheiten wie `size`).
    pub fn width(&self, s: &str, size: f32) -> f32 {
        let k = size / BAKE_PX;
        s.chars().map(|c| self.glyph(c).advance * k).sum()
    }
}
