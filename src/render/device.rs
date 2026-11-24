//! Grafikkarte auswaehlen und das Vulkan-Geraet mit den gewuenschten Zusaetzen anlegen.

use super::diagnostics::Diagnostics;
use super::latency::{self, Support};
use super::vendor::Vendors;
use super::Result;
use ash::vk;
use std::ffi::CStr;

/// Umgebungsvariable: Nummer oder Teil des Namens der Grafikkarte (zum Testen)
const GPU_ENV: &str = "METTGINE_GPU";

/// Was beim Anlegen zugeschaltet werden soll. Kann die Karte etwas nicht, bleibt es aus.
#[derive(Clone, Copy, Debug, Default)]
pub struct Want {
    pub ray_tracing: bool,
    pub low_latency: bool,
    pub frame_generation: bool,
    pub diagnostics: bool,
    pub hdr: bool,
    pub gpu_culling: bool,
}

/// Funktionen fuer Raytracing (nur, wenn eingeschaltet und von der Karte unterstuetzt).
pub struct RtFns {
    pub accel: ash::khr::acceleration_structure::Device,
    /// Ausrichtung des Arbeitsspeichers beim Bauen
    pub scratch_align: u64,
}

/// Fertiges Geraet samt zugeschalteten Teilen.
pub struct Made {
    pub device: ash::Device,
    pub rt: Option<RtFns>,
    pub low_latency: Support,
    pub hdr_metadata: bool,
    /// indirekte Zeichenbefehle mit eigenem Anfang
    pub gpu_culling: bool,
    pub diagnostics: Diagnostics,
    /// hoechste Filterstufe schraeg gesehener Texturen (1 = kann die Karte nicht)
    pub max_anisotropy: f32,
}

const RT_EXTENSIONS: [&CStr; 3] = [
    ash::khr::acceleration_structure::NAME,
    ash::khr::ray_query::NAME,
    ash::khr::deferred_host_operations::NAME,
];

/// Erweiterungen einer Karte.
pub struct Extensions(Vec<vk::ExtensionProperties>);

impl Extensions {
    unsafe fn of(instance: &ash::Instance, pd: vk::PhysicalDevice) -> Self {
        Self(
            instance
                .enumerate_device_extension_properties(pd)
                .unwrap_or_default(),
        )
    }

    pub fn has(&self, name: &CStr) -> bool {
        self.0
            .iter()
            .any(|e| e.extension_name_as_c_str().ok() == Some(name))
    }
}

/// Passt die Karte zum Wunsch aus der Umgebung?
fn matches_wish(wish: &str, index: usize, name: &str) -> bool {
    match wish.parse::<usize>() {
        Ok(n) => n == index,
        Err(_) => name.to_lowercase().contains(&wish.to_lowercase()),
    }
}

/// Karte mit Vulkan 1.3 und einer Queue fuer Grafik und Anzeige; eigenstaendige bevorzugt.
///
/// # Safety
/// `instance` und `surface` muessen gueltig sein.
pub unsafe fn choose(
    instance: &ash::Instance,
    surface_loader: &ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
) -> Result<(vk::PhysicalDevice, u32)> {
    let wish = std::env::var(GPU_ENV).unwrap_or_default();
    let mut best: Option<(vk::PhysicalDevice, u32, u32)> = None;
    for (index, pd) in instance
        .enumerate_physical_devices()?
        .into_iter()
        .enumerate()
    {
        let props = instance.get_physical_device_properties(pd);
        if props.api_version < vk::API_VERSION_1_3 {
            continue;
        }
        let name = props.device_name_as_c_str().unwrap_or_default();
        let wished = !wish.is_empty() && matches_wish(&wish, index, &name.to_string_lossy());
        let families = instance.get_physical_device_queue_family_properties(pd);
        for (i, fam) in families.iter().enumerate() {
            let i = i as u32;
            if fam.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                && surface_loader.get_physical_device_surface_support(pd, i, surface)?
            {
                let score = match props.device_type {
                    _ if wished => 4,
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
    let (physical, family, _) =
        best.ok_or("Keine Vulkan-1.3-GPU mit Grafik- und Present-Queue gefunden")?;
    Ok((physical, family))
}

/// Kann die Karte Strahlabfragen samt Texturfeldern?
unsafe fn supports_rt(instance: &ash::Instance, pd: vk::PhysicalDevice, ext: &Extensions) -> bool {
    if !RT_EXTENSIONS.iter().all(|e| ext.has(e)) {
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

/// Uebernimmt eine Technik der Hersteller auch Reflex?
fn full_reflex(vendors: &Vendors) -> bool {
    vendors.vulkan().is_some()
}

/// Legt das Geraet an.
///
/// # Safety
/// `physical` muss zu `instance` gehoeren, `queue_family` zu `physical`.
pub unsafe fn create(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    queue_family: u32,
    want: Want,
    vendors: &Vendors,
) -> Result<Made> {
    let ext = Extensions::of(instance, physical);
    let props = instance.get_physical_device_properties(physical);
    let priorities = [1.0f32];
    let queue_info = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(queue_family)
        .queue_priorities(&priorities)];
    let use_rt = want.ray_tracing && supports_rt(instance, physical, &ext);
    let mut names = vec![ash::khr::swapchain::NAME.as_ptr()];
    if use_rt {
        names.extend(RT_EXTENSIONS.map(CStr::as_ptr));
    }
    let mut plan = latency::Plan::new(instance, physical, &ext, want.low_latency);
    if want.frame_generation && full_reflex(vendors) {
        // dann fuehrt Streamline Reflex, nie beide
        plan.reflex = false;
    }
    names.extend(plan.extensions().iter().map(|n| n.as_ptr()));
    let fault = want.diagnostics && super::diagnostics::can_fault(instance, physical, &ext);
    if fault {
        names.push(ash::ext::device_fault::NAME.as_ptr());
    }
    let budget = want.diagnostics && ext.has(ash::ext::memory_budget::NAME);
    if budget {
        names.push(ash::ext::memory_budget::NAME.as_ptr());
    }

    let hdr_metadata = want.hdr && ext.has(ash::ext::hdr_metadata::NAME);
    if hdr_metadata {
        names.push(ash::ext::hdr_metadata::NAME.as_ptr());
    }

    let can = instance.get_physical_device_features(physical);
    let gpu_culling = want.gpu_culling && can.draw_indirect_first_instance == vk::TRUE;
    let features = vk::PhysicalDeviceFeatures::default()
        .sampler_anisotropy(can.sampler_anisotropy == vk::TRUE)
        .draw_indirect_first_instance(gpu_culling);
    let max_anisotropy = if can.sampler_anisotropy == vk::TRUE {
        props.limits.max_sampler_anisotropy
    } else {
        1.0
    };
    let mut features13 = vk::PhysicalDeviceVulkan13Features::default().dynamic_rendering(true);
    let mut v12 = vk::PhysicalDeviceVulkan12Features::default()
        .buffer_device_address(use_rt)
        .descriptor_indexing(use_rt)
        .shader_sampled_image_array_non_uniform_indexing(use_rt)
        .descriptor_binding_partially_bound(use_rt)
        .timeline_semaphore(plan.reflex);
    let mut v11 = vk::PhysicalDeviceVulkan11Features::default();
    let mut matrix = vk::PhysicalDeviceCooperativeMatrixFeaturesKHR::default();
    let mut all = vk::PhysicalDeviceFeatures2::default().features(features);
    // Techniken der Hersteller setzen viel voraus: alles einschalten, was die Karte kann
    let more = if vendors.any() {
        vendors.device_extensions(instance, physical)
    } else {
        Vec::new()
    };
    let full = vendors.any();
    let use_matrix = full && ext.has(ash::khr::cooperative_matrix::NAME);
    if full {
        let mut q = vk::PhysicalDeviceFeatures2::default()
            .push_next(&mut v11)
            .push_next(&mut v12)
            .push_next(&mut features13);
        if use_matrix {
            q = q.push_next(&mut matrix);
        }
        instance.get_physical_device_features2(physical, &mut q);
        all.features = q.features;
        all.features.robust_buffer_access = vk::FALSE;
        v11.protected_memory = vk::FALSE;
        v12.buffer_device_address_capture_replay = vk::FALSE;
        for p in [
            &mut v11.p_next,
            &mut v12.p_next,
            &mut features13.p_next,
            &mut matrix.p_next,
        ] {
            *p = std::ptr::null_mut();
        }
        let wanted = more.iter().map(|e| e.as_c_str());
        let wanted = wanted.chain(use_matrix.then_some(ash::khr::cooperative_matrix::NAME));
        let wanted: Vec<_> = wanted.collect();
        for e in wanted {
            if ext.has(e) && !names.iter().any(|n| CStr::from_ptr(*n) == e) {
                names.push(e.as_ptr());
            }
        }
    }
    let mut accel =
        vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default().acceleration_structure(true);
    let mut query = vk::PhysicalDeviceRayQueryFeaturesKHR::default().ray_query(true);
    let mut present_id = vk::PhysicalDevicePresentIdFeaturesKHR::default().present_id(true);
    let mut present_wait = vk::PhysicalDevicePresentWaitFeaturesKHR::default().present_wait(true);
    let mut anti_lag = latency::antilag::Features::enabled();
    let mut device_fault = vk::PhysicalDeviceFaultFeaturesEXT::default().device_fault(true);
    let mut info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_info)
        .enabled_extension_names(&names)
        .push_next(&mut features13);
    if full {
        info = info.push_next(&mut v11);
        if use_matrix {
            info = info.push_next(&mut matrix);
        }
    }
    if use_rt || plan.reflex || full {
        info = info.push_next(&mut v12);
    }
    if use_rt {
        info = info.push_next(&mut accel).push_next(&mut query);
    }
    if plan.present_id {
        info = info.push_next(&mut present_id);
    }
    if plan.present_wait {
        info = info.push_next(&mut present_wait);
    }
    if plan.anti_lag {
        info = info.push_next(&mut anti_lag);
    }
    if fault {
        info = info.push_next(&mut device_fault);
    }
    if full {
        // Faehigkeiten als Kettenanfang, damit die Bibliotheken sie ergaenzen koennen
        info = info.push_next(&mut all);
        let head = vendors.patch_features(instance, physical, info.p_next as *mut _);
        info.p_next = head;
    } else {
        info = info.enabled_features(&features);
    }
    let device = instance.create_device(physical, &info, None)?;
    let rt = use_rt.then(|| {
        let mut limits = vk::PhysicalDeviceAccelerationStructurePropertiesKHR::default();
        let mut props = vk::PhysicalDeviceProperties2::default().push_next(&mut limits);
        instance.get_physical_device_properties2(physical, &mut props);
        RtFns {
            accel: ash::khr::acceleration_structure::Device::new(instance, &device),
            scratch_align: limits.min_acceleration_structure_scratch_offset_alignment as u64,
        }
    });
    Ok(Made {
        low_latency: Support::load(instance, &device, plan),
        diagnostics: Diagnostics::new(instance, &device, fault, budget),
        hdr_metadata,
        gpu_culling,
        rt,
        max_anisotropy,
        device,
    })
}

#[cfg(test)]
mod tests {
    use super::matches_wish;

    #[test]
    fn gpu_can_be_wished_by_number_or_name() {
        assert!(matches_wish("1", 1, "AMD Radeon(TM) Graphics"));
        assert!(!matches_wish("1", 0, "NVIDIA GeForce RTX 5090"));
        assert!(matches_wish("radeon", 7, "AMD Radeon(TM) Graphics"));
        assert!(!matches_wish("radeon", 0, "NVIDIA GeForce RTX 5090"));
    }
}
