//! Skelett und Animationen einer glTF-Datei als [`AnimSet`]: Clips werden mit fester Bildrate
//! abgetastet.

use crate::anim::{AnimSet, ClipData, Local};
use glam::{Mat4, Quat, Vec3};
use gltf::animation::util::ReadOutputs;
use std::collections::HashMap;

/// Bilder pro Sekunde der abgetasteten Clips
const FPS: f32 = 30.0;
/// Laengste Animation (Bilder), gegen kaputte Dateien
const MAX_FRAMES: usize = 30 * 600;

/// Skelett einer Haut, Eltern vor ihren Kindern.
pub(super) struct Rig {
    /// Knoten-Index je Knochen
    nodes: Vec<usize>,
    names: Vec<String>,
    parents: Vec<i32>,
    inverse_bind: Vec<Mat4>,
    /// Ruhelage relativ zum Eltern-Knochen
    rest: Vec<Local>,
    /// Lage der Knoten ueber einem Knochen, die selbst keine Knochen sind
    above: Vec<Mat4>,
    /// Haut-Nummer -> Knochen
    joint_map: Vec<u16>,
}

fn local(node: &gltf::Node) -> Local {
    let (t, r, s) = node.transform().decomposed();
    Local {
        rot: Quat::from_array(r).normalize(),
        pos: Vec3::from(t),
        scale: Vec3::from(s),
    }
}

impl Rig {
    pub fn new(
        doc: &gltf::Document,
        skin: &gltf::Skin,
        buffers: &[gltf::buffer::Data],
    ) -> Result<Self, String> {
        let parent: HashMap<usize, usize> = doc
            .nodes()
            .flat_map(|n| {
                let i = n.index();
                n.children()
                    .map(move |c| (c.index(), i))
                    .collect::<Vec<_>>()
            })
            .collect();
        let nodes: HashMap<usize, gltf::Node> = doc.nodes().map(|n| (n.index(), n)).collect();
        let joints: Vec<usize> = skin.joints().map(|j| j.index()).collect();
        let reader = skin.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
        let binds: Vec<Mat4> = match reader.read_inverse_bind_matrices() {
            Some(m) => m.map(|m| Mat4::from_cols_array_2d(&m)).collect(),
            None => vec![Mat4::IDENTITY; joints.len()],
        };
        if binds.len() < joints.len() {
            return Err("Skelett: Bindematrizen fehlen".into());
        }
        let depth = |mut n: usize| {
            let mut d = 0;
            while let Some(&p) = parent.get(&n) {
                (n, d) = (p, d + 1);
                if d > 10_000 {
                    break; // Kreis in einer kaputten Datei
                }
            }
            d
        };
        let mut order: Vec<usize> = (0..joints.len()).collect();
        order.sort_by_key(|&k| depth(joints[k]));
        let place: HashMap<usize, usize> = order
            .iter()
            .enumerate()
            .map(|(bone, &k)| (joints[k], bone))
            .collect();
        let mut rig = Self {
            nodes: order.iter().map(|&k| joints[k]).collect(),
            names: Vec::new(),
            parents: Vec::new(),
            inverse_bind: order.iter().map(|&k| binds[k]).collect(),
            rest: Vec::new(),
            above: Vec::new(),
            joint_map: vec![0; joints.len()],
        };
        for (bone, &k) in order.iter().enumerate() {
            rig.joint_map[k] = bone as u16;
            let node = &nodes[&joints[k]];
            let fallback = format!("Knochen {bone}");
            rig.names.push(node.name().unwrap_or(&fallback).to_string());
            rig.rest.push(local(node));
            // naechster Knochen darueber; was dazwischen liegt, ist starr
            let (mut above, mut at) = (Mat4::IDENTITY, joints[k]);
            let mut up = -1;
            while let Some(&p) = parent.get(&at) {
                if let Some(&b) = place.get(&p) {
                    up = b as i32;
                    break;
                }
                above = Mat4::from_cols_array_2d(&nodes[&p].transform().matrix()) * above;
                at = p;
            }
            rig.parents.push(up);
            rig.above.push(above);
        }
        Ok(rig)
    }

    /// Haut-Nummer in Knochen-Nummer umrechnen.
    pub fn joint(&self, j: usize) -> u16 {
        self.joint_map.get(j).copied().unwrap_or(0)
    }

    /// Lokale Lage samt starrer Knoten darueber.
    fn combine(&self, bone: usize, l: Local) -> Local {
        if self.above[bone] == Mat4::IDENTITY {
            return l;
        }
        let m = self.above[bone] * Mat4::from_scale_rotation_translation(l.scale, l.rot, l.pos);
        let (scale, rot, pos) = m.to_scale_rotation_translation();
        Local { rot, pos, scale }
    }

    pub fn animations(
        self,
        doc: &gltf::Document,
        buffers: &[gltf::buffer::Data],
    ) -> Result<AnimSet, String> {
        let (mut clips, mut names) = (Vec::new(), Vec::new());
        for (i, a) in doc.animations().enumerate() {
            clips.push(self.clip(&a, buffers)?);
            names.push(a.name().map_or(format!("Clip {i}"), str::to_string));
        }
        Ok(AnimSet::new(
            self.names,
            self.parents,
            self.inverse_bind,
            clips,
            names,
        ))
    }

    fn clip(
        &self,
        a: &gltf::Animation,
        buffers: &[gltf::buffer::Data],
    ) -> Result<ClipData, String> {
        let mut tracks: Vec<Track> = Vec::new();
        for c in a.channels() {
            let node = c.target().node().index();
            let Some(bone) = self.nodes.iter().position(|n| *n == node) else {
                continue;
            };
            if let Some(t) = Track::read(&c, bone, buffers) {
                tracks.push(t);
            }
        }
        let end = tracks
            .iter()
            .filter_map(|t| t.times.last().copied())
            .filter(|t| t.is_finite())
            .fold(0.0f32, f32::max);
        // Das letzte Bild gleicht bei Schleifen dem ersten
        let frames = ((end * FPS).round() as usize).clamp(1, MAX_FRAMES);
        let mut locals = Vec::with_capacity(frames * self.nodes.len());
        for f in 0..frames {
            let mut pose = self.rest.clone();
            for t in &tracks {
                t.apply(f as f32 / FPS, &mut pose[t.bone]);
            }
            locals.extend((0..pose.len()).map(|b| self.combine(b, pose[b])));
        }
        Ok(ClipData {
            fps: FPS,
            frames,
            looped: true,
            locals,
        })
    }
}

enum Values {
    Pos(Vec<Vec3>),
    Rot(Vec<Quat>),
    Scale(Vec<Vec3>),
}

/// Verlauf einer Groesse eines Knochens.
struct Track {
    bone: usize,
    times: Vec<f32>,
    values: Values,
    step: bool,
}

impl Track {
    fn read(
        c: &gltf::animation::Channel,
        bone: usize,
        buffers: &[gltf::buffer::Data],
    ) -> Option<Self> {
        use gltf::animation::Interpolation as I;
        let r = c.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
        let times: Vec<f32> = r.read_inputs()?.collect();
        let mode = c.sampler().interpolation();
        // Kurven: (Tangente, Wert, Tangente) - nur die Werte, linear verbunden
        let cubic = mode == I::CubicSpline;
        fn pick<T>(it: impl Iterator<Item = T>, cubic: bool) -> Vec<T> {
            it.enumerate()
                .filter(|(i, _)| !cubic || i % 3 == 1)
                .map(|(_, v)| v)
                .collect()
        }
        let values = match r.read_outputs()? {
            ReadOutputs::Translations(t) => Values::Pos(pick(t.map(Vec3::from), cubic)),
            ReadOutputs::Scales(s) => Values::Scale(pick(s.map(Vec3::from), cubic)),
            ReadOutputs::Rotations(q) => {
                let q = q.into_f32().map(|q| Quat::from_array(q).normalize());
                Values::Rot(pick(q, cubic))
            }
            ReadOutputs::MorphTargetWeights(_) => return None,
        };
        let n = match &values {
            Values::Pos(v) | Values::Scale(v) => v.len(),
            Values::Rot(v) => v.len(),
        };
        (!times.is_empty() && n >= times.len()).then_some(Self {
            bone,
            times,
            values,
            step: mode == I::Step,
        })
    }

    /// Nachbarn und Mischung zur Zeit `t`.
    fn at(&self, t: f32) -> (usize, usize, f32) {
        let i = self.times.partition_point(|k| *k <= t);
        if i == 0 {
            return (0, 0, 0.0);
        }
        if i >= self.times.len() {
            return (i - 1, i - 1, 0.0);
        }
        let (a, b) = (self.times[i - 1], self.times[i]);
        let w = if self.step || b <= a {
            0.0
        } else {
            (t - a) / (b - a)
        };
        (i - 1, i, w)
    }

    fn apply(&self, t: f32, out: &mut Local) {
        let (a, b, w) = self.at(t);
        match &self.values {
            Values::Pos(v) => out.pos = v[a].lerp(v[b], w),
            Values::Scale(v) => out.scale = v[a].lerp(v[b], w),
            Values::Rot(v) => out.rot = v[a].slerp(v[b], w),
        }
    }
}
