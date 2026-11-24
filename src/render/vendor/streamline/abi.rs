//! Strukturen und Funktionstypen von NVIDIA Streamline 2.14 (`sl_core_types.h`, `sl_consts.h`,
//! `sl_dlss.h`, `sl_dlss_d.h`).

use ash::vk;
use std::ffi::{c_char, c_void};
use std::ptr;

/// Version, nach deren Beschreibung dies geschrieben ist (2.14.16, dazu die Kennzahl)
pub const SDK_VERSION: u64 = 2 << 48 | 14 << 32 | 16 << 16 | 0xfedc;
pub const OK: i32 = 0;

pub const FEATURE_DLSS: u32 = 0;
pub const FEATURE_RR: u32 = 1001;

/// Befehlspuffer nicht mitverfolgen (wir setzen alles neu), kein Text im Bild, Bilder je Bildnummer
/// anmelden. Kein Nachladen aus dem Netz.
pub const FLAGS: u64 = 1 | 1 << 1 | 1 << 7;
pub const API_VULKAN: u32 = 2;

pub const TAG_DEPTH: u32 = 0;
pub const TAG_MOTION: u32 = 1;
pub const TAG_COLOR_IN: u32 = 3;
pub const TAG_COLOR_OUT: u32 = 4;
pub const TAG_ALBEDO: u32 = 7;
pub const TAG_SPECULAR: u32 = 8;
pub const TAG_NORMAL_ROUGHNESS: u32 = 14;
pub const TAG_REACTIVE: u32 = 36;

/// Kopf jeder Struktur.
#[repr(C)]
pub struct Base {
    pub next: *mut c_void,
    pub id: [u32; 4],
    pub version: usize,
}

/// Kennung aus ihren drei Teilen (wie im Quelltext der Beschreibung).
pub const fn id(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u32; 4] {
    [
        a,
        b as u32 | (c as u32) << 16,
        u32::from_le_bytes([d[0], d[1], d[2], d[3]]),
        u32::from_le_bytes([d[4], d[5], d[6], d[7]]),
    ]
}

pub const fn base(id: [u32; 4], version: usize) -> Base {
    Base {
        next: ptr::null_mut(),
        id,
        version,
    }
}

pub type Log = unsafe extern "C" fn(u32, *const c_char);

#[repr(C)]
pub struct Preferences {
    base: Base,
    pub console: bool,
    pub log_level: u32,
    pub plugin_paths: *const *const u16,
    pub plugin_path_count: u32,
    pub log_path: *const u16,
    pub allocate: *const c_void,
    pub release: *const c_void,
    pub log: Option<Log>,
    pub flags: u64,
    pub features: *const u32,
    pub feature_count: u32,
    pub application: u32,
    pub engine: u32,
    pub engine_version: *const c_char,
    pub project: *const c_char,
    pub api: u32,
}

impl Preferences {
    pub fn new() -> Self {
        let guid = id(
            0x1ca1_0965,
            0xbf8e,
            0x432b,
            [0x8d, 0xa1, 0x67, 0x16, 0xd8, 0x79, 0xfb, 0x14],
        );
        Self {
            base: base(guid, 1),
            console: false,
            log_level: 1,
            plugin_paths: ptr::null(),
            plugin_path_count: 0,
            log_path: ptr::null(),
            allocate: ptr::null(),
            release: ptr::null(),
            log: None,
            flags: FLAGS,
            features: ptr::null(),
            feature_count: 0,
            application: 0,
            engine: 0,
            engine_version: ptr::null(),
            project: ptr::null(),
            api: API_VULKAN,
        }
    }
}

#[repr(C)]
pub struct AdapterInfo {
    base: Base,
    luid: *const u8,
    luid_size: u32,
    physical: vk::PhysicalDevice,
}

impl AdapterInfo {
    pub fn new(physical: vk::PhysicalDevice) -> Self {
        let guid = id(
            0x0677_315f,
            0xa746,
            0x4492,
            [0x9f, 0x42, 0xcb, 0x61, 0x42, 0xc9, 0xc3, 0xd4],
        );
        Self {
            base: base(guid, 1),
            luid: ptr::null(),
            luid_size: 0,
            physical,
        }
    }
}

#[repr(C)]
pub struct Viewport {
    base: Base,
    value: u32,
}

impl Viewport {
    pub fn new(value: u32) -> Self {
        let guid = id(
            0x171b_6435,
            0x9b3c,
            0x4fc8,
            [0x99, 0x94, 0xfb, 0xe5, 0x25, 0x69, 0xaa, 0xa4],
        );
        Self {
            base: base(guid, 1),
            value,
        }
    }
}

#[repr(C)]
pub struct Resource {
    base: Base,
    kind: i8,
    native: vk::Image,
    memory: *const c_void,
    view: vk::ImageView,
    state: u32,
    width: u32,
    height: u32,
    format: i32,
    mips: u32,
    layers: u32,
    address: u64,
    flags: u32,
    usage: u32,
    internal: u16,
    reserved: u16,
}

impl Resource {
    pub fn new(
        p: &super::super::Picture,
        layout: vk::ImageLayout,
        usage: vk::ImageUsageFlags,
    ) -> Self {
        let guid = id(
            0x3a9d_70cf,
            0x2418,
            0x4b72,
            [0x83, 0x91, 0x13, 0xf8, 0x72, 0x1c, 0x72, 0x61],
        );
        Self {
            base: base(guid, 1),
            kind: 0,
            native: p.image,
            memory: ptr::null(),
            view: p.view,
            state: layout.as_raw() as u32,
            width: p.extent.width,
            height: p.extent.height,
            format: p.format.as_raw(),
            mips: 1,
            layers: 1,
            address: 0,
            flags: 0,
            usage: usage.as_raw(),
            internal: 0,
            reserved: 0,
        }
    }
}

#[repr(C)]
pub struct Tag {
    base: Base,
    resource: *const Resource,
    kind: u32,
    lifecycle: i32,
    /// oben, links, Breite, Hoehe
    extent: [u32; 4],
}

impl Tag {
    /// Gilt so lange wie `lifecycle` sagt (1 = bis zum Anzeigen, 2 = bis zur Auswertung).
    pub fn lasting(resource: &Resource, kind: u32, lifecycle: i32) -> Self {
        Self {
            lifecycle,
            ..Self::new(resource, kind)
        }
    }

    /// Gilt bis zur Auswertung.
    pub fn new(resource: &Resource, kind: u32) -> Self {
        let guid = id(
            0x4c6a_5aad,
            0xb445,
            0x496c,
            [0x87, 0xff, 0x1a, 0xf3, 0x84, 0x5b, 0xe6, 0x53],
        );
        Self {
            base: base(guid, 1),
            resource,
            kind,
            lifecycle: 2,
            extent: [0, 0, resource.width, resource.height],
        }
    }
}

pub type M4 = [[f32; 4]; 4];
/// 0 = nein, 1 = ja, 2 = unbekannt
pub type Bool = i8;

#[repr(C)]
pub struct Constants {
    base: Base,
    pub view_to_clip: M4,
    pub clip_to_view: M4,
    pub clip_to_lens: M4,
    pub clip_to_prev: M4,
    pub prev_to_clip: M4,
    pub jitter: [f32; 2],
    pub motion_scale: [f32; 2],
    pub pinhole: [f32; 2],
    pub pos: [f32; 3],
    pub up: [f32; 3],
    pub right: [f32; 3],
    pub forward: [f32; 3],
    pub near: f32,
    pub far: f32,
    pub fov: f32,
    pub aspect: f32,
    pub invalid_motion: f32,
    pub depth_inverted: Bool,
    pub camera_motion: Bool,
    pub motion_3d: Bool,
    pub reset: Bool,
    pub ortho: Bool,
    pub dilated: Bool,
    pub jittered: Bool,
    pub separation: f32,
}

impl Constants {
    pub fn new() -> Self {
        let guid = id(
            0xdcd3_5ad7,
            0x4e4a,
            0x4bad,
            [0xa9, 0x0c, 0xe0, 0xc4, 0x9e, 0xb2, 0x3a, 0xfe],
        );
        let unit = glam::Mat4::IDENTITY.to_cols_array_2d();
        Self {
            base: base(guid, 2),
            view_to_clip: unit,
            clip_to_view: unit,
            clip_to_lens: unit,
            clip_to_prev: unit,
            prev_to_clip: unit,
            jitter: [0.0; 2],
            motion_scale: [1.0; 2],
            pinhole: [0.0; 2],
            pos: [0.0; 3],
            up: [0.0; 3],
            right: [0.0; 3],
            forward: [0.0; 3],
            near: 0.1,
            far: 100.0,
            fov: 1.0,
            aspect: 1.0,
            invalid_motion: f32::MAX,
            depth_inverted: 0,
            camera_motion: 1,
            motion_3d: 0,
            reset: 0,
            ortho: 0,
            dilated: 0,
            jittered: 0,
            separation: 40.0,
        }
    }
}

#[repr(C)]
pub struct DlssOptions {
    base: Base,
    pub mode: u32,
    pub width: u32,
    pub height: u32,
    sharpness: f32,
    pre_exposure: f32,
    exposure_scale: f32,
    hdr: Bool,
    invert_x: Bool,
    invert_y: Bool,
    presets: [u32; 6],
    auto_exposure: Bool,
    alpha: Bool,
}

impl DlssOptions {
    pub fn new(mode: u32, [width, height]: [u32; 2]) -> Self {
        let guid = id(
            0x6ac8_26e4,
            0x4c61,
            0x4101,
            [0xa9, 0x2d, 0x63, 0x8d, 0x42, 0x10, 0x57, 0xb8],
        );
        Self {
            base: base(guid, 3),
            mode,
            width,
            height,
            sharpness: 0.0,
            pre_exposure: 1.0,
            exposure_scale: 1.0,
            hdr: 1,
            invert_x: 0,
            invert_y: 0,
            presets: [0; 6],
            auto_exposure: 1,
            alpha: 0,
        }
    }
}

#[repr(C)]
pub struct RrOptions {
    base: Base,
    pub mode: u32,
    pub width: u32,
    pub height: u32,
    sharpness: f32,
    pre_exposure: f32,
    exposure_scale: f32,
    hdr: Bool,
    invert_x: Bool,
    invert_y: Bool,
    /// 1 = Rauheit im vierten Kanal der Normalen
    normal_mode: u32,
    pub world_to_view: M4,
    pub view_to_world: M4,
    alpha: Bool,
    presets: [u32; 6],
}

impl RrOptions {
    pub fn new(mode: u32, [width, height]: [u32; 2], view: glam::Mat4) -> Self {
        let guid = id(
            0x0ad8_7504,
            0x774e,
            0x4bf3,
            [0x96, 0x33, 0xa4, 0x4d, 0x1f, 0x7f, 0x9c, 0xb8],
        );
        Self {
            base: base(guid, 3),
            mode,
            width,
            height,
            sharpness: 0.0,
            pre_exposure: 1.0,
            exposure_scale: 1.0,
            hdr: 1,
            invert_x: 0,
            invert_y: 0,
            normal_mode: 1,
            world_to_view: view.to_cols_array_2d(),
            view_to_world: view.inverse().to_cols_array_2d(),
            alpha: 0,
            presets: [0; 6],
        }
    }
}

#[repr(C)]
pub struct FeatureVersion {
    base: Base,
    pub streamline: [u32; 3],
    pub ngx: [u32; 3],
}

impl FeatureVersion {
    pub fn new() -> Self {
        let guid = id(
            0x6d5b_51f0,
            0x076b,
            0x486d,
            [0x99, 0x95, 0x5a, 0x56, 0x10, 0x43, 0xf5, 0xc1],
        );
        Self {
            base: base(guid, 1),
            streamline: [0; 3],
            ngx: [0; 3],
        }
    }
}

/// Ein Bild; gehoert der Bibliothek.
pub type Token = *mut c_void;

pub type Init = unsafe extern "C" fn(*const Preferences, u64) -> i32;
pub type Shutdown = unsafe extern "C" fn() -> i32;
pub type IsSupported = unsafe extern "C" fn(u32, *const AdapterInfo) -> i32;
pub type Evaluate =
    unsafe extern "C" fn(u32, Token, *const *const c_void, u32, vk::CommandBuffer) -> i32;
pub type FreeResources = unsafe extern "C" fn(u32, *const Viewport) -> i32;
pub type SetTags =
    unsafe extern "C" fn(Token, *const Viewport, *const Tag, u32, vk::CommandBuffer) -> i32;
pub type SetConstants = unsafe extern "C" fn(*const Constants, Token, *const Viewport) -> i32;
pub type GetVersion = unsafe extern "C" fn(u32, *mut FeatureVersion) -> i32;
pub type GetFunction = unsafe extern "C" fn(u32, *const c_char, *mut *mut c_void) -> i32;
pub type NewToken = unsafe extern "C" fn(*mut Token, *const u32) -> i32;
pub type SetDlss = unsafe extern "C" fn(*const Viewport, *const DlssOptions) -> i32;
pub type SetRr = unsafe extern "C" fn(*const Viewport, *const RrOptions) -> i32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structures_match_the_headers() {
        assert_eq!(size_of::<Base>(), 32);
        assert_eq!(size_of::<Viewport>(), 40);
        assert_eq!(size_of::<Resource>(), 32 + 8 + 24 + 24 + 8 + 8 + 8);
        assert_eq!(size_of::<Tag>(), 32 + 8 + 8 + 16);
        // 5 Matrizen, 18 + 5 Zahlen, 7 Wahrheitswerte (auf 8), 1 Zahl
        assert_eq!(size_of::<Constants>(), 32 + 320 + 23 * 4 + 8 + 4);
        assert_eq!(size_of::<DlssOptions>(), 32 + 24 + 4 + 24 + 4);
        assert_eq!(size_of::<RrOptions>(), 32 + 24 + 4 + 4 + 128 + 4 + 24 + 4);
        assert_eq!(size_of::<Preferences>(), 144);
    }

    #[test]
    fn identifiers_keep_their_byte_order() {
        let v = Viewport::new(1);
        let bytes: Vec<u8> = v.base.id.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(bytes[..4], 0x171b_6435u32.to_le_bytes());
        assert_eq!(bytes[4..6], 0x9b3cu16.to_le_bytes());
        assert_eq!(bytes[8..], [0x99, 0x94, 0xfb, 0xe5, 0x25, 0x69, 0xaa, 0xa4]);
    }
}
