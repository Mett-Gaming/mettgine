// Schnelle Kantenglaettung auf dem fertigen Bild. tex0 = Bild; a = 1/Breite, 1/Hoehe

#include "_p_common.wgsl"

fn at(uv: vec2<f32>, x: f32, y: f32) -> vec3<f32> {
    return textureSampleLevel(tex0, linear_samp, uv + vec2<f32>(x, y) * pc.a.xy, 0.0).rgb;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let m = at(uv, 0.0, 0.0);
    let nw = luma(at(uv, -1.0, -1.0));
    let ne = luma(at(uv, 1.0, -1.0));
    let sw = luma(at(uv, -1.0, 1.0));
    let se = luma(at(uv, 1.0, 1.0));
    let mid = luma(m);
    let lo = min(mid, min(min(nw, ne), min(sw, se)));
    let hi = max(mid, max(max(nw, ne), max(sw, se)));
    if (hi - lo < max(0.04, hi * 0.11)) {
        return vec4<f32>(m, 1.0);
    }
    var dir = vec2<f32>(-((nw + ne) - (sw + se)), (nw + sw) - (ne + se));
    let reduce = max((nw + ne + sw + se) * 0.03125, 0.0078125);
    let k = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    dir = clamp(dir * k, vec2<f32>(-8.0), vec2<f32>(8.0));
    let a = 0.5 * (at(uv, dir.x * -0.16667, dir.y * -0.16667) + at(uv, dir.x * 0.16667, dir.y * 0.16667));
    let b = a * 0.5 + 0.25 * (at(uv, dir.x * -0.5, dir.y * -0.5) + at(uv, dir.x * 0.5, dir.y * 0.5));
    let l = luma(b);
    if (l < lo || l > hi) {
        return vec4<f32>(a, 1.0);
    }
    return vec4<f32>(b, 1.0);
}
