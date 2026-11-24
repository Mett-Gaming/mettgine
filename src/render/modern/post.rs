//! Aufnahme der Nachbearbeitung: Verdeckung, TAA, Belichtung, Ueberstrahlen, Bildschirmwerte.

use super::super::frame::area;
use super::super::transfer::set_viewport;
use super::super::UiImage;
use super::memory::GpuImage;
use super::scene::{barrier, color_read, color_target};
use super::sets::{IMAGE_SETS, POST_SETS};
use super::targets::Targets;
use super::types::*;
use super::{Inputs, Modern};
use ash::vk;
use glam::Vec4;

pub(super) const FRAG: vk::PipelineStageFlags = vk::PipelineStageFlags::FRAGMENT_SHADER;
pub(super) const OUT: vk::PipelineStageFlags = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;

/// Werte eines Durchgangs, gleich aufgebaut wie `Push` in `_p_common.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Push {
    pub a: [f32; 4],
    pub b: [f32; 4],
    inv: [[f32; 4]; 4],
    c: [f32; 4],
    d: [f32; 4],
}

impl Push {
    pub fn a(a: [f32; 4]) -> Self {
        Self {
            a,
            ..bytemuck::Zeroable::zeroed()
        }
    }
}

/// Wohin ein Durchgang zeichnet.
struct Target {
    view: vk::ImageView,
    extent: vk::Extent2D,
    /// vorhandenen Inhalt behalten (zum Dazuzaehlen)
    keep: bool,
}

/// Nachbearbeitung eines Bildes.
pub(super) struct Post<'a> {
    pub m: &'a Modern,
    pub i: &'a Inputs<'a>,
    pub s: &'a SceneInput<'a>,
    /// so viele Sets sind in diesem Bild schon vergeben
    pub used: usize,
}

/// Bilder der Oberflaeche aufs Fenster (Teil eines laufenden Durchgangs).
///
/// # Safety
/// `cmd` zeichnet gerade aufs Fenster, die GPU ist mit Bild `fi` fertig.
pub(crate) unsafe fn record_images(
    m: &Modern,
    d: &ash::Device,
    (cmd, fi): (vk::CommandBuffer, usize),
    area: (f32, f32),
    images: &[UiImage],
) {
    let gfx = vk::PipelineBindPoint::GRAPHICS;
    let layout = m.pipes.post_layout;
    for (k, image) in images.iter().take(IMAGE_SETS).enumerate() {
        let Some(t) = m.store.texture(image.texture) else {
            continue;
        };
        let set = m.sets.post[fi][POST_SETS + k];
        m.sets.write_post(d, set, [t.view; 4], None);
        d.cmd_bind_pipeline(cmd, gfx, m.pipes.image);
        d.cmd_bind_descriptor_sets(cmd, gfx, layout, 0, &[set], &[]);
        let push = Push {
            a: image.rect,
            b: image.uv,
            c: image.color,
            d: [area.0, area.1, 0.0, 0.0],
            ..bytemuck::Zeroable::zeroed()
        };
        d.cmd_push_constants(
            cmd,
            layout,
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            0,
            bytemuck::bytes_of(&push),
        );
        d.cmd_draw(cmd, 6, 1, 0, 0);
    }
}

impl<'a> Post<'a> {
    pub fn new(m: &'a Modern, i: &'a Inputs<'a>, s: &'a SceneInput<'a>) -> Self {
        Self { m, i, s, used: 0 }
    }

    /// Ein bildschirmfuellender Durchgang. Fehlende Bilder werden mit Weiss aufgefuellt.
    unsafe fn pass(
        &mut self,
        pipe: vk::Pipeline,
        target: Target,
        views: &[vk::ImageView],
        push: &Push,
    ) {
        let (d, cmd) = (&self.i.ctx.device, self.i.cmd);
        if self.used >= POST_SETS {
            return;
        }
        let set = self.m.sets.post[self.i.fi][self.used];
        self.used += 1;
        let white = self.m.store.texture(self.m.store.white);
        let white = white.map_or(vk::ImageView::null(), |w| w.view);
        let all = std::array::from_fn(|k| views.get(k).copied().unwrap_or(white));
        // Die Tiefe liegt in einem anderen Lese-Zustand als die Farbbilder
        let depth = self.m.targets.as_ref().map(|t| t.depth.view);
        self.m.sets.write_post(d, set, all, depth);
        let load = if target.keep {
            vk::AttachmentLoadOp::LOAD
        } else {
            vk::AttachmentLoadOp::DONT_CARE
        };
        let att = [vk::RenderingAttachmentInfo::default()
            .image_view(target.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(load)
            .store_op(vk::AttachmentStoreOp::STORE)];
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area(target.extent))
                .layer_count(1)
                .color_attachments(&att),
        );
        set_viewport(d, cmd, target.extent);
        let gfx = vk::PipelineBindPoint::GRAPHICS;
        let layout = self.m.pipes.post_layout;
        d.cmd_bind_pipeline(cmd, gfx, pipe);
        d.cmd_bind_descriptor_sets(cmd, gfx, layout, 0, &[set], &[]);
        d.cmd_push_constants(
            cmd,
            layout,
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            0,
            bytemuck::bytes_of(push),
        );
        d.cmd_draw(cmd, 3, 1, 0, 0);
        d.cmd_end_rendering(cmd);
    }

    /// Durchgang in ein Bild, das danach gelesen wird.
    pub unsafe fn draw_into(
        &mut self,
        pipe: vk::Pipeline,
        (image, extent): (&GpuImage, vk::Extent2D),
        views: &[vk::ImageView],
        push: &Push,
    ) {
        let (d, cmd) = (&self.i.ctx.device, self.i.cmd);
        barrier(d, cmd, (FRAG | OUT, OUT), &[color_target(image.image)]);
        let target = Target {
            view: image.view,
            extent,
            keep: false,
        };
        self.pass(pipe, target, views, push);
        barrier(d, cmd, (OUT, FRAG), &[color_read(image.image)]);
    }

    /// Verdeckung (und Sonnenschatten aus Strahlen) berechnen und glaetten, nach dem
    /// Tiefen-Durchgang.
    pub unsafe fn occlusion(&mut self) {
        let m = self.m;
        let Some(t) = &m.targets else {
            return;
        };
        let Some([a, b]) = &t.ao else {
            return;
        };
        let ray = &self.s.features.ray_tracing;
        if ray.sun_shadows || ray.occlusion {
            self.ray_visibility(a);
        } else {
            self.ssao(t, a);
        }
        let cam = &self.s.camera;
        for (from, to, dir) in [(a, b, [1.0, 0.0]), (b, a, [0.0, 1.0])] {
            let push = Push::a([dir[0], dir[1], cam.near, cam.far]);
            let views = [from.view, t.depth.view];
            self.draw_into(m.pipes.post.blur, (to, self.i.extent), &views, &push);
        }
    }

    unsafe fn ssao(&mut self, t: &Targets, out: &GpuImage) {
        let m = self.m;
        let cfg = self.s.features.ssao.unwrap_or_default();
        let cam = &self.s.camera;
        let extent = self.i.extent;
        let (w, h) = (extent.width as f32, extent.height as f32);
        let jitter = if t.wants.taa {
            super::gpu::jitter(m.number)
        } else {
            glam::Vec2::ZERO
        };
        let inv = super::gpu::jittered(cam, jitter, extent).inverse();
        let turn = if t.wants.taa {
            (m.number % 8) as f32
        } else {
            0.0
        };
        let dir = -cam.view.inverse().z_axis.truncate();
        let per_meter = h * 0.5 / (cam.fov_y * 0.5).tan();
        let push = Push {
            a: [w, h, cfg.radius.max(0.01), cfg.strength],
            b: cam.pos.extend(turn).to_array(),
            inv: inv.to_cols_array_2d(),
            c: dir.extend(per_meter).to_array(),
            d: Vec4::ZERO.to_array(),
        };
        let views = [t.depth.view, t.normal.view];
        self.draw_into(m.pipes.post.ssao, (out, extent), &views, &push);
    }

    /// Alles nach der Beleuchtung, bis ins Szenenbild des Renderers.
    pub unsafe fn finish(&mut self) {
        let m = self.m;
        let Some(t) = &m.targets else {
            return;
        };
        let hdr = self.taa(t);
        let exposure = self.exposure(hdr);
        let bloom = self.bloom(t, hdr, exposure);
        self.tonemap(t, hdr, bloom, exposure);
    }

    /// Liefert das Bild, mit dem es weitergeht.
    unsafe fn taa(&mut self, t: &Targets) -> vk::ImageView {
        let m = self.m;
        let Some(h) = &t.history else {
            return t.hdr.view;
        };
        let (new, old) = (&h[m.ping ^ 1], &h[m.ping]);
        // ohne Verlauf ist das alte Bild noch leer: lieber das neue lesen
        let old_view = if m.history_valid {
            old.view
        } else {
            t.hdr.view
        };
        let (e, out) = (self.i.extent, self.i.out);
        let valid = m.history_valid as u8 as f32;
        let mut push = Push::a([e.width as f32, e.height as f32, valid, 0.0]);
        let jitter = super::gpu::jitter(m.number);
        push.b = [out.width as f32, out.height as f32, jitter.x, jitter.y];
        let views = [t.hdr.view, old_view, t.motion.view, t.depth.view];
        self.draw_into(m.pipes.post.taa, (new, out), &views, &push);
        new.view
    }

    unsafe fn exposure(&mut self, hdr: vk::ImageView) -> vk::ImageView {
        let m = self.m;
        let (new, old) = (&m.fixed.exposure[m.ping ^ 1], &m.fixed.exposure[m.ping]);
        let a = match self.s.features.exposure {
            Exposure::Fixed(v) => [0.0, 0.0, 0.0, v.max(0.0001)],
            Exposure::Auto { min, max, speed } => {
                let step = 1.0 - (-self.s.dt.max(0.0) * speed).exp();
                [if m.history_valid { step } else { 1.0 }, min, max, 0.0]
            }
        };
        let one = vk::Extent2D {
            width: 1,
            height: 1,
        };
        let views = [hdr, old.view];
        self.draw_into(m.pipes.post.exposure, (new, one), &views, &Push::a(a));
        new.view
    }

    unsafe fn bloom(
        &mut self,
        t: &Targets,
        hdr: vk::ImageView,
        exposure: vk::ImageView,
    ) -> Option<vk::ImageView> {
        let m = self.m;
        let cfg = self.s.features.bloom?;
        let (d, cmd) = (&self.i.ctx.device, self.i.cmd);
        let inv = |e: vk::Extent2D| (1.0 / e.width as f32, 1.0 / e.height as f32);
        let has_taa = t.history.is_some();
        let full = if has_taa { self.i.out } else { self.i.extent };
        let (mut src, mut size) = (hdr, full);
        for (k, (image, extent)) in t.bloom.iter().enumerate() {
            let (x, y) = inv(size);
            let first = (k == 0) as u8 as f32;
            let push = Push::a([x, y, cfg.threshold, first]);
            self.draw_into(m.pipes.post.down, (image, *extent), &[src, exposure], &push);
            (src, size) = (image.view, *extent);
        }
        for k in (1..t.bloom.len()).rev() {
            let (small, from) = &t.bloom[k];
            let (big, extent) = &t.bloom[k - 1];
            let (x, y) = inv(*from);
            // wieder beschreiben, Inhalt behalten
            let mut again = color_target(big.image);
            again.old_layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
            barrier(d, cmd, (FRAG, OUT), &[again]);
            let target = Target {
                view: big.view,
                extent: *extent,
                keep: true,
            };
            let push = Push::a([x, y, 1.0, 0.0]);
            self.pass(m.pipes.post.up, target, &[small.view], &push);
            barrier(d, cmd, (OUT, FRAG), &[color_read(big.image)]);
        }
        t.bloom.first().map(|b| b.0.view)
    }

    unsafe fn tonemap(
        &mut self,
        t: &Targets,
        hdr: vk::ImageView,
        bloom: Option<vk::ImageView>,
        exposure: vk::ImageView,
    ) {
        let m = self.m;
        let f = &self.s.features;
        let kind = match f.tonemap {
            Tonemap::None => 0.0,
            Tonemap::Aces => 1.0,
            Tonemap::Agx => 2.0,
        };
        let strength = f
            .bloom
            .filter(|_| bloom.is_some())
            .map_or(0.0, |b| b.strength);
        let push = Push::a([kind, strength, self.i.headroom, 0.0]);
        let views = [hdr, bloom.unwrap_or(hdr), exposure];
        let (d, cmd) = (&self.i.ctx.device, self.i.cmd);
        let extent = self.i.out;
        barrier(d, cmd, (FRAG | OUT, OUT), &[color_target(self.i.color)]);
        let screen = Target {
            view: self.i.color_view,
            extent,
            keep: false,
        };
        match &t.ldr {
            None => self.pass(m.pipes.post.tonemap, screen, &views, &push),
            Some(ldr) => {
                self.draw_into(m.pipes.post.tonemap, (ldr, extent), &views, &push);
                let (x, y) = (1.0 / extent.width as f32, 1.0 / extent.height as f32);
                let fxaa = f.anti_alias == AntiAlias::Fxaa;
                let pipe = if fxaa {
                    m.pipes.post.fxaa
                } else {
                    m.pipes.post.sharpen
                };
                let push = Push::a([x, y, f.sharpen.clamp(0.0, 1.0), 0.0]);
                self.pass(pipe, screen, &[ldr.view], &push);
            }
        }
    }
}
