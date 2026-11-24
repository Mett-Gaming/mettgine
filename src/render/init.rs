//! Aufbau des Renderers: Bilder, Befehlspuffer, Puffer.

use super::descriptors::FONT_MIPS;
use super::pipelines::create_pipelines;
use super::transfer::{halve_r8, one_shot, upload_mips, upload_r8};
use super::util::{image_barrier, ImageDesc};
use super::*;
use crate::gpu::MeshData;
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

/// Graustufen-Textur anlegen und hochladen, mit `mips` Verkleinerungsstufen.
unsafe fn r8_texture(
    ctx: &Context,
    pool: vk::CommandPool,
    (px, width, height): (&[u8], u32, u32),
    mips: u32,
) -> Result<Image> {
    // klein genug, dass auch die kleinste Stufe noch einen Bildpunkt hat
    let mips = mips.min(width.min(height).max(1).ilog2() + 1);
    let img = Image::with(
        ctx,
        &ImageDesc {
            width,
            height,
            format: vk::Format::R8_UNORM,
            usage: vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            aspect: vk::ImageAspectFlags::COLOR,
            samples: vk::SampleCountFlags::TYPE_1,
            mips,
        },
    )?;
    if mips == 1 {
        upload_r8(ctx, pool, &img, px, width, height)?;
        return Ok(img);
    }
    let mut small: Vec<(Vec<u8>, u32, u32)> = Vec::new();
    for _ in 1..mips {
        let next = match small.last() {
            Some((px, w, h)) => halve_r8(px, *w, *h),
            None => halve_r8(px, width, height),
        };
        small.push(next);
    }
    let mut levels = vec![(px, width, height)];
    levels.extend(small.iter().map(|(px, w, h)| (&px[..], *w, *h)));
    upload_mips(ctx, pool, &img, &levels)?;
    Ok(img)
}

/// Farbbild, in das gezeichnet und aus dem gelesen wird.
fn color_target(ctx: &Context, e: vk::Extent2D) -> Result<Image> {
    let usage = vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED;
    let aspect = vk::ImageAspectFlags::COLOR;
    Image::new(ctx, e.width, e.height, COLOR_FORMAT, usage, aspect)
}

/// Tiefenbild, `sampled` = auch lesbar (Schattenkarte).
fn depth_target(ctx: &Context, e: vk::Extent2D, sampled: bool) -> Result<Image> {
    let mut usage = vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT;
    if sampled {
        usage |= vk::ImageUsageFlags::SAMPLED;
    }
    let aspect = vk::ImageAspectFlags::DEPTH;
    Image::new(ctx, e.width, e.height, DEPTH_FORMAT, usage, aspect)
}

fn square(n: u32) -> vk::Extent2D {
    vk::Extent2D {
        width: n,
        height: n,
    }
}

/// Bilder einmal in einen lesbaren Zustand bringen (auch wenn ihr Pass nie laeuft).
unsafe fn make_readable(
    ctx: &Context,
    pool: vk::CommandPool,
    images: &[(&Image, vk::ImageAspectFlags)],
) -> Result<()> {
    let barriers: Vec<_> = images
        .iter()
        .map(|(i, aspect)| {
            image_barrier(
                i.image,
                *aspect,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::SHADER_READ,
            )
        })
        .collect();
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
    })
}

impl SecondCamera {
    fn new(ctx: &Context, size: Option<(u32, u32)>) -> Result<Self> {
        let extent = size.map_or(square(1), |(width, height)| vk::Extent2D { width, height });
        Ok(Self {
            color: color_target(ctx, extent)?,
            depth: depth_target(ctx, extent, false)?,
            extent,
            enabled: size.is_some(),
        })
    }
}

impl UiLayer {
    fn new(ctx: &Context, size: UiSize) -> Result<Self> {
        let (extent, enabled) = match size {
            UiSize::Fixed(width, height) => (vk::Extent2D { width, height }, true),
            UiSize::Height(_) => (square(1), false),
        };
        Ok(Self {
            image: color_target(ctx, extent)?,
            extent,
            enabled,
        })
    }
}

impl Renderer {
    /// Renderer mit Vorgaben: nur Modelle, keine Figuren, eingebaute Bitmap-Schrift (`ui`).
    pub fn simple(window: &Window, meshes: &[MeshData]) -> Result<Self> {
        let font = crate::ui::font_atlas();
        Self::new(window, &Config::new(meshes, &[], (&font, 128, 128)))
    }

    /// Baut den Renderer fuer `window`.
    pub fn new(window: &Window, cfg: &Config) -> Result<Self> {
        let size = window.inner_size();
        let window_size = (size.width.max(1), size.height.max(1));
        let ctx = Context::new(window, cfg.ray_tracing)?;
        let swapchain = Swapchain::new(&ctx, window_size, true, vk::SwapchainKHR::null())?;
        let offscreen = offscreen_extent(window_size, cfg.resolution);
        let samples = vk::SampleCountFlags::TYPE_1;
        let (color, depth, ms_color) = Self::make_targets(&ctx, offscreen, samples)?;
        let shadow = depth_target(&ctx, square(SHADOW_SIZE), true)?;
        let dummy_shadow = depth_target(&ctx, square(1), true)?;
        let second = SecondCamera::new(&ctx, cfg.second_camera)?;
        let ui_layer = UiLayer::new(&ctx, cfg.ui)?;
        let setup = SceneSetup {
            depth_compare: if cfg.last_wins {
                vk::CompareOp::LESS_OR_EQUAL
            } else {
                vk::CompareOp::LESS
            },
            world_vert: cfg.world.vert.to_vec(),
            world_frag: cfg.world.frag.to_vec(),
        };
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let uniform = vk::BufferUsageFlags::UNIFORM_BUFFER;
        let vertex = vk::BufferUsageFlags::VERTEX_BUFFER;
        let mesh_lists: Vec<&[_]> = cfg.meshes.iter().map(|m| &m.vertices[..]).collect();
        let (mesh_vertex_buf, mesh_ranges) = vertex_buffer(&ctx, &mesh_lists)?;
        let skin_lists: Vec<&[_]> = cfg.skins.iter().map(|s| &s.vertices[..]).collect();
        let (skin_vertex_buf, skin_ranges) = vertex_buffer(&ctx, &skin_lists)?;
        // SAFETY: alle Handles stammen von diesem Geraet; Aufbau laeuft vor dem ersten Bild.
        unsafe {
            let d = &ctx.device;
            let desc = Descriptors::new(d)?;
            let pipes = create_pipelines(&ctx.gpu(), &desc, swapchain.format, &setup)?;
            let (command_pool, frames) = present::create_frames(&ctx, FRAMES_IN_FLIGHT)?;
            let splash = Splash::new(&ctx, command_pool, swapchain.format, FRAMES_IN_FLIGHT)?;
            let timer = GpuTimer::new(&ctx, FRAMES_IN_FLIGHT)?;
            let font = r8_texture(&ctx, command_pool, cfg.font, FONT_MIPS)?;
            let signs = r8_texture(&ctx, command_pool, cfg.signs.unwrap_or((&[0], 1, 1)), 1)?;
            let (color_bit, depth_bit) = (vk::ImageAspectFlags::COLOR, vk::ImageAspectFlags::DEPTH);
            let readable = [
                (&shadow, depth_bit),
                (&dummy_shadow, depth_bit),
                (&second.color, color_bit),
                (&ui_layer.image, color_bit),
            ];
            make_readable(&ctx, command_pool, &readable)?;
            let renderer = Self {
                inst_bufs: per_frame(&ctx, MAX_INSTANCES * size_of::<GpuInst>(), storage)?,
                bone_bufs: per_frame(&ctx, MAX_BONES * 64, storage)?,
                globals_bufs: per_frame(&ctx, size_of::<Globals>(), uniform)?,
                second_globals: per_frame(&ctx, size_of::<Globals>(), uniform)?,
                ui_bufs: per_frame(&ctx, MAX_UI_VERTS * size_of::<UiVertex>(), vertex)?,
                particle_bufs: per_frame(
                    &ctx,
                    START_PARTICLES * size_of::<ParticleInst>(),
                    vertex,
                )?,
                box_buf: Buffer::new_host(&ctx, 64, storage)?,
                light_buf: Buffer::new_host(&ctx, 64, storage)?,
                static_buf: Buffer::new_host(&ctx, 128, storage)?,
                box_count: 0,
                light_count: 0,
                static_runs: Runs::default(),
                box_runs: Runs::default(),
                visible_runs: VisibleRuns::default(),
                cull_boxes: cfg.world.vert == WorldShaders::default().vert,
                boxes: Vec::new(),
                statics: Vec::new(),
                swapchain,
                color,
                depth,
                ms_color,
                samples,
                wanted_samples: samples,
                offscreen,
                resolution: cfg.resolution,
                ui_size: cfg.ui,
                second,
                ui_layer,
                shadow,
                dummy_shadow,
                font,
                signs,
                smooth: cfg.smooth,
                font_mipmaps: false,
                desc,
                setup,
                pipes,
                command_pool,
                frames,
                frame_index: 0,
                mesh_vertex_buf,
                mesh_ranges,
                skin_vertex_buf,
                skin_ranges,
                particles: Vec::new(),
                mesh_bounds: cfg
                    .meshes
                    .iter()
                    .map(|m| Sphere::of_mesh(&m.vertices))
                    .collect(),
                skin_bounds: cfg
                    .skins
                    .iter()
                    .map(|s| SkinBounds::new(&s.vertices))
                    .collect(),
                culling: true,
                lists: DrawLists::default(),
                timer,
                modern: None,
                splash,
                stats: FrameStats::default(),
                scratch_insts: Vec::with_capacity(1024),
                scratch_bones: Vec::with_capacity(1024),
                window_size,
                vsync: true,
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
        let color = color_target(ctx, ext)?;
        if samples == vk::SampleCountFlags::TYPE_1 {
            return Ok((color, depth_target(ctx, ext, false)?, None));
        }
        let make = |format, usage, aspect| {
            Image::with(
                ctx,
                &ImageDesc {
                    width: ext.width,
                    height: ext.height,
                    format,
                    usage,
                    aspect,
                    samples,
                    mips: 1,
                },
            )
        };
        let depth = make(
            DEPTH_FORMAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            vk::ImageAspectFlags::DEPTH,
        )?;
        let ms = make(
            COLOR_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT,
            vk::ImageAspectFlags::COLOR,
        )?;
        Ok((color, depth, Some(ms)))
    }
}
