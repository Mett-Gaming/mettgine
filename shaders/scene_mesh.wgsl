// Statische Modelle (Fahrzeuge, Absperrungen, Haeuser, Sammelsachen) und Boxen.

#include "_scene.wgsl"

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) glow: f32,
};

@vertex
fn vs_main(v: VsIn, @builtin(instance_index) ii: u32) -> VsOut {
    let inst = insts[ii];
    let wp = inst.model * vec4<f32>(v.pos, 1.0);
    var out: VsOut;
    out.clip = g.view_proj * wp;
    out.world_pos = wp.xyz;
    out.normal = (inst.model * vec4<f32>(v.normal, 0.0)).xyz;
    out.color = v.color * inst.tint.rgb;
    // Blinken (Warnung vor Faehigkeiten): pulsierendes Eigenleuchten
    let blink = inst.extra.y * (0.5 + 0.5 * sin(g.cam_pos.w * 18.0));
    out.glow = inst.tint.a + blink;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let lit = shade(in.color, in.world_pos, in.normal);
    let glow = in.color * in.glow * (1.0 - fog_amount(in.world_pos));
    return vec4<f32>(lit + glow, 1.0);
}
