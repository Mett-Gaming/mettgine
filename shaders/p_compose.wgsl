// Pathtracing zusammensetzen: Licht mal Farbe der Flaeche. tex0 = Licht, tex1 = Farbe

#include "_p_common.wgsl"

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    return vec4<f32>(textureLoad(tex0, px, 0).rgb * textureLoad(tex1, px, 0).rgb, 1.0);
}
