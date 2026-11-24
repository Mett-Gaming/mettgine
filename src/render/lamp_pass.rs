//! Lampenschatten zeichnen (einfache Grafik). Auswahl: lamps.rs, Pipelines: lamp_pipes.rs.
//!
//! 1. stehende Welt in ihren Atlas, nur veraltete Felder
//! 2. Kopie in den Atlas, den die Shader lesen; bewegte Modelle und Figuren obendrauf

use super::frame::{depth_clear, Counts};
use super::lamp_pipes::LampPipelines;
use super::lamps::{rows, LampShadows, Lamps, Swaying, COLUMNS, MAX_LAMPS};
use super::util::image_barrier;
use super::*;
use glam::Vec4;

/// Alles zu den Lampenschatten.
pub(super) struct LampPass {
    pub cfg: LampShadows,
    pub lamps: Lamps,
    pub pipes: LampPipelines,
    /// was die Shader lesen
    pub atlas: Image,
    /// nur die stehende Welt
    pub fixed: Image,
    extent: vk::Extent2D,
    /// `fixed` enthaelt noch nichts
    pub empty: bool,
    pub runs: Vec<Group>,
}

impl LampPass {
    pub unsafe fn new(
        ctx: &Context,
        set: vk::DescriptorSetLayout,
        color: vk::Format,
    ) -> Result<Self> {
        let (atlas, fixed, extent) = Self::images(ctx, &LampShadows::default())?;
        Ok(Self {
            cfg: LampShadows::default(),
            lamps: Lamps::default(),
            pipes: LampPipelines::new(&ctx.gpu(), set, color)?,
            atlas,
            fixed,
            extent,
            empty: true,
            runs: Vec::new(),
        })
    }

    fn images(ctx: &Context, cfg: &LampShadows) -> Result<(Image, Image, vk::Extent2D)> {
        let extent = match cfg.count {
            0 => vk::Extent2D {
                width: 1,
                height: 1,
            },
            n => vk::Extent2D {
                width: COLUMNS * cfg.resolution,
                height: rows(n) * cfg.resolution,
            },
        };
        let make = |extra| {
            let usage = vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | extra;
            let aspect = vk::ImageAspectFlags::DEPTH;
            Image::new(
                ctx,
                extent.width,
                extent.height,
                DEPTH_FORMAT,
                usage,
                aspect,
            )
        };
        Ok((
            make(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)?,
            make(vk::ImageUsageFlags::TRANSFER_SRC)?,
            extent,
        ))
    }

    pub unsafe fn destroy(&self, d: &ash::Device) {
        self.pipes.destroy(d);
        self.atlas.destroy(d);
        self.fixed.destroy(d);
    }

    /// Feld im Atlas.
    pub fn area(&self, slot: usize) -> vk::Rect2D {
        let (n, r) = (slot as u32, self.cfg.resolution);
        vk::Rect2D {
            offset: vk::Offset2D {
                x: (n % COLUMNS * r) as i32,
                y: (n / COLUMNS * r) as i32,
            },
            extent: vk::Extent2D {
                width: r,
                height: r,
            },
        }
    }
}

pub(super) const DEPTH_STAGES: vk::PipelineStageFlags = vk::PipelineStageFlags::from_raw(
    vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS.as_raw()
        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS.as_raw(),
);
pub(super) const DEPTH_ACCESS: vk::AccessFlags = vk::AccessFlags::from_raw(
    vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ.as_raw()
        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE.as_raw(),
);

impl Renderer {
    /// Schatten von Lampen und Verdunkelung in Ecken bei [`Lighting::Rooms`] (Vorgabe: aus; dann
    /// aendert sich am Bild nichts). Gilt auch fuer eigene Welt-Materialien und die zweite
    /// Kamera. Wartet auf die GPU, also nur beim Umstellen aufrufen.
    pub fn set_lamp_shadows(&mut self, cfg: LampShadows) -> Result<()> {
        let cfg = cfg.clamped();
        let old = self.lamp.cfg;
        if cfg == old {
            return Ok(());
        }
        if (cfg.count, cfg.resolution) != (old.count, old.resolution) {
            // SAFETY: danach nutzt die GPU die alten Bilder nicht mehr.
            unsafe { self.ctx.device.device_wait_idle()? };
            let (atlas, fixed, extent) = LampPass::images(&self.ctx, &cfg)?;
            let l = &mut self.lamp;
            let d = &self.ctx.device;
            std::mem::replace(&mut l.atlas, atlas).destroy(d);
            std::mem::replace(&mut l.fixed, fixed).destroy(d);
            (l.extent, l.empty) = (extent, true);
            l.lamps.world_changed();
            self.lamp.cfg = cfg;
            // SAFETY: wie oben.
            unsafe { self.ready_lamp_atlas()? };
            for fi in 0..FRAMES_IN_FLIGHT {
                self.write_scene_descriptors(fi);
            }
        }
        self.lamp.cfg = cfg;
        Ok(())
    }

    pub fn lamp_shadows(&self) -> LampShadows {
        self.lamp.cfg
    }

    /// Atlas lesbar machen, auch wenn nie etwas hineingezeichnet wird.
    ///
    /// # Safety
    /// Die GPU darf ihn gerade nicht benutzen.
    pub(super) unsafe fn ready_lamp_atlas(&self) -> Result<()> {
        init::make_readable(
            &self.ctx,
            self.command_pool,
            &[(&self.lamp.atlas, vk::ImageAspectFlags::DEPTH)],
        )
    }

    /// Werfen in diesem Bild Lampen Schatten?
    fn lamps_on(&self, f: &FrameInput) -> bool {
        self.lamp.cfg.count > 0 && f.scene.is_none() && matches!(f.lighting, Lighting::Rooms(_))
    }

    /// Lampen dieses Bildes waehlen und ihre Daten hochladen; liefert ihre Anzahl.
    pub(super) fn prepare_lamps(&mut self, fi: usize, f: &FrameInput) -> usize {
        let Lighting::Rooms(rooms) = &f.lighting else {
            return 0;
        };
        if !self.lamps_on(f) {
            return 0;
        }
        let l = &mut self.lamp;
        let now = Swaying {
            sway: rooms.sway,
            time: f.time,
        };
        l.lamps.update(&self.lights, f.view.pos, now, &l.cfg);
        let tiles = l.lamps.tiles(&l.cfg);
        self.lamp_bufs[fi].write(bytemuck::cast_slice(&tiles));
        tiles.len().min(MAX_LAMPS)
    }

    /// Kugeln, in denen bewegte Dinge Schatten werfen.
    pub(super) fn lamp_spheres(&self, f: &FrameInput) -> Vec<Sphere> {
        if !self.lamps_on(f) {
            return Vec::new();
        }
        self.lamp.lamps.active().map(|(_, s)| s.sphere).collect()
    }

    pub(super) unsafe fn begin_depth(
        &self,
        cmd: vk::CommandBuffer,
        img: &Image,
        load: vk::AttachmentLoadOp,
    ) {
        let att = vk::RenderingAttachmentInfo::default()
            .image_view(img.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(load)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(depth_clear());
        self.ctx.device.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(super::frame::area(self.lamp.extent))
                .layer_count(1)
                .depth_attachment(&att),
        );
    }

    /// Schatten aller gewaehlten Lampen.
    pub(super) unsafe fn record_lamps(
        &mut self,
        cmd: vk::CommandBuffer,
        fi: usize,
        f: &FrameInput,
        n: Counts,
    ) {
        if !self.lamps_on(f) {
            return;
        }
        let sway = match &f.lighting {
            Lighting::Rooms(r) => r.sway,
            Lighting::Sun(_) => Vec4::ZERO,
        };
        self.draw_fixed(cmd, fi, sway);
        let d = &self.ctx.device;
        let depth = vk::ImageAspectFlags::DEPTH;
        let barrier = |src, dst, b: &[vk::ImageMemoryBarrier]| {
            d.cmd_pipeline_barrier(cmd, src, dst, vk::DependencyFlags::empty(), &[], &[], b)
        };
        use vk::{AccessFlags as A, ImageLayout as L, PipelineStageFlags as S};
        let l = &self.lamp;
        barrier(
            S::FRAGMENT_SHADER,
            S::TRANSFER,
            &[image_barrier(
                l.atlas.image,
                depth,
                L::UNDEFINED,
                L::TRANSFER_DST_OPTIMAL,
                A::empty(),
                A::TRANSFER_WRITE,
            )],
        );
        let layers = vk::ImageSubresourceLayers {
            aspect_mask: depth,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        };
        let copies: Vec<_> = l
            .lamps
            .active()
            .map(|(i, _)| {
                let a = l.area(i);
                let o = vk::Offset3D {
                    x: a.offset.x,
                    y: a.offset.y,
                    z: 0,
                };
                vk::ImageCopy {
                    src_subresource: layers,
                    src_offset: o,
                    dst_subresource: layers,
                    dst_offset: o,
                    extent: vk::Extent3D {
                        width: a.extent.width,
                        height: a.extent.height,
                        depth: 1,
                    },
                }
            })
            .collect();
        if !copies.is_empty() {
            d.cmd_copy_image(
                cmd,
                l.fixed.image,
                L::TRANSFER_SRC_OPTIMAL,
                l.atlas.image,
                L::TRANSFER_DST_OPTIMAL,
                &copies,
            );
        }
        barrier(
            S::TRANSFER,
            DEPTH_STAGES,
            &[image_barrier(
                l.atlas.image,
                depth,
                L::TRANSFER_DST_OPTIMAL,
                L::DEPTH_ATTACHMENT_OPTIMAL,
                A::TRANSFER_WRITE,
                DEPTH_ACCESS,
            )],
        );
        self.begin_depth(cmd, &l.atlas, vk::AttachmentLoadOp::LOAD);
        self.draw_moving(cmd, fi, f, n);
        d.cmd_end_rendering(cmd);
        barrier(
            S::LATE_FRAGMENT_TESTS,
            S::FRAGMENT_SHADER,
            &[image_barrier(
                l.atlas.image,
                depth,
                L::DEPTH_ATTACHMENT_OPTIMAL,
                L::SHADER_READ_ONLY_OPTIMAL,
                A::DEPTH_STENCIL_ATTACHMENT_WRITE,
                A::SHADER_READ,
            )],
        );
    }
}
