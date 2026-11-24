// Lampenschatten: animierte Figuren.

#include "_common.wgsl"

struct Tile { view_proj: mat4x4<f32> };
var<push_constant> tile: Tile;

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
    return tile.view_proj * (inst.model * (skin * vec4<f32>(pos, 1.0)));
}
