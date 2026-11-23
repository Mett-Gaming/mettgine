//! Sampler, Descriptor-Sets und ihr Inhalt.
//!
//! Set 0 der Szene: 0 = Instanzen, 1 = Knochen, 2 = [`Globals`], 3 = Schattenkarte,
//! 4 = Vergleichs-Sampler, 5 = Lampen, 6 = Welt-Boxen, 7 = Schild-Atlas, 8 = weicher Sampler,
//! 9 = Bild der zweiten Kamera. Je Bild im Flug gibt es ein Set pro Durchgang ([`Pass`]) und
//! Instanz-Puffer (beweglich, fest).

use super::*;

/// Verkleinerungsstufen der Schrift; der Abstand der Zeichen im Atlas reicht dafuer
pub(super) const FONT_MIPS: u32 = 3;

/// Durchgaenge mit eigenem Set: jeder darf nicht lesen, was er gerade beschreibt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pass {
    Main = 0,
    Second = 1,
    Shadow = 2,
}
const PASSES: usize = 3;

/// Sets eines Bildes im Flug: `[Durchgang][0 = bewegliche, 1 = feste Instanzen]`.
pub(super) type FrameSets = [[vk::DescriptorSet; 2]; PASSES];

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

/// Alle Sampler.
pub(super) struct Samplers {
    pub nearest: vk::Sampler,
    pub linear: vk::Sampler,
    /// wie `linear`, aber mit Verkleinerungsstufen (Schrift)
    pub linear_mips: vk::Sampler,
    pub shadow: vk::Sampler,
}

impl Samplers {
    unsafe fn new(d: &ash::Device) -> Result<Self> {
        let clamp = vk::SamplerAddressMode::CLAMP_TO_EDGE;
        let make = |filter, max_lod: f32| {
            d.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(filter)
                    .min_filter(filter)
                    .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                    .address_mode_u(clamp)
                    .address_mode_v(clamp)
                    .address_mode_w(clamp)
                    .max_lod(max_lod),
                None,
            )
        };
        // Vergleichs-Sampler: die Hardware filtert die Schattenkante (PCF)
        let border = vk::SamplerAddressMode::CLAMP_TO_BORDER;
        let shadow = d.create_sampler(
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
        Ok(Self {
            nearest: make(vk::Filter::NEAREST, 0.0)?,
            linear: make(vk::Filter::LINEAR, 0.0)?,
            linear_mips: make(vk::Filter::LINEAR, (FONT_MIPS - 1) as f32)?,
            shadow,
        })
    }

    pub unsafe fn destroy(&self, d: &ash::Device) {
        for s in [self.nearest, self.linear, self.linear_mips, self.shadow] {
            d.destroy_sampler(s, None);
        }
    }
}

/// Sampler, Set-Layouts, Pool und Sets.
pub(super) struct Descriptors {
    pub samplers: Samplers,
    pub scene_layout: vk::DescriptorSetLayout,
    pub tex_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    pub frames: Vec<FrameSets>,
    pub post: vk::DescriptorSet,
    pub ui: vk::DescriptorSet,
}

const STORAGE: vk::DescriptorType = vk::DescriptorType::STORAGE_BUFFER;
const UNIFORM: vk::DescriptorType = vk::DescriptorType::UNIFORM_BUFFER;
const IMAGE: vk::DescriptorType = vk::DescriptorType::SAMPLED_IMAGE;
const SAMPLER: vk::DescriptorType = vk::DescriptorType::SAMPLER;
/// Arten der Bindings 0..9 der Szene
const SCENE: [vk::DescriptorType; 10] = [
    STORAGE, STORAGE, UNIFORM, IMAGE, SAMPLER, STORAGE, STORAGE, IMAGE, SAMPLER, IMAGE,
];
/// Bindings 0..2 von Nachbearbeitung und UI: Bild, Sampler, zweites Bild
const TEX: [vk::DescriptorType; 3] = [IMAGE, SAMPLER, IMAGE];

unsafe fn set_layout(
    d: &ash::Device,
    types: &[vk::DescriptorType],
    stages: vk::ShaderStageFlags,
) -> Result<vk::DescriptorSetLayout> {
    let bindings: Vec<_> = types
        .iter()
        .enumerate()
        .map(|(b, ty)| binding(b as u32, *ty, stages))
        .collect();
    Ok(d.create_descriptor_set_layout(
        &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
        None,
    )?)
}

impl Descriptors {
    pub unsafe fn new(d: &ash::Device) -> Result<Self> {
        let frag = vk::ShaderStageFlags::FRAGMENT;
        let scene_layout = set_layout(d, &SCENE, vk::ShaderStageFlags::VERTEX | frag)?;
        let tex_layout = set_layout(d, &TEX, frag)?;
        let scene_sets = FRAMES_IN_FLIGHT * PASSES * 2;
        let count = |ty| {
            let of = |list: &[vk::DescriptorType]| list.iter().filter(|t| **t == ty).count();
            vk::DescriptorPoolSize {
                ty,
                descriptor_count: (of(&SCENE) * scene_sets + of(&TEX) * 2) as u32,
            }
        };
        let sizes = [count(STORAGE), count(UNIFORM), count(IMAGE), count(SAMPLER)];
        let pool = d.create_descriptor_pool(
            &vk::DescriptorPoolCreateInfo::default()
                .max_sets(scene_sets as u32 + 2)
                .pool_sizes(&sizes),
            None,
        )?;
        let mut layouts = vec![scene_layout; scene_sets];
        layouts.extend([tex_layout; 2]);
        let sets = d.allocate_descriptor_sets(
            &vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(pool)
                .set_layouts(&layouts),
        )?;
        let frames = sets[..scene_sets]
            .chunks(PASSES * 2)
            .map(|c| [[c[0], c[1]], [c[2], c[3]], [c[4], c[5]]])
            .collect();
        Ok(Self {
            samplers: Samplers::new(d)?,
            scene_layout,
            tex_layout,
            pool,
            frames,
            post: sets[scene_sets],
            ui: sets[scene_sets + 1],
        })
    }

    pub unsafe fn destroy(&self, d: &ash::Device) {
        d.destroy_descriptor_pool(self.pool, None);
        d.destroy_descriptor_set_layout(self.scene_layout, None);
        d.destroy_descriptor_set_layout(self.tex_layout, None);
        self.samplers.destroy(d);
    }
}

fn image_info(view: vk::ImageView) -> [vk::DescriptorImageInfo; 1] {
    [vk::DescriptorImageInfo::default()
        .image_view(view)
        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
}

fn sampler_info(s: vk::Sampler) -> [vk::DescriptorImageInfo; 1] {
    [vk::DescriptorImageInfo::default().sampler(s)]
}

fn buffer_info(b: &Buffer) -> [vk::DescriptorBufferInfo; 1] {
    [vk::DescriptorBufferInfo::default()
        .buffer(b.buffer)
        .range(vk::WHOLE_SIZE)]
}

impl Renderer {
    /// Sets eines Bildes im Flug schreiben (die GPU benutzt sie gerade nicht).
    pub(super) fn write_scene_descriptors(&self, fi: usize) {
        let s = &self.desc.samplers;
        let bones = buffer_info(&self.bone_bufs[fi]);
        let (lights, boxes) = (buffer_info(&self.light_buf), buffer_info(&self.box_buf));
        let signs = image_info(self.signs.view);
        let (cmp, linear) = (sampler_info(s.shadow), sampler_info(s.linear));
        for pass in [Pass::Main, Pass::Second, Pass::Shadow] {
            let globals = buffer_info(match pass {
                Pass::Second => &self.second_globals[fi],
                _ => &self.globals_bufs[fi],
            });
            let shadow = image_info(match pass {
                Pass::Shadow => self.dummy_shadow.view,
                _ => self.shadow.view,
            });
            // die zweite Kamera sieht ihr eigenes Bild nicht: Platzhalter
            let camera = image_info(match pass {
                Pass::Second => self.signs.view,
                _ => self.second.color.view,
            });
            for (kind, insts) in [&self.inst_bufs[fi], &self.static_buf].iter().enumerate() {
                let insts = buffer_info(insts);
                let w = |b: usize| {
                    vk::WriteDescriptorSet::default()
                        .dst_set(self.desc.frames[fi][pass as usize][kind])
                        .dst_binding(b as u32)
                        .descriptor_type(SCENE[b])
                };
                let writes = [
                    w(0).buffer_info(&insts),
                    w(1).buffer_info(&bones),
                    w(2).buffer_info(&globals),
                    w(3).image_info(&shadow),
                    w(4).image_info(&cmp),
                    w(5).buffer_info(&lights),
                    w(6).buffer_info(&boxes),
                    w(7).image_info(&signs),
                    w(8).image_info(&linear),
                    w(9).image_info(&camera),
                ];
                // SAFETY: Sets und Ressourcen gehoeren diesem Geraet und sind nicht in Benutzung.
                unsafe { self.ctx.device.update_descriptor_sets(&writes, &[]) };
            }
        }
    }

    /// Nachbearbeitung liest Szene und UI-Ebene, UI den Schrift-Atlas (neu nach Groessenwechsel).
    pub(super) fn write_tex_descriptors(&self) {
        let s = &self.desc.samplers;
        let scale = if self.smooth { s.linear } else { s.nearest };
        let font = if self.font_mipmaps {
            s.linear_mips
        } else {
            scale
        };
        for (set, view, second, sampler) in [
            (
                self.desc.post,
                self.color.view,
                self.ui_layer.image.view,
                scale,
            ),
            (self.desc.ui, self.font.view, self.font.view, font),
        ] {
            let (first, second) = (image_info(view), image_info(second));
            let sampler = sampler_info(sampler);
            let w = |b: usize| {
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(b as u32)
                    .descriptor_type(TEX[b])
            };
            let writes = [
                w(0).image_info(&first),
                w(1).image_info(&sampler),
                w(2).image_info(&second),
            ];
            // SAFETY: wie oben; aufgerufen nur beim Aufbau und nach device_wait_idle.
            unsafe { self.ctx.device.update_descriptor_sets(&writes, &[]) };
        }
    }
}
