//! Zuschaltbare Bildqualitaet (Kantenglaettung, Schrift-Verkleinerung), Aussortieren, Messwerte.
//! Alles Sichtbare ist zu Beginn aus: Spiele sehen ohne eigenes Zutun aus wie vorher.

use super::pipelines::create_scene_pipelines;
use super::*;
use crate::particles::ParticleInst;
use crate::profile::FrameStats;

/// Anzahl Abtastungen als Vulkan-Wert: groesster erlaubter Wert, der `wanted` nicht uebersteigt.
pub(super) fn clamp_samples(wanted: u32, supported: vk::SampleCountFlags) -> vk::SampleCountFlags {
    [
        (8, vk::SampleCountFlags::TYPE_8),
        (4, vk::SampleCountFlags::TYPE_4),
        (2, vk::SampleCountFlags::TYPE_2),
    ]
    .into_iter()
    .find(|(n, f)| *n <= wanted && supported.contains(*f))
    .map_or(vk::SampleCountFlags::TYPE_1, |(_, f)| f)
}

impl SceneRenderer {
    /// Kantenglaettung (MSAA): 1 = aus (Vorgabe), 2, 4 oder 8 Abtastungen. Kann die Karte den
    /// Wert nicht, gilt der naechstkleinere; siehe [`SceneRenderer::msaa`].
    pub fn set_msaa(&mut self, samples: u32) {
        // SAFETY: Instanz und Geraet leben so lange wie der Renderer.
        let limits = unsafe {
            self.ctx
                .instance
                .get_physical_device_properties(self.ctx.physical)
                .limits
        };
        let supported =
            limits.framebuffer_color_sample_counts & limits.framebuffer_depth_sample_counts;
        let s = clamp_samples(samples, supported);
        if s != self.wanted_samples {
            self.wanted_samples = s;
            self.needs_recreate = true;
        }
    }

    /// Eingestellte Abtastungen pro Bildpunkt.
    pub fn msaa(&self) -> u32 {
        self.wanted_samples.as_raw()
    }

    /// Schrift mit vorverkleinerten Stufen (Mipmaps): kleine Schrift flimmert weniger und wird
    /// etwas weicher. Vorgabe: aus.
    pub fn set_font_mipmaps(&mut self, on: bool) {
        if on != self.font_mipmaps {
            self.font_mipmaps = on;
            self.needs_recreate = true;
        }
    }

    /// Nicht Sichtbares aussortieren (Vorgabe: an; das Bild ist in beiden Faellen gleich).
    pub fn set_culling(&mut self, on: bool) {
        self.culling = on;
    }

    /// Partikel fuer das naechste `draw` (siehe `particles::Particles::instances`). Mehrere
    /// Aufrufe haengen an; nach dem Bild ist die Liste leer.
    pub fn queue_particles(&mut self, list: &[ParticleInst]) {
        self.particles.extend_from_slice(list);
    }

    /// Messwerte des letzten Bildes (Auswertung: `profile::Meter`).
    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    /// Neue Abtastung uebernehmen: Pipelines der Szene neu bauen (die GPU ist untaetig).
    pub(super) fn apply_samples(&mut self) -> Result<()> {
        if self.wanted_samples == self.samples {
            return Ok(());
        }
        // SAFETY: aufgerufen nach device_wait_idle; alte Pipelines werden genau einmal zerstoert.
        unsafe {
            let d = &self.ctx.device;
            let new = create_scene_pipelines(d, self.pipes.scene_layout, self.wanted_samples)?;
            let old = std::mem::replace(&mut self.pipes.scene, new);
            for p in old.all() {
                d.destroy_pipeline(p, None);
            }
        }
        self.samples = self.wanted_samples;
        Ok(())
    }
}
