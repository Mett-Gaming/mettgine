// Pathtracing: das ganze Bild aus Strahlen. Ziel 0 = Licht (ohne Farbe der ersten Flaeche),
// Ziel 1 = deren Farbe. a = Abpraller, Strahlen je Bildpunkt

#include "_rt_common.wgsl"

struct Out {
    @location(0) light: vec4<f32>,
    @location(1) albedo: vec4<f32>,
};

struct Path {
    light: vec3<f32>,
    albedo: vec3<f32>,
};

fn follow(start: vec3<f32>, dir: vec3<f32>) -> Path {
    var out: Path;
    out.light = vec3<f32>(0.0);
    out.albedo = vec3<f32>(1.0);
    var through = vec3<f32>(1.0);
    var o = start;
    var d = dir;
    let bounces = i32(pc.a.x);
    for (var i = 0; i <= bounces; i = i + 1) {
        let h = closest(o, d, 2000.0, select(1.5, 0.12, i == 0));
        if (h.t < 0.0) {
            out.light = out.light + through * sky(d, 0.0) * select(s.sky_b.w, 1.0, i == 0);
            break;
        }
        if (i == 0) {
            // Farbe der ersten Flaeche getrennt: Entrauschen verwischt sie dann nicht
            out.albedo = max(h.albedo, vec3<f32>(0.02));
            out.light = out.light + h.emissive / out.albedo;
            if (h.emissive.x + h.emissive.y + h.emissive.z > 0.5) {
                break;
            }
        } else {
            out.light = out.light + through * h.emissive;
        }
        let tone = select(h.albedo, vec3<f32>(1.0), i == 0);
        let f0 = mix(0.04, 1.0, h.metal);
        let ndv = max(dot(h.n, -d), 0.0);
        let fres = f0 + (1.0 - f0) * pow(1.0 - ndv, 5.0);
        let mirror = clamp(fres, 0.05, 1.0);
        o = h.p + h.n * 0.004;
        if (rnd() < mirror) {
            let m = ggx_normal(h.n, h.rough);
            d = reflect(d, m);
            if (dot(d, h.n) <= 0.0) {
                break;
            }
            // Kunststoff spiegelt weiss, Metall in seiner Farbe
            let spec = mix(vec3<f32>(1.0) / select(vec3<f32>(1.0), out.albedo, i == 0), tone, h.metal);
            through = through * spec * fres / mirror;
        } else {
            through = through * tone * (1.0 - h.metal) * (1.0 - fres) / (1.0 - mirror);
            out.light = out.light + through * direct(h.p, h.n);
            d = cosine_dir(h.n);
        }
        // dunkle Wege frueher beenden
        let p = clamp(max(through.x, max(through.y, through.z)), 0.05, 1.0);
        if (i > 1) {
            if (rnd() > p) {
                break;
            }
            through = through / p;
        }
        d = normalize(d);
    }
    return out;
}

@fragment
fn fs_main(in: VsOut) -> Out {
    start_random(in.pos.xy);
    var light = vec3<f32>(0.0);
    var albedo = vec3<f32>(0.0);
    let count = max(i32(pc.a.y), 1);
    for (var i = 0; i < count; i = i + 1) {
        let at = in.pos.xy + vec2<f32>(rnd() - 0.5, rnd() - 0.5) * pc.a.z;
        let ndc = vec2<f32>(at.x * s.screen.z * 2.0 - 1.0, 1.0 - at.y * s.screen.w * 2.0);
        let far = s.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
        let path = follow(s.cam_pos.xyz, normalize(far.xyz / far.w - s.cam_pos.xyz));
        light = light + min(path.light, vec3<f32>(40.0));
        albedo = albedo + path.albedo;
    }
    var out: Out;
    out.light = vec4<f32>(light / f32(count), 1.0);
    out.albedo = vec4<f32>(albedo / f32(count), 1.0);
    return out;
}
