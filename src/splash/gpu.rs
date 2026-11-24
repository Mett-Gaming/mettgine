//! Startbild zeichnen: ein Atlas, eine Pipeline, direkt auf die Swapchain.

use super::{build, Timeline, BACK, MAX_VERTS};
use crate::render::context::Context;
use crate::render::pipeline::{create_pipeline, Blend, PipelineDesc};
use crate::render::transfer::{set_viewport, upload_mips};
use crate::render::util::{image_barrier, Buffer, Image, ImageDesc};
use crate::render::{Result, UiVertex};
use ash::vk;
use std::mem::size_of;

static ATLAS_PNG: &[u8] = include_bytes!("../../assets/splash.png");
/// Verkleinerungsstufen, damit auch kleine Fenster sauber aussehen
const MIPS: u32 = 4;

/// Atlas entpacken: (RGBA mit vormultipliziertem Alpha, Breite, Hoehe).
fn decode() -> Result<(Vec<u8>, u32, u32)> {
    let mut reader = png::Decoder::new(ATLAS_PNG).read_info()?;
    let mut px = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut px)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err("Startbild: RGBA mit 8 Bit erwartet".into());
    }
    px.truncate(info.buffer_size());
    for p in px.chunks_mut(4) {
        let a = p[3] as u32;
        p[..3]
            .iter_mut()
            .for_each(|c| *c = ((*c as u32 * a + 127) / 255) as u8);
    }
    Ok((px, info.width, info.height))
}

/// Halbiert ein RGBA-Bild (Mittel aus je 2x2 Bildpunkten).
fn halve(px: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = Vec::with_capacity((nw * nh * 4) as usize);
    let at =
        |x: u32, y: u32, k: u32| px[((y.min(h - 1) * w + x.min(w - 1)) * 4 + k) as usize] as u32;
    for y in (0..nh).map(|y| y * 2) {
        for x in (0..nw).map(|x| x * 2) {
            for k in 0..4 {
                let sum = at(x, y, k) + at(x + 1, y, k) + at(x, y + 1, k) + at(x + 1, y + 1, k);
                out.push(((sum + 2) / 4) as u8);
            }
        }
    }
    (out, nw, nh)
}

/// Startbild samt GPU-Ressourcen; freigegeben in `destroy` (aufgerufen vom Renderer).
pub(crate) struct Splash {
    time: Timeline,
    image: Image,
    sampler: vk::Sampler,
    set_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    verts: Vec<UiVertex>,
    bufs: Vec<Buffer>,
}

unsafe fn upload_atlas(ctx: &Context, cmds: vk::CommandPool) -> Result<Image> {
    let mut levels = vec![decode()?];
    for i in 1..MIPS as usize {
        let (px, w, h) = &levels[i - 1];
        let next = halve(px, *w, *h);
        levels.push(next);
    }
    let image = Image::with(
        ctx,
        &ImageDesc {
            width: levels[0].1,
            height: levels[0].2,
            format: vk::Format::R8G8B8A8_UNORM,
            usage: vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            aspect: vk::ImageAspectFlags::COLOR,
            samples: vk::SampleCountFlags::TYPE_1,
            mips: MIPS,
        },
    )?;
    let refs: Vec<_> = levels.iter().map(|l| (&l.0[..], l.1, l.2)).collect();
    upload_mips(ctx, cmds, &image, &refs)?;
    Ok(image)
}

unsafe fn create_set(
    d: &ash::Device,
    view: vk::ImageView,
    sampler: vk::Sampler,
) -> Result<(
    vk::DescriptorSetLayout,
    vk::DescriptorPool,
    vk::DescriptorSet,
)> {
    let types = [
        vk::DescriptorType::SAMPLED_IMAGE,
        vk::DescriptorType::SAMPLER,
    ];
    let bind = |b: usize| {
        vk::DescriptorSetLayoutBinding::default()
            .binding(b as u32)
            .descriptor_type(types[b])
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)
    };
    let layout = d.create_descriptor_set_layout(
        &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[bind(0), bind(1)]),
        None,
    )?;
    let sizes = types.map(|ty| vk::DescriptorPoolSize {
        ty,
        descriptor_count: 1,
    });
    let pool = d.create_descriptor_pool(
        &vk::DescriptorPoolCreateInfo::default()
            .max_sets(1)
            .pool_sizes(&sizes),
        None,
    )?;
    let set = d.allocate_descriptor_sets(
        &vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&[layout]),
    )?[0];
    let img = [vk::DescriptorImageInfo::default()
        .image_view(view)
        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
    let smp = [vk::DescriptorImageInfo::default().sampler(sampler)];
    let w = |b: usize| {
        vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(b as u32)
            .descriptor_type(types[b])
    };
    d.update_descriptor_sets(&[w(0).image_info(&img), w(1).image_info(&smp)], &[]);
    Ok((layout, pool, set))
}

impl Splash {
    /// # Safety
    /// `ctx` muss gueltig sein; aufrufen, bevor das erste Bild gezeichnet wird.
    pub unsafe fn new(
        ctx: &Context,
        cmds: vk::CommandPool,
        format: vk::Format,
        frames: usize,
    ) -> Result<Self> {
        let d = &ctx.device;
        let image = upload_atlas(ctx, cmds)?;
        let clamp = vk::SamplerAddressMode::CLAMP_TO_EDGE;
        let sampler = d.create_sampler(
            &vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                .address_mode_u(clamp)
                .address_mode_v(clamp)
                .address_mode_w(clamp)
                .max_lod((MIPS - 1) as f32),
            None,
        )?;
        let (set_layout, pool, set) = create_set(d, image.view, sampler)?;
        let layout = d.create_pipeline_layout(
            &vk::PipelineLayoutCreateInfo::default().set_layouts(&[set_layout]),
            None,
        )?;
        let attr = |location, offset, format| vk::VertexInputAttributeDescription {
            location,
            binding: 0,
            format,
            offset,
        };
        let f2 = vk::Format::R32G32_SFLOAT;
        let pipeline = create_pipeline(
            &ctx.gpu(),
            layout,
            &PipelineDesc {
                vs_entry: c"vs_main",
                fs_entry: c"fs_main",
                more_colors: &[],
                dynamic_cull: false,
                vs: include_bytes!(concat!(env!("OUT_DIR"), "/splash.vert.spv")),
                fs: include_bytes!(concat!(env!("OUT_DIR"), "/splash.frag.spv")),
                color_format: Some(format),
                depth_format: None,
                depth_test: false,
                depth_write: false,
                depth_compare: vk::CompareOp::ALWAYS,
                blend: Blend::Premultiplied,
                samples: vk::SampleCountFlags::TYPE_1,
                bindings: &[vk::VertexInputBindingDescription {
                    binding: 0,
                    stride: size_of::<UiVertex>() as u32,
                    input_rate: vk::VertexInputRate::VERTEX,
                }],
                attributes: &[
                    attr(0, 0, f2),
                    attr(1, 8, f2),
                    attr(2, 16, vk::Format::R32G32B32A32_SFLOAT),
                ],
            },
        )?;
        let bytes = (MAX_VERTS * size_of::<UiVertex>()) as u64;
        let bufs = (0..frames)
            .map(|_| Buffer::new_host(ctx, bytes, vk::BufferUsageFlags::VERTEX_BUFFER))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            time: Timeline::default(),
            image,
            sampler,
            set_layout,
            pool,
            set,
            layout,
            pipeline,
            verts: Vec::with_capacity(MAX_VERTS),
            bufs,
        })
    }

    /// Laeuft es noch?
    pub fn active(&self) -> bool {
        self.time.active()
    }

    /// Ganzes Bild aufnehmen (statt der Szene, ohne den Befehlspuffer abzuschliessen) und die Zeit weiterzaehlen.
    ///
    /// # Safety
    /// Die GPU muss mit dem Bild `fi` fertig sein, `cmd` gehoert zu ihm.
    pub unsafe fn record(
        &mut self,
        d: &ash::Device,
        cmd: vk::CommandBuffer,
        fi: usize,
        (target, view, extent): (vk::Image, vk::ImageView, vk::Extent2D),
    ) -> Result<()> {
        self.time.tick();
        let aspect = extent.width.max(1) as f32 / extent.height.max(1) as f32;
        build(self.time.t, aspect, &mut self.verts);
        self.bufs[fi].write(bytemuck::cast_slice(&self.verts));
        d.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
        d.begin_command_buffer(
            cmd,
            &vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )?;
        let output = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
        d.cmd_pipeline_barrier(
            cmd,
            output,
            output,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier(
                target,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::COLOR_ATTACHMENT_READ,
            )],
        );
        self.draw(d, cmd, fi, (view, extent));
        Ok(())
    }

    unsafe fn draw(
        &self,
        d: &ash::Device,
        cmd: vk::CommandBuffer,
        fi: usize,
        (view, extent): (vk::ImageView, vk::Extent2D),
    ) {
        let k = super::brightness(self.time.t);
        let att = [vk::RenderingAttachmentInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [BACK[0] * k, BACK[1] * k, BACK[2] * k, 1.0],
                },
            })];
        d.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent,
                })
                .layer_count(1)
                .color_attachments(&att),
        );
        set_viewport(d, cmd, extent);
        let gfx = vk::PipelineBindPoint::GRAPHICS;
        d.cmd_bind_pipeline(cmd, gfx, self.pipeline);
        d.cmd_bind_descriptor_sets(cmd, gfx, self.layout, 0, &[self.set], &[]);
        d.cmd_bind_vertex_buffers(cmd, 0, &[self.bufs[fi].buffer], &[0]);
        d.cmd_draw(cmd, self.verts.len() as u32, 1, 0, 0);
        d.cmd_end_rendering(cmd);
    }

    /// # Safety
    /// Die GPU darf nichts davon mehr benutzen.
    pub unsafe fn destroy(&self, d: &ash::Device) {
        d.destroy_pipeline(self.pipeline, None);
        d.destroy_pipeline_layout(self.layout, None);
        d.destroy_descriptor_pool(self.pool, None);
        d.destroy_descriptor_set_layout(self.set_layout, None);
        d.destroy_sampler(self.sampler, None);
        self.image.destroy(d);
        for b in &self.bufs {
            b.destroy(d);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn atlas_has_the_size_the_layout_expects() {
        let (px, w, h) = super::decode().unwrap();
        assert_eq!((w as f32, h as f32), super::super::ATLAS);
        assert_eq!(px.len(), (w * h * 4) as usize);
        assert!(px
            .chunks(4)
            .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]));
        let (small, w2, h2) = super::halve(&px, w, h);
        assert_eq!(small.len(), (w2 * h2 * 4) as usize);
    }
}
