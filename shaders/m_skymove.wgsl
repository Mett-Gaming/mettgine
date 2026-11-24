// Moderne Grafik: Bewegung des Himmels (wo im Tiefen-Durchgang nichts steht).

#include "_m_scene.wgsl"

struct SkyOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> SkyOut {
    let x = f32((vi << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vi & 2u) * 2.0 - 1.0;
    var out: SkyOut;
    out.pos = vec4<f32>(x, y, 1.0, 1.0);
    out.ndc = vec2<f32>(x, y);
    return out;
}

struct PreOut {
    @location(0) normal: vec4<f32>,
    @location(1) motion: vec2<f32>,
};

struct GuideOut {
    @location(0) normal: vec4<f32>,
    @location(1) motion: vec2<f32>,
    @location(2) albedo: vec4<f32>,
    @location(3) specular: vec4<f32>,
    @location(4) surface: vec4<f32>,
};

@fragment
fn fs_guides(in: SkyOut) -> GuideOut {
    let p = sky_pre(in);
    var out: GuideOut;
    out.normal = p.normal;
    out.motion = p.motion;
    out.albedo = vec4<f32>(0.0);
    out.specular = vec4<f32>(0.0);
    out.surface = vec4<f32>(0.0, 1.0, 0.0, 1.0);
    return out;
}

fn sky_pre(in: SkyOut) -> PreOut {
    let far = s.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let p = vec4<f32>(far.xyz / far.w, 1.0);
    let now = s.plain_view_proj * p;
    let before = s.prev_view_proj * p;
    var out: PreOut;
    out.normal = vec4<f32>(0.0);
    out.motion = (now.xy / now.w - before.xy / before.w) * vec2<f32>(0.5, -0.5);
    return out;
}

@fragment
fn fs_main(in: SkyOut) -> PreOut {
    return sky_pre(in);
}
