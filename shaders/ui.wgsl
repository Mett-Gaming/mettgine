// UI: Rechtecke und Bitmap-Text im 480x270-Raster (laeuft vor dem VHS-Pass,
// damit auch Menues und HUD "auf Band" liegen).

struct Push {
    screen: vec4<f32>, // xy = Rastergroesse
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
    out.clip = vec4<f32>(v.pos.x / pc.screen.x * 2.0 - 1.0, 1.0 - v.pos.y / pc.screen.y * 2.0, 0.0, 1.0);
    out.uv = v.uv;
    out.color = v.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // uv.x < 0: einfarbiges Rechteck
    let glyph = textureSampleLevel(font_tex, font_samp, max(in.uv, vec2<f32>(0.0)), 0.0).r;
    let a = select(glyph, 1.0, in.uv.x < 0.0);
    return vec4<f32>(in.color.rgb, in.color.a * a);
}
