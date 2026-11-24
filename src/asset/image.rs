//! Bilder (PNG, JPEG) als Texturen laden. Feature `images`.

use super::Asset;
use crate::render::TextureData;

/// Aus PNG oder JPEG. `srgb` = Farbbild (Vorgabe), sonst Daten (Normalen, Rauheit).
pub fn decode(bytes: &[u8], srgb: bool) -> Result<TextureData, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let img = img.into_rgba8();
    Ok(TextureData {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
        srgb,
        mips: true,
    })
}

/// Dateinamen auf `_n`, `_normal`, `_mr`, `_rough`, `_ao` (vor der Endung) sind Daten, keine Farben.
fn is_data(name: &str) -> bool {
    let stem = name.rsplit_once('.').map_or(name, |s| s.0).to_lowercase();
    ["_n", "_normal", "_mr", "_rough", "_metal", "_ao"]
        .iter()
        .any(|s| stem.ends_with(s))
}

impl Asset for TextureData {
    fn parse(bytes: Vec<u8>, name: &str) -> Result<Self, String> {
        decode(&bytes, !is_data(name)).map_err(|e| format!("{name}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_is_read_and_normal_maps_stay_linear() {
        let mut png = Vec::new();
        {
            let mut e = png::Encoder::new(&mut png, 2, 1);
            e.set_color(png::ColorType::Rgb);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 0, 0, 255]).unwrap();
        }
        let t = TextureData::parse(png.clone(), "wand.png").unwrap();
        assert_eq!((t.width, t.height, t.srgb), (2, 1, true));
        assert_eq!(t.rgba, [255, 0, 0, 255, 0, 0, 255, 255]);
        assert!(!TextureData::parse(png, "wand_N.png").unwrap().srgb);
        assert!(TextureData::parse(vec![1, 2, 3], "x.png").is_err());
    }
}
