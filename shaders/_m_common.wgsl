// Moderne Grafik: gemeinsame Daten. Aufbau wie in src/render/modern (gpu.rs, sets.rs).

struct Scene {
    view_proj: mat4x4<f32>,      // mit TAA-Versatz
    plain_view_proj: mat4x4<f32>,
    prev_view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,  // zu view_proj
    cascade_vp: array<mat4x4<f32>, 4>,
    cam_pos: vec4<f32>,          // w = Zeit
    cam_dir: vec4<f32>,          // Blickrichtung
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
    params: vec4<f32>,   // Metall, Rauheit, Grenze der Loecher, -
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

@group(1) @binding(0) var base_tex: texture_2d<f32>;
@group(1) @binding(1) var metal_rough_tex: texture_2d<f32>;
@group(1) @binding(2) var normal_tex: texture_2d<f32>;
@group(1) @binding(3) var occlusion_tex: texture_2d<f32>;
@group(1) @binding(4) var emissive_tex: texture_2d<f32>;
@group(1) @binding(5) var mat_samp: sampler;

struct Push {
    view_proj: mat4x4<f32>, // Schatten: Sicht der Lampe
};
var<push_constant> pc: Push;

struct VsOut {
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) now: vec4<f32>,
    @location(6) before: vec4<f32>,
    @location(7) @interpolate(flat) material: u32,
    @location(8) glow: f32,
};

struct ShadowOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
    @location(2) @interpolate(flat) material: u32,
};

fn finish(o: Obj, world: vec3<f32>, before: vec3<f32>, n: vec3<f32>, t: vec4<f32>, uv: vec2<f32>, color: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.clip = s.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.normal = n;
    out.tangent = t;
    out.uv = uv;
    out.color = color * o.tint;
    out.now = s.plain_view_proj * vec4<f32>(world, 1.0);
    out.before = s.prev_view_proj * vec4<f32>(before, 1.0);
    out.material = u32(o.extra.y + 0.5);
    out.glow = o.extra.w;
    return out;
}

fn finish_shadow(o: Obj, world: vec3<f32>, uv: vec2<f32>, alpha: f32) -> ShadowOut {
    var out: ShadowOut;
    out.clip = pc.view_proj * vec4<f32>(world, 1.0);
    out.uv = uv;
    out.alpha = alpha * o.tint.a;
    out.material = u32(o.extra.y + 0.5);
    return out;
}

fn skin_matrix(base: u32, j: vec4<u32>, w: vec4<f32>) -> mat4x4<f32> {
    return bones[base + j.x] * w.x + bones[base + j.y] * w.y
        + bones[base + j.z] * w.z + bones[base + j.w] * w.w;
}

// Vertexfarben der einfachen Modelle sind Anzeigewerte
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

@fragment
fn fs_shadow(in: ShadowOut) {
    let m = mats[in.material];
    let a = textureSample(base_tex, mat_samp, in.uv).a * m.base.a * in.alpha;
    if (a < m.params.z) {
        discard;
    }
}

struct PreOut {
    @location(0) normal: vec4<f32>,
    @location(1) motion: vec2<f32>,
};

fn surface_normal(in: VsOut, m: Mat, front: bool) -> vec3<f32> {
    let k = textureSample(normal_tex, mat_samp, in.uv).xyz * 2.0 - vec3<f32>(1.0);
    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    if (m.emissive.w == 0.0) {
        return n;
    }
    let t = in.tangent.xyz - n * dot(n, in.tangent.xyz);
    let len = length(t);
    if (len < 0.0001) {
        return n;
    }
    let tn = t / len;
    let b = cross(n, tn) * in.tangent.w;
    return normalize(tn * k.x * m.emissive.w + b * k.y * m.emissive.w + n * max(k.z, 0.001));
}

// Tiefe zuerst: Normale (fuer die Verdeckung) und Bewegung (fuer TAA)
@fragment
fn fs_pre(in: VsOut, @builtin(front_facing) front: bool) -> PreOut {
    let m = mats[in.material];
    let a = textureSample(base_tex, mat_samp, in.uv).a * m.base.a * in.color.a;
    let n = surface_normal(in, m, front);
    if (a < m.params.z) {
        discard;
    }
    var out: PreOut;
    out.normal = vec4<f32>(n * 0.5 + vec3<f32>(0.5), 1.0);
    let now = in.now.xy / in.now.w;
    let before = in.before.xy / in.before.w;
    // Verschiebung im Bild (0..1, y nach unten)
    out.motion = (now - before) * vec2<f32>(0.5, -0.5);
    return out;
}
