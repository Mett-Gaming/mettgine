// Szenen-Renderer (mettgine::scene): gemeinsame Daten und Beleuchtung - Sonne mit Schattenkarte,
// Distanznebel in Horizontfarbe.

struct Globals {
    view_proj: mat4x4<f32>,
    light_vp: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,   // xyz, w = Zeit
    sun_dir: vec4<f32>,   // Richtung ZUR Sonne, w = Schattenstaerke
    sun_color: vec4<f32>, // rgb, w = Schattenkarten-Groesse (Texel)
    sky_top: vec4<f32>,
    horizon: vec4<f32>,   // rgb = Nebel/Horizont, w = Nebeldichte
    ground: vec4<f32>,    // Umgebungslicht von unten
};

struct Inst {
    model: mat4x4<f32>,
    tint: vec4<f32>,  // rgb = Farbmultiplikator, a = Eigenleuchten
    extra: vec4<f32>, // x = erster Knochen (Figuren), y = Blinken (0..1)
};

@group(0) @binding(0) var<storage, read> insts: array<Inst>;
@group(0) @binding(1) var<storage, read> bones: array<mat4x4<f32>>;
@group(0) @binding(2) var<uniform> g: Globals;
@group(0) @binding(3) var shadow_map: texture_depth_2d;
@group(0) @binding(4) var shadow_samp: sampler_comparison;

fn fog_amount(p: vec3<f32>) -> f32 {
    let d = length(p - g.cam_pos.xyz);
    return clamp(1.0 - exp(-max(d - 25.0, 0.0) * g.horizon.w), 0.0, 1.0);
}

fn shadow_at(p: vec3<f32>, n: vec3<f32>) -> f32 {
    let lp = g.light_vp * vec4<f32>(p + n * 0.06, 1.0);
    let ndc = lp.xyz / lp.w;
    // NDC y = +1 (oben) liegt in Bildzeile 0 der Schattenkarte
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0) {
        return 1.0;
    }
    let texel = 1.0 / g.sun_color.w;
    var sum = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let o = vec2<f32>(f32(x), f32(y)) * texel;
            sum = sum + textureSampleCompareLevel(shadow_map, shadow_samp, uv + o, ndc.z - 0.0015);
        }
    }
    return sum / 9.0;
}

fn shade(base: vec3<f32>, p: vec3<f32>, n_in: vec3<f32>) -> vec3<f32> {
    let n = normalize(n_in);
    let sun = normalize(g.sun_dir.xyz);
    let ndl = max(dot(n, sun), 0.0);
    let sh = mix(1.0, shadow_at(p, n), g.sun_dir.w);
    let hemi = mix(g.ground.rgb, g.sky_top.rgb, n.y * 0.5 + 0.5);
    let col = base * (hemi * 0.6 + g.sun_color.rgb * ndl * sh);
    return mix(col, g.horizon.rgb, fog_amount(p));
}
