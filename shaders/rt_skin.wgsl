// Figuren fuer Strahlen: rechnet Eckpunkte mit Knochen aus und legt Dreiecke ab.
// Aufbau der Eckpunkte: `gpu::SkinVertex` (17 Zahlen) bzw. `Vertex` (16) und `Joints` (6).

struct Push {
    tris: u32,
    out_tri: u32,  // erstes Dreieck im Ziel
    first: u32,    // erster Eckpunkt
    bones: u32,
    kind: u32,     // 0 = Modell mit Index, 1 = einfache Figur
    pad0: u32,
    pad1: u32,
    pad2: u32,
};
var<push_constant> pc: Push;

struct Tri {
    n0: vec4<f32>,
    n1: vec4<f32>,
    n2: vec4<f32>,
    v: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<storage, read> verts: array<u32>;
@group(0) @binding(1) var<storage, read> joints: array<u32>;
@group(0) @binding(2) var<storage, read> indices: array<u32>;
@group(0) @binding(3) var<storage, read> bones: array<mat4x4<f32>>;
@group(0) @binding(4) var<storage, read_write> positions: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> out_tris: array<Tri>;

fn num(i: u32) -> f32 {
    return bitcast<f32>(verts[i]);
}

struct Corner {
    p: vec3<f32>,
    n: vec3<f32>,
    uv: vec2<f32>,
    color: vec3<f32>,
};

fn blend(j: vec4<u32>, w: vec4<f32>) -> mat4x4<f32> {
    let b = pc.bones;
    return bones[b + j.x] * w.x + bones[b + j.y] * w.y + bones[b + j.z] * w.z + bones[b + j.w] * w.w;
}

fn corner(k: u32) -> Corner {
    var c: Corner;
    var m: mat4x4<f32>;
    if (pc.kind == 1u) {
        let i = (pc.first + k) * 17u;
        c.p = vec3<f32>(num(i), num(i + 1u), num(i + 2u));
        c.n = vec3<f32>(num(i + 3u), num(i + 4u), num(i + 5u));
        c.uv = vec2<f32>(0.0);
        // Vertexfarben der einfachen Modelle sind Anzeigewerte
        c.color = pow(max(vec3<f32>(num(i + 6u), num(i + 7u), num(i + 8u)), vec3<f32>(0.0)), vec3<f32>(2.2));
        let j = vec4<u32>(vec4<f32>(num(i + 9u), num(i + 10u), num(i + 11u), num(i + 12u)));
        m = blend(j, vec4<f32>(num(i + 13u), num(i + 14u), num(i + 15u), num(i + 16u)));
    } else {
        let v = indices[k];
        let i = v * 16u;
        c.p = vec3<f32>(num(i), num(i + 1u), num(i + 2u));
        c.n = vec3<f32>(num(i + 3u), num(i + 4u), num(i + 5u));
        c.uv = vec2<f32>(num(i + 10u), num(i + 11u));
        c.color = vec3<f32>(num(i + 12u), num(i + 13u), num(i + 14u));
        let q = v * 6u;
        let a = joints[q];
        let b = joints[q + 1u];
        let j = vec4<u32>(a & 0xffffu, a >> 16u, b & 0xffffu, b >> 16u);
        let w = vec4<u32>(joints[q + 2u], joints[q + 3u], joints[q + 4u], joints[q + 5u]);
        m = blend(j, bitcast<vec4<f32>>(w));
    }
    c.p = (m * vec4<f32>(c.p, 1.0)).xyz;
    c.n = (m * vec4<f32>(c.n, 0.0)).xyz;
    return c;
}

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let t = id.x;
    if (t >= pc.tris) {
        return;
    }
    let a = corner(t * 3u);
    let b = corner(t * 3u + 1u);
    let c = corner(t * 3u + 2u);
    let o = pc.out_tri + t;
    positions[o * 3u] = vec4<f32>(a.p, 1.0);
    positions[o * 3u + 1u] = vec4<f32>(b.p, 1.0);
    positions[o * 3u + 2u] = vec4<f32>(c.p, 1.0);
    var tri: Tri;
    tri.n0 = vec4<f32>(a.n, a.uv.x);
    tri.n1 = vec4<f32>(b.n, b.uv.x);
    tri.n2 = vec4<f32>(c.n, c.uv.x);
    tri.v = vec4<f32>(a.uv.y, b.uv.y, c.uv.y, 0.0);
    tri.color = vec4<f32>((a.color + b.color + c.color) / 3.0, 1.0);
    out_tris[o] = tri;
}
