//! Aufbau des Szenen-Renderers: Bilder, Sampler, Descriptor-Sets, Befehlspuffer, Puffer.

use super::descriptors::{create_descriptors, FONT_MIPS};
use super::pipelines::create_pipelines;
use super::*;
use crate::gpu::{MeshData, SkinData};
use crate::render::transfer::{halve_r8, one_shot, upload_mips};
use crate::render::util::{image_barrier, ImageDesc};
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

/// Schrift-Atlas samt Verkleinerungsstufen hochladen.
unsafe fn upload_font(
    ctx: &Context,
    pool: vk::CommandPool,
    img: &Image,
    font: (&[u8], u32, u32),
    mips: u32,
) -> Result<()> {
    let mut small: Vec<(Vec<u8>, u32, u32)> = Vec::new();
    for _ in 1..mips {
        let next = match small.last() {
            Some((px, w, h)) => halve_r8(px, *w, *h),
            None => halve_r8(font.0, font.1, font.2),
        };
        small.push(next);
    }
    let mut levels = vec![font];
    levels.extend(small.iter().map(|(px, w, h)| (&px[..], *w, *h)));
    upload_mips(ctx, pool, img, &levels)
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
        let samples = vk::SampleCountFlags::TYPE_1;
        let (color, depth, ms_color) = Self::make_targets(&ctx, offscreen, samples)?;
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
        // klein genug, dass auch die kleinste Stufe noch einen Bildpunkt hat
        let mips = FONT_MIPS.min(font.1.min(font.2).max(1).ilog2() + 1);
        let font_img = Image::with(
            &ctx,
            &ImageDesc {
                width: font.1,
                height: font.2,
                format: vk::Format::R8_UNORM,
                usage: vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                aspect: vk::ImageAspectFlags::COLOR,
                samples,
                mips,
            },
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
            let splash = Splash::new(&ctx, command_pool, swapchain.format, FRAMES_IN_FLIGHT)?;
            let timer = GpuTimer::new(&ctx, FRAMES_IN_FLIGHT)?;
            upload_font(&ctx, command_pool, &font_img, font, mips)?;
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
            let particle_bufs = per_frame(
                &ctx,
                START_PARTICLES * size_of::<ParticleInst>(),
                vk::BufferUsageFlags::VERTEX_BUFFER,
            )?;
            let renderer = Self {
                swapchain,
                color,
                depth,
                ms_color,
                samples,
                wanted_samples: samples,
                offscreen,
                render_scale,
                shadow,
                dummy_shadow,
                font: font_img,
                linear: desc.linear,
                linear_mips: desc.linear_mips,
                font_mipmaps: false,
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
                particle_bufs,
                particles: Vec::new(),
                mesh_bounds: meshes
                    .iter()
                    .map(|m| Sphere::of_mesh(&m.vertices))
                    .collect(),
                skin_bounds: skins.iter().map(|s| SkinBounds::new(&s.vertices)).collect(),
                culling: true,
                lists: DrawLists::default(),
                timer,
                splash,
                stats: FrameStats::default(),
                scratch_insts: Vec::with_capacity(1024),
                scratch_bones: Vec::with_capacity(1024),
                window_size,
                needs_recreate: false,
                capture: Capture::default(),
                ctx,
            };
            for fi in 0..FRAMES_IN_FLIGHT {
                renderer.write_scene_descriptors(fi);
            }
            renderer.write_tex_descriptors();
            Ok(renderer)
        }
    }

    /// Zielbilder der Szene: Farbe, Tiefe und (mit Kantenglaettung) mehrfach abgetastete Farbe.
    pub(super) fn make_targets(
        ctx: &Context,
        ext: vk::Extent2D,
        samples: vk::SampleCountFlags,
    ) -> Result<(Image, Image, Option<Image>)> {
        let color_desc = ImageDesc {
            width: ext.width,
            height: ext.height,
            format: COLOR_FORMAT,
            usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            aspect: vk::ImageAspectFlags::COLOR,
            samples: vk::SampleCountFlags::TYPE_1,
            mips: 1,
        };
        let color = Image::with(ctx, &color_desc)?;
        let depth = Image::with(
            ctx,
            &ImageDesc {
                format: DEPTH_FORMAT,
                usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                aspect: vk::ImageAspectFlags::DEPTH,
                samples,
                ..color_desc
            },
        )?;
        let ms_color = if samples == vk::SampleCountFlags::TYPE_1 {
            None
        } else {
            Some(Image::with(
                ctx,
                &ImageDesc {
                    usage: vk::ImageUsageFlags::COLOR_ATTACHMENT,
                    samples,
                    ..color_desc
                },
            )?)
        };
        Ok((color, depth, ms_color))
    }
}
