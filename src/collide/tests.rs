//! Tests der Kollision: Strahlen, Waende, Ecken, Stufen, Raster.

use super::*;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn floor() -> World {
    let mut w = World::default();
    w.add(Solid::new(v(0.0, -0.5, 0.0), v(500.0, 0.5, 500.0), 0.0));
    w
}

fn man(x: f32, z: f32) -> Capsule {
    Capsule::new(v(x, 0.0, z), 0.3, 1.8)
}

#[test]
fn rays_hit_boxes_spheres_and_capsules() {
    let b = Aabb::centered(v(0.0, 0.0, 10.0), Vec3::ONE);
    let r = Ray::new(Vec3::ZERO, Vec3::Z);
    let h = b.raycast(&r, 100.0).unwrap();
    assert!((h.t - 9.0).abs() < 1e-5 && h.normal == Vec3::NEG_Z);
    assert!(b.raycast(&r, 8.0).is_none(), "zu kurz");
    assert!(b
        .raycast(&Ray::new(Vec3::ZERO, Vec3::NEG_Z), 100.0)
        .is_none());
    assert_eq!(
        b.raycast(&Ray::new(b.min + Vec3::ONE, Vec3::X), 9.0)
            .unwrap()
            .t,
        0.0
    );
    let h = ray_sphere(&r, v(0.0, 0.0, 5.0), 1.0, 100.0).unwrap();
    assert!((h.t - 4.0).abs() < 1e-5);
    let c = man(0.0, 6.0);
    let side = c
        .raycast(&Ray::new(v(0.0, 0.9, 0.0), Vec3::Z), 50.0)
        .unwrap();
    assert!((side.t - 5.7).abs() < 1e-4 && side.normal.z < -0.99);
    let top = c
        .raycast(&Ray::new(v(0.0, 9.0, 6.0), Vec3::NEG_Y), 50.0)
        .unwrap();
    assert!((top.t - 7.2).abs() < 1e-4, "{top:?}");
    assert!(c
        .raycast(&Ray::new(v(0.0, 2.5, 0.0), Vec3::Z), 50.0)
        .is_none());
}

#[test]
fn turned_boxes_are_hit_where_they_stand() {
    // lange Kante entlang z, um 90 Grad gedreht: jetzt entlang x
    let s = Solid::new(
        v(0.0, 0.0, 0.0),
        v(0.5, 1.0, 4.0),
        std::f32::consts::FRAC_PI_2,
    );
    assert!(s
        .raycast(&Ray::new(v(3.0, 0.0, -9.0), Vec3::Z), 50.0)
        .is_some());
    assert!(s
        .raycast(&Ray::new(v(0.0, 0.0, -9.0), Vec3::Z), 8.0)
        .is_none());
    let b = s.aabb();
    assert!((b.max.x - 4.0).abs() < 1e-4 && (b.max.z - 0.5).abs() < 1e-4);
    // gleiche Drehrichtung wie glam
    let q = glam::Quat::from_rotation_y(0.7);
    let t = Solid::new(Vec3::ZERO, Vec3::ONE, 0.7);
    assert!((t.dir_to_world(Vec3::X) - q * Vec3::X).length() < 1e-5);
}

#[test]
fn walls_stop_and_let_slide() {
    let mut w = floor();
    w.add(Solid::new(v(2.0, 1.5, 0.0), v(0.5, 1.5, 20.0), 0.0));
    let m = w.move_capsule(&man(0.0, 0.0), v(5.0, 0.0, 1.0), 0.0);
    assert!((m.foot.x - 1.2).abs() < 0.01, "vor der Wand: {m:?}");
    assert!(m.foot.z > 0.95, "gleitet weiter: {m:?}");
    assert!(m.wall.is_some_and(|n| n.x < -0.9));
    assert!(m.grounded);
}

#[test]
fn thin_walls_hold_at_high_speed() {
    let mut w = floor();
    w.add(Solid::new(v(5.0, 1.5, 0.0), v(0.02, 1.5, 20.0), 0.0));
    let m = w.move_capsule(&man(0.0, 0.0), v(9.0, 0.0, 0.0), 0.0);
    assert!(m.foot.x < 5.0, "durchgebrochen: {m:?}");
}

#[test]
fn inner_corners_do_not_leak() {
    let mut w = floor();
    w.add(Solid::new(v(2.0, 1.5, 0.0), v(0.5, 1.5, 5.0), 0.0));
    w.add(Solid::new(v(0.0, 1.5, 2.0), v(5.0, 1.5, 0.5), 0.0));
    let mut c = man(0.0, 0.0);
    for _ in 0..200 {
        c.foot = w.move_capsule(&c, v(0.1, 0.0, 0.1), 0.3).foot;
    }
    assert!(c.foot.x < 1.25 && c.foot.z < 1.25, "{:?}", c.foot);
    assert!(!w.blocked(&Capsule {
        radius: 0.28,
        foot: c.foot + Vec3::Y * 0.05,
        ..c
    }));
}

#[test]
fn low_steps_are_climbed_high_ones_are_not() {
    for (h, climbs) in [(0.25f32, true), (0.9, false)] {
        let mut w = floor();
        w.add(Solid::new(v(5.0, h * 0.5, 0.0), v(3.0, h * 0.5, 5.0), 0.0));
        let mut c = man(0.0, 0.0);
        for _ in 0..60 {
            c.foot = w.move_capsule(&c, v(0.08, -0.02, 0.0), 0.4).foot;
        }
        assert_eq!(c.foot.y > h - 0.05, climbs, "Stufe {h}: {:?}", c.foot);
    }
}

#[test]
fn gravity_lands_and_ceiling_bumps() {
    let mut w = floor();
    w.add(Solid::new(v(0.0, 3.0, 0.0), v(5.0, 0.5, 5.0), 0.0));
    let mut c = Capsule::new(v(0.0, 0.5, 0.0), 0.3, 1.8);
    let up = w.move_capsule(&c, Vec3::Y * 2.0, 0.0);
    assert!(up.ceiling && up.foot.y < 0.75, "{up:?}");
    for _ in 0..50 {
        c.foot = w.move_capsule(&c, Vec3::NEG_Y * 0.1, 0.0).foot;
    }
    let m = w.move_capsule(&c, Vec3::ZERO, 0.0);
    assert!(m.grounded && m.foot.y.abs() < 0.01, "{m:?}");
    let air = w.move_capsule(&Capsule::new(v(0.0, 0.4, 0.0), 0.3, 1.8), Vec3::ZERO, 0.0);
    assert!(!air.grounded);
}

#[test]
fn grid_rays_match_checking_everything() {
    let mut w = World::new(3.0);
    let h = |i: usize, k: usize| ((i * 7919 + k * 104_729 + 13) % 1000) as f32 / 1000.0;
    let mut all = Vec::new();
    for i in 0..300 {
        let s = Solid::new(
            v(
                h(i, 1) * 120.0 - 60.0,
                h(i, 2) * 6.0,
                h(i, 3) * 120.0 - 60.0,
            ),
            v(
                0.2 + h(i, 4) * 3.0,
                0.2 + h(i, 5) * 2.0,
                0.2 + h(i, 6) * 3.0,
            ),
            h(i, 7) * 6.0,
        );
        w.add(s);
        all.push(s);
    }
    for i in 0..400 {
        let dir = v(h(i, 11) - 0.5, (h(i, 12) - 0.5) * 0.3, h(i, 13) - 0.5);
        let r = Ray::new(
            v(
                h(i, 8) * 120.0 - 60.0,
                h(i, 9) * 4.0,
                h(i, 10) * 120.0 - 60.0,
            ),
            dir,
        );
        let slow = all
            .iter()
            .filter_map(|s| s.raycast(&r, 80.0))
            .map(|x| x.t)
            .fold(f32::INFINITY, f32::min);
        let fast = w.raycast(&r, 80.0).map_or(f32::INFINITY, |x| x.1.t);
        assert!(
            slow == fast || (slow - fast).abs() < 1e-3,
            "{i}: {slow} {fast}"
        );
    }
}

#[test]
fn sight_lines_and_removal() {
    let mut w = floor();
    let id = w.add(Solid::new(v(0.0, 1.0, 5.0), v(2.0, 1.0, 0.2), 0.0));
    let (a, b) = (v(0.0, 1.0, 0.0), v(0.0, 1.0, 10.0));
    assert!(!w.line_of_sight(a, b));
    assert!(w.line_of_sight(a, v(0.0, 1.0, 4.0)), "davor frei");
    assert_eq!(w.len(), 2);
    assert!(w.remove(id).is_some() && w.remove(id).is_none());
    assert!(w.line_of_sight(a, b));
    let down = w.raycast(&Ray::new(v(3.0, 5.0, 3.0), Vec3::NEG_Y), 50.0);
    assert!(
        (down.unwrap().1.t - 5.0).abs() < 1e-4,
        "senkrecht nach unten"
    );
}
