// Nachschaerfen: hebt Unterschiede an, ohne helle Saeume. tex0 = Bild; a.z = Staerke

#include "_p_common.wgsl"

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let last = vec2<i32>(textureDimensions(tex0)) - vec2<i32>(1);
    let m = textureLoad(tex0, px, 0).rgb;
    let a = textureLoad(tex0, clamp(px + vec2<i32>(0, -1), vec2<i32>(0), last), 0).rgb;
    let b = textureLoad(tex0, clamp(px + vec2<i32>(-1, 0), vec2<i32>(0), last), 0).rgb;
    let c = textureLoad(tex0, clamp(px + vec2<i32>(1, 0), vec2<i32>(0), last), 0).rgb;
    let d = textureLoad(tex0, clamp(px + vec2<i32>(0, 1), vec2<i32>(0), last), 0).rgb;
    let lo = min(m, min(min(a, b), min(c, d)));
    let hi = max(m, max(max(a, b), max(c, d)));
    // wie viel Spielraum bis Schwarz und Weiss bleibt
    let room = sqrt(clamp(min(lo, vec3<f32>(1.0) - hi) / max(hi, vec3<f32>(0.001)), vec3<f32>(0.0), vec3<f32>(1.0)));
    let w = -room * mix(0.04, 0.2, pc.a.z);
    let col = (m + (a + b + c + d) * w) / (vec3<f32>(1.0) + 4.0 * w);
    return vec4<f32>(clamp(col, lo * 0.9, hi * 1.1 + vec3<f32>(0.005)), 1.0);
}
