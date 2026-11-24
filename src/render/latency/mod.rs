//! Verzoegerung von der Eingabe bis zum Bild: NVIDIA Reflex, AMD Anti-Lag 2, sonst Warten auf den
//! Bildschirm; dazu eine Bildbegrenzung. Alles wahlfrei ([`super::Config::low_latency`]).
//!
//! Gewartet wird in [`super::Renderer::begin_frame`], also bevor das Spiel Eingaben auswertet.

pub(crate) mod antilag;
mod limiter;
mod reflex;

use super::device::Extensions;
use super::{Renderer, Result, Swapchain};
use antilag::AntiLag;
use ash::vk;
use limiter::Limiter;
use reflex::Reflex;
use std::ffi::CStr;

/// Hoechstens so lange wird auf ein Bild gewartet (Nanosekunden)
const PATIENCE: u64 = 100_000_000;
/// Umgebungsvariable zum Testen: `wait` = Reflex und Anti-Lag nicht benutzen
const TECH_ENV: &str = "METTGINE_LATENCY";

/// Wie stark die Verzoegerung verringert wird.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LowLatency {
    #[default]
    Off,
    On,
    /// dazu hoher Takt der Grafikkarte (nur Reflex, sonst wie `On`)
    Boost,
}

/// Womit die Verzoegerung verringert wird.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencyTech {
    /// nur durch Warten, bis die Karte mit dem vorigen Bild fertig ist
    Basic,
    /// Warten, bis das vorige Bild zu sehen ist (`VK_KHR_present_wait`)
    PresentWait,
    /// NVIDIA Reflex (`VK_NV_low_latency2`)
    Reflex,
    /// AMD Anti-Lag 2 (`VK_AMD_anti_lag`)
    AntiLag,
}

/// Gemessene Zeiten eines Bildes (Millisekunden), nur mit Reflex.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LatencyReport {
    /// laufende Nummer des Bildes
    pub frame: u64,
    /// Beginn der Spielrechnung bis die Grafikkarte fertig ist
    pub total_ms: f32,
    /// Spielrechnung
    pub simulation_ms: f32,
    /// Befehle aufnehmen und abschicken
    pub submit_ms: f32,
    /// Warteschlange vor der Grafikkarte
    pub queue_ms: f32,
    /// Grafikkarte
    pub gpu_ms: f32,
}

/// Was das Geraet bekommen soll.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Plan {
    pub reflex: bool,
    pub anti_lag: bool,
    pub present_id: bool,
    pub present_wait: bool,
}

impl Plan {
    /// # Safety
    /// `pd` muss zu `instance` gehoeren.
    pub unsafe fn new(
        instance: &ash::Instance,
        pd: vk::PhysicalDevice,
        ext: &Extensions,
        want: bool,
    ) -> Self {
        if !want {
            return Self::default();
        }
        let mut id = vk::PhysicalDevicePresentIdFeaturesKHR::default();
        let mut wait = vk::PhysicalDevicePresentWaitFeaturesKHR::default();
        let mut v12 = vk::PhysicalDeviceVulkan12Features::default();
        let mut lag = antilag::Features::enabled();
        lag.anti_lag = vk::FALSE;
        let (has_id, has_wait) = (
            ext.has(ash::khr::present_id::NAME),
            ext.has(ash::khr::present_wait::NAME),
        );
        let has_lag = ext.has(antilag::NAME);
        let mut all = vk::PhysicalDeviceFeatures2::default().push_next(&mut v12);
        if has_id {
            all = all.push_next(&mut id);
        }
        if has_wait {
            all = all.push_next(&mut wait);
        }
        if has_lag {
            all = all.push_next(&mut lag);
        }
        instance.get_physical_device_features2(pd, &mut all);
        let vendor = std::env::var(TECH_ENV).as_deref() != Ok("wait");
        let present_id = has_id && id.present_id == vk::TRUE;
        Self {
            reflex: vendor
                && present_id
                && v12.timeline_semaphore == vk::TRUE
                && ext.has(ash::nv::low_latency2::NAME),
            anti_lag: vendor && has_lag && lag.anti_lag == vk::TRUE,
            present_id,
            present_wait: present_id && has_wait && wait.present_wait == vk::TRUE,
        }
    }

    pub fn extensions(&self) -> Vec<&'static CStr> {
        [
            (self.reflex, ash::nv::low_latency2::NAME),
            (self.anti_lag, antilag::NAME),
            (self.present_id, ash::khr::present_id::NAME),
            (self.present_wait, ash::khr::present_wait::NAME),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect()
    }
}

/// Geladene Funktionen eines Geraets.
pub struct Support {
    reflex: Option<ash::nv::low_latency2::Device>,
    anti_lag: Option<AntiLag>,
    wait: Option<ash::khr::present_wait::Device>,
    present_id: bool,
}

impl Support {
    /// # Safety
    /// `device` muss nach `plan` angelegt sein.
    pub(crate) unsafe fn load(instance: &ash::Instance, device: &ash::Device, plan: Plan) -> Self {
        let reflex = plan.reflex;
        Self {
            reflex: reflex.then(|| ash::nv::low_latency2::Device::new(instance, device)),
            anti_lag: plan
                .anti_lag
                .then(|| AntiLag::load(instance, device))
                .flatten(),
            wait: plan
                .present_wait
                .then(|| ash::khr::present_wait::Device::new(instance, device)),
            present_id: plan.present_id,
        }
    }

    /// Braucht die Swapchain die Reflex-Angabe?
    pub fn wants_swapchain_info(&self) -> bool {
        self.reflex.is_some()
    }

    fn tech(&self) -> LatencyTech {
        if self.reflex.is_some() {
            LatencyTech::Reflex
        } else if self.anti_lag.is_some() {
            LatencyTech::AntiLag
        } else if self.wait.is_some() {
            LatencyTech::PresentWait
        } else {
            LatencyTech::Basic
        }
    }
}

/// Zustand eines Renderers.
pub(crate) struct Latency {
    mode: LowLatency,
    /// Bilder pro Sekunde, 0 = unbegrenzt
    fps: f32,
    /// Nummer des laufenden Bildes; steigt immer
    frame: u64,
    /// auf dieser Swapchain zuletzt angezeigt (0 = noch nichts)
    shown: u64,
    /// Einstellungen gelten noch nicht fuer die Swapchain
    dirty: bool,
    /// in diesem Bild schon gewartet
    begun: bool,
    limiter: Limiter,
    /// Streamline fuehrt Schlafpunkt und Marken
    driver: Option<std::rc::Rc<super::vendor::Generation>>,
    reflex: Option<Reflex>,
}

impl Latency {
    pub fn new(ctx: &super::Context) -> Result<Self> {
        let reflex = match &ctx.low_latency.reflex {
            Some(_) => Some(Reflex::new(&ctx.device)?),
            None => None,
        };
        Ok(Self {
            mode: LowLatency::Off,
            fps: 0.0,
            frame: 0,
            shown: 0,
            dirty: false,
            begun: false,
            limiter: Limiter::default(),
            driver: None,
            reflex,
        })
    }

    /// Streamline uebernimmt Schlafpunkt und Marken.
    pub fn drive(&mut self, g: std::rc::Rc<super::vendor::Generation>) {
        self.driver = Some(g);
    }

    pub fn active(&self) -> bool {
        self.mode != LowLatency::Off || self.fps > 0.0
    }

    /// Eine neue Swapchain weiss noch nichts von den Einstellungen.
    pub fn swapchain_changed(&mut self) {
        self.dirty = true;
        self.shown = 0;
    }

    /// # Safety
    /// Die GPU darf nichts mehr benutzen.
    pub unsafe fn destroy(&self, d: &ash::Device) {
        if let Some(r) = &self.reflex {
            r.destroy(d);
        }
    }

    /// Nummer fuer Abschicken und Anzeigen, falls gebraucht.
    pub fn present_id(&self, ctx: &super::Context) -> Option<u64> {
        (ctx.low_latency.present_id && self.active()).then_some(self.frame)
    }

    /// Marke im laufenden Bild (nur Reflex).
    pub fn mark(&self, ctx: &super::Context, sc: &Swapchain, marker: vk::LatencyMarkerNV) {
        if let Some(g) = &self.driver {
            g.mark(marker);
        } else if let (Some(f), true) = (&ctx.low_latency.reflex, self.active()) {
            // SAFETY: Swapchain und Geraet leben, die Erweiterung ist eingeschaltet.
            unsafe { reflex::mark(f, sc.handle, self.frame, marker) };
        }
    }

    /// Kurz vor dem Anzeigen (nur Anti-Lag).
    pub fn before_present(&self, ctx: &super::Context) {
        if let (Some(a), true) = (&ctx.low_latency.anti_lag, self.mode != LowLatency::Off) {
            // SAFETY: Das Geraet lebt.
            unsafe { a.mark(true, self.frame, self.max_fps()) };
        }
    }

    fn max_fps(&self) -> u32 {
        self.fps.round() as u32
    }

    /// Bild gezeigt (oder verworfen): das naechste darf wieder warten.
    pub fn end(&mut self, shown: bool) {
        if shown {
            self.shown = self.frame;
        }
        self.begun = false;
    }

    /// Nummer vergeben, falls das Spiel ohne `begin_frame` zeichnet.
    pub fn ensure_begun(&mut self) {
        if !self.begun {
            self.frame += 1;
            self.begun = true;
            if let Some(g) = &self.driver {
                g.begin(self.fps, self.mode == LowLatency::Boost);
                g.mark(vk::LatencyMarkerNV::SIMULATION_START);
            }
        }
    }

    fn apply(&mut self, ctx: &super::Context, sc: &Swapchain) {
        if !std::mem::take(&mut self.dirty) {
            return;
        }
        if let Some(f) = &ctx.low_latency.reflex {
            let on = self.mode != LowLatency::Off;
            let boost = self.mode == LowLatency::Boost;
            // SAFETY: Swapchain und Geraet leben.
            let r = unsafe { reflex::set_mode(f, sc.handle, on, boost, interval_us(self.fps)) };
            if let Err(e) = r {
                crate::crash::log(&format!("Reflex liess sich nicht einstellen: {e}"));
            }
        }
        if let (Some(a), LowLatency::Off) = (&ctx.low_latency.anti_lag, self.mode) {
            // SAFETY: Das Geraet lebt.
            unsafe { a.off() };
        }
    }

    /// Schlafpunkt der Technik.
    fn sleep(&mut self, ctx: &super::Context, sc: &Swapchain) {
        let s = &ctx.low_latency;
        let on = self.mode != LowLatency::Off;
        // SAFETY: Geraet und Swapchain leben; gewartet wird mit Obergrenze.
        unsafe {
            if let (Some(f), Some(r)) = (&s.reflex, &mut self.reflex) {
                r.sleep(&ctx.device, f, sc.handle, PATIENCE * 5);
            } else if let (Some(a), true) = (&s.anti_lag, on) {
                a.mark(false, self.frame, self.max_fps());
            } else if let (Some(w), true) = (&s.wait, on && self.shown > 0) {
                // Zeitablauf ist kein Fehler (verdecktes Fenster)
                let _ = w.wait_for_present(sc.handle, self.shown, PATIENCE);
            }
        }
        // Anti-Lag kennt eine Grenze, der Treiber haelt sie im Versuch aber nicht ein
        if s.reflex.is_none() {
            self.limiter.wait(self.fps);
        }
    }
}

/// Kleinster Abstand zweier Bilder in Mikrosekunden (0 = keiner).
fn interval_us(fps: f32) -> u32 {
    if fps > 0.0 {
        (1e6 / fps) as u32
    } else {
        0
    }
}

impl Renderer {
    /// Verzoegerung verringern (Vorgabe: aus). Reflex und Anti-Lag brauchen dazu
    /// [`super::Config::low_latency`].
    pub fn set_low_latency(&mut self, mode: LowLatency) {
        if mode != self.latency.mode {
            self.latency.mode = mode;
            self.latency.dirty = true;
        }
    }

    pub fn low_latency(&self) -> LowLatency {
        self.latency.mode
    }

    /// Womit [`Renderer::set_low_latency`] auf dieser Karte arbeitet.
    pub fn latency_tech(&self) -> LatencyTech {
        self.ctx.low_latency.tech()
    }

    /// Hoechstens so viele Bilder pro Sekunde, 0 = unbegrenzt (Vorgabe).
    pub fn set_fps_limit(&mut self, fps: f32) {
        let fps = if fps.is_finite() { fps.max(0.0) } else { 0.0 };
        if fps != self.latency.fps {
            self.latency.fps = fps;
            self.latency.dirty = true;
        }
    }

    /// Vor dem Auswerten der Eingaben eines Bildes aufrufen; [`crate::app`] macht das selbst.
    /// Wartet, bis ein neues Bild sinnvoll beginnt. Ist nichts davon eingeschaltet, passiert nichts.
    pub fn begin_frame(&mut self) -> Result<()> {
        let hidden = self.window_size.0 == 0 || self.window_size.1 == 0;
        if self.latency.begun || hidden {
            return Ok(());
        }
        if self.latency.dirty && !self.latency.active() {
            self.latency.apply(&self.ctx, &self.swapchain);
        }
        if self.latency.driver.is_some() {
            self.latency.ensure_begun();
            return Ok(());
        }
        if !self.latency.active() {
            return Ok(());
        }
        self.latency.ensure_begun();
        let frame = self.frames[self.frame_index];
        if self.latency.mode != LowLatency::Off {
            // SAFETY: Der Fence gehoert zu diesem Geraet und ist abgeschickt oder frei.
            unsafe {
                let d = &self.ctx.device;
                d.wait_for_fences(&[frame.in_flight], true, u64::MAX)?;
            }
        }
        self.latency.apply(&self.ctx, &self.swapchain);
        self.latency.sleep(&self.ctx, &self.swapchain);
        let start = vk::LatencyMarkerNV::SIMULATION_START;
        self.latency.mark(&self.ctx, &self.swapchain, start);
        Ok(())
    }

    /// Die Eingaben dieses Bildes sind da; gleich rechnet das Spiel.
    pub(crate) fn input_sampled(&self) {
        if self.latency.begun {
            let m = vk::LatencyMarkerNV::INPUT_SAMPLE;
            self.latency.mark(&self.ctx, &self.swapchain, m);
        }
    }

    /// Zeiten des letzten ausgemessenen Bildes. `None` = kein Reflex oder noch nichts gemessen.
    pub fn latency_report(&self) -> Option<LatencyReport> {
        let f = self.ctx.low_latency.reflex.as_ref()?;
        if !self.latency.active() || self.latency.shown == 0 {
            return None;
        }
        // SAFETY: Swapchain und Geraet leben.
        unsafe { reflex::report(f, self.swapchain.handle) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_follows_the_limit() {
        assert_eq!(interval_us(0.0), 0);
        assert_eq!(interval_us(100.0), 10_000);
        assert_eq!(interval_us(0.5), 2_000_000);
    }

    #[test]
    fn nothing_is_requested_without_support() {
        assert!(Plan::default().extensions().is_empty());
        let all = Plan {
            reflex: true,
            anti_lag: true,
            present_id: true,
            present_wait: true,
        };
        assert_eq!(all.extensions().len(), 4);
    }
}
