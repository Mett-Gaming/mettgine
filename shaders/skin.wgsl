// Skin-Shader: animierte Figuren. Jeder Vertex haengt an bis zu 4 Knochen;
// die Knochen-Matrizen kommen pro Frame von der CPU (Animation abgetastet).

#include "_common.wgsl"

struct MeshInst {
    model: mat4x4<f32>,
    tint: vec4<f32>,  // rgb = Farbmultiplikator
    extra: vec4<f32>, // x = Eigenleuchten, y = erster Knochen im Knochen-Puffer, z = Zerreissen
};

@group(0) @binding(0) var<storage, read> minst: array<MeshInst>;
@group(0) @binding(4) var<storage, read> bones: array<mat4x4<f32>>;

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
    @location(3) emissive: f32,
};

@vertex
fn vs_main(v: VsIn, @builtin(instance_index) ii: u32) -> VsOut {
    let inst = minst[ii];
    let base = u32(inst.extra.y + 0.5);
    let skin = bones[base + u32(v.joints.x)] * v.weights.x
        + bones[base + u32(v.joints.y)] * v.weights.y
        + bones[base + u32(v.joints.z)] * v.weights.z
        + bones[base + u32(v.joints.w)] * v.weights.w;
    let local = skin * vec4<f32>(v.pos, 1.0);
    var wp = inst.model * local;
    // Zerreissen (spaete Stufen, wenn man direkt hinsieht): Scheiben verrutschen, Flaeche flimmert
    let d = inst.extra.z;
    if (d > 0.0) {
        let t = pc.params.x;
        let slice = floor(local.y * 14.0);
        let h = hash11(slice * 3.7 + floor(t * 18.0) + inst.extra.y);
        let shift = select(0.0, (h - 0.5) * 0.35 * d, h > 1.0 - 0.45 * d);
        let n = normalize((inst.model * (skin * vec4<f32>(v.normal, 0.0))).xyz);
        let wobble = (hash11(dot(v.pos, vec3<f32>(17.0, 31.0, 7.0)) + floor(t * 24.0)) - 0.5) * 0.06 * d;
        wp = wp + vec4<f32>(shift, 0.0, shift * 0.5, 0.0) + vec4<f32>(n * wobble, 0.0);
    }
    var out: VsOut;
    out.clip = pc.view_proj * wp;
    out.world_pos = wp.xyz;
    out.normal = normalize((inst.model * (skin * vec4<f32>(v.normal, 0.0))).xyz);
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
