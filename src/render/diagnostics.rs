//! Zuschaltbare Diagnose: Grund eines Grafikkarten-Ausfalls und belegter Grafikspeicher.

use super::device::Extensions;
use ash::vk;
use std::ptr;

/// Hoechstens so viele Eintraege je Liste landen im Absturzbericht
const MAX_ENTRIES: usize = 8;

/// Kann die Karte melden, warum sie ausgefallen ist?
///
/// # Safety
/// `pd` muss zu `instance` gehoeren.
pub unsafe fn can_fault(
    instance: &ash::Instance,
    pd: vk::PhysicalDevice,
    ext: &Extensions,
) -> bool {
    if !ext.has(ash::ext::device_fault::NAME) {
        return false;
    }
    let mut f = vk::PhysicalDeviceFaultFeaturesEXT::default();
    let mut all = vk::PhysicalDeviceFeatures2::default().push_next(&mut f);
    instance.get_physical_device_features2(pd, &mut all);
    f.device_fault == vk::TRUE
}

/// Zugeschaltete Diagnose eines Geraets.
pub struct Diagnostics {
    fault: Option<ash::ext::device_fault::Device>,
    budget: bool,
}

impl Diagnostics {
    pub fn new(instance: &ash::Instance, device: &ash::Device, fault: bool, budget: bool) -> Self {
        Self {
            fault: fault.then(|| ash::ext::device_fault::Device::new(instance, device)),
            budget,
        }
    }

    /// Grafikspeicher der Karte: belegt und verfuegbar (Byte). `None` = nicht messbar.
    pub fn memory(&self, instance: &ash::Instance, pd: vk::PhysicalDevice) -> Option<(u64, u64)> {
        if !self.budget {
            return None;
        }
        let mut budget = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
        // SAFETY: Die Erweiterung ist eingeschaltet, `pd` gehoert zur Instanz.
        let heaps = unsafe {
            let mut p = vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budget);
            instance.get_physical_device_memory_properties2(pd, &mut p);
            p.memory_properties
        };
        (0..heaps.memory_heap_count as usize)
            .filter(|&i| {
                let local = vk::MemoryHeapFlags::DEVICE_LOCAL;
                heaps.memory_heaps[i].flags.contains(local)
            })
            .map(|i| (budget.heap_usage[i], budget.heap_budget[i]))
            .max_by_key(|h| h.1)
    }

    /// Nach einem Ausfall: was der Treiber ueber den Grund weiss. `None` = nichts.
    pub fn fault_report(&self) -> Option<String> {
        let f = self.fault.as_ref()?;
        let get = f.fp().get_device_fault_info_ext;
        let mut counts = vk::DeviceFaultCountsEXT::default();
        // SAFETY: Geraet mit eingeschalteter Erweiterung; die Listen sind so lang wie gemeldet.
        unsafe {
            let r = get(f.device(), &mut counts, ptr::null_mut());
            if r != vk::Result::SUCCESS {
                return None;
            }
            let mut addresses =
                vec![vk::DeviceFaultAddressInfoEXT::default(); counts.address_info_count as usize];
            let mut vendor =
                vec![vk::DeviceFaultVendorInfoEXT::default(); counts.vendor_info_count as usize];
            counts.vendor_binary_size = 0;
            let mut info = vk::DeviceFaultInfoEXT {
                p_address_infos: addresses.as_mut_ptr(),
                p_vendor_infos: vendor.as_mut_ptr(),
                ..Default::default()
            };
            let r = get(f.device(), &mut counts, &mut info);
            if r != vk::Result::SUCCESS && r != vk::Result::INCOMPLETE {
                return None;
            }
            let text = |c: &[std::ffi::c_char]| {
                let bytes: Vec<u8> = c.iter().map(|&b| b as u8).take_while(|&b| b != 0).collect();
                String::from_utf8_lossy(&bytes).into_owned()
            };
            let mut out = text(&info.description);
            for a in addresses.iter().take(MAX_ENTRIES) {
                out += &format!("; {:?} bei {:#x}", a.address_type, a.reported_address);
            }
            for v in vendor.iter().take(MAX_ENTRIES) {
                out += &format!(
                    "; {} ({:#x}, {:#x})",
                    text(&v.description),
                    v.vendor_fault_code,
                    v.vendor_fault_data
                );
            }
            Some(out)
        }
    }
}
