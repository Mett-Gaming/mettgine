//! Zielbilder der modernen Grafik. Angelegt wird nur, was die eingeschalteten Zusaetze brauchen.

use super::super::context::Context;
use super::super::transfer::one_shot;
use super::super::util::image_barrier;
use super::super::Result;
use super::atlas::ATLAS_SIZE;
use super::cascades::CASCADES;
use super::memory::{Arena, GpuImage, TexDesc};
use super::types::{AntiAlias, Features};
use ash::vk;

pub const HDR: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
pub const NORMAL: vk::Format = vk::Format::A2B10G10R10_UNORM_PACK32;
pub const MOTION: vk::Format = vk::Format::R16G16_SFLOAT;
pub const AO: vk::Format = vk::Format::R8_UNORM;
pub const EXPOSURE: vk::Format = vk::Format::R16_SFLOAT;
pub const LDR: vk::Format = vk::Format::R8G8B8A8_UNORM;
/// Stufen des Ueberstrahlens
const BLOOM_LEVELS: u32 = 6;

/// Was gebraucht wird; aendert es sich, entstehen die Bilder neu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wants {
    pub width: u32,
    pub height: u32,
    /// 0 = keine Sonnenschatten
    pub shadow: u32,
    pub lamps: bool,
    pub ssao: bool,
    pub bloom: bool,
    pub taa: bool,
    pub fxaa: bool,
}

impl Wants {
    pub fn new(extent: vk::Extent2D, f: &Features) -> Self {
        Self {
            width: extent.width,
            height: extent.height,
            shadow: f.sun_shadow_size.min(8192),
            lamps: f.lamp_shadows,
            ssao: f.ssao.is_some(),
            bloom: f.bloom.is_some(),
            taa: f.anti_alias == AntiAlias::Taa,
            fxaa: f.anti_alias == AntiAlias::Fxaa,
        }
    }
}

/// Alle Zielbilder.
pub struct Targets {
    pub wants: Wants,
    pub hdr: GpuImage,
    pub depth: GpuImage,
    pub normal: GpuImage,
    pub motion: GpuImage,
    /// Sonnenschatten, je Stufe eine Ebene
    pub sun: Option<GpuImage>,
    pub lamps: Option<GpuImage>,
    pub ao: Option<[GpuImage; 2]>,
    pub history: Option<[GpuImage; 2]>,
    pub bloom: Vec<(GpuImage, vk::Extent2D)>,
    pub ldr: Option<GpuImage>,
}

impl Targets {
    pub fn new(ctx: &Context, arena: &mut Arena, w: Wants) -> Result<Self> {
        let mut color = |format| arena.image(ctx, &TexDesc::target(w.width, w.height, format));
        let (hdr, normal, motion) = (color(HDR)?, color(NORMAL)?, color(MOTION)?);
        let ao = if w.ssao {
            Some([color(AO)?, color(AO)?])
        } else {
            None
        };
        let history = if w.taa {
            Some([color(HDR)?, color(HDR)?])
        } else {
            None
        };
        let ldr = if w.fxaa { Some(color(LDR)?) } else { None };
        let mut bloom = Vec::new();
        let mut size = (w.width / 2, w.height / 2);
        while w.bloom && bloom.len() < BLOOM_LEVELS as usize && size.0.min(size.1) >= 4 {
            let extent = vk::Extent2D {
                width: size.0,
                height: size.1,
            };
            bloom.push((
                arena.image(ctx, &TexDesc::target(size.0, size.1, HDR))?,
                extent,
            ));
            size = (size.0 / 2, size.1 / 2);
        }
        let sun = if w.shadow > 0 {
            let desc = TexDesc {
                layers: CASCADES as u32,
                array: true,
                ..TexDesc::depth(w.shadow, w.shadow)
            };
            Some(arena.image(ctx, &desc)?)
        } else {
            None
        };
        let lamps = if w.lamps {
            Some(arena.image(ctx, &TexDesc::depth(ATLAS_SIZE, ATLAS_SIZE))?)
        } else {
            None
        };
        Ok(Self {
            depth: arena.image(ctx, &TexDesc::depth(w.width, w.height))?,
            wants: w,
            hdr,
            normal,
            motion,
            sun,
            lamps,
            ao,
            history,
            bloom,
            ldr,
        })
    }

    /// # Safety
    /// Die GPU darf die Bilder nicht mehr benutzen.
    pub unsafe fn destroy(&self, arena: &mut Arena, d: &ash::Device) {
        let always = [&self.hdr, &self.depth, &self.normal, &self.motion];
        let pairs = self.ao.iter().chain(&self.history).flatten();
        let single = self.sun.iter().chain(&self.lamps).chain(&self.ldr);
        let levels = self.bloom.iter().map(|b| &b.0);
        for i in always.into_iter().chain(pairs).chain(single).chain(levels) {
            arena.drop_image(d, i);
        }
    }
}

/// Immer vorhandene kleine Bilder: Platzhalter und die Belichtung (1x1).
pub struct Fixed {
    pub dummy_depth: GpuImage,
    pub dummy_array: GpuImage,
    pub exposure: [GpuImage; 2],
}

impl Fixed {
    pub fn new(ctx: &Context, pool: vk::CommandPool, arena: &mut Arena) -> Result<Self> {
        let depth = TexDesc::depth(1, 1);
        let array = TexDesc {
            layers: CASCADES as u32,
            array: true,
            ..depth
        };
        let exposure = TexDesc::target(1, 1, EXPOSURE);
        let f = Self {
            dummy_depth: arena.image(ctx, &depth)?,
            dummy_array: arena.image(ctx, &array)?,
            exposure: [arena.image(ctx, &exposure)?, arena.image(ctx, &exposure)?],
        };
        let (c, z) = (vk::ImageAspectFlags::COLOR, vk::ImageAspectFlags::DEPTH);
        let images = [
            (&f.dummy_depth, z),
            (&f.dummy_array, z),
            (&f.exposure[0], c),
            (&f.exposure[1], c),
        ];
        let barriers = images.map(|(i, aspect)| {
            let mut b = image_barrier(
                i.image,
                aspect,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::SHADER_READ,
            );
            b.subresource_range.layer_count = vk::REMAINING_ARRAY_LAYERS;
            b
        });
        // SAFETY: einmalig beim Aufbau, danach wird gewartet.
        unsafe {
            one_shot(ctx, pool, |cmd| {
                ctx.device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &barriers,
                );
            })?;
        }
        Ok(f)
    }

    /// # Safety
    /// Die GPU darf die Bilder nicht mehr benutzen.
    pub unsafe fn destroy(&self, arena: &mut Arena, d: &ash::Device) {
        let [a, b] = &self.exposure;
        for i in [&self.dummy_depth, &self.dummy_array, a, b] {
            arena.drop_image(d, i);
        }
    }
}
