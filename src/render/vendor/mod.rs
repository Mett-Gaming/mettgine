//! Hochskalieren mit Techniken der Hersteller (FSR, XeSS, DLSS). Alles wahlfrei: eigene
//! Cargo-Features, DLLs erst zur Laufzeit, gewuenscht mit [`super::Config::upscalers`]. Fehlt etwas,
//! bleibt die eigene Technik (TAA).

#[cfg(feature = "fsr")]
mod fsr;
mod library;
#[cfg(feature = "streamline")]
mod streamline;
#[cfg(feature = "xess")]
mod xess;

use super::context::Context;
use super::Result;
use ash::vk;
use glam::Vec2;

/// Womit die Szene hochskaliert und geglaettet wird.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Upscaler {
    /// eigenes TAA (bzw. was `Features::anti_alias` sagt)
    #[default]
    Own,
    /// AMD FSR
    Fsr,
    /// Intel XeSS
    Xess,
    /// NVIDIA DLSS
    Dlss,
}

/// Uebliche Stufen; der Wert ist fuer `Features::render_scale` gedacht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// volle Aufloesung, nur Kantenglaettung (bei NVIDIA: DLAA)
    Native,
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
}

impl Quality {
    pub fn scale(self) -> f32 {
        match self {
            Self::Native => 1.0,
            Self::Quality => 1.0 / 1.5,
            Self::Balanced => 1.0 / 1.7,
            Self::Performance => 0.5,
            Self::UltraPerformance => 1.0 / 3.0,
        }
    }
}

/// Ein Bild fuer die Technik.
#[derive(Clone, Copy)]
#[allow(dead_code)] // nicht jede Technik braucht alles
pub(crate) struct Picture {
    pub image: vk::Image,
    pub view: vk::ImageView,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
}

/// Zusatzbilder fuer das Entrauschen (Ray Reconstruction).
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct Guides {
    /// Farbe ohne Licht, Anteil der Spiegelung
    pub albedo: Picture,
    pub specular: Picture,
    /// Normale (Welt, mit Vorzeichen), Rauheit
    pub normal: Picture,
}

/// Ein Auftrag. Eingaben liegen in `SHADER_READ_ONLY_OPTIMAL`, das Ziel in `GENERAL`.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct Job {
    pub cmd: vk::CommandBuffer,
    /// Nummer des Bildes im Flug
    pub fi: usize,
    pub color: Picture,
    pub depth: Picture,
    /// Verschiebung seit dem letzten Bild (jetzt minus vorher, Anteil am Bild, y nach unten)
    pub motion: Picture,
    /// 0..1: Stellen ohne verlaessliche Bewegung
    pub reactive: Option<Picture>,
    pub output: Picture,
    /// Szene ist verrauscht: diese Bilder helfen beim Entrauschen
    pub guides: Option<Guides>,
    /// Versatz der Kamera: so viele Bildpunkte wandert das Bild nach rechts und unten
    pub jitter: Vec2,
    /// vergangene Zeit (Sekunden)
    pub dt: f32,
    pub camera: super::Camera,
    /// Lage der Kamera im letzten Bild (Projektion mal Ansicht)
    pub before: glam::Mat4,
    pub reset: bool,
}

/// Eine Technik.
pub(crate) trait Backend {
    /// Name samt Version.
    fn name(&self) -> String;

    /// Kann die Technik auch entrauschen?
    fn denoises(&self) -> bool {
        false
    }

    /// Befehle aufnehmen. Bei geaenderten Groessen darf gewartet werden.
    ///
    /// # Safety
    /// `job.cmd` wird ausserhalb eines Durchgangs aufgenommen, alle Bilder gehoeren zu `ctx`.
    unsafe fn evaluate(&mut self, ctx: &Context, job: &Job) -> Result<()>;

    /// # Safety
    /// Die GPU ist untaetig.
    unsafe fn destroy(&mut self, ctx: &Context);
}

/// Was vor dem Anlegen von Instanz und Geraet geladen wird.
#[derive(Default)]
pub(crate) struct Vendors {
    #[cfg(feature = "fsr")]
    fsr: Option<fsr::Api>,
    #[cfg(feature = "xess")]
    xess: Option<xess::Api>,
    #[cfg(feature = "streamline")]
    streamline: Option<streamline::Api>,
}

impl Vendors {
    pub fn load(wanted: &[Upscaler]) -> Self {
        let _ = wanted;
        Self {
            #[cfg(feature = "fsr")]
            fsr: wanted
                .contains(&Upscaler::Fsr)
                .then(fsr::Api::load)
                .flatten(),
            #[cfg(feature = "xess")]
            xess: wanted
                .contains(&Upscaler::Xess)
                .then(xess::Api::load)
                .flatten(),
            #[cfg(feature = "streamline")]
            streamline: wanted
                .contains(&Upscaler::Dlss)
                .then(streamline::Api::load)
                .flatten(),
        }
    }

    /// Ist etwas da, das ein vollstaendig ausgestattetes Geraet braucht?
    pub fn any(&self) -> bool {
        let mut any = false;
        #[cfg(feature = "fsr")]
        {
            any |= self.fsr.is_some();
        }
        #[cfg(feature = "xess")]
        {
            any |= self.xess.is_some();
        }
        #[cfg(feature = "streamline")]
        {
            any |= self.streamline.is_some();
        }
        any
    }

    /// Bibliothek, aus der Vulkan geladen werden soll (None = wie ueblich).
    pub fn vulkan(&self) -> Option<std::path::PathBuf> {
        #[cfg(feature = "streamline")]
        if self.streamline.is_some() {
            return streamline::vulkan_path();
        }
        None
    }

    /// Vor dem Freigeben des Geraets, wenn alle Techniken weg sind.
    pub fn stop(&mut self) {
        #[cfg(feature = "streamline")]
        if let Some(s) = self.streamline.take() {
            s.stop();
        }
    }

    /// Zusaetzliche Erweiterungen der Instanz.
    pub fn instance_extensions(&self) -> Vec<std::ffi::CString> {
        let mut list: Vec<std::ffi::CString> = Vec::new();
        #[cfg(feature = "xess")]
        if let Some(x) = &self.xess {
            list.extend(x.instance_extensions());
        }
        list.clone()
    }

    /// Zusaetzliche Erweiterungen des Geraets.
    ///
    /// # Safety
    /// `pd` gehoert zu `instance`.
    pub unsafe fn device_extensions(
        &self,
        instance: &ash::Instance,
        pd: vk::PhysicalDevice,
    ) -> Vec<std::ffi::CString> {
        let _ = (instance, pd);
        let mut list: Vec<std::ffi::CString> = Vec::new();
        #[cfg(feature = "xess")]
        if let Some(x) = &self.xess {
            list.extend(x.device_extensions(instance, pd));
        }
        list.clone()
    }

    /// Kette der Geraete-Faehigkeiten um Noetiges ergaenzen; `chain` beginnt mit
    /// `VkPhysicalDeviceFeatures2`.
    ///
    /// # Safety
    /// `chain` ist eine gueltige, beschreibbare Kette.
    pub unsafe fn patch_features(
        &self,
        instance: &ash::Instance,
        pd: vk::PhysicalDevice,
        chain: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void {
        let _ = (instance, pd);
        #[cfg(feature = "xess")]
        if let Some(x) = &self.xess {
            return x.patch_features(instance, pd, chain);
        }
        chain
    }

    /// Technik fuer dieses Geraet anlegen. `None` = geht hier nicht.
    pub fn create(&self, ctx: &Context, which: Upscaler) -> Option<Box<dyn Backend>> {
        let _ = ctx;
        match which {
            #[cfg(feature = "fsr")]
            Upscaler::Fsr => self.fsr.as_ref().and_then(|a| a.create(ctx)),
            #[cfg(feature = "xess")]
            Upscaler::Xess => self.xess.as_ref().and_then(|a| a.create(ctx)),
            #[cfg(feature = "streamline")]
            Upscaler::Dlss => self.streamline.as_ref().and_then(|a| a.create(ctx)),
            _ => None,
        }
    }
}

/// Anzahl verschiedener Kamera-Versaetze fuer ein Verhaeltnis von Ziel zu Szene.
pub(crate) fn jitter_phases(scene: u32, out: u32) -> u64 {
    let k = out.max(1) as f32 / scene.max(1) as f32;
    (8.0 * k * k).ceil().clamp(8.0, 128.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_grows_with_the_upscale_factor() {
        assert_eq!(jitter_phases(1920, 1920), 8);
        assert_eq!(jitter_phases(960, 1920), 32);
        assert_eq!(jitter_phases(640, 1920), 72);
    }

    #[test]
    fn nothing_is_loaded_unless_wanted() {
        let v = Vendors::load(&[]);
        assert!(!v.any() && v.instance_extensions().is_empty());
        assert!((Quality::Quality.scale() - 0.6667).abs() < 0.001);
    }
}
