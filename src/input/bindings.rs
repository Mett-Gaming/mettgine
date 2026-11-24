//! Umstellbare Belegungen: Aktionen mit Namen, speicherbar in den Einstellungen.

use super::{Bind, Input, MouseButton, Pad, Stick};
use crate::settings::KeyValues;
use winit::keyboard::KeyCode;

/// Vorsatz der Schluessel in der Einstellungsdatei
const PREFIX: &str = "taste_";

/// Belegung aller Aktionen eines Spiels. `A` ist eine Aufzaehlung des Spiels.
///
/// ```
/// use mettgine::input::{Bind, Bindings, Key, Pad};
/// #[derive(Clone, Copy, PartialEq)]
/// enum Aktion { Springen }
/// let tasten = Bindings::new(&[
///     (Aktion::Springen, "springen", &[Bind::Key(Key::Space), Bind::Pad(Pad::South)]),
/// ]);
/// assert_eq!(tasten.of(Aktion::Springen).len(), 2);
/// ```
#[derive(Clone, Debug)]
pub struct Bindings<A> {
    list: Vec<(A, &'static str, Vec<Bind>)>,
}

impl<A: Copy + PartialEq> Bindings<A> {
    /// Aktionen mit Speichernamen und Vorgabe.
    pub fn new(defaults: &[(A, &'static str, &[Bind])]) -> Self {
        Self {
            list: defaults
                .iter()
                .map(|(a, name, binds)| (*a, *name, binds.to_vec()))
                .collect(),
        }
    }

    /// Belegungen einer Aktion (leer, wenn es sie nicht gibt).
    pub fn of(&self, action: A) -> &[Bind] {
        self.list
            .iter()
            .find(|(a, _, _)| *a == action)
            .map_or(&[], |(_, _, b)| b)
    }

    /// Aktion neu belegen.
    pub fn set(&mut self, action: A, binds: &[Bind]) {
        if let Some(e) = self.list.iter_mut().find(|(a, _, _)| *a == action) {
            e.2 = binds.to_vec();
        }
    }

    pub fn pressed(&self, input: &Input, action: A) -> bool {
        input.pressed(self.of(action))
    }

    pub fn held(&self, input: &Input, action: A) -> bool {
        input.held(self.of(action))
    }

    /// Fuer den festen Takt, siehe [`Input::take_pressed`].
    pub fn take_pressed(&self, input: &mut Input, action: A) -> bool {
        input.take_pressed(self.of(action))
    }

    /// In die Einstellungen schreiben.
    pub fn save(&self, kv: &mut KeyValues) {
        for (_, name, binds) in &self.list {
            let text: Vec<String> = binds.iter().map(|b| bind_name(*b)).collect();
            kv.set(&format!("{PREFIX}{name}"), text.join(","));
        }
    }

    /// Aus den Einstellungen lesen; Unbekanntes faellt weg, ohne Eintrag bleibt die Vorgabe.
    pub fn load(&mut self, kv: &KeyValues) {
        for (_, name, binds) in &mut self.list {
            let Some(text) = kv.str(&format!("{PREFIX}{name}")) else {
                continue;
            };
            let read: Vec<Bind> = text.split(',').filter_map(parse_bind).collect();
            if !read.is_empty() {
                *binds = read;
            }
        }
    }
}

const PADS: [Pad; 14] = [
    Pad::South,
    Pad::East,
    Pad::West,
    Pad::North,
    Pad::LeftBumper,
    Pad::RightBumper,
    Pad::LeftTrigger,
    Pad::RightTrigger,
    Pad::Start,
    Pad::Select,
    Pad::Up,
    Pad::Down,
    Pad::Left,
    Pad::Right,
];
const STICKS: [Stick; 4] = [Stick::Up, Stick::Down, Stick::Left, Stick::Right];
const MOUSE: [MouseButton; 5] = [
    MouseButton::Left,
    MouseButton::Right,
    MouseButton::Middle,
    MouseButton::Back,
    MouseButton::Forward,
];
/// Tasten, die sich speichern lassen
const KEYS: &[KeyCode] = {
    use KeyCode::*;
    &[
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        Numpad0,
        Numpad1,
        Numpad2,
        Numpad3,
        Numpad4,
        Numpad5,
        Numpad6,
        Numpad7,
        Numpad8,
        Numpad9,
        NumpadAdd,
        NumpadSubtract,
        NumpadMultiply,
        NumpadDivide,
        NumpadDecimal,
        NumpadEnter,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        Escape,
        Enter,
        Space,
        Tab,
        Backspace,
        Insert,
        Delete,
        Home,
        End,
        PageUp,
        PageDown,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        ShiftLeft,
        ShiftRight,
        ControlLeft,
        ControlRight,
        AltLeft,
        AltRight,
        CapsLock,
        Minus,
        Equal,
        BracketLeft,
        BracketRight,
        Backslash,
        Semicolon,
        Quote,
        Comma,
        Period,
        Slash,
        Backquote,
        IntlBackslash,
    ]
};

fn bind_name(b: Bind) -> String {
    match b {
        Bind::Key(k) => format!("{k:?}"),
        Bind::Pad(p) => format!("Pad.{p:?}"),
        Bind::Stick(s) => format!("Stick.{s:?}"),
        Bind::Mouse(m) => format!("Mouse.{m:?}"),
    }
}

fn parse_bind(text: &str) -> Option<Bind> {
    let text = text.trim();
    let keys = KEYS.iter().map(|k| Bind::Key(*k));
    let pads = PADS.iter().map(|p| Bind::Pad(*p));
    let sticks = STICKS.iter().map(|s| Bind::Stick(*s));
    let mouse = MOUSE.iter().map(|m| Bind::Mouse(*m));
    keys.chain(pads)
        .chain(sticks)
        .chain(mouse)
        .find(|b| bind_name(*b) == text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, PartialEq)]
    enum Act {
        Jump,
        Fire,
    }

    fn bindings() -> Bindings<Act> {
        Bindings::new(&[
            (Act::Jump, "jump", &[Bind::Key(KeyCode::Space)]),
            (Act::Fire, "fire", &[Bind::Mouse(MouseButton::Left)]),
        ])
    }

    #[test]
    fn rebinding_survives_saving() {
        let mut b = bindings();
        let new = [
            Bind::Key(KeyCode::KeyJ),
            Bind::Pad(Pad::North),
            Bind::Stick(Stick::Up),
            Bind::Mouse(MouseButton::Right),
        ];
        b.set(Act::Jump, &new);
        let mut kv = KeyValues::default();
        b.save(&mut kv);
        let mut back = bindings();
        back.load(&KeyValues::parse(&kv.to_text()));
        assert_eq!(back.of(Act::Jump), new);
        assert_eq!(back.of(Act::Fire), bindings().of(Act::Fire));
    }

    #[test]
    fn broken_entries_keep_the_default() {
        let mut b = bindings();
        b.load(&KeyValues::parse("taste_jump=Quatsch,,\n"));
        assert_eq!(b.of(Act::Jump), [Bind::Key(KeyCode::Space)]);
    }
}
