//! Datenformate, die zwischen Spiel und Renderer ausgetauscht werden: Welt-Boxen und
//! Lichter, Modell-Vertices, Figuren-Vertices mit Knochen-Gewichten (.k0s).

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct GpuInstance {
    /// xyz, w = Drehung um die Hochachse
    pub center: [f32; 4],
    pub half: [f32; 4],
    pub color: [f32; 4],
    /// x = Material, y = Seed, z = Leuchtstaerke, w = Flackern
    pub extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct GpuLight {
    pub pos: [f32; 4],
    pub color: [f32; 4],
    pub extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct MeshVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
}

pub struct MeshData {
    pub vertices: Vec<MeshVertex>,
    #[allow(dead_code)]
    pub min: Vec3,
    #[allow(dead_code)]
    pub max: Vec3,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SkinVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
    /// Knochen-Indizes (als Gleitkommazahl)
    pub joints: [f32; 4],
    pub weights: [f32; 4],
}

pub struct SkinData {
    pub vertices: Vec<SkinVertex>,
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

pub fn parse_skin(b: &[u8]) -> SkinData {
    assert_eq!(&b[0..4], b"K0S1", "keine K0S1-Figur");
    let n = u32_at(b, 4) as usize;
    let data = &b[32..32 + n * 68];
    let vertices = data
        .chunks_exact(68)
        .map(|v| {
            let f = |i: usize| f32_at(v, i * 4);
            SkinVertex {
                pos: [f(0), f(1), f(2)],
                normal: [f(3), f(4), f(5)],
                color: [f(6), f(7), f(8)],
                joints: [f(9), f(10), f(11), f(12)],
                weights: [f(13), f(14), f(15), f(16)],
            }
        })
        .collect();
    SkinData { vertices }
}

/// Liest ein Modell im .k0m-Format: `b"K0M1"`, u32 Vertex-Anzahl, f32[3] min, f32[3] max,
/// dann je Vertex f32[3] Position, f32[3] Normale, f32[3] Farbe (nicht indiziert, Dreiecke).
/// Fehler, wenn die Kennung oder die Laenge nicht stimmt.
pub fn parse_mesh(b: &[u8]) -> Result<MeshData, String> {
    if b.len() < 32 || &b[0..4] != b"K0M1" {
        return Err("kein K0M1-Modell".into());
    }
    let n = u32_at(b, 4) as usize;
    if b.len() < 32 + n * 36 {
        return Err(format!(
            "K0M1-Modell zu kurz ({} Byte fuer {n} Vertices)",
            b.len()
        ));
    }
    let min = Vec3::new(f32_at(b, 8), f32_at(b, 12), f32_at(b, 16));
    let max = Vec3::new(f32_at(b, 20), f32_at(b, 24), f32_at(b, 28));
    let vertices = b[32..32 + n * 36]
        .chunks_exact(36)
        .map(|v| {
            let f = |i: usize| f32_at(v, i * 4);
            MeshVertex {
                pos: [f(0), f(1), f(2)],
                normal: [f(3), f(4), f(5)],
                color: [f(6), f(7), f(8)],
            }
        })
        .collect();
    Ok(MeshData { vertices, min, max })
}

#[cfg(test)]
mod mesh_tests {
    use super::*;

    #[test]
    fn k0m_meshes_are_read_and_bad_data_is_rejected() {
        let mut b = b"K0M1".to_vec();
        b.extend_from_slice(&3u32.to_le_bytes());
        for f in [0.0f32, 0.0, 0.0, 1.0, 1.0, 0.0] {
            b.extend_from_slice(&f.to_le_bytes());
        }
        for i in 0..3 {
            for f in [i as f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.5, 0.5] {
                b.extend_from_slice(&f.to_le_bytes());
            }
        }
        let m = parse_mesh(&b).unwrap();
        assert_eq!(m.vertices.len(), 3);
        assert_eq!(m.vertices[2].pos, [2.0, 0.0, 0.0]);
        assert_eq!(m.max, Vec3::new(1.0, 1.0, 0.0));
        assert!(parse_mesh(&b[..40]).is_err());
        assert!(parse_mesh(b"K0S1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx").is_err());
    }
}
