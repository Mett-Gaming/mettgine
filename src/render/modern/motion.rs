//! Wo Dinge im letzten Bild waren (fuer TAA, Hochskalieren und Bilderzeugung).

use super::super::FrameInput;
use super::SceneInput;
use glam::Mat4;
use std::collections::HashMap;

/// Kennungen, an denen die Engine Dinge von Bild zu Bild wiedererkennt. Jede Liste laeuft parallel
/// zu ihren Zeichenaufrufen und darf kuerzer sein; 0 = keine Kennung (gilt als unbewegt). Innerhalb
/// einer Liste muss jede Kennung eindeutig sein.
#[derive(Clone, Copy, Debug, Default)]
pub struct MotionIds<'a> {
    /// zu `SceneInput::objects` (ein gesetztes `previous` hat Vorrang)
    pub objects: &'a [u32],
    /// zu `FrameInput::meshes`
    pub meshes: &'a [u32],
    /// zu `FrameInput::skins`
    pub skins: &'a [u32],
}

/// Welche Liste.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum List {
    Objects,
    Meshes,
    Skins,
}

/// Lage im letzten Bild.
#[derive(Clone, Copy, Debug)]
pub(super) struct Before {
    pub model: Mat4,
    /// Wert fuer `GpuObj::extra[2]`: 0 = keine alten Knochen, sonst erster alter Knochen + 1
    pub bones: f32,
}

/// Gedaechtnis ueber ein Bild.
#[derive(Default)]
pub(super) struct Motion {
    last: HashMap<(List, u32), (Mat4, u32)>,
    next: HashMap<(List, u32), (Mat4, u32)>,
    bones: Vec<[[f32; 4]; 4]>,
    /// Stelle der alten Knochen im Puffer dieses Bildes
    base: u32,
}

impl Motion {
    /// Knochen des letzten Bildes; gehoeren hinter die `base` Knochen dieses Bildes.
    pub fn bones(&mut self, base: usize) -> &[[[f32; 4]; 4]] {
        self.base = base as u32;
        &self.bones
    }

    /// Kamerasprung, Szenenwechsel.
    pub fn forget(&mut self) {
        self.last.clear();
        self.next.clear();
    }

    /// Ding dieses Bildes eintragen; liefert, wo es vorher war.
    pub fn track(&mut self, list: List, id: u32, model: Mat4, first_bone: u32) -> Option<Before> {
        if id == 0 {
            return None;
        }
        self.next.insert((list, id), (model, first_bone));
        let &(model, first) = self.last.get(&(list, id))?;
        let known = (first as usize) < self.bones.len();
        let bones = if known {
            (self.base + first + 1) as f32
        } else {
            0.0
        };
        Some(Before { model, bones })
    }

    /// Ende des Bildes.
    pub fn finish(&mut self, f: &FrameInput, s: &SceneInput) {
        std::mem::swap(&mut self.last, &mut self.next);
        self.next.clear();
        self.bones.clear();
        if !s.ids.skins.is_empty() || !s.ids.objects.is_empty() {
            self.bones
                .extend(f.bones.iter().map(|m| m.to_cols_array_2d()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Camera;
    use glam::Vec3;

    #[test]
    fn remembers_one_frame_by_id() {
        let mut m = Motion::default();
        let bones = [Mat4::IDENTITY; 3];
        let f = FrameInput {
            bones: &bones,
            ..Default::default()
        };
        let camera = Camera::looking(Vec3::ZERO, Vec3::NEG_Z, 1.0);
        let s = SceneInput {
            ids: MotionIds {
                skins: &[7],
                ..Default::default()
            },
            ..SceneInput::new(camera)
        };
        let a = Mat4::from_translation(Vec3::X);
        assert!(m.track(List::Skins, 7, a, 1).is_none());
        assert!(m.track(List::Skins, 0, a, 0).is_none());
        m.finish(&f, &s);
        assert_eq!(m.bones(10).len(), 3);
        let b = m.track(List::Skins, 7, Mat4::IDENTITY, 2).unwrap();
        assert_eq!((b.model, b.bones), (a, 12.0));
        assert!(m.track(List::Meshes, 7, a, 0).is_none(), "andere Liste");
        m.forget();
        assert!(m.track(List::Skins, 7, a, 1).is_none());
    }
}
