//! Eingabe: Tastatur, Maus (winit) und Controller (gilrs) in einem Zustand pro Bild.
//!
//! Das Spiel legt seine Aktionen selbst fest und belegt sie mit [`Bind`]s (Taste, Mausknopf,
//! Knopf oder Stick-Richtung), fest im Code oder umstellbar mit [`Bindings`]. Mit `app::run`
//! ist alles schon verdrahtet; sonst pro Bild `begin_frame()` aufrufen und Fenster-Ereignisse
//! mit `event()` melden. Abfragen: `pressed()`/`held()`, im festen Takt `take_pressed()`.

mod bindings;
mod pad;

pub use bindings::Bindings;
pub use pad::{Pad, Stick, Sticks};

use gilrs::{Axis, EventType, Gilrs};
use std::collections::HashSet;
use winit::event::{ElementState, KeyEvent, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

pub use winit::event::MouseButton;
/// Tasten nach ihrer Lage auf der Tastatur
pub use winit::keyboard::KeyCode as Key;

/// Ab dieser Auslenkung zaehlt der Stick als gedrueckt (0..1)
const STICK_ON: f32 = 0.6;
/// Unter dieser Auslenkung gilt er wieder als losgelassen (Hysterese gegen Flattern)
const STICK_OFF: f32 = 0.35;
/// Bildpunkte, die bei Touchpads einer Raste des Mausrads entsprechen
const PIXELS_PER_NOTCH: f64 = 40.0;

/// Eine Belegung fuer eine Aktion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bind {
    Key(KeyCode),
    Pad(Pad),
    Stick(Stick),
    Mouse(MouseButton),
}

/// Eingabezustand: gehaltene und in diesem Bild neu gedrueckte Belegungen.
pub struct Input {
    gilrs: Option<Gilrs>,
    held: HashSet<Bind>,
    pressed: HashSet<Bind>,
    /// gedrueckt, aber noch nicht mit `take_pressed` abgeholt
    latched: HashSet<Bind>,
    sticks: Sticks,
    mouse: (f32, f32),
    wheel: f32,
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
            latched: HashSet::new(),
            sticks: Sticks::default(),
            mouse: (0.0, 0.0),
            wheel: 0.0,
            using_pad: false,
        }
    }

    /// Zu Beginn jedes Bildes: alte "gedrueckt"-Meldungen weg, Controller abfragen.
    pub fn begin_frame(&mut self) {
        self.pressed.clear();
        self.mouse = (0.0, 0.0);
        self.wheel = 0.0;
        let mut events = Vec::new();
        if let Some(g) = self.gilrs.as_mut() {
            while let Some(ev) = g.next_event() {
                events.push(ev.event);
            }
        }
        for ev in events {
            match ev {
                EventType::ButtonPressed(b, _) => {
                    if let Some(p) = pad::pad_of(b) {
                        self.set(Bind::Pad(p), true);
                        self.using_pad = true;
                    }
                }
                EventType::ButtonReleased(b, _) => {
                    if let Some(p) = pad::pad_of(b) {
                        self.set(Bind::Pad(p), false);
                    }
                }
                EventType::AxisChanged(Axis::LeftStickX, v, _) => self.sticks.left.0 = v,
                EventType::AxisChanged(Axis::LeftStickY, v, _) => self.sticks.left.1 = v,
                EventType::AxisChanged(Axis::RightStickX, v, _) => self.sticks.right.0 = v,
                EventType::AxisChanged(Axis::RightStickY, v, _) => self.sticks.right.1 = v,
                EventType::ButtonChanged(b, v, _) => self.sticks.trigger(b, v),
                EventType::Disconnected => {
                    self.held
                        .retain(|b| matches!(b, Bind::Key(_) | Bind::Mouse(_)));
                    self.sticks = Sticks::default();
                }
                _ => {}
            }
        }
        self.update_stick();
    }

    fn update_stick(&mut self) {
        let (x, y) = self.sticks.left;
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
                self.latched.insert(b);
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

    /// Fenster-Ereignis melden: Tasten (ohne Wiederholung), Mausknoepfe, Mausrad, Fokus.
    pub fn event(&mut self, e: &WindowEvent) {
        match e {
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        repeat: false,
                        ..
                    },
                ..
            } => self.key(*code, *state == ElementState::Pressed),
            WindowEvent::MouseInput { state, button, .. } => {
                let down = *state == ElementState::Pressed;
                self.set(Bind::Mouse(*button), down);
                if down {
                    self.using_pad = false;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.wheel += match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / PIXELS_PER_NOTCH) as f32,
                };
            }
            WindowEvent::Focused(false) => self.release_all(),
            _ => {}
        }
    }

    /// Rohe Mausbewegung melden (Geraete-Ereignis).
    pub fn mouse_moved(&mut self, dx: f32, dy: f32) {
        self.mouse.0 += dx;
        self.mouse.1 += dy;
    }

    /// Mausbewegung in diesem Bild (rohe Schritte, y nach unten).
    pub fn mouse_delta(&self) -> (f32, f32) {
        self.mouse
    }

    /// Mausrad in diesem Bild (Rasten, positiv = nach oben).
    pub fn wheel(&self) -> f32 {
        self.wheel
    }

    /// Sticks und Trigger stufenlos.
    pub fn sticks(&self) -> &Sticks {
        &self.sticks
    }

    /// Alle Controller vibrieren lassen: Staerke 0..1, Dauer in Millisekunden. Ohne Motor
    /// passiert nichts.
    pub fn rumble(&mut self, strength: f32, millis: u32) {
        if let Some(g) = self.gilrs.as_mut() {
            pad::rumble(g, strength, millis);
        }
    }

    /// Alles loslassen (z. B. wenn das Fenster den Fokus verliert).
    pub fn release_all(&mut self) {
        self.held.clear();
        self.pressed.clear();
        self.latched.clear();
        self.sticks = Sticks::default();
    }

    /// Wurde eine der Belegungen in diesem Bild neu gedrueckt?
    pub fn pressed(&self, binds: &[Bind]) -> bool {
        binds.iter().any(|b| self.pressed.contains(b))
    }

    /// Wie [`Input::pressed`], aber fuer den festen Takt: Jeder Druck zaehlt genau einmal, auch
    /// wenn in einem Bild kein oder mehrere Schritte laufen.
    pub fn take_pressed(&mut self, binds: &[Bind]) -> bool {
        let mut hit = false;
        for b in binds {
            hit |= self.latched.remove(b);
        }
        hit
    }

    /// Noch nicht abgeholte Drucke verwerfen (z. B. beim Wechsel vom Menue ins Spiel).
    pub fn clear_pressed(&mut self) {
        self.latched.clear();
        self.pressed.clear();
    }

    /// Alles, was in diesem Bild neu gedrueckt wurde (zum Umbelegen).
    pub fn any_pressed(&self) -> Option<Bind> {
        self.pressed.iter().next().copied()
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
    fn fixed_steps_see_every_press_exactly_once() {
        let mut i = Input::keyboard_only();
        let jump = [Bind::Key(KeyCode::Space)];
        i.begin_frame();
        i.key(KeyCode::Space, true);
        i.key(KeyCode::Space, false);
        i.begin_frame(); // kein Schritt in diesem Bild
        assert!(!i.pressed(&jump));
        assert!(i.take_pressed(&jump), "Druck ging verloren");
        assert!(!i.take_pressed(&jump), "Druck zaehlt doppelt");
    }

    #[test]
    fn mouse_buttons_and_focus() {
        let mut i = Input::keyboard_only();
        let fire = [Bind::Mouse(MouseButton::Left)];
        i.set(fire[0], true);
        i.mouse_moved(2.0, -1.0);
        i.mouse_moved(1.0, 0.0);
        assert!(i.held(&fire) && i.mouse_delta() == (3.0, -1.0));
        i.event(&WindowEvent::Focused(false));
        assert!(!i.held(&fire) && !i.take_pressed(&fire));
        i.begin_frame();
        assert_eq!(i.mouse_delta(), (0.0, 0.0));
    }

    #[test]
    fn stick_acts_like_buttons_with_hysteresis() {
        let mut i = Input::keyboard_only();
        let left = [Bind::Stick(Stick::Left)];
        i.sticks.left = (-0.8, 0.0);
        i.begin_frame();
        assert!(i.pressed(&left));
        i.sticks.left = (-0.5, 0.0); // zwischen den Schwellen: bleibt gehalten
        i.begin_frame();
        assert!(i.held(&left) && !i.pressed(&left));
        i.sticks.left = (-0.1, 0.0);
        i.begin_frame();
        assert!(!i.held(&left));
        assert!(i.using_pad);
    }
}
