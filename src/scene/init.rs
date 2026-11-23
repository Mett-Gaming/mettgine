//! Aufbau des Szenen-Renderers: Bilder, Sampler, Descriptor-Sets, Befehlspuffer, Puffer.

use super::helpers::{one_shot, upload_r8};
use super::pipelines::create_pipelines;
use super::*;
use crate::gpu::{MeshData, SkinData};
use crate::render::util::image_barrier;
use std::mem::size_of;
use winit::window::Window;

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

/// Sampler, Set-Layouts, Pool und Sets.
struct Descriptors {
    linear: vk::Sampler,
    shadow_sampler: vk::Sampler,
    scene_set_layout: vk::DescriptorSetLayout,
    tex_set_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    main_sets: Vec<vk::DescriptorSet>,
    shadow_sets: Vec<vk::DescriptorSet>,
    post_set: vk::DescriptorSet,
    ui_set: vk::DescriptorSet,
}

unsafe fn create_descriptors(d: &ash::Device) -> Result<Descriptors> {
    let clamp = vk::SamplerAddressMode::CLAMP_TO_EDGE;
    let linear = d.create_sampler(
        &vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(clamp)
            .address_mode_v(clamp)
            .address_mode_w(clamp)
            .max_lod(0.0),
        None,
    )?;
    // Vergleichs-Sampler: die Hardware filtert die Schattenkante (PCF)
    let border = vk::SamplerAddressMode::CLAMP_TO_BORDER;
    let shadow_sampler = d.create_sampler(
        &vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(border)
            .address_mode_v(border)
            .address_mode_w(border)
            .border_color(vk::BorderColor::FLOAT_OPAQUE_WHITE)
            .compare_enable(true)
            .compare_op(vk::CompareOp::LESS_OR_EQUAL)
            .max_lod(0.0),
        None,
    )?;
    let vf = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
    let scene_bindings = [
        binding(0, vk::DescriptorType::STORAGE_BUFFER, vf),
        binding(1, vk::DescriptorType::STORAGE_BUFFER, vf),
        binding(2, vk::DescriptorType::UNIFORM_BUFFER, vf),
        binding(3, vk::DescriptorType::SAMPLED_IMAGE, vf),
        binding(4, vk::DescriptorType::SAMPLER, vf),
    ];
    let scene_set_layout = d.create_descriptor_set_layout(
        &vk::DescriptorSetLayoutCreateInfo::default().bindings(&scene_bindings),
        None,
    )?;
    let frag = vk::ShaderStageFlags::FRAGMENT;
    let tex_bindings = [
        binding(0, vk::DescriptorType::SAMPLED_IMAGE, frag),
        binding(1, vk::DescriptorType::SAMPLER, frag),
    ];
    let tex_set_layout = d.create_descriptor_set_layout(
        &vk::DescriptorSetLayoutCreateInfo::default().bindings(&tex_bindings),
        None,
    )?;
    // je Bild im Flug: ein Set fuer die Szene, eins fuer den Schatten-Pass; dazu Post und UI
    let scene_sets = 2 * FRAMES_IN_FLIGHT as u32;
    let size = |ty, descriptor_count| vk::DescriptorPoolSize {
        ty,
        descriptor_count,
    };
    let pool_sizes = [
        size(vk::DescriptorType::STORAGE_BUFFER, 2 * scene_sets),
        size(vk::DescriptorType::UNIFORM_BUFFER, scene_sets),
        size(vk::DescriptorType::SAMPLED_IMAGE, scene_sets + 2),
        size(vk::DescriptorType::SAMPLER, scene_sets + 2),
    ];
    let pool = d.create_descriptor_pool(
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
            .descriptor_pool(pool)
            .set_layouts(&layouts),
    )?;
    Ok(Descriptors {
        linear,
        shadow_sampler,
        scene_set_layout,
        tex_set_layout,
        pool,
        main_sets: sets[0..FRAMES_IN_FLIGHT].to_vec(),
        shadow_sets: sets[FRAMES_IN_FLIGHT..2 * FRAMES_IN_FLIGHT].to_vec(),
        post_set: sets[scene_sets as usize],
        ui_set: sets[scene_sets as usize + 1],
    })
}

unsafe fn create_frames(ctx: &Context) -> Result<(vk::CommandPool, Vec<Frame>)> {
    let d = &ctx.device;
    let pool = d.create_command_pool(
        &vk::CommandPoolCreateInfo::default()
            .queue_family_index(ctx.queue_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
        None,
    )?;
    let cmds = d.allocate_command_buffers(
        &vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAMES_IN_FLIGHT as u32),
    )?;
    let mut frames = Vec::with_capacity(FRAMES_IN_FLIGHT);
    for cmd in cmds {
        frames.push(Frame {
            cmd,
            image_available: d.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
            in_flight: d.create_fence(
                &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )?,
        });
    }
    Ok((pool, frames))
}

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
            let (command_pool, frames) = create_frames(&ctx)?;
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
                capture: None,
                capture_buf: None,
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

    fn write_scene_descriptors(&self) {
        for fi in 0..FRAMES_IN_FLIGHT {
            for (set, shadow_view) in [
                (self.main_sets[fi], self.shadow.view),
                (self.shadow_sets[fi], self.dummy_shadow.view),
            ] {
                let buf = |b: &Buffer| {
                    [vk::DescriptorBufferInfo::default()
                        .buffer(b.buffer)
                        .range(vk::WHOLE_SIZE)]
                };
                let (inst, bones, globals) = (
                    buf(&self.inst_bufs[fi]),
                    buf(&self.bone_bufs[fi]),
                    buf(&self.globals_bufs[fi]),
                );
                let shadow = [vk::DescriptorImageInfo::default()
                    .image_view(shadow_view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                let samp = [vk::DescriptorImageInfo::default().sampler(self.shadow_sampler)];
                let w = |b: u32, ty| {
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(b)
                        .descriptor_type(ty)
                };
                let writes = [
                    w(0, vk::DescriptorType::STORAGE_BUFFER).buffer_info(&inst),
                    w(1, vk::DescriptorType::STORAGE_BUFFER).buffer_info(&bones),
                    w(2, vk::DescriptorType::UNIFORM_BUFFER).buffer_info(&globals),
                    w(3, vk::DescriptorType::SAMPLED_IMAGE).image_info(&shadow),
                    w(4, vk::DescriptorType::SAMPLER).image_info(&samp),
                ];
                // SAFETY: Sets und Ressourcen gehoeren diesem Geraet und sind nicht in Benutzung.
                unsafe { self.ctx.device.update_descriptor_sets(&writes, &[]) };
            }
        }
    }

    /// Post-Pass liest das Offscreen-Bild, UI den Schrift-Atlas (neu nach Groessenwechsel).
    pub(super) fn write_tex_descriptors(&self) {
        for (set, view) in [
            (self.post_set, self.color.view),
            (self.ui_set, self.font.view),
        ] {
            let image_info = [vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let sampler_info = [vk::DescriptorImageInfo::default().sampler(self.linear)];
            let writes = [
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
            ];
            // SAFETY: wie oben; aufgerufen nur beim Aufbau und nach device_wait_idle.
            unsafe { self.ctx.device.update_descriptor_sets(&writes, &[]) };
        }
    }
}
