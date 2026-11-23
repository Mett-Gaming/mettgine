//! Aufbau des Szenen-Renderers: Bilder, Sampler, Descriptor-Sets, Befehlspuffer, Puffer.

use super::descriptors::create_descriptors;
use super::pipelines::create_pipelines;
use super::*;
use crate::gpu::{MeshData, SkinData};
use crate::render::transfer::{one_shot, upload_r8};
use crate::render::util::image_barrier;
use std::mem::size_of;
use winit::window::Window;

/// Alle Vertices hintereinander in einen Puffer; liefert (Start, Anzahl) je Eintrag.
fn vertex_buffer<T: Pod>(ctx: &Context, items: &[&[T]]) -> Result<(Buffer, Vec<(u32, u32)>)> {
    let mut all: Vec<T> = Vec::new();
    let mut ranges = Vec::with_capacity(items.len());
    for v in items {
        ranges.push((all.len() as u32, v.len() as u32));
        all.extend_from_slice(v);
    }
    let bytes: &[u8] = bytemuck::cast_slice(&all);
    let buf = Buffer::new_host(
        ctx,
        bytes.len().max(64) as u64,
        vk::BufferUsageFlags::VERTEX_BUFFER,
    )?;
    buf.write(bytes);
    Ok((buf, ranges))
}

fn per_frame(ctx: &Context, bytes: usize, usage: vk::BufferUsageFlags) -> Result<Vec<Buffer>> {
    (0..FRAMES_IN_FLIGHT)
        .map(|_| Buffer::new_host(ctx, bytes as u64, usage))
        .collect()
}

impl SceneRenderer {
    /// Baut den Renderer fuer `window`. `meshes` und `skins` sind alle Modelle und Figuren
    /// (Index = `MeshDraw::mesh` bzw. `SkinDraw::skin`), `font` = (R8-Pixel, Breite, Hoehe)
    /// des Schrift-Atlas, `render_scale` = Szenen-Aufloesung relativ zum Fenster.
    pub fn new(
        window: &Window,
        meshes: &[MeshData],
        skins: &[SkinData],
        font: (&[u8], u32, u32),
        render_scale: f32,
    ) -> Result<Self> {
        let size = window.inner_size();
        let window_size = (size.width.max(1), size.height.max(1));
        let ctx = Context::new(window)?;
        let swapchain =
            Swapchain::new(&ctx, window_size.0, window_size.1, vk::SwapchainKHR::null())?;
        let render_scale = render_scale.clamp(0.25, 2.0);
        let offscreen = offscreen_extent(window_size, render_scale);
        let (color, depth) = Self::make_targets(&ctx, offscreen)?;
        let depth_sampled =
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED;
        let shadow = Image::new(
            &ctx,
            SHADOW_SIZE,
            SHADOW_SIZE,
            DEPTH_FORMAT,
            depth_sampled,
            vk::ImageAspectFlags::DEPTH,
        )?;
        let dummy_shadow = Image::new(
            &ctx,
            1,
            1,
            DEPTH_FORMAT,
            depth_sampled,
            vk::ImageAspectFlags::DEPTH,
        )?;
        let font_img = Image::new(
            &ctx,
            font.1,
            font.2,
            vk::Format::R8_UNORM,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
        )?;
        // SAFETY: alle Handles stammen von diesem Geraet; Aufbau laeuft vor dem ersten Bild.
        unsafe {
            let d = &ctx.device;
            let desc = create_descriptors(d)?;
            let pipes = create_pipelines(
                d,
                desc.scene_set_layout,
                desc.tex_set_layout,
                swapchain.format,
            )?;
            let (command_pool, frames) = present::create_frames(&ctx, FRAMES_IN_FLIGHT)?;
            upload_r8(&ctx, command_pool, &font_img, font.0, font.1, font.2)?;
            // Platzhalter-Tiefe einmal in einen lesbaren Zustand bringen
            one_shot(&ctx, command_pool, |cmd| {
                d.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[image_barrier(
                        dummy_shadow.image,
                        vk::ImageAspectFlags::DEPTH,
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        vk::AccessFlags::empty(),
                        vk::AccessFlags::SHADER_READ,
                    )],
                );
            })?;
            let mesh_lists: Vec<&[_]> = meshes.iter().map(|m| &m.vertices[..]).collect();
            let (mesh_vertex_buf, mesh_ranges) = vertex_buffer(&ctx, &mesh_lists)?;
            let skin_lists: Vec<&[_]> = skins.iter().map(|s| &s.vertices[..]).collect();
            let (skin_vertex_buf, skin_ranges) = vertex_buffer(&ctx, &skin_lists)?;
            let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
            let inst_bufs = per_frame(&ctx, MAX_INSTANCES * size_of::<GpuInst>(), storage)?;
            let bone_bufs = per_frame(&ctx, MAX_BONES * 64, storage)?;
            let globals_bufs = per_frame(
                &ctx,
                size_of::<Globals>(),
                vk::BufferUsageFlags::UNIFORM_BUFFER,
            )?;
            let ui_bufs = per_frame(
                &ctx,
                MAX_UI_VERTS * size_of::<UiVertex>(),
                vk::BufferUsageFlags::VERTEX_BUFFER,
            )?;
            let renderer = Self {
                swapchain,
                color,
                depth,
                offscreen,
                render_scale,
                shadow,
                dummy_shadow,
                font: font_img,
                linear: desc.linear,
                shadow_sampler: desc.shadow_sampler,
                scene_set_layout: desc.scene_set_layout,
                tex_set_layout: desc.tex_set_layout,
                descriptor_pool: desc.pool,
                main_sets: desc.main_sets,
                shadow_sets: desc.shadow_sets,
                post_set: desc.post_set,
                ui_set: desc.ui_set,
                pipes,
                command_pool,
                frames,
                frame_index: 0,
                mesh_vertex_buf,
                mesh_ranges,
                skin_vertex_buf,
                skin_ranges,
                inst_bufs,
                bone_bufs,
                globals_bufs,
                ui_bufs,
                scratch_insts: Vec::with_capacity(1024),
                scratch_bones: Vec::with_capacity(1024),
                window_size,
                needs_recreate: false,
                capture: Capture::default(),
                ctx,
            };
            renderer.write_scene_descriptors();
            renderer.write_tex_descriptors();
            Ok(renderer)
        }
    }

    pub(super) fn make_targets(ctx: &Context, ext: vk::Extent2D) -> Result<(Image, Image)> {
        let color = Image::new(
            ctx,
            ext.width,
            ext.height,
            COLOR_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            vk::ImageAspectFlags::COLOR,
        )?;
        let depth = Image::new(
            ctx,
            ext.width,
            ext.height,
            DEPTH_FORMAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            vk::ImageAspectFlags::DEPTH,
        )?;
        Ok((color, depth))
    }
}
