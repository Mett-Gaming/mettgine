//! Sampler, Set-Layouts und Sets der modernen Grafik.
//!
//! Set 0 ([`FRAME`]): 0 = Objekte, 1 = Knochen, 2 = Szene, 3 = Lampen, 4 = Zellen, 5 = Lampen je
//! Zelle, 6 = Materialwerte, 7 = Sonnenschatten (Feld), 8 = Lampenschatten (Atlas),
//! 9 = Vergleichs-Sampler, 10 = Verdeckung, 11 = weicher Sampler, 12 = Himmelsbild,
//! 13 = Tiefe, 14 = Reihenfolge, 15 = Streulicht, 16 = Spiegelung. Set 1: Texturen eines Materials (0..4) und Sampler (5).
//! Nachbearbeitung: 0..3 = Bilder, 4 = weicher, 5 = harter Sampler.

use super::super::context::Context;
use super::super::util::Buffer;
use super::super::Result;
use ash::vk;

const STORAGE: vk::DescriptorType = vk::DescriptorType::STORAGE_BUFFER;
const UNIFORM: vk::DescriptorType = vk::DescriptorType::UNIFORM_BUFFER;
const IMAGE: vk::DescriptorType = vk::DescriptorType::SAMPLED_IMAGE;
const SAMPLER: vk::DescriptorType = vk::DescriptorType::SAMPLER;

/// Arten der Bindings von Set 0
pub const FRAME: [vk::DescriptorType; 19] = [
    STORAGE, STORAGE, UNIFORM, STORAGE, STORAGE, STORAGE, STORAGE, IMAGE, IMAGE, SAMPLER, IMAGE,
    SAMPLER, IMAGE, IMAGE, STORAGE, IMAGE, IMAGE, IMAGE, IMAGE,
];
const MATERIAL: [vk::DescriptorType; 6] = [IMAGE, IMAGE, IMAGE, IMAGE, IMAGE, SAMPLER];
const POST: [vk::DescriptorType; 6] = [IMAGE, IMAGE, IMAGE, IMAGE, SAMPLER, SAMPLER];
/// Sets je Pool fuer Materialien
const CHUNK: u32 = 256;
/// Sets der Nachbearbeitung je Bild im Flug
pub const POST_SETS: usize = 64;
/// danach Sets fuer Bilder der Oberflaeche
pub const IMAGE_SETS: usize = 64;
const ALL_SETS: usize = POST_SETS + IMAGE_SETS;

unsafe fn layout(d: &ash::Device, types: &[vk::DescriptorType]) -> Result<vk::DescriptorSetLayout> {
    let bindings: Vec<_> = types
        .iter()
        .enumerate()
        .map(|(b, ty)| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(b as u32)
                .descriptor_type(*ty)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
        })
        .collect();
    Ok(d.create_descriptor_set_layout(
        &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
        None,
    )?)
}

unsafe fn pool(
    d: &ash::Device,
    types: &[vk::DescriptorType],
    sets: u32,
    flags: vk::DescriptorPoolCreateFlags,
) -> Result<vk::DescriptorPool> {
    let sizes: Vec<_> = [STORAGE, UNIFORM, IMAGE, SAMPLER]
        .into_iter()
        .map(|ty| vk::DescriptorPoolSize {
            ty,
            descriptor_count: types.iter().filter(|t| **t == ty).count() as u32 * sets,
        })
        .filter(|s| s.descriptor_count > 0)
        .collect();
    Ok(d.create_descriptor_pool(
        &vk::DescriptorPoolCreateInfo::default()
            .flags(flags)
            .max_sets(sets)
            .pool_sizes(&sizes),
        None,
    )?)
}

unsafe fn alloc(
    d: &ash::Device,
    pool: vk::DescriptorPool,
    layout: vk::DescriptorSetLayout,
    n: usize,
) -> Result<Vec<vk::DescriptorSet>> {
    let layouts = vec![layout; n];
    Ok(d.allocate_descriptor_sets(
        &vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&layouts),
    )?)
}

pub fn image_info(view: vk::ImageView) -> [vk::DescriptorImageInfo; 1] {
    [vk::DescriptorImageInfo::default()
        .image_view(view)
        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
}

/// Sets der Materialien; waechst in Stuecken.
pub struct MaterialSets {
    pub layout: vk::DescriptorSetLayout,
    pools: Vec<vk::DescriptorPool>,
    free: Vec<vk::DescriptorSet>,
    sampler: vk::Sampler,
}

impl MaterialSets {
    pub fn new(ctx: &Context) -> Result<Self> {
        let d = &ctx.device;
        // SAFETY: gueltiges Geraet; freigegeben in `destroy`.
        unsafe {
            let repeat = vk::SamplerAddressMode::REPEAT;
            let sampler = d.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                    .address_mode_u(repeat)
                    .address_mode_v(repeat)
                    .address_mode_w(repeat)
                    .anisotropy_enable(ctx.max_anisotropy > 1.0)
                    .max_anisotropy(ctx.max_anisotropy.clamp(1.0, 8.0))
                    .max_lod(vk::LOD_CLAMP_NONE),
                None,
            )?;
            Ok(Self {
                layout: layout(d, &MATERIAL)?,
                pools: Vec::new(),
                free: Vec::new(),
                sampler,
            })
        }
    }

    pub fn alloc(&mut self, ctx: &Context) -> Result<vk::DescriptorSet> {
        if let Some(s) = self.free.pop() {
            return Ok(s);
        }
        // SAFETY: wie oben.
        unsafe {
            let d = &ctx.device;
            let p = pool(d, &MATERIAL, CHUNK, Default::default())?;
            self.pools.push(p);
            self.free = alloc(d, p, self.layout, CHUNK as usize)?;
        }
        self.free.pop().ok_or_else(|| "kein Material-Set".into())
    }

    /// Zur Wiederverwendung zurueckgeben (die GPU benutzt es nicht mehr).
    pub fn free(&mut self, _d: &ash::Device, s: vk::DescriptorSet) {
        self.free.push(s);
    }

    pub fn write(&self, ctx: &Context, set: vk::DescriptorSet, views: [vk::ImageView; 5]) {
        let infos = views.map(image_info);
        let sampler = [vk::DescriptorImageInfo::default().sampler(self.sampler)];
        let writes: Vec<_> = (0..6)
            .map(|b| {
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(b as u32)
                    .descriptor_type(MATERIAL[b])
                    .image_info(if b < 5 { &infos[b] } else { &sampler })
            })
            .collect();
        // SAFETY: frisches, unbenutztes Set; alle Ansichten leben noch.
        unsafe { ctx.device.update_descriptor_sets(&writes, &[]) };
    }

    /// # Safety
    /// Die GPU ist untaetig.
    pub unsafe fn destroy(&self, d: &ash::Device) {
        for p in &self.pools {
            d.destroy_descriptor_pool(*p, None);
        }
        d.destroy_descriptor_set_layout(self.layout, None);
        d.destroy_sampler(self.sampler, None);
    }
}

/// Layouts und Sets pro Bild.
pub struct Sets {
    pub frame_layout: vk::DescriptorSetLayout,
    pub post_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    post_pool: vk::DescriptorPool,
    /// je Bild im Flug: 0 = Schatten und Tiefe (Platzhalterbilder), 1 = Beleuchtung,
    /// 2 = Halbtransparentes (Tiefe lesbar)
    pub frames: Vec<[vk::DescriptorSet; 3]>,
    /// je Bild im Flug ein Vorrat fuer die Nachbearbeitung
    pub post: Vec<Vec<vk::DescriptorSet>>,
    pub linear: vk::Sampler,
    pub nearest: vk::Sampler,
    pub compare: vk::Sampler,
}

impl Sets {
    pub fn new(ctx: &Context, frames: usize) -> Result<Self> {
        let d = &ctx.device;
        let clamp = vk::SamplerAddressMode::CLAMP_TO_EDGE;
        let base = |filter| {
            vk::SamplerCreateInfo::default()
                .mag_filter(filter)
                .min_filter(filter)
                .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                .address_mode_u(clamp)
                .address_mode_v(clamp)
                .address_mode_w(clamp)
                .max_lod(vk::LOD_CLAMP_NONE)
        };
        // SAFETY: gueltiges Geraet; freigegeben in `destroy`.
        unsafe {
            let frame_layout = layout(d, &FRAME)?;
            let post_layout = layout(d, &POST)?;
            let pool_ = pool(d, &FRAME, frames as u32 * 3, Default::default())?;
            let count = (frames * ALL_SETS) as u32;
            let post_pool = pool(d, &POST, count, Default::default())?;
            let all = alloc(d, pool_, frame_layout, frames * 3)?;
            let post = alloc(d, post_pool, post_layout, frames * ALL_SETS)?;
            Ok(Self {
                frame_layout,
                post_layout,
                pool: pool_,
                post_pool,
                frames: all.chunks(3).map(|c| [c[0], c[1], c[2]]).collect(),
                post: post.chunks(ALL_SETS).map(<[_]>::to_vec).collect(),
                linear: d.create_sampler(&base(vk::Filter::LINEAR), None)?,
                nearest: d.create_sampler(&base(vk::Filter::NEAREST), None)?,
                compare: d.create_sampler(
                    &base(vk::Filter::LINEAR)
                        .compare_enable(true)
                        .compare_op(vk::CompareOp::LESS_OR_EQUAL),
                    None,
                )?,
            })
        }
    }

    /// Set 0 schreiben. Die GPU benutzt es gerade nicht.
    pub fn write_frame(
        &self,
        ctx: &Context,
        set: vk::DescriptorSet,
        buffers: [&Buffer; 8],
        images: [vk::ImageView; 9],
        depth_layout: vk::ImageLayout,
    ) {
        let buf = buffers.map(|b| {
            [vk::DescriptorBufferInfo::default()
                .buffer(b.buffer)
                .range(vk::WHOLE_SIZE)]
        });
        let mut img = images.map(image_info);
        img[4][0].image_layout = depth_layout;
        let compare = [vk::DescriptorImageInfo::default().sampler(self.compare)];
        let linear = [vk::DescriptorImageInfo::default().sampler(self.linear)];
        let writes: Vec<_> = (0..FRAME.len())
            .map(|b| {
                let w = vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(b as u32)
                    .descriptor_type(FRAME[b]);
                match b {
                    0..=6 => w.buffer_info(&buf[b]),
                    7 => w.image_info(&img[0]),
                    8 => w.image_info(&img[1]),
                    9 => w.image_info(&compare),
                    10 => w.image_info(&img[2]),
                    11 => w.image_info(&linear),
                    12 => w.image_info(&img[3]),
                    13 => w.image_info(&img[4]),
                    14 => w.buffer_info(&buf[7]),
                    _ => w.image_info(&img[b - 10]),
                }
            })
            .collect();
        // SAFETY: siehe oben.
        unsafe { ctx.device.update_descriptor_sets(&writes, &[]) };
    }

    /// Ein Set der Nachbearbeitung schreiben; `depth` ist die Ansicht des Tiefenbilds.
    pub fn write_post(
        &self,
        d: &ash::Device,
        set: vk::DescriptorSet,
        views: [vk::ImageView; 4],
        depth: Option<vk::ImageView>,
    ) {
        let img = views.map(|v| {
            let mut info = image_info(v);
            if Some(v) == depth {
                info[0].image_layout = vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL;
            }
            info
        });
        let linear = [vk::DescriptorImageInfo::default().sampler(self.linear)];
        let nearest = [vk::DescriptorImageInfo::default().sampler(self.nearest)];
        let writes: Vec<_> = (0..6)
            .map(|b| {
                let w = vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(b as u32)
                    .descriptor_type(POST[b]);
                w.image_info(match b {
                    0..=3 => &img[b],
                    4 => &linear,
                    _ => &nearest,
                })
            })
            .collect();
        // SAFETY: Set des Bildes, mit dem die GPU fertig ist.
        unsafe { d.update_descriptor_sets(&writes, &[]) };
    }

    /// # Safety
    /// Die GPU ist untaetig.
    pub unsafe fn destroy(&self, d: &ash::Device) {
        d.destroy_descriptor_pool(self.pool, None);
        d.destroy_descriptor_pool(self.post_pool, None);
        d.destroy_descriptor_set_layout(self.frame_layout, None);
        d.destroy_descriptor_set_layout(self.post_layout, None);
        for s in [self.linear, self.nearest, self.compare] {
            d.destroy_sampler(s, None);
        }
    }
}
