//! Kleine Helfer fuer Buffer, Images und Barrieren.

use super::{context::Context, Result};
use ash::vk;

pub fn find_memory_type(
    props: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    flags: vk::MemoryPropertyFlags,
) -> Option<u32> {
    (0..props.memory_type_count).find(|&i| {
        type_bits & (1 << i) != 0
            && props.memory_types[i as usize]
                .property_flags
                .contains(flags)
    })
}

pub fn full_range(aspect: vk::ImageAspectFlags) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: aspect,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

pub fn image_barrier(
    image: vk::Image,
    aspect: vk::ImageAspectFlags,
    old: vk::ImageLayout,
    new: vk::ImageLayout,
    src_access: vk::AccessFlags,
    dst_access: vk::AccessFlags,
) -> vk::ImageMemoryBarrier<'static> {
    vk::ImageMemoryBarrier::default()
        .image(image)
        .old_layout(old)
        .new_layout(new)
        .src_access_mask(src_access)
        .dst_access_mask(dst_access)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .subresource_range(full_range(aspect))
}

/// Dauerhaft gemappter, host-sichtbarer Buffer.
pub struct Buffer {
    pub buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    mapped: *mut u8,
    pub size: u64,
}

impl Buffer {
    pub fn new_host(ctx: &Context, size: u64, usage: vk::BufferUsageFlags) -> Result<Self> {
        unsafe {
            let d = &ctx.device;
            let buffer = d.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let req = d.get_buffer_memory_requirements(buffer);
            let ty = find_memory_type(
                &ctx.mem_props,
                req.memory_type_bits,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            )
            .ok_or("Kein host-sichtbarer Speicher")?;
            let memory = d.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(ty),
                None,
            )?;
            d.bind_buffer_memory(buffer, memory, 0)?;
            let mapped =
                d.map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())? as *mut u8;
            Ok(Self {
                buffer,
                memory,
                mapped,
                size,
            })
        }
    }

    pub fn write(&self, data: &[u8]) {
        assert!(data.len() as u64 <= self.size);
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), self.mapped, data.len()) }
    }

    pub fn read(&self, len: usize) -> Vec<u8> {
        assert!(len as u64 <= self.size);
        let mut out = vec![0u8; len];
        unsafe { std::ptr::copy_nonoverlapping(self.mapped, out.as_mut_ptr(), len) }
        out
    }

    pub fn destroy(&self, device: &ash::Device) {
        unsafe {
            device.unmap_memory(self.memory);
            device.destroy_buffer(self.buffer, None);
            device.free_memory(self.memory, None);
        }
    }
}

pub struct Image {
    pub image: vk::Image,
    memory: vk::DeviceMemory,
    pub view: vk::ImageView,
}

impl Image {
    pub fn new(
        ctx: &Context,
        width: u32,
        height: u32,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        aspect: vk::ImageAspectFlags,
    ) -> Result<Self> {
        unsafe {
            let d = &ctx.device;
            let image = d.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(usage)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )?;
            let req = d.get_image_memory_requirements(image);
            let ty = find_memory_type(
                &ctx.mem_props,
                req.memory_type_bits,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )
            .ok_or("Kein Geraetespeicher fuer Image")?;
            let memory = d.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(ty),
                None,
            )?;
            d.bind_image_memory(image, memory, 0)?;
            let view = d.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(full_range(aspect)),
                None,
            )?;
            Ok(Self {
                image,
                memory,
                view,
            })
        }
    }

    pub fn destroy(&self, device: &ash::Device) {
        unsafe {
            device.destroy_image_view(self.view, None);
            device.destroy_image(self.image, None);
            device.free_memory(self.memory, None);
        }
    }
}
