// UI: Rechtecke und Text (Schrift-Atlas) in virtuellen Einheiten (Hoehe 720, y nach unten).

struct Push {
    screen: vec4<f32>, // xy = virtuelle Groesse
};
var<push_constant> pc: Push;

@group(0) @binding(0) var font_tex: texture_2d<f32>;
@group(0) @binding(1) var font_samp: sampler;

struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var out: VsOut;
    // naga dreht y wie WebGPU: NDC y = +1 ist oben, UI-y waechst nach unten
    out.clip = vec4<f32>(v.pos.x / pc.screen.x * 2.0 - 1.0, 1.0 - v.pos.y / pc.screen.y * 2.0, 0.0, 1.0);
    out.uv = v.uv;
    out.color = v.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // ohne Verkleinerungsstufen begrenzt der Sampler auf Stufe 0
    // uv.x < 0: einfarbiges Rechteck
    let glyph = textureSample(font_tex, font_samp, max(in.uv, vec2<f32>(0.0))).r;
    let a = select(glyph, 1.0, in.uv.x < 0.0);
    return vec4<f32>(in.color.rgb, in.color.a * a);
}
