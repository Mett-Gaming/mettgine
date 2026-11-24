// Lampenschatten: Modelle (fest und beweglich) nur in die Tiefe, aus Sicht einer Lampe.

#include "_common.wgsl"

struct Tile { view_proj: mat4x4<f32> };
var<push_constant> tile: Tile;

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @builtin(instance_index) ii: u32) -> @builtin(position) vec4<f32> {
    let inst = insts[ii];
    var wp = inst.model * vec4<f32>(pos, 1.0);
    // wie mesh.wgsl: feste Modelle schwanken mit
    if (inst.extra.w > 0.5) {
        let s = sway(wp.xyz);
        wp = wp + vec4<f32>(s.x, s.y * (1.0 - smoothstep(2.6, 3.4, wp.y)), 0.0, 0.0);
    }
    return tile.view_proj * wp;
}
