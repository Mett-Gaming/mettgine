//! Overlay des Launchers bedienen: Tastenkuerzel erkennen und bei offenem Feld Maus, Tasten und
//! Controller an den Launcher reichen statt ans Spiel.

use super::link::Link;
use crate::input::{Bind, Input, MouseButton, Pad, Stick};
use winit::event::{ElementState, KeyEvent, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

/// Beide zusammen oeffnen und schliessen das Feld
const PAD_TOGGLE: [Bind; 2] = [Bind::Pad(Pad::Select), Bind::Pad(Pad::Start)];

/// Tastenkuerzel fuer das Feld.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayKey {
    pub key: KeyCode,
    pub shift: bool,
}

impl OverlayKey {
    /// Schreibweise des Launchers, z. B. `shift+tab` oder `f12`.
    pub fn parse(s: &str) -> Option<Self> {
        let (shift, name) = match s.strip_prefix("shift+") {
            Some(rest) => (true, rest),
            None => (false, s),
        };
        let key = match name {
            "tab" => KeyCode::Tab,
            "f1" => KeyCode::F1,
            "f9" => KeyCode::F9,
            "f12" => KeyCode::F12,
            _ => return None,
        };
        Some(Self { key, shift })
    }
}

/// Was mit einem Fenster-Ereignis geschieht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Game,
    /// nicht ans Spiel weitergeben
    Swallow,
}

/// Zustand auf der Seite des Spiels.
pub(crate) struct Overlay {
    link: Option<Link>,
    key: Option<OverlayKey>,
    /// Umschalttaste gehalten; gilt auch, waehrend das Feld offen ist
    shift: bool,
    /// Mauszeiger, 0..1 im Fenster
    mouse: (f64, f64),
    moved: bool,
    wheel: f32,
}

impl Overlay {
    pub fn new(link: Option<Link>, key: Option<OverlayKey>) -> Self {
        Self {
            link,
            key,
            shift: false,
            mouse: (0.5, 0.5),
            moved: false,
            wheel: 0.0,
        }
    }

    pub fn open(&self) -> bool {
        self.link.as_ref().is_some_and(Link::overlay)
    }

    fn toggle(&self) {
        self.send("{\"overlay\":\"toggle\"}".into());
    }

    fn send(&self, line: String) {
        if let Some(l) = &self.link {
            l.ui(line);
        }
    }

    fn ui(&self, what: &str) {
        self.send(format!("{{\"ui\":[\"{what}\"]}}"));
    }

    /// Fenster-Ereignis ansehen; `size` ist die Fenstergroesse in Pixeln.
    pub fn event(&mut self, e: &WindowEvent, size: (u32, u32)) -> Route {
        if self.link.is_none() {
            return Route::Game;
        }
        let open = self.open();
        match e {
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        repeat,
                        ..
                    },
                ..
            } => return self.key(*code, *state == ElementState::Pressed, *repeat, open),
            WindowEvent::Focused(false) => {
                self.shift = false;
                if open {
                    self.pointer("release");
                }
            }
            WindowEvent::KeyboardInput { .. } if open => return Route::Swallow,
            WindowEvent::CursorMoved { position, .. } if open => {
                self.mouse = (
                    position.x / f64::from(size.0.max(1)),
                    position.y / f64::from(size.1.max(1)),
                );
                self.moved = true;
                return Route::Swallow;
            }
            WindowEvent::MouseInput { state, button, .. } if open => {
                if *button == MouseButton::Left {
                    let down = *state == ElementState::Pressed;
                    self.pointer(if down { "click" } else { "release" });
                }
                return Route::Swallow;
            }
            WindowEvent::MouseWheel { delta, .. } if open => {
                self.wheel += match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                return Route::Swallow;
            }
            _ => {}
        }
        Route::Game
    }

    /// Liste im Overlay blaettern; positiv = nach oben.
    fn scroll(&self, by: f32) {
        self.send(format!("{{\"ui\":[\"scroll\",{by:.2}]}}"));
    }

    fn pointer(&self, what: &str) {
        let (x, y) = self.mouse;
        self.send(format!("{{\"ui\":[\"{what}\",{x:.5},{y:.5}]}}"));
    }

    fn key(&mut self, code: KeyCode, down: bool, repeat: bool, open: bool) -> Route {
        if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
            self.shift = down;
        }
        let shortcut = |k: &OverlayKey| k.key == code && (!k.shift || self.shift);
        if down && self.key.as_ref().is_some_and(shortcut) {
            if !repeat {
                self.toggle();
            }
            return Route::Swallow;
        }
        if !open {
            return Route::Game;
        }
        if down {
            match code {
                KeyCode::ArrowUp | KeyCode::KeyW => self.ui("up"),
                KeyCode::ArrowDown | KeyCode::KeyS => self.ui("down"),
                KeyCode::ArrowLeft | KeyCode::KeyA => self.ui("left"),
                KeyCode::ArrowRight | KeyCode::KeyD => self.ui("right"),
                KeyCode::PageUp => self.scroll(1.0),
                KeyCode::PageDown => self.scroll(-1.0),
                KeyCode::Enter | KeyCode::Space if !repeat => self.ui("ok"),
                KeyCode::Escape if !repeat => self.ui("back"),
                _ => {}
            }
        }
        Route::Swallow
    }

    /// Einmal pro Bild, nachdem die Controller abgefragt sind.
    pub fn frame(&mut self, input: &mut Input) {
        if self.link.is_none() {
            return;
        }
        let toggled = input.pressed(&PAD_TOGGLE) && PAD_TOGGLE.iter().all(|b| input.held(&[*b]));
        if toggled {
            self.toggle();
        }
        if !self.open() {
            (self.moved, self.wheel) = (false, 0.0);
            return;
        }
        if std::mem::take(&mut self.moved) {
            self.pointer("move");
        }
        let wheel = std::mem::take(&mut self.wheel);
        if wheel != 0.0 {
            self.scroll(wheel);
        }
        if !toggled {
            for (binds, what) in [
                ([Bind::Pad(Pad::Up), Bind::Stick(Stick::Up)], "up"),
                ([Bind::Pad(Pad::Down), Bind::Stick(Stick::Down)], "down"),
                ([Bind::Pad(Pad::Left), Bind::Stick(Stick::Left)], "left"),
                ([Bind::Pad(Pad::Right), Bind::Stick(Stick::Right)], "right"),
                ([Bind::Pad(Pad::South); 2], "ok"),
                ([Bind::Pad(Pad::East); 2], "back"),
            ] {
                if input.pressed(&binds) {
                    self.ui(what);
                }
            }
        }
        for (pad, by) in [(Pad::LeftBumper, 1.0), (Pad::RightBumper, -1.0)] {
            if input.pressed(&[Bind::Pad(pad)]) {
                self.scroll(by);
            }
        }
        input.hide_from_game();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_parsed() {
        let k = OverlayKey::parse("shift+tab").unwrap();
        assert!(k.shift && k.key == KeyCode::Tab);
        assert_eq!(OverlayKey::parse("f12").map(|k| k.shift), Some(false));
        assert_eq!(OverlayKey::parse("f9").map(|k| k.key), Some(KeyCode::F9));
        assert_eq!(OverlayKey::parse("shift+quatsch"), None);
    }

    #[test]
    fn shortcut_needs_shift_and_open_panel_takes_everything() {
        let mut o = Overlay::new(None, OverlayKey::parse("shift+tab"));
        assert_eq!(o.key(KeyCode::Tab, true, false, false), Route::Game);
        assert_eq!(o.key(KeyCode::ShiftLeft, true, false, false), Route::Game);
        assert_eq!(o.key(KeyCode::Tab, true, false, false), Route::Swallow);
        assert_eq!(o.key(KeyCode::KeyW, true, false, true), Route::Swallow);
        o.key(KeyCode::ShiftLeft, false, false, true);
        assert_eq!(o.key(KeyCode::Tab, true, false, false), Route::Game);
        assert!(!o.open(), "ohne Launcher nie offen");
    }
}
