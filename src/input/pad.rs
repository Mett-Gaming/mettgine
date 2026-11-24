//! Controller: Knoepfe, stufenlose Sticks, Vibration.

use gilrs::ff::{BaseEffect, BaseEffectType, Effect, EffectBuilder, Replay, Ticks};
use gilrs::{Button, Gilrs};
use std::cell::RefCell;

/// Innerhalb dieser Auslenkung gilt ein Stick als losgelassen
const DEAD_ZONE: f32 = 0.15;

/// Controller-Knopf (Xbox-Benennung, gilt fuer alle Controller).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pad {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    Start,
    Select,
    Up,
    Down,
    Left,
    Right,
}

/// Richtung des linken Sticks als Knopf.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stick {
    Up,
    Down,
    Left,
    Right,
}

pub(super) fn pad_of(b: Button) -> Option<Pad> {
    Some(match b {
        Button::South => Pad::South,
        Button::East => Pad::East,
        Button::West => Pad::West,
        Button::North => Pad::North,
        Button::LeftTrigger => Pad::LeftBumper,
        Button::RightTrigger => Pad::RightBumper,
        Button::LeftTrigger2 => Pad::LeftTrigger,
        Button::RightTrigger2 => Pad::RightTrigger,
        Button::Start => Pad::Start,
        Button::Select => Pad::Select,
        Button::DPadUp => Pad::Up,
        Button::DPadDown => Pad::Down,
        Button::DPadLeft => Pad::Left,
        Button::DPadRight => Pad::Right,
        _ => return None,
    })
}

/// Stufenlose Werte aller Sticks und Trigger (roh, -1..1 bzw. 0..1; oben und rechts positiv).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sticks {
    pub left: (f32, f32),
    pub right: (f32, f32),
    pub left_trigger: f32,
    pub right_trigger: f32,
}

/// Tote Zone herausrechnen: dahinter beginnt der Wert wieder bei 0.
fn dead_zone((x, y): (f32, f32)) -> (f32, f32) {
    let len = x.hypot(y);
    if len <= DEAD_ZONE {
        return (0.0, 0.0);
    }
    let k = ((len - DEAD_ZONE) / (1.0 - DEAD_ZONE)).min(1.0) / len;
    (x * k, y * k)
}

impl Sticks {
    /// Linker Stick ohne tote Zone (Laufen).
    pub fn walk(&self) -> (f32, f32) {
        dead_zone(self.left)
    }

    /// Rechter Stick ohne tote Zone (Umsehen).
    pub fn look(&self) -> (f32, f32) {
        dead_zone(self.right)
    }

    pub(super) fn trigger(&mut self, b: Button, v: f32) {
        match b {
            Button::LeftTrigger2 => self.left_trigger = v,
            Button::RightTrigger2 => self.right_trigger = v,
            _ => {}
        }
    }
}

thread_local! {
    /// laufende Vibration: muss leben, solange sie spielt
    static PLAYING: RefCell<Option<Effect>> = const { RefCell::new(None) };
}

pub(super) fn rumble(g: &mut Gilrs, strength: f32, millis: u32) {
    let pads: Vec<_> = g
        .gamepads()
        .filter(|(_, p)| p.is_ff_supported())
        .map(|(id, _)| id)
        .collect();
    if pads.is_empty() {
        return;
    }
    let magnitude = (strength.clamp(0.0, 1.0) * u16::MAX as f32) as u16;
    let effect = EffectBuilder::new()
        .add_effect(BaseEffect {
            kind: BaseEffectType::Strong { magnitude },
            scheduling: Replay {
                play_for: Ticks::from_ms(millis),
                ..Default::default()
            },
            envelope: Default::default(),
        })
        .repeat(gilrs::ff::Repeat::For(Ticks::from_ms(millis)))
        .gamepads(&pads)
        .finish(g);
    if let Ok(e) = effect {
        let _ = e.play();
        PLAYING.with(|p| *p.borrow_mut() = Some(e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_movements_are_ignored_and_the_edge_stays_reachable() {
        assert_eq!(dead_zone((0.1, 0.05)), (0.0, 0.0));
        let (x, _) = dead_zone((1.0, 0.0));
        assert!((x - 1.0).abs() < 1e-5);
        let (x, y) = dead_zone((0.16, 0.0));
        assert!(x > 0.0 && x < 0.05 && y == 0.0, "kein Sprung an der Grenze");
    }
}
