//! Pipelines der Lampenschatten und der Verdunkelung in Ecken. Gleiche Sets wie die Szene, dazu
//! die Matrix des Schattenfelds als Push-Constant.

use super::cache::Gpu;
use super::pipeline::{create_pipeline, Blend, PipelineDesc};
use super::pipelines::{attr, mesh_attrs, scene_desc, spv, vertex_binding, F3, F4};
use super::*;
use crate::gpu::{MeshVertex, SkinVertex};
use std::mem::size_of;

/// Push-Constants der Schatten-Shader: Matrix des Feldes
const PUSH: u32 = 64;

/// Pipelines der Lampenschatten.
pub(super) struct LampPipelines {
    pub layout: vk::PipelineLayout,
    pub boxes: vk::Pipeline,
    pub mesh: vk::Pipeline,
    pub skin: vk::Pipeline,
    /// Ecken verdunkeln; haengt am Format des Szenenbilds
    pub contact: vk::Pipeline,
}

impl LampPipelines {
    pub unsafe fn new(d: &Gpu, set: vk::DescriptorSetLayout, color: vk::Format) -> Result<Self> {
        let push = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::VERTEX,
            offset: 0,
            size: PUSH,
        }];
        let layout = d.create_pipeline_layout(
            &vk::PipelineLayoutCreateInfo::default()
                .set_layouts(&[set])
                .push_constant_ranges(&push),
            None,
        )?;
        let mesh_bind = vertex_binding(size_of::<MeshVertex>());
        let skin_bind = vertex_binding(size_of::<SkinVertex>());
        let mesh_attrs = mesh_attrs();
        let skin_attrs = [attr(0, 0, F3), attr(3, 36, F4), attr(4, 52, F4)];
        let depth = |vs, bindings, attributes| {
            let desc = PipelineDesc {
                color_format: None,
                ..scene_desc(vs, &[], bindings, attributes)
            };
            create_pipeline(d, layout, &desc)
        };
        Ok(Self {
            boxes: depth(spv!("lamp_box.vert"), &[], &[])?,
            mesh: depth(spv!("lamp_mesh.vert"), &mesh_bind, &mesh_attrs[..1])?,
            skin: depth(spv!("lamp_skin.vert"), &skin_bind, &skin_attrs)?,
            contact: create_pipeline(
                d,
                layout,
                &PipelineDesc {
                    color_format: Some(color),
                    depth_format: None,
                    depth_test: false,
                    depth_write: false,
                    blend: Blend::Multiply,
                    ..scene_desc(spv!("contact.vert"), spv!("contact.frag"), &[], &[])
                },
            )?,
            layout,
        })
    }

    pub unsafe fn destroy(&self, d: &ash::Device) {
        for p in [self.boxes, self.mesh, self.skin, self.contact] {
            d.destroy_pipeline(p, None);
        }
        d.destroy_pipeline_layout(self.layout, None);
    }
}
