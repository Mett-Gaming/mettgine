//! Grafikspeicher in grossen Bloecken: Vulkan erlaubt nur wenige tausend Einzelanforderungen,
//! darum teilen sich Modelle und Texturen Bloecke. Puffer und Bilder liegen getrennt
//! (bufferImageGranularity).

use super::super::context::Context;
use super::super::util::{find_memory_type, full_range};
use super::super::Result;
use ash::vk;

/// Groesse eines Blocks
const BLOCK: u64 = 64 << 20;

/// Freie Stuecke eines Blocks, nach Lage sortiert.
#[derive(Clone, Debug)]
struct FreeList {
    free: Vec<(u64, u64)>,
}

impl FreeList {
    fn new(size: u64) -> Self {
        Self {
            free: vec![(0, size)],
        }
    }

    /// Erstes passendes Stueck; liefert die Lage.
    fn alloc(&mut self, size: u64, align: u64) -> Option<u64> {
        let align = align.max(1);
        for i in 0..self.free.len() {
            let (at, len) = self.free[i];
            let start = at.next_multiple_of(align);
            let end = start + size;
            if end > at + len {
                continue;
            }
            let rest = [(at, start - at), (end, at + len - end)];
            let keep = rest.into_iter().filter(|r| r.1 > 0);
            self.free.splice(i..=i, keep);
            return Some(start);
        }
        None
    }

    fn free(&mut self, at: u64, size: u64) {
        let i = self.free.partition_point(|f| f.0 < at);
        self.free.insert(i, (at, size));
        // mit Nachbarn verschmelzen
        if i + 1 < self.free.len() && at + size == self.free[i + 1].0 {
            self.free[i].1 += self.free[i + 1].1;
            self.free.remove(i + 1);
        }
        if i > 0 && self.free[i - 1].0 + self.free[i - 1].1 == at {
            self.free[i - 1].1 += self.free[i].1;
            self.free.remove(i);
        }
    }
}

struct Block {
    memory: vk::DeviceMemory,
    ty: u32,
    images: bool,
    list: FreeList,
}

/// Ein Stueck Speicher.
#[derive(Clone, Copy, Debug)]
pub struct Slice {
    block: usize,
    at: u64,
    size: u64,
}

/// Alle Bloecke.
#[derive(Default)]
pub struct Arena {
    blocks: Vec<Block>,
}

impl Arena {
    fn alloc(
        &mut self,
        ctx: &Context,
        req: &vk::MemoryRequirements,
        images: bool,
    ) -> Result<Slice> {
        let flags = vk::MemoryPropertyFlags::DEVICE_LOCAL;
        let ty = find_memory_type(&ctx.mem_props, req.memory_type_bits, flags)
            .ok_or("Kein Geraetespeicher")?;
        for (i, b) in self.blocks.iter_mut().enumerate() {
            if b.ty == ty && b.images == images {
                if let Some(at) = b.list.alloc(req.size, req.alignment) {
                    return Ok(Slice {
                        block: i,
                        at,
                        size: req.size,
                    });
                }
            }
        }
        let size = req.size.max(BLOCK);
        // SAFETY: gueltiges Geraet; freigegeben in `destroy`.
        let memory = unsafe {
            ctx.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(size)
                    .memory_type_index(ty),
                None,
            )?
        };
        let mut list = FreeList::new(size);
        let at = list.alloc(req.size, req.alignment).unwrap_or(0);
        self.blocks.push(Block {
            memory,
            ty,
            images,
            list,
        });
        Ok(Slice {
            block: self.blocks.len() - 1,
            at,
            size: req.size,
        })
    }

    fn free(&mut self, s: Slice) {
        if let Some(b) = self.blocks.get_mut(s.block) {
            b.list.free(s.at, s.size);
        }
    }

    /// Puffer im Grafikspeicher.
    pub fn buffer(
        &mut self,
        ctx: &Context,
        size: u64,
        usage: vk::BufferUsageFlags,
    ) -> Result<GpuBuffer> {
        let d = &ctx.device;
        // SAFETY: gueltiges Geraet; bei Fehlern wird der Puffer gleich wieder freigegeben.
        unsafe {
            let buffer = d.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size.max(16))
                    .usage(usage | vk::BufferUsageFlags::TRANSFER_DST),
                None,
            )?;
            let req = d.get_buffer_memory_requirements(buffer);
            let slice = match self.alloc(ctx, &req, false) {
                Ok(s) => s,
                Err(e) => {
                    d.destroy_buffer(buffer, None);
                    return Err(e);
                }
            };
            d.bind_buffer_memory(buffer, self.blocks[slice.block].memory, slice.at)?;
            Ok(GpuBuffer { buffer, slice })
        }
    }

    /// Bild im Grafikspeicher samt Ansicht ueber alle Ebenen und Stufen.
    pub fn image(&mut self, ctx: &Context, desc: &TexDesc) -> Result<GpuImage> {
        let d = &ctx.device;
        // SAFETY: wie oben.
        unsafe {
            let image = d.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(desc.format)
                    .extent(vk::Extent3D {
                        width: desc.width,
                        height: desc.height,
                        depth: 1,
                    })
                    .mip_levels(desc.mips)
                    .array_layers(desc.layers)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(desc.usage),
                None,
            )?;
            let req = d.get_image_memory_requirements(image);
            let slice = match self.alloc(ctx, &req, true) {
                Ok(s) => s,
                Err(e) => {
                    d.destroy_image(image, None);
                    return Err(e);
                }
            };
            d.bind_image_memory(image, self.blocks[slice.block].memory, slice.at)?;
            let ty = if desc.array {
                vk::ImageViewType::TYPE_2D_ARRAY
            } else {
                vk::ImageViewType::TYPE_2D
            };
            let range = |base_layer, layers| vk::ImageSubresourceRange {
                base_array_layer: base_layer,
                layer_count: layers,
                ..full_range(desc.aspect)
            };
            let make = |ty, range| {
                d.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(ty)
                        .format(desc.format)
                        .subresource_range(range),
                    None,
                )
            };
            let view = make(ty, range(0, desc.layers))?;
            let mut layers = Vec::new();
            if desc.array {
                for l in 0..desc.layers {
                    layers.push(make(vk::ImageViewType::TYPE_2D, range(l, 1))?);
                }
            }
            Ok(GpuImage {
                image,
                view,
                layers,
                mips: desc.mips,
                slice,
            })
        }
    }

    /// # Safety
    /// Die GPU darf den Puffer nicht mehr benutzen.
    pub unsafe fn drop_buffer(&mut self, d: &ash::Device, b: &GpuBuffer) {
        d.destroy_buffer(b.buffer, None);
        self.free(b.slice);
    }

    /// # Safety
    /// Die GPU darf das Bild nicht mehr benutzen.
    pub unsafe fn drop_image(&mut self, d: &ash::Device, i: &GpuImage) {
        for v in i.layers.iter().chain([&i.view]) {
            d.destroy_image_view(*v, None);
        }
        d.destroy_image(i.image, None);
        self.free(i.slice);
    }

    /// # Safety
    /// Alles daraus muss freigegeben sein.
    pub unsafe fn destroy(&mut self, d: &ash::Device) {
        for b in self.blocks.drain(..) {
            d.free_memory(b.memory, None);
        }
    }
}

/// Puffer im Grafikspeicher.
#[derive(Clone, Copy)]
pub struct GpuBuffer {
    pub buffer: vk::Buffer,
    slice: Slice,
}

/// Bild im Grafikspeicher.
#[derive(Clone)]
pub struct GpuImage {
    pub image: vk::Image,
    pub view: vk::ImageView,
    /// je Ebene eine Ansicht (nur Feldbilder)
    pub layers: Vec<vk::ImageView>,
    pub mips: u32,
    slice: Slice,
}

/// Beschreibung eines Bildes.
#[derive(Clone, Copy)]
pub struct TexDesc {
    pub width: u32,
    pub height: u32,
    pub format: vk::Format,
    pub usage: vk::ImageUsageFlags,
    pub aspect: vk::ImageAspectFlags,
    pub mips: u32,
    pub layers: u32,
    /// als Feld (mehrere Ebenen) lesen
    pub array: bool,
}

impl TexDesc {
    /// Farbbild, in das gezeichnet und aus dem gelesen wird.
    pub fn target(width: u32, height: u32, format: vk::Format) -> Self {
        Self {
            width,
            height,
            format,
            usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            aspect: vk::ImageAspectFlags::COLOR,
            mips: 1,
            layers: 1,
            array: false,
        }
    }

    /// Tiefenbild, das auch gelesen wird.
    pub fn depth(width: u32, height: u32) -> Self {
        Self {
            usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            aspect: vk::ImageAspectFlags::DEPTH,
            ..Self::target(width, height, super::super::DEPTH_FORMAT)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freed_memory_is_reused_and_pieces_merge() {
        let mut f = FreeList::new(1000);
        let a = f.alloc(100, 16).unwrap();
        let b = f.alloc(100, 64).unwrap();
        let c = f.alloc(100, 1).unwrap();
        assert_eq!((a, b % 64), (0, 0));
        assert!(f.alloc(900, 1).is_none());
        f.free(b, 100);
        f.free(a, 100);
        f.free(c, 100);
        assert_eq!(f.free, vec![(0, 1000)], "alles wieder ein Stueck");
        assert_eq!(f.alloc(1000, 256), Some(0));
    }
}
