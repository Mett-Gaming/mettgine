// Moderne Grafik: gemeinsame Teile der Objekt-Shader.

#include "_m_scene.wgsl"

@group(1) @binding(0) var base_tex: texture_2d<f32>;
@group(1) @binding(1) var metal_rough_tex: texture_2d<f32>;
@group(1) @binding(2) var normal_tex: texture_2d<f32>;
@group(1) @binding(3) var occlusion_tex: texture_2d<f32>;
@group(1) @binding(4) var emissive_tex: texture_2d<f32>;
@group(1) @binding(5) var mat_samp: sampler;

struct Push {
    view_proj: mat4x4<f32>, // Schatten: Sicht der Lampe; Beleuchtung: [0].x = 1 bei Halbtransparentem
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
    let a = textureSampleBias(base_tex, mat_samp, in.uv, s.cam_dir.w).a * m.base.a * in.alpha;
    if (a < m.params.z) {
        discard;
    }
}

struct PreOut {
    @location(0) normal: vec4<f32>,
    @location(1) motion: vec2<f32>,
};

fn surface_normal(in: VsOut, m: Mat, front: bool) -> vec3<f32> {
    let k = textureSampleBias(normal_tex, mat_samp, in.uv, s.cam_dir.w).xyz * 2.0 - vec3<f32>(1.0);
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
    let a = textureSampleBias(base_tex, mat_samp, in.uv, s.cam_dir.w).a * m.base.a * in.color.a;
    let n = surface_normal(in, m, front);
    let rough = textureSampleBias(metal_rough_tex, mat_samp, in.uv, s.cam_dir.w).g;
    if (a < m.params.z) {
        discard;
    }
    var out: PreOut;
    out.normal = vec4<f32>(n * 0.5 + vec3<f32>(0.5), clamp(m.params.y * rough, 0.0, 1.0));
    let now = in.now.xy / in.now.w;
    let before = in.before.xy / in.before.w;
    // Verschiebung im Bild (0..1, y nach unten)
    out.motion = (now - before) * vec2<f32>(0.5, -0.5);
    return out;
}
