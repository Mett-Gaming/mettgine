//! Ausgabe fuer HDR-Bildschirme. Mit [`super::Config::hdr_output`] entsteht das Bild zuerst in einem
//! Zwischenbild (Anzeigewerte, 1 = Papierweiss, hellere Stellen darueber) und wird am Schluss fuer
//! den Bildschirm umgerechnet (`p_hdr.wgsl`). Die Oberflaeche mischt dadurch wie immer.

mod photo;

pub(in crate::render) use photo::{pixel_bytes, to_bgra};

use super::context::Context;
use super::descriptors::Descriptors;
use super::frame::{area, to_readable};
use super::pipeline::{create_pipeline, Blend};
use super::pipelines::{screen_desc, spv};
use super::transfer::set_viewport;
use super::util::{image_barrier, Image};
use super::{Renderer, Result, Swapchain};
use ash::vk;

/// Format des Zwischenbilds und des Szenenbilds
pub(super) const FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;

/// Helligkeiten der HDR-Ausgabe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HdrLevels {
    /// Weiss der Oberflaeche (Nits), 80 bis 500
    pub paper_white: f32,
    /// hellste Stellen (Nits), hoechstens was der Bildschirm kann
    pub peak: f32,
}

impl Default for HdrLevels {
    fn default() -> Self {
        Self {
            paper_white: 200.0,
            peak: 1000.0,
        }
    }
}

impl HdrLevels {
    fn clean(self) -> Self {
        let ok = |v: f32, or: f32| if v.is_finite() { v } else { or };
        let paper_white = ok(self.paper_white, 200.0).clamp(80.0, 500.0);
        Self {
            paper_white,
            peak: ok(self.peak, 1000.0).clamp(paper_white, 10_000.0),
        }
    }
}

/// Wie ein Farbraum beschrieben wird (Werte wie in `p_hdr.wgsl`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Encoding {
    Sdr = 0,
    Hdr10 = 1,
    ScRgb = 2,
}

impl Encoding {
    pub fn of(space: vk::ColorSpaceKHR) -> Self {
        match space {
            vk::ColorSpaceKHR::HDR10_ST2084_EXT => Self::Hdr10,
            vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT => Self::ScRgb,
            _ => Self::Sdr,
        }
    }
}

/// Bestes HDR-Format des Fensters; `None` = Bildschirm oder Treiber zeigt gerade kein HDR.
pub(super) fn choose(formats: &[vk::SurfaceFormatKHR]) -> Option<vk::SurfaceFormatKHR> {
    let wanted = [
        (
            vk::Format::A2B10G10R10_UNORM_PACK32,
            vk::ColorSpaceKHR::HDR10_ST2084_EXT,
        ),
        (
            vk::Format::A2R10G10B10_UNORM_PACK32,
            vk::ColorSpaceKHR::HDR10_ST2084_EXT,
        ),
        (FORMAT, vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT),
    ];
    wanted.into_iter().find_map(|(format, color_space)| {
        let hit = |f: &&vk::SurfaceFormatKHR| f.format == format && f.color_space == color_space;
        formats.iter().find(hit).copied()
    })
}

/// Zwischenbild und Umrechnung.
pub(crate) struct Hdr {
    /// HDR gewuenscht (sonst SDR ueber denselben Weg)
    on: bool,
    levels: HdrLevels,
    pub image: Image,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    pipe: vk::Pipeline,
    pipe_format: vk::Format,
    metadata: Option<ash::ext::hdr_metadata::Device>,
}

fn target(ctx: &Context, e: vk::Extent2D) -> Result<Image> {
    let usage = vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED;
    let aspect = vk::ImageAspectFlags::COLOR;
    Image::new(ctx, e.width, e.height, FORMAT, usage, aspect)
}

unsafe fn pipeline(
    ctx: &Context,
    layout: vk::PipelineLayout,
    format: vk::Format,
) -> Result<vk::Pipeline> {
    let (vs, fs) = (spv!("p_hdr.vert"), spv!("p_hdr.frag"));
    let desc = screen_desc(vs, fs, format, Blend::Opaque, &[], &[]);
    create_pipeline(&ctx.gpu(), layout, &desc)
}

impl Hdr {
    /// # Safety
    /// `desc` und `layout` gehoeren zum Geraet von `ctx`.
    pub unsafe fn new(
        ctx: &Context,
        desc: &Descriptors,
        layout: vk::PipelineLayout,
        sc: &Swapchain,
    ) -> Result<Self> {
        let d = &ctx.device;
        let sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLED_IMAGE,
                descriptor_count: 2,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLER,
                descriptor_count: 1,
            },
        ];
        let info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(1)
            .pool_sizes(&sizes);
        let pool = d.create_descriptor_pool(&info, None)?;
        let layouts = [desc.tex_layout];
        let info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&layouts);
        let set = d.allocate_descriptor_sets(&info)?[0];
        let hdr = Self {
            on: true,
            levels: HdrLevels::default(),
            image: target(ctx, sc.extent)?,
            pool,
            set,
            pipe: pipeline(ctx, layout, sc.format)?,
            pipe_format: sc.format,
            metadata: ctx
                .hdr_metadata
                .then(|| ash::ext::hdr_metadata::Device::new(&ctx.instance, d)),
        };
        hdr.write_set(d, desc);
        hdr.describe(sc);
        Ok(hdr)
    }

    fn write_set(&self, d: &ash::Device, desc: &Descriptors) {
        let image = [vk::DescriptorImageInfo::default()
            .image_view(self.image.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let sampler = [vk::DescriptorImageInfo::default().sampler(desc.samplers.nearest)];
        let w = |b: u32, ty| {
            vk::WriteDescriptorSet::default()
                .dst_set(self.set)
                .dst_binding(b)
                .descriptor_type(ty)
        };
        let picture = vk::DescriptorType::SAMPLED_IMAGE;
        let writes = [
            w(0, picture).image_info(&image),
            w(1, vk::DescriptorType::SAMPLER).image_info(&sampler),
            w(2, picture).image_info(&image),
        ];
        // SAFETY: Set und Bild gehoeren diesem Geraet und sind nicht in Benutzung.
        unsafe { d.update_descriptor_sets(&writes, &[]) };
    }

    /// Dem Bildschirm sagen, wie hell das Bild wird.
    fn describe(&self, sc: &Swapchain) {
        let Some(f) = self.metadata.as_ref() else {
            return;
        };
        if Encoding::of(sc.space) != Encoding::Hdr10 {
            return;
        }
        let xy = |x, y| vk::XYColorEXT { x, y };
        let info = [vk::HdrMetadataEXT::default()
            .display_primary_red(xy(0.708, 0.292))
            .display_primary_green(xy(0.170, 0.797))
            .display_primary_blue(xy(0.131, 0.046))
            .white_point(xy(0.3127, 0.3290))
            .max_luminance(self.levels.peak)
            .min_luminance(0.001)
            .max_content_light_level(self.levels.peak)
            .max_frame_average_light_level(self.levels.paper_white)];
        // SAFETY: Die Erweiterung ist eingeschaltet, die Swapchain lebt.
        unsafe { f.set_hdr_metadata(&[sc.handle], &info) };
    }

    /// Nach dem Neubau der Swapchain.
    ///
    /// # Safety
    /// Die GPU ist untaetig.
    pub unsafe fn rebuild(
        &mut self,
        ctx: &Context,
        desc: &Descriptors,
        layout: vk::PipelineLayout,
        sc: &Swapchain,
    ) -> Result<()> {
        let d = &ctx.device;
        let new = target(ctx, sc.extent)?;
        std::mem::replace(&mut self.image, new).destroy(d);
        if sc.format != self.pipe_format {
            let new = pipeline(ctx, layout, sc.format)?;
            d.destroy_pipeline(std::mem::replace(&mut self.pipe, new), None);
            self.pipe_format = sc.format;
        }
        self.write_set(d, desc);
        self.describe(sc);
        Ok(())
    }

    /// Zwischenbild umgerechnet aufs Fenster.
    ///
    /// # Safety
    /// `cmd` wird aufgenommen, das Zwischenbild ist gerade Farbziel.
    pub unsafe fn encode(
        &self,
        d: &ash::Device,
        cmd: vk::CommandBuffer,
        layout: vk::PipelineLayout,
        sc: &Swapchain,
        image: usize,
    ) {
        let out = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
        let to_target = image_barrier(
            sc.images[image],
            vk::ImageAspectFlags::COLOR,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            vk::AccessFlags::empty(),
            vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        );
        d.cmd_pipeline_barrier(
            cmd,
            out,
            out | vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[to_readable(self.image.image), to_target],
        );
        let att = [vk::RenderingAttachmentInfo::default()
            .image_view(sc.views[image])
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::DONT_CARE)
            .store_op(vk::AttachmentStoreOp::STORE)];
        let info = vk::RenderingInfo::default()
            .render_area(area(sc.extent))
            .layer_count(1)
            .color_attachments(&att);
        d.cmd_begin_rendering(cmd, &info);
        set_viewport(d, cmd, sc.extent);
        let gfx = vk::PipelineBindPoint::GRAPHICS;
        d.cmd_bind_pipeline(cmd, gfx, self.pipe);
        d.cmd_bind_descriptor_sets(cmd, gfx, layout, 0, &[self.set], &[]);
        let kind = Encoding::of(sc.space) as u32 as f32;
        let l = self.levels;
        let params = [kind, l.paper_white, l.peak, 0.0f32];
        let stages = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
        d.cmd_push_constants(cmd, layout, stages, 0, bytemuck::cast_slice(&params));
        d.cmd_draw(cmd, 3, 1, 0, 0);
        d.cmd_end_rendering(cmd);
    }

    /// # Safety
    /// Die GPU darf nichts davon mehr benutzen.
    pub unsafe fn destroy(&self, d: &ash::Device) {
        self.image.destroy(d);
        d.destroy_descriptor_pool(self.pool, None);
        d.destroy_pipeline(self.pipe, None);
    }
}

impl Renderer {
    /// HDR-Ausgabe an oder aus (nur mit [`super::Config::hdr_output`]; dann zu Beginn an). Zeigt der
    /// Bildschirm kein HDR, bleibt es bei SDR; siehe [`Renderer::hdr_active`].
    pub fn set_hdr_output(&mut self, on: bool) {
        if let Some(h) = self.hdr.as_mut().filter(|h| h.on != on) {
            h.on = on;
            self.needs_recreate = true;
        }
    }

    /// Helligkeiten der HDR-Ausgabe.
    pub fn set_hdr_levels(&mut self, levels: HdrLevels) {
        if let Some(h) = &mut self.hdr {
            let levels = levels.clean();
            if h.levels != levels {
                h.levels = levels;
                h.describe(&self.swapchain);
            }
        }
    }

    /// Wird gerade wirklich HDR ausgegeben?
    pub fn hdr_active(&self) -> bool {
        self.hdr.is_some() && Encoding::of(self.swapchain.space) != Encoding::Sdr
    }

    pub(super) fn wants_hdr(&self) -> bool {
        self.hdr.as_ref().is_some_and(|h| h.on)
    }

    /// So viel heller als Papierweiss duerfen Lichter werden (Anzeigewert, 1 = SDR).
    pub(super) fn headroom(&self) -> f32 {
        match &self.hdr {
            Some(h) if self.hdr_active() => {
                let k = h.levels.peak / h.levels.paper_white;
                1.055 * k.powf(1.0 / 2.4) - 0.055
            }
            _ => 1.0,
        }
    }

    pub(super) fn paper_white(&self) -> f32 {
        let l = self
            .hdr
            .as_ref()
            .map_or_else(HdrLevels::default, |h| h.levels);
        l.paper_white
    }

    /// Format, in das Nachbearbeitung und Oberflaeche zeichnen.
    pub(super) fn screen_format(&self) -> vk::Format {
        match self.hdr {
            Some(_) => FORMAT,
            None => self.swapchain.format,
        }
    }

    /// Bild, in das Nachbearbeitung und Oberflaeche zeichnen.
    pub(super) fn screen_target(&self, image: usize) -> (vk::Image, vk::ImageView) {
        match &self.hdr {
            Some(h) => (h.image.image, h.image.view),
            None => (self.swapchain.images[image], self.swapchain.views[image]),
        }
    }

    /// Ende eines Bildes: umrechnen (HDR), ggf. Foto, zum Anzeigen freigeben.
    ///
    /// # Safety
    /// `cmd` wird aufgenommen, das Ziel aus `screen_target` ist gerade Farbziel.
    pub(super) unsafe fn finish_commands(
        &self,
        cmd: vk::CommandBuffer,
        image: usize,
        capture: Option<vk::Buffer>,
    ) -> Result<()> {
        let (d, sc) = (&self.ctx.device, &self.swapchain);
        if let Some(h) = &self.hdr {
            h.encode(d, cmd, self.pipes.tex_layout, sc, image);
        }
        super::present::record_present(d, cmd, sc, sc.images[image], capture);
        d.end_command_buffer(cmd)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr10_comes_before_scrgb_and_sdr_is_never_taken() {
        let f = |format, color_space| vk::SurfaceFormatKHR {
            format,
            color_space,
        };
        let sdr = f(
            vk::Format::B8G8R8A8_UNORM,
            vk::ColorSpaceKHR::SRGB_NONLINEAR,
        );
        let sc = f(FORMAT, vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT);
        let ten = f(
            vk::Format::A2B10G10R10_UNORM_PACK32,
            vk::ColorSpaceKHR::HDR10_ST2084_EXT,
        );
        assert!(choose(&[sdr]).is_none());
        assert_eq!(choose(&[sdr, sc]), Some(sc));
        assert_eq!(choose(&[sc, sdr, ten]), Some(ten));
    }

    #[test]
    fn levels_stay_sane() {
        let l = HdrLevels {
            paper_white: f32::NAN,
            peak: 10.0,
        }
        .clean();
        assert_eq!((l.paper_white, l.peak), (200.0, 200.0));
    }
}
