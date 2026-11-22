// Gemeinsame Beleuchtung fuer Boxen und Meshes.

struct Light {
    pos: vec4<f32>,   // xyz, w = Radius
    color: vec4<f32>, // rgb, w = Intensitaet
    extra: vec4<f32>, // x = Seed, y = Flackern
};

struct Push {
    view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    params: vec4<f32>, // x = Zeit, y = Lichtanzahl, z = gekippt, w = Nebeldichte
    fog_color: vec4<f32>,
    sway: vec4<f32>, // x = seitlich, y = hoch, z = Gangende z, w = Gangende x
};

@group(0) @binding(1) var<storage, read> lights: array<Light>;
var<push_constant> pc: Push;

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

fn fog_amount(p: vec3<f32>) -> f32 {
    let dist = length(p - pc.cam_pos.xyz);
    return clamp(1.0 - exp(-dist * pc.params.w), 0.0, 1.0);
}

fn shade(base: vec3<f32>, p: vec3<f32>, n_in: vec3<f32>) -> vec3<f32> {
    let n = normalize(n_in);
    let time = pc.params.x;
    var light = vec3<f32>(0.0);
    let count = u32(pc.params.y);
    for (var i = 0u; i < count; i = i + 1u) {
        let l = lights[i];
        let d = l.pos.xyz - p;
        let dl = length(d);
        if (dl < l.pos.w) {
            let att = pow(1.0 - dl / l.pos.w, 2.0);
            let diffuse = max(dot(n, d / dl), 0.0) * 0.75 + 0.25;
            light = light + l.color.rgb * l.color.w * att * diffuse * flicker(l.extra.x, l.extra.y, time);
        }
    }
    let tipped = pc.params.z > 0.5;
    let ambient = select(vec3<f32>(0.30, 0.30, 0.29), vec3<f32>(0.04, 0.05, 0.045), tipped);
    // Schwaches Camcorder-Licht im gekippten Markt, damit man Gestalten erkennt
    if (tipped) {
        let dc = pc.cam_pos.xyz - p;
        let lc = length(dc);
        let att = pow(max(1.0 - lc / 11.0, 0.0), 2.0);
        light = light + vec3<f32>(0.75, 0.8, 0.7) * 0.55 * att * (max(dot(n, dc / max(lc, 0.001)), 0.0) * 0.8 + 0.2);
    }
    let ao = mix(0.75, 1.0, clamp(p.y / 1.2, 0.0, 1.0));
    let col = base * (ambient + light) * ao;
    return mix(col, pc.fog_color.rgb, fog_amount(p));
}

// Live-Verbiegen der Gaenge, identisch zu world::sway in Rust:
// x = seitliches Schwanken (nur Regale/Ware), y = Hoehenwelle des Bodens (Steigung).
fn sway(p: vec3<f32>) -> vec3<f32> {
    let w = pc.sway;
    if (w.x == 0.0 && w.y == 0.0) {
        return vec3<f32>(0.0);
    }
    let t = pc.params.x;
    // Gangbereich beginnt bei x = 2.5, z = 4.0 (wie world::AISLES_X0/Z0)
    let mask = smoothstep(4.0, 6.0, p.z) * (1.0 - smoothstep(w.z - 2.0, w.z, p.z))
        * smoothstep(1.5, 3.0, p.x) * (1.0 - smoothstep(w.w - 0.5, w.w + 1.0, p.x));
    let lat = w.x * mask * sin(p.z * 0.45 + t * 0.7) * sin(t * 0.27 + 1.3);
    let up = w.y * mask * sin(p.z * 0.38 - t * 0.35 + 0.8);
    return vec3<f32>(lat, up, 0.0);
}
