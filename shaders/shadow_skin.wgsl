// Schattenkarte fuer animierte Figuren.

#include "_common.wgsl"

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(3) joints: vec4<f32>,
    @location(4) weights: vec4<f32>,
    @builtin(instance_index) ii: u32,
) -> @builtin(position) vec4<f32> {
    let inst = insts[ii];
    let base = u32(inst.extra.x + 0.5);
    let skin = bones[base + u32(joints.x)] * weights.x
        + bones[base + u32(joints.y)] * weights.y
        + bones[base + u32(joints.z)] * weights.z
        + bones[base + u32(joints.w)] * weights.w;
    return g.light_vp * (inst.model * (skin * vec4<f32>(pos, 1.0)));
}

@fragment
fn fs_main() {}
