//! Objekte mit Lage und Hierarchie: ein Kind bewegt sich mit seinem Elternteil. Eigene Daten
//! (`T`) haengen an jedem Knoten. Wer viele gleichartige Dinge mit vielen Eigenschaften hat,
//! nimmt stattdessen `ecs` (Feature `ecs`).

use glam::{Mat4, Quat, Vec3};

/// Lage: Verschiebung, Drehung, Groesse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::at(Vec3::ZERO)
    }
}

impl Transform {
    pub fn at(pos: Vec3) -> Self {
        Self {
            pos,
            rot: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// Gedreht um die Hochachse.
    pub fn yaw(mut self, angle: f32) -> Self {
        self.rot = Quat::from_rotation_y(angle);
        self
    }

    pub fn scaled(mut self, s: f32) -> Self {
        self.scale = Vec3::splat(s);
        self
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rot, self.pos)
    }

    /// Zwischenstand (z. B. mit `FixedStep::alpha`).
    pub fn lerp(&self, to: &Self, t: f32) -> Self {
        Self {
            pos: self.pos.lerp(to.pos, t),
            rot: self.rot.slerp(to.rot, t),
            scale: self.scale.lerp(to.scale, t),
        }
    }

    /// Blickrichtung (-z).
    pub fn forward(&self) -> Vec3 {
        self.rot * Vec3::NEG_Z
    }
}

/// Kennung eines Knotens. Nach dem Entfernen zeigt sie ins Leere, auch wenn der Platz neu
/// vergeben wird.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId {
    index: u32,
    age: u32,
}

struct Node<T> {
    local: Transform,
    global: Mat4,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    data: T,
}

struct Slot<T> {
    age: u32,
    node: Option<Node<T>>,
}

/// Alle Knoten.
pub struct Scene<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
}

impl<T> Default for Scene<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> Scene<T> {
    pub fn new() -> Self {
        Self::default()
    }

    fn node(&self, id: NodeId) -> Option<&Node<T>> {
        let s = self.slots.get(id.index as usize)?;
        (s.age == id.age).then_some(s.node.as_ref()).flatten()
    }

    fn node_mut(&mut self, id: NodeId) -> Option<&mut Node<T>> {
        let s = self.slots.get_mut(id.index as usize)?;
        (s.age == id.age).then_some(s.node.as_mut()).flatten()
    }

    /// Knoten anlegen. Ein unbekannter Elternteil zaehlt als keiner.
    pub fn add(&mut self, parent: Option<NodeId>, local: Transform, data: T) -> NodeId {
        let parent = parent.filter(|p| self.node(*p).is_some());
        let index = match self.free.pop() {
            Some(i) => i,
            None => {
                self.slots.push(Slot { age: 0, node: None });
                self.slots.len() as u32 - 1
            }
        };
        let slot = &mut self.slots[index as usize];
        let id = NodeId {
            index,
            age: slot.age,
        };
        slot.node = Some(Node {
            local,
            global: local.matrix(),
            parent,
            children: Vec::new(),
            data,
        });
        if let Some(p) = parent.and_then(|p| self.node_mut(p)) {
            p.children.push(id);
            let above = p.global;
            if let Some(n) = self.node_mut(id) {
                n.global = above * local.matrix();
            }
        }
        id
    }

    /// Knoten samt Kindern entfernen; liefert die Daten aller entfernten.
    pub fn remove(&mut self, id: NodeId) -> Vec<T> {
        let Some(parent) = self.node(id).map(|n| n.parent) else {
            return Vec::new();
        };
        if let Some(p) = parent.and_then(|p| self.node_mut(p)) {
            p.children.retain(|c| *c != id);
        }
        let mut out = Vec::new();
        let mut open = vec![id];
        while let Some(i) = open.pop() {
            let slot = &mut self.slots[i.index as usize];
            if let Some(n) = slot.node.take() {
                slot.age = slot.age.wrapping_add(1);
                self.free.push(i.index);
                open.extend(n.children);
                out.push(n.data);
            }
        }
        out
    }

    /// Anderen Elternteil geben. `false` = unbekannt oder es entstuende ein Kreis.
    pub fn set_parent(&mut self, id: NodeId, parent: Option<NodeId>) -> bool {
        let mut up = parent;
        while let Some(p) = up {
            if p == id {
                return false;
            }
            match self.node(p) {
                Some(n) => up = n.parent,
                None => return false,
            }
        }
        let Some(old) = self.node(id).map(|n| n.parent) else {
            return false;
        };
        if let Some(p) = old.and_then(|p| self.node_mut(p)) {
            p.children.retain(|c| *c != id);
        }
        if let Some(p) = parent.and_then(|p| self.node_mut(p)) {
            p.children.push(id);
        }
        if let Some(n) = self.node_mut(id) {
            n.parent = parent;
        }
        true
    }

    pub fn get(&self, id: NodeId) -> Option<&T> {
        self.node(id).map(|n| &n.data)
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut T> {
        self.node_mut(id).map(|n| &mut n.data)
    }

    /// Lage relativ zum Elternteil.
    pub fn local(&self, id: NodeId) -> Option<Transform> {
        self.node(id).map(|n| n.local)
    }

    pub fn set_local(&mut self, id: NodeId, t: Transform) {
        if let Some(n) = self.node_mut(id) {
            n.local = t;
        }
    }

    /// Lage in der Welt, Stand des letzten [`Scene::update`].
    pub fn global(&self, id: NodeId) -> Option<Mat4> {
        self.node(id).map(|n| n.global)
    }

    /// Weltlagen neu berechnen (einmal pro Bild, vor dem Zeichnen).
    pub fn update(&mut self) {
        let mut open: Vec<(NodeId, Mat4)> = self
            .iter_ids()
            .filter(|id| self.node(*id).is_some_and(|n| n.parent.is_none()))
            .map(|id| (id, Mat4::IDENTITY))
            .collect();
        while let Some((id, above)) = open.pop() {
            if let Some(n) = self.node_mut(id) {
                n.global = above * n.local.matrix();
                let here = n.global;
                open.extend(n.children.iter().map(|c| (*c, here)));
            }
        }
    }

    fn iter_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            s.node.as_ref().map(|_| NodeId {
                index: i as u32,
                age: s.age,
            })
        })
    }

    /// Alle Knoten mit Weltlage.
    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &Mat4, &T)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            let n = s.node.as_ref()?;
            let id = NodeId {
                index: i as u32,
                age: s.age,
            };
            Some((id, &n.global, &n.data))
        })
    }

    pub fn len(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_follow_their_parent() {
        let mut s = Scene::new();
        let car = s.add(None, Transform::at(Vec3::X * 5.0), "Auto");
        let wheel = s.add(Some(car), Transform::at(Vec3::Z), "Rad");
        s.set_local(car, Transform::at(Vec3::Y).yaw(std::f32::consts::FRAC_PI_2));
        s.update();
        let at = s.global(wheel).unwrap().w_axis.truncate();
        assert!((at - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-5, "{at}");
        assert!(!s.set_parent(car, Some(wheel)), "kein Kreis");
        assert!(s.set_parent(wheel, None));
        s.update();
        assert!((s.global(wheel).unwrap().w_axis.truncate() - Vec3::Z).length() < 1e-5);
    }

    #[test]
    fn removed_ids_stay_invalid_when_their_slot_is_reused() {
        let mut s = Scene::new();
        let a = s.add(None, Transform::default(), 1);
        let b = s.add(Some(a), Transform::default(), 2);
        assert_eq!(s.remove(a).len(), 2);
        assert!(s.get(a).is_none() && s.get(b).is_none() && s.is_empty());
        let c = s.add(Some(a), Transform::default(), 3);
        assert_eq!((s.get(c), s.get(a), s.len()), (Some(&3), None, 1));
        assert_eq!(s.iter().count(), 1);
        assert!(s.remove(a).is_empty());
    }
}
