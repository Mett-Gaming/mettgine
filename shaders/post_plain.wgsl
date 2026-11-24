// Nachbearbeitung: skaliert das Szenenbild auf Fenstergroesse, sonst unveraendert.

struct Push {
    params: vec4<f32>, // x = Schwarzblende (0..1)
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
    let col = textureSample(src_tex, src_samp, in.uv).rgb;
    return vec4<f32>(col * (1.0 - pc.params.x), 1.0);
}
