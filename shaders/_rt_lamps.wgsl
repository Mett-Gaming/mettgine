// Lampenlicht aus Strahlen: je Bildpunkt wird ueber viele Kandidaten, das letzte Bild und die
// Nachbarn eine passende Lampe gewaehlt (ReSTIR), und nur zu ihr geht ein Strahl.

#include "_rt_common.wgsl"

// Auswahl je Bildpunkt: x = Lampe, y = Gewicht, z = Anzahl Kandidaten (16 Bit) und Rauheit, w = Normale und Entfernung
@group(1) @binding(8) var picks: texture_2d<u32>;
@group(1) @binding(9) var motion_img: texture_2d<f32>;

struct Pick {
    lamp: u32,
    weight: f32,
    seen: f32,
    n: vec3<f32>,
    dist: f32,
};

const NO_LAMP: u32 = 0xffffffffu;

fn oct(v: vec3<f32>) -> vec2<f32> {
    let p = v.xy / (abs(v.x) + abs(v.y) + abs(v.z));
    if (v.z >= 0.0) {
        return p;
    }
    let s = select(vec2<f32>(-1.0), vec2<f32>(1.0), p >= vec2<f32>(0.0));
    return (vec2<f32>(1.0) - abs(p.yx)) * s;
}

fn unoct(p: vec2<f32>) -> vec3<f32> {
    var v = vec3<f32>(p, 1.0 - abs(p.x) - abs(p.y));
    if (v.z < 0.0) {
        let s = select(vec2<f32>(-1.0), vec2<f32>(1.0), v.xy >= vec2<f32>(0.0));
        v = vec3<f32>((vec2<f32>(1.0) - abs(v.yx)) * s, v.z);
    }
    return normalize(v);
}

fn pack(p: Pick) -> vec4<u32> {
    return vec4<u32>(
        p.lamp,
        bitcast<u32>(p.weight),
        pack2x16float(vec2<f32>(min(p.seen, 4000.0), p.dist)),
        pack2x16snorm(oct(p.n)));
}

fn unpack(v: vec4<u32>) -> Pick {
    var p: Pick;
    p.lamp = v.x;
    p.weight = bitcast<f32>(v.y);
    let a = unpack2x16float(v.z);
    p.seen = a.x;
    p.dist = a.y;
    p.n = unoct(unpack2x16snorm(v.w));
    return p;
}

// Wie wichtig eine Lampe hier ist
fn worth(lamp: u32, p: vec3<f32>, n: vec3<f32>) -> f32 {
    if (lamp >= u32(s.counts.x)) {
        return 0.0;
    }
    return dot(lamp_simple(lamps[lamp], p, n), vec3<f32>(0.3, 0.6, 0.1));
}

struct Reservoir {
    lamp: u32,
    sum: f32,
    seen: f32,
};

fn offer(r: Reservoir, lamp: u32, w: f32, seen: f32) -> Reservoir {
    var o = r;
    o.sum = o.sum + w;
    o.seen = o.seen + seen;
    if (w > 0.0 && rnd() * o.sum <= w) {
        o.lamp = lamp;
    }
    return o;
}

// Auswahl eines anderen Bildpunkts dazunehmen
fn join_pick(r: Reservoir, q: Pick, most: f32, p: vec3<f32>, n: vec3<f32>) -> Reservoir {
    let seen = min(q.seen, most);
    if (q.lamp == NO_LAMP) {
        return offer(r, NO_LAMP, 0.0, seen);
    }
    return offer(r, q.lamp, worth(q.lamp, p, n) * q.weight * seen, seen);
}

fn similar(q: Pick, n: vec3<f32>, dist: f32) -> bool {
    return dot(q.n, n) > 0.9 && abs(q.dist - dist) < 0.1 * dist;
}

// Gewicht der gewaehlten Lampe
fn final_weight(r: Reservoir, p: vec3<f32>, n: vec3<f32>) -> f32 {
    if (r.lamp == NO_LAMP || r.seen <= 0.0) {
        return 0.0;
    }
    let t = worth(r.lamp, p, n);
    if (t <= 0.0) {
        return 0.0;
    }
    return r.sum / (r.seen * t);
}

// Freie Sicht zu einem Punkt auf der (kleinen) Lampe; ihr Gehaeuse (25 cm) zaehlt nicht
fn lamp_free(lamp: u32, o: vec3<f32>) -> bool {
    let r = 0.06;
    let off = normalize(vec3<f32>(rnd() - 0.5, rnd() - 0.5, rnd() - 0.5) + vec3<f32>(0.0001)) * r;
    let d = lamps[lamp].pos.xyz + off - o;
    let far = length(d);
    return !occluded(o, d / far, max(far - 0.25, 0.002));
}
