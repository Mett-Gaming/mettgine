// Fertiges Bild (Anzeigewerte, 1 = Papierweiss, darueber Lichter) fuer den Bildschirm umrechnen.
// params.x = Art (0 SDR, 1 HDR10, 2 scRGB), y = Papierweiss in Nits, z = Spitze in Nits

struct Push {
    params: vec4<f32>,
};
var<push_constant> pc: Push;

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;

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

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.0)) + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn pq(nits: vec3<f32>) -> vec3<f32> {
    let y = pow(clamp(nits / 10000.0, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(0.1593017578125));
    return pow((vec3<f32>(0.8359375) + 18.8515625 * y) / (vec3<f32>(1.0) + 18.6875 * y), vec3<f32>(78.84375));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = max(textureSampleLevel(src_tex, src_samp, in.uv, 0.0).rgb, vec3<f32>(0.0));
    if (pc.params.x < 0.5) {
        return vec4<f32>(min(c, vec3<f32>(1.0)), 1.0);
    }
    let nits = min(to_linear(c) * pc.params.y, vec3<f32>(pc.params.z));
    if (pc.params.x > 1.5) {
        return vec4<f32>(nits / 80.0, 1.0);
    }
    // Rec. 709 nach Rec. 2020
    let wide = vec3<f32>(
        dot(nits, vec3<f32>(0.627404, 0.329283, 0.043313)),
        dot(nits, vec3<f32>(0.069097, 0.919540, 0.011362)),
        dot(nits, vec3<f32>(0.016391, 0.088013, 0.895595)));
    return vec4<f32>(pq(wide), 1.0);
}
