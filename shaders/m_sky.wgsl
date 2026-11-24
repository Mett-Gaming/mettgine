// Moderne Grafik: Himmel (hinter allem, Tiefe 1) mit Sonnenscheibe.

#include "_m_common.wgsl"
#include "_m_light.wgsl"

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

@fragment
fn fs_main(in: SkyOut) -> @location(0) vec4<f32> {
    let far = s.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let dir = normalize(far.xyz / far.w - s.cam_pos.xyz);
    var col = sky(dir, 0.0);
    if (s.sun_dir.w > 0.5 && s.sky_a.w < 0.5) {
        let k = max(dot(dir, s.sun_dir.xyz), 0.0);
        col = col + s.sun_color.rgb * (smoothstep(0.99985, 0.9999, k) * 12.0 + pow(k, 400.0) * 0.6 + pow(k, 16.0) * 0.06);
    }
    // unendlich weit weg: Nebel nur am Horizont
    if (s.fog.w > 0.0) {
        let f = exp(-max(dir.y, 0.0) * 6.0 * max(s.misc.x, 0.02) * 10.0);
        col = mix(col, s.fog.rgb, clamp(f, 0.0, 1.0));
    }
    return vec4<f32>(col, 1.0);
}
