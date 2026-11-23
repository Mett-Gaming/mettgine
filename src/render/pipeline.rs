//! Grafik-Pipelines bauen (Dynamic Rendering), gemeinsam fuer `render` und `scene`.

use super::Result;
use ash::vk;

/// Deckkraft halbtransparenter Figuren und Modelle ([`Blend::Ghost`])
const GHOST_ALPHA: f32 = 0.55;

/// Wie eine Pipeline ihre Farbe mit dem Bild mischt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Blend {
    /// deckend
    Opaque,
    /// nach Alpha (UI)
    Alpha,
    /// feste Deckkraft (Hologramme)
    Ghost,
    /// Farbe ist schon mit Alpha multipliziert; Alpha 0 = reines Aufhellen (Partikel)
    Premultiplied,
}

impl Blend {
    fn attachment(self) -> vk::PipelineColorBlendAttachmentState {
        use vk::BlendFactor as F;
        let base = vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA);
        let (src, dst, src_a, dst_a) = match self {
            Blend::Opaque => return base,
            Blend::Alpha => (
                F::SRC_ALPHA,
                F::ONE_MINUS_SRC_ALPHA,
                F::ONE,
                F::ONE_MINUS_SRC_ALPHA,
            ),
            Blend::Ghost => (
                F::CONSTANT_ALPHA,
                F::ONE_MINUS_CONSTANT_ALPHA,
                F::ZERO,
                F::ONE,
            ),
            Blend::Premultiplied => (
                F::ONE,
                F::ONE_MINUS_SRC_ALPHA,
                F::ONE,
                F::ONE_MINUS_SRC_ALPHA,
            ),
        };
        base.blend_enable(true)
            .src_color_blend_factor(src)
            .dst_color_blend_factor(dst)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(src_a)
            .dst_alpha_blend_factor(dst_a)
            .alpha_blend_op(vk::BlendOp::ADD)
    }
}

/// Beschreibung einer Grafik-Pipeline.
pub(crate) struct PipelineDesc<'a> {
    pub vs: &'a [u8],
    pub fs: &'a [u8],
    /// None = nur Tiefe (Schattenkarte)
    pub color_format: Option<vk::Format>,
    pub depth_format: Option<vk::Format>,
    pub depth_test: bool,
    pub depth_write: bool,
    pub depth_compare: vk::CompareOp,
    pub blend: Blend,
    /// Abtastungen pro Bildpunkt (Kantenglaettung)
    pub samples: vk::SampleCountFlags,
    pub bindings: &'a [vk::VertexInputBindingDescription],
    pub attributes: &'a [vk::VertexInputAttributeDescription],
}

pub(crate) unsafe fn create_pipeline(
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
    let multisample =
        vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(desc.samples);
    let depth = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(desc.depth_test)
        .depth_write_enable(desc.depth_write)
        .depth_compare_op(desc.depth_compare);
    let blend_attachments = [desc.blend.attachment()];
    let blend = vk::PipelineColorBlendStateCreateInfo::default()
        .attachments(if desc.color_format.is_some() {
            &blend_attachments[..]
        } else {
            &[]
        })
        .blend_constants([0.0, 0.0, 0.0, GHOST_ALPHA]);
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
