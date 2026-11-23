//! Kleine Vulkan-Helfer des Szenen-Renderers: Pipeline bauen, Einmal-Befehle, Textur hochladen,
//! Viewport, BMP-Export.

use super::*;
use crate::render::util::image_barrier;

/// Beschreibung einer Grafik-Pipeline (Dynamic Rendering).
pub(super) struct PipelineDesc<'a> {
    pub vs: &'a [u8],
    pub fs: &'a [u8],
    /// None = nur Tiefe (Schattenkarte)
    pub color_format: Option<vk::Format>,
    pub depth_format: Option<vk::Format>,
    pub depth_test: bool,
    pub depth_write: bool,
    /// Alpha-Mischung (UI)
    pub blend: bool,
    pub bindings: &'a [vk::VertexInputBindingDescription],
    pub attributes: &'a [vk::VertexInputAttributeDescription],
}

/// 32-Bit-BMP schreiben (Pixel als BGRA bzw. RGBA, oben links zuerst).
pub(super) fn write_bmp(
    path: &std::path::Path,
    w: u32,
    h: u32,
    px: &[u8],
    rgba: bool,
) -> std::io::Result<()> {
    let row = (w * 4) as usize;
    let data_len = row * h as usize;
    let mut out = Vec::with_capacity(54 + data_len);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + data_len as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes()); // positiv = unten zuerst
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in (0..h as usize).rev() {
        for p in px[y * row..(y + 1) * row].chunks_exact(4) {
            if rgba {
                out.extend_from_slice(&[p[2], p[1], p[0], 255]);
            } else {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
    }
    std::fs::write(path, out)
}

/// Einmal-Befehlspuffer aufnehmen, abschicken und warten (nur beim Aufbau).
pub(super) unsafe fn one_shot(
    ctx: &Context,
    pool: vk::CommandPool,
    record: impl FnOnce(vk::CommandBuffer),
) -> Result<()> {
    let d = &ctx.device;
    let cmd = d.allocate_command_buffers(
        &vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1),
    )?[0];
    d.begin_command_buffer(
        cmd,
        &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
    )?;
    record(cmd);
    d.end_command_buffer(cmd)?;
    let cmds = [cmd];
    d.queue_submit(
        ctx.queue,
        &[vk::SubmitInfo::default().command_buffers(&cmds)],
        vk::Fence::null(),
    )?;
    d.queue_wait_idle(ctx.queue)?;
    d.free_command_buffers(pool, &cmds);
    Ok(())
}

/// Graustufen-Textur (R8) hochladen und fuer Shader lesbar machen.
pub(super) unsafe fn upload_r8(
    ctx: &Context,
    pool: vk::CommandPool,
    img: &Image,
    pixels: &[u8],
    width: u32,
    height: u32,
) -> Result<()> {
    let d = &ctx.device;
    let staging = Buffer::new_host(ctx, pixels.len() as u64, vk::BufferUsageFlags::TRANSFER_SRC)?;
    staging.write(pixels);
    let result = one_shot(ctx, pool, |cmd| {
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier(
                img.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::TRANSFER_WRITE,
            )],
        );
        d.cmd_copy_buffer_to_image(
            cmd,
            staging.buffer,
            img.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[vk::BufferImageCopy {
                buffer_offset: 0,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image_subresource: vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                image_offset: vk::Offset3D::default(),
                image_extent: vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                },
            }],
        );
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier(
                img.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::SHADER_READ,
            )],
        );
    });
    staging.destroy(d);
    result
}

pub(super) unsafe fn set_viewport(d: &ash::Device, cmd: vk::CommandBuffer, extent: vk::Extent2D) {
    d.cmd_set_viewport(
        cmd,
        0,
        &[vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: extent.width as f32,
            height: extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }],
    );
    d.cmd_set_scissor(
        cmd,
        0,
        &[vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent,
        }],
    );
}

pub(super) unsafe fn create_pipeline(
    d: &ash::Device,
    layout: vk::PipelineLayout,
    desc: &PipelineDesc,
) -> Result<vk::Pipeline> {
    let vs_code = ash::util::read_spv(&mut std::io::Cursor::new(desc.vs))?;
    let fs_code = ash::util::read_spv(&mut std::io::Cursor::new(desc.fs))?;
    let vs_mod =
        d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&vs_code), None)?;
    let fs_mod =
        d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&fs_code), None)?;
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vs_mod)
            .name(c"vs_main"),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fs_mod)
            .name(c"fs_main"),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(desc.bindings)
        .vertex_attribute_descriptions(desc.attributes);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let depth = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(desc.depth_test)
        .depth_write_enable(desc.depth_write)
        .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
    let mut blend_att = vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::RGBA);
    if desc.blend {
        blend_att = blend_att
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD);
    }
    let blend_attachments = [blend_att];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(
        if desc.color_format.is_some() {
            &blend_attachments[..]
        } else {
            &[]
        },
    );
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let color_formats: Vec<vk::Format> = desc.color_format.into_iter().collect();
    let mut rendering = vk::PipelineRenderingCreateInfo::default()
        .color_attachment_formats(&color_formats)
        .depth_attachment_format(desc.depth_format.unwrap_or(vk::Format::UNDEFINED));
    let info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport)
        .rasterization_state(&raster)
        .multisample_state(&multisample)
        .depth_stencil_state(&depth)
        .color_blend_state(&blend)
        .dynamic_state(&dynamic)
        .layout(layout)
        .push_next(&mut rendering);
    let result = d.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None);
    d.destroy_shader_module(vs_mod, None);
    d.destroy_shader_module(fs_mod, None);
    let pipelines = result.map_err(|(_, e)| e)?;
    Ok(pipelines[0])
}
