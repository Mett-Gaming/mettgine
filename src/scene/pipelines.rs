//! Pipeline-Layouts und Pipelines des Szenen-Renderers (Himmel, Modelle, Figuren, Schatten,
//! Nachbearbeitung, UI). Vertex-Layouts: `gpu::MeshVertex` (36 Byte), `gpu::SkinVertex`
//! (68 Byte), `render::UiVertex` (32 Byte).

use super::*;
use crate::gpu::{MeshVertex, SkinVertex};
use crate::render::pipeline::{create_pipeline, Blend, PipelineDesc};
use std::mem::size_of;

macro_rules! spv {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".spv"))
    };
}

/// Push-Constants: Nachbearbeitung und UI je 16 Byte (ein vec4).
const PUSH_BYTES: u32 = 16;

fn attr(location: u32, offset: u32, format: vk::Format) -> vk::VertexInputAttributeDescription {
    vk::VertexInputAttributeDescription {
        location,
        binding: 0,
        format,
        offset,
    }
}

fn vertex_binding(stride: usize) -> [vk::VertexInputBindingDescription; 1] {
    [vk::VertexInputBindingDescription {
        binding: 0,
        stride: stride as u32,
        input_rate: vk::VertexInputRate::VERTEX,
    }]
}

unsafe fn layout(
    d: &ash::Device,
    set: vk::DescriptorSetLayout,
    push: Option<vk::ShaderStageFlags>,
) -> Result<vk::PipelineLayout> {
    let ranges: Vec<vk::PushConstantRange> = push
        .map(|stage_flags| vk::PushConstantRange {
            stage_flags,
            offset: 0,
            size: PUSH_BYTES,
        })
        .into_iter()
        .collect();
    Ok(d.create_pipeline_layout(
        &vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&[set])
            .push_constant_ranges(&ranges),
        None,
    )?)
}

pub(super) unsafe fn create_pipelines(
    d: &ash::Device,
    scene_set: vk::DescriptorSetLayout,
    tex_set: vk::DescriptorSetLayout,
    swapchain_format: vk::Format,
) -> Result<Pipelines> {
    let scene_layout = layout(d, scene_set, None)?;
    let post_layout = layout(d, tex_set, Some(vk::ShaderStageFlags::FRAGMENT))?;
    let ui_layout = layout(d, tex_set, Some(vk::ShaderStageFlags::VERTEX))?;

    let f3 = vk::Format::R32G32B32_SFLOAT;
    let f4 = vk::Format::R32G32B32A32_SFLOAT;
    let mesh_bind = vertex_binding(size_of::<MeshVertex>());
    let mesh_attrs = [attr(0, 0, f3), attr(1, 12, f3), attr(2, 24, f3)];
    let skin_bind = vertex_binding(size_of::<SkinVertex>());
    let skin_attrs = [
        attr(0, 0, f3),
        attr(1, 12, f3),
        attr(2, 24, f3),
        attr(3, 36, f4),
        attr(4, 52, f4),
    ];
    let skin_shadow_attrs = [attr(0, 0, f3), attr(3, 36, f4), attr(4, 52, f4)];
    let ui_bind = vertex_binding(size_of::<UiVertex>());
    let ui_attrs = [
        attr(0, 0, vk::Format::R32G32_SFLOAT),
        attr(1, 8, vk::Format::R32G32_SFLOAT),
        attr(2, 16, f4),
    ];

    let scene = |vs, fs, bindings, attributes| PipelineDesc {
        vs,
        fs,
        color_format: Some(COLOR_FORMAT),
        depth_format: Some(DEPTH_FORMAT),
        depth_test: true,
        depth_write: true,
        depth_compare: vk::CompareOp::LESS_OR_EQUAL,
        blend: Blend::Opaque,
        samples: vk::SampleCountFlags::TYPE_1,
        bindings,
        attributes,
    };
    let shadow = |vs, fs, bindings, attributes| PipelineDesc {
        color_format: None,
        ..scene(vs, fs, bindings, attributes)
    };
    let screen = |vs, fs, blend, bindings, attributes| PipelineDesc {
        vs,
        fs,
        color_format: Some(swapchain_format),
        depth_format: None,
        depth_test: false,
        depth_write: false,
        depth_compare: vk::CompareOp::LESS_OR_EQUAL,
        blend,
        samples: vk::SampleCountFlags::TYPE_1,
        bindings,
        attributes,
    };

    let sky_desc = PipelineDesc {
        depth_test: false,
        depth_write: false,
        ..scene(spv!("scene_sky.vert"), spv!("scene_sky.frag"), &[], &[])
    };
    Ok(Pipelines {
        sky: create_pipeline(d, scene_layout, &sky_desc)?,
        mesh: create_pipeline(
            d,
            scene_layout,
            &scene(
                spv!("scene_mesh.vert"),
                spv!("scene_mesh.frag"),
                &mesh_bind,
                &mesh_attrs,
            ),
        )?,
        skin: create_pipeline(
            d,
            scene_layout,
            &scene(
                spv!("scene_skin.vert"),
                spv!("scene_skin.frag"),
                &skin_bind,
                &skin_attrs,
            ),
        )?,
        shadow: create_pipeline(
            d,
            scene_layout,
            &shadow(
                spv!("scene_shadow.vert"),
                spv!("scene_shadow.frag"),
                &mesh_bind,
                &mesh_attrs[..1],
            ),
        )?,
        shadow_skin: create_pipeline(
            d,
            scene_layout,
            &shadow(
                spv!("scene_shadow_skin.vert"),
                spv!("scene_shadow_skin.frag"),
                &skin_bind,
                &skin_shadow_attrs,
            ),
        )?,
        ui: create_pipeline(
            d,
            ui_layout,
            &screen(
                spv!("scene_ui.vert"),
                spv!("scene_ui.frag"),
                Blend::Alpha,
                &ui_bind,
                &ui_attrs,
            ),
        )?,
        post: create_pipeline(
            d,
            post_layout,
            &screen(
                spv!("scene_post.vert"),
                spv!("scene_post.frag"),
                Blend::Opaque,
                &[],
                &[],
            ),
        )?,
        scene_layout,
        post_layout,
        ui_layout,
    })
}
