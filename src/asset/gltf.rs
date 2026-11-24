//! Modelle im glTF-Format (.glb, .gltf mit eingebetteten Daten): Netze, Materialien, Texturen,
//! Knochen und Animationen. Feature `gltf`.
//!
//! ```ignore
//! let scene = mettgine::asset::gltf::parse(include_bytes!("figur.glb"))?;
//! let uploaded = scene.upload(&mut renderer)?;
//! let objects: Vec<_> = uploaded.draws(Mat4::IDENTITY, 0).collect();
//! ```

mod anim;
mod mesh;

use super::Asset;
use crate::anim::AnimSet;
use crate::render::{
    Alpha, Material, MaterialId, ModelData, ModelId, ObjectDraw, Renderer, TextureData, TextureId,
};
use glam::{Mat4, Vec3, Vec4};

/// Ein Teil der Szene: Netz mit Material an einer Stelle.
pub struct Part {
    pub name: String,
    pub data: ModelData,
    /// Index in [`Scene::materials`]; None = Vorgabe
    pub material: Option<usize>,
    /// Lage in der Szene (Teile mit Knochen: Einheit)
    pub transform: Mat4,
}

/// Ein Material; Texturen sind Indizes in [`Scene::textures`].
#[derive(Clone, Debug)]
pub struct SceneMaterial {
    pub name: String,
    /// Zahlenwerte (Texturen leer)
    pub values: Material,
    pub base: Option<usize>,
    pub metal_rough: Option<usize>,
    pub normal: Option<usize>,
    pub occlusion: Option<usize>,
    pub emissive: Option<usize>,
}

/// Inhalt einer Datei.
pub struct Scene {
    pub parts: Vec<Part>,
    pub materials: Vec<SceneMaterial>,
    pub textures: Vec<TextureData>,
    /// Knochen und Clips (Reihenfolge wie in der Datei), falls die Datei eine Haut hat
    pub anim: Option<AnimSet>,
}

impl Asset for Scene {
    fn parse(bytes: Vec<u8>, name: &str) -> Result<Self, String> {
        parse(&bytes).map_err(|e| format!("{name}: {e}"))
    }
}

/// Liest eine Datei.
pub fn parse(bytes: &[u8]) -> Result<Scene, String> {
    let (doc, buffers, images) = gltf::import_slice(bytes).map_err(|e| e.to_string())?;
    let mut textures = Textures::default();
    let materials: Vec<SceneMaterial> = doc
        .materials()
        .map(|m| material(&m, &mut textures))
        .collect();
    let textures = textures.build(&images)?;
    let skin = doc.skins().next();
    let rig = match &skin {
        Some(s) => Some(anim::Rig::new(&doc, s, &buffers)?),
        None => None,
    };
    let mut parts = Vec::new();
    let scene = doc.default_scene().or_else(|| doc.scenes().next());
    if let Some(scene) = scene {
        for n in scene.nodes() {
            walk(&n, Mat4::IDENTITY, &buffers, rig.as_ref(), &mut parts)?;
        }
    }
    if parts.is_empty() {
        return Err("keine Netze".into());
    }
    let anim = match rig {
        Some(r) => Some(r.animations(&doc, &buffers)?),
        None => None,
    };
    Ok(Scene {
        parts,
        materials,
        textures,
        anim,
    })
}

fn walk(
    node: &gltf::Node,
    parent: Mat4,
    buffers: &[gltf::buffer::Data],
    rig: Option<&anim::Rig>,
    out: &mut Vec<Part>,
) -> Result<(), String> {
    let here = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(m) = node.mesh() {
        let skinned = node.skin().is_some();
        for p in m.primitives() {
            let Some(data) = mesh::read(&p, buffers, rig.filter(|_| skinned))? else {
                continue;
            };
            out.push(Part {
                name: node.name().or(m.name()).unwrap_or("").to_string(),
                data,
                material: p.material().index(),
                // glTF: bei Teilen mit Knochen zaehlt nur die Haut
                transform: if skinned { Mat4::IDENTITY } else { here },
            });
        }
    }
    for c in node.children() {
        walk(&c, here, buffers, rig, out)?;
    }
    Ok(())
}

/// Sammelt benutzte Bilder; dasselbe Bild als Farbe und als Daten gibt zwei Texturen.
#[derive(Default)]
struct Textures {
    used: Vec<(usize, bool)>,
}

impl Textures {
    fn add(&mut self, t: &gltf::Texture, srgb: bool) -> usize {
        let key = (t.source().index(), srgb);
        match self.used.iter().position(|u| *u == key) {
            Some(i) => i,
            None => {
                self.used.push(key);
                self.used.len() - 1
            }
        }
    }

    fn build(self, images: &[gltf::image::Data]) -> Result<Vec<TextureData>, String> {
        self.used
            .iter()
            .map(|&(i, srgb)| {
                let img = images.get(i).ok_or("Bild fehlt")?;
                Ok(TextureData {
                    width: img.width,
                    height: img.height,
                    rgba: rgba(img)?,
                    srgb,
                    mips: true,
                })
            })
            .collect()
    }
}

fn rgba(img: &gltf::image::Data) -> Result<Vec<u8>, String> {
    use gltf::image::Format as F;
    let px = &img.pixels;
    Ok(match img.format {
        F::R8G8B8A8 => px.clone(),
        F::R8G8B8 => px
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        F::R8 => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        F::R8G8 => px
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], 0, 255])
            .collect(),
        // 16 Bit je Kanal: nur das hohe Byte
        F::R16G16B16A16 => px
            .as_chunks::<8>()
            .0
            .iter()
            .flat_map(|p| [p[1], p[3], p[5], p[7]])
            .collect(),
        F::R16G16B16 => px
            .as_chunks::<6>()
            .0
            .iter()
            .flat_map(|p| [p[1], p[3], p[5], 255])
            .collect(),
        other => return Err(format!("Bildformat {other:?} wird nicht unterstuetzt")),
    })
}

fn material(m: &gltf::Material, t: &mut Textures) -> SceneMaterial {
    let p = m.pbr_metallic_roughness();
    let alpha = match m.alpha_mode() {
        gltf::material::AlphaMode::Opaque => Alpha::Opaque,
        gltf::material::AlphaMode::Mask => Alpha::Mask(m.alpha_cutoff().unwrap_or(0.5)),
        gltf::material::AlphaMode::Blend => Alpha::Blend,
    };
    SceneMaterial {
        name: m.name().unwrap_or("").to_string(),
        values: Material {
            base_color: Vec4::from(p.base_color_factor()),
            metallic: p.metallic_factor(),
            roughness: p.roughness_factor(),
            normal_scale: m.normal_texture().map_or(1.0, |n| n.scale()),
            emissive: Vec3::from(m.emissive_factor()) * m.emissive_strength().unwrap_or(1.0),
            alpha,
            double_sided: m.double_sided(),
            ..Default::default()
        },
        base: p.base_color_texture().map(|i| t.add(&i.texture(), true)),
        metal_rough: p
            .metallic_roughness_texture()
            .map(|i| t.add(&i.texture(), false)),
        normal: m.normal_texture().map(|i| t.add(&i.texture(), false)),
        occlusion: m.occlusion_texture().map(|i| t.add(&i.texture(), false)),
        emissive: m.emissive_texture().map(|i| t.add(&i.texture(), true)),
    }
}

/// Eine hochgeladene Szene.
pub struct Uploaded {
    /// Modell, Material, Lage
    pub parts: Vec<(ModelId, MaterialId, Mat4)>,
    pub materials: Vec<MaterialId>,
    pub textures: Vec<TextureId>,
}

impl Scene {
    /// Alles in den Grafikspeicher laden.
    pub fn upload(&self, r: &mut Renderer) -> Result<Uploaded, Box<dyn std::error::Error>> {
        let textures = self
            .textures
            .iter()
            .map(|t| r.add_texture(t))
            .collect::<Result<Vec<_>, _>>()?;
        let tex = |i: Option<usize>| i.and_then(|i| textures.get(i).copied());
        let mut materials = Vec::new();
        for m in &self.materials {
            materials.push(r.add_material(&Material {
                base_texture: tex(m.base),
                metal_rough_texture: tex(m.metal_rough),
                normal_texture: tex(m.normal),
                occlusion_texture: tex(m.occlusion),
                emissive_texture: tex(m.emissive),
                ..m.values
            })?);
        }
        let mut plain = None;
        let mut parts = Vec::new();
        for p in &self.parts {
            let material = match p.material.and_then(|i| materials.get(i)) {
                Some(m) => *m,
                None => match plain {
                    Some(m) => m,
                    None => *plain.insert(r.add_material(&Material::default())?),
                },
            };
            parts.push((r.add_model(&p.data)?, material, p.transform));
        }
        materials.extend(plain);
        Ok(Uploaded {
            parts,
            materials,
            textures,
        })
    }
}

impl Uploaded {
    /// Alle Teile an einer Stelle zeichnen. `first_bone` nur fuer Teile mit Knochen.
    pub fn draws(&self, at: Mat4, first_bone: u32) -> impl Iterator<Item = ObjectDraw> + '_ {
        self.parts.iter().map(move |(model, material, local)| {
            let mut o = ObjectDraw::new(*model, *material, at * *local);
            o.first_bone = first_bone;
            o
        })
    }

    /// Aus dem Grafikspeicher entfernen.
    pub fn remove(self, r: &mut Renderer) -> Result<(), Box<dyn std::error::Error>> {
        for (model, _, _) in self.parts {
            r.remove_model(model);
        }
        for m in self.materials {
            r.remove_material(m);
        }
        for t in self.textures {
            r.remove_texture(t)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
