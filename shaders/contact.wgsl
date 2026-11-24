// Ecken verdunkeln (einfache Grafik): Verdeckung aus der Tiefe, wird auf das Bild multipliziert.
// Tiefe = binding 12, Staerke = g.extra.z

#include "_common.wgsl"

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn world(px: vec2<i32>, size: vec2<f32>) -> vec4<f32> {
    let z = textureLoad(scene_depth, px, 0);
    let uv = (vec2<f32>(px) + vec2<f32>(0.5)) / size;
    let p = g.inv_view_proj * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, z, 1.0);
    return vec4<f32>(p.xyz / p.w, z);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let dim = vec2<i32>(textureDimensions(scene_depth));
    let size = vec2<f32>(dim);
    let px = vec2<i32>(pos.xy);
    let here = world(px, size);
    if (here.w >= 1.0) {
        return vec4<f32>(1.0);
    }
    // Normale aus den naeheren Nachbarn, sonst franst sie an Kanten aus
    let last = dim - vec2<i32>(1);
    let l = world(max(px - vec2<i32>(1, 0), vec2<i32>(0)), size).xyz - here.xyz;
    let r = world(min(px + vec2<i32>(1, 0), last), size).xyz - here.xyz;
    let u = world(max(px - vec2<i32>(0, 1), vec2<i32>(0)), size).xyz - here.xyz;
    let d = world(min(px + vec2<i32>(0, 1), last), size).xyz - here.xyz;
    let dx = select(r, -l, dot(l, l) < dot(r, r) && px.x > 0);
    let dy = select(d, -u, dot(u, u) < dot(d, d) && px.y > 0);
    let c = cross(dy, dx);
    let len = length(c);
    if (len < 1e-9) {
        return vec4<f32>(1.0);
    }
    let to_eye = g.cam_pos.xyz - here.xyz;
    var n = c / len;
    n = select(-n, n, dot(n, to_eye) > 0.0);
    let dist = length(to_eye);
    let radius = 0.45;
    // Bildpunkte, die der Radius hier einnimmt: Abstand zum Nachbarpunkt als Mass
    let per_px = max(min(length(dx), length(dy)), dist * 0.0008);
    let reach = clamp(radius / per_px, 2.0, 40.0);
    let turn = fract(52.9829189 * fract(dot(pos.xy, vec2<f32>(0.06711056, 0.00583715)))) * 6.2831853;
    var sum = 0.0;
    let count = 12;
    for (var i = 0; i < count; i = i + 1) {
        let k = (f32(i) + 0.5) / f32(count);
        let a = f32(i) * 2.39996323 + turn;
        let o = vec2<i32>(vec2<f32>(cos(a), sin(a)) * k * reach);
        let q = clamp(px + o, vec2<i32>(0), last);
        let v = world(q, size).xyz - here.xyz;
        let d2 = dot(v, v);
        let h = max(dot(v, n) - 0.015 - dist * 0.004, 0.0);
        sum = sum + h / sqrt(d2 + 0.0001) * max(1.0 - d2 / (radius * radius), 0.0);
    }
    // im Nebel verschwindet die Verdunkelung wie alles andere
    let shown = g.extra.z * (1.0 - fog_rooms(here.xyz));
    return vec4<f32>(vec3<f32>(max(1.0 - sum / f32(count) * 2.6 * shown, 0.0)), 1.0);
}
