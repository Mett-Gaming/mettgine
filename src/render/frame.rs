//! Aufnahme eines Bildes: Schattenkarte, zweite Kamera und Szene (der Rest steht in screen.rs).

use super::descriptors::Pass;
use super::lists::{SkinKind, SkinSlot};
use super::pipelines::ScenePipelines;
use super::transfer::set_viewport;
use super::util::image_barrier;
use super::*;

/// Zahlen, die die Aufnahme aus der Vorbereitung braucht.
#[derive(Clone, Copy)]
pub(super) struct Counts {
    /// erste Figuren-Instanz (hinter Modellen und Hologrammen)
    pub skin_base: u32,
    pub ui: u32,
    pub particles: u32,
}

pub(super) fn depth_clear() -> vk::ClearValue {
    vk::ClearValue {
        depth_stencil: vk::ClearDepthStencilValue {
            depth: 1.0,
            stencil: 0,
        },
    }
}

pub(super) fn area(extent: vk::Extent2D) -> vk::Rect2D {
    vk::Rect2D {
        offset: vk::Offset2D::default(),
        extent,
    }
}

/// Horizont- bzw. Nebelfarbe: damit beginnt jedes Bild.
fn background(f: &FrameInput) -> vk::ClearValue {
    let c = match &f.lighting {
        Lighting::Sun(s) => s.horizon,
        Lighting::Rooms(r) => r.fog_color,
    };
    vk::ClearValue {
        color: vk::ClearColorValue {
            float32: [c.x, c.y, c.z, 1.0],
        },
    }
}

/// Ein Farbbild wird (neu) beschrieben, sein alter Inhalt ist egal.
fn to_color_target(image: vk::Image) -> vk::ImageMemoryBarrier<'static> {
    image_barrier(
        image,
        vk::ImageAspectFlags::COLOR,
        vk::ImageLayout::UNDEFINED,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        vk::AccessFlags::SHADER_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
    )
}

/// Ein Tiefenbild wird (neu) beschrieben.
fn to_depth_target(image: vk::Image) -> vk::ImageMemoryBarrier<'static> {
    image_barrier(
        image,
        vk::ImageAspectFlags::DEPTH,
        vk::ImageLayout::UNDEFINED,
        vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
        vk::AccessFlags::SHADER_READ | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
        vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
    )
}

/// Ein fertiges Farbbild wird gelesen.
pub(super) fn to_readable(image: vk::Image) -> vk::ImageMemoryBarrier<'static> {
    image_barrier(
        image,
        vk::ImageAspectFlags::COLOR,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        vk::AccessFlags::SHADER_READ,
    )
}

impl Renderer {
    pub(super) unsafe fn record(
        &mut self,
        cmd: vk::CommandBuffer,
        fi: usize,
        image: usize,
        f: &FrameInput,
        n: Counts,
        capture: Option<vk::Buffer>,
    ) -> Result<()> {
        let d = &self.ctx.device;
        d.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
        d.begin_command_buffer(
            cmd,
            &vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )?;
        self.timer.begin(d, cmd, fi);
        if let Some(scene) = f.scene {
            self.record_modern(cmd, fi, f, scene, n)?;
        } else {
            if let Some(m) = &mut self.modern {
                m.upload_only(&self.ctx, cmd);
            }
            if let Lighting::Sun(sun) = &f.lighting {
                self.record_shadow(cmd, fi, sun, n);
                self.timer.mark(d, cmd, fi, "Schatten");
            }
            if self.second.enabled {
                self.record_second(cmd, fi, f, n);
                self.timer.mark(d, cmd, fi, "Zweite Kamera");
            }
            self.record_scene(cmd, fi, f, n);
            self.timer.mark(d, cmd, fi, "Szene");
        }
        let d = &self.ctx.device;
        self.record_screen(cmd, image, f, fi, n.ui);
        self.timer.mark(d, cmd, fi, "Nachbearbeitung");
        self.finish_commands(cmd, image, capture)
    }

    /// Moderne Grafik statt des einfachen Szenen-Durchgangs (Ergebnis ebenfalls in `color`).
    unsafe fn record_modern(
        &mut self,
        cmd: vk::CommandBuffer,
        fi: usize,
        f: &FrameInput,
        scene: &SceneInput,
        n: Counts,
    ) -> Result<()> {
        let mut m = self.modern.take().ok_or("moderne Grafik fehlt")?;
        let mut inputs = modern::Inputs {
            ctx: &self.ctx,
            cmd,
            fi,
            extent: self.offscreen,
            out: self.offscreen,
            headroom: self.headroom(),
            generation: self
                .generator
                .as_ref()
                .map(|g| g.gen.clone())
                .filter(|g| g.frames() > 0),
            color: self.color.image,
            color_view: self.color.view,
            bones: &self.bone_bufs[fi],
            particles: (self.particle_bufs[fi].buffer, n.particles),
            mesh_vertices: self.mesh_vertex_buf.buffer,
            mesh_buffer: &self.mesh_vertex_buf,
            mesh_ranges: &self.mesh_ranges,
            skin_vertices: self.skin_vertex_buf.buffer,
            skin_ranges: &self.skin_ranges,
            mesh_bounds: &self.mesh_bounds,
            skin_bounds: &self.skin_bounds,
            boxes: &self.boxes,
            statics: &self.statics,
        };
        let timer = &self.timer;
        let d = &self.ctx.device;
        let result = m.record(&mut inputs, f, scene, &mut self.upscale, &mut |name| {
            timer.mark(d, cmd, fi, name)
        });
        let (drawn, culled, calls) = m.counts();
        self.stats.drawn = drawn;
        self.stats.culled = culled;
        self.stats.draw_calls = calls;
        self.modern = Some(m);
        result
    }

    /// Pipeline binden; `fixed` = feste Instanzen statt der beweglichen.
    unsafe fn bind(
        &self,
        cmd: vk::CommandBuffer,
        pipe: vk::Pipeline,
        fi: usize,
        pass: Pass,
        fixed: bool,
    ) {
        let d = &self.ctx.device;
        let gfx = vk::PipelineBindPoint::GRAPHICS;
        let set = self.desc.frames[fi][pass as usize][fixed as usize];
        d.cmd_bind_pipeline(cmd, gfx, pipe);
        d.cmd_bind_descriptor_sets(cmd, gfx, self.pipes.scene_layout, 0, &[set], &[]);
    }

    unsafe fn draw_groups(&self, cmd: vk::CommandBuffer, gs: &[Group]) {
        let d = &self.ctx.device;
        d.cmd_bind_vertex_buffers(cmd, 0, &[self.mesh_vertex_buf.buffer], &[0]);
        for g in gs {
            let (first, count) = self.mesh_ranges[g.mesh as usize];
            d.cmd_draw(cmd, count, g.count, first, g.first_inst);
        }
    }

    unsafe fn draw_skins(
        &self,
        cmd: vk::CommandBuffer,
        n: Counts,
        shown: impl Fn(&SkinSlot) -> bool,
    ) {
        if self.lists.skins.is_empty() || self.skin_ranges.is_empty() {
            return;
        }
        let d = &self.ctx.device;
        d.cmd_bind_vertex_buffers(cmd, 0, &[self.skin_vertex_buf.buffer], &[0]);
        for (k, s) in self.lists.skins.iter().enumerate() {
            if shown(s) {
                let (first, count) = self.skin_ranges[s.draw.skin % self.skin_ranges.len()];
                d.cmd_draw(cmd, count, 1, first, n.skin_base + k as u32);
            }
        }
    }

    /// Himmel, Welt-Boxen, feste und bewegliche Modelle, feste Figuren: gleich fuer alle Kameras.
    unsafe fn draw_solid(
        &self,
        cmd: vk::CommandBuffer,
        fi: usize,
        f: &FrameInput,
        n: Counts,
        (pass, pipes, groups): (Pass, &ScenePipelines, &[Group]),
    ) {
        let d = &self.ctx.device;
        if matches!(f.lighting, Lighting::Sun(_)) {
            self.bind(cmd, pipes.sky, fi, pass, false);
            d.cmd_draw(cmd, 3, 1, 0, 0);
        }
        let boxes = &self.visible_runs.boxes[pass as usize];
        if !boxes.is_empty() {
            self.bind(cmd, pipes.world, fi, pass, false);
            for g in boxes {
                d.cmd_draw(cmd, 36, g.count, 0, g.first_inst);
            }
        }
        let statics = &self.visible_runs.statics[pass as usize];
        if !statics.is_empty() {
            self.bind(cmd, pipes.mesh, fi, pass, true);
            self.draw_groups(cmd, statics);
        }
        self.bind(cmd, pipes.mesh, fi, pass, false);
        self.draw_groups(cmd, groups);
        self.bind(cmd, pipes.skin, fi, pass, false);
        self.draw_skins(cmd, n, |s| match pass {
            Pass::Main => s.kind == SkinKind::Solid && s.vis.camera,
            _ => s.vis.second,
        });
    }

    /// Halbtransparentes: Figuren-Hologramme, Modell-Hologramme, Partikel.
    unsafe fn draw_blended(&self, cmd: vk::CommandBuffer, fi: usize, n: Counts) {
        let d = &self.ctx.device;
        let pipes = &self.pipes.scene;
        self.bind(cmd, pipes.skin_ghost, fi, Pass::Main, false);
        self.draw_skins(cmd, n, |s| s.kind == SkinKind::Ghost && s.vis.camera);
        if !self.lists.ghost_groups.is_empty() {
            self.bind(cmd, pipes.ghost, fi, Pass::Main, false);
            self.draw_groups(cmd, &self.lists.ghost_groups);
        }
        if n.particles > 0 {
            self.bind(cmd, pipes.particle, fi, Pass::Main, false);
            d.cmd_bind_vertex_buffers(cmd, 0, &[self.particle_bufs[fi].buffer], &[0]);
            d.cmd_draw(cmd, 6, n.particles, 0, 0);
        }
    }

    /// Schattenkarte: Tiefe aus Sicht der Sonne.
    unsafe fn record_shadow(&self, cmd: vk::CommandBuffer, fi: usize, sun: &Sun, n: Counts) {
        let d = &self.ctx.device;
        let depth_stages = vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
            | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS;
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            depth_stages,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[to_depth_target(self.shadow.image)],
        );
        let extent = vk::Extent2D {
            width: SHADOW_SIZE,
            height: SHADOW_SIZE,
        };
        let att = vk::RenderingAttachmentInfo::default()
            .image_view(self.shadow.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(depth_clear());
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area(extent))
                .layer_count(1)
                .depth_attachment(&att),
        );
        if sun.shadow_strength > 0.0 {
            set_viewport(d, cmd, extent);
            let pass = Pass::Shadow;
            let statics = &self.visible_runs.statics[pass as usize];
            if !statics.is_empty() {
                self.bind(cmd, self.pipes.shadow, fi, pass, true);
                self.draw_groups(cmd, statics);
            }
            self.bind(cmd, self.pipes.shadow, fi, pass, false);
            self.draw_groups(cmd, &self.lists.shadow_groups);
            self.bind(cmd, self.pipes.shadow_skin, fi, pass, false);
            self.draw_skins(cmd, n, |s| s.vis.shadow);
        }
        d.cmd_end_rendering(cmd);
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier(
                self.shadow.image,
                vk::ImageAspectFlags::DEPTH,
                vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                vk::AccessFlags::SHADER_READ,
            )],
        );
    }

    /// Farbe und Tiefe als Ziel beginnen, mit der Hintergrundfarbe gefuellt.
    unsafe fn begin_scene(
        &self,
        cmd: vk::CommandBuffer,
        f: &FrameInput,
        extent: vk::Extent2D,
        (color, resolve, depth): (&Image, Option<&Image>, &Image),
    ) {
        let d = &self.ctx.device;
        let images = [
            to_depth_target(depth.image),
            to_color_target(color.image),
            to_color_target(resolve.unwrap_or(color).image),
        ];
        let used = if resolve.is_some() { 3 } else { 2 };
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::LATE_FRAGMENT_TESTS
                | vk::PipelineStageFlags::FRAGMENT_SHADER
                | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &images[..used],
        );
        let mut color_att = vk::RenderingAttachmentInfo::default()
            .image_view(color.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(background(f));
        if let Some(r) = resolve {
            color_att = color_att
                .store_op(vk::AttachmentStoreOp::DONT_CARE)
                .resolve_mode(vk::ResolveModeFlags::AVERAGE)
                .resolve_image_view(r.view)
                .resolve_image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
        }
        let depth_att = vk::RenderingAttachmentInfo::default()
            .image_view(depth.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::DONT_CARE)
            .clear_value(depth_clear());
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area(extent))
                .layer_count(1)
                .color_attachments(std::slice::from_ref(&color_att))
                .depth_attachment(&depth_att),
        );
        set_viewport(d, cmd, extent);
    }

    /// Zweite Kamera in ihr eigenes Bild (ohne Hologramme und Partikel).
    unsafe fn record_second(&self, cmd: vk::CommandBuffer, fi: usize, f: &FrameInput, n: Counts) {
        let d = &self.ctx.device;
        let s = &self.second;
        self.begin_scene(cmd, f, s.extent, (&s.color, None, &s.depth));
        if f.second_camera.is_some() {
            let groups = &self.lists.second_groups[..];
            let how = (Pass::Second, &self.pipes.second, groups);
            self.draw_solid(cmd, fi, f, n, how);
        }
        d.cmd_end_rendering(cmd);
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[to_readable(s.color.image)],
        );
    }

    /// Szene ins Offscreen-Bild.
    unsafe fn record_scene(&self, cmd: vk::CommandBuffer, fi: usize, f: &FrameInput, n: Counts) {
        let targets = match &self.ms_color {
            Some(ms) => (ms, Some(&self.color), &self.depth),
            None => (&self.color, None, &self.depth),
        };
        self.begin_scene(cmd, f, self.offscreen, targets);
        let groups = &self.lists.groups[..];
        let how = (Pass::Main, &self.pipes.scene, groups);
        self.draw_solid(cmd, fi, f, n, how);
        self.draw_blended(cmd, fi, n);
        self.ctx.device.cmd_end_rendering(cmd);
    }
}
