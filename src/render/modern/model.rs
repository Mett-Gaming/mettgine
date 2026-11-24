//! Modelle, Texturen und Materialien der modernen Grafik, wie das Spiel sie uebergibt.

use glam::{Mat4, Vec2, Vec3, Vec4};

/// Ein Eckpunkt eines Modells mit Texturen (64 Byte).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// xyz = Tangente, w = Richtung der Bitangente (+1 / -1)
    pub tangent: [f32; 4],
    pub uv: [f32; 2],
    /// Farbmultiplikator (linear)
    pub color: [f32; 4],
}

impl Default for Vertex {
    fn default() -> Self {
        Self {
            pos: [0.0; 3],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
            uv: [0.0; 2],
            color: [1.0; 4],
        }
    }
}

/// Knochen eines Eckpunkts (Indizes und Gewichte).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Joints {
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

/// Modell mit Index-Liste (Dreiecke, gegen den Uhrzeigersinn).
#[derive(Clone, Debug, Default)]
pub struct ModelData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// leer = ohne Knochen, sonst ein Eintrag je Eckpunkt
    pub joints: Vec<Joints>,
}

impl ModelData {
    /// Aus einem einfachen Modell (Vertexfarben, ohne Texturkoordinaten).
    pub fn from_mesh(m: &crate::gpu::MeshData) -> Self {
        let linear = |c: f32| c.max(0.0).powf(2.2);
        let vertices: Vec<Vertex> = m
            .vertices
            .iter()
            .map(|v| Vertex {
                pos: v.pos,
                normal: v.normal,
                color: [
                    linear(v.color[0]),
                    linear(v.color[1]),
                    linear(v.color[2]),
                    1.0,
                ],
                ..Default::default()
            })
            .collect();
        Self {
            indices: (0..vertices.len() as u32).collect(),
            vertices,
            joints: Vec::new(),
        }
    }

    /// Texturkoordinaten aus der Lage berechnen: jede Flaeche bekommt ihre Hauptrichtung,
    /// `scale` = Wiederholungen pro Meter.
    pub fn box_uvs(mut self, scale: f32) -> Self {
        for v in &mut self.vertices {
            let n = Vec3::from(v.normal).abs();
            let [x, y, z] = v.pos;
            let uv = if n.x >= n.y && n.x >= n.z {
                [z, -y]
            } else if n.y >= n.z {
                [x, z]
            } else {
                [x, -y]
            };
            v.uv = uv.map(|c| c * scale);
        }
        self.compute_tangents();
        self
    }

    /// Tangenten aus Lage, Normalen und Texturkoordinaten berechnen.
    pub fn compute_tangents(&mut self) {
        let mut sum = vec![(Vec3::ZERO, Vec3::ZERO); self.vertices.len()];
        for t in self.indices.as_chunks::<3>().0 {
            let i = t.map(|i| i as usize);
            let [a, b, c] = i.map(|i| self.vertices[i]);
            let p = |v: Vertex| Vec3::from(v.pos);
            let uv = |v: Vertex| Vec2::from(v.uv);
            let (e1, e2) = (p(b) - p(a), p(c) - p(a));
            let (d1, d2) = (uv(b) - uv(a), uv(c) - uv(a));
            let det = d1.x * d2.y - d2.x * d1.y;
            if det.abs() < 1e-12 {
                continue;
            }
            let tangent = (e1 * d2.y - e2 * d1.y) / det;
            let bitangent = (e2 * d1.x - e1 * d2.x) / det;
            for i in i {
                sum[i].0 += tangent;
                sum[i].1 += bitangent;
            }
        }
        for (v, (t, b)) in self.vertices.iter_mut().zip(sum) {
            let n = Vec3::from(v.normal);
            let t = (t - n * n.dot(t)).normalize_or(n.any_orthonormal_vector());
            let side = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
            v.tangent = t.extend(side).to_array();
        }
    }

    /// Mittelpunkt und Radius einer Huellkugel.
    pub fn bounds(&self) -> (Vec3, f32) {
        let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for v in &self.vertices {
            min = min.min(v.pos.into());
            max = max.max(v.pos.into());
        }
        if self.vertices.is_empty() {
            return (Vec3::ZERO, 0.0);
        }
        let mid = (min + max) * 0.5;
        let far = |v: &Vertex| Vec3::from(v.pos).distance_squared(mid);
        let r2 = self.vertices.iter().map(far).fold(0.0, f32::max);
        (mid, r2.sqrt())
    }
}

/// Ein Bild fuer die Grafikkarte: RGBA, 8 Bit, oben links zuerst.
#[derive(Clone, Debug)]
pub struct TextureData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Farbe (sRGB) oder Daten (Normalen, Rauheit: linear)
    pub srgb: bool,
    /// Verkleinerungsstufen erzeugen
    pub mips: bool,
}

/// Textur im Grafikspeicher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextureId(pub(super) u32);
/// Material im Grafikspeicher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MaterialId(pub(super) u32);
/// Modell im Grafikspeicher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModelId(pub(super) u32);

/// Deckkraft eines Materials.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alpha {
    Opaque,
    /// Loecher: darunter unsichtbar (Blaetter, Gitter)
    Mask(f32),
    /// halbtransparent, von hinten nach vorn gezeichnet
    Blend,
}

/// Material nach dem Metall-Rauheits-Modell. Farben linear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub base_color: Vec4,
    pub base_texture: Option<TextureId>,
    /// 0 = Kunststoff, Holz, Stein … 1 = Metall
    pub metallic: f32,
    /// 0 = spiegelglatt, 1 = matt
    pub roughness: f32,
    /// G = Rauheit, B = Metall (wie glTF)
    pub metal_rough_texture: Option<TextureId>,
    pub normal_texture: Option<TextureId>,
    pub normal_scale: f32,
    /// R = Verdeckung
    pub occlusion_texture: Option<TextureId>,
    /// Eigenleuchten (darf ueber 1 liegen)
    pub emissive: Vec3,
    pub emissive_texture: Option<TextureId>,
    pub alpha: Alpha,
    pub double_sided: bool,
}

impl Default for Material {
    /// Mattes Weiss.
    fn default() -> Self {
        Self {
            base_color: Vec4::ONE,
            base_texture: None,
            metallic: 0.0,
            roughness: 0.8,
            metal_rough_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
            occlusion_texture: None,
            emissive: Vec3::ZERO,
            emissive_texture: None,
            alpha: Alpha::Opaque,
            double_sided: false,
        }
    }
}

/// Ein Modell mit Material in diesem Bild.
#[derive(Clone, Copy, Debug)]
pub struct ObjectDraw {
    pub model: ModelId,
    pub material: MaterialId,
    pub transform: Mat4,
    /// Lage im letzten Bild (fuer TAA und Bewegung); None = unbewegt
    pub previous: Option<Mat4>,
    /// Farbmultiplikator
    pub tint: Vec4,
    /// erster Knochen in `FrameInput::bones` (nur Modelle mit Knochen)
    pub first_bone: u32,
    pub shadow: bool,
}

impl ObjectDraw {
    pub fn new(model: ModelId, material: MaterialId, transform: Mat4) -> Self {
        Self {
            model,
            material,
            transform,
            previous: None,
            tint: Vec4::ONE,
            first_bone: 0,
            shadow: true,
        }
    }
}

/// Dasselbe Ding in mehreren Feinheiten.
#[derive(Clone, Debug, Default)]
pub struct Lod {
    /// (bis zu dieser Entfernung in Metern, Modell), aufsteigend
    levels: Vec<(f32, ModelId)>,
}

impl Lod {
    pub fn new(mut levels: Vec<(f32, ModelId)>) -> Self {
        levels.sort_by(|a, b| a.0.total_cmp(&b.0));
        Self { levels }
    }

    /// Modell fuer eine Entfernung; `None` = zu weit weg, gar nicht zeichnen.
    pub fn pick(&self, distance: f32) -> Option<ModelId> {
        let l = self.levels.iter().find(|l| distance <= l.0)?;
        Some(l.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lod_picks_by_distance() {
        let l = Lod::new(vec![(80.0, ModelId(2)), (20.0, ModelId(1))]);
        assert_eq!(l.pick(5.0), Some(ModelId(1)));
        assert_eq!(l.pick(50.0), Some(ModelId(2)));
        assert_eq!(l.pick(100.0), None);
    }

    #[test]
    fn tangents_follow_the_texture_axes() {
        let m = ModelData::from_mesh(&crate::shapes::plane(2.0)).box_uvs(1.0);
        for v in &m.vertices {
            let (n, t) = (Vec3::from(v.normal), Vec4::from(v.tangent));
            assert!(n.dot(t.truncate()).abs() < 1e-5);
            // u waechst mit x
            assert!(t.x > 0.99, "{t}");
        }
        let (mid, r) = m.bounds();
        assert!(mid.length() < 1e-5 && (r - 2f32.sqrt()).abs() < 1e-4);
        assert_eq!(std::mem::size_of::<Vertex>(), 64);
        assert_eq!(std::mem::size_of::<Joints>(), 24);
    }
}
