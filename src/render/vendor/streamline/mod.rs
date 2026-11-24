//! NVIDIA Streamline (`sl.interposer.dll` und Begleiter): DLSS, DLAA und Ray Reconstruction. Die
//! Vulkan-Einsprungpunkte kommen dann aus dem Interposer, er richtet Geraet und Queues mit ein.

mod abi;

use super::super::context::Context;
use super::super::Result;
use super::library::{self, Library};
use super::{Backend, Job, Picture};
use abi::*;
use ash::vk;
use std::ffi::{c_char, c_void, CStr, CString};
use std::os::raw::c_int;
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

const DLL: &str = "sl.interposer.dll";
/// Kennung der Engine bei NVIDIA (frei gewaehlt, nicht personenbezogen)
const PROJECT: &CStr = c"8d1f0f7c-6f0b-4a52-9f0e-3c5a9b7a1e42";
const VIEWPORT: u32 = 1;
/// Kantenlaengen-Verhaeltnis (Ziel / Szene) und Modus
const MODES: [(f32, u32); 5] = [(1.0, 6), (1.5, 3), (1.72, 2), (2.0, 1), (3.0, 4)];

/// Streamline laesst sich je Prozess nur einmal benutzen.
static IN_USE: AtomicBool = AtomicBool::new(false);

/// Geladene Funktionen.
#[derive(Clone, Copy)]
pub struct Api {
    shutdown: Shutdown,
    is_supported: IsSupported,
    evaluate: Evaluate,
    free: FreeResources,
    set_tags: SetTags,
    set_constants: SetConstants,
    version: GetVersion,
    function: GetFunction,
    token: NewToken,
}

unsafe extern "C" fn on_log(kind: u32, text: *const c_char) {
    // 0 = Info, 1 = Warnung, 2 = Fehler
    if kind >= 2 && !text.is_null() {
        let text = CStr::from_ptr(text).to_string_lossy();
        crate::crash::log(&format!("Streamline: {}", text.trim_end()));
    }
}

/// Pfad der Bibliothek, aus der Vulkan geladen werden soll.
pub fn vulkan_path() -> Option<PathBuf> {
    library::find(DLL)
}

impl Api {
    /// Laedt und startet Streamline; muss vor dem Anlegen der Vulkan-Instanz geschehen.
    pub fn load() -> Option<Self> {
        let path = vulkan_path()?;
        if IN_USE.swap(true, Ordering::SeqCst) {
            crate::crash::log("Streamline ist schon in Benutzung");
            return None;
        }
        let api = Self::start(&path);
        if api.is_none() {
            IN_USE.store(false, Ordering::SeqCst);
        }
        api
    }

    fn start(path: &std::path::Path) -> Option<Self> {
        let lib = Library::load(DLL)?;
        // SAFETY: Typen nach `sl_core_api.h`.
        unsafe {
            let init: Init = lib.get(c"slInit")?;
            let api = Self {
                shutdown: lib.get(c"slShutdown")?,
                is_supported: lib.get(c"slIsFeatureSupported")?,
                evaluate: lib.get(c"slEvaluateFeature")?,
                free: lib.get(c"slFreeResources")?,
                set_tags: lib.get(c"slSetTagForFrame")?,
                set_constants: lib.get(c"slSetConstants")?,
                version: lib.get(c"slGetFeatureVersion")?,
                function: lib.get(c"slGetFeatureFunction")?,
                token: lib.get(c"slGetNewFrameToken")?,
            };
            let features = [FEATURE_DLSS, FEATURE_RR];
            let folder = path.parent()?.to_string_lossy().into_owned();
            let folder: Vec<u16> = folder.encode_utf16().chain([0]).collect();
            let folders = [folder.as_ptr()];
            let mut p = Preferences::new();
            p.plugin_paths = folders.as_ptr();
            p.plugin_path_count = 1;
            p.log = Some(on_log);
            p.features = features.as_ptr();
            p.feature_count = features.len() as u32;
            p.project = PROJECT.as_ptr();
            let version = CString::new(env!("CARGO_PKG_VERSION")).ok()?;
            p.engine_version = version.as_ptr();
            match init(&p, SDK_VERSION) {
                OK => Some(api),
                r => {
                    crate::crash::log(&format!("Streamline startet nicht ({r})"));
                    None
                }
            }
        }
    }

    unsafe fn feature_function<F: Copy>(&self, feature: u32, name: &CStr) -> Option<F> {
        let mut p: *mut c_void = ptr::null_mut();
        let r = (self.function)(feature, name.as_ptr(), &mut p);
        if r != OK || p.is_null() {
            return None;
        }
        Some(std::mem::transmute_copy(&p))
    }

    pub fn create(&self, ctx: &Context) -> Option<Box<dyn Backend>> {
        let adapter = AdapterInfo::new(ctx.physical);
        // SAFETY: Geraet und Karte sind gueltig, Typen nach `sl_dlss.h`.
        unsafe {
            let r = (self.is_supported)(FEATURE_DLSS, &adapter);
            if r != OK {
                crate::crash::log(&format!("DLSS: auf dieser Karte nicht moeglich ({r})"));
                return None;
            }
            let set_dlss = self.feature_function(FEATURE_DLSS, c"slDLSSSetOptions")?;
            let rr = (self.is_supported)(FEATURE_RR, &adapter) == OK;
            let set_rr = match rr {
                true => self.feature_function(FEATURE_RR, c"slDLSSDSetOptions"),
                false => None,
            };
            let mut v = FeatureVersion::new();
            (self.version)(FEATURE_DLSS, &mut v);
            let [a, b, c] = v.ngx;
            Some(Box::new(Dlss {
                api: *self,
                set_dlss,
                set_rr,
                viewport: Viewport::new(VIEWPORT),
                frame: 0,
                name: format!("DLSS {a}.{b}.{c}"),
                used: [false; 2],
            }))
        }
    }

    /// Vor dem Freigeben des Geraets.
    pub fn stop(&self) {
        // SAFETY: Die GPU ist untaetig, danach wird nichts mehr aufgerufen.
        unsafe { (self.shutdown)() };
    }
}

struct Dlss {
    api: Api,
    set_dlss: SetDlss,
    set_rr: Option<SetRr>,
    viewport: Viewport,
    frame: u32,
    name: String,
    /// DLSS, Ray Reconstruction haben Speicher belegt
    used: [bool; 2],
}

fn mode(render: [u32; 2], out: [u32; 2]) -> u32 {
    let k = out[0] as f32 / render[0].max(1) as f32;
    let best = MODES
        .iter()
        .min_by(|a, b| (a.0 - k).abs().total_cmp(&(b.0 - k).abs()));
    best.map_or(1, |m| m.1)
}

fn constants(job: &Job) -> Constants {
    let cam = &job.camera;
    let (proj, view) = (cam.proj(), cam.view);
    let now = proj * view;
    let world = view.inverse();
    let mut c = Constants::new();
    // Spalten einer Matrix fuer Spaltenvektoren sind die Zeilen einer fuer Zeilenvektoren
    c.view_to_clip = proj.to_cols_array_2d();
    c.clip_to_view = proj.inverse().to_cols_array_2d();
    let to_prev = job.before * now.inverse();
    c.clip_to_prev = to_prev.to_cols_array_2d();
    c.prev_to_clip = to_prev.inverse().to_cols_array_2d();
    c.jitter = job.jitter.to_array();
    // erwartet wird vorher minus jetzt, Anteil am Bild
    c.motion_scale = [-1.0, -1.0];
    c.pos = cam.pos.to_array();
    c.right = world.x_axis.truncate().to_array();
    c.up = world.y_axis.truncate().to_array();
    c.forward = (-world.z_axis.truncate()).to_array();
    (c.near, c.far) = (cam.near, cam.far);
    (c.fov, c.aspect) = (cam.fov_y, cam.aspect);
    c.reset = job.reset as i8;
    c
}

fn check(what: &str, r: c_int) -> Result<()> {
    match r {
        OK => Ok(()),
        _ => Err(format!("DLSS: {what} ({r})").into()),
    }
}

impl Backend for Dlss {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn denoises(&self) -> bool {
        self.set_rr.is_some()
    }

    unsafe fn evaluate(&mut self, _ctx: &Context, job: &Job) -> Result<()> {
        let size = |p: &Picture| [p.extent.width, p.extent.height];
        let (render, out) = (size(&job.color), size(&job.output));
        self.frame = self.frame.wrapping_add(1);
        let mut token: Token = ptr::null_mut();
        check("Bildnummer", (self.api.token)(&mut token, &self.frame))?;
        let c = constants(job);
        check(
            "Kamera",
            (self.api.set_constants)(&c, token, &self.viewport),
        )?;
        let m = mode(render, out);
        let guides = job.guides.filter(|_| self.set_rr.is_some());
        let feature = match (guides, self.set_rr) {
            (Some(_), Some(set)) => {
                let o = RrOptions::new(m, out, job.camera.view);
                check("Einstellung", set(&self.viewport, &o))?;
                FEATURE_RR
            }
            _ => {
                let o = DlssOptions::new(m, out);
                check("Einstellung", (self.set_dlss)(&self.viewport, &o))?;
                FEATURE_DLSS
            }
        };
        self.used[(feature == FEATURE_RR) as usize] = true;
        let read = |p: &Picture| {
            let usage = vk::ImageUsageFlags::SAMPLED;
            Resource::new(p, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, usage)
        };
        let mut list = vec![
            (read(&job.color), TAG_COLOR_IN),
            (read(&job.depth), TAG_DEPTH),
            (read(&job.motion), TAG_MOTION),
            (
                Resource::new(
                    &job.output,
                    vk::ImageLayout::GENERAL,
                    vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::STORAGE,
                ),
                TAG_COLOR_OUT,
            ),
        ];
        if let Some(r) = &job.reactive {
            list.push((read(r), TAG_REACTIVE));
        }
        if let Some(g) = &guides {
            list.push((read(&g.albedo), TAG_ALBEDO));
            list.push((read(&g.specular), TAG_SPECULAR));
            list.push((read(&g.normal), TAG_NORMAL_ROUGHNESS));
        }
        let tags: Vec<Tag> = list.iter().map(|(r, k)| Tag::new(r, *k)).collect();
        let n = tags.len() as u32;
        check(
            "Bilder",
            (self.api.set_tags)(token, &self.viewport, tags.as_ptr(), n, job.cmd),
        )?;
        let inputs = [(&raw const self.viewport).cast::<c_void>()];
        check(
            "Auswertung",
            (self.api.evaluate)(feature, token, inputs.as_ptr(), 1, job.cmd),
        )
    }

    unsafe fn destroy(&mut self, _ctx: &Context) {
        for (used, feature) in self.used.iter().zip([FEATURE_DLSS, FEATURE_RR]) {
            if *used {
                (self.api.free)(feature, &self.viewport);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_follows_the_ratio() {
        assert_eq!(mode([1920, 1080], [1920, 1080]), 6);
        assert_eq!(mode([1280, 720], [1920, 1080]), 3);
        assert_eq!(mode([960, 540], [1920, 1080]), 1);
        assert_eq!(mode([640, 360], [1920, 1080]), 4);
    }
}
