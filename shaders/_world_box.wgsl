// Welt-Boxen: Wuerfel aus dem Vertex-Index, gedreht und geschwankt.

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) extra: vec4<f32>,
    @location(4) center: vec3<f32>,
    @location(5) half_ext: vec3<f32>,
    @location(6) yaw: f32,
};

fn box_vertex(vi: u32, ii: u32) -> VsOut {
    var normals = array<vec3<f32>, 6>(
        vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, -1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, -1.0),
    );
    var us = array<vec3<f32>, 6>(
        vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, 1.0),
        vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(1.0, 0.0, 0.0),
    );
    var vs = array<vec3<f32>, 6>(
        vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, 1.0),
        vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 1.0, 0.0),
    );
    var qa = array<f32, 6>(-1.0, 1.0, 1.0, -1.0, 1.0, -1.0);
    var qb = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);

    let face = vi / 6u;
    let corner = vi % 6u;
    let n = normals[face];
    let local = n + us[face] * qa[corner] + vs[face] * qb[corner];
    let inst = boxes[ii];
    // center.w = Drehung um die Hochachse (fuer gebogene Regale)
    let yaw = inst.center.w;
    let c = cos(yaw);
    let s = sin(yaw);
    let lp = local * inst.half_ext.xyz;
    let rotated = vec3<f32>(lp.x * c + lp.z * s, lp.y, -lp.x * s + lp.z * c);
    var wp = inst.center.xyz + rotated;
    // Live-Verbiegen: Boden hebt und senkt sich, Regale/Ware/Schilder schwanken zusaetzlich seitlich.
    // Decke und grosse Teile (Waende) bleiben fest; was hoch haengt, geht nicht durch die Decke.
    let material = inst.extra.x;
    let sw = sway(wp);
    let lift = sw.y * (1.0 - smoothstep(2.6, 3.4, wp.y));
    if ((material > 0.5 && material < 1.5) || inst.extra.y < -0.5) {
        // Boden und alles, was flach darauf liegt (Lochraender, Truemmer)
        wp.y = wp.y + sw.y;
    } else if (!(material > 1.5 && material < 2.5) && max(inst.half_ext.x, inst.half_ext.z) < 3.0) {
        wp = wp + vec3<f32>(sw.x, lift, 0.0);
    }

    var out: VsOut;
    out.clip = g.view_proj * vec4<f32>(wp, 1.0);
    out.world_pos = wp;
    out.normal = vec3<f32>(n.x * c + n.z * s, n.y, -n.x * s + n.z * c);
    out.color = inst.color;
    out.extra = inst.extra;
    out.center = inst.center.xyz;
    out.half_ext = inst.half_ext.xyz;
    out.yaw = yaw;
    return out;
}
