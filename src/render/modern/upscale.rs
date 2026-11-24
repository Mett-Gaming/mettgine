//! Hochskalieren mit Techniken der Hersteller: Auswahl, Bildzustaende, Maske.

use super::super::context::Context;
use super::super::util::image_barrier;
use super::super::vendor::{Backend, Guides, Job, Picture, Upscaler, Vendors};
use super::memory::GpuImage;
use super::scene::barrier;
use super::targets::{GUIDE, HDR, MOTION, NORMAL, REACTIVE};
use super::{Inputs, Modern, SceneInput};
use ash::vk;
use glam::Vec2;

/// Angelegte Techniken eines Renderers.
pub(crate) struct Upscale {
    vendors: Vendors,
    wanted: Vec<Upscaler>,
    /// None = geht hier nicht
    made: Vec<(Upscaler, Option<Box<dyn Backend>>)>,
}

impl Upscale {
    pub fn new(vendors: Vendors, wanted: &[Upscaler]) -> Self {
        Self {
            vendors,
            wanted: wanted.to_vec(),
            made: Vec::new(),
        }
    }

    fn get(&mut self, ctx: &Context, which: Upscaler) -> Option<&mut Box<dyn Backend>> {
        if which == Upscaler::Own || !self.wanted.contains(&which) {
            return None;
        }
        if !self.made.iter().any(|m| m.0 == which) {
            let b = self.vendors.create(ctx, which);
            if let Some(b) = &b {
                crate::crash::log(&format!("Hochskalieren: {}", b.name()));
            }
            self.made.push((which, b));
        }
        let m = self.made.iter_mut().find(|m| m.0 == which)?;
        m.1.as_mut()
    }

    /// Steht die Technik bereit? (Legt sie beim ersten Mal an.)
    pub fn ready(&mut self, ctx: &Context, which: Upscaler) -> bool {
        self.get(ctx, which).is_some()
    }

    /// Kann die Technik auch entrauschen?
    pub fn denoises(&mut self, ctx: &Context, which: Upscaler) -> bool {
        self.get(ctx, which).is_some_and(|b| b.denoises())
    }

    pub fn available(&mut self, ctx: &Context) -> Vec<(Upscaler, String)> {
        let list = [Upscaler::Dlss, Upscaler::Fsr, Upscaler::Xess];
        list.into_iter()
            .filter_map(|u| self.get(ctx, u).map(|b| (u, b.name())))
            .collect()
    }

    /// # Safety
    /// Die GPU ist untaetig.
    pub unsafe fn destroy(&mut self, ctx: &Context) {
        for (_, b) in &mut self.made {
            if let Some(b) = b {
                b.destroy(ctx);
            }
        }
        self.made.clear();
        self.vendors.stop();
    }
}

const COMPUTE: vk::PipelineStageFlags = vk::PipelineStageFlags::COMPUTE_SHADER;
const FRAGMENT: vk::PipelineStageFlags = vk::PipelineStageFlags::FRAGMENT_SHADER;
const READ: vk::ImageLayout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
const DEPTH_READ: vk::ImageLayout = vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL;

fn change(
    image: vk::Image,
    aspect: vk::ImageAspectFlags,
    (from, to): (vk::ImageLayout, vk::ImageLayout),
    (write, read): (vk::AccessFlags, vk::AccessFlags),
) -> vk::ImageMemoryBarrier<'static> {
    image_barrier(image, aspect, from, to, write, read)
}

fn picture(g: &GpuImage, format: vk::Format, extent: vk::Extent2D) -> Picture {
    Picture {
        image: g.image,
        view: g.view,
        format,
        extent,
    }
}

impl Modern {
    /// Bilder und Kamera dieses Bildes fuer die Techniken.
    pub(super) fn job(
        &self,
        i: &Inputs,
        s: &SceneInput,
        (before, jitter): (glam::Mat4, Vec2),
    ) -> Option<Job> {
        let t = self.targets.as_ref()?;
        Some(Job {
            cmd: i.cmd,
            fi: i.fi,
            color: picture(&t.hdr, HDR, i.extent),
            depth: picture(&t.depth, super::super::DEPTH_FORMAT, i.extent),
            motion: picture(&t.motion, MOTION, i.extent),
            reactive: t.reactive.as_ref().map(|r| picture(r, REACTIVE, i.extent)),
            output: match &t.upscaled {
                Some(out) => picture(out, HDR, i.out),
                None => picture(&t.hdr, HDR, i.extent),
            },
            guides: t.guides.as_ref().map(|[albedo, specular, normal]| Guides {
                albedo: picture(albedo, GUIDE, i.extent),
                specular: picture(specular, GUIDE, i.extent),
                normal: picture(normal, NORMAL, i.extent),
            }),
            // so viele Bildpunkte wandert die Szene nach rechts und unten
            jitter: Vec2::new(jitter.x, -jitter.y),
            dt: s.dt,
            camera: s.camera,
            before,
            reset: s.cut || !self.history_valid,
        })
    }

    /// Szenenbild hochskalieren; `None` = die Technik hat versagt, das Szenenbild gilt.
    ///
    /// # Safety
    /// Wie `Modern::record`; die Szene ist fertig gezeichnet und lesbar.
    pub(super) unsafe fn upscale(
        &self,
        i: &Inputs,
        s: &SceneInput,
        upscale: &mut Upscale,
        (before, jitter): (glam::Mat4, Vec2),
    ) -> Option<vk::ImageView> {
        let t = self.targets.as_ref()?;
        let out = t.upscaled.as_ref()?;
        let backend = upscale.get(i.ctx, s.features.upscaler)?;
        let (d, cmd) = (&i.ctx.device, i.cmd);
        let (color, depth) = (vk::ImageAspectFlags::COLOR, vk::ImageAspectFlags::DEPTH);
        let (read, write) = (vk::AccessFlags::SHADER_READ, vk::AccessFlags::SHADER_WRITE);
        let none = vk::AccessFlags::empty();
        let mut images = vec![
            change(t.hdr.image, color, (READ, READ), (none, read)),
            change(t.motion.image, color, (READ, READ), (none, read)),
            change(t.depth.image, depth, (DEPTH_READ, READ), (none, read)),
            change(
                out.image,
                color,
                (vk::ImageLayout::UNDEFINED, vk::ImageLayout::GENERAL),
                (none, write | read),
            ),
        ];
        if let Some(r) = &t.reactive {
            images.push(change(r.image, color, (READ, READ), (none, read)));
        }
        for g in t.guides.iter().flatten() {
            images.push(change(g.image, color, (READ, READ), (none, read)));
        }
        let before_stages = FRAGMENT
            | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
            | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS;
        barrier(d, cmd, (before_stages, COMPUTE), &images);
        let job = self.job(i, s, (before, jitter))?;
        let result = backend.evaluate(i.ctx, &job);
        let images = [
            change(
                out.image,
                color,
                (vk::ImageLayout::GENERAL, READ),
                (write, read),
            ),
            change(t.depth.image, depth, (READ, DEPTH_READ), (none, read)),
        ];
        barrier(d, cmd, (COMPUTE, FRAGMENT), &images);
        match result {
            Ok(()) => Some(out.view),
            Err(e) => {
                crate::crash::log(&format!("Hochskalieren: {e}"));
                None
            }
        }
    }
}
