//! Renderer aufbauen: Bilder, Sampler, Descriptor-Sets, Layouts, Pipelines, Befehlspuffer,
//! Vertex- und Instanzpuffer.

use super::pipelines::{create_pipelines, Pipelines};
use super::*;

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

            let Pipelines {
                world: world_pipeline,
                mesh: mesh_pipeline,
                ghost: ghost_pipeline,
                skin: skin_pipeline,
                skin_ghost: skin_ghost_pipeline,
                ui: ui_pipeline,
                post: post_pipeline,
            } = create_pipelines(d, scene_layout, post_layout, ui_layout, swapchain.format)?;

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
}
