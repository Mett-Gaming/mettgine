//! Bilderzeugung (DLSS Frame Generation). Streamline fuehrt dann auch Reflex und die Zeitmarken; der
//! eigene Reflex-Weg der Engine bleibt aus.

use super::abi::*;
use super::{constants, Api};
use crate::render::vendor::{Job, Picture};
use ash::vk;
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;

const FEATURE_REFLEX: u32 = 3;
const FEATURE_PCL: u32 = 4;
const FEATURE_G: u32 = 1000;
/// Merkmale, die fuer die Bilderzeugung geladen werden
pub const FEATURES: [u32; 3] = [FEATURE_REFLEX, FEATURE_PCL, FEATURE_G];

const TAG_HUDLESS: u32 = 2;
const UNTIL_PRESENT: i32 = 1;

#[repr(C)]
struct Options {
    base: Base,
    mode: u32,
    frames: u32,
    flags: u32,
    sizes: [u32; 7],
    formats: [u32; 5],
    on_error: *const c_void,
    reserved: i8,
    parallelism: u32,
    recompose: i8,
    target_fps: f32,
}

#[repr(C)]
struct State {
    base: Base,
    memory: u64,
    status: u32,
    min_size: u32,
    presented: u32,
    max: u32,
    reserved: i8,
    vsync: i8,
    fence: *const c_void,
    fence_value: u64,
    dynamic: i8,
}

#[repr(C)]
struct ReflexOptions {
    base: Base,
    mode: u32,
    limit_us: u32,
    markers: bool,
    key: u16,
    thread: u32,
}

fn options(mode: u32, frames: u32) -> Options {
    let guid = id(
        0xfac5_f1cb,
        0x2dfd,
        0x4f36,
        [0xa1, 0xe6, 0x3a, 0x9e, 0x86, 0x52, 0x56, 0xc5],
    );
    Options {
        base: base(guid, 5),
        mode,
        frames: frames.max(1),
        flags: 0,
        sizes: [0; 7],
        formats: [0; 5],
        on_error: ptr::null(),
        reserved: 2,
        parallelism: 0,
        recompose: 0,
        target_fps: 0.0,
    }
}

fn state() -> State {
    let guid = id(
        0xcc8a_c8e1,
        0xa179,
        0x44f5,
        [0x97, 0xfa, 0xe7, 0x41, 0x12, 0xf9, 0xbc, 0x61],
    );
    State {
        base: base(guid, 4),
        memory: 0,
        status: 0,
        min_size: 0,
        presented: 0,
        max: 0,
        reserved: 2,
        vsync: 2,
        fence: ptr::null(),
        fence_value: 0,
        dynamic: 2,
    }
}

type SetOptions = unsafe extern "C" fn(*const Viewport, *const Options) -> i32;
type GetState = unsafe extern "C" fn(*const Viewport, *mut State, *const Options) -> i32;
type Sleep = unsafe extern "C" fn(Token) -> i32;
type SetReflex = unsafe extern "C" fn(*const ReflexOptions) -> i32;
type SetMarker = unsafe extern "C" fn(u32, Token) -> i32;

/// Zustand der Bilderzeugung.
pub struct Generation {
    api: Api,
    set_options: SetOptions,
    get_state: GetState,
    sleep: Sleep,
    set_reflex: SetReflex,
    set_marker: SetMarker,
    viewport: Viewport,
    /// Bilder zwischen zwei gerechneten (0 = aus)
    frames: Cell<u32>,
    /// (Begrenzung in Mikrosekunden, Verstaerkung)
    reflex: Cell<Option<(u32, bool)>>,
    presented: Cell<u32>,
    status: Cell<u32>,
    max: Cell<u32>,
}

impl Generation {
    pub fn new(api: &Api, physical: vk::PhysicalDevice) -> Option<Self> {
        let adapter = AdapterInfo::new(physical);
        // SAFETY: Karte gueltig, Typen nach `sl_dlss_g.h`, `sl_reflex.h`, `sl_pcl.h`.
        unsafe {
            for f in FEATURES {
                let r = (api.is_supported)(f, &adapter);
                if r != OK {
                    crate::crash::log(&format!("Bilderzeugung: Teil {f} fehlt ({r})"));
                    return None;
                }
            }
            let g = Self {
                set_options: api.feature_function(FEATURE_G, c"slDLSSGSetOptions")?,
                get_state: api.feature_function(FEATURE_G, c"slDLSSGGetState")?,
                sleep: api.feature_function(FEATURE_REFLEX, c"slReflexSleep")?,
                set_reflex: api.feature_function(FEATURE_REFLEX, c"slReflexSetOptions")?,
                set_marker: api.feature_function(FEATURE_PCL, c"slPCLSetMarker")?,
                viewport: Viewport::new(super::VIEWPORT),
                frames: Cell::new(0),
                reflex: Cell::new(None),
                presented: Cell::new(0),
                status: Cell::new(0),
                max: Cell::new(0),
                api: api.clone(),
            };
            let mut s = state();
            if (g.get_state)(&g.viewport, &mut s, ptr::null()) == OK {
                g.max.set(s.max);
            }
            api.shared.driven.set(true);
            Some(g)
        }
    }

    /// Hoechste Anzahl erzeugter Bilder je gerechnetem.
    pub fn max(&self) -> u32 {
        self.max.get()
    }

    pub fn frames(&self) -> u32 {
        self.frames.get()
    }

    /// Einstellen; danach muss die Swapchain neu gebaut werden.
    pub fn set_frames(&self, n: u32) {
        self.frames.set(n.min(self.max().max(1)));
        self.apply(self.frames.get() > 0);
    }

    /// Vor (false) und nach (true) dem Neubau der Swapchain.
    pub fn apply(&self, on: bool) {
        let on = on && self.frames.get() > 0;
        let o = options(on as u32, self.frames.get());
        // SAFETY: gueltige Strukturen.
        let r = unsafe { (self.set_options)(&self.viewport, &o) };
        if r != OK {
            crate::crash::log(&format!("Bilderzeugung laesst sich nicht einstellen ({r})"));
        }
    }

    /// Schlafpunkt am Anfang eines Bildes; neue Bildnummer.
    pub fn begin(&self, fps: f32, boost: bool) {
        let limit = if fps > 0.0 { (1e6 / fps) as u32 } else { 0 };
        let wish = (limit, boost);
        // SAFETY: gueltige Strukturen; die Bildnummer gehoert der Bibliothek.
        unsafe {
            if self.reflex.replace(Some(wish)) != Some(wish) {
                let guid = id(
                    0xf03a_f81a,
                    0x6d0b,
                    0x4902,
                    [0xa6, 0x51, 0xc4, 0x96, 0x5e, 0x21, 0x54, 0x34],
                );
                let o = ReflexOptions {
                    base: base(guid, 1),
                    mode: 1 + boost as u32,
                    limit_us: limit,
                    markers: false,
                    key: 0,
                    thread: 0,
                };
                (self.set_reflex)(&o);
            }
            let token = self.api.shared.next(&self.api);
            if !token.is_null() {
                (self.sleep)(token);
            }
        }
    }

    /// Zeitmarke (Werte wie `VkLatencyMarkerNV`, 0 bis 5).
    pub fn mark(&self, marker: vk::LatencyMarkerNV) {
        let token = self.api.shared.token.get();
        let raw = marker.as_raw();
        if (0..=5).contains(&raw) && !token.is_null() {
            // SAFETY: gueltige Bildnummer.
            unsafe { (self.set_marker)(raw as u32, token) };
        }
    }

    /// Kamera, Tiefe und Bewegung des Bildes anmelden.
    ///
    /// # Safety
    /// Die Bilder leben bis nach dem Anzeigen und liegen dann in den genannten Zustaenden.
    pub unsafe fn scene(&self, job: &Job) {
        let token = self.api.shared.token.get();
        if token.is_null() || self.frames.get() == 0 {
            return;
        }
        (self.api.set_constants)(&constants(job), token, &self.viewport);
        self.api.shared.constants.set(true);
        let usage = vk::ImageUsageFlags::SAMPLED;
        let depth = vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL;
        let list = [
            (Resource::new(&job.depth, depth, usage), TAG_DEPTH),
            (
                Resource::new(
                    &job.motion,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    usage,
                ),
                TAG_MOTION,
            ),
        ];
        self.tags(&list, token, job.cmd);
    }

    /// Fertiges Bild ohne Oberflaeche anmelden.
    ///
    /// # Safety
    /// Wie [`Generation::scene`].
    pub unsafe fn hudless(&self, cmd: vk::CommandBuffer, p: &Picture) {
        let token = self.api.shared.token.get();
        if token.is_null() || self.frames.get() == 0 {
            return;
        }
        let usage = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST;
        let layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        let list = [(Resource::new(p, layout, usage), TAG_HUDLESS)];
        self.tags(&list, token, cmd);
    }

    unsafe fn tags(&self, list: &[(Resource, u32)], token: Token, cmd: vk::CommandBuffer) {
        let tags: Vec<Tag> = list
            .iter()
            .map(|(r, k)| Tag::lasting(r, *k, UNTIL_PRESENT))
            .collect();
        let n = tags.len() as u32;
        (self.api.set_tags)(token, &self.viewport, tags.as_ptr(), n, cmd);
    }

    /// Nach dem Anzeigen: Zustand abfragen.
    pub fn presented(&self) {
        if self.frames.get() == 0 {
            return;
        }
        let mut s = state();
        // SAFETY: gueltige Struktur.
        if unsafe { (self.get_state)(&self.viewport, &mut s, ptr::null()) } == OK {
            self.presented.set(s.presented);
            self.status.set(s.status);
        }
    }

    /// Zuletzt gezeigte Bilder je gerechnetem, Fehlerzeichen (0 = in Ordnung).
    pub fn state(&self) -> (u32, u32) {
        (self.presented.get(), self.status.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structures_match_the_headers() {
        assert_eq!(size_of::<Options>(), 32 + 64 + 8 + 16);
        assert_eq!(size_of::<State>(), 32 + 8 + 16 + 8 + 8 + 8 + 8);
        assert_eq!(size_of::<ReflexOptions>(), 32 + 16);
    }
}
