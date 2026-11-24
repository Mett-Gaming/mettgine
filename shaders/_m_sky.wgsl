// Moderne Grafik: Himmel und Glanz-Rechnung.

const PI: f32 = 3.14159265;

// Drehung der Abtastung je Bildpunkt, damit weiche Schatten rauschen statt Stufen zu zeigen
fn noise(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

fn equirect(dir: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(atan2(dir.x, -dir.z) / (2.0 * PI) + 0.5, acos(clamp(dir.y, -1.0, 1.0)) / PI);
}

// Himmel in Richtung `dir`; `rough` 0 = scharf, 1 = ganz weich
fn sky(dir: vec3<f32>, rough: f32) -> vec3<f32> {
    if (s.sky_a.w > 0.5) {
        let lod = rough * max(s.sky_c.w - 2.0, 0.0);
        return textureSampleLevel(sky_tex, linear_samp, equirect(dir), lod).rgb * s.sky_a.rgb;
    }
    let up = clamp(dir.y, 0.0, 1.0);
    let soft = mix(0.02, 0.6, rough);
    let above = mix(s.sky_b.rgb, s.sky_a.rgb, pow(up, 0.5));
    return mix(s.sky_c.rgb, above, smoothstep(-soft, soft, dir.y));
}

fn sky_diffuse(n: vec3<f32>) -> vec3<f32> {
    if (s.sky_a.w > 0.5) {
        return sky(n, 1.0);
    }
    let above = (s.sky_a.rgb + s.sky_b.rgb) * 0.5;
    return mix(s.sky_c.rgb, above, n.y * 0.5 + 0.5);
}

struct Surface {
    albedo: vec3<f32>,
    f0: vec3<f32>,
    rough: f32,
    n: vec3<f32>,
    v: vec3<f32>,
};

// Licht aus Richtung `l` (diffus und Glanz), ohne Farbe der Lampe
fn brdf(f: Surface, l: vec3<f32>) -> vec3<f32> {
    let ndl = dot(f.n, l);
    if (ndl <= 0.0) {
        return vec3<f32>(0.0);
    }
    let h = normalize(l + f.v);
    let ndv = max(dot(f.n, f.v), 0.0001);
    let ndh = max(dot(f.n, h), 0.0);
    let vdh = max(dot(f.v, h), 0.0);
    let a = f.rough * f.rough;
    let a2 = a * a;
    let d = ndh * ndh * (a2 - 1.0) + 1.0;
    let dist = a2 / (PI * d * d);
    let vis = 0.5 / max(mix(2.0 * ndl * ndv, ndl + ndv, a), 0.0001);
    let fres = f.f0 + (vec3<f32>(1.0) - f.f0) * pow(1.0 - vdh, 5.0);
    return ((vec3<f32>(1.0) - fres) * f.albedo / PI + fres * dist * vis) * ndl * PI;
}

// Anteil des Umgebungsglanzes (Naeherung nach Karis)
fn env_brdf(f0: vec3<f32>, rough: f32, ndv: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = rough * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * ndv)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + vec3<f32>(ab.y);
}

