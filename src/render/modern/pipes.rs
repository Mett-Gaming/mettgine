//! Pipelines der modernen Grafik.

use super::super::context::Context;
use super::super::pipeline::{create_pipeline, Blend, PipelineDesc};
use super::super::pipelines::{
    attr, mesh_attrs, scene_desc, screen_desc, spv, vertex_binding, F3, F4,
};
use super::super::Result;
use super::model::{Joints, Vertex};
use super::prepare::{Kind, KINDS};
use super::targets::{AO, EXPOSURE, HDR, LDR, MOTION, NORMAL};
use crate::gpu::{MeshVertex, SkinVertex};
use crate::particles::ParticleInst;
use ash::vk;

/// Werte der Nachbearbeitung (siehe `_p_common.wgsl`)
pub const POST_PUSH: u32 = 128;
/// Sicht der Lampe beim Schattenzeichnen
const SCENE_PUSH: u32 = 64;

/// Shader einer Objektart.
struct Shaders {
    vs: &'static [u8],
    vs_shadow: &'static [u8],
    fs: &'static [u8],
    fs_pre: &'static [u8],
    fs_shadow: &'static [u8],
}

macro_rules! shaders {
    ($name:literal) => {
        Shaders {
            vs: include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".vert.spv")),
            vs_shadow: include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".vs_shadow.spv")),
            fs: include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".frag.spv")),
            fs_pre: include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".fs_pre.spv")),
            fs_shadow: include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".fs_shadow.spv")),
        }
    };
}

fn shaders(kind: Kind) -> Shaders {
    match kind {
        Kind::Object => shaders!("m_obj"),
        Kind::Skinned => shaders!("m_objskin"),
        Kind::Mesh => shaders!("m_mesh"),
        Kind::Skin => shaders!("m_skin"),
    }
}

/// Vertex-Layout einer Objektart.
fn vertex_layout(
    kind: Kind,
) -> (
    Vec<vk::VertexInputBindingDescription>,
    Vec<vk::VertexInputAttributeDescription>,
) {
    let f2 = vk::Format::R32G32_SFLOAT;
    match kind {
        Kind::Object | Kind::Skinned => {
            let mut bindings = vertex_binding(size_of::<Vertex>()).to_vec();
            let mut attrs = vec![
                attr(0, 0, F3),
                attr(1, 12, F3),
                attr(2, 24, F4),
                attr(3, 40, f2),
                attr(4, 48, F4),
            ];
            if kind == Kind::Skinned {
                let [mut b] = vertex_binding(size_of::<Joints>());
                b.binding = 1;
                bindings.push(b);
                for mut a in [attr(5, 0, vk::Format::R16G16B16A16_UINT), attr(6, 8, F4)] {
                    a.binding = 1;
                    attrs.push(a);
                }
            }
            (bindings, attrs)
        }
        Kind::Mesh => (
            vertex_binding(size_of::<MeshVertex>()).to_vec(),
            mesh_attrs().to_vec(),
        ),
        Kind::Skin => (
            vertex_binding(size_of::<SkinVertex>()).to_vec(),
            vec![
                attr(0, 0, F3),
                attr(1, 12, F3),
                attr(2, 24, F3),
                attr(3, 36, F4),
                attr(4, 52, F4),
            ],
        ),
    }
}

/// Pipelines einer Objektart.
#[derive(Clone, Copy)]
pub struct KindPipes {
    pub shadow: vk::Pipeline,
    /// mit Loechern (prueft die Textur)
    pub shadow_masked: vk::Pipeline,
    pub pre: vk::Pipeline,
    pub opaque: vk::Pipeline,
    pub blend: vk::Pipeline,
}

/// Nachbearbeitung.
#[derive(Clone, Copy)]
pub struct PostPipes {
    pub ssao: vk::Pipeline,
    pub blur: vk::Pipeline,
    pub taa: vk::Pipeline,
    pub down: vk::Pipeline,
    pub up: vk::Pipeline,
    pub exposure: vk::Pipeline,
    pub tonemap: vk::Pipeline,
    pub fxaa: vk::Pipeline,
}

/// Alle Pipelines und Layouts.
pub struct Pipes {
    pub layout: vk::PipelineLayout,
    pub post_layout: vk::PipelineLayout,
    kinds: [KindPipes; 4],
    pub sky: vk::Pipeline,
    pub particle: vk::Pipeline,
    pub post: PostPipes,
    /// Bilder der Oberflaeche
    pub image: vk::Pipeline,
}

unsafe fn pipeline_layout(
    d: &ash::Device,
    sets: &[vk::DescriptorSetLayout],
    push: u32,
) -> Result<vk::PipelineLayout> {
    let push = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
        offset: 0,
        size: push,
    }];
    Ok(d.create_pipeline_layout(
        &vk::PipelineLayoutCreateInfo::default()
            .set_layouts(sets)
            .push_constant_ranges(&push),
        None,
    )?)
}

unsafe fn kind_pipes(d: &ash::Device, layout: vk::PipelineLayout, kind: Kind) -> Result<KindPipes> {
    let s = shaders(kind);
    let (bindings, attrs) = vertex_layout(kind);
    let base = || PipelineDesc {
        color_format: Some(HDR),
        dynamic_cull: true,
        ..scene_desc(s.vs, s.fs, &bindings, &attrs)
    };
    let shadow = |fs| {
        create_pipeline(
            d,
            layout,
            &PipelineDesc {
                vs: s.vs_shadow,
                vs_entry: c"vs_shadow",
                fs,
                fs_entry: c"fs_shadow",
                color_format: None,
                ..base()
            },
        )
    };
    Ok(KindPipes {
        shadow: shadow(&[])?,
        shadow_masked: shadow(s.fs_shadow)?,
        pre: create_pipeline(
            d,
            layout,
            &PipelineDesc {
                fs: s.fs_pre,
                fs_entry: c"fs_pre",
                color_format: Some(NORMAL),
                more_colors: &[MOTION],
                ..base()
            },
        )?,
        opaque: create_pipeline(
            d,
            layout,
            &PipelineDesc {
                depth_write: false,
                ..base()
            },
        )?,
        blend: create_pipeline(
            d,
            layout,
            &PipelineDesc {
                depth_write: false,
                blend: Blend::Alpha,
                ..base()
            },
        )?,
    })
}

unsafe fn post_pipes(d: &ash::Device, layout: vk::PipelineLayout) -> Result<PostPipes> {
    let vs = spv!("p_tonemap.vert");
    let make = |fs: &'static [u8], format, blend| {
        create_pipeline(d, layout, &screen_desc(vs, fs, format, blend, &[], &[]))
    };
    Ok(PostPipes {
        ssao: make(spv!("p_ssao.frag"), AO, Blend::Opaque)?,
        blur: make(spv!("p_blur.frag"), AO, Blend::Opaque)?,
        taa: make(spv!("p_taa.frag"), HDR, Blend::Opaque)?,
        down: make(spv!("p_down.frag"), HDR, Blend::Opaque)?,
        up: make(spv!("p_up.frag"), HDR, Blend::Add)?,
        exposure: make(spv!("p_exposure.frag"), EXPOSURE, Blend::Opaque)?,
        tonemap: make(spv!("p_tonemap.frag"), LDR, Blend::Opaque)?,
        fxaa: make(spv!("p_fxaa.frag"), LDR, Blend::Opaque)?,
    })
}

impl Pipes {
    pub fn new(
        ctx: &Context,
        frame: vk::DescriptorSetLayout,
        material: vk::DescriptorSetLayout,
        post: vk::DescriptorSetLayout,
        swapchain: vk::Format,
    ) -> Result<Self> {
        let d = &ctx.device;
        // SAFETY: gueltiges Geraet; freigegeben in `destroy`.
        unsafe {
            let layout = pipeline_layout(d, &[frame, material], SCENE_PUSH)?;
            let post_layout = pipeline_layout(d, &[post], POST_PUSH)?;
            let k = |kind| kind_pipes(d, layout, kind);
            let particle_bind = [vk::VertexInputBindingDescription {
                binding: 0,
                stride: size_of::<ParticleInst>() as u32,
                input_rate: vk::VertexInputRate::INSTANCE,
            }];
            let particle_attrs = [attr(0, 0, F4), attr(1, 16, F4), attr(2, 32, F4)];
            let sky = PipelineDesc {
                color_format: Some(HDR),
                depth_write: false,
                ..scene_desc(spv!("m_sky.vert"), spv!("m_sky.frag"), &[], &[])
            };
            let particle = PipelineDesc {
                color_format: Some(HDR),
                depth_write: false,
                blend: Blend::Premultiplied,
                ..scene_desc(
                    spv!("m_particle.vert"),
                    spv!("m_particle.frag"),
                    &particle_bind,
                    &particle_attrs,
                )
            };
            Ok(Self {
                layout,
                post_layout,
                kinds: [k(KINDS[0])?, k(KINDS[1])?, k(KINDS[2])?, k(KINDS[3])?],
                sky: create_pipeline(d, layout, &sky)?,
                particle: create_pipeline(d, layout, &particle)?,
                post: post_pipes(d, post_layout)?,
                image: create_pipeline(
                    d,
                    post_layout,
                    &screen_desc(
                        spv!("p_image.vert"),
                        spv!("p_image.frag"),
                        swapchain,
                        Blend::Alpha,
                        &[],
                        &[],
                    ),
                )?,
            })
        }
    }

    pub fn kind(&self, k: Kind) -> &KindPipes {
        &self.kinds[k as usize]
    }

    /// # Safety
    /// Die GPU ist untaetig.
    pub unsafe fn destroy(&self, d: &ash::Device) {
        for k in &self.kinds {
            for p in [k.shadow, k.shadow_masked, k.pre, k.opaque, k.blend] {
                d.destroy_pipeline(p, None);
            }
        }
        let p = &self.post;
        for p in [
            p.ssao,
            p.blur,
            p.taa,
            p.down,
            p.up,
            p.exposure,
            p.tonemap,
            p.fxaa,
            self.sky,
            self.particle,
        ] {
            d.destroy_pipeline(p, None);
        }
        d.destroy_pipeline(self.image, None);
        d.destroy_pipeline_layout(self.layout, None);
        d.destroy_pipeline_layout(self.post_layout, None);
    }
}
