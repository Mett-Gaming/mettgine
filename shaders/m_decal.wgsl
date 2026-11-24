// Moderne Grafik: Aufkleber (Flecken, Spuren, Schilder), auf Flaechen in einem Wuerfel projiziert.
// `prev` des Objekts ist hier die Umkehrung seiner Lage.

#include "_m_common.wgsl"
#include "_m_light.wgsl"

struct DecalOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) object: u32,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @builtin(instance_index) ii: u32) -> DecalOut {
    let k = order[ii];
    var out: DecalOut;
    out.clip = s.view_proj * (objs[k].model * vec4<f32>(pos, 1.0));
    out.object = k;
    return out;
}

@fragment
fn fs_main(in: DecalOut) -> @location(0) vec4<f32> {
    let o = objs[in.object];
    let z = textureLoad(depth_tex, vec2<i32>(in.clip.xy), 0);
    let ndc = vec2<f32>(in.clip.x * s.screen.z * 2.0 - 1.0, 1.0 - in.clip.y * s.screen.w * 2.0);
    let q = s.inv_view_proj * vec4<f32>(ndc, z, 1.0);
    let world = q.xyz / q.w;
    let local = (o.prev * vec4<f32>(world, 1.0)).xyz;
    var n = normalize(cross(dpdy(world), dpdx(world)));
    if (dot(n, s.cam_pos.xyz - world) < 0.0) {
        n = -n;
    }
    var v: VsOut;
    v.clip = in.clip;
    v.world = world;
    v.normal = n;
    v.tangent = vec4<f32>(normalize(o.model[0].xyz), 1.0);
    v.uv = local.xz + vec2<f32>(0.5);
    v.color = o.tint;
    v.material = u32(o.extra.y + 0.5);
    v.glow = o.extra.w;
    let c = lit(v, true);
    let a = abs(local);
    // steil getroffene Seiten ausblenden, damit nichts langgezogen wird
    let up = normalize(o.model[1].xyz);
    let fade = smoothstep(0.1, 0.4, dot(n, up));
    if (max(a.x, max(a.y, a.z)) > 0.5 || fade <= 0.0) {
        discard;
    }
    return vec4<f32>(c.rgb, c.a * fade);
}
