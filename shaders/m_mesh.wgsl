// Moderne Grafik: einfache Modelle mit Vertexfarben (`MeshDraw`).

#include "_m_common.wgsl"
#include "_m_light.wgsl"

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};

@vertex
fn vs_main(v: VsIn, @builtin(instance_index) ii: u32) -> VsOut {
    let o = objs[order[ii]];
    let p = vec4<f32>(v.pos, 1.0);
    let n = (o.model * vec4<f32>(v.normal, 0.0)).xyz;
    let c = vec4<f32>(to_linear(v.color), 1.0);
    return finish(o, (o.model * p).xyz, (o.prev * p).xyz, n, vec4<f32>(1.0, 0.0, 0.0, 1.0), vec2<f32>(0.0), c);
}

@vertex
fn vs_shadow(v: VsIn, @builtin(instance_index) ii: u32) -> ShadowOut {
    let o = objs[order[ii]];
    return finish_shadow(o, (o.model * vec4<f32>(v.pos, 1.0)).xyz, vec2<f32>(0.0), 1.0);
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return lit(in, front);
}
