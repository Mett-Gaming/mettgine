// Bild in der Oberflaeche: ein Rechteck. tex0 = Bild; a = Flaeche (x, y, Breite, Hoehe),
// b = Ausschnitt (u0, v0, u1, v1), c = Farbe, d.xy = virtuelle Groesse

struct Push {
    a: vec4<f32>,
    b: vec4<f32>,
    inv: mat4x4<f32>,
    c: vec4<f32>,
    d: vec4<f32>,
};
var<push_constant> pc: Push;

@group(0) @binding(0) var tex0: texture_2d<f32>;
@group(0) @binding(4) var linear_samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var xs = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var ys = array<f32, 6>(0.0, 0.0, 1.0, 0.0, 1.0, 1.0);
    let k = vec2<f32>(xs[vi], ys[vi]);
    let p = (pc.a.xy + k * pc.a.zw) / pc.d.xy;
    var out: VsOut;
    out.pos = vec4<f32>(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
    out.uv = mix(pc.b.xy, pc.b.zw, k);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex0, linear_samp, in.uv);
    // Farbtexturen liegen linear vor, das Fenster erwartet Anzeigewerte
    let lo = c.rgb * 12.92;
    let hi = 1.055 * pow(max(c.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    let shown = select(hi, lo, c.rgb <= vec3<f32>(0.0031308));
    return vec4<f32>(shown * pc.c.rgb, c.a * pc.c.a);
}
