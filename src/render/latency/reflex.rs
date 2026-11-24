//! NVIDIA Reflex ueber `VK_NV_low_latency2`, ohne fremde Bibliothek.

use super::super::Result;
use super::LatencyReport;
use ash::nv::low_latency2::Device;
use ash::prelude::VkResult;
use ash::vk;

/// So viele Bilder liefert der Treiber hoechstens
const REPORTS: usize = 64;

/// Schlafpunkt: der Treiber signalisiert eine Zeitachse, sobald das naechste Bild beginnen soll.
pub(super) struct Reflex {
    timeline: vk::Semaphore,
    value: u64,
}

impl Reflex {
    pub fn new(d: &ash::Device) -> Result<Self> {
        let mut kind = vk::SemaphoreTypeCreateInfo::default()
            .semaphore_type(vk::SemaphoreType::TIMELINE)
            .initial_value(0);
        let info = vk::SemaphoreCreateInfo::default().push_next(&mut kind);
        // SAFETY: Das Geraet wurde mit Zeitachsen-Semaphoren angelegt.
        let timeline = unsafe { d.create_semaphore(&info, None)? };
        Ok(Self { timeline, value: 0 })
    }

    /// # Safety
    /// Swapchain und Geraet muessen leben.
    pub unsafe fn sleep(
        &mut self,
        d: &ash::Device,
        f: &Device,
        sc: vk::SwapchainKHR,
        patience: u64,
    ) {
        self.value += 1;
        let info = vk::LatencySleepInfoNV::default()
            .signal_semaphore(self.timeline)
            .value(self.value);
        if f.latency_sleep(sc, &info).is_ok() {
            let (semaphores, values) = ([self.timeline], [self.value]);
            let wait = vk::SemaphoreWaitInfo::default()
                .semaphores(&semaphores)
                .values(&values);
            let _ = d.wait_semaphores(&wait, patience);
        }
    }

    /// # Safety
    /// Niemand wartet mehr auf die Zeitachse.
    pub unsafe fn destroy(&self, d: &ash::Device) {
        d.destroy_semaphore(self.timeline, None);
    }
}

/// # Safety
/// Swapchain und Geraet muessen leben.
pub(super) unsafe fn set_mode(
    f: &Device,
    sc: vk::SwapchainKHR,
    on: bool,
    boost: bool,
    interval_us: u32,
) -> VkResult<()> {
    let info = vk::LatencySleepModeInfoNV::default()
        .low_latency_mode(on)
        .low_latency_boost(boost)
        .minimum_interval_us(interval_us);
    f.set_latency_sleep_mode(sc, Some(&info))
}

/// # Safety
/// Swapchain und Geraet muessen leben.
pub(super) unsafe fn mark(
    f: &Device,
    sc: vk::SwapchainKHR,
    frame: u64,
    marker: vk::LatencyMarkerNV,
) {
    let info = vk::SetLatencyMarkerInfoNV::default()
        .present_id(frame)
        .marker(marker);
    f.set_latency_marker(sc, &info);
}

/// Neuestes vollstaendig gemessenes Bild.
///
/// # Safety
/// Swapchain und Geraet muessen leben.
pub(super) unsafe fn report(f: &Device, sc: vk::SwapchainKHR) -> Option<LatencyReport> {
    let mut list = [vk::LatencyTimingsFrameReportNV::default(); REPORTS];
    let mut info = vk::GetLatencyMarkerInfoNV::default().timings(&mut list);
    f.get_latency_timings(sc, &mut info);
    let n = (info.timing_count as usize).min(REPORTS);
    list[..n]
        .iter()
        .filter(|t| t.gpu_render_end_time_us > 0 && t.sim_start_time_us > 0)
        .max_by_key(|t| t.present_id)
        .map(times)
}

fn times(t: &vk::LatencyTimingsFrameReportNV) -> LatencyReport {
    let ms = |a: u64, b: u64| b.saturating_sub(a) as f32 / 1000.0;
    LatencyReport {
        frame: t.present_id,
        total_ms: ms(t.sim_start_time_us, t.gpu_render_end_time_us),
        simulation_ms: ms(t.sim_start_time_us, t.sim_end_time_us),
        submit_ms: ms(t.render_submit_start_time_us, t.render_submit_end_time_us),
        queue_ms: ms(
            t.os_render_queue_start_time_us,
            t.os_render_queue_end_time_us,
        ),
        gpu_ms: ms(t.gpu_render_start_time_us, t.gpu_render_end_time_us),
    }
}
