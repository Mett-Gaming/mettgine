// Entrauschen 2: glaetten, ohne ueber Kanten zu verwischen. tex0 = Bild (w = Anzahl Bilder),
// tex1 = Tiefe, tex2 = Normale; a = Schrittweite, nah, fern, Strenge

#include "_p_common.wgsl"

fn far_of(z: f32) -> f32 {
    return pc.a.y * pc.a.z / (pc.a.z - z * (pc.a.z - pc.a.y));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let last = vec2<i32>(textureDimensions(tex0)) - vec2<i32>(1);
    let mid = textureLoad(tex0, px, 0);
    let z = far_of(textureLoad(tex1, px, 0).r);
    let g = textureLoad(tex2, px, 0);
    let n = g.xyz * 2.0 - vec3<f32>(1.0);
    let l = luma(mid.rgb);
    var weights = array<f32, 3>(0.375, 0.25, 0.0625);
    var sum = mid.rgb * 0.140625;
    var total = 0.140625;
    // wenig Vorwissen: staerker glaetten
    // Glattes spiegelt scharf; viele gemittelte Bilder brauchen kein Glaetten mehr
    let loose = (1.0 + 8.0 / max(mid.w, 1.0)) * clamp(g.w * 1.6, 0.12, 1.0) / (1.0 + mid.w / 48.0);
    for (var y = -2; y <= 2; y = y + 1) {
        for (var x = -2; x <= 2; x = x + 1) {
            if (x == 0 && y == 0) {
                continue;
            }
            let q = clamp(px + vec2<i32>(x, y) * i32(pc.a.x), vec2<i32>(0), last);
            let c = textureLoad(tex0, q, 0).rgb;
            let qz = far_of(textureLoad(tex1, q, 0).r);
            let qn = textureLoad(tex2, q, 0).xyz * 2.0 - vec3<f32>(1.0);
            let wz = exp(-abs(qz - z) / (0.02 * z * pc.a.x + 0.02));
            let wn = pow(max(dot(n, qn), 0.0), 32.0);
            let wl = exp(-abs(luma(c) - l) / (pc.a.w * loose * (l + 0.1)));
            let w = weights[abs(x)] * weights[abs(y)] * wz * wn * wl;
            sum = sum + c * w;
            total = total + w;
        }
    }
    return vec4<f32>(sum / total, mid.w);
}
