// Verdeckung in Ecken aus Tiefe und Normale. Ergebnis: 1 = frei, 0 = verdeckt.
// tex0 = Tiefe, tex1 = Normale; a = Breite, Hoehe, Reichweite, Staerke; b = Kamera xyz, Bild-Nr.;
// inv = Bild -> Welt; c = Blickrichtung xyz, Bildpunkte je Meter in 1 m Abstand

#include "_p_common.wgsl"

fn world(px: vec2<i32>) -> vec4<f32> {
    let z = textureLoad(tex0, px, 0).r;
    let uv = (vec2<f32>(px) + vec2<f32>(0.5)) / pc.a.xy;
    let p = pc.inv * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, z, 1.0);
    return vec4<f32>(p.xyz / p.w, z);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let here = world(px);
    if (here.w >= 1.0) {
        return vec4<f32>(1.0);
    }
    let n = normalize(textureLoad(tex1, px, 0).xyz * 2.0 - vec3<f32>(1.0));
    let depth = dot(here.xyz - pc.b.xyz, pc.c.xyz);
    let radius = pc.a.z;
    let reach = clamp(pc.c.w * radius / max(depth, 0.05), 3.0, 120.0);
    let turn = fract(52.9829189 * fract(dot(in.pos.xy + vec2<f32>(pc.b.w * 5.588238), vec2<f32>(0.06711056, 0.00583715)))) * 6.2831853;
    var sum = 0.0;
    let count = 14;
    for (var i = 0; i < count; i = i + 1) {
        let r = (f32(i) + 0.5) / f32(count);
        let a = f32(i) * 2.39996323 + turn;
        let o = vec2<i32>(vec2<f32>(cos(a), sin(a)) * r * reach);
        let q = clamp(px + o, vec2<i32>(0), vec2<i32>(pc.a.xy) - vec2<i32>(1));
        let there = world(q);
        let v = there.xyz - here.xyz;
        let d2 = dot(v, v);
        let h = max(dot(v, n) - 0.012 * depth * 0.1 - 0.01, 0.0);
        let fall = max(1.0 - d2 / (radius * radius), 0.0);
        sum = sum + h / sqrt(d2 + 0.0001) * fall;
    }
    let ao = max(1.0 - sum / f32(count) * 2.4 * pc.a.w, 0.0);
    return vec4<f32>(ao, 1.0, 0.0, 1.0);
}
