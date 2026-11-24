//! Netze einer glTF-Datei.

use super::anim::Rig;
use crate::render::{Joints, ModelData, Vertex};
use glam::Vec3;

/// Ein Netz lesen; `None` = keine Dreiecke (Linien, Punkte).
pub(super) fn read(
    p: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
    rig: Option<&Rig>,
) -> Result<Option<ModelData>, String> {
    if p.mode() != gltf::mesh::Mode::Triangles {
        return Ok(None);
    }
    let r = p.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
    let Some(pos) = r.read_positions() else {
        return Ok(None);
    };
    let mut vertices: Vec<Vertex> = pos
        .map(|pos| Vertex {
            pos,
            ..Default::default()
        })
        .collect();
    let indices: Vec<u32> = match r.read_indices() {
        Some(i) => i.into_u32().collect(),
        None => (0..vertices.len() as u32).collect(),
    };
    if indices.iter().any(|&i| i as usize >= vertices.len()) {
        return Err("Netz verweist auf fehlende Eckpunkte".into());
    }
    let mut set = |values: &mut dyn Iterator<Item = (usize, Set)>| {
        for (i, v) in values {
            let Some(vertex) = vertices.get_mut(i) else {
                continue;
            };
            match v {
                Set::Normal(n) => vertex.normal = n,
                Set::Tangent(t) => vertex.tangent = t,
                Set::Uv(uv) => vertex.uv = uv,
                Set::Color(c) => vertex.color = c,
            }
        }
    };
    let has_normals = r.read_normals().is_some();
    let has_tangents = r.read_tangents().is_some();
    if let Some(n) = r.read_normals() {
        set(&mut n.map(Set::Normal).enumerate());
    }
    if let Some(t) = r.read_tangents() {
        set(&mut t.map(Set::Tangent).enumerate());
    }
    if let Some(uv) = r.read_tex_coords(0) {
        set(&mut uv.into_f32().map(Set::Uv).enumerate());
    }
    if let Some(c) = r.read_colors(0) {
        set(&mut c.into_rgba_f32().map(Set::Color).enumerate());
    }
    let mut data = ModelData {
        vertices,
        indices,
        joints: Vec::new(),
    };
    if !has_normals {
        flat_normals(&mut data);
    }
    if !has_tangents {
        data.compute_tangents();
    }
    if let (Some(rig), Some(j), Some(w)) = (rig, r.read_joints(0), r.read_weights(0)) {
        data.joints = j
            .into_u16()
            .zip(w.into_f32())
            .map(|(j, w)| joints(rig, j, w))
            .collect();
        data.joints.resize(data.vertices.len(), Joints::default());
    }
    Ok(Some(data))
}

enum Set {
    Normal([f32; 3]),
    Tangent([f32; 4]),
    Uv([f32; 2]),
    Color([f32; 4]),
}

/// Gewichte auf Summe 1 bringen, Knochen in die Reihenfolge des Skeletts umrechnen.
fn joints(rig: &Rig, j: [u16; 4], w: [f32; 4]) -> Joints {
    let sum: f32 = w.iter().sum();
    let weights = if sum > 1e-6 {
        w.map(|w| w / sum)
    } else {
        [1.0, 0.0, 0.0, 0.0]
    };
    Joints {
        joints: j.map(|j| rig.joint(j as usize)),
        weights,
    }
}

/// Fehlende Normalen: je Dreieck eine (Eckpunkte werden dafuer getrennt).
fn flat_normals(m: &mut ModelData) {
    let mut out = Vec::with_capacity(m.indices.len());
    for t in m.indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|i| m.vertices[i as usize]);
        let p = |v: Vertex| Vec3::from(v.pos);
        let n = (p(b) - p(a)).cross(p(c) - p(a)).normalize_or(Vec3::Y);
        for mut v in [a, b, c] {
            v.normal = n.to_array();
            out.push(v);
        }
    }
    m.indices = (0..out.len() as u32).collect();
    m.vertices = out;
}
