// Partikel: zur Kamera gedrehte Flaechen (particles::ParticleInst), Farbe mit Alpha vormultipliziert.

#include "_common.wgsl"

struct VsIn {
    @location(0) pos_size: vec4<f32>, // xyz, w = halbe Kantenlaenge
    @location(1) color: vec4<f32>,
    @location(2) extra: vec4<f32>,    // x = Drehung, y = Aufhellen, z = Randweichheit
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) style: vec2<f32>, // x = Aufhellen, y = Randweichheit
};

@vertex
fn vs_main(v: VsIn, @builtin(vertex_index) vi: u32) -> VsOut {
    var xs = array<f32, 6>(-1.0, 1.0, 1.0, -1.0, 1.0, -1.0);
    var ys = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let uv = vec2<f32>(xs[vi], ys[vi]);
    // Zeilen 0 und 1 der Kamera-Matrix zeigen nach rechts und oben
    let m = g.view_proj;
    let right = normalize(vec3<f32>(m[0][0], m[1][0], m[2][0]));
    let up = normalize(vec3<f32>(m[0][1], m[1][1], m[2][1]));
    let c = cos(v.extra.x);
    let s = sin(v.extra.x);
    let o = vec2<f32>(uv.x * c - uv.y * s, uv.x * s + uv.y * c) * v.pos_size.w;
    let wp = v.pos_size.xyz + right * o.x + up * o.y;
    var out: VsOut;
    out.clip = m * vec4<f32>(wp, 1.0);
    out.world_pos = wp;
    out.uv = uv;
    out.color = v.color;
    out.style = vec2<f32>(v.extra.y, max(v.extra.z, 0.02));
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let edge = 1.0 - smoothstep(1.0 - in.style.y, 1.0, length(in.uv));
    let a = clamp(in.color.a, 0.0, 1.0) * edge;
    let fog = fog_amount(in.world_pos);
    // abdeckend: Farbe verschwindet im Nebel; aufhellend: Licht wird schwaecher
    let covered = mix(in.color.rgb, g.horizon.rgb, fog);
    let light = in.color.rgb * (1.0 - fog);
    let rgb = mix(covered, light, in.style.x);
    return vec4<f32>(rgb * a, a * (1.0 - in.style.x));
}
