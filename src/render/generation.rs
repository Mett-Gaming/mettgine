//! Bilderzeugung im Bildablauf: Bild ohne Oberflaeche merken, ein- und ausschalten.

use super::hdr::pixel_bytes;
use super::util::{image_barrier, Image};
use super::vendor::{Generation, Picture};
use super::{Context, Renderer, Result, Swapchain};
use ash::vk;
use std::rc::Rc;

/// Zustand der Bilderzeugung.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameGeneration {
    /// zuletzt gezeigte Bilder je gerechnetem (2 = ein erzeugtes dazwischen)
    pub presented: u32,
    /// Fehlerzeichen der Technik, 0 = in Ordnung
    pub status: u32,
}

/// Bilderzeugung eines Renderers.
pub(crate) struct Generator {
    pub gen: Rc<Generation>,
    hudless: Image,
    extent: vk::Extent2D,
    format: vk::Format,
}

fn image(ctx: &Context, sc: &Swapchain) -> Result<Image> {
    let usage = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST;
    let aspect = vk::ImageAspectFlags::COLOR;
    let e = sc.extent;
    Image::new(ctx, e.width, e.height, sc.format, usage, aspect)
}

impl Generator {
    pub fn new(ctx: &Context, sc: &Swapchain, gen: Rc<Generation>) -> Result<Self> {
        Ok(Self {
            gen,
            hudless: image(ctx, sc)?,
            extent: sc.extent,
            format: sc.format,
        })
    }

    /// # Safety
    /// Die GPU ist untaetig.
    pub unsafe fn rebuild(&mut self, ctx: &Context, sc: &Swapchain) -> Result<()> {
        if (self.extent, self.format) != (sc.extent, sc.format) {
            let new = image(ctx, sc)?;
            std::mem::replace(&mut self.hudless, new).destroy(&ctx.device);
            (self.extent, self.format) = (sc.extent, sc.format);
        }
        Ok(())
    }

    pub fn destroy(&self, d: &ash::Device) {
        self.hudless.destroy(d);
    }

    /// Swapchain-Bild (gerade Farbziel) als Bild ohne Oberflaeche merken.
    ///
    /// # Safety
    /// `cmd` wird ausserhalb eines Durchgangs aufgenommen.
    pub unsafe fn keep(&self, d: &ash::Device, cmd: vk::CommandBuffer, from: vk::Image) {
        use vk::{AccessFlags as A, ImageLayout as L, PipelineStageFlags as S};
        let color = vk::ImageAspectFlags::COLOR;
        let draw = A::COLOR_ATTACHMENT_READ | A::COLOR_ATTACHMENT_WRITE;
        let (target, src, dst) = (
            L::COLOR_ATTACHMENT_OPTIMAL,
            L::TRANSFER_SRC_OPTIMAL,
            L::TRANSFER_DST_OPTIMAL,
        );
        let to = self.hudless.image;
        let change = |from: S, till: S, list: &[vk::ImageMemoryBarrier]| {
            d.cmd_pipeline_barrier(cmd, from, till, Default::default(), &[], &[], list);
        };
        let out = S::COLOR_ATTACHMENT_OUTPUT;
        change(
            out | S::ALL_COMMANDS,
            S::TRANSFER,
            &[
                image_barrier(from, color, target, src, draw, A::TRANSFER_READ),
                image_barrier(to, color, L::UNDEFINED, dst, A::empty(), A::TRANSFER_WRITE),
            ],
        );
        let layers = vk::ImageSubresourceLayers {
            aspect_mask: color,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        };
        let region = vk::ImageCopy::default()
            .src_subresource(layers)
            .dst_subresource(layers)
            .extent(self.extent.into());
        d.cmd_copy_image(cmd, from, src, to, dst, &[region]);
        let read = L::SHADER_READ_ONLY_OPTIMAL;
        change(
            S::TRANSFER,
            out | S::ALL_COMMANDS,
            &[
                image_barrier(from, color, src, target, A::TRANSFER_READ, draw),
                image_barrier(to, color, dst, read, A::TRANSFER_WRITE, A::SHADER_READ),
            ],
        );
        let p = Picture {
            image: to,
            view: self.hudless.view,
            format: self.format,
            extent: self.extent,
        };
        self.gen.hudless(cmd, &p);
    }
}

impl Renderer {
    /// Hoechste Anzahl erzeugter Bilder je gerechnetem; 0 = Bilderzeugung gibt es hier nicht.
    pub fn frame_generation_max(&self) -> u32 {
        self.generator.as_ref().map_or(0, |g| g.gen.max())
    }

    /// Bilder zwischen zwei gerechneten (0 = aus, Vorgabe). Nur mit der modernen Grafik und ohne
    /// Bindung an den Bildschirm; sinnvoll ab etwa 60 gerechneten Bildern pro Sekunde.
    pub fn set_frame_generation(&mut self, frames: u32) {
        if let Some(g) = &self.generator {
            if g.gen.frames() != frames.min(g.gen.max().max(1)) {
                g.gen.set_frames(frames);
                self.needs_recreate = true;
            }
        }
    }

    /// `None` = aus oder nicht da.
    pub fn frame_generation(&self) -> Option<FrameGeneration> {
        let g = self.generator.as_ref().filter(|g| g.gen.frames() > 0)?;
        let (presented, status) = g.gen.state();
        Some(FrameGeneration { presented, status })
    }

    /// Laeuft die Bilderzeugung, und laesst sich das Bild ohne Oberflaeche merken?
    pub(super) fn keeps_hudless(&self) -> Option<&Generator> {
        let g = self.generator.as_ref().filter(|g| g.gen.frames() > 0)?;
        let plain = self.hdr.is_none() && pixel_bytes(self.swapchain.format) == 4;
        (plain && self.swapchain.can_capture).then_some(g)
    }
}
