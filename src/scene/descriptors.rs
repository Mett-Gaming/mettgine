//! Sampler, Descriptor-Sets und ihr Inhalt.

use super::*;

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
pub(super) struct Descriptors {
    pub linear: vk::Sampler,
    pub shadow_sampler: vk::Sampler,
    pub scene_set_layout: vk::DescriptorSetLayout,
    pub tex_set_layout: vk::DescriptorSetLayout,
    pub pool: vk::DescriptorPool,
    pub main_sets: Vec<vk::DescriptorSet>,
    pub shadow_sets: Vec<vk::DescriptorSet>,
    pub post_set: vk::DescriptorSet,
    pub ui_set: vk::DescriptorSet,
}

pub(super) unsafe fn create_descriptors(d: &ash::Device) -> Result<Descriptors> {
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

impl SceneRenderer {
    pub(super) fn write_scene_descriptors(&self) {
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
