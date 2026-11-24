use super::*;
use crate::anim::ClipId;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct First;
impl ClipId for First {
    fn index(self) -> usize {
        0
    }
}

/// Kleine Datei: ein Dreieck an zwei Knochen; der zweite hebt sich in einer Sekunde um 2.
fn glb() -> Vec<u8> {
    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut put = |bytes: Vec<u8>| {
        views.push((bin.len(), bytes.len()));
        bin.extend(bytes);
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        views.len() - 1
    };
    let f = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let pos = put(f(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]));
    let idx = put([0u16, 1, 2].iter().flat_map(|x| x.to_le_bytes()).collect());
    let joints = put(vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0]);
    let weights = put(f(&[
        2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0,
    ]));
    let id = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let binds = put(f(&[id, id].concat()));
    let times = put(f(&[0.0, 1.0]));
    let moves = put(f(&[0.0, 0.0, 0.0, 0.0, 2.0, 0.0]));
    let views: Vec<String> = views
        .iter()
        .map(|(o, l)| format!(r#"{{"buffer":0,"byteOffset":{o},"byteLength":{l}}}"#))
        .collect();
    let acc = |view: usize, ty: u32, count: u32, kind: &str, extra: &str| {
        format!(
            r#"{{"bufferView":{view},"componentType":{ty},"count":{count},"type":"{kind}"{extra}}}"#
        )
    };
    let accessors = [
        acc(pos, 5126, 3, "VEC3", r#","min":[0,0,0],"max":[1,1,0]"#),
        acc(idx, 5123, 3, "SCALAR", ""),
        acc(joints, 5121, 3, "VEC4", ""),
        acc(weights, 5126, 3, "VEC4", ""),
        acc(binds, 5126, 2, "MAT4", ""),
        acc(times, 5126, 2, "SCALAR", r#","min":[0],"max":[1]"#),
        acc(moves, 5126, 2, "VEC3", ""),
    ];
    let mut json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0,1]}}],
"nodes":[{{"mesh":0,"skin":0}},{{"name":"Halter","translation":[0,0,5],"children":[2]}},{{"name":"Root","children":[3]}},{{"name":"Head"}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"JOINTS_0":2,"WEIGHTS_0":3}},"indices":1,"material":0}}]}}],
"materials":[{{"name":"Rot","pbrMetallicRoughness":{{"baseColorFactor":[1,0,0,1],"metallicFactor":0.25}},"alphaMode":"MASK"}}],
"skins":[{{"joints":[3,2],"inverseBindMatrices":4}}],
"animations":[{{"name":"Heben","channels":[{{"sampler":0,"target":{{"node":3,"path":"translation"}}}}],"samplers":[{{"input":5,"output":6}}]}}],
"accessors":[{}],"bufferViews":[{}],"buffers":[{{"byteLength":{}}}]}}"#,
        accessors.join(","),
        views.join(","),
        bin.len()
    )
    .into_bytes();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut out = b"glTF".to_vec();
    out.extend(2u32.to_le_bytes());
    out.extend(((12 + 8 + json.len() + 8 + bin.len()) as u32).to_le_bytes());
    out.extend((json.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(json);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(b"BIN\0");
    out.extend(bin);
    out
}

#[test]
fn meshes_materials_bones_and_clips_are_read() {
    let s = parse(&glb()).unwrap();
    assert_eq!(s.parts.len(), 1);
    let p = &s.parts[0];
    assert_eq!((p.data.vertices.len(), p.data.joints.len()), (3, 3));
    assert!(
        p.data.vertices[0].normal[2] > 0.99,
        "fehlende Normale berechnet"
    );
    assert_eq!(s.materials[0].values.metallic, 0.25);
    assert_eq!(s.materials[0].values.alpha, Alpha::Mask(0.5));
    // Haut-Nummer 1 ist "Root" und steht im Skelett vor seinem Kind
    let a = s.anim.unwrap();
    assert_eq!((a.bone("Root"), a.bone("Head")), (Some(0), Some(1)));
    assert_eq!(p.data.joints[2].joints[0], 0);
    assert_eq!(p.data.joints[0].joints[0], 1);
    assert_eq!(p.data.joints[0].weights[0], 1.0, "Gewichte auf 1 gebracht");
    assert_eq!(a.clip_name(First), "Heben");
    let mut out = Vec::new();
    a.sample(First, 0.5, &mut out);
    let head = out[1].transform_point3(Vec3::ZERO);
    assert!((head - Vec3::new(0.0, 1.0, 5.0)).length() < 0.05, "{head}");
}

#[test]
fn broken_files_are_errors() {
    let good = glb();
    assert!(parse(b"nichts").is_err());
    for cut in [12, 40, good.len() / 2, good.len() - 5] {
        assert!(parse(&good[..cut]).is_err());
    }
}
