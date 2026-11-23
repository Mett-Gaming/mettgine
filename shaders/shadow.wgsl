// Schattenkarte: Modelle nur in die Tiefe aus Sicht der Sonne.

#include "_common.wgsl"

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @builtin(instance_index) ii: u32) -> @builtin(position) vec4<f32> {
    return g.light_vp * (insts[ii].model * vec4<f32>(pos, 1.0));
}

@fragment
fn fs_main() {}
