//! Eingabe: Tastatur (winit) und Controller (gilrs) in einem Zustand pro Bild.
//!
//! Das Spiel legt seine Aktionen selbst fest und belegt sie mit [`Bind`]s (Taste, Knopf oder
//! Stick-Richtung). Pro Bild: `begin_frame()` aufrufen, Tastatur-Ereignisse mit `key()`
//! melden, dann `pressed()`/`held()` abfragen.

use gilrs::{Axis, Button, EventType, Gilrs};
use std::collections::HashSet;
use winit::keyboard::KeyCode;

/// Ab dieser Auslenkung zaehlt der Stick als gedrueckt (0..1)
const STICK_ON: f32 = 0.6;
/// Unter dieser Auslenkung gilt er wieder als losgelassen (Hysterese gegen Flattern)
const STICK_OFF: f32 = 0.35;

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

/// Eine Belegung fuer eine Aktion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bind {
    Key(KeyCode),
    Pad(Pad),
    Stick(Stick),
}

fn pad_of(b: Button) -> Option<Pad> {
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

/// Eingabezustand: gehaltene und in diesem Bild neu gedrueckte Belegungen.
pub struct Input {
    gilrs: Option<Gilrs>,
    held: HashSet<Bind>,
    pressed: HashSet<Bind>,
    stick: (f32, f32),
    /// zuletzt benutztes Geraet war ein Controller (fuer Tastenhinweise im HUD)
    pub using_pad: bool,
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

impl Input {
    /// Ohne Controller-Unterstuetzung (z. B. kein Treiber) laeuft nur die Tastatur.
    pub fn new() -> Self {
        let gilrs = match Gilrs::new() {
            Ok(g) => Some(g),
            Err(e) => {
                crate::crash::log(&format!("Controller nicht verfuegbar: {e}"));
                None
            }
        };
        Self::with_gilrs(gilrs)
    }

    /// Nur Tastatur (Tests, Server ohne Fenster).
    pub fn keyboard_only() -> Self {
        Self::with_gilrs(None)
    }

    fn with_gilrs(gilrs: Option<Gilrs>) -> Self {
        Self {
            gilrs,
            held: HashSet::new(),
            pressed: HashSet::new(),
            stick: (0.0, 0.0),
            using_pad: false,
        }
    }

    /// Zu Beginn jedes Bildes: alte "gedrueckt"-Meldungen weg, Controller abfragen.
    pub fn begin_frame(&mut self) {
        self.pressed.clear();
        let mut events = Vec::new();
        if let Some(g) = self.gilrs.as_mut() {
            while let Some(ev) = g.next_event() {
                events.push(ev.event);
            }
        }
        for ev in events {
            match ev {
                EventType::ButtonPressed(b, _) => {
                    if let Some(p) = pad_of(b) {
                        self.set(Bind::Pad(p), true);
                        self.using_pad = true;
                    }
                }
                EventType::ButtonReleased(b, _) => {
                    if let Some(p) = pad_of(b) {
                        self.set(Bind::Pad(p), false);
                    }
                }
                EventType::AxisChanged(Axis::LeftStickX, v, _) => self.stick.0 = v,
                EventType::AxisChanged(Axis::LeftStickY, v, _) => self.stick.1 = v,
                EventType::Disconnected => {
                    self.held.retain(|b| matches!(b, Bind::Key(_)));
                    self.stick = (0.0, 0.0);
                }
                _ => {}
            }
        }
        self.update_stick();
    }

    fn update_stick(&mut self) {
        let (x, y) = self.stick;
        for (dir, v) in [
            (Stick::Right, x),
            (Stick::Left, -x),
            (Stick::Up, y),
            (Stick::Down, -y),
        ] {
            let b = Bind::Stick(dir);
            if v > STICK_ON && !self.held.contains(&b) {
                self.set(b, true);
                self.using_pad = true;
            } else if v < STICK_OFF && self.held.contains(&b) {
                self.set(b, false);
            }
        }
    }

    fn set(&mut self, b: Bind, down: bool) {
        if down {
            if self.held.insert(b) {
                self.pressed.insert(b);
            }
        } else {
            self.held.remove(&b);
        }
    }

    /// Tastatur-Ereignis melden (aus `WindowEvent::KeyboardInput`, ohne Tastenwiederholung).
    pub fn key(&mut self, code: KeyCode, down: bool) {
        self.set(Bind::Key(code), down);
        if down {
            self.using_pad = false;
        }
    }

    /// Alles loslassen (z. B. wenn das Fenster den Fokus verliert).
    pub fn release_all(&mut self) {
        self.held.clear();
        self.pressed.clear();
    }

    /// Wurde eine der Belegungen in diesem Bild neu gedrueckt?
    pub fn pressed(&self, binds: &[Bind]) -> bool {
        binds.iter().any(|b| self.pressed.contains(b))
    }

    /// Wird eine der Belegungen gehalten?
    pub fn held(&self, binds: &[Bind]) -> bool {
        binds.iter().any(|b| self.held.contains(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_pressed_once_and_held_until_released() {
        let mut i = Input::keyboard_only();
        let jump = [Bind::Key(KeyCode::Space), Bind::Pad(Pad::South)];
        i.begin_frame();
        i.key(KeyCode::Space, true);
        assert!(i.pressed(&jump) && i.held(&jump));
        i.begin_frame();
        assert!(!i.pressed(&jump) && i.held(&jump), "nur einmal gedrueckt");
        i.key(KeyCode::Space, true); // Wiederholung zaehlt nicht neu
        assert!(!i.pressed(&jump));
        i.key(KeyCode::Space, false);
        assert!(!i.held(&jump));
    }

    #[test]
    fn stick_acts_like_buttons_with_hysteresis() {
        let mut i = Input::keyboard_only();
        let left = [Bind::Stick(Stick::Left)];
        i.stick = (-0.8, 0.0);
        i.begin_frame();
        assert!(i.pressed(&left));
        i.stick = (-0.5, 0.0); // zwischen den Schwellen: bleibt gehalten
        i.begin_frame();
        assert!(i.held(&left) && !i.pressed(&left));
        i.stick = (-0.1, 0.0);
        i.begin_frame();
        assert!(!i.held(&left));
        assert!(i.using_pad);
    }
}
