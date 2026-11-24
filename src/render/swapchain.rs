//! Swapchain inklusive Image-Views und einem Render-Finished-Semaphor pro Image.

use super::{context::Context, Result};
use ash::vk;

pub struct Swapchain {
    pub can_capture: bool,
    pub handle: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub render_finished: Vec<vk::Semaphore>,
}

impl Swapchain {
    pub fn new(ctx: &Context, width: u32, height: u32, old: vk::SwapchainKHR) -> Result<Self> {
        unsafe {
            let caps = ctx
                .surface_loader
                .get_physical_device_surface_capabilities(ctx.physical, ctx.surface)?;
            let formats = ctx
                .surface_loader
                .get_physical_device_surface_formats(ctx.physical, ctx.surface)?;
            // UNORM bevorzugt: der Post-Shader liefert bereits Anzeige-Farbwerte.
            let format = formats
                .iter()
                .find(|f| f.format == vk::Format::B8G8R8A8_UNORM)
                .or_else(|| {
                    formats
                        .iter()
                        .find(|f| f.format == vk::Format::R8G8B8A8_UNORM)
                })
                .copied()
                .or_else(|| formats.first().copied())
                .ok_or("Das Fenster bietet kein Bildformat an")?;

            let extent = if caps.current_extent.width != u32::MAX {
                caps.current_extent
            } else {
                vk::Extent2D {
                    width: width.clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                    height: height
                        .clamp(caps.min_image_extent.height, caps.max_image_extent.height),
                }
            };
            let mut count = caps.min_image_count + 1;
            if caps.max_image_count > 0 {
                count = count.min(caps.max_image_count);
            }
            let alpha = if caps
                .supported_composite_alpha
                .contains(vk::CompositeAlphaFlagsKHR::OPAQUE)
            {
                vk::CompositeAlphaFlagsKHR::OPAQUE
            } else {
                vk::CompositeAlphaFlagsKHR::INHERIT
            };

            let handle = ctx.swapchain_loader.create_swapchain(
                &vk::SwapchainCreateInfoKHR::default()
                    .surface(ctx.surface)
                    .min_image_count(count)
                    .image_format(format.format)
                    .image_color_space(format.color_space)
                    .image_extent(extent)
                    .image_array_layers(1)
                    .image_usage(
                        vk::ImageUsageFlags::COLOR_ATTACHMENT
                            | (caps.supported_usage_flags & vk::ImageUsageFlags::TRANSFER_SRC),
                    )
                    .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .pre_transform(caps.current_transform)
                    .composite_alpha(alpha)
                    .present_mode(vk::PresentModeKHR::FIFO)
                    .clipped(true)
                    .old_swapchain(old),
                None,
            )?;
            let images = ctx.swapchain_loader.get_swapchain_images(handle)?;
            let mut views = Vec::with_capacity(images.len());
            let mut render_finished = Vec::with_capacity(images.len());
            for &image in &images {
                views.push(
                    ctx.device.create_image_view(
                        &vk::ImageViewCreateInfo::default()
                            .image(image)
                            .view_type(vk::ImageViewType::TYPE_2D)
                            .format(format.format)
                            .subresource_range(super::util::full_range(
                                vk::ImageAspectFlags::COLOR,
                            )),
                        None,
                    )?,
                );
                render_finished.push(
                    ctx.device
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
                );
            }
            let can_capture = caps
                .supported_usage_flags
                .contains(vk::ImageUsageFlags::TRANSFER_SRC);
            Ok(Self {
                can_capture,
                handle,
                images,
                views,
                format: format.format,
                extent,
                render_finished,
            })
        }
    }

    pub fn destroy(&self, ctx: &Context) {
        unsafe {
            for &s in &self.render_finished {
                ctx.device.destroy_semaphore(s, None);
            }
            for &v in &self.views {
                ctx.device.destroy_image_view(v, None);
            }
            ctx.swapchain_loader.destroy_swapchain(self.handle, None);
        }
    }
}
