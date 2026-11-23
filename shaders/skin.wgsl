// Animierte Figuren: jeder Vertex haengt an bis zu 4 Knochen (Matrizen pro Frame von der CPU).

#include "_common.wgsl"

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) joints: vec4<f32>,
    @location(4) weights: vec4<f32>,
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
    let base = u32(inst.extra.x + 0.5);
    let skin = bones[base + u32(v.joints.x)] * v.weights.x
        + bones[base + u32(v.joints.y)] * v.weights.y
        + bones[base + u32(v.joints.z)] * v.weights.z
        + bones[base + u32(v.joints.w)] * v.weights.w;
    let local = skin * vec4<f32>(v.pos, 1.0);
    var wp = inst.model * local;
    // Zerreissen: Scheiben verrutschen, Flaeche flimmert
    let d = inst.extra.z;
    if (d > 0.0) {
        let t = g.cam_pos.w;
        let slice = floor(local.y * 14.0);
        let h = hash11(slice * 3.7 + floor(t * 18.0) + inst.extra.x);
        let shift = select(0.0, (h - 0.5) * 0.35 * d, h > 1.0 - 0.45 * d);
        let n = normalize((inst.model * (skin * vec4<f32>(v.normal, 0.0))).xyz);
        let wobble = (hash11(dot(v.pos, vec3<f32>(17.0, 31.0, 7.0)) + floor(t * 24.0)) - 0.5) * 0.06 * d;
        wp = wp + vec4<f32>(shift, 0.0, shift * 0.5, 0.0) + vec4<f32>(n * wobble, 0.0);
    }
    var out: VsOut;
    out.clip = g.view_proj * wp;
    out.world_pos = wp.xyz;
    out.normal = vertex_normal((inst.model * (skin * vec4<f32>(v.normal, 0.0))).xyz);
    out.color = v.color * inst.tint.rgb;
    let blink = inst.extra.y * (0.5 + 0.5 * sin(g.cam_pos.w * 18.0));
    out.glow = inst.tint.a + blink;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let lit = shade(in.color, in.world_pos, in.normal);
    if (is_rooms()) {
        return vec4<f32>(lit + glow_rooms(in.color * in.glow, in.world_pos), 1.0);
    }
    return vec4<f32>(lit + in.color * in.glow, 1.0);
}
