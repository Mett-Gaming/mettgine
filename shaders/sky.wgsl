// Himmel: Verlauf vom Horizont nach oben, dazu die Sonne. Bildschirmfuellendes Dreieck.

#include "_common.wgsl"

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    let x = f32((vi << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vi & 2u) * 2.0 - 1.0;
    var out: VsOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.ndc = vec2<f32>(x, y);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let far = g.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let near = g.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let dir = normalize(far.xyz / far.w - near.xyz / near.w);
    let up = clamp(dir.y, 0.0, 1.0);
    var col = mix(g.horizon.rgb, g.sky_top.rgb, pow(up, 0.6));
    // unterhalb des Horizonts: dunstiger Boden
    col = mix(col, g.ground.rgb * 0.8 + g.horizon.rgb * 0.4, clamp(-dir.y * 6.0, 0.0, 1.0));
    let sun = normalize(g.sun_dir.xyz);
    let s = max(dot(dir, sun), 0.0);
    col = col + g.sun_color.rgb * (pow(s, 900.0) * 1.5 + pow(s, 12.0) * 0.18);
    return vec4<f32>(col, 1.0);
}
