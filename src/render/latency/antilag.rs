//! Bindung fuer `VK_AMD_anti_lag` (fehlt in `ash` 0.38). Werte nach der Vulkan-Beschreibung.

use ash::vk;
use std::ffi::{c_void, CStr};
use std::ptr;

pub const NAME: &CStr = c"VK_AMD_anti_lag";
const FEATURES: i32 = 1_000_476_000;
const DATA: i32 = 1_000_476_001;
const PRESENTATION: i32 = 1_000_476_002;

const MODE_ON: i32 = 1;
const MODE_OFF: i32 = 2;
const STAGE_INPUT: i32 = 0;
const STAGE_PRESENT: i32 = 1;

/// `VkPhysicalDeviceAntiLagFeaturesAMD`
#[repr(C)]
pub struct Features {
    s_type: vk::StructureType,
    p_next: *mut c_void,
    pub anti_lag: vk::Bool32,
}

impl Features {
    pub fn enabled() -> Self {
        Self {
            s_type: vk::StructureType::from_raw(FEATURES),
            p_next: ptr::null_mut(),
            anti_lag: vk::TRUE,
        }
    }
}

// SAFETY: Aufbau und Kennung entsprechen der Vulkan-Beschreibung.
unsafe impl vk::ExtendsDeviceCreateInfo for Features {}
unsafe impl vk::ExtendsPhysicalDeviceFeatures2 for Features {}

#[repr(C)]
struct Presentation {
    s_type: vk::StructureType,
    p_next: *const c_void,
    stage: i32,
    frame_index: u64,
}

#[repr(C)]
struct Data {
    s_type: vk::StructureType,
    p_next: *const c_void,
    mode: i32,
    max_fps: u32,
    presentation: *const Presentation,
}

type Update = unsafe extern "system" fn(vk::Device, *const Data);

/// Geladene Funktion.
pub struct AntiLag {
    device: vk::Device,
    update: Update,
}

impl AntiLag {
    /// `None` = der Treiber liefert die Funktion nicht.
    ///
    /// # Safety
    /// `device` muss mit der Erweiterung angelegt sein.
    pub unsafe fn load(instance: &ash::Instance, device: &ash::Device) -> Option<Self> {
        let name = c"vkAntiLagUpdateAMD".as_ptr();
        let f = instance.get_device_proc_addr(device.handle(), name)?;
        Some(Self {
            device: device.handle(),
            update: std::mem::transmute::<unsafe extern "system" fn(), Update>(f),
        })
    }

    /// Schlafpunkt (`present` = false) bzw. kurz vor dem Anzeigen. `max_fps` 0 = unbegrenzt.
    ///
    /// # Safety
    /// Das Geraet muss noch leben.
    pub unsafe fn mark(&self, present: bool, frame: u64, max_fps: u32) {
        let p = Presentation {
            s_type: vk::StructureType::from_raw(PRESENTATION),
            p_next: ptr::null(),
            stage: if present { STAGE_PRESENT } else { STAGE_INPUT },
            frame_index: frame,
        };
        self.call(MODE_ON, max_fps, &p);
    }

    /// # Safety
    /// Das Geraet muss noch leben.
    pub unsafe fn off(&self) {
        self.call(MODE_OFF, 0, ptr::null());
    }

    unsafe fn call(&self, mode: i32, max_fps: u32, presentation: *const Presentation) {
        let data = Data {
            s_type: vk::StructureType::from_raw(DATA),
            p_next: ptr::null(),
            mode,
            max_fps,
            presentation,
        };
        (self.update)(self.device, &data);
    }
}
