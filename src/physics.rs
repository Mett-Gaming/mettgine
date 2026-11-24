//! Physik (rapier3d): fallende und rollende Koerper, feste Welt, Ausloeser, Strahlen und eine
//! Figurensteuerung. Feature `physics`. Wer nur Kisten und Kapseln braucht, bleibt bei
//! [`crate::collide`].
//!
//! Im festen Takt aufrufen: [`Physics::step`] mit immer gleicher Schrittweite.

use crate::scene::Transform;
use glam::{Quat, Vec3};
use rapier3d::prelude as r;
use std::sync::mpsc::{channel, Receiver};

pub use rapier3d;

/// Kennung eines Koerpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyId(r::RigidBodyHandle);

/// Form eines Koerpers, um seinen Mittelpunkt.
#[derive(Clone, Debug)]
pub enum Shape {
    Ball(f32),
    /// halbe Kanten
    Box(Vec3),
    /// halbe Hoehe des geraden Teils, Radius
    Capsule(f32, f32),
    /// halbe Hoehe, Radius
    Cylinder(f32, f32),
    /// Huelle um Punkte
    Hull(Vec<Vec3>),
    /// Dreiecke (nur fuer Feststehendes)
    Mesh(Vec<Vec3>, Vec<[u32; 3]>),
}

/// Verhalten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// unbeweglich
    Fixed,
    /// wird vom Spiel bewegt und schiebt andere (Aufzug, Tuer)
    Moved,
    /// faellt, rollt, prallt ab
    Free,
}

/// Beschreibung eines neuen Koerpers.
#[derive(Clone, Debug)]
pub struct BodyDesc {
    pub shape: Shape,
    pub motion: Motion,
    pub transform: Transform,
    pub friction: f32,
    /// 0 = bleibt liegen, 1 = prallt voll ab
    pub bounce: f32,
    /// kg je Kubikmeter
    pub density: f32,
    /// nur melden, nicht aufhalten (Ausloeser)
    pub sensor: bool,
    /// schnelle kleine Koerper: nicht durch Waende schiessen
    pub fast: bool,
    /// bleibt aufrecht
    pub upright: bool,
    /// Gruppen, zu denen der Koerper gehoert (Bits) und mit denen er zusammenstoesst
    pub groups: (u32, u32),
    /// frei fuer das Spiel
    pub user: u64,
}

impl BodyDesc {
    pub fn new(shape: Shape, motion: Motion, transform: Transform) -> Self {
        Self {
            shape,
            motion,
            transform,
            friction: 0.6,
            bounce: 0.0,
            density: 500.0,
            sensor: false,
            fast: false,
            upright: false,
            groups: (u32::MAX, u32::MAX),
            user: 0,
        }
    }
}

/// Beruehrung, die begonnen oder geendet hat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Touch {
    pub a: BodyId,
    pub b: BodyId,
    pub started: bool,
    /// einer von beiden ist ein Ausloeser
    pub sensor: bool,
}

/// Treffer eines Strahls.
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub body: BodyId,
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
}

/// Ergebnis einer Figurenbewegung.
#[derive(Clone, Copy, Debug)]
pub struct CharacterMove {
    /// neue Lage des Kapsel-Mittelpunkts
    pub pos: Vec3,
    pub grounded: bool,
    /// rutscht einen zu steilen Hang hinunter
    pub sliding: bool,
}

/// Figur: Kapsel, die Stufen steigt und an Waenden entlanggleitet.
#[derive(Clone, Copy, Debug)]
pub struct Character {
    pub radius: f32,
    /// Gesamthoehe
    pub height: f32,
    /// so hohe Stufen werden gestiegen (Meter)
    pub step: f32,
    /// so steile Haenge (Radiant)
    pub slope: f32,
    /// schiebt freie Koerper mit diesem Gewicht (kg); 0 = gar nicht
    pub push_mass: f32,
}

impl Default for Character {
    fn default() -> Self {
        Self {
            radius: 0.3,
            height: 1.8,
            step: 0.35,
            slope: 0.8,
            push_mass: 80.0,
        }
    }
}

fn v(p: Vec3) -> r::Vector {
    r::Vector::new(p.x, p.y, p.z)
}

fn g(p: r::Vector) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

fn pose(t: &Transform) -> r::Pose {
    let q = t.rot;
    r::Pose::from_parts(v(t.pos), r::Rotation::from_xyzw(q.x, q.y, q.z, q.w))
}

/// Alle Koerper.
pub struct Physics {
    world: r::PhysicsWorld,
    events: r::ChannelEventCollector,
    incoming: Receiver<r::CollisionEvent>,
    // muessen leben, solange `events` sendet
    _forces: Receiver<r::ContactForceEvent>,
    _tears: Receiver<r::SoftBodyTearEvent>,
    touches: Vec<Touch>,
}

impl Default for Physics {
    fn default() -> Self {
        Self::new(Vec3::Y * -9.81)
    }
}

impl Physics {
    pub fn new(gravity: Vec3) -> Self {
        let (tx, incoming) = channel();
        let (forces_tx, forces) = channel();
        let (tears_tx, tears) = channel();
        let mut world = r::PhysicsWorld::new();
        world.gravity = v(gravity);
        Self {
            world,
            events: r::ChannelEventCollector::new(tx, forces_tx, tears_tx),
            incoming,
            _forces: forces,
            _tears: tears,
            touches: Vec::new(),
        }
    }

    /// Die rapier-Welt fuer alles, was hier fehlt (Gelenke, Motoren …).
    pub fn raw(&mut self) -> &mut r::PhysicsWorld {
        &mut self.world
    }

    /// Koerper hinzufuegen. Fehler bei unbrauchbarer Form.
    pub fn add(&mut self, d: &BodyDesc) -> Result<BodyId, String> {
        let c = match &d.shape {
            Shape::Ball(radius) => r::ColliderBuilder::ball(*radius),
            Shape::Box(h) => r::ColliderBuilder::cuboid(h.x, h.y, h.z),
            Shape::Capsule(half, radius) => r::ColliderBuilder::capsule_y(*half, *radius),
            Shape::Cylinder(half, radius) => r::ColliderBuilder::cylinder(*half, *radius),
            Shape::Hull(points) => {
                let p: Vec<_> = points.iter().map(|p| v(*p)).collect();
                r::ColliderBuilder::convex_hull(&p).ok_or("Huelle hat kein Volumen")?
            }
            Shape::Mesh(points, triangles) => {
                if d.motion == Motion::Free {
                    return Err("Dreiecksnetze koennen nicht frei fallen".into());
                }
                let p = points.iter().map(|p| v(*p)).collect();
                r::ColliderBuilder::trimesh(p, triangles.clone()).map_err(|e| e.to_string())?
            }
        };
        let groups = r::InteractionGroups::new(
            r::Group::from_bits_truncate(d.groups.0),
            r::Group::from_bits_truncate(d.groups.1),
            r::InteractionTestMode::And,
        );
        let c = c
            .friction(d.friction)
            .restitution(d.bounce)
            .density(d.density)
            .sensor(d.sensor)
            .collision_groups(groups)
            .active_events(r::ActiveEvents::COLLISION_EVENTS)
            // Ausloeser sollen auch Feststehendes und Bewegtes melden
            .active_collision_types(r::ActiveCollisionTypes::all());
        let mut b = match d.motion {
            Motion::Fixed => r::RigidBodyBuilder::fixed(),
            Motion::Moved => r::RigidBodyBuilder::kinematic_position_based(),
            Motion::Free => r::RigidBodyBuilder::dynamic(),
        }
        .pose(pose(&d.transform))
        .ccd_enabled(d.fast)
        .user_data(d.user as u128);
        if d.upright {
            b = b.lock_rotations();
        }
        Ok(BodyId(self.world.insert(b, c).0))
    }

    pub fn remove(&mut self, id: BodyId) {
        self.world.remove_body(id.0);
    }

    /// Ein Schritt von `dt` Sekunden (immer gleich). Danach [`Physics::touches`] lesen.
    pub fn step(&mut self, dt: f32) {
        self.world.integration_parameters.dt = dt;
        self.world.step_with_events(&(), &self.events);
        self.touches.clear();
        while let Ok(e) = self.incoming.try_recv() {
            let body = |c| {
                let parent = self.world.colliders.get(c)?.parent()?;
                Some(BodyId(parent))
            };
            let (Some(a), Some(b)) = (body(e.collider1()), body(e.collider2())) else {
                continue; // schon entfernt
            };
            self.touches.push(Touch {
                a,
                b,
                started: e.started(),
                sensor: e.sensor(),
            });
        }
    }

    /// Beruehrungen des letzten Schritts.
    pub fn touches(&self) -> &[Touch] {
        &self.touches
    }

    pub fn transform(&self, id: BodyId) -> Option<Transform> {
        let b = self.world.bodies.get(id.0)?;
        let q = b.rotation();
        Some(Transform {
            pos: g(b.translation()),
            rot: Quat::from_xyzw(q.x, q.y, q.z, q.w),
            scale: Vec3::ONE,
        })
    }

    /// Freier Koerper: versetzen. Bewegter: im naechsten Schritt dorthin fahren.
    pub fn set_transform(&mut self, id: BodyId, t: &Transform) {
        if let Some(b) = self.world.bodies.get_mut(id.0) {
            if b.is_kinematic() {
                b.set_next_kinematic_position(pose(t));
            } else {
                b.set_position(pose(t), true);
            }
        }
    }

    pub fn velocity(&self, id: BodyId) -> Vec3 {
        let b = self.world.bodies.get(id.0);
        b.map_or(Vec3::ZERO, |b| g(b.linvel()))
    }

    pub fn set_velocity(&mut self, id: BodyId, velocity: Vec3) {
        if let Some(b) = self.world.bodies.get_mut(id.0) {
            b.set_linvel(v(velocity), true);
        }
    }

    /// Stoss in Newtonsekunden auf den Schwerpunkt.
    pub fn push(&mut self, id: BodyId, impulse: Vec3) {
        if let Some(b) = self.world.bodies.get_mut(id.0) {
            b.apply_impulse(v(impulse), true);
        }
    }

    /// Stoss an einem Weltpunkt (dreht den Koerper).
    pub fn push_at(&mut self, id: BodyId, impulse: Vec3, point: Vec3) {
        if let Some(b) = self.world.bodies.get_mut(id.0) {
            b.apply_impulse_at_point(v(impulse), v(point), true);
        }
    }

    /// Wert, den das Spiel dem Koerper gegeben hat.
    pub fn user(&self, id: BodyId) -> u64 {
        let b = self.world.bodies.get(id.0);
        b.map_or(0, |b| b.user_data as u64)
    }

    fn filter(skip: Option<BodyId>) -> r::QueryFilter<'static> {
        let f = r::QueryFilter::default().exclude_sensors();
        match skip {
            Some(b) => f.exclude_rigid_body(b.0),
            None => f,
        }
    }

    /// Naechster Treffer eines Strahls (Stand des letzten Schritts). `dir` muss nicht normiert
    /// sein; `skip` wird uebergangen.
    pub fn raycast(&self, from: Vec3, dir: Vec3, max: f32, skip: Option<BodyId>) -> Option<RayHit> {
        let dir = dir.try_normalize()?;
        let w = &self.world;
        let dispatcher = w.narrow_phase.query_dispatcher();
        let q = w.broad_phase.as_query_pipeline(
            dispatcher,
            &w.bodies,
            &w.colliders,
            Self::filter(skip),
        );
        let ray = r::Ray::new(v(from), v(dir));
        let (c, hit) = q.cast_ray_and_get_normal(&ray, max, true)?;
        Some(RayHit {
            body: BodyId(w.colliders.get(c)?.parent()?),
            distance: hit.time_of_impact,
            point: from + dir * hit.time_of_impact,
            normal: g(hit.normal),
        })
    }

    /// Alle Koerper, die eine Kugel beruehren (Explosion, Reichweite).
    pub fn in_sphere(&self, center: Vec3, radius: f32) -> Vec<BodyId> {
        let w = &self.world;
        let dispatcher = w.narrow_phase.query_dispatcher();
        let filter = Self::filter(None);
        let q = w
            .broad_phase
            .as_query_pipeline(dispatcher, &w.bodies, &w.colliders, filter);
        let ball = r::Ball::new(radius);
        let at = r::Pose::from_parts(v(center), r::Rotation::IDENTITY);
        let mut out: Vec<BodyId> = q
            .intersect_shape(at, &ball)
            .filter_map(|(_, c)| c.parent().map(BodyId))
            .collect();
        out.dedup();
        out
    }

    /// Figur (Kapsel-Mittelpunkt `pos`) um `delta` bewegen. `body` ist ihr eigener Koerper, falls
    /// es einen gibt. Die Schwerkraft rechnet das Spiel in `delta` ein.
    pub fn move_character(
        &mut self,
        c: &Character,
        pos: Vec3,
        delta: Vec3,
        dt: f32,
        body: Option<BodyId>,
    ) -> CharacterMove {
        use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
        let ctl = KinematicCharacterController {
            offset: CharacterLength::Absolute(0.02),
            autostep: (c.step > 0.0).then_some(CharacterAutostep {
                max_height: CharacterLength::Absolute(c.step),
                min_width: CharacterLength::Absolute(c.radius),
                include_dynamic_bodies: false,
            }),
            max_slope_climb_angle: c.slope,
            min_slope_slide_angle: c.slope,
            // beim Springen nicht am Boden kleben
            snap_to_ground: (delta.y <= 0.0).then_some(CharacterLength::Absolute(0.15)),
            ..Default::default()
        };
        let half = (c.height * 0.5 - c.radius).max(0.01);
        let shape = r::Capsule::new_y(half, c.radius);
        let at = r::Pose::from_parts(v(pos), r::Rotation::IDENTITY);
        let w = &mut self.world;
        let filter = Self::filter(body);
        let mut hits = Vec::new();
        let m = {
            let q = w.broad_phase.as_query_pipeline(
                w.narrow_phase.query_dispatcher(),
                &w.bodies,
                &w.colliders,
                filter,
            );
            ctl.move_shape(dt, &q, &shape, &at, v(delta), |h| hits.push(h))
        };
        if c.push_mass > 0.0 && !hits.is_empty() {
            let mut q = w.broad_phase.as_query_pipeline_mut(
                w.narrow_phase.query_dispatcher(),
                &mut w.bodies,
                &mut w.colliders,
                filter,
            );
            ctl.solve_character_collision_impulses(dt, &mut q, &shape, c.push_mass, &hits);
        }
        CharacterMove {
            pos: pos + g(m.translation),
            grounded: m.grounded,
            sliding: m.is_sliding_down_slope,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn world() -> Physics {
        let mut p = Physics::default();
        let floor = Shape::Box(Vec3::new(20.0, 0.5, 20.0));
        let at = Transform::at(Vec3::Y * -0.5);
        p.add(&BodyDesc::new(floor, Motion::Fixed, at)).unwrap();
        p
    }

    #[test]
    fn things_fall_land_and_report_it() {
        let mut p = world();
        let ball = BodyDesc {
            user: 42,
            ..BodyDesc::new(Shape::Ball(0.5), Motion::Free, Transform::at(Vec3::Y * 3.0))
        };
        let b = p.add(&ball).unwrap();
        let mut landed = 0;
        for _ in 0..180 {
            p.step(DT);
            landed += p.touches().iter().filter(|t| t.started).count();
        }
        let y = p.transform(b).unwrap().pos.y;
        assert!((y - 0.5).abs() < 0.03, "{y}");
        assert!(landed >= 1);
        assert_eq!(p.user(b), 42);
        let hit = p.raycast(Vec3::Y * 5.0, Vec3::NEG_Y, 10.0, None).unwrap();
        assert!(hit.body == b && (hit.distance - 4.0).abs() < 0.05 && hit.normal.y > 0.9);
        let below = p
            .raycast(Vec3::Y * 5.0, Vec3::NEG_Y, 10.0, Some(b))
            .unwrap();
        assert!(below.body != b);
        assert!(p.in_sphere(Vec3::Y * 0.5, 0.2).contains(&b));
        p.push(b, Vec3::X * 2000.0);
        p.step(DT);
        assert!(p.velocity(b).x > 0.5);
        p.remove(b);
        assert!(p.transform(b).is_none());
    }

    #[test]
    fn characters_climb_steps_stop_at_walls_and_trigger_sensors() {
        let mut p = world();
        let fixed = |p: &mut Physics, half: Vec3, at: Vec3| {
            p.add(&BodyDesc::new(
                Shape::Box(half),
                Motion::Fixed,
                Transform::at(at),
            ))
            .unwrap()
        };
        fixed(&mut p, Vec3::new(1.0, 0.1, 2.0), Vec3::new(3.0, 0.1, 0.0)); // Stufe 0,2 m
        fixed(&mut p, Vec3::new(0.2, 2.0, 2.0), Vec3::new(5.0, 2.0, 0.0)); // Wand
        p.step(DT);
        let c = Character::default();
        let mut pos = Vec3::new(0.0, 0.95, 0.0);
        let mut highest = 0.0f32;
        for _ in 0..300 {
            let m = p.move_character(&c, pos, Vec3::new(0.05, -0.05, 0.0), DT, None);
            pos = m.pos;
            highest = highest.max(pos.y);
            p.step(DT);
        }
        assert!(highest > 1.05, "Stufe nicht gestiegen: {highest}");
        assert!(pos.x > 4.2 && pos.x < 4.6, "an der Wand: {pos}");
        let sensor = BodyDesc {
            sensor: true,
            ..BodyDesc::new(
                Shape::Box(Vec3::ONE),
                Motion::Fixed,
                Transform::at(Vec3::new(-5.0, 1.0, 0.0)),
            )
        };
        let s = p.add(&sensor).unwrap();
        let drop = Transform::at(Vec3::new(-5.0, 4.0, 0.0));
        let ball = BodyDesc::new(Shape::Ball(0.2), Motion::Free, drop);
        p.add(&ball).unwrap();
        let mut seen = false;
        for _ in 0..120 {
            p.step(DT);
            let hit = |t: &Touch| t.sensor && t.started && (t.a == s || t.b == s);
            seen |= p.touches().iter().any(hit);
        }
        assert!(seen);
    }

    #[test]
    fn useless_shapes_are_errors() {
        let mut p = world();
        let flat = Shape::Hull(vec![Vec3::ZERO]);
        let d = BodyDesc::new(flat, Motion::Free, Transform::default());
        assert!(p.add(&d).is_err());
        let mesh = Shape::Mesh(vec![Vec3::ZERO, Vec3::X, Vec3::Z], vec![[0, 1, 2]]);
        let d = BodyDesc::new(mesh, Motion::Free, Transform::default());
        assert!(p.add(&d).is_err());
    }
}
