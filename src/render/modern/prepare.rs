//! Was gezeichnet wird: Objekte sammeln, sortieren, je Sicht (Kamera, Schatten) aussortieren
//! und zu Zeichenbefehlen buendeln.

use super::gpu::GpuObj;
use super::model::*;
use super::store::Store;
use super::types::*;
use crate::cull::{Frustum, SkinBounds, Sphere};
use crate::render::FrameInput;
use glam::{Mat4, Vec3, Vec4};
use std::ops::Range;

/// Deckkraft von Hologrammen (wie in der einfachen Grafik)
const GHOST_ALPHA: f32 = 0.55;
/// Eigenleuchten einfacher Modelle: Faktor auf die Anzeigefarbe
const PLAIN_GLOW: f32 = 1.5;

/// Woraus ein Objekt besteht.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// Modell mit Texturen
    Object,
    /// … mit Knochen
    Skinned,
    /// einfaches Modell (`MeshDraw`)
    Mesh,
    /// einfache Figur (`SkinDraw`)
    Skin,
}

pub const KINDS: [Kind; 4] = [Kind::Object, Kind::Skinned, Kind::Mesh, Kind::Skin];

/// Ein Zeichenbefehl.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Batch {
    pub kind: Kind,
    pub masked: bool,
    pub double_sided: bool,
    pub material: u32,
    pub model: u32,
    /// Stelle in der Reihenfolge-Liste
    pub first: u32,
    pub count: u32,
}

/// Sortierschluessel eines Objekts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    kind: Kind,
    masked: bool,
    double_sided: bool,
    material: u32,
    model: u32,
}

#[derive(Clone, Copy, Debug)]
struct Item {
    key: Key,
    /// Stelle in `objs`
    obj: u32,
    /// None = Ausdehnung unbekannt
    sphere: Option<Sphere>,
    shadow: bool,
    blend: bool,
}

/// Wiederverwendete Listen eines Bildes.
#[derive(Default)]
pub struct Lists {
    pub objs: Vec<GpuObj>,
    /// Reihenfolge der Instanzen aller Sichten (Stellen in `objs`)
    pub order: Vec<u32>,
    pub batches: Vec<Batch>,
    items: Vec<Item>,
    blended: Vec<(f32, u32)>,
    pub drawn: u32,
    pub culled: u32,
}

/// Was die einfachen Modelle und Figuren brauchen.
pub struct Plain<'a> {
    pub mesh_bounds: &'a [Sphere],
    pub skin_bounds: &'a [SkinBounds],
    /// Material fuer alle einfachen Dinge
    pub material: u32,
}

impl Lists {
    /// Objekte des Bildes sammeln und sortieren.
    pub fn collect(&mut self, f: &FrameInput, s: &SceneInput, store: &Store, plain: &Plain) {
        self.objs.clear();
        self.order.clear();
        self.batches.clear();
        self.items.clear();
        (self.drawn, self.culled) = (0, 0);
        self.add_objects(s, store);
        self.add_meshes(f, plain);
        self.add_skins(f, plain);
        self.items.sort_unstable_by_key(|i| (i.blend, i.key, i.obj));
    }

    fn push(&mut self, o: GpuObj, item: Item) {
        self.items.push(Item {
            obj: self.objs.len() as u32,
            ..item
        });
        self.objs.push(o);
    }

    fn add_objects(&mut self, s: &SceneInput, store: &Store) {
        for o in s.objects {
            let (Some(model), Some(mat)) = (store.model(o.model), store.material(o.material))
            else {
                continue;
            };
            let m = &mat.data;
            let skinned = model.joints.is_some();
            let key = Key {
                kind: if skinned { Kind::Skinned } else { Kind::Object },
                masked: matches!(m.alpha, Alpha::Mask(_)),
                double_sided: m.double_sided,
                material: o.material.0,
                model: o.model.0,
            };
            let obj = GpuObj {
                model: o.transform.to_cols_array_2d(),
                prev: o.previous.unwrap_or(o.transform).to_cols_array_2d(),
                tint: o.tint.to_array(),
                extra: [o.first_bone as f32, o.material.0 as f32, 0.0, 0.0],
            };
            // mit Knochen ist die Ausdehnung nicht bekannt
            let sphere = (!skinned).then(|| model.bounds.transformed(&o.transform));
            let blend = matches!(m.alpha, Alpha::Blend) || o.tint.w < 0.999;
            let item = Item {
                key,
                obj: 0,
                sphere,
                shadow: o.shadow && !blend,
                blend,
            };
            self.push(obj, item);
        }
    }

    fn add_meshes(&mut self, f: &FrameInput, plain: &Plain) {
        let solid = f.meshes.iter().map(|m| (m, false));
        let ghosts = f.ghosts.iter().map(|m| (m, true));
        for (m, ghost) in solid.chain(ghosts) {
            let Some(bounds) = plain.mesh_bounds.get(m.mesh) else {
                continue;
            };
            let key = plain_key(Kind::Mesh, plain.material, m.mesh as u32);
            let item = Item {
                key,
                obj: 0,
                sphere: Some(bounds.transformed(&m.model)),
                shadow: m.shadow && !ghost,
                blend: ghost,
            };
            self.push(
                plain_obj(&m.model, m.tint, 0, m.blink, ghost, plain.material),
                item,
            );
        }
    }

    fn add_skins(&mut self, f: &FrameInput, plain: &Plain) {
        for s in f.skins {
            if plain.skin_bounds.is_empty() {
                return;
            }
            let model = s.skin % plain.skin_bounds.len();
            let key = plain_key(Kind::Skin, plain.material, model as u32);
            let item = Item {
                key,
                obj: 0,
                sphere: super::super::lists::skin_sphere(s, plain.skin_bounds, f.bones),
                shadow: !s.ghost,
                blend: s.ghost,
            };
            let obj = plain_obj(
                &s.model,
                s.tint,
                s.first_bone,
                s.blink,
                s.ghost,
                plain.material,
            );
            self.push(obj, item);
        }
    }

    /// Deckende Objekte einer Sicht; liefert ihre Zeichenbefehle.
    pub fn view(&mut self, view_proj: &Mat4, shadow: bool) -> Range<usize> {
        let frustum = Frustum::new(view_proj);
        self.gather(shadow, |s| frustum.sees(s))
    }

    /// Wie [`Lists::view`] fuer eine Lampe: zusaetzlich nur, was in ihrer Reichweite liegt.
    pub fn lamp_view(&mut self, view_proj: &Mat4, pos: Vec3, range: f32) -> Range<usize> {
        let frustum = Frustum::new(view_proj);
        self.gather(true, |s| {
            let reach = range + s.radius;
            s.center.distance_squared(pos) < reach * reach && frustum.sees(s)
        })
    }

    fn gather(&mut self, shadow: bool, sees: impl Fn(&Sphere) -> bool) -> Range<usize> {
        let start = self.batches.len();
        let mut last: Option<Key> = None;
        for i in &self.items {
            if i.blend || (shadow && !i.shadow) {
                continue;
            }
            let visible = i.sphere.as_ref().is_none_or(&sees);
            if !shadow {
                if visible {
                    self.drawn += 1;
                } else {
                    self.culled += 1;
                }
            }
            if !visible {
                continue;
            }
            self.order.push(i.obj);
            match self.batches.last_mut() {
                Some(b) if last == Some(i.key) => b.count += 1,
                _ => self.batches.push(batch(i.key, self.order.len() as u32 - 1)),
            }
            last = Some(i.key);
        }
        start..self.batches.len()
    }

    /// Halbtransparentes von hinten nach vorn.
    pub fn blended(&mut self, view_proj: &Mat4, eye: Vec3) -> Range<usize> {
        let frustum = Frustum::new(view_proj);
        let start = self.batches.len();
        self.blended.clear();
        for (k, i) in self.items.iter().enumerate() {
            if !i.blend || !i.sphere.as_ref().is_none_or(|s| frustum.sees(s)) {
                continue;
            }
            let at = Vec4::from_array(self.objs[i.obj as usize].model[3]).truncate();
            let at = i.sphere.map_or(at, |s| s.center);
            self.blended.push((at.distance_squared(eye), k as u32));
        }
        self.blended.sort_by(|a, b| b.0.total_cmp(&a.0));
        for &(_, k) in &self.blended {
            let i = &self.items[k as usize];
            self.order.push(i.obj);
            self.batches.push(batch(i.key, self.order.len() as u32 - 1));
        }
        self.drawn += self.blended.len() as u32;
        start..self.batches.len()
    }
}

fn batch(k: Key, first: u32) -> Batch {
    Batch {
        kind: k.kind,
        masked: k.masked,
        double_sided: k.double_sided,
        material: k.material,
        model: k.model,
        first,
        count: 1,
    }
}

fn plain_key(kind: Kind, material: u32, model: u32) -> Key {
    Key {
        kind,
        masked: false,
        double_sided: true,
        material,
        model,
    }
}

/// Einfaches Modell: Farbe in Anzeigewerten, `tint.w` = Eigenleuchten.
fn plain_obj(
    model: &Mat4,
    tint: Vec4,
    first_bone: u32,
    blink: f32,
    ghost: bool,
    material: u32,
) -> GpuObj {
    let c = |v: f32| v.max(0.0).powf(2.2);
    let alpha = if ghost { GHOST_ALPHA } else { 1.0 };
    let glow = (tint.w + blink) * PLAIN_GLOW;
    GpuObj {
        model: model.to_cols_array_2d(),
        prev: model.to_cols_array_2d(),
        tint: [c(tint.x), c(tint.y), c(tint.z), alpha],
        extra: [first_bone as f32, material as f32, 0.0, glow],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(material: u32, x: f32, blend: bool) -> Item {
        Item {
            key: plain_key(Kind::Object, material, 0),
            obj: 0,
            sphere: Some(Sphere {
                center: Vec3::new(x, 0.0, -10.0),
                radius: 0.5,
            }),
            shadow: true,
            blend,
        }
    }

    fn lists(items: &[Item]) -> Lists {
        let mut l = Lists::default();
        for i in items {
            let m = Mat4::from_translation(i.sphere.unwrap().center);
            l.push(plain_obj(&m, Vec4::ONE, 0, 0.0, false, 0), *i);
        }
        l.items.sort_unstable_by_key(|i| (i.blend, i.key, i.obj));
        l
    }

    fn camera() -> Mat4 {
        crate::render::perspective(1.0, 1.0, 0.1, 100.0)
    }

    #[test]
    fn equal_objects_share_a_draw_and_hidden_ones_are_dropped() {
        let mut l = lists(&[
            item(1, 0.0, false),
            item(0, 1.0, false),
            item(1, -1.0, false),
            item(1, 500.0, false),
        ]);
        let r = l.view(&camera(), false);
        let b = &l.batches[r];
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].material, b[0].count), (0, 1));
        assert_eq!((b[1].material, b[1].first, b[1].count), (1, 1, 2));
        assert_eq!(l.order, vec![1, 0, 2]);
        assert_eq!((l.drawn, l.culled), (3, 1));
    }

    #[test]
    fn transparent_things_are_sorted_back_to_front_and_cast_no_shadow() {
        let mut far = item(0, 0.0, true);
        far.sphere = Some(Sphere {
            center: Vec3::new(0.0, 0.0, -30.0),
            radius: 0.5,
        });
        let mut l = lists(&[item(0, 0.0, true), far]);
        assert!(l.view(&camera(), true).is_empty());
        let r = l.blended(&camera(), Vec3::ZERO);
        assert_eq!(r.len(), 2);
        assert_eq!(l.order, vec![1, 0]);
    }
}
