// Kantenglaettung ueber mehrere Bilder.
// tex0 = Bild, tex1 = Verlauf, tex2 = Bewegung, tex3 = Tiefe; a = Breite, Hoehe, Verlauf gueltig, -

#include "_p_common.wgsl"

// Zwischenwerte scharf (Catmull-Rom mit fuenf Abtastungen)
fn history(uv: vec2<f32>) -> vec3<f32> {
    let size = pc.a.xy;
    let p = uv * size;
    let c = floor(p - vec2<f32>(0.5)) + vec2<f32>(0.5);
    let f = p - c;
    let w0 = f * (vec2<f32>(-0.5) + f * (vec2<f32>(1.0) - 0.5 * f));
    let w1 = vec2<f32>(1.0) + f * f * (vec2<f32>(-2.5) + 1.5 * f);
    let w2 = f * (vec2<f32>(0.5) + f * (vec2<f32>(2.0) - 1.5 * f));
    let w3 = f * f * (vec2<f32>(-0.5) + 0.5 * f);
    let w12 = w1 + w2;
    let t0 = (c - vec2<f32>(1.0)) / size;
    let t3 = (c + vec2<f32>(2.0)) / size;
    let t12 = (c + w2 / w12) / size;
    var sum = vec4<f32>(0.0);
    sum = sum + vec4<f32>(textureSampleLevel(tex1, linear_samp, vec2<f32>(t12.x, t0.y), 0.0).rgb, 1.0) * (w12.x * w0.y);
    sum = sum + vec4<f32>(textureSampleLevel(tex1, linear_samp, vec2<f32>(t0.x, t12.y), 0.0).rgb, 1.0) * (w0.x * w12.y);
    sum = sum + vec4<f32>(textureSampleLevel(tex1, linear_samp, t12, 0.0).rgb, 1.0) * (w12.x * w12.y);
    sum = sum + vec4<f32>(textureSampleLevel(tex1, linear_samp, vec2<f32>(t3.x, t12.y), 0.0).rgb, 1.0) * (w3.x * w12.y);
    sum = sum + vec4<f32>(textureSampleLevel(tex1, linear_samp, vec2<f32>(t12.x, t3.y), 0.0).rgb, 1.0) * (w12.x * w3.y);
    return max(sum.rgb / sum.w, vec3<f32>(0.0));
}

// Helle Punkte daempfen, damit sie beim Mitteln nicht flackern
fn squash(c: vec3<f32>) -> vec3<f32> {
    return c / (1.0 + luma(c));
}
fn expand(c: vec3<f32>) -> vec3<f32> {
    return c / max(1.0 - luma(c), 0.0001);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let last = vec2<i32>(pc.a.xy) - vec2<i32>(1);
    let now = textureLoad(tex0, px, 0).rgb;
    if (pc.a.z < 0.5) {
        return vec4<f32>(now, 1.0);
    }
    // Bewegung der naechsten Flaeche in der Umgebung: Kanten bleiben heil
    var best = px;
    var best_z = 2.0;
    var m1 = vec3<f32>(0.0);
    var m2 = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let q = clamp(px + vec2<i32>(x, y), vec2<i32>(0), last);
            let z = textureLoad(tex3, q, 0).r;
            if (z < best_z) {
                best_z = z;
                best = q;
            }
            let c = squash(textureLoad(tex0, q, 0).rgb);
            m1 = m1 + c;
            m2 = m2 + c * c;
        }
    }
    let motion = textureLoad(tex2, best, 0).xy;
    let uv = in.uv - motion;
    if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
        return vec4<f32>(now, 1.0);
    }
    let mean = m1 / 9.0;
    let dev = sqrt(max(m2 / 9.0 - mean * mean, vec3<f32>(0.0)));
    let old = clamp(squash(history(uv)), mean - dev * 1.25, mean + dev * 1.25);
    let speed = length(motion * pc.a.xy);
    let blend = mix(0.07, 0.22, clamp(speed * 0.1, 0.0, 1.0));
    return vec4<f32>(expand(mix(old, squash(now), blend)), 1.0);
}
