//! Bildschirmfotos aus HDR-Formaten: zurueck nach SDR.

use super::{Encoding, FORMAT};
use ash::vk;

/// Bytes je Bildpunkt eines Swapchain-Formats.
pub fn pixel_bytes(format: vk::Format) -> u64 {
    if format == FORMAT {
        8
    } else {
        4
    }
}

/// Swapchain-Pixel fuer ein Bildschirmfoto nach BGRA mit SDR-Werten; Helleres wird Weiss.
pub fn to_bgra(px: &[u8], format: vk::Format, space: vk::ColorSpaceKHR, paper: f32) -> Vec<u8> {
    let size = pixel_bytes(format) as usize;
    px.chunks_exact(size)
        .flat_map(|p| {
            let rgb = match Encoding::of(space) {
                Encoding::ScRgb => [0, 2, 4].map(|k| {
                    let v = half(u16::from_le_bytes([p[k], p[k + 1]]));
                    encode_srgb(v * 80.0 / paper)
                }),
                _ => hdr10_to_srgb(u32::from_le_bytes([p[0], p[1], p[2], p[3]]), format, paper),
            };
            let b = rgb.map(|v| (v * 255.0 + 0.5) as u8);
            [b[2], b[1], b[0], 255]
        })
        .collect()
}

fn hdr10_to_srgb(v: u32, format: vk::Format, paper: f32) -> [f32; 3] {
    let part = |shift: u32| pq_to_nits((v >> shift & 1023) as f32 / 1023.0) / paper;
    let (lo, mid, hi) = (part(0), part(10), part(20));
    let [r, g, b] = if format == vk::Format::A2B10G10R10_UNORM_PACK32 {
        [lo, mid, hi]
    } else {
        [hi, mid, lo]
    };
    // Rec. 2020 nach Rec. 709
    [
        1.660491 * r - 0.587641 * g - 0.072850 * b,
        -0.124550 * r + 1.132_9 * g - 0.008349 * b,
        -0.018151 * r - 0.100579 * g + 1.118_73 * b,
    ]
    .map(encode_srgb)
}

fn pq_to_nits(v: f32) -> f32 {
    let p = v.powf(1.0 / 78.84375);
    let y = (p - 0.8359375).max(0.0) / (18.851_563 - 18.6875 * p);
    10_000.0 * y.powf(1.0 / 0.159_301_76)
}

fn encode_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// 16-Bit-Gleitkommazahl.
fn half(v: u16) -> f32 {
    let sign = if v & 0x8000 != 0 { -1.0 } else { 1.0 };
    let (exp, frac) = ((v >> 10 & 31) as i32, (v & 1023) as f32);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_white_becomes_white_in_photos() {
        let paper = 200.0f32;
        // 200 Nits in PQ (10 Bit)
        let y = (paper / 10_000.0).powf(0.159_301_76);
        let pq = ((0.8359375 + 18.851_563 * y) / (1.0 + 18.6875 * y)).powf(78.84375);
        assert!((pq_to_nits(pq) - paper).abs() < 1.0);
        let v = (pq * 1023.0).round() as u32;
        let word = v | v << 10 | v << 20;
        let space = vk::ColorSpaceKHR::HDR10_ST2084_EXT;
        let format = vk::Format::A2B10G10R10_UNORM_PACK32;
        let px = to_bgra(&word.to_le_bytes(), format, space, paper);
        assert!(px[..3].iter().all(|&b| b >= 253), "{px:?}");
        // scRGB: 2,5 = 200 Nits
        let sc = [0x00, 0x41, 0x00, 0x41, 0x00, 0x41, 0x00, 0x3c];
        let space = vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT;
        assert_eq!(to_bgra(&sc, FORMAT, space, paper), [255; 4]);
        assert_eq!(half(0x3c00), 1.0);
    }
}
