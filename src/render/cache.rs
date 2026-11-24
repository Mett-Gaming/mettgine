//! Pipeline-Cache: uebersetzte Shader von einem Start zum naechsten aufheben. Die Datei liegt im
//! Datenordner des Spiels (`crash::init` muss gelaufen sein, sonst gibt es keine).

use super::context::Context;
use ash::vk;
use std::path::PathBuf;

const FILE: &str = "pipeline-cache.bin";
/// Groessere Dateien werden nicht gelesen
const MAX_BYTES: u64 = 64 << 20;

/// Geraet samt Cache, zum Bauen von Pipelines.
pub(crate) struct Gpu<'a> {
    device: &'a ash::Device,
    pub cache: vk::PipelineCache,
}

impl std::ops::Deref for Gpu<'_> {
    type Target = ash::Device;
    fn deref(&self) -> &ash::Device {
        self.device
    }
}

impl Context {
    pub fn gpu(&self) -> Gpu<'_> {
        Gpu {
            device: &self.device,
            cache: self.cache,
        }
    }
}

fn path() -> Option<PathBuf> {
    crate::crash::current_dir().map(|d| d.join(FILE))
}

/// Gehoert der Inhalt zu genau dieser Karte und diesem Treiber? (Kopf nach Vulkan-Vorgabe)
fn fits(data: &[u8], props: &vk::PhysicalDeviceProperties) -> bool {
    let u32_at = |o: usize| {
        data.get(o..o + 4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
    };
    u32_at(0).is_some_and(|n| n as usize >= 32 && n as usize <= data.len())
        && u32_at(4) == Some(vk::PipelineCacheHeaderVersion::ONE.as_raw() as u32)
        && u32_at(8) == Some(props.vendor_id)
        && u32_at(12) == Some(props.device_id)
        && data.get(16..32) == Some(&props.pipeline_cache_uuid[..])
}

fn read(props: &vk::PhysicalDeviceProperties) -> Vec<u8> {
    let Some(p) = path() else {
        return Vec::new();
    };
    let small = std::fs::metadata(&p).is_ok_and(|m| m.len() <= MAX_BYTES);
    let data = if small {
        std::fs::read(p).unwrap_or_default()
    } else {
        Vec::new()
    };
    if fits(&data, props) {
        data
    } else {
        Vec::new()
    }
}

/// Cache anlegen. Ohne brauchbare Datei leer; geht auch das nicht, ohne Cache.
///
/// # Safety
/// `device` muss gueltig sein.
pub(crate) unsafe fn open(
    device: &ash::Device,
    props: &vk::PhysicalDeviceProperties,
) -> vk::PipelineCache {
    let data = read(props);
    let make = |data: &[u8]| {
        let info = vk::PipelineCacheCreateInfo::default().initial_data(data);
        device.create_pipeline_cache(&info, None)
    };
    make(&data)
        .or_else(|_| make(&[]))
        .unwrap_or(vk::PipelineCache::null())
}

/// Speichern (wenn sich die Groesse geaendert hat) und freigeben.
///
/// # Safety
/// Der Cache wird nicht mehr benutzt.
pub(crate) unsafe fn close(device: &ash::Device, cache: vk::PipelineCache) {
    if cache == vk::PipelineCache::null() {
        return;
    }
    if let (Some(p), Ok(data)) = (path(), device.get_pipeline_cache_data(cache)) {
        let old = std::fs::metadata(&p).map_or(0, |m| m.len());
        if !data.is_empty() && data.len() as u64 <= MAX_BYTES && data.len() as u64 != old {
            if let Err(e) = crate::settings::write_atomic(&p, &data) {
                crate::crash::log(&format!("Pipeline-Cache nicht gespeichert: {e}"));
            }
        }
    }
    device.destroy_pipeline_cache(cache, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_data_of_this_card_and_driver_is_used() {
        let mut props = vk::PhysicalDeviceProperties {
            vendor_id: 7,
            device_id: 9,
            ..Default::default()
        };
        props.pipeline_cache_uuid = [3; 16];
        let mut data = Vec::new();
        for v in [32u32, 1, 7, 9] {
            data.extend(v.to_le_bytes());
        }
        data.extend([3u8; 16]);
        data.extend([0u8; 40]);
        assert!(fits(&data, &props));
        for at in [4, 8, 12, 20] {
            let mut bad = data.clone();
            bad[at] ^= 1;
            assert!(!fits(&bad, &props), "Byte {at}");
        }
        for cut in 0..32 {
            assert!(!fits(&data[..cut], &props));
        }
    }
}
