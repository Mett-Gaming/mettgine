//! Menues: Auswahl mit Tastatur, Controller und Maus, Schalter, Regler, Blaettern. Das Aussehen
//! bestimmt das Spiel; [`Menu::draw`] ist ein brauchbares einfaches.

use crate::canvas::{Align, Canvas, Color};
use crate::input::{Bind, Input, Key, MouseButton, Pad, Stick};

/// Was ein Eintrag ist.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// ausloesen
    Button,
    Toggle(bool),
    /// Wert zwischen `min` und `max` in Schritten
    Slider {
        value: f32,
        min: f32,
        max: f32,
        step: f32,
    },
    /// eine von mehreren Moeglichkeiten
    Choice {
        index: usize,
        names: Vec<String>,
    },
    /// nicht waehlbar (Ueberschrift, Hinweis)
    Label,
}

/// Ein Eintrag; `id` legt das Spiel fest.
#[derive(Clone, Debug, PartialEq)]
pub struct Item<I> {
    pub id: I,
    pub text: String,
    pub kind: Kind,
    pub enabled: bool,
}

impl<I> Item<I> {
    pub fn new(id: I, text: impl Into<String>, kind: Kind) -> Self {
        Self {
            id,
            text: text.into(),
            kind,
            enabled: true,
        }
    }

    pub fn button(id: I, text: impl Into<String>) -> Self {
        Self::new(id, text, Kind::Button)
    }

    fn selectable(&self) -> bool {
        self.enabled && self.kind != Kind::Label
    }

    /// Anzeige des Werts rechts neben dem Text.
    pub fn value_text(&self) -> String {
        match &self.kind {
            Kind::Button | Kind::Label => String::new(),
            Kind::Toggle(on) => if *on { "an" } else { "aus" }.to_string(),
            Kind::Slider {
                value,
                step,
                min,
                max,
                ..
            } => {
                if (*min, *max) == (0.0, 1.0) {
                    format!("{:.0} %", value * 100.0)
                } else if step.fract() == 0.0 {
                    format!("{value:.0}")
                } else {
                    format!("{value:.2}")
                }
            }
            Kind::Choice { index, names } => names.get(*index).cloned().unwrap_or_default(),
        }
    }
}

/// Was der Spieler getan hat.
#[derive(Clone, Debug, PartialEq)]
pub enum Event<I> {
    /// Auswahl gewechselt (Klick-Geraeusch)
    Moved,
    Pressed(I),
    Toggled(I, bool),
    Slid(I, f32),
    Chosen(I, usize),
    Back,
}

/// Tasten des Menues.
#[derive(Clone, Debug)]
pub struct Keys {
    pub up: Vec<Bind>,
    pub down: Vec<Bind>,
    pub left: Vec<Bind>,
    pub right: Vec<Bind>,
    pub confirm: Vec<Bind>,
    pub back: Vec<Bind>,
}

impl Default for Keys {
    fn default() -> Self {
        let k = Bind::Key;
        let b = |list: &[Bind]| list.to_vec();
        let pad = |p, s| [Bind::Pad(p), Bind::Stick(s)];
        Self {
            up: [
                b(&[k(Key::ArrowUp), k(Key::KeyW)]),
                b(&pad(Pad::Up, Stick::Up)),
            ]
            .concat(),
            down: [
                b(&[k(Key::ArrowDown), k(Key::KeyS)]),
                b(&pad(Pad::Down, Stick::Down)),
            ]
            .concat(),
            left: [
                b(&[k(Key::ArrowLeft), k(Key::KeyA)]),
                b(&pad(Pad::Left, Stick::Left)),
            ]
            .concat(),
            right: [
                b(&[k(Key::ArrowRight), k(Key::KeyD)]),
                b(&pad(Pad::Right, Stick::Right)),
            ]
            .concat(),
            confirm: b(&[
                k(Key::Enter),
                k(Key::NumpadEnter),
                k(Key::Space),
                Bind::Pad(Pad::South),
            ]),
            back: b(&[k(Key::Escape), Bind::Pad(Pad::East)]),
        }
    }
}

const CLICK: [Bind; 1] = [Bind::Mouse(MouseButton::Left)];
/// Halten: erste Wiederholung, danach (Sekunden)
const REPEAT: (f32, f32) = (0.4, 0.09);

/// Ein Menue.
#[derive(Clone, Debug)]
pub struct Menu<I> {
    pub items: Vec<Item<I>>,
    pub keys: Keys,
    sel: usize,
    /// Flaechen der Eintraege (x, y, Breite, Hoehe), vom Zeichnen gemerkt
    rects: Vec<[f32; 4]>,
    last_cursor: (f32, f32),
    held: f32,
}

impl<I: Clone + PartialEq> Menu<I> {
    pub fn new(items: Vec<Item<I>>) -> Self {
        let mut m = Self {
            items,
            keys: Keys::default(),
            sel: 0,
            rects: Vec::new(),
            last_cursor: (f32::NAN, f32::NAN),
            held: 0.0,
        };
        m.sel = m.next(0, 0);
        m
    }

    pub fn selected(&self) -> usize {
        self.sel
    }

    pub fn select(&mut self, i: usize) {
        if self.items.get(i).is_some_and(Item::selectable) {
            self.sel = i;
        }
    }

    /// Eintraege ersetzen; die Auswahl bleibt auf demselben Eintrag, wenn es ihn noch gibt.
    pub fn set_items(&mut self, items: Vec<Item<I>>) {
        let id = self.items.get(self.sel).map(|i| i.id.clone());
        self.items = items;
        let same = self.items.iter().position(|i| Some(&i.id) == id.as_ref());
        self.sel = same.unwrap_or(self.sel.min(self.items.len().saturating_sub(1)));
        self.sel = self.next(self.sel, 0);
        self.rects.clear();
    }

    /// Naechster waehlbarer Eintrag ab `from` in Richtung `dir` (0 = notfalls vorwaerts).
    fn next(&self, from: usize, dir: i32) -> usize {
        let n = self.items.len();
        if n == 0 {
            return 0;
        }
        let step = if dir < 0 { n - 1 } else { 1 };
        let mut i = if dir == 0 {
            from % n
        } else {
            (from + step) % n
        };
        for _ in 0..n {
            if self.items[i].selectable() {
                return i;
            }
            i = (i + step) % n;
        }
        from % n
    }

    /// Einmal pro Bild. `cursor` in denselben Einheiten wie beim Zeichnen.
    pub fn update(&mut self, input: &Input, cursor: (f32, f32), dt: f32) -> Vec<Event<I>> {
        let mut out = Vec::new();
        if self.items.is_empty() {
            return out;
        }
        for (binds, dir) in [(&self.keys.up, -1), (&self.keys.down, 1)] {
            if input.pressed(binds) {
                let n = self.next(self.sel, dir);
                if n != self.sel {
                    self.sel = n;
                    out.push(Event::Moved);
                }
            }
        }
        self.mouse(input, cursor, &mut out);
        let (l, r) = (input.held(&self.keys.left), input.held(&self.keys.right));
        let first = input.pressed(&self.keys.left) || input.pressed(&self.keys.right);
        self.held = if l != r && !first {
            self.held + dt
        } else {
            0.0
        };
        let mut again = false;
        if self.held >= REPEAT.0 {
            self.held -= REPEAT.1;
            again = true;
        }
        if (first || again) && l != r {
            self.change(if r { 1.0 } else { -1.0 }, &mut out);
        }
        if input.pressed(&self.keys.confirm) {
            self.confirm(&mut out);
        }
        if input.pressed(&self.keys.back) {
            out.push(Event::Back);
        }
        out
    }

    fn mouse(&mut self, input: &Input, cursor: (f32, f32), out: &mut Vec<Event<I>>) {
        let hit = self.rects.iter().position(|r| {
            let (x, y) = (cursor.0 - r[0], cursor.1 - r[1]);
            x >= 0.0 && y >= 0.0 && x < r[2] && y < r[3]
        });
        let moved = cursor != self.last_cursor;
        self.last_cursor = cursor;
        let Some(i) = hit.filter(|i| self.items.get(*i).is_some_and(Item::selectable)) else {
            return;
        };
        // eine liegende Maus stoert die Tasten nicht
        if moved && i != self.sel {
            self.sel = i;
            out.push(Event::Moved);
        }
        let slider = matches!(self.items[i].kind, Kind::Slider { .. });
        if slider && input.held(&CLICK) && self.sel == i {
            let r = self.rects[i];
            // rechte Haelfte der Zeile ist der Regler
            let t = ((cursor.0 - r[0] - r[2] * 0.5) / (r[2] * 0.5)).clamp(0.0, 1.0);
            self.slide_to(i, t, out);
        } else if input.pressed(&CLICK) {
            self.sel = i;
            self.confirm(out);
        }
    }

    fn slide_to(&mut self, i: usize, t: f32, out: &mut Vec<Event<I>>) {
        let item = &mut self.items[i];
        if let Kind::Slider {
            value,
            min,
            max,
            step,
        } = &mut item.kind
        {
            let v = snap(*min + (*max - *min) * t, *min, *max, *step);
            if v != *value {
                *value = v;
                out.push(Event::Slid(item.id.clone(), v));
            }
        }
    }

    fn change(&mut self, dir: f32, out: &mut Vec<Event<I>>) {
        let Some(item) = self.items.get_mut(self.sel) else {
            return;
        };
        match &mut item.kind {
            Kind::Slider {
                value,
                min,
                max,
                step,
            } => {
                let v = snap(*value + *step * dir, *min, *max, *step);
                if v != *value {
                    *value = v;
                    out.push(Event::Slid(item.id.clone(), v));
                }
            }
            Kind::Choice { index, names } if !names.is_empty() => {
                let n = names.len();
                *index = (*index + if dir > 0.0 { 1 } else { n - 1 }) % n;
                out.push(Event::Chosen(item.id.clone(), *index));
            }
            Kind::Toggle(on) => {
                *on = !*on;
                out.push(Event::Toggled(item.id.clone(), *on));
            }
            _ => {}
        }
    }

    fn confirm(&mut self, out: &mut Vec<Event<I>>) {
        let Some(item) = self.items.get(self.sel).filter(|i| i.selectable()) else {
            return;
        };
        match item.kind {
            Kind::Button => out.push(Event::Pressed(item.id.clone())),
            Kind::Toggle(_) | Kind::Choice { .. } => self.change(1.0, out),
            Kind::Slider { .. } | Kind::Label => {}
        }
    }

    /// Flaechen selbst vorgeben (eigenes Aussehen): je Eintrag x, y, Breite, Hoehe.
    pub fn set_rects(&mut self, rects: Vec<[f32; 4]>) {
        self.rects = rects;
    }

    /// Einfaches Aussehen: Liste ab (x, y), `w` breit, Zeilen `row` hoch.
    pub fn draw(&mut self, c: &mut Canvas, x: f32, y: f32, w: f32, row: f32, style: &Style) {
        self.rects.clear();
        for (i, item) in self.items.iter().enumerate() {
            let top = y + i as f32 * row;
            self.rects.push([x, top, w, row]);
            let chosen = i == self.sel && item.selectable();
            if chosen {
                c.rect(x, top, w, row * 0.92, style.highlight);
            }
            let color = match () {
                _ if !item.enabled => style.disabled,
                _ if item.kind == Kind::Label => style.label,
                _ if chosen => style.selected,
                _ => style.text,
            };
            let size = row * 0.6;
            let ty = top + row * 0.14;
            c.text(x + row * 0.4, ty, &item.text, size, color, Align::Left);
            if let Kind::Slider {
                value, min, max, ..
            } = item.kind
            {
                let (bx, bw) = (x + w * 0.5, w * 0.5 - row * 3.0);
                let t = (value - min) / (max - min).max(1e-6);
                c.bar(
                    bx,
                    top + row * 0.36,
                    bw,
                    row * 0.2,
                    t,
                    color,
                    style.disabled,
                );
            }
            let value = item.value_text();
            c.text(x + w - row * 0.4, ty, &value, size, color, Align::Right);
        }
    }
}

/// Farben von [`Menu::draw`].
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub text: Color,
    pub selected: Color,
    pub label: Color,
    pub disabled: Color,
    pub highlight: Color,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            text: [0.85, 0.85, 0.82, 1.0],
            selected: [1.0, 1.0, 1.0, 1.0],
            label: [0.6, 0.6, 0.58, 1.0],
            disabled: [0.4, 0.4, 0.4, 1.0],
            highlight: [1.0, 1.0, 1.0, 0.15],
        }
    }
}

fn snap(v: f32, min: f32, max: f32, step: f32) -> f32 {
    let v = if step > 0.0 {
        min + ((v - min) / step).round() * step
    } else {
        v
    };
    // Rundungsreste der Schritte vermeiden
    (v.clamp(min, max) * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Menu<&'static str> {
        Menu::new(vec![
            Item::new("titel", "Optionen", Kind::Label),
            Item::button("start", "Start"),
            Item::new(
                "laut",
                "Lautstaerke",
                Kind::Slider {
                    value: 0.5,
                    min: 0.0,
                    max: 1.0,
                    step: 0.1,
                },
            ),
            Item::new("voll", "Vollbild", Kind::Toggle(false)),
            Item::new(
                "stufe",
                "Stufe",
                Kind::Choice {
                    index: 0,
                    names: vec!["leicht".into(), "schwer".into()],
                },
            ),
        ])
    }

    fn tap(m: &mut Menu<&'static str>, k: Key) -> Vec<Event<&'static str>> {
        let mut i = Input::keyboard_only();
        i.key(k, true);
        m.update(&i, (-1.0, -1.0), 0.016)
    }

    #[test]
    fn keys_move_over_labels_and_change_values() {
        let mut m = menu();
        assert_eq!(m.selected(), 1, "Ueberschriften sind nicht waehlbar");
        assert_eq!(tap(&mut m, Key::Enter), [Event::Pressed("start")]);
        tap(&mut m, Key::ArrowDown);
        assert_eq!(tap(&mut m, Key::ArrowRight), [Event::Slid("laut", 0.6)]);
        assert_eq!(m.items[2].value_text(), "60 %");
        tap(&mut m, Key::ArrowDown);
        assert_eq!(tap(&mut m, Key::Space), [Event::Toggled("voll", true)]);
        tap(&mut m, Key::ArrowDown);
        assert_eq!(tap(&mut m, Key::ArrowLeft), [Event::Chosen("stufe", 1)]);
        tap(&mut m, Key::ArrowDown);
        assert_eq!(m.selected(), 1, "unten geht es oben weiter");
        assert_eq!(tap(&mut m, Key::Escape), [Event::Back]);
    }

    #[test]
    fn mouse_selects_clicks_and_drags() {
        let mut m = menu();
        m.set_rects(
            (0..5)
                .map(|i| [0.0, i as f32 * 10.0, 100.0, 10.0])
                .collect(),
        );
        let mut i = Input::keyboard_only();
        assert_eq!(m.update(&i, (20.0, 35.0), 0.0), [Event::Moved]);
        assert_eq!(m.selected(), 3);
        i.press_mouse(MouseButton::Left, true);
        assert_eq!(
            m.update(&i, (20.0, 35.0), 0.0),
            [Event::Toggled("voll", true)]
        );
        i.begin_frame();
        m.update(&i, (20.0, 25.0), 0.0);
        assert_eq!(m.update(&i, (100.0, 25.0), 0.0), []);
        assert_eq!(m.update(&i, (99.9, 25.0), 0.0), [Event::Slid("laut", 1.0)]);
    }

    #[test]
    fn new_items_keep_the_selection() {
        let mut m = menu();
        m.select(3);
        let mut items = menu().items;
        items.remove(1);
        m.set_items(items);
        assert_eq!(m.items[m.selected()].id, "voll");
        m.set_items(Vec::new());
        assert!(tap(&mut m, Key::Enter).is_empty());
    }
}
