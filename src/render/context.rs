//! Vulkan-Instanz, Surface, Geraet und Queue (Auswahl und Anlegen des Geraets: `device`).

use super::device::{self, RtFns};
use super::Result;
use ash::vk;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::ffi::{c_void, CStr};
use winit::window::Window;

const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";

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
    /// Helligkeit laesst sich dem Bildschirm mitteilen
    pub hdr_metadata: bool,
    pub low_latency: super::latency::Support,
    pub diagnostics: super::diagnostics::Diagnostics,
    /// uebersetzte Shader (siehe `cache`)
    pub cache: vk::PipelineCache,
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
    /// `want` = zuschaltbare Teile, soweit die Karte sie kann.
    pub fn new(window: &Window, want: device::Want) -> Result<Self> {
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
            if want.hdr {
                let name = ash::ext::swapchain_colorspace::NAME;
                let known = entry.enumerate_instance_extension_properties(None)?;
                if known
                    .iter()
                    .any(|e| e.extension_name_as_c_str().ok() == Some(name))
                {
                    extensions.push(name.as_ptr());
                }
            }
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

            let (physical, queue_family) = device::choose(&instance, &surface_loader, surface)?;
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

            let made = device::create(&instance, physical, queue_family, want)?;
            let device::Made {
                device,
                rt,
                low_latency,
                hdr_metadata,
                diagnostics,
                max_anisotropy,
            } = made;
            let cache = super::cache::open(&device, &props);
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
                low_latency,
                hdr_metadata,
                diagnostics,
                cache,
                max_anisotropy,
                gpu_name,
            })
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            super::cache::close(&self.device, self.cache);
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            if let Some((loader, messenger)) = self.debug.take() {
                loader.destroy_debug_utils_messenger(messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}
