// Lampenlicht aus Strahlen, Teil 2: Nachbarn dazunehmen, einen Strahl verfolgen, beleuchten.
// Ziel 0 = diffuses Licht ohne Farbe der Flaeche, Ziel 1 = Glanz ohne Spiegelfarbe.

#include "_rt_lamps.wgsl"

struct Out {
    @location(0) diffuse: vec4<f32>,
    @location(1) gloss: vec4<f32>,
};

@fragment
fn fs_main(in: VsOut) -> Out {
    var out: Out;
    out.diffuse = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    out.gloss = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let g = pixel(in.pos.xy);
    if (!g.hit) {
        return out;
    }
    start_random(in.pos.yx + vec2<f32>(17.0, 59.0));
    let px = vec2<i32>(in.pos.xy);
    let size = vec2<i32>(textureDimensions(picks));
    let mine = unpack(textureLoad(picks, px, 0));
    var r: Reservoir;
    r.lamp = NO_LAMP;
    r.sum = 0.0;
    r.seen = 0.0;
    r = join_pick(r, mine, 1000.0, g.p, g.n);
    for (var i = 0; i < 4; i = i + 1) {
        let a = rnd() * 2.0 * PI;
        let d = sqrt(rnd()) * 16.0;
        let q = px + vec2<i32>(i32(cos(a) * d), i32(sin(a) * d));
        if (any(q < vec2<i32>(0)) || any(q >= size)) {
            continue;
        }
        let other = unpack(textureLoad(picks, q, 0));
        if (similar(other, g.n, mine.dist)) {
            r = join_pick(r, other, 1000.0, g.p, g.n);
        }
    }
    let weight = min(final_weight(r, g.p, g.n), 4000.0);
    if (weight <= 0.0) {
        return out;
    }
    let l = lamps[r.lamp];
    let o = lift(g.p, g.n);
    if (!lamp_free(r.lamp, o)) {
        return out;
    }
    let light = lamp_simple(l, g.p, g.n) * weight;
    out.diffuse = vec4<f32>(min(light, vec3<f32>(60.0)), 1.0);
    // Glanz wie in `brdf`, ohne Fresnel
    let v = normalize(s.cam_pos.xyz - g.p);
    let dir = normalize(l.pos.xyz - g.p);
    let h = normalize(v + dir);
    let ndl = max(dot(g.n, dir), 0.0001);
    let ndv = max(dot(g.n, v), 0.0001);
    let ndh = max(dot(g.n, h), 0.0);
    let rough = clamp(g.rough, 0.045, 1.0);
    let a2 = rough * rough * rough * rough;
    let k = ndh * ndh * (a2 - 1.0) + 1.0;
    let dist = a2 / (PI * k * k);
    let vis = 0.5 / max(mix(2.0 * ndl * ndv, ndl + ndv, rough * rough), 0.0001);
    out.gloss = vec4<f32>(min(light * dist * vis * PI, vec3<f32>(60.0)), 1.0);
    return out;
}
