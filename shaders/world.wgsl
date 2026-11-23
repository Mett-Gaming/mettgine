// Vorgegebener Welt-Shader (Materialien aus KASSE 0).

#include "_common.wgsl"
#include "_world_base.wgsl"
#include "_world_kasse0.wgsl"

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
