// VHS-Post-Pass: skaliert das Szenenbild hoch und legt Bandeffekte darueber.

struct Push {
    params: vec4<f32>, // x = Zeit, y = Glitch (0..1), z = gekippt, w = VHS-Staerke (Option)
    res: vec4<f32>,    // xy = Ausgabegroesse, zw = Quellgroesse
    extra: vec4<f32>,  // x = Schwarzblende (0..1), y = Rotblitz (0..1), z = Farbumkehr, w = Ueberbelichtung
};
var<push_constant> pc: Push;

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
// UI-Ebene (vormultipliziertes Alpha), bekommt nur einen Hauch der Bandstoerung
@group(0) @binding(2) var ui_tex: texture_2d<f32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    var out: VsOut;
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let time = pc.params.x;
    let vhs = pc.params.w;
    let glitch = clamp(pc.params.y, 0.0, 1.0);
    let tipped = pc.params.z;
    let src_h = pc.res.w;
    var uv = in.uv;
    let line = floor(uv.y * src_h);

    // Zeilen-Wobble (Bandlauf)
    let wobble = (hash21(vec2<f32>(line, floor(time * 30.0))) - 0.5)
        * ((0.0012 + tipped * 0.0008) * vhs + glitch * 0.012);
    // Wanderndes Tracking-Band
    let band_pos = fract(time * 0.06);
    let band = smoothstep(0.035, 0.0, abs(uv.y - band_pos)) * ((0.15 + tipped * 0.2) * vhs + glitch * 0.8);
    uv.x = uv.x + wobble + band * 0.015 * (hash21(vec2<f32>(line, time)) - 0.5);
    // Glitch: einzelne Bloecke verrutschen
    let block = hash21(vec2<f32>(floor(uv.y * 12.0), floor(time * 8.0)));
    uv.x = uv.x + select(0.0, (block - 0.5) * 0.04 * glitch, block > 1.0 - glitch * 0.3);

    // Chromatische Verschiebung
    let ca = (0.0012 + tipped * 0.001) * vhs + glitch * 0.005;
    let r = textureSample(src_tex, src_samp, uv + vec2<f32>(ca, 0.0)).r;
    let g = textureSample(src_tex, src_samp, uv).g;
    let b = textureSample(src_tex, src_samp, uv - vec2<f32>(ca, 0.0)).b;
    var col = vec3<f32>(r, g, b);

    // Entsaettigung, im gekippten Zustand staerker mit Gruenstich
    let luma = dot(col, vec3<f32>(0.299, 0.587, 0.114));
    col = mix(col, vec3<f32>(luma), (0.12 + tipped * 0.12) * min(vhs, 1.0));
    col = col * mix(vec3<f32>(1.0), vec3<f32>(0.92, 1.04, 0.95), tipped);

    // Rauschen
    let px = floor(in.uv * pc.res.xy * 0.5);
    let noise = hash21(px + vec2<f32>(time * 91.0, time * 37.0)) - 0.5;
    col = col + noise * ((0.035 + tipped * 0.015) * vhs + glitch * 0.12);
    col = col + band * 0.12 * hash21(vec2<f32>(floor(in.uv.x * pc.res.z), line + floor(time * 60.0)));

    // Text/HUD darueberlegen: nur leichtes Wackeln und wenig Farbversatz, damit er lesbar bleibt
    let ui_uv = in.uv + vec2<f32>(wobble * 0.2, 0.0);
    let ca_ui = min(ca * 0.25, 0.0015);
    let ur = textureSample(ui_tex, src_samp, ui_uv + vec2<f32>(ca_ui, 0.0));
    let ug = textureSample(ui_tex, src_samp, ui_uv);
    let ub = textureSample(ui_tex, src_samp, ui_uv - vec2<f32>(ca_ui, 0.0));
    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)) * (1.0 - ug.a) + vec3<f32>(ur.r, ug.g, ub.b);

    // Scanlines im Raster des Quellbilds
    col = col * (1.0 - 0.1 * vhs + 0.1 * vhs * sin(in.uv.y * src_h * 6.2831853));
    // Vignette
    let d = in.uv - vec2<f32>(0.5);
    col = col * (1.0 - dot(d, d) * 0.9 * min(vhs + 0.3, 1.0));
    // Posterize (billiges Band-Feeling)
    col = floor(col * 40.0 + 0.5) / 40.0;

    // Schreck: grell ausschlagendes Licht, dann Farbumkehr (Negativ)
    let flash = clamp(pc.extra.w, 0.0, 1.0);
    col = clamp(col * (1.0 + flash * 2.5) + vec3<f32>(flash * 0.12), vec3<f32>(0.0), vec3<f32>(1.0));
    col = mix(col, vec3<f32>(1.0) - col, clamp(pc.extra.z, 0.0, 1.0));

    // Blenden
    col = mix(col, vec3<f32>(0.7, 0.0, 0.0), pc.extra.y * 0.6);
    col = col * (1.0 - pc.extra.x);
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
