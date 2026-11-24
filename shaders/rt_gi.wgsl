// Strahlen: Streulicht und Spiegelungen je Bildpunkt (ein Strahl je Art).

#include "_rt_common.wgsl"

struct Out {
    @location(0) diffuse: vec4<f32>,
    @location(1) mirror: vec4<f32>,
};

@fragment
fn fs_main(in: VsOut) -> Out {
    var out: Out;
    out.diffuse = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    out.mirror = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let g = pixel(in.pos.xy);
    if (!g.hit) {
        return out;
    }
    start_random(in.pos.xy);
    let o = lift(g.p, g.n);
    if (s.rt.y > 0.5) {
        out.diffuse = vec4<f32>(min(incoming(o, cosine_dir(g.n), 1.0), vec3<f32>(30.0)), 1.0);
    }
    if (s.rt.z > 0.5) {
        let v = normalize(g.p - s.cam_pos.xyz);
        var d = reflect(v, ggx_normal(g.n, max(g.rough, 0.02)));
        if (dot(d, g.n) < 0.01) {
            d = reflect(v, g.n);
        }
        out.mirror = vec4<f32>(min(incoming(o, normalize(d), g.rough), vec3<f32>(30.0)), 1.0);
    }
    return out;
}
