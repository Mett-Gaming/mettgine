//! Pipeline-Layouts und Pipelines des Szenen-Renderers (Himmel, Modelle, Figuren, Schatten,
//! Nachbearbeitung, UI). Vertex-Layouts: `gpu::MeshVertex` (36 Byte), `gpu::SkinVertex`
//! (68 Byte), `render::UiVertex` (32 Byte), `particles::ParticleInst` (48 Byte, je Instanz).

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

const F3: vk::Format = vk::Format::R32G32B32_SFLOAT;
const F4: vk::Format = vk::Format::R32G32B32A32_SFLOAT;

fn mesh_attrs() -> [vk::VertexInputAttributeDescription; 3] {
    [attr(0, 0, F3), attr(1, 12, F3), attr(2, 24, F3)]
}

/// Farbe und Tiefe der Szene, deckend.
fn scene_desc<'a>(
    vs: &'a [u8],
    fs: &'a [u8],
    bindings: &'a [vk::VertexInputBindingDescription],
    attributes: &'a [vk::VertexInputAttributeDescription],
) -> PipelineDesc<'a> {
    PipelineDesc {
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
    }
}

/// Pipelines des Szenen-Passes fuer die gegebene Kantenglaettung.
pub(super) unsafe fn create_scene_pipelines(
    d: &ash::Device,
    layout: vk::PipelineLayout,
    samples: vk::SampleCountFlags,
) -> Result<ScenePipelines> {
    let skin_attrs = [
        attr(0, 0, F3),
        attr(1, 12, F3),
        attr(2, 24, F3),
        attr(3, 36, F4),
        attr(4, 52, F4),
    ];
    let particle_bind = [vk::VertexInputBindingDescription {
        binding: 0,
        stride: size_of::<ParticleInst>() as u32,
        input_rate: vk::VertexInputRate::INSTANCE,
    }];
    let particle_attrs = [attr(0, 0, F4), attr(1, 16, F4), attr(2, 32, F4)];
    let mesh_bind = vertex_binding(size_of::<MeshVertex>());
    let mesh_attrs = mesh_attrs();
    let skin_bind = vertex_binding(size_of::<SkinVertex>());
    let make = |desc: PipelineDesc| create_pipeline(d, layout, &PipelineDesc { samples, ..desc });
    Ok(ScenePipelines {
        sky: make(PipelineDesc {
            depth_test: false,
            depth_write: false,
            ..scene_desc(spv!("scene_sky.vert"), spv!("scene_sky.frag"), &[], &[])
        })?,
        mesh: make(scene_desc(
            spv!("scene_mesh.vert"),
            spv!("scene_mesh.frag"),
            &mesh_bind,
            &mesh_attrs,
        ))?,
        skin: make(scene_desc(
            spv!("scene_skin.vert"),
            spv!("scene_skin.frag"),
            &skin_bind,
            &skin_attrs,
        ))?,
        particle: make(PipelineDesc {
            depth_write: false,
            blend: Blend::Premultiplied,
            ..scene_desc(
                spv!("scene_particle.vert"),
                spv!("scene_particle.frag"),
                &particle_bind,
                &particle_attrs,
            )
        })?,
    })
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

    let mesh_bind = vertex_binding(size_of::<MeshVertex>());
    let mesh_attrs = mesh_attrs();
    let skin_bind = vertex_binding(size_of::<SkinVertex>());
    let skin_shadow_attrs = [attr(0, 0, F3), attr(3, 36, F4), attr(4, 52, F4)];
    let ui_bind = vertex_binding(size_of::<UiVertex>());
    let ui_attrs = [
        attr(0, 0, vk::Format::R32G32_SFLOAT),
        attr(1, 8, vk::Format::R32G32_SFLOAT),
        attr(2, 16, F4),
    ];

    let shadow = |vs, fs, bindings, attributes| PipelineDesc {
        color_format: None,
        ..scene_desc(vs, fs, bindings, attributes)
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

    Ok(Pipelines {
        scene: create_scene_pipelines(d, scene_layout, vk::SampleCountFlags::TYPE_1)?,
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
