// Grundlage jedes Welt-Shaders: Boxen und Vertex-Stufe. Vorher `_common.wgsl` einbinden,
// danach folgt die eigene `fs_main(in: VsOut)` des Spiels (siehe render::WorldShaders).

#include "_world_box.wgsl"

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    return box_vertex(vi, ii);
}
