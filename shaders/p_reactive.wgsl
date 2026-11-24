// Wo hat Halbtransparentes das Bild veraendert? tex0 = fertige Szene, tex1 = nur Deckendes.

#include "_p_common.wgsl"

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let a = textureLoad(tex0, px, 0).rgb;
    let b = textureLoad(tex1, px, 0).rgb;
    let d = abs(a - b) / (vec3<f32>(0.05) + max(a, b));
    let m = max(d.x, max(d.y, d.z));
    return vec4<f32>(min(m * 1.2, 0.9), 0.0, 0.0, 1.0);
}
