//! Hochladen und Auslesen: Einmal-Befehle, Texturen,
//! Viewport, BMP-Export.

use super::context::Context;
use super::util::{image_barrier, Buffer, Image};
use super::Result;
use ash::vk;

/// 32-Bit-BMP schreiben (Pixel als BGRA bzw. RGBA, oben links zuerst).
pub(crate) fn write_bmp(
    path: &std::path::Path,
    w: u32,
    h: u32,
    px: &[u8],
    rgba: bool,
) -> std::io::Result<()> {
    let row = (w * 4) as usize;
    let data_len = row * h as usize;
    let mut out = Vec::with_capacity(54 + data_len);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + data_len as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes()); // positiv = unten zuerst
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in (0..h as usize).rev() {
        for p in px[y * row..(y + 1) * row].chunks_exact(4) {
            if rgba {
                out.extend_from_slice(&[p[2], p[1], p[0], 255]);
            } else {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
    }
    std::fs::write(path, out)
}

/// Einmal-Befehlspuffer aufnehmen, abschicken und warten (nur beim Aufbau).
pub(crate) unsafe fn one_shot(
    ctx: &Context,
    pool: vk::CommandPool,
    record: impl FnOnce(vk::CommandBuffer),
) -> Result<()> {
    let d = &ctx.device;
    let cmd = d.allocate_command_buffers(
        &vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1),
    )?[0];
    d.begin_command_buffer(
        cmd,
        &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
    )?;
    record(cmd);
    d.end_command_buffer(cmd)?;
    let cmds = [cmd];
    d.queue_submit(
        ctx.queue,
        &[vk::SubmitInfo::default().command_buffers(&cmds)],
        vk::Fence::null(),
    )?;
    d.queue_wait_idle(ctx.queue)?;
    d.free_command_buffers(pool, &cmds);
    Ok(())
}

/// Graustufen-Textur (R8) hochladen und fuer Shader lesbar machen.
pub(crate) unsafe fn upload_r8(
    ctx: &Context,
    pool: vk::CommandPool,
    img: &Image,
    pixels: &[u8],
    width: u32,
    height: u32,
) -> Result<()> {
    upload_mips(ctx, pool, img, &[(pixels, width, height)])
}

/// Halbiert ein R8-Bild (Mittel aus je 2x2 Bildpunkten).
pub(crate) fn halve_r8(px: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let at = |x: u32, y: u32| px[(y.min(h - 1) * w + x.min(w - 1)) as usize] as u32;
    let mut out = Vec::with_capacity((nw * nh) as usize);
    for y in 0..nh {
        for x in 0..nw {
            let (x, y) = (x * 2, y * 2);
            let sum = at(x, y) + at(x + 1, y) + at(x, y + 1) + at(x + 1, y + 1);
            out.push(((sum + 2) / 4) as u8);
        }
    }
    (out, nw, nh)
}

/// Wie [`upload_r8`] fuer beliebige Formate, mit allen Verkleinerungsstufen
/// (Pixel, Breite, Hoehe), groesste zuerst.
pub(crate) unsafe fn upload_mips(
    ctx: &Context,
    pool: vk::CommandPool,
    img: &Image,
    levels: &[(&[u8], u32, u32)],
) -> Result<()> {
    let d = &ctx.device;
    let all = levels.iter().map(|l| l.0).collect::<Vec<_>>().concat();
    let mut offset = 0u64;
    let mut copies = Vec::with_capacity(levels.len());
    for (level, (px, width, height)) in levels.iter().enumerate() {
        copies.push(vk::BufferImageCopy {
            buffer_offset: offset,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image_subresource: vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: level as u32,
                base_array_layer: 0,
                layer_count: 1,
            },
            image_offset: vk::Offset3D::default(),
            image_extent: vk::Extent3D {
                width: *width,
                height: *height,
                depth: 1,
            },
        });
        offset += px.len() as u64;
    }
    let staging = Buffer::new_host(ctx, all.len() as u64, vk::BufferUsageFlags::TRANSFER_SRC)?;
    staging.write(&all);
    let result = one_shot(ctx, pool, |cmd| {
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier(
                img.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::TRANSFER_WRITE,
            )],
        );
        d.cmd_copy_buffer_to_image(
            cmd,
            staging.buffer,
            img.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &copies,
        );
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier(
                img.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::SHADER_READ,
            )],
        );
    });
    staging.destroy(d);
    result
}

pub(crate) unsafe fn set_viewport(d: &ash::Device, cmd: vk::CommandBuffer, extent: vk::Extent2D) {
    d.cmd_set_viewport(
        cmd,
        0,
        &[vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: extent.width as f32,
            height: extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }],
    );
    d.cmd_set_scissor(
        cmd,
        0,
        &[vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent,
        }],
    );
}

#[cfg(test)]
mod tests {
    use super::halve_r8;

    #[test]
    fn halving_averages_and_keeps_odd_sizes() {
        let (px, w, h) = halve_r8(&[0, 100, 200, 100, 50, 50, 50, 50], 4, 2);
        assert_eq!((px, w, h), (vec![50, 100], 2, 1));
        let (px, w, h) = halve_r8(&[8, 8, 8], 3, 1);
        assert_eq!((px, w, h), (vec![8], 1, 1));
    }
}
