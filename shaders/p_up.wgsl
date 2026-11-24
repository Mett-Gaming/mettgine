// Ueberstrahlen: doppelt so gross, weich. tex0 = kleinere Stufe; a = 1/Breite, 1/Hoehe (Quelle), Anteil

#include "_p_common.wgsl"

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var c = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let w = (2.0 - abs(f32(x))) * (2.0 - abs(f32(y))) / 16.0;
            c = c + textureSampleLevel(tex0, linear_samp, in.uv + vec2<f32>(f32(x), f32(y)) * pc.a.xy, 0.0).rgb * w;
        }
    }
    return vec4<f32>(c * pc.a.z, 1.0);
}
