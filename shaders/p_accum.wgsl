// Entrauschen 1: ueber mehrere Bilder mitteln. tex0 = neu, tex1 = Verlauf (w = Anzahl Bilder),
// tex2 = Bewegung, tex3 = Tiefe; a = Breite, Hoehe, Verlauf gueltig, hoechste Anzahl; b.x = Schranke (0 = aus)

#include "_p_common.wgsl"

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let last = vec2<i32>(pc.a.xy) - vec2<i32>(1);
    let now = textureLoad(tex0, px, 0).rgb;
    if (pc.a.z < 0.5) {
        return vec4<f32>(now, 1.0);
    }
    // Bewegung der vordersten Flaeche ringsum: Kanten nehmen ihren Verlauf mit
    var best = px;
    var best_z = 2.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let q = clamp(px + vec2<i32>(x, y), vec2<i32>(0), last);
            let z = textureLoad(tex3, q, 0).r;
            if (z < best_z) {
                best_z = z;
                best = q;
            }
        }
    }
    let motion = textureLoad(tex2, best, 0).xy;
    let uv = in.uv - motion;
    if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
        return vec4<f32>(now, 1.0);
    }
    var old = textureSampleLevel(tex1, linear_samp, uv, 0.0);
    if (pc.b.x > 0.0) {
        // Verlauf auf das beschraenken, was die Umgebung jetzt zeigt
        var m1 = vec3<f32>(0.0);
        var m2 = vec3<f32>(0.0);
        for (var y = -2; y <= 2; y = y + 1) {
            for (var x = -2; x <= 2; x = x + 1) {
                let c = textureLoad(tex0, clamp(px + vec2<i32>(x, y), vec2<i32>(0), last), 0).rgb;
                m1 = m1 + c;
                m2 = m2 + c * c;
            }
        }
        let mean = m1 / 25.0;
        let dev = sqrt(max(m2 / 25.0 - mean * mean, vec3<f32>(0.0)));
        old = vec4<f32>(clamp(old.rgb, mean - dev * pc.b.x, mean + dev * pc.b.x), old.w);
    }
    let n = min(old.w + 1.0, pc.a.w);
    return vec4<f32>(mix(old.rgb, now, 1.0 / n), n);
}
