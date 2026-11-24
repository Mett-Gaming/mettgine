// Ueberstrahlen: halb so gross. tex0 = Bild, tex1 = Belichtung; a = 1/Breite, 1/Hoehe (Quelle), Schwelle, erste Stufe

#include "_p_common.wgsl"

fn at(uv: vec2<f32>, x: f32, y: f32) -> vec3<f32> {
    return textureSampleLevel(tex0, linear_samp, uv + vec2<f32>(x, y) * pc.a.xy, 0.0).rgb;
}

fn weight(c: vec3<f32>) -> f32 {
    return 1.0 / (1.0 + luma(c));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    if (pc.a.w < 0.5) {
        var c = at(uv, 0.0, 0.0) * 0.25;
        c = c + (at(uv, -1.0, -1.0) + at(uv, 1.0, -1.0) + at(uv, -1.0, 1.0) + at(uv, 1.0, 1.0)) * 0.125;
        c = c + (at(uv, -2.0, 0.0) + at(uv, 2.0, 0.0) + at(uv, 0.0, -2.0) + at(uv, 0.0, 2.0)) * 0.0625;
        return vec4<f32>(c, 1.0);
    }
    // erste Stufe: einzelne grelle Punkte daempfen und Schwelle anwenden
    var sum = vec3<f32>(0.0);
    var w = 0.0;
    for (var y = -1; y <= 1; y = y + 2) {
        for (var x = -1; x <= 1; x = x + 2) {
            let c = at(uv, f32(x), f32(y));
            let k = weight(c);
            sum = sum + c * k;
            w = w + k;
        }
    }
    var c = sum / w * textureLoad(tex1, vec2<i32>(0), 0).r;
    let l = luma(c);
    let knee = pc.a.z * 0.5 + 0.0001;
    let soft = clamp(l - pc.a.z + knee, 0.0, 2.0 * knee);
    let part = max(soft * soft / (4.0 * knee), l - pc.a.z) / max(l, 0.0001);
    return vec4<f32>(min(c * max(part, 0.0), vec3<f32>(60.0)), 1.0);
}
