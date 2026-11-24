// Verdeckung glaetten, ohne ueber Kanten zu verwischen.
// tex0 = Verdeckung, tex1 = Tiefe; a = Richtung xy in Bildpunkten, nah, fern

#include "_p_common.wgsl"

fn linear_depth(px: vec2<i32>) -> f32 {
    let z = textureLoad(tex1, px, 0).r;
    return pc.a.z * pc.a.w / (pc.a.w - z * (pc.a.w - pc.a.z));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let size = vec2<i32>(textureDimensions(tex0));
    let d = linear_depth(px);
    var sum = 0.0;
    var weight = 0.0;
    for (var i = -4; i <= 4; i = i + 1) {
        let q = clamp(px + vec2<i32>(pc.a.xy) * i, vec2<i32>(0), size - vec2<i32>(1));
        let w = exp(-f32(i * i) * 0.12) * exp(-abs(linear_depth(q) - d) * 12.0 / max(d, 0.1));
        sum = sum + textureLoad(tex0, q, 0).r * w;
        weight = weight + w;
    }
    let ao = sum / weight;
    return vec4<f32>(ao, ao, ao, 1.0);
}
