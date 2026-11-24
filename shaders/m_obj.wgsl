// Moderne Grafik: Modelle mit Texturen.

#include "_m_common.wgsl"
#include "_m_light.wgsl"

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
};

@vertex
fn vs_main(v: VsIn, @builtin(instance_index) ii: u32) -> VsOut {
    let o = objs[order[ii]];
    let p = vec4<f32>(v.pos, 1.0);
    let t = vec4<f32>((o.model * vec4<f32>(v.tangent.xyz, 0.0)).xyz, v.tangent.w);
    return finish(o, (o.model * p).xyz, (o.prev * p).xyz, (o.model * vec4<f32>(v.normal, 0.0)).xyz, t, v.uv, v.color);
}

@vertex
fn vs_shadow(v: VsIn, @builtin(instance_index) ii: u32) -> ShadowOut {
    let o = objs[order[ii]];
    return finish_shadow(o, (o.model * vec4<f32>(v.pos, 1.0)).xyz, v.uv, v.color.a);
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return lit(in, front);
}
