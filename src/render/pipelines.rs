//! Pipeline-Layouts und Pipelines. Vertex-Layouts: `gpu::MeshVertex` (36 Byte),
//! `gpu::SkinVertex` (68 Byte), [`UiVertex`] (32 Byte), `particles::ParticleInst` (48 Byte, je
//! Instanz). Welt-Boxen brauchen keine Vertices (Wuerfel aus dem Vertex-Index).

use super::pipeline::{create_pipeline, Blend, PipelineDesc};
use super::*;
use crate::gpu::{MeshVertex, SkinVertex};
use std::mem::size_of;

macro_rules! spv {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".spv"))
    };
}
pub(super) use spv;

/// Push-Constants von Nachbearbeitung (drei vec4) und UI (ein vec4)
pub(super) const PUSH_BYTES: u32 = 48;

/// Shader der Welt-Boxen als SPIR-V (Einstiege `vs_main` und `fs_main`).
///
/// Damit bringt ein Spiel eigene Materialien mit. Schnittstelle: `shaders/_common.wgsl` und
/// `shaders/_world_base.wgsl` einbinden und `fs_main(in: VsOut)` schreiben; uebersetzt wird im
/// `build.rs` des Spiels mit naga, wie im `build.rs` der Engine. Die Bindings (Set 0, 0 bis 9)
/// muessen gleich bleiben.
#[derive(Clone, Copy)]
pub struct WorldShaders<'a> {
    pub vert: &'a [u8],
    pub frag: &'a [u8],
}

impl Default for WorldShaders<'_> {
    fn default() -> Self {
        Self {
            vert: spv!("world.vert"),
            frag: spv!("world.frag"),
        }
    }
}

/// Pipelines der Szene (haengen von der Kantenglaettung ab).
pub(super) struct ScenePipelines {
    pub sky: vk::Pipeline,
    pub world: vk::Pipeline,
    pub mesh: vk::Pipeline,
    pub ghost: vk::Pipeline,
    pub skin: vk::Pipeline,
    pub skin_ghost: vk::Pipeline,
    pub particle: vk::Pipeline,
}

impl ScenePipelines {
    pub fn all(&self) -> [vk::Pipeline; 7] {
        [
            self.sky,
            self.world,
            self.mesh,
            self.ghost,
            self.skin,
            self.skin_ghost,
            self.particle,
        ]
    }
}

/// Alle Pipelines und Layouts.
pub(super) struct Pipelines {
    pub scene_layout: vk::PipelineLayout,
    /// Nachbearbeitung und UI
    pub tex_layout: vk::PipelineLayout,
    pub scene: ScenePipelines,
    /// zweite Kamera: ohne Kantenglaettung
    pub second: ScenePipelines,
    pub shadow: vk::Pipeline,
    pub shadow_skin: vk::Pipeline,
    /// UI aufs Fenster und in die eigene Ebene
    pub ui: vk::Pipeline,
    pub ui_layer: vk::Pipeline,
    pub post_clean: vk::Pipeline,
    pub post_vhs: vk::Pipeline,
    pub post_plain: vk::Pipeline,
}

impl Pipelines {
    pub unsafe fn destroy(&self, d: &ash::Device) {
        let single = [
            self.shadow,
            self.shadow_skin,
            self.ui,
            self.ui_layer,
            self.post_clean,
            self.post_vhs,
            self.post_plain,
        ];
        let scenes = self.scene.all().into_iter().chain(self.second.all());
        for p in scenes.chain(single) {
            d.destroy_pipeline(p, None);
        }
        d.destroy_pipeline_layout(self.scene_layout, None);
        d.destroy_pipeline_layout(self.tex_layout, None);
    }
}

pub(super) fn attr(
    location: u32,
    offset: u32,
    format: vk::Format,
) -> vk::VertexInputAttributeDescription {
    vk::VertexInputAttributeDescription {
        location,
        binding: 0,
        format,
        offset,
    }
}

pub(super) fn vertex_binding(stride: usize) -> [vk::VertexInputBindingDescription; 1] {
    [vk::VertexInputBindingDescription {
        binding: 0,
        stride: stride as u32,
        input_rate: vk::VertexInputRate::VERTEX,
    }]
}

unsafe fn layout(
    d: &ash::Device,
    set: vk::DescriptorSetLayout,
    push: &[vk::PushConstantRange],
) -> Result<vk::PipelineLayout> {
    Ok(d.create_pipeline_layout(
        &vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&[set])
            .push_constant_ranges(push),
        None,
    )?)
}

pub(super) const F3: vk::Format = vk::Format::R32G32B32_SFLOAT;
pub(super) const F4: vk::Format = vk::Format::R32G32B32A32_SFLOAT;

pub(super) fn mesh_attrs() -> [vk::VertexInputAttributeDescription; 3] {
    [attr(0, 0, F3), attr(1, 12, F3), attr(2, 24, F3)]
}

/// Farbe und Tiefe der Szene, deckend.
pub(super) fn scene_desc<'a>(
    vs: &'a [u8],
    fs: &'a [u8],
    bindings: &'a [vk::VertexInputBindingDescription],
    attributes: &'a [vk::VertexInputAttributeDescription],
) -> PipelineDesc<'a> {
    PipelineDesc {
        vs,
        fs,
        vs_entry: c"vs_main",
        fs_entry: c"fs_main",
        color_format: Some(COLOR_FORMAT),
        more_colors: &[],
        depth_format: Some(DEPTH_FORMAT),
        depth_test: true,
        depth_write: true,
        depth_compare: vk::CompareOp::LESS_OR_EQUAL,
        blend: Blend::Opaque,
        samples: vk::SampleCountFlags::TYPE_1,
        bindings,
        attributes,
        dynamic_cull: false,
    }
}

/// Was die Szenen-Pipelines einer Renderer-Instanz bestimmt (fuer den Neubau aufgehoben).
pub(super) struct SceneSetup {
    pub depth_compare: vk::CompareOp,
    pub world_vert: Vec<u8>,
    pub world_frag: Vec<u8>,
}

/// Pipelines der Szene fuer die gegebene Kantenglaettung.
pub(super) unsafe fn create_scene_pipelines(
    d: &ash::Device,
    layout: vk::PipelineLayout,
    samples: vk::SampleCountFlags,
    setup: &SceneSetup,
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
    let make = |desc: PipelineDesc| {
        let desc = PipelineDesc {
            samples,
            depth_compare: setup.depth_compare,
            ..desc
        };
        create_pipeline(d, layout, &desc)
    };
    let mesh = || {
        scene_desc(
            spv!("mesh.vert"),
            spv!("mesh.frag"),
            &mesh_bind,
            &mesh_attrs,
        )
    };
    let skin = || {
        scene_desc(
            spv!("skin.vert"),
            spv!("skin.frag"),
            &skin_bind,
            &skin_attrs,
        )
    };
    Ok(ScenePipelines {
        sky: make(PipelineDesc {
            depth_test: false,
            depth_write: false,
            ..scene_desc(spv!("sky.vert"), spv!("sky.frag"), &[], &[])
        })?,
        world: make(scene_desc(&setup.world_vert, &setup.world_frag, &[], &[]))?,
        mesh: make(mesh())?,
        ghost: make(PipelineDesc {
            depth_write: false,
            blend: Blend::Ghost,
            ..mesh()
        })?,
        skin: make(skin())?,
        skin_ghost: make(PipelineDesc {
            depth_write: false,
            blend: Blend::Ghost,
            ..skin()
        })?,
        particle: make(PipelineDesc {
            depth_write: false,
            blend: Blend::Premultiplied,
            ..scene_desc(
                spv!("particle.vert"),
                spv!("particle.frag"),
                &particle_bind,
                &particle_attrs,
            )
        })?,
    })
}

/// Bildschirm-Pipelines: Nachbearbeitung und UI.
pub(super) fn screen_desc<'a>(
    vs: &'a [u8],
    fs: &'a [u8],
    format: vk::Format,
    blend: Blend,
    bindings: &'a [vk::VertexInputBindingDescription],
    attributes: &'a [vk::VertexInputAttributeDescription],
) -> PipelineDesc<'a> {
    PipelineDesc {
        color_format: Some(format),
        depth_format: None,
        depth_test: false,
        depth_write: false,
        blend,
        ..scene_desc(vs, fs, bindings, attributes)
    }
}

pub(super) unsafe fn create_pipelines(
    d: &ash::Device,
    desc: &Descriptors,
    swapchain_format: vk::Format,
    setup: &SceneSetup,
) -> Result<Pipelines> {
    let scene_layout = layout(d, desc.scene_layout, &[])?;
    let push = vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
        offset: 0,
        size: PUSH_BYTES,
    };
    let tex_layout = layout(d, desc.tex_layout, &[push])?;
    let mesh_bind = vertex_binding(size_of::<MeshVertex>());
    let mesh_attrs = mesh_attrs();
    let skin_bind = vertex_binding(size_of::<SkinVertex>());
    let skin_shadow_attrs = [attr(0, 0, F3), attr(3, 36, F4), attr(4, 52, F4)];
    let ui_bind = vertex_binding(size_of::<UiVertex>());
    let f2 = vk::Format::R32G32_SFLOAT;
    let ui_attrs = [attr(0, 0, f2), attr(1, 8, f2), attr(2, 16, F4)];
    let shadow = |vs, fs, bindings, attributes| {
        let desc = PipelineDesc {
            color_format: None,
            ..scene_desc(vs, fs, bindings, attributes)
        };
        create_pipeline(d, scene_layout, &desc)
    };
    let ui = |format| {
        let (vs, fs) = (spv!("ui.vert"), spv!("ui.frag"));
        let desc = screen_desc(vs, fs, format, Blend::Alpha, &ui_bind, &ui_attrs);
        create_pipeline(d, tex_layout, &desc)
    };
    let post = |vs, fs| {
        let desc = screen_desc(vs, fs, swapchain_format, Blend::Opaque, &[], &[]);
        create_pipeline(d, tex_layout, &desc)
    };
    let single = vk::SampleCountFlags::TYPE_1;
    Ok(Pipelines {
        scene: create_scene_pipelines(d, scene_layout, single, setup)?,
        second: create_scene_pipelines(d, scene_layout, single, setup)?,
        shadow: shadow(
            spv!("shadow.vert"),
            spv!("shadow.frag"),
            &mesh_bind,
            &mesh_attrs[..1],
        )?,
        shadow_skin: shadow(
            spv!("shadow_skin.vert"),
            spv!("shadow_skin.frag"),
            &skin_bind,
            &skin_shadow_attrs,
        )?,
        ui: ui(swapchain_format)?,
        ui_layer: ui(COLOR_FORMAT)?,
        post_clean: post(spv!("post_clean.vert"), spv!("post_clean.frag"))?,
        post_vhs: post(spv!("post_vhs.vert"), spv!("post_vhs.frag"))?,
        post_plain: post(spv!("post_plain.vert"), spv!("post_plain.frag"))?,
        scene_layout,
        tex_layout,
    })
}
