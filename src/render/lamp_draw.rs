//! Was Lampenschatten wirft: stehende Welt (nur veraltete Felder) und Bewegtes.

use super::descriptors::Pass;
use super::frame::{depth_clear, Counts};
use super::lamp_pass::{DEPTH_ACCESS, DEPTH_STAGES};
use super::lists::skin_sphere;
use super::util::image_barrier;
use super::*;
use crate::cull::Frustum;
use glam::Vec4;

unsafe fn viewport(d: &ash::Device, cmd: vk::CommandBuffer, a: vk::Rect2D) {
    d.cmd_set_viewport(
        cmd,
        0,
        &[vk::Viewport {
            x: a.offset.x as f32,
            y: a.offset.y as f32,
            width: a.extent.width as f32,
            height: a.extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }],
    );
    d.cmd_set_scissor(cmd, 0, &[a]);
}

impl Renderer {
    unsafe fn bind_lamp(&self, cmd: vk::CommandBuffer, pipe: vk::Pipeline, fi: usize, fixed: bool) {
        let d = &self.ctx.device;
        let gfx = vk::PipelineBindPoint::GRAPHICS;
        let set = self.desc.frames[fi][Pass::Shadow as usize][fixed as usize];
        d.cmd_bind_pipeline(cmd, gfx, pipe);
        d.cmd_bind_descriptor_sets(cmd, gfx, self.lamp.pipes.layout, 0, &[set], &[]);
    }

    unsafe fn push_tile(&self, cmd: vk::CommandBuffer, m: &Mat4) {
        self.ctx.device.cmd_push_constants(
            cmd,
            self.lamp.pipes.layout,
            vk::ShaderStageFlags::VERTEX,
            0,
            bytemuck::cast_slice(&m.to_cols_array()),
        );
    }

    /// Stehende Welt: Boxen und feste Modelle in die veralteten Felder.
    pub(super) unsafe fn draw_fixed(&mut self, cmd: vk::CommandBuffer, fi: usize, sway: Vec4) {
        let todo: Vec<(usize, Mat4, Sphere)> = self
            .lamp
            .lamps
            .active()
            .filter(|(_, s)| s.redraw)
            .map(|(i, s)| (i, s.view_proj, s.sphere))
            .collect();
        if todo.is_empty() {
            return;
        }
        use vk::{AccessFlags as A, ImageLayout as L, PipelineStageFlags as S};
        let mut runs = std::mem::take(&mut self.lamp.runs);
        let l = &self.lamp;
        let d = &self.ctx.device;
        let old = if l.empty {
            L::UNDEFINED
        } else {
            L::TRANSFER_SRC_OPTIMAL
        };
        let depth = vk::ImageAspectFlags::DEPTH;
        let none = vk::DependencyFlags::empty();
        let to_draw = image_barrier(
            l.fixed.image,
            depth,
            old,
            L::DEPTH_ATTACHMENT_OPTIMAL,
            A::TRANSFER_READ,
            DEPTH_ACCESS,
        );
        d.cmd_pipeline_barrier(cmd, S::TRANSFER, DEPTH_STAGES, none, &[], &[], &[to_draw]);
        let load = if l.empty {
            vk::AttachmentLoadOp::CLEAR
        } else {
            vk::AttachmentLoadOp::LOAD
        };
        self.begin_depth(cmd, &l.fixed, load);
        let grow = sway.x.abs() + sway.y.abs();
        for (slot, vp, lamp) in &todo {
            let a = l.area(*slot);
            viewport(d, cmd, a);
            let clear = vk::ClearAttachment {
                aspect_mask: depth,
                color_attachment: 0,
                clear_value: depth_clear(),
            };
            let rect = vk::ClearRect {
                rect: a,
                base_array_layer: 0,
                layer_count: 1,
            };
            d.cmd_clear_attachments(cmd, &[clear], &[rect]);
            let frustum = Frustum::new(vp);
            self.box_runs.lit(lamp, &frustum, grow, &mut runs);
            if !runs.is_empty() {
                self.bind_lamp(cmd, l.pipes.boxes, fi, true);
                self.push_tile(cmd, vp);
                for g in &runs {
                    d.cmd_draw(cmd, 36, g.count, 0, g.first_inst);
                }
            }
            self.static_runs.lit(lamp, &frustum, grow, &mut runs);
            if !runs.is_empty() {
                self.bind_lamp(cmd, l.pipes.mesh, fi, true);
                self.push_tile(cmd, vp);
                self.draw_groups(cmd, &runs);
            }
        }
        d.cmd_end_rendering(cmd);
        let to_copy = image_barrier(
            l.fixed.image,
            depth,
            L::DEPTH_ATTACHMENT_OPTIMAL,
            L::TRANSFER_SRC_OPTIMAL,
            A::DEPTH_STENCIL_ATTACHMENT_WRITE,
            A::TRANSFER_READ,
        );
        let late = S::LATE_FRAGMENT_TESTS;
        d.cmd_pipeline_barrier(cmd, late, S::TRANSFER, none, &[], &[], &[to_copy]);
        self.lamp.empty = false;
        self.lamp.runs = runs;
    }

    /// Bewegte Modelle und Figuren.
    pub(super) unsafe fn draw_moving(
        &self,
        cmd: vk::CommandBuffer,
        fi: usize,
        f: &FrameInput,
        n: Counts,
    ) {
        let l = &self.lamp;
        let d = &self.ctx.device;
        let spheres: Vec<_> = self
            .lists
            .skins
            .iter()
            .map(|s| skin_sphere(&s.draw, &self.skin_bounds, f.bones))
            .collect();
        for (slot, s) in l.lamps.active().filter(|(_, s)| s.strength > 0.0) {
            viewport(d, cmd, l.area(slot));
            if !self.lists.shadow_groups.is_empty() {
                self.bind_lamp(cmd, l.pipes.mesh, fi, false);
                self.push_tile(cmd, &s.view_proj);
                self.draw_groups(cmd, &self.lists.shadow_groups);
            }
            let frustum = Frustum::new(&s.view_proj);
            let seen: Vec<usize> = self
                .lists
                .skins
                .iter()
                .enumerate()
                .filter(|(k, slot)| {
                    slot.vis.shadow && spheres[*k].as_ref().is_none_or(|s| frustum.sees(s))
                })
                .map(|(k, _)| k)
                .collect();
            if seen.is_empty() || self.skin_ranges.is_empty() {
                continue;
            }
            self.bind_lamp(cmd, l.pipes.skin, fi, false);
            self.push_tile(cmd, &s.view_proj);
            d.cmd_bind_vertex_buffers(cmd, 0, &[self.skin_vertex_buf.buffer], &[0]);
            for k in seen {
                let skin = self.lists.skins[k].draw.skin % self.skin_ranges.len();
                let (first, count) = self.skin_ranges[skin];
                d.cmd_draw(cmd, count, 1, first, n.skin_base + k as u32);
            }
        }
    }
}
