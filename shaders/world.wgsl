// Welt-Shader: instanzierte Boxen. Der Wuerfel wird aus vertex_index erzeugt.

#include "_common.wgsl"

struct Instance {
    center: vec4<f32>,
    half_ext: vec4<f32>,
    color: vec4<f32>,
    // x = Material (0 normal, 1 Boden, 2 Decke, 3 leuchtend), y = Seed,
    // z = Leuchtstaerke, w = Flackern
    extra: vec4<f32>,
};

@group(0) @binding(0) var<storage, read> instances: array<Instance>;
// Schild-Atlas: je Zeile (32 px hoch, 512 px breit) ein Schriftzug
@group(0) @binding(2) var sign_tex: texture_2d<f32>;
@group(0) @binding(3) var sign_samp: sampler;
// Bild der Ueberwachungskamera (Monitor-Material 6)
@group(0) @binding(5) var monitor_tex: texture_2d<f32>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) extra: vec4<f32>,
    @location(4) center: vec3<f32>,
    @location(5) half_ext: vec3<f32>,
    @location(6) yaw: f32,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    var normals = array<vec3<f32>, 6>(
        vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, -1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, -1.0),
    );
    var us = array<vec3<f32>, 6>(
        vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, 1.0),
        vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(1.0, 0.0, 0.0),
    );
    var vs = array<vec3<f32>, 6>(
        vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, 1.0),
        vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 1.0, 0.0),
    );
    var qa = array<f32, 6>(-1.0, 1.0, 1.0, -1.0, 1.0, -1.0);
    var qb = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);

    let face = vi / 6u;
    let corner = vi % 6u;
    let n = normals[face];
    let local = n + us[face] * qa[corner] + vs[face] * qb[corner];
    let inst = instances[ii];
    // center.w = Drehung um die Hochachse (fuer gebogene Regale)
    let yaw = inst.center.w;
    let c = cos(yaw);
    let s = sin(yaw);
    let lp = local * inst.half_ext.xyz;
    let rotated = vec3<f32>(lp.x * c + lp.z * s, lp.y, -lp.x * s + lp.z * c);
    var wp = inst.center.xyz + rotated;
    // Live-Verbiegen: Boden hebt und senkt sich, Regale/Ware/Schilder schwanken zusaetzlich seitlich.
    // Decke und grosse Teile (Waende) bleiben fest; was hoch haengt, geht nicht durch die Decke.
    let material = inst.extra.x;
    let sw = sway(wp);
    let lift = sw.y * (1.0 - smoothstep(2.6, 3.4, wp.y));
    if ((material > 0.5 && material < 1.5) || inst.extra.y < -0.5) {
        // Boden und alles, was flach darauf liegt (Lochraender, Truemmer)
        wp.y = wp.y + sw.y;
    } else if (!(material > 1.5 && material < 2.5) && max(inst.half_ext.x, inst.half_ext.z) < 3.0) {
        wp = wp + vec3<f32>(sw.x, lift, 0.0);
    }

    var out: VsOut;
    out.clip = pc.view_proj * vec4<f32>(wp, 1.0);
    out.world_pos = wp;
    out.normal = vec3<f32>(n.x * c + n.z * s, n.y, -n.x * s + n.z * c);
    out.color = inst.color;
    out.extra = inst.extra;
    out.center = inst.center.xyz;
    out.half_ext = inst.half_ext.xyz;
    out.yaw = yaw;
    return out;
}

// Werbeposter mit Maskottchen-Gesicht. decay 0 = frisch, 1 = voellig verrottet und verstoerend.
fn poster(uv_in: vec2<f32>, seed: f32, base: vec3<f32>, decay: f32, time: f32) -> vec3<f32> {
    let d = clamp(decay, 0.0, 1.0);
    var uv = uv_in;
    uv.y = uv.y + d * d * 0.06 * sin(uv.x * 9.0 + time * 0.7 + seed);
    let jitter = (hash11(floor(uv.y * 20.0) + floor(time * 3.0) + seed) - 0.5) * 0.06;
    uv.x = uv.x + select(0.0, jitter, d > 0.75);

    var col = mix(base, vec3<f32>(0.95, 0.93, 0.85), 0.25);
    col = mix(col, vec3<f32>(0.25, 0.03, 0.02), smoothstep(0.6, 1.0, d) * 0.85);
    let border = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    col = select(col, vec3<f32>(0.97, 0.97, 0.95), border < 0.04);

    // Schriftzug oben kommt als eigenes Schild aus dem Atlas

    // Gesicht
    var p = uv - vec2<f32>(0.5, 0.47);
    p.y = p.y / (1.0 + d * d * 0.6);
    if (length(p) < 0.27) {
        col = mix(vec3<f32>(1.0, 0.85, 0.4), vec3<f32>(0.72, 0.78, 0.68), d);
        let er = 0.035 + d * 0.05;
        for (var k = 0; k < 2; k = k + 1) {
            let s = f32(k) * 2.0 - 1.0;
            let ec = vec2<f32>(s * 0.1, 0.07);
            let q = (p - ec) * vec2<f32>(1.0, 1.0 - d * 0.4);
            if (length(q) < er) {
                col = vec3<f32>(0.02);
                if (d > 0.85 && length(p - ec) < 0.012) {
                    col = vec3<f32>(0.95);
                }
            }
            // Blut-Traenen
            if (d > 0.55) {
                let len = (d - 0.55) * 0.6 * (0.6 + 0.4 * hash11(s + seed));
                if (abs(p.x - ec.x) < 0.012 && p.y < ec.y && p.y > ec.y - len) {
                    col = vec3<f32>(0.45, 0.0, 0.0);
                }
            }
        }
        // Mund: Laecheln -> gerade -> verkehrt -> aufgerissen mit Zaehnen
        let bend = mix(0.12, -0.18, smoothstep(0.2, 0.7, d));
        let xn = p.x / 0.13;
        let curve = -0.09 - bend * (1.0 - xn * xn) * 0.5;
        let open = smoothstep(0.7, 1.0, d) * 0.08;
        let dy = abs(p.y - curve);
        if (abs(p.x) < 0.13 && dy < 0.012 + open) {
            col = vec3<f32>(0.05, 0.0, 0.0);
            if (open > 0.02 && dy > open * 0.6 && fract(p.x * 40.0) < 0.5) {
                col = vec3<f32>(0.9, 0.88, 0.8);
            }
        }
    }

    // Preis-Stoerer
    if (length(uv - vec2<f32>(0.78, 0.15)) < 0.1) {
        col = mix(vec3<f32>(0.98, 0.85, 0.1), vec3<f32>(0.2, 0.0, 0.0), d);
    }
    // Flecken und eingerissene Ecke
    let stain = hash11(floor(uv.x * 14.0) * 7.1 + floor(uv.y * 18.0) * 3.3 + seed);
    col = col * (1.0 - d * 0.5 * step(0.7, stain));
    if (d > 0.4 && uv.x + (1.0 - uv.y) < 0.25 * d) {
        col = vec3<f32>(0.12, 0.12, 0.1);
    }
    return col;
}

// Textfarbe eines Schilds (color.a): 0 weiss, 1 dunkelblau, 2 schwarz, 3 gelb, 4 rot
fn sign_ink(code: f32) -> vec3<f32> {
    if (code < 0.5) { return vec3<f32>(0.97, 0.97, 0.94); }
    if (code < 1.5) { return vec3<f32>(0.08, 0.13, 0.3); }
    if (code < 2.5) { return vec3<f32>(0.05, 0.05, 0.05); }
    if (code < 3.5) { return vec3<f32>(0.98, 0.82, 0.15); }
    return vec3<f32>(0.85, 0.1, 0.08);
}

// Schild mit Schriftzug aus dem Atlas. Beide grossen Seiten sind lesbar.
fn sign_face(in: VsOut) -> vec3<f32> {
    let c = cos(in.yaw);
    let s = sin(in.yaw);
    let d = in.world_pos - in.center;
    let l = vec3<f32>(d.x * c - d.z * s, d.y, d.x * s + d.z * c);
    let n = vec3<f32>(in.normal.x * c - in.normal.z * s, in.normal.y, in.normal.x * s + in.normal.z * c);
    let h = in.half_ext;
    var bg = in.color.rgb;
    let decay = clamp(pc.fog_color.w, 0.0, 1.0);
    // Flecken im Verfall
    let stain = hash11(floor(l.x * 9.0 + l.z * 9.0) * 3.7 + floor(l.y * 11.0) * 1.3 + in.extra.y);
    bg = bg * (1.0 - decay * 0.45 * step(0.65, stain));
    if (abs(n.y) > 0.5) {
        return bg;
    }
    var u = 0.0;
    var w = 0.0;
    if (abs(n.x) > abs(n.z)) {
        w = 2.0 * h.z;
        u = 0.5 - sign(n.x) * l.z / w;
    } else {
        w = 2.0 * h.x;
        u = 0.5 + sign(n.z) * l.x / w;
    }
    let v = 0.5 + l.y / (2.0 * h.y);
    let aspect = w / (2.0 * h.y);
    let dims = vec2<f32>(textureDimensions(sign_tex));
    let px = dims.x * 0.5 + (u - 0.5) * aspect * 32.0;
    let py = (in.extra.y + 1.0 - v) * 32.0;
    var cov = textureSampleLevel(sign_tex, sign_samp, vec2<f32>(px, py) / dims, 0.0).r;
    cov = select(cov, 0.0, px < 0.0 || px > dims.x);
    // Im Verfall fallen einzelne Buchstaben aus und flackern
    let letter = floor(px / 13.0);
    let t = floor(pc.params.x * 2.0);
    if (decay > 0.3 && hash11(letter * 1.7 + in.extra.y * 5.3 + t * 0.37) < (decay - 0.3) * 0.5) {
        cov = cov * 0.1;
    }
    let ink = sign_ink(in.color.a);
    return mix(bg, ink, cov);
}

// Bildschirm des Ueberwachungsmonitors. In der Kamera selbst bleibt er dunkel.
fn monitor_face(in: VsOut) -> vec3<f32> {
    let dark = vec3<f32>(0.03, 0.035, 0.03);
    if (pc.cam_pos.w > 1.5) {
        return dark;
    }
    let c = cos(in.yaw);
    let s = sin(in.yaw);
    let d = in.world_pos - in.center;
    let l = vec3<f32>(d.x * c - d.z * s, d.y, d.x * s + d.z * c);
    let n = vec3<f32>(in.normal.x * c - in.normal.z * s, in.normal.y, in.normal.x * s + in.normal.z * c);
    if (n.z > -0.5) {
        return dark;
    }
    let h = in.half_ext;
    let u = 0.5 - l.x / (2.0 * h.x);
    let v = 0.5 - l.y / (2.0 * h.y);
    let t = pc.params.x;
    // Bildzittern und laufender Stoerbalken
    let roll = fract(t * 0.13);
    let bar = 1.0 - smoothstep(0.0, 0.05, abs(v - roll)) * 0.5;
    let jitter = (hash11(floor(v * 90.0) + floor(t * 12.0)) - 0.5) * 0.004;
    let src = textureSampleLevel(monitor_tex, sign_samp, vec2<f32>(clamp(u + jitter, 0.0, 1.0), clamp(v, 0.0, 1.0)), 0.0).rgb;
    let lum = dot(src, vec3<f32>(0.3, 0.55, 0.15));
    var col = vec3<f32>(lum) * vec3<f32>(0.85, 1.0, 0.9) * 1.6 + vec3<f32>(0.03);
    col = col * (0.82 + 0.18 * sin(v * 3.14159 * 90.0)) * bar;
    col = col + (hash11(floor(u * 160.0) * 1.7 + floor(v * 90.0) * 13.1 + floor(t * 24.0)) - 0.5) * 0.06;
    let edge = min(min(u, 1.0 - u), min(v, 1.0 - v));
    col = col * smoothstep(0.0, 0.08, edge);
    return max(col, vec3<f32>(0.0)) * in.extra.z;
}

// Spiegel am Ende des Fluchtkorridors: Bild der zweiten Kamera (vom gespiegelten Auge aus),
// seitenverkehrt, in Farbe, alt und fleckig. In der Kamera selbst bleibt er dunkel.
fn mirror_face(in: VsOut) -> vec3<f32> {
    let dark = vec3<f32>(0.05, 0.05, 0.055);
    if (pc.cam_pos.w > 1.5) {
        return dark;
    }
    let c = cos(in.yaw);
    let s = sin(in.yaw);
    let d = in.world_pos - in.center;
    let l = vec3<f32>(d.x * c - d.z * s, d.y, d.x * s + d.z * c);
    let n = vec3<f32>(in.normal.x * c - in.normal.z * s, in.normal.y, in.normal.x * s + in.normal.z * c);
    if (n.z > -0.5) {
        return dark;
    }
    let h = in.half_ext;
    let u = 0.5 - l.x / (2.0 * h.x);
    let v = 0.5 - l.y / (2.0 * h.y);
    let t = pc.params.x;
    // ganz leichtes Wabern, als ob das Glas nicht ganz eben waere
    let wob = sin(v * 23.0 + t * 1.3) * 0.002;
    let src = textureSampleLevel(monitor_tex, sign_samp, vec2<f32>(clamp(1.0 - u + wob, 0.0, 1.0), clamp(v, 0.0, 1.0)), 0.0).rgb;
    var col = src * vec3<f32>(0.92, 0.95, 0.97) * 1.15;
    // blinde Flecken und Kratzer im Silber
    let blot = hash11(floor(u * 24.0) * 3.1 + floor(v * 14.0) * 17.3);
    col = col * (0.85 + 0.15 * blot);
    let scratch = select(0.0, 0.12, abs(fract(u * 3.0 + v * 7.0) - 0.5) < 0.004);
    col = col + vec3<f32>(scratch);
    let edge = min(min(u, 1.0 - u), min(v, 1.0 - v));
    col = col * mix(0.35, 1.0, smoothstep(0.0, 0.12, edge));
    return max(col, vec3<f32>(0.0)) * in.extra.z;
}

fn grid_line(v: f32, cell: f32, width: f32) -> f32 {
    let f = fract(v / cell);
    return select(0.0, 1.0, min(f, 1.0 - f) < width);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let material = in.extra.x;
    let p = in.world_pos;
    var base = in.color.rgb;

    // Spiegel (Material 7): Bild der zweiten Kamera, seitenverkehrt
    if (material > 6.5) {
        return vec4<f32>(mirror_face(in), 1.0);
    }
    // Ueberwachungsmonitor (Material 6): Bild der Deckenkamera, grau, mit Zeilen und Rauschen
    if (material > 5.5) {
        return vec4<f32>(monitor_face(in), 1.0);
    }
    // Schilder mit Schriftzug (Material 5)
    if (material > 4.5) {
        let col = sign_face(in);
        if (in.extra.z > 0.0) {
            let f = flicker(in.extra.y, in.extra.w, pc.params.x);
            let lit = col * in.extra.z * f;
            return vec4<f32>(mix(lit, pc.fog_color.rgb, fog_amount(p) * 0.4), 1.0);
        }
        return vec4<f32>(shade(col, p, in.normal), 1.0);
    }
    // Werbeposter (prozedural, verrotten mit der Eskalation)
    if (material > 3.5) {
        let c = in.center;
        let h = in.half_ext;
        var uv = vec2<f32>((p.x - c.x) / (2.0 * h.x) + 0.5, (p.y - c.y) / (2.0 * h.y) + 0.5);
        if (h.x < h.z) {
            uv.x = (p.z - c.z) / (2.0 * h.z) + 0.5;
        }
        let decay = pc.fog_color.w;
        let pc_col = poster(uv, in.extra.y, base, decay, pc.params.x);
        let lit = shade(pc_col, p, in.normal) + pc_col * 0.12 * (0.5 + decay);
        return vec4<f32>(lit, 1.0);
    }
    // Leuchtende Flaechen (Roehren, Schilder, Glastuer)
    if (material > 2.5) {
        let f = flicker(in.extra.y, in.extra.w, pc.params.x);
        let lit = base * in.extra.z * f;
        return vec4<f32>(mix(lit, pc.fog_color.rgb, fog_amount(p) * 0.4), 1.0);
    }
    // Boden: Fliesen mit Fugen
    if (material > 0.5 && material < 1.5) {
        let cell = floor(p.x / 0.6) + floor(p.z / 0.6);
        let checker = fract(cell * 0.5) * 2.0;
        base = base * (0.9 + 0.1 * checker);
        let grout = max(grid_line(p.x, 0.6, 0.025), grid_line(p.z, 0.6, 0.025));
        base = base * (1.0 - 0.3 * grout);
    }
    // Decke: Rasterplatten
    if (material > 1.5 && material < 2.5) {
        let grout = max(grid_line(p.x, 1.2, 0.012), grid_line(p.z, 0.6, 0.02));
        base = base * (1.0 - 0.35 * grout);
    }
    return vec4<f32>(shade(base, p, in.normal), 1.0);
}
