// Moderne Grafik: Daten der Szene (Set 0). Aufbau wie in src/render/modern (gpu.rs, sets.rs).

struct Scene {
    view_proj: mat4x4<f32>,      // mit TAA-Versatz
    plain_view_proj: mat4x4<f32>,
    prev_view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,  // zu view_proj
    cascade_vp: array<mat4x4<f32>, 4>,
    cam_pos: vec4<f32>,          // w = Zeit
    cam_dir: vec4<f32>,          // Blickrichtung, w = Schaerfung der Texturen beim Hochskalieren
    sun_dir: vec4<f32>,          // Richtung zur Sonne, w = 1: Sonne an
    sun_color: vec4<f32>,        // w = 1: mit Schatten
    cascade_ends: vec4<f32>,
    cascade_texels: vec4<f32>,
    sky_a: vec4<f32>,            // Zenit (bzw. Faktor des Bildes), w = 1: Bild
    sky_b: vec4<f32>,            // Horizont, w = Umgebungslicht-Faktor
    sky_c: vec4<f32>,            // Boden, w = Stufen des Himmelsbilds
    fog: vec4<f32>,              // rgb, w = Dichte
    misc: vec4<f32>,             // x = Nebel-Abnahme, y = Verdeckung an, z = 1/Schattengroesse, w = 1/Atlasgroesse
    cluster: vec4<f32>,          // x, y = Tiefenscheibe, z, w = Zellen pro Bildpunkt
    screen: vec4<f32>,           // Breite, Hoehe, 1/Breite, 1/Hoehe
    rt: vec4<f32>,               // Strahlen: x = Sonne, y = Streulicht, z = Spiegelung, w = Bildnummer
    counts: vec4<f32>,           // x = Lampen, y = Sonnenradius, z = Reichweite der Verdeckung, w = Staerke
    tiles: array<Tile, 64>,
};

// Ein Feld im Schatten-Atlas der Lampen
struct Tile {
    view_proj: mat4x4<f32>,
    rect: vec4<f32>, // Ursprung und Groesse im Atlas (0..1)
};

struct Obj {
    model: mat4x4<f32>,
    prev: mat4x4<f32>,
    tint: vec4<f32>,
    extra: vec4<f32>, // x = erster Knochen, y = Material, z = -, w = Eigenleuchten
};

struct Lamp {
    pos: vec4<f32>,   // w = Reichweite
    color: vec4<f32>, // w = 1: Kegel
    dir: vec4<f32>,   // w = Kosinus aussen
    extra: vec4<f32>, // x = Kosinus innen, y = erstes Schattenfeld (-1 = keins)
};

struct Mat {
    base: vec4<f32>,
    emissive: vec4<f32>, // w = Staerke der Normalenkarte
    params: vec4<f32>,   // Metall, Rauheit, Grenze der Loecher, Farbtextur fuer Strahlen (0 = keine)
};

@group(0) @binding(0) var<storage, read> objs: array<Obj>;
@group(0) @binding(1) var<storage, read> bones: array<mat4x4<f32>>;
@group(0) @binding(2) var<uniform> s: Scene;
@group(0) @binding(3) var<storage, read> lamps: array<Lamp>;
@group(0) @binding(4) var<storage, read> cells: array<vec2<u32>>;
@group(0) @binding(5) var<storage, read> entries: array<u32>;
@group(0) @binding(6) var<storage, read> mats: array<Mat>;
@group(0) @binding(7) var sun_shadow: texture_depth_2d_array;
@group(0) @binding(8) var lamp_shadow: texture_depth_2d;
@group(0) @binding(9) var shadow_samp: sampler_comparison;
@group(0) @binding(10) var ao_tex: texture_2d<f32>;
@group(0) @binding(11) var linear_samp: sampler;
@group(0) @binding(12) var sky_tex: texture_2d<f32>;
@group(0) @binding(13) var depth_tex: texture_depth_2d;

@group(0) @binding(14) var<storage, read> order: array<u32>;
@group(0) @binding(15) var gi_tex: texture_2d<f32>;
@group(0) @binding(16) var refl_tex: texture_2d<f32>;

