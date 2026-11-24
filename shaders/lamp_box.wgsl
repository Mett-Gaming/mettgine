// Lampenschatten: Welt-Boxen. Leuchtendes (Material 3) wirft keinen Schatten.

#include "_common.wgsl"
#include "_world_box.wgsl"

struct Tile { view_proj: mat4x4<f32> };
var<push_constant> tile: Tile;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> @builtin(position) vec4<f32> {
    let m = boxes[ii].extra.x;
    if (m > 2.5 && m < 3.5) {
        return vec4<f32>(0.0, 0.0, 2.0, 1.0);
    }
    return tile.view_proj * vec4<f32>(box_vertex(vi, ii).world_pos, 1.0);
}
