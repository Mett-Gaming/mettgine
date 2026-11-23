//! Bilder im Flug, Abschicken, Anzeigen und Bildschirmfotos, gemeinsam fuer `render` und `scene`.

use super::context::Context;
use super::swapchain::Swapchain;
use super::transfer::write_bmp;
use super::util::Buffer;
use super::Result;
use ash::vk;
use std::path::PathBuf;

/// Befehlspuffer und Synchronisation eines Bildes im Flug.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    pub cmd: vk::CommandBuffer,
    pub image_available: vk::Semaphore,
    pub in_flight: vk::Fence,
}

/// Befehls-Pool und `count` Bilder im Flug anlegen.
///
/// # Safety
/// `ctx` muss ein gueltiges Geraet enthalten.
pub(crate) unsafe fn create_frames(
    ctx: &Context,
    count: usize,
) -> Result<(vk::CommandPool, Vec<Frame>)> {
    let d = &ctx.device;
    let pool = d.create_command_pool(
        &vk::CommandPoolCreateInfo::default()
            .queue_family_index(ctx.queue_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
        None,
    )?;
    let cmds = d.allocate_command_buffers(
        &vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(count as u32),
    )?;
    let mut frames = Vec::with_capacity(count);
    for cmd in cmds {
        frames.push(Frame {
            cmd,
            image_available: d.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
            in_flight: d.create_fence(
                &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )?,
        });
    }
    Ok((pool, frames))
}

/// # Safety
/// Die GPU darf keins der Bilder mehr benutzen.
pub(crate) unsafe fn destroy_frames(d: &ash::Device, frames: &[Frame]) {
    for f in frames {
        d.destroy_semaphore(f.image_available, None);
        d.destroy_fence(f.in_flight, None);
    }
}

/// Wartet auf das Bild im Flug und holt das naechste Swapchain-Bild. `None` = die Swapchain
/// passt nicht mehr und muss neu gebaut werden (dieses Bild faellt aus).
///
/// # Safety
/// `frame` und `swapchain` muessen zu `ctx` gehoeren.
pub(crate) unsafe fn acquire(
    ctx: &Context,
    swapchain: &Swapchain,
    frame: &Frame,
) -> Result<Option<u32>> {
    ctx.device
        .wait_for_fences(&[frame.in_flight], true, u64::MAX)?;
    match ctx.swapchain_loader.acquire_next_image(
        swapchain.handle,
        u64::MAX,
        frame.image_available,
        vk::Fence::null(),
    ) {
        Ok((i, _)) => {
            ctx.device.reset_fences(&[frame.in_flight])?;
            Ok(Some(i))
        }
        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Schickt das Bild ab und zeigt es an. `true` = Swapchain muss neu gebaut werden.
///
/// # Safety
/// Der Befehlspuffer von `frame` muss fertig aufgenommen sein.
pub(crate) unsafe fn submit_and_present(
    ctx: &Context,
    swapchain: &Swapchain,
    frame: &Frame,
    image_index: u32,
) -> Result<bool> {
    let wait = [frame.image_available];
    let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
    let cmds = [frame.cmd];
    let signal = [swapchain.render_finished[image_index as usize]];
    ctx.device.queue_submit(
        ctx.queue,
        &[vk::SubmitInfo::default()
            .wait_semaphores(&wait)
            .wait_dst_stage_mask(&stages)
            .command_buffers(&cmds)
            .signal_semaphores(&signal)],
        frame.in_flight,
    )?;
    let swapchains = [swapchain.handle];
    let indices = [image_index];
    match ctx.swapchain_loader.queue_present(
        ctx.queue,
        &vk::PresentInfoKHR::default()
            .wait_semaphores(&signal)
            .swapchains(&swapchains)
            .image_indices(&indices),
    ) {
        Ok(suboptimal) => Ok(suboptimal),
        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Ok(true),
        Err(e) => Err(e.into()),
    }
}

/// Bildschirmfoto: gewuenschter Pfad und wiederverwendeter Zielpuffer.
#[derive(Default)]
pub(crate) struct Capture {
    path: Option<PathBuf>,
    buf: Option<Buffer>,
}

impl Capture {
    /// Das naechste fertige Bild unter `path` speichern.
    pub fn request(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    /// Zielpuffer fuer die Kopie, falls ein Foto gewuenscht und moeglich ist.
    pub fn prepare(&mut self, ctx: &Context, sc: &Swapchain) -> Result<Option<vk::Buffer>> {
        if self.path.is_none() || !sc.can_capture {
            return Ok(None);
        }
        let bytes = (sc.extent.width * sc.extent.height * 4) as u64;
        if self.buf.as_ref().is_none_or(|b| b.size < bytes) {
            if let Some(old) = self.buf.take() {
                old.destroy(&ctx.device);
            }
            let usage = vk::BufferUsageFlags::TRANSFER_DST;
            self.buf = Some(Buffer::new_host(ctx, bytes, usage)?);
        }
        Ok(self.buf.as_ref().map(|b| b.buffer))
    }

    /// Foto auslesen und speichern (wartet einmal auf die GPU, nur bei Fotos).
    pub fn finish(&mut self, ctx: &Context, sc: &Swapchain) -> Result<()> {
        // SAFETY: Warten, damit die Kopie fertig im Puffer liegt.
        unsafe { ctx.device.device_wait_idle()? };
        if let (Some(path), Some(buf)) = (self.path.take(), &self.buf) {
            let (w, h) = (sc.extent.width, sc.extent.height);
            let px = buf.read((w * h * 4) as usize);
            let rgba = sc.format == vk::Format::R8G8B8A8_UNORM;
            if let Err(e) = write_bmp(&path, w, h, &px, rgba) {
                crate::crash::log(&format!("Bildschirmfoto fehlgeschlagen: {e}"));
            }
        }
        Ok(())
    }

    pub fn destroy(&self, d: &ash::Device) {
        if let Some(b) = &self.buf {
            b.destroy(d);
        }
    }
}
