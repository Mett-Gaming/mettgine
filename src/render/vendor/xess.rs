//! Intel XeSS (`libxess.dll`), nur Hochskalieren. Strukturen nach `xess_vk.h` 3.0.

use super::super::context::Context;
use super::super::Result;
use super::library::Library;
use super::{Backend, Job, Picture};
use ash::vk;
use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;

const DLL: &str = "libxess.dll";
const FLAG_RESPONSIVE_MASK: u32 = 1 << 3;
const FLAG_AUTO_EXPOSURE: u32 = 1 << 8;

#[repr(C)]
#[derive(Clone, Copy)]
struct View {
    view: vk::ImageView,
    image: vk::Image,
    range: vk::ImageSubresourceRange,
    format: vk::Format,
    width: u32,
    height: u32,
}

#[repr(C)]
struct Execute {
    color: View,
    motion: View,
    depth: View,
    exposure: View,
    mask: View,
    output: View,
    jitter: [f32; 2],
    exposure_scale: f32,
    reset: u32,
    input: [u32; 2],
    color_base: [u32; 2],
    motion_base: [u32; 2],
    depth_base: [u32; 2],
    mask_base: [u32; 2],
    reserved: [u32; 2],
    output_base: [u32; 2],
}

#[repr(C)]
struct Init {
    out: [u32; 2],
    quality: i32,
    flags: u32,
    creation_mask: u32,
    visible_mask: u32,
    buffer_heap: vk::DeviceMemory,
    buffer_offset: u64,
    texture_heap: vk::DeviceMemory,
    texture_offset: u64,
    cache: vk::PipelineCache,
}

type Handle = *mut c_void;
type Names = *const *const c_char;
type InstanceExtensions = unsafe extern "C" fn(*mut u32, *mut Names, *mut u32) -> i32;
type DeviceExtensions =
    unsafe extern "C" fn(vk::Instance, vk::PhysicalDevice, *mut u32, *mut Names) -> i32;
type DeviceFeatures =
    unsafe extern "C" fn(vk::Instance, vk::PhysicalDevice, *mut *mut c_void) -> i32;
type Create =
    unsafe extern "C" fn(vk::Instance, vk::PhysicalDevice, vk::Device, *mut Handle) -> i32;
type InitFn = unsafe extern "C" fn(Handle, *const Init) -> i32;
type ExecuteFn = unsafe extern "C" fn(Handle, vk::CommandBuffer, *const Execute) -> i32;
type Destroy = unsafe extern "C" fn(Handle) -> i32;
type SetScale = unsafe extern "C" fn(Handle, f32, f32) -> i32;
type Version = unsafe extern "C" fn(*mut [u16; 4]) -> i32;

/// Geladene Funktionen.
#[derive(Clone, Copy)]
pub struct Api {
    instance_extensions: InstanceExtensions,
    device_extensions: DeviceExtensions,
    device_features: DeviceFeatures,
    create: Create,
    init: InitFn,
    execute: ExecuteFn,
    destroy: Destroy,
    set_velocity_scale: SetScale,
    version: Version,
}

unsafe fn names(list: Names, count: u32) -> Vec<CString> {
    if list.is_null() {
        return Vec::new();
    }
    (0..count as usize)
        .map(|i| CStr::from_ptr(*list.add(i)).to_owned())
        .collect()
}

impl Api {
    pub fn load() -> Option<Self> {
        let lib = Library::load(DLL)?;
        // SAFETY: Typen nach `xess.h` und `xess_vk.h`.
        unsafe {
            Some(Self {
                instance_extensions: lib.get(c"xessVKGetRequiredInstanceExtensions")?,
                device_extensions: lib.get(c"xessVKGetRequiredDeviceExtensions")?,
                device_features: lib.get(c"xessVKGetRequiredDeviceFeatures")?,
                create: lib.get(c"xessVKCreateContext")?,
                init: lib.get(c"xessVKInit")?,
                execute: lib.get(c"xessVKExecute")?,
                destroy: lib.get(c"xessDestroyContext")?,
                set_velocity_scale: lib.get(c"xessSetVelocityScale")?,
                version: lib.get(c"xessGetVersion")?,
            })
        }
    }

    pub fn instance_extensions(&self) -> Vec<CString> {
        let (mut count, mut list, mut api) = (0, ptr::null(), 0);
        // SAFETY: Ausgaben sind gueltig; die Liste gehoert der Bibliothek.
        unsafe {
            match (self.instance_extensions)(&mut count, &mut list, &mut api) {
                r if r < 0 => Vec::new(),
                _ => names(list, count),
            }
        }
    }

    /// # Safety
    /// `pd` gehoert zu `instance`.
    pub unsafe fn device_extensions(
        &self,
        instance: &ash::Instance,
        pd: vk::PhysicalDevice,
    ) -> Vec<CString> {
        let (mut count, mut list) = (0, ptr::null());
        match (self.device_extensions)(instance.handle(), pd, &mut count, &mut list) {
            r if r < 0 => Vec::new(),
            _ => names(list, count),
        }
    }

    /// # Safety
    /// `chain` ist eine gueltige, beschreibbare Kette.
    pub unsafe fn patch_features(
        &self,
        instance: &ash::Instance,
        pd: vk::PhysicalDevice,
        chain: *mut c_void,
    ) -> *mut c_void {
        let mut head = chain;
        match (self.device_features)(instance.handle(), pd, &mut head) {
            r if r < 0 => chain,
            _ => head,
        }
    }

    pub fn create(&self, ctx: &Context) -> Option<Box<dyn Backend>> {
        let mut handle: Handle = ptr::null_mut();
        // SAFETY: gueltige Handles.
        let r = unsafe {
            (self.create)(
                ctx.instance.handle(),
                ctx.physical,
                ctx.device.handle(),
                &mut handle,
            )
        };
        if r < 0 || handle.is_null() {
            crate::crash::log(&format!("XeSS: auf dieser Karte nicht moeglich ({r})"));
            return None;
        }
        let mut v = [0u16; 4];
        // SAFETY: gueltige Ausgabe.
        unsafe { (self.version)(&mut v) };
        Some(Box::new(Xess {
            api: *self,
            handle,
            made: None,
            name: format!("XeSS {}.{}.{}", v[0], v[1], v[2]),
        }))
    }
}

struct Xess {
    api: Api,
    handle: Handle,
    /// (Szene, Ziel, mit Maske)
    made: Option<([u32; 2], [u32; 2], bool)>,
    name: String,
}

/// Stufe, deren Verhaeltnis am besten passt.
fn quality(render: [u32; 2], out: [u32; 2]) -> i32 {
    let k = out[0] as f32 / render[0].max(1) as f32;
    let steps = [
        (1.0, 106),
        (1.3, 105),
        (1.5, 104),
        (1.7, 103),
        (2.0, 102),
        (2.3, 101),
        (3.0, 100),
    ];
    steps
        .into_iter()
        .min_by(|a, b| (a.0 - k).abs().total_cmp(&(b.0 - k).abs()))
        .map_or(102, |s: (f32, i32)| s.1)
}

fn view(p: &Picture) -> View {
    let aspect = if p.format == vk::Format::D32_SFLOAT {
        vk::ImageAspectFlags::DEPTH
    } else {
        vk::ImageAspectFlags::COLOR
    };
    View {
        view: p.view,
        image: p.image,
        range: vk::ImageSubresourceRange {
            aspect_mask: aspect,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        },
        format: p.format,
        width: p.extent.width,
        height: p.extent.height,
    }
}

fn no_view() -> View {
    View {
        view: vk::ImageView::null(),
        image: vk::Image::null(),
        range: vk::ImageSubresourceRange::default(),
        format: vk::Format::UNDEFINED,
        width: 0,
        height: 0,
    }
}

impl Xess {
    unsafe fn ensure(&mut self, ctx: &Context, key: ([u32; 2], [u32; 2], bool)) -> Result<()> {
        if self.made == Some(key) {
            return Ok(());
        }
        let (render, out, mask) = key;
        ctx.device.device_wait_idle()?;
        let init = Init {
            out,
            quality: quality(render, out),
            flags: FLAG_AUTO_EXPOSURE | if mask { FLAG_RESPONSIVE_MASK } else { 0 },
            creation_mask: 0,
            visible_mask: 0,
            buffer_heap: vk::DeviceMemory::null(),
            buffer_offset: 0,
            texture_heap: vk::DeviceMemory::null(),
            texture_offset: 0,
            cache: vk::PipelineCache::null(),
        };
        let r = (self.api.init)(self.handle, &init);
        if r < 0 {
            return Err(format!("XeSS: Aufbau fehlgeschlagen ({r})").into());
        }
        // die Technik erwartet vorher minus jetzt in Bildpunkten der Szene
        let (x, y) = (-(render[0] as f32), -(render[1] as f32));
        (self.api.set_velocity_scale)(self.handle, x, y);
        self.made = Some(key);
        Ok(())
    }
}

impl Backend for Xess {
    fn name(&self) -> String {
        self.name.clone()
    }

    unsafe fn evaluate(&mut self, ctx: &Context, job: &Job) -> Result<()> {
        let size = |p: &Picture| [p.extent.width, p.extent.height];
        let (render, out) = (size(&job.color), size(&job.output));
        self.ensure(ctx, (render, out, job.reactive.is_some()))?;
        let e = Execute {
            color: view(&job.color),
            motion: view(&job.motion),
            depth: view(&job.depth),
            exposure: no_view(),
            mask: job.reactive.as_ref().map_or_else(no_view, view),
            output: view(&job.output),
            jitter: job.jitter.to_array(),
            exposure_scale: 1.0,
            reset: job.reset as u32,
            input: render,
            color_base: [0; 2],
            motion_base: [0; 2],
            depth_base: [0; 2],
            mask_base: [0; 2],
            reserved: [0; 2],
            output_base: [0; 2],
        };
        match (self.api.execute)(self.handle, job.cmd, &e) {
            r if r < 0 => Err(format!("XeSS: Fehler {r}").into()),
            _ => Ok(()),
        }
    }

    unsafe fn destroy(&mut self, _ctx: &Context) {
        (self.api.destroy)(self.handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structures_match_the_headers() {
        assert_eq!(size_of::<View>(), 48);
        assert_eq!(size_of::<Execute>(), 6 * 48 + 18 * 4);
        assert_eq!(size_of::<Init>(), 24 + 40);
    }

    #[test]
    fn quality_follows_the_ratio() {
        assert_eq!(quality([1920, 1080], [1920, 1080]), 106);
        assert_eq!(quality([960, 540], [1920, 1080]), 102);
        assert_eq!(quality([640, 360], [1920, 1080]), 100);
    }
}
