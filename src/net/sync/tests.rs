//! Tests des Abgleichs: Packen, Uhr, Zwischenbilder, Vorhersage mit Verlust.

use super::clock::*;
use super::interp::*;
use super::predict::*;
use super::wire::*;
use super::*;
use crate::net::loopback::Loopback;
use crate::net::{Delivery, NetEvent, Transport, HOST};
use glam::{Quat, Vec3};

#[test]
fn wire_roundtrip_and_broken_data() {
    let mut w = Writer::new();
    w.u8(7);
    w.bool(true);
    w.u16(65_000);
    w.u32(4_000_000_000);
    w.f32(-1.5);
    w.f64(1e9);
    w.vec3(Vec3::new(1.0, 2.0, 3.0));
    w.ranged(0.3, -1.0, 1.0);
    w.ranged(9.0, -1.0, 1.0);
    w.angle(-1.0);
    w.angle(std::f32::consts::TAU - 1e-7);
    w.str("Mettbrötchen");
    w.str(&"ö".repeat(200));
    let mut r = Reader::new(w.bytes());
    assert_eq!(
        (r.u8(), r.bool(), r.u16()),
        (Some(7), Some(true), Some(65_000))
    );
    assert_eq!(
        (r.u32(), r.f32(), r.f64()),
        (Some(4_000_000_000), Some(-1.5), Some(1e9))
    );
    assert_eq!(r.vec3(), Some(Vec3::new(1.0, 2.0, 3.0)));
    assert!((r.ranged(-1.0, 1.0).unwrap() - 0.3).abs() < 1e-4);
    assert_eq!(r.ranged(-1.0, 1.0), Some(1.0));
    assert!((r.angle().unwrap() - (std::f32::consts::TAU - 1.0)).abs() < 1e-3);
    assert_eq!(r.angle(), Some(0.0), "fast einmal herum ist 0");
    assert_eq!(r.str(), Some("Mettbrötchen"));
    assert_eq!(r.str().map(|s| s.chars().count()), Some(127));
    assert_eq!((r.left(), r.u8()), (0, None));
    for n in 0..w.bytes().len() {
        let mut r = Reader::new(&w.bytes()[..n]);
        while r.str().is_some() {}
        let _ = (r.vec3(), r.u32(), r.angle());
    }
    assert_eq!(Reader::new(&f32::NAN.to_le_bytes()).f32(), None);
    assert_eq!(Reader::new(&[200, 1, 2]).str(), None, "Laenge gelogen");
}

#[test]
fn ticker_is_independent_of_frame_rate_and_survives_hangs() {
    for fps in [30.0, 59.0, 144.0] {
        let mut t = Ticker::new(60.0);
        let n: u32 = (0..(fps as u32 * 10)).map(|_| t.advance(1.0 / fps)).sum();
        assert!((599..=600).contains(&n), "{fps}: {n}");
        assert!((0.0..1.0).contains(&t.fraction()));
    }
    let mut t = Ticker::new(60.0);
    assert_eq!(t.advance(30.0), 8, "holt nicht endlos nach");
    assert_eq!(t.tick(), 8);
}

#[test]
fn clock_settles_despite_jitter_and_jumps_after_a_break() {
    let mut c = RemoteClock::new();
    assert_eq!(c.now(1.0), None);
    for i in 0..400 {
        let local = i as f64 * 0.05;
        let late = ((i * 7919) % 100) as f64 / 100.0 * 0.04;
        c.observe(local, local + 500.0 - 0.05 - late);
    }
    let off = c.now(20.0).unwrap() - 20.0;
    assert!((off - 499.93).abs() < 0.015, "{off}");
    assert!(c.jitter() > 0.001 && c.jitter() < 0.04);
    assert!(c.display_time(20.0, 0.05).unwrap() < c.now(20.0).unwrap() - 0.07);
    c.observe(21.0, 900.0);
    assert_eq!(c.now(21.0), Some(900.0));
}

#[test]
fn snapshots_blend_reorder_and_extrapolate_shortly() {
    let mut s = Snapshots::new(0.1);
    assert!(s.sample(0.0).is_none());
    s.push(1.0, 10.0f32);
    assert_eq!(s.sample(5.0), Some(10.0));
    s.push(1.2, 30.0);
    s.push(1.1, 20.0); // verspaetet
    s.push(1.1, 20.0); // doppelt
    assert_eq!(s.sample(0.5), Some(10.0));
    assert!((s.sample(1.05).unwrap() - 15.0).abs() < 1e-3);
    assert!((s.sample(1.15).unwrap() - 25.0).abs() < 1e-3);
    assert!((s.sample(1.25).unwrap() - 35.0).abs() < 1e-2);
    assert!(
        (s.sample(9.0).unwrap() - 40.0).abs() < 1e-2,
        "bleibt stehen"
    );
    for i in 0..500 {
        s.push(2.0 + i as f64, 0.0);
    }
    assert_eq!(s.latest(), Some(&0.0));
    let a = Angle(6.2).blend(&Angle(0.1), 0.5).0;
    assert!((a - (6.2 + 0.0916)).abs() < 1e-2, "kurzer Weg: {a}");
    let q = Quat::IDENTITY.blend(&Quat::from_rotation_y(1.0), 0.5);
    assert!(q.angle_between(Quat::from_rotation_y(0.5)) < 1e-3);
}

#[test]
fn sequence_numbers_may_wrap() {
    assert!(newer(1, 0) && !newer(0, 0) && newer(3, u32::MAX - 2) && !newer(u32::MAX, 5));
}

/// Gemeinsame Rechnung: Ort plus Tempo, Wand bei x = 5 kennt nur der Host.
fn step(s: &mut f32, i: &f32) {
    *s += i * 0.1;
}

#[test]
fn prediction_follows_the_host_despite_lost_messages() {
    let mut net = Loopback::network(&["Host", "Mitspieler"]);
    let mut me = Predictor::new(0.0f32);
    let mut host_state = 0.0f32;
    let mut inputs = InputQueue::new();
    let mut w = Writer::new();
    for n in 0..400u32 {
        let lost = |salt: u32| (n * 31 + salt) % 5 == 0; // jede fuenfte geht verloren
        let dir = if n < 300 { 1.0 } else { 0.0 };
        me.apply(dir, step);
        w.clear();
        for (seq, i) in me.recent(20) {
            w.u32(*seq);
            w.f32(*i);
        }
        if !lost(0) {
            net[1].send(HOST, w.bytes(), Delivery::Unreliable);
        }
        for e in net[0].poll() {
            if let NetEvent::Message { data, .. } = e {
                let mut r = Reader::new(&data);
                while let Some((s, i)) = r.u32().zip(r.f32()) {
                    inputs.push(s, i);
                }
            }
        }
        while let Some(i) = inputs.pop() {
            step(&mut host_state, &i);
            host_state = host_state.min(5.0);
        }
        w.clear();
        w.u32(inputs.ack());
        w.f32(host_state);
        if !lost(2) {
            net[0].send(1, w.bytes(), Delivery::Unreliable);
        }
        for e in net[1].poll() {
            if let NetEvent::Message { data, .. } = e {
                let mut r = Reader::new(&data);
                let (ack, s) = r.u32().zip(r.f32()).unwrap();
                me.reconcile(ack, s, step);
            }
        }
        assert!((me.state() - host_state).abs() < 1.0, "laeuft auseinander");
    }
    assert_eq!(*me.state(), 5.0, "der Host hat recht");
    assert!(!me.reconcile(1, 99.0, step), "veraltet");
    assert!(!me.reconcile(9999, 99.0, step), "nie gesendet");
    assert_eq!(*me.state(), 5.0);
}

#[test]
fn inputs_are_ordered_once_and_corrections_fade() {
    let mut q = InputQueue::new();
    for s in [3, 1, 2, 2, 1, 5] {
        q.push(s, s);
    }
    let got: Vec<u32> = std::iter::from_fn(|| q.pop()).collect();
    assert_eq!(got, [1, 2, 3, 5]);
    q.push(4, 4);
    assert!(q.is_empty() && q.ack() == 5, "zu spaet");
    let mut s = Smoothing::new(0.1, 3.0);
    s.corrected(Vec3::X, Vec3::ZERO);
    assert_eq!(s.update(0.0, Vec3::ZERO), Vec3::X, "kein Sprung");
    assert!((s.update(0.1, Vec3::ZERO).x - 0.5).abs() < 1e-4);
    s.corrected(Vec3::X * 50.0, Vec3::ZERO);
    assert_eq!(s.update(0.0, Vec3::ZERO), Vec3::ZERO, "weit weg: springen");
}
