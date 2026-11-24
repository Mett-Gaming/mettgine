// Verdecktes aussortieren: Kugel jedes Dings gegen die Tiefenpyramide pruefen.
// Durchgang 1: Pyramide und Kamera des letzten Bildes. Durchgang 2: nur was dort verdeckt war, gegen
// die frische Pyramide; so fehlt nie etwas, das gerade erst sichtbar wurde.

struct Item {
    sphere: vec4<f32>, // w < 0 = immer sichtbar
    obj: u32,
    batch: u32,
    first: u32,        // Ziel in `order` fuer Durchgang 1
    second: u32,       // … fuer Durchgang 2
};

struct Push {
    view_proj: mat4x4<f32>,
    count: u32,
    pass_no: u32,      // 1 oder 2
    commands: u32,     // erster Befehl dieses Durchgangs
    valid: u32,        // 0 = es gibt keine Pyramide
};
var<push_constant> pc: Push;

@group(0) @binding(0) var<storage, read> items: array<Item>;
@group(0) @binding(1) var<storage, read_write> commands: array<atomic<u32>>;
@group(0) @binding(2) var<storage, read_write> order: array<u32>;
@group(0) @binding(3) var<storage, read_write> hidden: array<u32>;
@group(0) @binding(4) var hiz: texture_2d<f32>;

fn is_visible(sphere: vec4<f32>) -> bool {
    if (sphere.w < 0.0 || pc.valid == 0u) {
        return true;
    }
    var lo = vec3<f32>(9.0);
    var hi = vec3<f32>(-9.0);
    for (var i = 0u; i < 8u; i = i + 1u) {
        let s = vec3<f32>(
            select(-1.0, 1.0, (i & 1u) != 0u),
            select(-1.0, 1.0, (i & 2u) != 0u),
            select(-1.0, 1.0, (i & 4u) != 0u));
        let c = pc.view_proj * vec4<f32>(sphere.xyz + s * sphere.w, 1.0);
        if (c.w <= 0.01) {
            return true;
        }
        let n = c.xyz / c.w;
        lo = min(lo, n);
        hi = max(hi, n);
    }
    if (hi.x < -1.0 || hi.y < -1.0 || lo.x > 1.0 || lo.y > 1.0) {
        // Durchgang 1 prueft mit der alten Kamera: dort ausserhalb sagt nichts
        return pc.pass_no == 1u;
    }
    let a = clamp(vec2<f32>(lo.x * 0.5 + 0.5, 0.5 - hi.y * 0.5), vec2<f32>(0.0), vec2<f32>(1.0));
    let b = clamp(vec2<f32>(hi.x * 0.5 + 0.5, 0.5 - lo.y * 0.5), vec2<f32>(0.0), vec2<f32>(1.0));
    let full = vec2<f32>(textureDimensions(hiz));
    let span = (b - a) * full;
    let top = i32(textureNumLevels(hiz)) - 1;
    let mip = clamp(i32(ceil(log2(max(max(span.x, span.y), 1.0)))), 0, top);
    let size = vec2<i32>(textureDimensions(hiz, mip));
    let p = clamp(vec2<i32>(a * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1));
    let q = clamp(vec2<i32>(b * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1));
    var far = max(textureLoad(hiz, p, mip).r, textureLoad(hiz, q, mip).r);
    far = max(far, max(textureLoad(hiz, vec2<i32>(p.x, q.y), mip).r, textureLoad(hiz, vec2<i32>(q.x, p.y), mip).r));
    return lo.z <= far + 0.00001;
}

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= pc.count) {
        return;
    }
    if (pc.pass_no == 2u && hidden[i] == 0u) {
        return;
    }
    let it = items[i];
    let seen = is_visible(it.sphere);
    if (pc.pass_no == 1u) {
        hidden[i] = select(1u, 0u, seen);
    }
    if (!seen) {
        return;
    }
    let slot = atomicAdd(&commands[(pc.commands + it.batch) * 5u + 1u], 1u);
    order[select(it.second, it.first, pc.pass_no == 1u) + slot] = it.obj;
}
