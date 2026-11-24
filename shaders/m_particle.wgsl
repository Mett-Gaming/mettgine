// Moderne Grafik: Partikel (linear, ohne Obergrenze), vor Flaechen weich auslaufend.

#include "_m_common.wgsl"

struct VsIn {
    @location(0) pos_size: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) extra: vec4<f32>,
};

struct PartOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) style: vec3<f32>, // x = Aufhellen, y = Randweichheit, z = Groesse
    @location(3) w: f32,
};

@vertex
fn vs_main(v: VsIn, @builtin(vertex_index) vi: u32) -> PartOut {
    var xs = array<f32, 6>(-1.0, 1.0, 1.0, -1.0, 1.0, -1.0);
    var ys = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let uv = vec2<f32>(xs[vi], ys[vi]);
    let m = s.plain_view_proj;
    let right = normalize(vec3<f32>(m[0][0], m[1][0], m[2][0]));
    let up = normalize(vec3<f32>(m[0][1], m[1][1], m[2][1]));
    let c = cos(v.extra.x);
    let k = sin(v.extra.x);
    let o = vec2<f32>(uv.x * c - uv.y * k, uv.x * k + uv.y * c) * v.pos_size.w;
    let wp = v.pos_size.xyz + right * o.x + up * o.y;
    var out: PartOut;
    out.clip = s.view_proj * vec4<f32>(wp, 1.0);
    out.uv = uv;
    out.color = vec4<f32>(to_linear(v.color.rgb), v.color.a);
    out.style = vec3<f32>(v.extra.y, max(v.extra.z, 0.02), v.pos_size.w);
    out.w = out.clip.w;
    return out;
}

// Entfernung (Meter) aus einem Tiefenwert
fn distance_of(frag: vec2<f32>, z: f32) -> f32 {
    let ndc = vec2<f32>(frag.x * s.screen.z * 2.0 - 1.0, 1.0 - frag.y * s.screen.w * 2.0);
    let p = s.inv_view_proj * vec4<f32>(ndc, z, 1.0);
    return dot(p.xyz / p.w - s.cam_pos.xyz, s.cam_dir.xyz);
}

@fragment
fn fs_main(in: PartOut) -> @location(0) vec4<f32> {
    let edge = 1.0 - smoothstep(1.0 - in.style.y, 1.0, length(in.uv));
    let behind = distance_of(in.clip.xy, textureLoad(depth_tex, vec2<i32>(in.clip.xy), 0));
    let soft = clamp((behind - in.w) / max(in.style.z * 0.5, 0.02), 0.0, 1.0);
    let a = clamp(in.color.a, 0.0, 1.0) * edge * soft;
    return vec4<f32>(in.color.rgb * a, a * (1.0 - in.style.x));
}
