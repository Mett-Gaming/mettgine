//! Vulkan-Renderer.
//!
//! Pass 1 (480x270 Offscreen): statische Welt-Boxen, Meshes (Modelle + dynamische Boxen), UI.
//! Pass 2 (Swapchain): VHS-Post-Pass. Dynamic Rendering (Vulkan 1.3).

mod context;
mod frame;
mod pipeline;
mod swapchain;
mod util;

use crate::gpu::{GpuInstance, GpuLight, MeshData};
use ash::vk;
use bytemuck::{Pod, Zeroable};
use context::Context;
use glam::{Mat4, Vec3, Vec4};
use pipeline::{create_pipeline, set_viewport, upload_r8, write_bmp};
use std::mem::size_of;
use swapchain::Swapchain;
use util::{image_barrier, Buffer, Image};
use winit::window::Window;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub const RENDER_WIDTH: u32 = 480;
pub const RENDER_HEIGHT: u32 = 270;
const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;
const FRAMES_IN_FLIGHT: usize = 2;
const MAX_MESH_DRAWS: usize = 4096;
const MAX_UI_VERTS: usize = 60_000;
/// Knochen-Matrizen pro Frame (animierte Figuren)
const MAX_BONES: usize = 16_384;
/// Aufloesung des Ueberwachungsmonitors (zweite Kamera)
pub const MONITOR_WIDTH: u32 = 160;
pub const MONITOR_HEIGHT: u32 = 90;

macro_rules! spv {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".spv"))
    };
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WorldPush {
    view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    params: [f32; 4],
    fog_color: [f32; 4],
    /// Live-Verbiegen: seitlich, hoch, Gangbeginn z, Gangende z
    sway: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PostPush {
    params: [f32; 4],
    res: [f32; 4],
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuMeshInst {
    model: [[f32; 4]; 4],
    tint: [f32; 4],
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct UiVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// Ein Modell (oder eine dynamische Box) fuer diesen Frame.
#[derive(Clone, Copy)]
pub struct MeshDraw {
    pub mesh: usize,
    pub model: Mat4,
    /// rgb = Farbe (multipliziert), a = Eigenleuchten
    pub tint: Vec4,
}

/// Eine animierte Figur: Mesh `skin` (Index in den Figuren), Knochen ab `first_bone` im Knochen-Puffer.
#[derive(Clone, Copy, Debug)]
pub struct SkinDraw {
    pub skin: usize,
    pub model: Mat4,
    /// rgb = Farbmultiplikator, w = Eigenleuchten
    pub tint: Vec4,
    pub first_bone: u32,
    /// halbtransparent (Hologramm)
    pub ghost: bool,
    /// Zerreissen im Shader (0..1)
    pub distort: f32,
}

pub struct FrameParams {
    pub view_proj: Mat4,
    pub cam_pos: Vec3,
    pub time: f32,
    pub tipped: bool,
    pub fog_color: Vec3,
    pub fog_density: f32,
    pub glitch: f32,
    pub vhs: f32,
    pub fade: f32,
    pub red: f32,
    /// Verfall der Werbeposter (0..1)
    pub decay: f32,
    /// Live-Verbiegen der Gaenge (siehe world::sway)
    pub sway: Vec4,
    /// Farbumkehr (0..1) und Ueberbelichtung (0..1) beim Schreck
    pub invert: f32,
    pub flash: f32,
    /// Kamera des Ueberwachungsmonitors (view_proj, Position); None = Bild bleibt schwarz
    pub monitor: Option<(Mat4, Vec3)>,
}

#[derive(Clone, Copy)]
struct Frame {
    cmd: vk::CommandBuffer,
    image_available: vk::Semaphore,
    in_flight: vk::Fence,
}

struct PipelineDesc<'a> {
    vs: &'a [u8],
    fs: &'a [u8],
    color_format: vk::Format,
    depth_format: Option<vk::Format>,
    depth_test: bool,
    blend: bool,
    /// Halbtransparent (Hologramme): konstantes Alpha, schreibt keine Tiefe
    ghost: bool,
    bindings: &'a [vk::VertexInputBindingDescription],
    attributes: &'a [vk::VertexInputAttributeDescription],
}

pub struct Renderer {
    swapchain: Swapchain,
    color: Image,
    depth: Image,
    font: Image,
    ui_target: Image,
    /// Bild der Ueberwachungskamera und seine Tiefe
    monitor: Image,
    monitor_depth: Image,
    sampler: vk::Sampler,
    /// Schild-Atlas (Schriftzuege im Markt), linear gefiltert
    signs: Image,
    sign_sampler: vk::Sampler,
    scene_set_layout: vk::DescriptorSetLayout,
    tex_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    world_set: vk::DescriptorSet,
    /// wie world_set, aber ohne Monitor-Bild (fuer den Kamera-Pass, der es gerade beschreibt)
    world_set_cam: vk::DescriptorSet,
    mesh_sets: Vec<vk::DescriptorSet>,
    /// statische Mesh-Instanzen (Waren in den Regalen), nach Modell sortiert
    static_set: vk::DescriptorSet,
    static_buf: Buffer,
    static_ranges: Vec<(u32, u32, u32)>,
    post_set: vk::DescriptorSet,
    ui_set: vk::DescriptorSet,
    scene_layout: vk::PipelineLayout,
    post_layout: vk::PipelineLayout,
    ui_layout: vk::PipelineLayout,
    world_pipeline: vk::Pipeline,
    mesh_pipeline: vk::Pipeline,
    ghost_pipeline: vk::Pipeline,
    skin_pipeline: vk::Pipeline,
    skin_ghost_pipeline: vk::Pipeline,
    skin_vertex_buf: Buffer,
    skin_ranges: Vec<(u32, u32)>,
    /// Knochen-Matrizen je Frame im Flug
    bone_bufs: Vec<Buffer>,
    ui_pipeline: vk::Pipeline,
    post_pipeline: vk::Pipeline,
    command_pool: vk::CommandPool,
    frames: Vec<Frame>,
    frame_index: usize,
    instance_buf: Buffer,
    light_buf: Buffer,
    instance_count: u32,
    light_count: u32,
    mesh_vertex_buf: Buffer,
    mesh_ranges: Vec<(u32, u32)>,
    mesh_inst_bufs: Vec<Buffer>,
    ui_bufs: Vec<Buffer>,
    window_size: (u32, u32),
    needs_recreate: bool,
    capture: Option<std::path::PathBuf>,
    capture_buf: Option<Buffer>,
    // Muss zuletzt freigegeben werden (Drop-Reihenfolge der Felder).
    ctx: Context,
}

fn binding(
    b: u32,
    ty: vk::DescriptorType,
    stages: vk::ShaderStageFlags,
) -> vk::DescriptorSetLayoutBinding<'static> {
    vk::DescriptorSetLayoutBinding::default()
        .binding(b)
        .descriptor_type(ty)
        .descriptor_count(1)
        .stage_flags(stages)
}

impl Renderer {
    /// `font` ist ein 128x128-Graustufen-Atlas (16x16 Zeichen a 8x8 Pixel).
    /// `signs` = (R8-Pixel, Breite, Hoehe) des Schild-Atlas.
    pub fn new(
        window: &Window,
        meshes: &[MeshData],
        skins: &[crate::gpu::SkinData],
        font: &[u8],
        signs: (&[u8], u32, u32),
    ) -> Result<Self> {
        let size = window.inner_size();
        let ctx = Context::new(window)?;
        let swapchain = Swapchain::new(&ctx, size.width, size.height, vk::SwapchainKHR::null())?;
        let color = Image::new(
            &ctx,
            RENDER_WIDTH,
            RENDER_HEIGHT,
            COLOR_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            vk::ImageAspectFlags::COLOR,
        )?;
        let depth = Image::new(
            &ctx,
            RENDER_WIDTH,
            RENDER_HEIGHT,
            DEPTH_FORMAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            vk::ImageAspectFlags::DEPTH,
        )?;
        let font_img = Image::new(
            &ctx,
            128,
            128,
            vk::Format::R8_UNORM,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
        )?;
        let sign_img = Image::new(
            &ctx,
            signs.1,
            signs.2,
            vk::Format::R8_UNORM,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
        )?;
        // Eigene UI-Ebene: Text bekommt im VHS-Pass nur wenig Stoerung ab
        let ui_target = Image::new(
            &ctx,
            RENDER_WIDTH,
            RENDER_HEIGHT,
            COLOR_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            vk::ImageAspectFlags::COLOR,
        )?;
        let monitor = Image::new(
            &ctx,
            MONITOR_WIDTH,
            MONITOR_HEIGHT,
            COLOR_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            vk::ImageAspectFlags::COLOR,
        )?;
        let monitor_depth = Image::new(
            &ctx,
            MONITOR_WIDTH,
            MONITOR_HEIGHT,
            DEPTH_FORMAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            vk::ImageAspectFlags::DEPTH,
        )?;

        unsafe {
            let d = &ctx.device;
            let sampler = d.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::NEAREST)
                    .min_filter(vk::Filter::NEAREST)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .max_lod(0.0),
                None,
            )?;

            let sign_sampler = d.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .max_lod(0.0),
                None,
            )?;

            // Szene: binding 0 = Instanzen (Vertex), binding 1 = Lichter (Fragment), 2 = Schild-Atlas, 3 = Sampler
            let scene_bindings = [
                binding(
                    0,
                    vk::DescriptorType::STORAGE_BUFFER,
                    vk::ShaderStageFlags::VERTEX,
                ),
                binding(
                    1,
                    vk::DescriptorType::STORAGE_BUFFER,
                    vk::ShaderStageFlags::FRAGMENT,
                ),
                binding(
                    2,
                    vk::DescriptorType::SAMPLED_IMAGE,
                    vk::ShaderStageFlags::FRAGMENT,
                ),
                binding(
                    3,
                    vk::DescriptorType::SAMPLER,
                    vk::ShaderStageFlags::FRAGMENT,
                ),
                // 4 = Knochen-Matrizen der animierten Figuren
                binding(
                    4,
                    vk::DescriptorType::STORAGE_BUFFER,
                    vk::ShaderStageFlags::VERTEX,
                ),
                // 5 = Bild der Ueberwachungskamera (Monitor-Material)
                binding(
                    5,
                    vk::DescriptorType::SAMPLED_IMAGE,
                    vk::ShaderStageFlags::FRAGMENT,
                ),
            ];
            let scene_set_layout = d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&scene_bindings),
                None,
            )?;
            // Textur: binding 0 = Bild, binding 1 = Sampler, binding 2 = zweites Bild (UI-Ebene im Post-Pass)
            let tex_bindings = [
                binding(
                    0,
                    vk::DescriptorType::SAMPLED_IMAGE,
                    vk::ShaderStageFlags::FRAGMENT,
                ),
                binding(
                    1,
                    vk::DescriptorType::SAMPLER,
                    vk::ShaderStageFlags::FRAGMENT,
                ),
                binding(
                    2,
                    vk::DescriptorType::SAMPLED_IMAGE,
                    vk::ShaderStageFlags::FRAGMENT,
                ),
            ];
            let tex_set_layout = d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&tex_bindings),
                None,
            )?;

            // Welt + statische Waren + je Frame ein Set fuer dynamische Meshes
            let scene_sets = 3 + FRAMES_IN_FLIGHT as u32;
            let pool_sizes = [
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::STORAGE_BUFFER,
                    descriptor_count: 3 * scene_sets,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: 4 + 2 * scene_sets,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::SAMPLER,
                    descriptor_count: 2 + scene_sets,
                },
            ];
            let descriptor_pool = d.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(scene_sets + 2)
                    .pool_sizes(&pool_sizes),
                None,
            )?;
            let mut layouts = vec![scene_set_layout; scene_sets as usize];
            layouts.push(tex_set_layout);
            layouts.push(tex_set_layout);
            let sets = d.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(descriptor_pool)
                    .set_layouts(&layouts),
            )?;
            let world_set = sets[0];
            let static_set = sets[1];
            let world_set_cam = sets[2];
            let mesh_sets = sets[3..scene_sets as usize].to_vec();
            let post_set = sets[scene_sets as usize];
            let ui_set = sets[scene_sets as usize + 1];

            for (set, view, second) in [
                (post_set, color.view, ui_target.view),
                (ui_set, font_img.view, font_img.view),
            ] {
                let image_info = [vk::DescriptorImageInfo::default()
                    .image_view(view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                let second_info = [vk::DescriptorImageInfo::default()
                    .image_view(second)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                let sampler_info = [vk::DescriptorImageInfo::default().sampler(sampler)];
                d.update_descriptor_sets(
                    &[
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(0)
                            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                            .image_info(&image_info),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(1)
                            .descriptor_type(vk::DescriptorType::SAMPLER)
                            .image_info(&sampler_info),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(2)
                            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                            .image_info(&second_info),
                    ],
                    &[],
                );
            }

            let push = |stages, size: usize| {
                [vk::PushConstantRange {
                    stage_flags: stages,
                    offset: 0,
                    size: size as u32,
                }]
            };
            let scene_push = push(
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                size_of::<WorldPush>(),
            );
            let scene_layout = d.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[scene_set_layout])
                    .push_constant_ranges(&scene_push),
                None,
            )?;
            let post_push = push(vk::ShaderStageFlags::FRAGMENT, size_of::<PostPush>());
            let post_layout = d.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[tex_set_layout])
                    .push_constant_ranges(&post_push),
                None,
            )?;
            let ui_push = push(vk::ShaderStageFlags::VERTEX, 16);
            let ui_layout = d.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[tex_set_layout])
                    .push_constant_ranges(&ui_push),
                None,
            )?;

            let world_pipeline = create_pipeline(
                d,
                scene_layout,
                &PipelineDesc {
                    vs: spv!("world.vert"),
                    fs: spv!("world.frag"),
                    color_format: COLOR_FORMAT,
                    depth_format: Some(DEPTH_FORMAT),
                    depth_test: true,
                    blend: false,
                    ghost: false,
                    bindings: &[],
                    attributes: &[],
                },
            )?;
            let mesh_bindings = [vk::VertexInputBindingDescription {
                binding: 0,
                stride: 36,
                input_rate: vk::VertexInputRate::VERTEX,
            }];
            let attr = |location, offset, format| vk::VertexInputAttributeDescription {
                location,
                binding: 0,
                format,
                offset,
            };
            let mesh_attrs = [
                attr(0, 0, vk::Format::R32G32B32_SFLOAT),
                attr(1, 12, vk::Format::R32G32B32_SFLOAT),
                attr(2, 24, vk::Format::R32G32B32_SFLOAT),
            ];
            let mesh_pipeline = create_pipeline(
                d,
                scene_layout,
                &PipelineDesc {
                    vs: spv!("mesh.vert"),
                    fs: spv!("mesh.frag"),
                    color_format: COLOR_FORMAT,
                    depth_format: Some(DEPTH_FORMAT),
                    depth_test: true,
                    blend: false,
                    ghost: false,
                    bindings: &mesh_bindings,
                    attributes: &mesh_attrs,
                },
            )?;
            // Hologramme / Fata Morgana: gleiche Meshes, halbtransparent
            let ghost_pipeline = create_pipeline(
                d,
                scene_layout,
                &PipelineDesc {
                    vs: spv!("mesh.vert"),
                    fs: spv!("mesh.frag"),
                    color_format: COLOR_FORMAT,
                    depth_format: Some(DEPTH_FORMAT),
                    depth_test: true,
                    blend: false,
                    ghost: true,
                    bindings: &mesh_bindings,
                    attributes: &mesh_attrs,
                },
            )?;
            // Animierte Figuren: Skinning im Vertex-Shader
            let skin_bindings = [vk::VertexInputBindingDescription {
                binding: 0,
                stride: size_of::<crate::gpu::SkinVertex>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            }];
            let skin_attrs = [
                attr(0, 0, vk::Format::R32G32B32_SFLOAT),
                attr(1, 12, vk::Format::R32G32B32_SFLOAT),
                attr(2, 24, vk::Format::R32G32B32_SFLOAT),
                attr(3, 36, vk::Format::R32G32B32A32_SFLOAT),
                attr(4, 52, vk::Format::R32G32B32A32_SFLOAT),
            ];
            let mut skin_pipes = Vec::new();
            for ghost in [false, true] {
                skin_pipes.push(create_pipeline(
                    d,
                    scene_layout,
                    &PipelineDesc {
                        vs: spv!("skin.vert"),
                        fs: spv!("skin.frag"),
                        color_format: COLOR_FORMAT,
                        depth_format: Some(DEPTH_FORMAT),
                        depth_test: true,
                        blend: false,
                        ghost,
                        bindings: &skin_bindings,
                        attributes: &skin_attrs,
                    },
                )?);
            }
            let (skin_pipeline, skin_ghost_pipeline) = (skin_pipes[0], skin_pipes[1]);
            let ui_bindings = [vk::VertexInputBindingDescription {
                binding: 0,
                stride: size_of::<UiVertex>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            }];
            let ui_attrs = [
                attr(0, 0, vk::Format::R32G32_SFLOAT),
                attr(1, 8, vk::Format::R32G32_SFLOAT),
                attr(2, 16, vk::Format::R32G32B32A32_SFLOAT),
            ];
            let ui_pipeline = create_pipeline(
                d,
                ui_layout,
                &PipelineDesc {
                    vs: spv!("ui.vert"),
                    fs: spv!("ui.frag"),
                    color_format: COLOR_FORMAT,
                    depth_format: None,
                    depth_test: false,
                    blend: true,
                    ghost: false,
                    bindings: &ui_bindings,
                    attributes: &ui_attrs,
                },
            )?;
            let post_pipeline = create_pipeline(
                d,
                post_layout,
                &PipelineDesc {
                    vs: spv!("post.vert"),
                    fs: spv!("post.frag"),
                    color_format: swapchain.format,
                    depth_format: None,
                    depth_test: false,
                    blend: false,
                    ghost: false,
                    bindings: &[],
                    attributes: &[],
                },
            )?;

            let command_pool = d.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(ctx.queue_family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?;
            let cmds = d.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(FRAMES_IN_FLIGHT as u32),
            )?;
            let mut frames = Vec::with_capacity(FRAMES_IN_FLIGHT);
            for cmd in cmds {
                frames.push(Frame {
                    cmd,
                    image_available: d
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
                    in_flight: d.create_fence(
                        &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                        None,
                    )?,
                });
            }

            // Font-Atlas hochladen
            upload_r8(&ctx, command_pool, &font_img, font, 128, 128)?;
            upload_r8(&ctx, command_pool, &sign_img, signs.0, signs.1, signs.2)?;

            // Mesh-Vertices: alle Modelle hintereinander in einem Buffer
            let mut all = Vec::new();
            let mut mesh_ranges = Vec::new();
            for m in meshes {
                mesh_ranges.push((all.len() as u32, m.vertices.len() as u32));
                all.extend_from_slice(&m.vertices);
            }
            let vbytes: &[u8] = bytemuck::cast_slice(&all);
            let mesh_vertex_buf = Buffer::new_host(
                &ctx,
                vbytes.len().max(64) as u64,
                vk::BufferUsageFlags::VERTEX_BUFFER,
            )?;
            mesh_vertex_buf.write(vbytes);

            // Figuren-Vertices: alle Figuren hintereinander
            let mut skin_all: Vec<crate::gpu::SkinVertex> = Vec::new();
            let mut skin_ranges = Vec::new();
            for s in skins {
                skin_ranges.push((skin_all.len() as u32, s.vertices.len() as u32));
                skin_all.extend_from_slice(&s.vertices);
            }
            let sbytes: &[u8] = bytemuck::cast_slice(&skin_all);
            let skin_vertex_buf = Buffer::new_host(
                &ctx,
                sbytes.len().max(64) as u64,
                vk::BufferUsageFlags::VERTEX_BUFFER,
            )?;
            skin_vertex_buf.write(sbytes);

            let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
            let instance_buf = Buffer::new_host(&ctx, 64, storage)?;
            let static_buf = Buffer::new_host(&ctx, 128, storage)?;
            let light_buf = Buffer::new_host(&ctx, 64, storage)?;
            let mut mesh_inst_bufs = Vec::new();
            let mut bone_bufs = Vec::new();
            let mut ui_bufs = Vec::new();
            for _ in 0..FRAMES_IN_FLIGHT {
                bone_bufs.push(Buffer::new_host(&ctx, (MAX_BONES * 64) as u64, storage)?);
                mesh_inst_bufs.push(Buffer::new_host(
                    &ctx,
                    (MAX_MESH_DRAWS * size_of::<GpuMeshInst>()) as u64,
                    storage,
                )?);
                ui_bufs.push(Buffer::new_host(
                    &ctx,
                    (MAX_UI_VERTS * size_of::<UiVertex>()) as u64,
                    vk::BufferUsageFlags::VERTEX_BUFFER,
                )?);
            }

            let mut renderer = Self {
                swapchain,
                color,
                depth,
                font: font_img,
                ui_target,
                monitor,
                monitor_depth,
                sampler,
                signs: sign_img,
                sign_sampler,
                scene_set_layout,
                tex_set_layout,
                descriptor_pool,
                world_set,
                world_set_cam,
                mesh_sets,
                static_set,
                static_buf,
                static_ranges: Vec::new(),
                post_set,
                ui_set,
                scene_layout,
                post_layout,
                ui_layout,
                world_pipeline,
                mesh_pipeline,
                ghost_pipeline,
                skin_pipeline,
                skin_ghost_pipeline,
                skin_vertex_buf,
                skin_ranges,
                bone_bufs,
                ui_pipeline,
                post_pipeline,
                command_pool,
                frames,
                frame_index: 0,
                instance_buf,
                light_buf,
                instance_count: 0,
                light_count: 0,
                mesh_vertex_buf,
                mesh_ranges,
                mesh_inst_bufs,
                ui_bufs,
                window_size: (size.width, size.height),
                needs_recreate: false,
                capture: None,
                capture_buf: None,
                ctx,
            };
            renderer.write_scene_descriptors();
            Ok(renderer)
        }
    }

    /// Name der Grafikkarte samt Treiber.
    pub fn gpu_name(&self) -> &str {
        &self.ctx.gpu_name
    }

    /// Speichert das naechste fertige Bild (nach VHS-Pass) als BMP.
    pub fn capture_next(&mut self, path: std::path::PathBuf) {
        self.capture = Some(path);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.window_size = (width, height);
        self.needs_recreate = true;
    }

    /// Laedt die Boxen und Lichter der Welt hoch (wartet auf die GPU, nur bei Weltwechsel).
    pub fn upload_world(&mut self, instances: &[GpuInstance], lights: &[GpuLight]) -> Result<()> {
        let inst_bytes: &[u8] = bytemuck::cast_slice(instances);
        let light_bytes: &[u8] = bytemuck::cast_slice(lights);
        unsafe { self.ctx.device.device_wait_idle()? };

        let usage = vk::BufferUsageFlags::STORAGE_BUFFER;
        if inst_bytes.len() as u64 > self.instance_buf.size {
            let new = Buffer::new_host(
                &self.ctx,
                (inst_bytes.len() as u64).next_power_of_two(),
                usage,
            )?;
            std::mem::replace(&mut self.instance_buf, new).destroy(&self.ctx.device);
        }
        if light_bytes.len() as u64 > self.light_buf.size {
            let new = Buffer::new_host(
                &self.ctx,
                (light_bytes.len() as u64).next_power_of_two(),
                usage,
            )?;
            std::mem::replace(&mut self.light_buf, new).destroy(&self.ctx.device);
        }
        self.instance_buf.write(inst_bytes);
        self.light_buf.write(light_bytes);
        self.instance_count = instances.len() as u32;
        self.light_count = lights.len() as u32;
        self.write_scene_descriptors();
        Ok(())
    }

    /// Statische Mesh-Instanzen (Waren). Werden nach Modell sortiert und pro Modell mit
    /// einem einzigen instanzierten Draw-Call gezeichnet.
    pub fn upload_statics(&mut self, statics: &[MeshDraw]) -> Result<()> {
        let mut sorted: Vec<&MeshDraw> = statics.iter().collect();
        sorted.sort_by_key(|m| m.mesh);
        let insts: Vec<GpuMeshInst> = sorted
            .iter()
            .map(|m| GpuMeshInst {
                model: m.model.to_cols_array_2d(),
                tint: [m.tint.x, m.tint.y, m.tint.z, 1.0],
                extra: [m.tint.w, 0.0, 1.0, 0.0],
            })
            .collect();
        let mut ranges: Vec<(u32, u32, u32)> = Vec::new();
        for (i, m) in sorted.iter().enumerate() {
            match ranges.last_mut() {
                Some(r) if r.0 == m.mesh as u32 => r.2 += 1,
                _ => ranges.push((m.mesh as u32, i as u32, 1)),
            }
        }
        let bytes: &[u8] = bytemuck::cast_slice(&insts);
        unsafe { self.ctx.device.device_wait_idle()? };
        if bytes.len() as u64 > self.static_buf.size {
            let new = Buffer::new_host(
                &self.ctx,
                (bytes.len() as u64).next_power_of_two(),
                vk::BufferUsageFlags::STORAGE_BUFFER,
            )?;
            std::mem::replace(&mut self.static_buf, new).destroy(&self.ctx.device);
        }
        self.static_buf.write(bytes);
        self.static_ranges = ranges;
        self.write_scene_descriptors();
        Ok(())
    }

    fn write_scene_descriptors(&mut self) {
        let light = [vk::DescriptorBufferInfo::default()
            .buffer(self.light_buf.buffer)
            .range(vk::WHOLE_SIZE)];
        // (Set, Instanzen, Knochen, Monitor-Bild an binding 5)
        let mut targets = vec![
            (
                self.world_set,
                self.instance_buf.buffer,
                self.bone_bufs[0].buffer,
                true,
            ),
            (
                self.world_set_cam,
                self.instance_buf.buffer,
                self.bone_bufs[0].buffer,
                false,
            ),
            (
                self.static_set,
                self.static_buf.buffer,
                self.bone_bufs[0].buffer,
                true,
            ),
        ];
        for (i, &s) in self.mesh_sets.iter().enumerate() {
            targets.push((
                s,
                self.mesh_inst_bufs[i].buffer,
                self.bone_bufs[i].buffer,
                true,
            ));
        }
        let sign_info = [vk::DescriptorImageInfo::default()
            .image_view(self.signs.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let sign_sampler = [vk::DescriptorImageInfo::default().sampler(self.sign_sampler)];
        let monitor_info = [vk::DescriptorImageInfo::default()
            .image_view(self.monitor.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        // Platzhalter fuer den Kamera-Pass: der Schild-Atlas (wird dort nie abgetastet)
        let no_monitor = [vk::DescriptorImageInfo::default()
            .image_view(self.signs.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        for (set, buf, bones, with_monitor) in targets {
            let inst = [vk::DescriptorBufferInfo::default()
                .buffer(buf)
                .range(vk::WHOLE_SIZE)];
            let bone_info = [vk::DescriptorBufferInfo::default()
                .buffer(bones)
                .range(vk::WHOLE_SIZE)];
            unsafe {
                self.ctx.device.update_descriptor_sets(
                    &[
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(0)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .buffer_info(&inst),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(1)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .buffer_info(&light),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(2)
                            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                            .image_info(&sign_info),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(3)
                            .descriptor_type(vk::DescriptorType::SAMPLER)
                            .image_info(&sign_sampler),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(4)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .buffer_info(&bone_info),
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(5)
                            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                            .image_info(if with_monitor {
                                &monitor_info
                            } else {
                                &no_monitor
                            }),
                    ],
                    &[],
                );
            }
        }
    }

    fn recreate_swapchain(&mut self) -> Result<()> {
        let (w, h) = self.window_size;
        if w == 0 || h == 0 {
            return Ok(());
        }
        unsafe { self.ctx.device.device_wait_idle()? };
        let new = Swapchain::new(&self.ctx, w, h, self.swapchain.handle)?;
        std::mem::replace(&mut self.swapchain, new).destroy(&self.ctx);
        self.needs_recreate = false;
        Ok(())
    }

    /// `ghosts` werden halbtransparent ueber die Szene gelegt (Hologramme).
    /// `monitor_skins` erscheinen nur auf dem Ueberwachungsmonitor.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        p: &FrameParams,
        meshes: &[MeshDraw],
        ghosts: &[MeshDraw],
        skins: &[SkinDraw],
        monitor_skins: &[SkinDraw],
        bones: &[Mat4],
        ui: &[UiVertex],
    ) -> Result<()> {
        if self.window_size.0 == 0 || self.window_size.1 == 0 {
            return Ok(()); // minimiert
        }
        if self.needs_recreate {
            self.recreate_swapchain()?;
        }
        let fi = self.frame_index;
        let frame = self.frames[fi];
        unsafe {
            let d = &self.ctx.device;
            d.wait_for_fences(&[frame.in_flight], true, u64::MAX)?;
            let image_index = match self.ctx.swapchain_loader.acquire_next_image(
                self.swapchain.handle,
                u64::MAX,
                frame.image_available,
                vk::Fence::null(),
            ) {
                Ok((i, _)) => i,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.needs_recreate = true;
                    return Ok(());
                }
                Err(e) => return Err(e.into()),
            };
            d.reset_fences(&[frame.in_flight])?;

            // Per-Frame-Daten schreiben (der Fence garantiert, dass die GPU damit fertig ist)
            let mesh_count = meshes.len().min(MAX_MESH_DRAWS);
            let ghost_count = ghosts.len().min(MAX_MESH_DRAWS - mesh_count);
            let mut insts: Vec<GpuMeshInst> = meshes[..mesh_count]
                .iter()
                .chain(ghosts[..ghost_count].iter())
                .map(|m| GpuMeshInst {
                    model: m.model.to_cols_array_2d(),
                    tint: [m.tint.x, m.tint.y, m.tint.z, 1.0],
                    extra: [m.tint.w, 0.0, 0.0, 0.0],
                })
                .collect();
            // Figuren dahinter: erst undurchsichtige, dann Hologramme; extra.y = erster Knochen
            let bone_count = bones.len().min(MAX_BONES);
            let room = MAX_MESH_DRAWS - insts.len();
            let mut solid_skins: Vec<SkinDraw> =
                skins.iter().filter(|s| !s.ghost).copied().collect();
            let mut ghost_skins: Vec<SkinDraw> =
                skins.iter().filter(|s| s.ghost).copied().collect();
            solid_skins.truncate(room);
            ghost_skins.truncate(room - solid_skins.len());
            let mut mon_skins: Vec<SkinDraw> = monitor_skins.to_vec();
            mon_skins.truncate(room - solid_skins.len() - ghost_skins.len());
            for s in solid_skins
                .iter()
                .chain(ghost_skins.iter())
                .chain(mon_skins.iter())
            {
                insts.push(GpuMeshInst {
                    model: s.model.to_cols_array_2d(),
                    tint: [s.tint.x, s.tint.y, s.tint.z, 1.0],
                    extra: [s.tint.w, s.first_bone as f32, s.distort, 0.0],
                });
            }
            let bone_data: Vec<[[f32; 4]; 4]> = bones[..bone_count]
                .iter()
                .map(|m| m.to_cols_array_2d())
                .collect();
            self.bone_bufs[fi].write(bytemuck::cast_slice(&bone_data));
            self.mesh_inst_bufs[fi].write(bytemuck::cast_slice(&insts));
            let ui_count = ui.len().min(MAX_UI_VERTS) / 3 * 3;
            self.ui_bufs[fi].write(bytemuck::cast_slice(&ui[..ui_count]));

            let ext = self.swapchain.extent;
            let cap_bytes = (ext.width * ext.height * 4) as u64;
            let capturing = self.capture.is_some() && self.swapchain.can_capture;
            if capturing
                && self
                    .capture_buf
                    .as_ref()
                    .map_or(true, |b| b.size < cap_bytes)
            {
                if let Some(old) = self.capture_buf.take() {
                    old.destroy(&self.ctx.device);
                }
                self.capture_buf = Some(Buffer::new_host(
                    &self.ctx,
                    cap_bytes,
                    vk::BufferUsageFlags::TRANSFER_DST,
                )?);
            }
            let cap_target = if capturing {
                self.capture_buf.as_ref().map(|b| b.buffer)
            } else {
                None
            };

            self.record(
                frame.cmd,
                fi,
                image_index as usize,
                p,
                &meshes[..mesh_count],
                &ghosts[..ghost_count],
                &solid_skins,
                &ghost_skins,
                &mon_skins,
                ui_count as u32,
                cap_target,
            )?;

            let wait = [frame.image_available];
            let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            let cmds = [frame.cmd];
            let signal = [self.swapchain.render_finished[image_index as usize]];
            d.queue_submit(
                self.ctx.queue,
                &[vk::SubmitInfo::default()
                    .wait_semaphores(&wait)
                    .wait_dst_stage_mask(&stages)
                    .command_buffers(&cmds)
                    .signal_semaphores(&signal)],
                frame.in_flight,
            )?;

            let swapchains = [self.swapchain.handle];
            let indices = [image_index];
            match self.ctx.swapchain_loader.queue_present(
                self.ctx.queue,
                &vk::PresentInfoKHR::default()
                    .wait_semaphores(&signal)
                    .swapchains(&swapchains)
                    .image_indices(&indices),
            ) {
                Ok(false) => {}
                Ok(true) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.needs_recreate = true,
                Err(e) => return Err(e.into()),
            }
            if capturing {
                self.ctx.device.device_wait_idle()?;
                if let (Some(path), Some(buf)) = (self.capture.take(), &self.capture_buf) {
                    let px = buf.read(cap_bytes as usize);
                    let rgba = self.swapchain.format == vk::Format::R8G8B8A8_UNORM;
                    if let Err(e) = write_bmp(&path, ext.width, ext.height, &px, rgba) {
                        crate::crash::log(&format!("Screenshot fehlgeschlagen: {e}"));
                    }
                }
            }
        }
        self.frame_index = (self.frame_index + 1) % FRAMES_IN_FLIGHT;
        Ok(())
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let d = &self.ctx.device;
            let _ = d.device_wait_idle();
            for f in &self.frames {
                d.destroy_semaphore(f.image_available, None);
                d.destroy_fence(f.in_flight, None);
            }
            d.destroy_command_pool(self.command_pool, None);
            self.instance_buf.destroy(d);
            self.static_buf.destroy(d);
            self.light_buf.destroy(d);
            self.mesh_vertex_buf.destroy(d);
            self.skin_vertex_buf.destroy(d);
            for b in &self.bone_bufs {
                b.destroy(d);
            }
            if let Some(b) = &self.capture_buf {
                b.destroy(d);
            }
            for b in self.mesh_inst_bufs.iter().chain(self.ui_bufs.iter()) {
                b.destroy(d);
            }
            for p in [
                self.world_pipeline,
                self.mesh_pipeline,
                self.ghost_pipeline,
                self.skin_pipeline,
                self.skin_ghost_pipeline,
                self.ui_pipeline,
                self.post_pipeline,
            ] {
                d.destroy_pipeline(p, None);
            }
            for l in [self.scene_layout, self.post_layout, self.ui_layout] {
                d.destroy_pipeline_layout(l, None);
            }
            d.destroy_descriptor_pool(self.descriptor_pool, None);
            d.destroy_descriptor_set_layout(self.scene_set_layout, None);
            d.destroy_descriptor_set_layout(self.tex_set_layout, None);
            d.destroy_sampler(self.sampler, None);
            d.destroy_sampler(self.sign_sampler, None);
            self.signs.destroy(d);
            self.color.destroy(d);
            self.depth.destroy(d);
            self.font.destroy(d);
            self.ui_target.destroy(d);
            self.monitor.destroy(d);
            self.monitor_depth.destroy(d);
            self.swapchain.destroy(&self.ctx);
        }
    }
}
