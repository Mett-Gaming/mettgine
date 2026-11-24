//! Vulkan-Instanz, Surface, Geraet und Queue.

use super::Result;
use ash::vk;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::ffi::{c_void, CStr};
use winit::window::Window;

const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";

/// Funktionen fuer Raytracing (nur, wenn eingeschaltet und von der Karte unterstuetzt).
pub struct RtFns {
    pub accel: ash::khr::acceleration_structure::Device,
    /// Ausrichtung des Arbeitsspeichers beim Bauen
    pub scratch_align: u64,
}

/// Kann die Karte Strahlabfragen samt Texturfeldern?
unsafe fn supports_rt(instance: &ash::Instance, pd: vk::PhysicalDevice) -> bool {
    let Ok(list) = instance.enumerate_device_extension_properties(pd) else {
        return false;
    };
    let has = |name: &CStr| {
        list.iter()
            .any(|e| e.extension_name_as_c_str().ok() == Some(name))
    };
    if !RT_EXTENSIONS.iter().all(|e| has(e)) {
        return false;
    }
    let mut v12 = vk::PhysicalDeviceVulkan12Features::default();
    let mut accel = vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default();
    let mut query = vk::PhysicalDeviceRayQueryFeaturesKHR::default();
    let mut f = vk::PhysicalDeviceFeatures2::default()
        .push_next(&mut v12)
        .push_next(&mut accel)
        .push_next(&mut query);
    instance.get_physical_device_features2(pd, &mut f);
    let yes = |b: vk::Bool32| b == vk::TRUE;
    yes(v12.buffer_device_address)
        && yes(v12.shader_sampled_image_array_non_uniform_indexing)
        && yes(v12.descriptor_binding_partially_bound)
        && yes(accel.acceleration_structure)
        && yes(query.ray_query)
}

const RT_EXTENSIONS: [&CStr; 3] = [
    ash::khr::acceleration_structure::NAME,
    ash::khr::ray_query::NAME,
    ash::khr::deferred_host_operations::NAME,
];

pub struct Context {
    pub _entry: ash::Entry,
    pub instance: ash::Instance,
    debug: Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    pub surface_loader: ash::khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    pub physical: vk::PhysicalDevice,
    pub mem_props: vk::PhysicalDeviceMemoryProperties,
    pub queue_family: u32,
    pub device: ash::Device,
    pub queue: vk::Queue,
    pub swapchain_loader: ash::khr::swapchain::Device,
    pub rt: Option<RtFns>,
    /// hoechste Filterstufe schraeg gesehener Texturen (1 = kann die Karte nicht)
    pub max_anisotropy: f32,
    /// Name der Grafikkarte (fuer Absturzberichte)
    pub gpu_name: String,
}

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    ty: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user: *mut c_void,
) -> vk::Bool32 {
    let msg = if data.is_null() || (*data).p_message.is_null() {
        "".into()
    } else {
        CStr::from_ptr((*data).p_message).to_string_lossy()
    };
    crate::crash::log(&format!("[vulkan {severity:?} {ty:?}] {msg}"));
    vk::FALSE
}

impl Context {
    /// `want_rt` = Raytracing einschalten, falls die Karte es kann.
    pub fn new(window: &Window, want_rt: bool) -> Result<Self> {
        unsafe {
            let entry = ash::Entry::load()?;
            let display = window.display_handle()?.as_raw();
            let win = window.window_handle()?.as_raw();

            let validation = cfg!(debug_assertions)
                && entry
                    .enumerate_instance_layer_properties()?
                    .iter()
                    .any(|l| l.layer_name_as_c_str().ok() == Some(VALIDATION_LAYER));
            if cfg!(debug_assertions) && !validation {
                crate::crash::log(
                    "Hinweis: Validation-Layer nicht gefunden (Vulkan SDK nicht installiert).",
                );
            }

            let app_info = vk::ApplicationInfo::default()
                .application_name(c"Mettgine")
                .engine_name(c"Mettgine")
                .api_version(vk::API_VERSION_1_3);
            let mut extensions = ash_window::enumerate_required_extensions(display)?.to_vec();
            let mut layers = Vec::new();
            if validation {
                extensions.push(ash::ext::debug_utils::NAME.as_ptr());
                layers.push(VALIDATION_LAYER.as_ptr());
            }
            let instance = entry.create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(&app_info)
                    .enabled_extension_names(&extensions)
                    .enabled_layer_names(&layers),
                None,
            )?;

            let debug = if validation {
                let loader = ash::ext::debug_utils::Instance::new(&entry, &instance);
                let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                    .message_severity(
                        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                            | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
                    )
                    .message_type(
                        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                            | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                            | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                    )
                    .pfn_user_callback(Some(debug_callback));
                let messenger = loader.create_debug_utils_messenger(&info, None)?;
                Some((loader, messenger))
            } else {
                None
            };

            let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
            let surface = ash_window::create_surface(&entry, &instance, display, win, None)?;

            // Geraet waehlen: Vulkan 1.3, eine Queue mit Grafik + Present, diskrete GPU bevorzugt.
            let mut best: Option<(vk::PhysicalDevice, u32, u32)> = None;
            for pd in instance.enumerate_physical_devices()? {
                let props = instance.get_physical_device_properties(pd);
                if props.api_version < vk::API_VERSION_1_3 {
                    continue;
                }
                let families = instance.get_physical_device_queue_family_properties(pd);
                for (i, fam) in families.iter().enumerate() {
                    let i = i as u32;
                    if fam.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                        && surface_loader.get_physical_device_surface_support(pd, i, surface)?
                    {
                        let score = match props.device_type {
                            vk::PhysicalDeviceType::DISCRETE_GPU => 3,
                            vk::PhysicalDeviceType::INTEGRATED_GPU => 2,
                            _ => 1,
                        };
                        if best.is_none_or(|b| score > b.2) {
                            best = Some((pd, i, score));
                        }
                        break;
                    }
                }
            }
            let (physical, queue_family, _) =
                best.ok_or("Keine Vulkan-1.3-GPU mit Grafik- und Present-Queue gefunden")?;
            let props = instance.get_physical_device_properties(physical);
            let gpu_name = format!(
                "{} (Treiber {}, Vulkan {}.{})",
                props
                    .device_name_as_c_str()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default(),
                props.driver_version,
                vk::api_version_major(props.api_version),
                vk::api_version_minor(props.api_version)
            );

            let priorities = [1.0f32];
            let queue_info = [vk::DeviceQueueCreateInfo::default()
                .queue_family_index(queue_family)
                .queue_priorities(&priorities)];
            let use_rt = want_rt && supports_rt(&instance, physical);
            let mut device_extensions = vec![ash::khr::swapchain::NAME.as_ptr()];
            if use_rt {
                device_extensions.extend(RT_EXTENSIONS.map(CStr::as_ptr));
            }
            let can = instance.get_physical_device_features(physical);
            let features = vk::PhysicalDeviceFeatures::default()
                .sampler_anisotropy(can.sampler_anisotropy == vk::TRUE);
            let max_anisotropy = if can.sampler_anisotropy == vk::TRUE {
                props.limits.max_sampler_anisotropy
            } else {
                1.0
            };
            let mut features13 =
                vk::PhysicalDeviceVulkan13Features::default().dynamic_rendering(true);
            let mut v12 = vk::PhysicalDeviceVulkan12Features::default()
                .buffer_device_address(true)
                .descriptor_indexing(true)
                .shader_sampled_image_array_non_uniform_indexing(true)
                .descriptor_binding_partially_bound(true);
            let mut accel = vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default()
                .acceleration_structure(true);
            let mut query = vk::PhysicalDeviceRayQueryFeaturesKHR::default().ray_query(true);
            let mut info = vk::DeviceCreateInfo::default()
                .queue_create_infos(&queue_info)
                .enabled_extension_names(&device_extensions)
                .enabled_features(&features)
                .push_next(&mut features13);
            if use_rt {
                info = info
                    .push_next(&mut v12)
                    .push_next(&mut accel)
                    .push_next(&mut query);
            }
            let device = instance.create_device(physical, &info, None)?;
            let rt = use_rt.then(|| {
                let mut limits = vk::PhysicalDeviceAccelerationStructurePropertiesKHR::default();
                let mut props = vk::PhysicalDeviceProperties2::default().push_next(&mut limits);
                instance.get_physical_device_properties2(physical, &mut props);
                RtFns {
                    accel: ash::khr::acceleration_structure::Device::new(&instance, &device),
                    scratch_align: limits.min_acceleration_structure_scratch_offset_alignment
                        as u64,
                }
            });
            let queue = device.get_device_queue(queue_family, 0);
            let swapchain_loader = ash::khr::swapchain::Device::new(&instance, &device);
            let mem_props = instance.get_physical_device_memory_properties(physical);

            Ok(Self {
                _entry: entry,
                instance,
                debug,
                surface_loader,
                surface,
                physical,
                mem_props,
                queue_family,
                device,
                queue,
                swapchain_loader,
                rt,
                max_anisotropy,
                gpu_name,
            })
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            if let Some((loader, messenger)) = self.debug.take() {
                loader.destroy_debug_utils_messenger(messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}
