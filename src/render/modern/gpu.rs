//! Daten fuer die Shader der modernen Grafik, gleich aufgebaut wie in `_m_common.wgsl`.

use super::cascades::{ends, Cascade, CASCADES};
use super::clusters::{slice_params, GRID};
use super::types::*;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3, Vec4};

/// Texturen beim Hochskalieren schaerfer waehlen (Stufen je Halbierung der Aufloesung)
const MIP_BIAS: f32 = 1.3;
/// Streuung der Sonnenstrahlen (weiche Schattenraender)
const SUN_RADIUS: f32 = 0.012;
/// Felder im Schatten-Atlas der Lampen
pub const TILES: usize = 64;

/// Ein Objekt (binding 0).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GpuObj {
    pub model: [[f32; 4]; 4],
    pub prev: [[f32; 4]; 4],
    pub tint: [f32; 4],
    /// erster Knochen, Material, erster alter Knochen + 1 (0 = keiner), Eigenleuchten
    pub extra: [f32; 4],
}

/// Ein Feld im Schatten-Atlas.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GpuTile {
    pub view_proj: [[f32; 4]; 4],
    /// Ursprung und Groesse (0..1)
    pub rect: [f32; 4],
}

/// Szene (binding 2).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GpuScene {
    pub view_proj: [[f32; 4]; 4],
    pub plain_view_proj: [[f32; 4]; 4],
    pub prev_view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub cascade_vp: [[[f32; 4]; 4]; CASCADES],
    pub cam_pos: [f32; 4],
    pub cam_dir: [f32; 4],
    pub sun_dir: [f32; 4],
    pub sun_color: [f32; 4],
    pub cascade_ends: [f32; 4],
    pub cascade_texels: [f32; 4],
    pub sky_a: [f32; 4],
    pub sky_b: [f32; 4],
    pub sky_c: [f32; 4],
    pub fog: [f32; 4],
    pub misc: [f32; 4],
    pub cluster: [f32; 4],
    pub screen: [f32; 4],
    /// Strahlen: Sonne, Streulicht, Spiegelung, Bildnummer
    pub rt: [f32; 4],
    /// Lampen, Sonnenradius, Reichweite und Staerke der Verdeckung
    pub counts: [f32; 4],
    pub tiles: [GpuTile; TILES],
}

/// Eine Lampe (binding 3).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GpuLamp {
    pos: [f32; 4],
    color: [f32; 4],
    dir: [f32; 4],
    extra: [f32; 4],
}

impl GpuLamp {
    /// `tile` = erstes Feld im Schatten-Atlas, None = ohne Schatten.
    pub fn new(l: &Light, tile: Option<usize>) -> Self {
        let (dir, outer, inner, cone) = match l.kind {
            LightKind::Point => (Vec3::NEG_Y, -1.0, -1.0, 0.0),
            LightKind::Spot { dir, inner, outer } => {
                let outer = outer.clamp(0.01, 1.5);
                let inner = inner.clamp(0.0, outer * 0.99);
                (dir.normalize_or(Vec3::NEG_Y), outer.cos(), inner.cos(), 1.0)
            }
        };
        Self {
            pos: l.pos.extend(l.range.max(0.01)).to_array(),
            color: l.color.extend(cone).to_array(),
            dir: dir.extend(outer).to_array(),
            extra: [inner, tile.map_or(-1.0, |t| t as f32), 0.0, 0.0],
        }
    }
}

/// Versatz der Kamera fuer TAA im Bild `n` (Bildpunkte, -0,5..0,5); wiederholt sich nach `phases`.
pub fn jitter(n: u64, phases: u64) -> Vec2 {
    let halton = |mut i: u64, base: u64| {
        let (mut f, mut r) = (1.0f32, 0.0f32);
        while i > 0 {
            f /= base as f32;
            r += f * (i % base) as f32;
            i /= base;
        }
        r
    };
    let i = n % phases.max(1) + 1;
    Vec2::new(halton(i, 2) - 0.5, halton(i, 3) - 0.5)
}

/// Was die Szene ausser der Eingabe braucht.
pub struct SceneExtras<'a> {
    pub extent: vk::Extent2D,
    pub prev_view_proj: Mat4,
    /// Versatz in Bildpunkten
    pub jitter: Vec2,
    pub cascades: Option<&'a [Cascade; CASCADES]>,
    pub tiles: &'a [GpuTile],
    /// Kantenlaenge des Atlas (Texel)
    pub atlas: u32,
    pub sky_mips: u32,
    pub time: f32,
    /// Bildnummer
    pub number: u64,
    /// Lampen
    pub lamps: usize,
}

/// Kamera mit TAA-Versatz.
pub fn jittered(cam: &Camera, jitter: Vec2, extent: vk::Extent2D) -> Mat4 {
    let shift = Vec3::new(
        jitter.x * 2.0 / extent.width as f32,
        jitter.y * 2.0 / extent.height as f32,
        0.0,
    );
    Mat4::from_translation(shift) * cam.view_proj()
}

impl GpuScene {
    pub fn new(s: &SceneInput, x: &SceneExtras) -> Self {
        let cam = &s.camera;
        let view_proj = jittered(cam, x.jitter, x.extent);
        let (w, h) = (x.extent.width as f32, x.extent.height as f32);
        let (scale, offset) = slice_params(cam);
        let mut g = Self {
            view_proj: view_proj.to_cols_array_2d(),
            plain_view_proj: cam.view_proj().to_cols_array_2d(),
            prev_view_proj: x.prev_view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            cam_pos: cam.pos.extend(x.time).to_array(),
            cam_dir: (-cam.view.inverse().z_axis.truncate())
                .extend((s.features.render_scale.clamp(0.33, 1.0)).log2() * MIP_BIAS)
                .to_array(),
            cluster: [scale, offset, GRID[0] as f32 / w, GRID[1] as f32 / h],
            screen: [w, h, 1.0 / w, 1.0 / h],
            ..Self::zeroed()
        };
        if let Some(sun) = &s.sun {
            g.sun_dir = sun.dir.normalize_or(Vec3::Y).extend(1.0).to_array();
            let shadow = x.cascades.is_some() as u8 as f32;
            g.sun_color = sun.color.extend(shadow).to_array();
        }
        if let Some(c) = x.cascades {
            for (m, c) in g.cascade_vp.iter_mut().zip(c) {
                *m = c.view_proj.to_cols_array_2d();
            }
            g.cascade_ends = ends(c).to_array();
            g.cascade_texels = c.map(|c| c.texel);
        }
        let [a, b, c] = match s.sky {
            Sky::Gradient {
                zenith,
                horizon,
                ground,
            } => [zenith.extend(0.0), horizon.extend(0.0), ground.extend(0.0)],
            Sky::Image { strength, .. } => {
                [Vec3::splat(strength).extend(1.0), Vec4::ZERO, Vec4::ZERO]
            }
        };
        g.sky_a = a.to_array();
        g.sky_b = b.truncate().extend(s.ambient).to_array();
        g.sky_c = c.truncate().extend(x.sky_mips as f32).to_array();
        if let Some(f) = &s.features.fog {
            g.fog = f.color.extend(f.density).to_array();
            g.misc[0] = f.falloff;
        }
        let ray = &s.features.ray_tracing;
        let on = |b: bool| b as u8 as f32;
        g.rt = [
            on(ray.sun_shadows),
            on(ray.indirect),
            on(ray.reflections),
            (x.number % 4096) as f32,
        ];
        let ssao = s.features.ssao.unwrap_or_default();
        g.counts = [x.lamps as f32, SUN_RADIUS, ssao.radius, ssao.strength];
        g.misc[1] = on(s.features.ssao.is_some() || ray.occlusion);
        g.misc[2] = 1.0 / s.features.sun_shadow_size.max(1) as f32;
        g.misc[3] = 1.0 / x.atlas.max(1) as f32;
        for (t, new) in g.tiles.iter_mut().zip(x.tiles) {
            *t = *new;
        }
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_stays_inside_a_pixel_and_averages_out() {
        let sum: Vec2 = (0..32).map(|n| jitter(n, 32)).sum();
        assert!((0..40).all(|n| jitter(n, 32).abs().max_element() <= 0.5));
        assert!((sum / 32.0).length() < 0.1);
    }

    #[test]
    fn uniform_fits_the_guaranteed_size() {
        assert!(std::mem::size_of::<GpuScene>() <= 16384);
        assert_eq!(std::mem::size_of::<GpuScene>() % 16, 0);
        assert_eq!(std::mem::size_of::<GpuObj>(), 160);
    }
}
