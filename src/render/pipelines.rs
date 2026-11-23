//! Alle Grafik-Pipelines des Renderers: Welt-Boxen, Modelle, Hologramme, Figuren (fest und
//! halbtransparent), UI und VHS-Nachbearbeitung.

use super::*;

/// Die fertigen Pipelines (werden in `Drop for Renderer` freigegeben).
pub(super) struct Pipelines {
    pub world: vk::Pipeline,
    pub mesh: vk::Pipeline,
    pub ghost: vk::Pipeline,
    pub skin: vk::Pipeline,
    pub skin_ghost: vk::Pipeline,
    pub ui: vk::Pipeline,
    pub post: vk::Pipeline,
}

/// Pipelines fuer die gegebenen Layouts erzeugen.
///
/// # Safety
/// `d` muss ein gueltiges Geraet sein; die Layouts muessen zu ihm gehoeren.
pub(super) unsafe fn create_pipelines(
    d: &ash::Device,
    scene_layout: vk::PipelineLayout,
    post_layout: vk::PipelineLayout,
    ui_layout: vk::PipelineLayout,
    swapchain_format: vk::Format,
) -> Result<Pipelines> {
    let world_pipeline = create_pipeline(
        d,
        scene_layout,
        &PipelineDesc {
            vs: spv!("world.vert"),
            fs: spv!("world.frag"),
            color_format: Some(COLOR_FORMAT),
            depth_format: Some(DEPTH_FORMAT),
            depth_test: true,
            depth_write: true,
            depth_compare: vk::CompareOp::LESS,
            blend: Blend::Opaque,
            samples: vk::SampleCountFlags::TYPE_1,
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
            color_format: Some(COLOR_FORMAT),
            depth_format: Some(DEPTH_FORMAT),
            depth_test: true,
            depth_write: true,
            depth_compare: vk::CompareOp::LESS,
            blend: Blend::Opaque,
            samples: vk::SampleCountFlags::TYPE_1,
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
            color_format: Some(COLOR_FORMAT),
            depth_format: Some(DEPTH_FORMAT),
            depth_test: true,
            depth_write: false,
            depth_compare: vk::CompareOp::LESS,
            blend: Blend::Ghost,
            samples: vk::SampleCountFlags::TYPE_1,
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
                color_format: Some(COLOR_FORMAT),
                depth_format: Some(DEPTH_FORMAT),
                depth_test: true,
                depth_write: !ghost,
                depth_compare: vk::CompareOp::LESS,
                blend: if ghost { Blend::Ghost } else { Blend::Opaque },
                samples: vk::SampleCountFlags::TYPE_1,
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
            color_format: Some(COLOR_FORMAT),
            depth_format: None,
            depth_test: false,
            depth_write: false,
            depth_compare: vk::CompareOp::LESS,
            blend: Blend::Alpha,
            samples: vk::SampleCountFlags::TYPE_1,
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
            color_format: Some(swapchain_format),
            depth_format: None,
            depth_test: false,
            depth_write: false,
            depth_compare: vk::CompareOp::LESS,
            blend: Blend::Opaque,
            samples: vk::SampleCountFlags::TYPE_1,
            bindings: &[],
            attributes: &[],
        },
    )?;
    Ok(Pipelines {
        world: world_pipeline,
        mesh: mesh_pipeline,
        ghost: ghost_pipeline,
        skin: skin_pipeline,
        skin_ghost: skin_ghost_pipeline,
        ui: ui_pipeline,
        post: post_pipeline,
    })
}
