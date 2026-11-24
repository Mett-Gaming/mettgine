// Moderne Grafik: Beleuchtung (Metall-Rauheit, GGX), Schatten, Himmel, Nebel.

#include "_m_sky.wgsl"

fn fog_amount(p: vec3<f32>) -> f32 {
    if (s.fog.w <= 0.0) {
        return 0.0;
    }
    let v = p - s.cam_pos.xyz;
    let k = s.misc.x * v.y;
    var run = 1.0;
    if (abs(k) > 0.001) {
        run = (1.0 - exp(-k)) / k;
    }
    let start = exp(-s.misc.x * s.cam_pos.y);
    return 1.0 - exp(-length(v) * s.fog.w * start * run);
}

fn vogel(i: i32, n: f32, turn: f32) -> vec2<f32> {
    let r = sqrt((f32(i) + 0.5) / n);
    let a = f32(i) * 2.39996323 + turn;
    return vec2<f32>(cos(a), sin(a)) * r;
}

fn sun_visibility(p: vec3<f32>, n: vec3<f32>, frag: vec2<f32>) -> f32 {
    if (s.sun_color.w < 0.5) {
        return 1.0;
    }
    let depth = dot(p - s.cam_pos.xyz, s.cam_dir.xyz);
    if (depth > s.cascade_ends.w) {
        return 1.0;
    }
    var c = 0;
    if (depth > s.cascade_ends.x) { c = 1; }
    if (depth > s.cascade_ends.y) { c = 2; }
    if (depth > s.cascade_ends.z) { c = 3; }
    // weicher Uebergang: ein Teil der Bildpunkte nimmt schon die naechste Stufe
    let turn = noise(frag);
    let end = s.cascade_ends[c];
    if (c < 3 && depth > mix(end, end * 0.9, turn)) {
        c = c + 1;
    }
    let slope = clamp(1.0 - dot(n, s.sun_dir.xyz), 0.0, 1.0);
    let at = p + n * s.cascade_texels[c] * (0.8 + 2.2 * slope);
    let q = s.cascade_vp[c] * vec4<f32>(at, 1.0);
    let uv = vec2<f32>(q.x * 0.5 + 0.5, 0.5 - q.y * 0.5);
    let soft = 1.6 * s.misc.z;
    var sum = 0.0;
    for (var i = 0; i < 12; i = i + 1) {
        let o = vogel(i, 12.0, turn * 2.0 * PI) * soft;
        sum = sum + textureSampleCompareLevel(sun_shadow, shadow_samp, uv + o, c, q.z - 0.0006);
    }
    // zur Grenze hin ausblenden
    let fade = smoothstep(s.cascade_ends.w * 0.9, s.cascade_ends.w, depth);
    return mix(sum / 12.0, 1.0, fade);
}

fn lamp_visibility(l: Lamp, p: vec3<f32>, n: vec3<f32>, frag: vec2<f32>) -> f32 {
    if (l.extra.y < 0.0) {
        return 1.0;
    }
    var t = i32(l.extra.y + 0.5);
    let d = p - l.pos.xyz;
    if (l.color.w < 0.5) {
        // Punktlampe: Wuerfelseite nach der groessten Richtung
        let a = abs(d);
        if (a.x >= a.y && a.x >= a.z) {
            t = t + select(1, 0, d.x > 0.0);
        } else if (a.y >= a.z) {
            t = t + select(3, 2, d.y > 0.0);
        } else {
            t = t + select(5, 4, d.z > 0.0);
        }
    }
    let tile = s.tiles[t];
    let dist = length(d);
    let q = tile.view_proj * vec4<f32>(p + n * dist * 0.006, 1.0);
    if (q.w <= 0.0) {
        return 1.0;
    }
    let ndc = q.xyz / q.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    // am Rand eines Feldes nicht ins Nachbarfeld greifen
    let edge = s.misc.w / tile.rect.z * 1.5;
    var sum = 0.0;
    let turn = noise(frag) * 2.0 * PI;
    for (var i = 0; i < 6; i = i + 1) {
        let o = uv + vogel(i, 6.0, turn) * edge;
        let at = tile.rect.xy + clamp(o, vec2<f32>(edge), vec2<f32>(1.0 - edge)) * tile.rect.zw;
        sum = sum + textureSampleCompareLevel(lamp_shadow, shadow_samp, at, ndc.z - 0.0004);
    }
    return sum / 6.0;
}

fn lamp_light(l: Lamp, f: Surface, p: vec3<f32>, frag: vec2<f32>) -> vec3<f32> {
    let d = l.pos.xyz - p;
    let dist2 = dot(d, d);
    let range = l.pos.w;
    if (dist2 >= range * range) {
        return vec3<f32>(0.0);
    }
    let dir = d / sqrt(max(dist2, 0.000001));
    // Abfall mit dem Quadrat, am Ende der Reichweite sauber auf 0
    let k = dist2 / (range * range);
    let window = (1.0 - k * k) * (1.0 - k * k);
    var att = window / (dist2 + 0.05);
    if (l.color.w > 0.5) {
        let c = dot(-dir, l.dir.xyz);
        att = att * smoothstep(l.dir.w, l.extra.x, c);
    }
    if (att <= 0.0) {
        return vec3<f32>(0.0);
    }
    let b = brdf(f, dir);
    if (b.x + b.y + b.z <= 0.0) {
        return vec3<f32>(0.0);
    }
    return b * l.color.rgb * att * lamp_visibility(l, p, f.n, frag);
}

fn lamps_at(f: Surface, p: vec3<f32>, frag: vec2<f32>) -> vec3<f32> {
    let depth = max(dot(p - s.cam_pos.xyz, s.cam_dir.xyz), 0.0001);
    let z = u32(clamp(log2(depth) * s.cluster.x + s.cluster.y, 0.0, 23.0));
    let x = min(u32(frag.x * s.cluster.z), 15u);
    let y = min(u32(frag.y * s.cluster.w), 8u);
    let cell = cells[(z * 9u + y) * 16u + x];
    var sum = vec3<f32>(0.0);
    for (var i = cell.x; i < cell.x + cell.y; i = i + 1u) {
        sum = sum + lamp_light(lamps[entries[i]], f, p, frag);
    }
    return sum;
}

struct GuideOut {
    @location(0) normal: vec4<f32>,
    @location(1) motion: vec2<f32>,
    @location(2) albedo: vec4<f32>,
    @location(3) specular: vec4<f32>,
    @location(4) surface: vec4<f32>, // Normale mit Vorzeichen, Rauheit
};

// Wie `fs_pre`, dazu die Bilder fuers Entrauschen durch die Technik eines Herstellers
@fragment
fn fs_guides(in: VsOut, @builtin(front_facing) front: bool) -> GuideOut {
    let m = mats[in.material];
    let base = textureSampleBias(base_tex, mat_samp, in.uv, s.cam_dir.w) * m.base * in.color;
    let n = surface_normal(in, m, front);
    let mr = textureSampleBias(metal_rough_tex, mat_samp, in.uv, s.cam_dir.w);
    if (base.a < m.params.z) {
        discard;
    }
    let rough = clamp(m.params.y * mr.g, 0.0, 1.0);
    let metal = clamp(m.params.x * mr.b, 0.0, 1.0);
    var out: GuideOut;
    out.normal = vec4<f32>(n * 0.5 + vec3<f32>(0.5), rough);
    let now = in.now.xy / in.now.w;
    let before = in.before.xy / in.before.w;
    out.motion = (now - before) * vec2<f32>(0.5, -0.5);
    out.albedo = vec4<f32>(base.rgb * (1.0 - metal), 1.0);
    let ndv = max(dot(n, normalize(s.cam_pos.xyz - in.world)), 0.0001);
    let f0 = mix(vec3<f32>(0.04), base.rgb, metal);
    out.specular = vec4<f32>(env_brdf(f0, max(rough, 0.045), ndv), 1.0);
    out.surface = vec4<f32>(n, rough);
    return out;
}

// Fertige Farbe einer Flaeche (linear, ohne Obergrenze)
fn lit(in: VsOut, front: bool) -> vec4<f32> {
    let m = mats[in.material];
    let base = textureSampleBias(base_tex, mat_samp, in.uv, s.cam_dir.w) * m.base * in.color;
    let mr = textureSampleBias(metal_rough_tex, mat_samp, in.uv, s.cam_dir.w);
    var ao = textureSampleBias(occlusion_tex, mat_samp, in.uv, s.cam_dir.w).r;
    let glow = textureSampleBias(emissive_tex, mat_samp, in.uv, s.cam_dir.w).rgb * m.emissive.rgb;
    let normal = surface_normal(in, m, front);
    // Glanz auf feinen Unebenheiten flimmert: Rauheit dort anheben
    let dn = fwidth(normal);
    if (base.a < m.params.z) {
        discard;
    }
    let metal = clamp(m.params.x * mr.b, 0.0, 1.0);
    var f: Surface;
    f.n = normal;
    f.v = normalize(s.cam_pos.xyz - in.world);
    let extra = min(dot(dn, dn) * 0.25, 0.18);
    let rough = clamp(m.params.y * mr.g, 0.045, 1.0);
    f.rough = sqrt(sqrt(clamp(rough * rough * rough * rough + extra, 0.0, 1.0)));
    f.albedo = base.rgb * (1.0 - metal);
    f.f0 = mix(vec3<f32>(0.04), base.rgb, metal);
    let frag = in.clip.xy;

    // Werte aus Bildschirm-Durchgaengen gelten nur fuer die vorderste deckende Flaeche
    let solid = pc.view_proj[0].x < 0.5;
    let traced = solid && s.rt.y > 0.5;
    var sun_ray = 1.0;
    if (solid) {
        let v = textureLoad(ao_tex, vec2<i32>(frag), 0).rg;
        if (s.misc.y > 0.5) {
            ao = min(ao, v.r);
        }
        if (s.rt.x > 0.5) {
            sun_ray = v.g;
        }
    }
    var col = vec3<f32>(0.0);
    if (s.sun_dir.w > 0.5) {
        let b = brdf(f, s.sun_dir.xyz);
        if (b.x + b.y + b.z > 0.0) {
            col = col + b * s.sun_color.rgb * sun_ray * sun_visibility(in.world, normalize(in.normal) * select(-1.0, 1.0, front), frag);
        }
    }
    if (solid && s.more.x > 0.5) {
        let q = vec2<i32>(frag);
        let fres = env_brdf(f.f0, f.rough, max(dot(f.n, f.v), 0.0001));
        col = col + textureLoad(lamp_tex, q, 0).rgb * f.albedo * (vec3<f32>(1.0) - f.f0) + textureLoad(gloss_tex, q, 0).rgb * max(fres, f.f0 * 0.5);
    } else {
        col = col + lamps_at(f, in.world, frag);
    }
    let ndv = max(dot(f.n, f.v), 0.0001);
    let env = env_brdf(f.f0, f.rough, ndv);
    let r = reflect(-f.v, f.n);
    let spec_ao = clamp(pow(ndv + ao, exp2(-16.0 * f.rough - 1.0)) - 1.0 + ao, 0.0, 1.0);
    var diffuse = sky_diffuse(f.n) * ao * s.sky_b.w;
    var mirror = sky(r, f.rough) * spec_ao * s.sky_b.w;
    if (traced) {
        diffuse = textureLoad(gi_tex, vec2<i32>(frag), 0).rgb;
    }
    if (solid && s.rt.z > 0.5) {
        mirror = textureLoad(refl_tex, vec2<i32>(frag), 0).rgb;
    }
    col = col + diffuse * f.albedo + mirror * env;
    col = col + glow;
    col = col + base.rgb * in.glow;
    col = mix(col, s.fog.rgb, fog_amount(in.world));
    return vec4<f32>(col, base.a);
}
