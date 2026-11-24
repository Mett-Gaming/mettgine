// Helle Werte auf den Bildschirm bringen. tex0 = Bild, tex1 = Ueberstrahlen, tex2 = Belichtung;
// a = Art (0 hart, 1 ACES, 2 AgX), Anteil Ueberstrahlen

#include "_p_common.wgsl"

fn aces(x: vec3<f32>) -> vec3<f32> {
    let c = x * 0.6;
    return clamp((c * (2.51 * c + vec3<f32>(0.03))) / (c * (2.43 * c + vec3<f32>(0.59)) + vec3<f32>(0.14)), vec3<f32>(0.0), vec3<f32>(1.0));
}

// AgX (Naeherung nach Benjamin Wrensch)
fn agx(c: vec3<f32>) -> vec3<f32> {
    let into = mat3x3<f32>(
        vec3<f32>(0.842479062253094, 0.0423282422610123, 0.0423756549057051),
        vec3<f32>(0.0784335999999992, 0.878468636469772, 0.0784336),
        vec3<f32>(0.0792237451477643, 0.0791661274605434, 0.879142973793104));
    let outof = mat3x3<f32>(
        vec3<f32>(1.19687900512017, -0.0528968517574562, -0.0529716355144438),
        vec3<f32>(-0.0980208811401368, 1.15190312990417, -0.0980434501171241),
        vec3<f32>(-0.0990297440797205, -0.0989611768448433, 1.15107367264116));
    var v = into * c;
    v = clamp(log2(max(v, vec3<f32>(0.0000000001))), vec3<f32>(-12.47393), vec3<f32>(4.026069));
    v = (v + vec3<f32>(12.47393)) / 16.5;
    let v2 = v * v;
    let v4 = v2 * v2;
    v = 15.5 * v4 * v2 - 40.14 * v4 * v + 31.96 * v4 - 6.868 * v2 * v + 0.4298 * v2 + 0.1191 * v - vec3<f32>(0.00232);
    // etwas mehr Kontrast und Farbe als das neutrale Grundbild
    let l = luma(v);
    v = pow(max(v, vec3<f32>(0.0)), vec3<f32>(1.12));
    v = vec3<f32>(l) + (v - vec3<f32>(l)) * 1.2;
    v = outof * v;
    return pow(clamp(v, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2));
}

fn to_display(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var c = textureSampleLevel(tex0, linear_samp, in.uv, 0.0).rgb * textureLoad(tex2, vec2<i32>(0), 0).r;
    if (pc.a.y > 0.0) {
        c = mix(c, c + textureSampleLevel(tex1, linear_samp, in.uv, 0.0).rgb, pc.a.y);
    }
    if (pc.a.x > 1.5) {
        c = agx(c);
    } else if (pc.a.x > 0.5) {
        c = aces(c);
    }
    c = to_display(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)));
    // feines Rauschen gegen Farbstufen
    let n = fract(52.9829189 * fract(dot(in.pos.xy, vec2<f32>(0.06711056, 0.00583715)))) - 0.5;
    return vec4<f32>(c + vec3<f32>(n / 255.0), 1.0);
}
