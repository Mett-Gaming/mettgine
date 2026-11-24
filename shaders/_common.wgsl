// Gemeinsame Daten und Beleuchtung aller Shader: Sonne mit Schattenkarte oder Raeume mit Lampen,
// jeweils mit Nebel. Set 0, Aufbau wie `Globals` und `GpuInst` in src/render/mod.rs.

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
    params: vec4<f32>,    // x = Lampenzahl, y = dunkle Raeume, z = 1 Raeume / 0 Sonne, w = zweite Kamera
    sway: vec4<f32>,      // Schwanken der Boxen: x = seitlich, y = hoch, z = Gangende z, w = Gangende x
    // x = Verfall der Welt-Materialien (0..1), y = Zahl der Lampen mit Schatten,
    // z = Verdunkelung in Ecken (0..1), w = Licht an der Kamera in dunklen Raeumen (1 = ueblich)
    extra: vec4<f32>,
};

struct Light {
    pos: vec4<f32>,   // xyz, w = Radius
    color: vec4<f32>, // rgb, w = Intensitaet
    extra: vec4<f32>, // x = Seed, y = Flackern
};

struct Box {
    center: vec4<f32>,
    half_ext: vec4<f32>,
    color: vec4<f32>,
    // x = Material (0 normal, 1 Boden, 2 Decke, 3 leuchtend), y = Seed,
    // z = Leuchtstaerke, w = Flackern
    extra: vec4<f32>,
};

struct Inst {
    model: mat4x4<f32>,
    tint: vec4<f32>,  // rgb = Farbmultiplikator, a = Eigenleuchten
    extra: vec4<f32>, // x = erster Knochen (Figuren), y = Blinken, z = Zerreissen, w = 1 schwankt mit
};

@group(0) @binding(0) var<storage, read> insts: array<Inst>;
@group(0) @binding(1) var<storage, read> bones: array<mat4x4<f32>>;
@group(0) @binding(2) var<uniform> g: Globals;
@group(0) @binding(3) var shadow_map: texture_depth_2d;
@group(0) @binding(4) var shadow_samp: sampler_comparison;
@group(0) @binding(5) var<storage, read> lights: array<Light>;
@group(0) @binding(6) var<storage, read> boxes: array<Box>;
// Schild-Atlas: je Zeile (32 px hoch, 512 px breit) ein Schriftzug
@group(0) @binding(7) var sign_tex: texture_2d<f32>;
@group(0) @binding(8) var linear_samp: sampler;
// Bild der zweiten Kamera
@group(0) @binding(9) var camera_tex: texture_2d<f32>;

// Schattenfeld einer Lampe, wie `GpuLampTile` in src/render/lamps.rs
struct LampTile {
    view_proj: mat4x4<f32>,
    rect: vec4<f32>, // Feld im Atlas: x, y, Breite, Hoehe
    info: vec4<f32>, // x = Nummer der Lampe, y = Staerke, z = Texelgroesse im Feld
};
@group(0) @binding(10) var lamp_atlas: texture_depth_2d;
@group(0) @binding(11) var<storage, read> lamp_tiles: array<LampTile>;
// Tiefe der Szene (nur fuer contact.wgsl)
@group(0) @binding(12) var scene_depth: texture_depth_2d;

// Beleuchtung durch Lampen statt Sonne?
fn is_rooms() -> bool {
    return g.params.z > 0.5;
}

fn fog_sun(p: vec3<f32>) -> f32 {
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

fn shade_sun(base: vec3<f32>, p: vec3<f32>, n_in: vec3<f32>) -> vec3<f32> {
    let n = normalize(n_in);
    let sun = normalize(g.sun_dir.xyz);
    let ndl = max(dot(n, sun), 0.0);
    let sh = mix(1.0, shadow_at(p, n), g.sun_dir.w);
    let hemi = mix(g.ground.rgb, g.sky_top.rgb, n.y * 0.5 + 0.5);
    let col = base * (hemi * 0.6 + g.sun_color.rgb * ndl * sh);
    return mix(col, g.horizon.rgb, fog_sun(p));
}

fn hash11(p: f32) -> f32 {
    return fract(sin(p * 127.1 + 311.7) * 43758.5453);
}

// Neonroehren-Flackern. Jede Roehre wechselt alle paar Sekunden zufaellig ihr Muster:
// ruhig, Stroboskop, Pulsieren, fast aus mit Aufblitzen, Unterspannung, Brummen.
fn flicker(seed: f32, amount: f32, time: f32) -> f32 {
    if (amount <= 0.0) {
        return 1.0;
    }
    let epoch = floor(time / 7.0 + seed * 0.37);
    if (hash11(epoch * 3.7 + seed * 11.0) > amount) {
        return 1.0; // in dieser Phase ruhig
    }
    let mode = hash11(epoch * 1.3 + seed * 5.0);
    if (mode < 0.3) {
        let fast = hash11(floor(time * 14.0) + seed * 17.0);
        return select(1.0, 0.12, fast > 0.45);
    }
    if (mode < 0.5) {
        return 0.55 + 0.45 * sin(time * (2.0 + seed % 3.0) + seed);
    }
    if (mode < 0.7) {
        return select(0.04, 1.0, hash11(floor(time * 6.0) + seed * 3.0) > 0.88);
    }
    if (mode < 0.85) {
        return 0.3;
    }
    return 0.75 + 0.25 * hash11(floor(time * 30.0) + seed);
}

fn fog_rooms(p: vec3<f32>) -> f32 {
    let dist = length(p - g.cam_pos.xyz);
    return clamp(1.0 - exp(-dist * g.horizon.w), 0.0, 1.0);
}

// Wie viel Licht der Lampe `i` hier ankommt (1 = alles). `to_lamp` = Richtung, `dl` = Abstand.
fn lamp_shadow(i: u32, p: vec3<f32>, n: vec3<f32>, to_lamp: vec3<f32>, dl: f32) -> f32 {
    let count = u32(g.extra.y);
    for (var k = 0u; k < count; k = k + 1u) {
        if (u32(lamp_tiles[k].info.x + 0.5) != i) {
            continue;
        }
        let t = lamp_tiles[k];
        // vom Objekt weg, sonst wirft es Schatten auf sich selbst
        let q = t.view_proj * vec4<f32>(p + n * (0.02 + dl * 0.012) + to_lamp * 0.03, 1.0);
        if (q.w <= 0.0) {
            return 1.0;
        }
        let ndc = q.xyz / q.w;
        let side = max(abs(ndc.x), abs(ndc.y));
        if (side >= 1.0 || ndc.z >= 1.0) {
            return 1.0;
        }
        let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        let edge = t.info.z;
        var sum = 0.0;
        for (var y = -1; y <= 1; y = y + 1) {
            for (var x = -1; x <= 1; x = x + 1) {
                let o = clamp(uv + vec2<f32>(f32(x), f32(y)) * edge, vec2<f32>(edge), vec2<f32>(1.0 - edge));
                sum = sum + textureSampleCompareLevel(lamp_atlas, shadow_samp, t.rect.xy + o * t.rect.zw, ndc.z);
            }
        }
        // am Rand des Kegels weich auslaufen
        let shown = 1.0 - smoothstep(0.8, 1.0, side);
        return 1.0 - (1.0 - sum / 9.0) * t.info.y * shown;
    }
    return 1.0;
}

fn shade_rooms(base: vec3<f32>, p: vec3<f32>, n_in: vec3<f32>) -> vec3<f32> {
    let n = normalize(n_in);
    let time = g.cam_pos.w;
    var light = vec3<f32>(0.0);
    var open = vec3<f32>(0.0); // Lampenlicht ohne Schatten
    let count = u32(g.params.x);
    for (var i = 0u; i < count; i = i + 1u) {
        let l = lights[i];
        let d = l.pos.xyz - p;
        let dl = length(d);
        if (dl < l.pos.w) {
            let att = pow(1.0 - dl / l.pos.w, 2.0);
            let diffuse = max(dot(n, d / dl), 0.0) * 0.75 + 0.25;
            var add = l.color.rgb * l.color.w * att * diffuse * flicker(l.extra.x, l.extra.y, time);
            open = open + add;
            if (g.extra.y > 0.5) {
                add = add * lamp_shadow(i, p, n, d / dl, dl);
            }
            light = light + add;
        }
    }
    let tipped = g.params.y > 0.5;
    var ambient = select(vec3<f32>(0.30, 0.30, 0.29), vec3<f32>(0.04, 0.05, 0.045), tipped);
    // Viele ueberlappende Lampen machen Schatten blass: Wo Lampenlicht fehlt, fehlt auch ein Teil
    // des Streulichts
    if (g.extra.y > 0.5) {
        let seen = dot(light, vec3<f32>(1.0)) / max(dot(open, vec3<f32>(1.0)), 0.001);
        let keep = clamp(seen, 0.0, 1.0);
        ambient = ambient * mix(1.0, keep, 0.7);
        light = light * mix(1.0, keep, 0.8);
    }
    // Schwaches Camcorder-Licht im gekippten Markt, damit man Gestalten erkennt
    if (tipped) {
        let dc = g.cam_pos.xyz - p;
        let lc = length(dc);
        let att = pow(max(1.0 - lc / 11.0, 0.0), 2.0);
        light = light + vec3<f32>(0.75, 0.8, 0.7) * 0.55 * g.extra.w * att * (max(dot(n, dc / max(lc, 0.001)), 0.0) * 0.8 + 0.2);
    }
    let ao = mix(0.75, 1.0, clamp(p.y / 1.2, 0.0, 1.0));
    let col = base * (ambient + light) * ao;
    return mix(col, g.horizon.rgb, fog_rooms(p));
}

// Live-Verbiegen der Gaenge, identisch zu world::sway in Rust:
// x = seitliches Schwanken (nur Regale/Ware), y = Hoehenwelle des Bodens (Steigung).
fn sway(p: vec3<f32>) -> vec3<f32> {
    let w = g.sway;
    if (w.x == 0.0 && w.y == 0.0) {
        return vec3<f32>(0.0);
    }
    let t = g.cam_pos.w;
    // Gangbereich beginnt bei x = 2.5, z = 4.0 (wie world::AISLES_X0/Z0)
    let mask = smoothstep(4.0, 6.0, p.z) * (1.0 - smoothstep(w.z - 2.0, w.z, p.z))
        * smoothstep(1.5, 3.0, p.x) * (1.0 - smoothstep(w.w - 0.5, w.w + 1.0, p.x));
    let lat = w.x * mask * sin(p.z * 0.45 + t * 0.7) * sin(t * 0.27 + 1.3);
    let up = w.y * mask * sin(p.z * 0.38 - t * 0.35 + 0.8);
    return vec3<f32>(lat, up, 0.0);
}

fn fog_amount(p: vec3<f32>) -> f32 {
    if (is_rooms()) {
        return fog_rooms(p);
    }
    return fog_sun(p);
}

fn shade(base: vec3<f32>, p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    if (is_rooms()) {
        return shade_rooms(base, p, n);
    }
    return shade_sun(base, p, n);
}

// Eigenleuchten im Nebel der Raeume
fn glow_rooms(color: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    return mix(color, g.horizon.rgb, fog_rooms(p) * 0.4);
}

// Normale im Vertex-Shader: Raeume rechnen mit Einheitslaenge, die Sonne normiert im Fragment
fn vertex_normal(n: vec3<f32>) -> vec3<f32> {
    if (is_rooms()) {
        return normalize(n);
    }
    return n;
}
