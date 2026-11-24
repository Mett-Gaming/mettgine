//! Texturen auf die Grafikkarte kopieren und dort verkleinern.

use super::super::util::image_barrier;
use ash::vk;

/// Bild aus dem Zwischenpuffer holen und die Stufen auf der GPU verkleinern.
///
/// # Safety
/// `cmd` muss aufgenommen werden; `src` enthaelt RGBA-Bildpunkte, `dst` hat `mips` Stufen.
pub unsafe fn copy_image(
    d: &ash::Device,
    cmd: vk::CommandBuffer,
    src: vk::Buffer,
    dst: vk::Image,
    (width, height, mips): (u32, u32, u32),
) {
    use vk::{AccessFlags as A, ImageLayout as L, PipelineStageFlags as S};
    let color = vk::ImageAspectFlags::COLOR;
    let layer = |mip_level| vk::ImageSubresourceLayers {
        aspect_mask: color,
        mip_level,
        base_array_layer: 0,
        layer_count: 1,
    };
    let barrier = |from: S, to: S, b: vk::ImageMemoryBarrier, level: Option<u32>| {
        let mut b = b;
        if let Some(l) = level {
            b.subresource_range.base_mip_level = l;
            b.subresource_range.level_count = 1;
        }
        d.cmd_pipeline_barrier(cmd, from, to, Default::default(), &[], &[], &[b]);
    };
    let all = image_barrier(
        dst,
        color,
        L::UNDEFINED,
        L::TRANSFER_DST_OPTIMAL,
        A::empty(),
        A::TRANSFER_WRITE,
    );
    barrier(S::TOP_OF_PIPE, S::TRANSFER, all, None);
    let size = |m: u32| ((width >> m).max(1), (height >> m).max(1));
    d.cmd_copy_buffer_to_image(
        cmd,
        src,
        dst,
        L::TRANSFER_DST_OPTIMAL,
        &[vk::BufferImageCopy::default()
            .image_subresource(layer(0))
            .image_extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })],
    );
    for m in 0..mips {
        let (from, access) = (L::TRANSFER_DST_OPTIMAL, A::TRANSFER_WRITE);
        if m + 1 < mips {
            let b = image_barrier(
                dst,
                color,
                from,
                L::TRANSFER_SRC_OPTIMAL,
                access,
                A::TRANSFER_READ,
            );
            barrier(S::TRANSFER, S::TRANSFER, b, Some(m));
            let end = |m| {
                let (w, h) = size(m);
                [
                    vk::Offset3D::default(),
                    vk::Offset3D {
                        x: w as i32,
                        y: h as i32,
                        z: 1,
                    },
                ]
            };
            let blit = vk::ImageBlit::default()
                .src_subresource(layer(m))
                .src_offsets(end(m))
                .dst_subresource(layer(m + 1))
                .dst_offsets(end(m + 1));
            d.cmd_blit_image(
                cmd,
                dst,
                L::TRANSFER_SRC_OPTIMAL,
                dst,
                L::TRANSFER_DST_OPTIMAL,
                &[blit],
                vk::Filter::LINEAR,
            );
        }
        let (layout, access) = if m + 1 < mips {
            (L::TRANSFER_SRC_OPTIMAL, A::TRANSFER_READ)
        } else {
            (from, access)
        };
        let read = L::SHADER_READ_ONLY_OPTIMAL;
        let b = image_barrier(dst, color, layout, read, access, A::SHADER_READ);
        barrier(S::TRANSFER, S::FRAGMENT_SHADER, b, Some(m));
    }
}
