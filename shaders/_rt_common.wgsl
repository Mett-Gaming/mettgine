// Strahlen: Treffer, Licht am Treffer, Zufall. Set 1 siehe src/render/modern/rt.

#include "_m_scene.wgsl"
#include "_m_sky.wgsl"

struct Tri {
    n0: vec4<f32>, // Normale, w = u
    n1: vec4<f32>,
    n2: vec4<f32>,
    v: vec4<f32>,  // v der drei Ecken
    color: vec4<f32>,
};

struct Info {
    tint: vec4<f32>,
    first: u32,    // erstes Dreieck
    material: u32,
    glow: f32,
    figure: u32,   // 1 = Dreiecke in `figure_tris`
};

@group(1) @binding(0) var acc: acceleration_structure;
@group(1) @binding(1) var<storage, read> tris: array<Tri>;
@group(1) @binding(2) var<storage, read> infos: array<Info>;
@group(1) @binding(3) var depth_img: texture_2d<f32>;
@group(1) @binding(4) var normal_img: texture_2d<f32>;
@group(1) @binding(5) var textures: binding_array<texture_2d<f32>, 512>;
@group(1) @binding(6) var tex_samp: sampler;
@group(1) @binding(7) var<storage, read> figure_tris: array<Tri>;

struct Push {
    a: vec4<f32>,
    b: vec4<f32>,
    inv: mat4x4<f32>,
    c: vec4<f32>,
    d: vec4<f32>,
};
var<push_constant> pc: Push;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    var out: VsOut;
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

var<private> seed: u32;

fn start_random(px: vec2<f32>) {
    let p = vec2<u32>(px);
    seed = (p.x * 1973u + p.y * 9277u + u32(s.rt.w) * 26699u) | 1u;
    seed = seed * 747796405u + 2891336453u;
}

fn rnd() -> f32 {
    seed = seed * 747796405u + 2891336453u;
    let w = ((seed >> ((seed >> 28u) + 4u)) ^ seed) * 277803737u;
    return f32(((w >> 22u) ^ w) >> 8u) / 16777216.0;
}

fn basis(n: vec3<f32>) -> mat3x3<f32> {
    let k = select(1.0, -1.0, n.z < 0.0);
    let a = -1.0 / (k + n.z);
    let b = n.x * n.y * a;
    return mat3x3<f32>(
        vec3<f32>(1.0 + k * n.x * n.x * a, k * b, -k * n.x),
        vec3<f32>(b, k + n.y * n.y * a, -n.y),
        n);
}

fn cosine_dir(n: vec3<f32>) -> vec3<f32> {
    let u = rnd();
    let a = rnd() * 2.0 * PI;
    let r = sqrt(u);
    return normalize(basis(n) * vec3<f32>(cos(a) * r, sin(a) * r, sqrt(max(1.0 - u, 0.0))));
}

// Spiegelnde Mikroflaeche (GGX), `rough` = wahrgenommene Rauheit
fn ggx_normal(n: vec3<f32>, rough: f32) -> vec3<f32> {
    let a = rough * rough;
    let u = rnd();
    let phi = rnd() * 2.0 * PI;
    let c = sqrt((1.0 - u) / (1.0 + (a * a - 1.0) * u));
    let t = sqrt(max(1.0 - c * c, 0.0));
    return normalize(basis(n) * vec3<f32>(cos(phi) * t, sin(phi) * t, c));
}

// Richtung zur Sonne mit kleiner Streuung (weiche Schattenraender)
fn sun_sample() -> vec3<f32> {
    let a = rnd() * 2.0 * PI;
    let r = sqrt(rnd()) * s.counts.y;
    return normalize(s.sun_dir.xyz + basis(s.sun_dir.xyz) * vec3<f32>(cos(a) * r, sin(a) * r, 0.0));
}

struct Found {
    t: f32, // < 0 = nichts
    info: u32,
    tri: u32,
    bary: vec2<f32>,
    m: mat4x3<f32>,
};

fn triangle(info: Info, k: u32) -> Tri {
    if (info.figure == 1u) {
        return figure_tris[info.first + k];
    }
    return tris[info.first + k];
}

fn uv_of(info: Info, f: Found) -> vec2<f32> {
    let tri = triangle(info, f.tri);
    let b = vec3<f32>(1.0 - f.bary.x - f.bary.y, f.bary.x, f.bary.y);
    return vec2<f32>(
        tri.n0.w * b.x + tri.n1.w * b.y + tri.n2.w * b.z,
        tri.v.x * b.x + tri.v.y * b.y + tri.v.z * b.z);
}

// Trifft der Strahl ein Loch (Blaetter, Gitter)?
fn is_hole(f: Found) -> bool {
    let info = infos[f.info];
    let m = mats[info.material];
    if (m.params.z <= 0.0 || m.params.w < 0.5) {
        return false;
    }
    let k = u32(m.params.w - 0.5);
    let a = textureSampleLevel(textures[k], tex_samp, uv_of(info, f), 1.0).a;
    return a * m.base.a < m.params.z;
}

fn find(o: vec3<f32>, d: vec3<f32>, from_t: f32, far: f32, flags: u32) -> Found {
    var f: Found;
    f.t = -1.0;
    var rq: ray_query;
    rayQueryInitialize(&rq, acc, RayDesc(flags, 0xFFu, from_t, far, o, d));
    rayQueryProceed(&rq);
    let hit = rayQueryGetCommittedIntersection(&rq);
    if (hit.kind != RAY_QUERY_INTERSECTION_NONE) {
        f.t = hit.t;
        f.info = hit.instance_custom_index;
        f.tri = hit.primitive_index;
        f.bary = hit.barycentrics;
        f.m = hit.object_to_world;
    }
    return f;
}

// Naechster Treffer, Loecher uebersprungen
fn find_solid(o: vec3<f32>, d: vec3<f32>, far: f32) -> Found {
    var from_t = 0.001;
    var f = find(o, d, from_t, far, RAY_FLAG_FORCE_OPAQUE);
    for (var i = 0; i < 4 && f.t >= 0.0; i = i + 1) {
        if (!is_hole(f)) {
            return f;
        }
        from_t = f.t + 0.001;
        f = find(o, d, from_t, far, RAY_FLAG_FORCE_OPAQUE);
    }
    return f;
}

fn occluded(o: vec3<f32>, d: vec3<f32>, far: f32) -> bool {
    var from_t = 0.001;
    for (var i = 0; i < 4; i = i + 1) {
        let f = find(o, d, from_t, far, RAY_FLAG_FORCE_OPAQUE | RAY_FLAG_TERMINATE_ON_FIRST_HIT);
        if (f.t < 0.0) {
            return false;
        }
        if (!is_hole(f)) {
            return true;
        }
        from_t = f.t + 0.001;
    }
    return false;
}

struct Hit {
    t: f32,        // < 0 = nichts getroffen
    p: vec3<f32>,
    n: vec3<f32>,
    albedo: vec3<f32>,
    emissive: vec3<f32>,
    rough: f32,
    metal: f32,
};

// `cone` = wie schnell Texturen mit der Entfernung unscharf werden duerfen
fn closest(o: vec3<f32>, d: vec3<f32>, far: f32, cone: f32) -> Hit {
    var h: Hit;
    h.t = -1.0;
    let f = find_solid(o, d, far);
    if (f.t < 0.0) {
        return h;
    }
    let info = infos[f.info];
    let tri = triangle(info, f.tri);
    let b = vec3<f32>(1.0 - f.bary.x - f.bary.y, f.bary.x, f.bary.y);
    let n = tri.n0.xyz * b.x + tri.n1.xyz * b.y + tri.n2.xyz * b.z;
    let m = mats[info.material];
    var base = m.base.rgb * info.tint.rgb * tri.color.rgb;
    if (m.params.w > 0.5) {
        let lod = clamp(log2(f.t * cone + 1.0), 0.0, 6.0);
        let k = u32(m.params.w - 0.5);
        base = base * textureSampleLevel(textures[k], tex_samp, uv_of(info, f), lod).rgb;
    }
    h.t = f.t;
    h.p = o + d * f.t;
    h.n = normalize(f.m * vec4<f32>(n, 0.0));
    if (dot(h.n, d) > 0.0) {
        h.n = -h.n;
    }
    h.albedo = base;
    h.emissive = m.emissive.rgb + base * info.glow;
    h.rough = clamp(m.params.y, 0.045, 1.0);
    h.metal = m.params.x;
    return h;
}

// Licht einer Lampe ohne Schatten (Diffusanteil ohne Farbe der Flaeche)
fn lamp_simple(l: Lamp, p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let d = l.pos.xyz - p;
    let d2 = dot(d, d);
    let r = l.pos.w;
    if (d2 >= r * r) {
        return vec3<f32>(0.0);
    }
    let dir = d / sqrt(max(d2, 0.000001));
    let k = d2 / (r * r);
    var att = (1.0 - k * k) * (1.0 - k * k) / (d2 + 0.05);
    if (l.color.w > 0.5) {
        att = att * smoothstep(l.dir.w, l.extra.x, dot(-dir, l.dir.xyz));
    }
    return l.color.rgb * att * max(dot(n, dir), 0.0);
}

// Direktes Licht an einem Punkt (Sonne und eine zufaellig gewaehlte Lampe), ohne Flaechenfarbe
fn direct(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let o = p + n * 0.004;
    var sum = vec3<f32>(0.0);
    if (s.sun_dir.w > 0.5) {
        let d = sun_sample();
        let ndl = dot(n, d);
        if (ndl > 0.0 && !occluded(o, d, 1000.0)) {
            sum = sum + s.sun_color.rgb * ndl;
        }
    }
    let count = u32(s.counts.x);
    if (count > 0u) {
        // aus bis zu acht zufaelligen Lampen eine nach ihrer Staerke waehlen
        let tries = min(count, 8u);
        var total = 0.0;
        var pick = 0u;
        var value = vec3<f32>(0.0);
        for (var i = 0u; i < tries; i = i + 1u) {
            var k = i;
            if (count > 8u) {
                k = min(u32(rnd() * f32(count)), count - 1u);
            }
            let c = lamp_simple(lamps[k], p, n);
            let w = dot(c, vec3<f32>(0.3, 0.6, 0.1));
            total = total + w;
            if (w > 0.0 && rnd() * total <= w) {
                pick = k;
                value = c;
            }
        }
        if (total > 0.0) {
            let w = dot(value, vec3<f32>(0.3, 0.6, 0.1));
            let d = lamps[pick].pos.xyz - p;
            let far = length(d);
            if (!occluded(o, d / far, max(far - 0.15, 0.002))) {
                sum = sum + value * (total / w) * (f32(count) / f32(tries));
            }
        }
    }
    return sum;
}

struct Pixel {
    p: vec3<f32>,
    n: vec3<f32>,
    rough: f32,
    hit: bool,
};

fn pixel(frag: vec2<f32>) -> Pixel {
    let px = vec2<i32>(frag);
    let z = textureLoad(depth_img, px, 0).r;
    let ndc = vec2<f32>(frag.x * s.screen.z * 2.0 - 1.0, 1.0 - frag.y * s.screen.w * 2.0);
    let q = s.inv_view_proj * vec4<f32>(ndc, z, 1.0);
    let g = textureLoad(normal_img, px, 0);
    var out: Pixel;
    out.p = q.xyz / q.w;
    out.n = normalize(g.xyz * 2.0 - vec3<f32>(1.0));
    out.rough = g.w;
    out.hit = z < 1.0;
    return out;
}

// Anhebung gegen Selbsttreffer, waechst mit der Entfernung
fn lift(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    return p + n * (0.004 + length(p - s.cam_pos.xyz) * 0.0015);
}

// Licht, das aus einer Richtung ankommt: Treffer mit direktem Licht, Streulicht aus dem Himmel
fn incoming(o: vec3<f32>, d: vec3<f32>, rough: f32) -> vec3<f32> {
    let h = closest(o, d, 1000.0, 1.5);
    if (h.t < 0.0) {
        return sky(d, rough) * s.sky_b.w;
    }
    let light = direct(h.p, h.n);
    // zweiter Abpraller grob: Himmel, soweit nicht verdeckt
    let bounce = sky_diffuse(h.n) * s.sky_b.w * 0.35;
    return h.emissive + h.albedo * (light + bounce);
}
