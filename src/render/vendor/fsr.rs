//! AMD FSR ueber die FidelityFX-API (`amd_fidelityfx_loader_vk.dll` und
//! `amd_fidelityfx_upscaler_vk.dll`). Strukturen nach den Beschreibungen der API 2.4.

use super::super::context::Context;
use super::super::Result;
use super::library::Library;
use super::{Backend, Job, Picture};
use ash::vk;
use ash::vk::Handle;
use std::ffi::{c_char, c_void, CStr};
use std::ptr;

const DLL: &str = "amd_fidelityfx_loader_vk.dll";
const OK: u32 = 0;
const CREATE_UPSCALE: u64 = 0x0001_0000;
const DISPATCH_UPSCALE: u64 = 0x0001_0001;
const UPSCALE_VERSION: u64 = 0x0001_000b;
const BACKEND_VK: u64 = 0x0100_0003;
const QUERY_PROVIDER: u64 = 6;
/// FSR-Version, nach deren Beschreibung dies geschrieben ist (4.1.2)
const VERSION: u32 = 4 << 22 | 1 << 12 | 2;
const FLAG_HDR: u32 = 1;
const FLAG_AUTO_EXPOSURE: u32 = 1 << 5;

const FORMAT_RGBA16F: u32 = 4;
const FORMAT_RG16F: u32 = 18;
const FORMAT_R8: u32 = 25;
const FORMAT_R32F: u32 = 28;
const USAGE_UAV: u32 = 2;
const USAGE_DEPTH: u32 = 8;
const STATE_UAV: u32 = 2;
const STATE_READ: u32 = 4;
const TYPE_2D: u32 = 2;

#[repr(C)]
struct Header {
    kind: u64,
    next: *mut Header,
}

#[repr(C)]
struct CreateUpscale {
    header: Header,
    flags: u32,
    max_render: [u32; 2],
    max_out: [u32; 2],
    message: *const c_void,
}

#[repr(C)]
struct CreateVersion {
    header: Header,
    version: u32,
}

#[repr(C)]
struct CreateBackend {
    header: Header,
    device: vk::Device,
    physical: vk::PhysicalDevice,
    proc_addr: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Resource {
    resource: u64,
    kind: u32,
    format: u32,
    width: u32,
    height: u32,
    depth: u32,
    mips: u32,
    flags: u32,
    usage: u32,
    state: u32,
}

#[repr(C)]
struct DispatchUpscale {
    header: Header,
    commands: vk::CommandBuffer,
    color: Resource,
    depth: Resource,
    motion: Resource,
    exposure: Resource,
    reactive: Resource,
    transparency: Resource,
    output: Resource,
    jitter: [f32; 2],
    motion_scale: [f32; 2],
    render: [u32; 2],
    out: [u32; 2],
    sharpen: bool,
    sharpness: f32,
    frame_ms: f32,
    pre_exposure: f32,
    reset: bool,
    near: f32,
    far: f32,
    fov_y: f32,
    meters: f32,
    flags: u32,
}

#[repr(C)]
struct QueryProvider {
    header: Header,
    id: u64,
    name: *const c_char,
}

type FfxContext = *mut c_void;
type Create = unsafe extern "C" fn(*mut FfxContext, *mut Header, *const c_void) -> u32;
type Destroy = unsafe extern "C" fn(*mut FfxContext, *const c_void) -> u32;
type Query = unsafe extern "C" fn(*mut FfxContext, *mut Header) -> u32;
type Dispatch = unsafe extern "C" fn(*mut FfxContext, *const Header) -> u32;

/// Geladene Funktionen.
#[derive(Clone, Copy)]
pub struct Api {
    create: Create,
    destroy: Destroy,
    query: Query,
    dispatch: Dispatch,
}

impl Api {
    pub fn load() -> Option<Self> {
        let lib = Library::load(DLL)?;
        // SAFETY: Typen nach `ffx_api.h`.
        unsafe {
            Some(Self {
                create: lib.get(c"ffxCreateContext")?,
                destroy: lib.get(c"ffxDestroyContext")?,
                query: lib.get(c"ffxQuery")?,
                dispatch: lib.get(c"ffxDispatch")?,
            })
        }
    }

    pub fn create(&self, ctx: &Context) -> Option<Box<dyn Backend>> {
        let mut fsr = Fsr {
            api: *self,
            made: None,
            name: String::new(),
        };
        // kleiner Versuch: Nimmt die DLL dieses Geraet an?
        // SAFETY: gueltiges Geraet.
        unsafe {
            if let Err(e) = fsr.ensure(ctx, [640, 360], [1280, 720]) {
                crate::crash::log(&format!("FSR: {e}"));
                return None;
            }
        }
        Some(Box::new(fsr))
    }
}

/// Ein Kontext fuer feste Groessen.
struct Made {
    context: FfxContext,
    render: [u32; 2],
    out: [u32; 2],
}

struct Fsr {
    api: Api,
    made: Option<Made>,
    name: String,
}

fn header(kind: u64) -> Header {
    Header {
        kind,
        next: ptr::null_mut(),
    }
}

impl Fsr {
    unsafe fn ensure(&mut self, ctx: &Context, render: [u32; 2], out: [u32; 2]) -> Result<()> {
        let fits = |m: &Made| m.render == render && m.out == out;
        if self.made.as_ref().is_some_and(fits) {
            return Ok(());
        }
        ctx.device.device_wait_idle()?;
        self.drop_context();
        let mut backend = CreateBackend {
            header: header(BACKEND_VK),
            device: ctx.device.handle(),
            physical: ctx.physical,
            proc_addr: ctx.instance.fp_v1_0().get_device_proc_addr as *const c_void,
        };
        let mut version = CreateVersion {
            header: header(UPSCALE_VERSION),
            version: VERSION,
        };
        version.header.next = (&raw mut backend).cast();
        let mut create = CreateUpscale {
            header: header(CREATE_UPSCALE),
            flags: FLAG_HDR | FLAG_AUTO_EXPOSURE,
            max_render: render,
            max_out: out,
            message: ptr::null(),
        };
        create.header.next = (&raw mut version).cast();
        let mut context: FfxContext = ptr::null_mut();
        let r = (self.api.create)(&mut context, (&raw mut create).cast(), ptr::null());
        if r != OK || context.is_null() {
            return Err(format!("Anlegen fehlgeschlagen ({r})").into());
        }
        self.made = Some(Made {
            context,
            render,
            out,
        });
        self.name = self.provider();
        Ok(())
    }

    unsafe fn provider(&mut self) -> String {
        let Some(m) = &mut self.made else {
            return String::new();
        };
        let mut q = QueryProvider {
            header: header(QUERY_PROVIDER),
            id: 0,
            name: ptr::null(),
        };
        let r = (self.api.query)(&mut m.context, (&raw mut q).cast());
        if r != OK || q.name.is_null() {
            return "FSR".into();
        }
        format!("FSR {}", CStr::from_ptr(q.name).to_string_lossy())
    }

    unsafe fn drop_context(&mut self) {
        if let Some(mut m) = self.made.take() {
            (self.api.destroy)(&mut m.context, ptr::null());
        }
    }
}

fn resource(p: &Picture, state: u32) -> Resource {
    let depth = p.format == vk::Format::D32_SFLOAT;
    Resource {
        resource: p.image.as_raw(),
        kind: TYPE_2D,
        format: match p.format {
            vk::Format::R16G16_SFLOAT => FORMAT_RG16F,
            vk::Format::R8_UNORM => FORMAT_R8,
            _ if depth => FORMAT_R32F,
            _ => FORMAT_RGBA16F,
        },
        width: p.extent.width,
        height: p.extent.height,
        depth: 1,
        mips: 1,
        flags: 0,
        usage: if state == STATE_UAV {
            USAGE_UAV
        } else if depth {
            USAGE_DEPTH
        } else {
            0
        },
        state,
    }
}

fn no_resource() -> Resource {
    Resource {
        resource: 0,
        kind: 0,
        format: 0,
        width: 0,
        height: 0,
        depth: 0,
        mips: 0,
        flags: 0,
        usage: 0,
        state: 0,
    }
}

impl Backend for Fsr {
    fn name(&self) -> String {
        self.name.clone()
    }

    unsafe fn evaluate(&mut self, ctx: &Context, job: &Job) -> Result<()> {
        let size = |p: &Picture| [p.extent.width, p.extent.height];
        let (render, out) = (size(&job.color), size(&job.output));
        self.ensure(ctx, render, out)?;
        let Some(m) = &mut self.made else {
            return Err("FSR fehlt".into());
        };
        let d = DispatchUpscale {
            header: header(DISPATCH_UPSCALE),
            commands: job.cmd,
            color: resource(&job.color, STATE_READ),
            depth: resource(&job.depth, STATE_READ),
            motion: resource(&job.motion, STATE_READ),
            exposure: no_resource(),
            reactive: job
                .reactive
                .as_ref()
                .map_or_else(no_resource, |r| resource(r, STATE_READ)),
            transparency: no_resource(),
            output: resource(&job.output, STATE_UAV),
            jitter: job.jitter.to_array(),
            // die Technik erwartet vorher minus jetzt in Bildpunkten
            motion_scale: [-(render[0] as f32), -(render[1] as f32)],
            render,
            out,
            sharpen: false,
            sharpness: 0.0,
            frame_ms: (job.dt * 1000.0).clamp(0.1, 200.0),
            pre_exposure: 1.0,
            reset: job.reset,
            near: job.camera.near,
            far: job.camera.far,
            fov_y: job.camera.fov_y,
            meters: 1.0,
            flags: 0,
        };
        match (self.api.dispatch)(&mut m.context, (&raw const d).cast()) {
            OK => Ok(()),
            r => Err(format!("FSR: Fehler {r}").into()),
        }
    }

    unsafe fn destroy(&mut self, _ctx: &Context) {
        self.drop_context();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structures_match_the_headers() {
        assert_eq!(size_of::<Header>(), 16);
        assert_eq!(size_of::<CreateUpscale>(), 48);
        assert_eq!(size_of::<Resource>(), 48);
        assert_eq!(
            size_of::<DispatchUpscale>(),
            16 + 8 + 7 * 48 + 8 * 4 + 10 * 4
        );
    }
}
