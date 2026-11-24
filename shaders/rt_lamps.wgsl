// Lampenlicht aus Strahlen, Teil 1: Kandidaten und das letzte Bild. `picks` = Auswahl des letzten Bildes.
// a.x = Verlauf gueltig

#include "_rt_lamps.wgsl"

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<u32> {
    let g = pixel(in.pos.xy);
    var out: Pick;
    out.lamp = NO_LAMP;
    out.weight = 0.0;
    out.seen = 0.0;
    out.n = vec3<f32>(0.0, 1.0, 0.0);
    out.dist = 0.0;
    let count = u32(s.counts.x);
    if (!g.hit || count == 0u) {
        return pack(out);
    }
    start_random(in.pos.xy);
    out.n = g.n;
    out.dist = length(g.p - s.cam_pos.xyz);
    var r: Reservoir;
    r.lamp = NO_LAMP;
    r.sum = 0.0;
    r.seen = 0.0;
    let tries = min(count, 16u);
    for (var i = 0u; i < tries; i = i + 1u) {
        var k = i;
        if (count > 16u) {
            k = min(u32(rnd() * f32(count)), count - 1u);
        }
        // jeder Kandidat steht fuer count / tries Lampen
        r = offer(r, k, worth(k, g.p, g.n) * f32(count), 1.0);
    }
    out.weight = final_weight(r, g.p, g.n);
    out.lamp = r.lamp;
    if (r.lamp != NO_LAMP && !lamp_free(r.lamp, lift(g.p, g.n))) {
        out.weight = 0.0;
    }
    out.seen = r.seen;
    if (pc.a.x > 0.5) {
        let uv = in.uv - textureLoad(motion_img, vec2<i32>(in.pos.xy), 0).xy;
        let size = vec2<f32>(textureDimensions(picks));
        let q = vec2<i32>(floor(uv * size));
        if (all(q >= vec2<i32>(0)) && all(q < vec2<i32>(size))) {
            let old = unpack(textureLoad(picks, q, 0));
            if (similar(old, g.n, out.dist)) {
                var t: Reservoir;
                t.lamp = NO_LAMP;
                t.sum = 0.0;
                t.seen = 0.0;
                t = join_pick(t, out, 64.0, g.p, g.n);
                t = join_pick(t, old, 20.0 * r.seen, g.p, g.n);
                out.weight = final_weight(t, g.p, g.n);
                out.lamp = t.lamp;
                out.seen = t.seen;
            }
        }
    }
    return pack(out);
}
