// Nachbearbeitung: skaliert das Szenenbild auf Fenstergroesse, weiche Lichter, Vignette, Blenden.

struct Push {
    params: vec4<f32>, // x = Schwarzblende (0..1), y = Rotblitz (0..1), z = Saettigung, w = Spielraum ueber Weiss (0 = SDR)
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
    // naga dreht y wie WebGPU: NDC y = +1 ist oben, Bildzeile 0 (uv.y = 0) ebenfalls
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var col = textureSample(src_tex, src_samp, in.uv).rgb;
    // weiche Schulter gegen ausgebrannte Lichter
    if (pc.params.w == 0.0) {
        col = col / (vec3<f32>(1.0) + max(col - vec3<f32>(0.8), vec3<f32>(0.0)));
    }
    let luma = dot(col, vec3<f32>(0.299, 0.587, 0.114));
    col = mix(vec3<f32>(luma), col, pc.params.z);
    let d = in.uv - vec2<f32>(0.5);
    col = col * (1.0 - dot(d, d) * 0.45);
    col = mix(col, vec3<f32>(0.85, 0.1, 0.05), pc.params.y * 0.45);
    col = col * (1.0 - pc.params.x);
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(max(pc.params.w, 1.0))), 1.0);
}
