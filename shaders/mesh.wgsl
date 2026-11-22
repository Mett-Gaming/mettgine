// Mesh-Shader: geladene Modelle (Mannequins, Wagen, Kasse ...) und dynamische Boxen.

#include "_common.wgsl"

struct MeshInst {
    model: mat4x4<f32>,
    tint: vec4<f32>,  // rgb = Farbmultiplikator
    extra: vec4<f32>, // x = Eigenleuchten
};

@group(0) @binding(0) var<storage, read> minst: array<MeshInst>;

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
    @location(3) emissive: f32,
};

@vertex
fn vs_main(v: VsIn, @builtin(instance_index) ii: u32) -> VsOut {
    let inst = minst[ii];
    var wp = inst.model * vec4<f32>(v.pos, 1.0);
    // Ware im Regal (statisch, extra.z = 1) folgt dem Live-Verbiegen der Gaenge
    if (inst.extra.z > 0.5) {
        let s = sway(wp.xyz);
        wp = wp + vec4<f32>(s.x, s.y * (1.0 - smoothstep(2.6, 3.4, wp.y)), 0.0, 0.0);
    }
    var out: VsOut;
    out.clip = pc.view_proj * wp;
    out.world_pos = wp.xyz;
    out.normal = normalize((inst.model * vec4<f32>(v.normal, 0.0)).xyz);
    out.color = v.color * inst.tint.rgb;
    out.emissive = inst.extra.x;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let lit = shade(in.color, in.world_pos, in.normal);
    let glow = mix(in.color * in.emissive, pc.fog_color.rgb, fog_amount(in.world_pos) * 0.4);
    return vec4<f32>(lit + glow, 1.0);
}
