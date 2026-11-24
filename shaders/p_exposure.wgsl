// Belichtung messen (1x1). tex0 = Bild, tex1 = voriger Wert; a = Anpassung 0..1, min, max, fester Wert (> 0 = fest)

#include "_p_common.wgsl"

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (pc.a.w > 0.0) {
        return vec4<f32>(pc.a.w, 0.0, 0.0, 1.0);
    }
    var sum = 0.0;
    for (var y = 0; y < 16; y = y + 1) {
        for (var x = 0; x < 16; x = x + 1) {
            let uv = (vec2<f32>(f32(x), f32(y)) + vec2<f32>(0.5)) / 16.0;
            sum = sum + log2(max(luma(textureSampleLevel(tex0, linear_samp, uv, 0.0).rgb), 0.001));
        }
    }
    let want = clamp(0.16 / exp2(sum / 256.0), pc.a.y, pc.a.z);
    let old = textureLoad(tex1, vec2<i32>(0), 0).r;
    return vec4<f32>(mix(old, want, pc.a.x), 0.0, 0.0, 1.0);
}
