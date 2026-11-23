// Welt-Materialien aus KASSE 0 (Altlast, nicht ausbauen): Poster, Schilder, Monitor, Spiegel.
// Neue Spiele bringen eigene Materialien mit (siehe render::WorldShaders).

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
