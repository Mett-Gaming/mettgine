// Strahlen: Verdeckung und Sonnenschatten je Bildpunkt.

#include "_rt_common.wgsl"

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let g = pixel(in.pos.xy);
    if (!g.hit) {
        return vec4<f32>(1.0, 1.0, 0.0, 1.0);
    }
    start_random(in.pos.xy);
    let o = lift(g.p, g.n);
    var ao = 1.0;
    if (s.misc.y > 0.5) {
        var open = 0.0;
        for (var i = 0; i < 2; i = i + 1) {
            open = open + select(0.5, 0.0, occluded(o, cosine_dir(g.n), s.counts.z));
        }
        ao = mix(1.0, open, clamp(s.counts.w, 0.0, 1.0));
    }
    var sun = 1.0;
    if (s.rt.x > 0.5 && s.sun_dir.w > 0.5) {
        let d = sun_sample();
        if (dot(g.n, s.sun_dir.xyz) > -0.05 && occluded(o, d, 1000.0)) {
            sun = 0.0;
        }
    }
    return vec4<f32>(ao, sun, 0.0, 1.0);
}
