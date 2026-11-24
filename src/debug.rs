//! Werkzeuge fuer Entwickler: Linien und Formen in der Welt ([`Lines`]), Konsole mit Befehlen und
//! Werten ([`Console`]), Zeitmessung von Abschnitten ([`Scopes`]).

use crate::canvas::{Align, Canvas, Color};
use crate::render::{project, UiVertex};
use glam::{Mat4, Vec2, Vec3};
use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

/// Linien in der Welt, als Teil der Oberflaeche gezeichnet (immer sichtbar).
#[derive(Clone, Debug, Default)]
pub struct Lines {
    list: Vec<(Vec3, Vec3, Color)>,
}

impl Lines {
    pub fn clear(&mut self) {
        self.list.clear();
    }

    pub fn line(&mut self, a: Vec3, b: Vec3, color: Color) {
        self.list.push((a, b, color));
    }

    /// Kreuz an einem Punkt.
    pub fn point(&mut self, p: Vec3, size: f32, color: Color) {
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            self.line(p - axis * size, p + axis * size, color);
        }
    }

    pub fn boxed(&mut self, min: Vec3, max: Vec3, color: Color) {
        let c = |i: usize| {
            let pick = |bit: usize, a: f32, b: f32| if i & bit == 0 { a } else { b };
            Vec3::new(
                pick(1, min.x, max.x),
                pick(2, min.y, max.y),
                pick(4, min.z, max.z),
            )
        };
        for i in 0..8 {
            for bit in [1, 2, 4] {
                if i & bit == 0 {
                    self.line(c(i), c(i | bit), color);
                }
            }
        }
    }

    pub fn circle(&mut self, center: Vec3, radius: f32, color: Color) {
        let at = |i: i32| {
            let a = i as f32 / 24.0 * std::f32::consts::TAU;
            center + Vec3::new(a.cos(), 0.0, a.sin()) * radius
        };
        for i in 0..24 {
            self.line(at(i), at(i + 1), color);
        }
    }

    pub fn arrow(&mut self, from: Vec3, to: Vec3, color: Color) {
        let d = to - from;
        let side = d.normalize_or_zero().any_orthonormal_vector() * d.length() * 0.12;
        let back = to - d * 0.2;
        self.line(from, to, color);
        self.line(to, back + side, color);
        self.line(to, back - side, color);
    }

    /// In UI-Dreiecke umrechnen; `area` = virtuelle Flaeche, `width` = Strichbreite.
    pub fn draw(&self, view_proj: &Mat4, area: (f32, f32), width: f32, out: &mut Vec<UiVertex>) {
        for (a, b, color) in &self.list {
            let (Some(a), Some(b)) = (project(view_proj, *a), project(view_proj, *b)) else {
                continue; // Enden hinter der Kamera lassen sich nicht zeichnen
            };
            let size = Vec2::new(area.0, area.1);
            let (a, b) = (a * size, b * size);
            let side = (b - a).perp().normalize_or_zero() * width * 0.5;
            let v = |p: Vec2| UiVertex {
                pos: p.to_array(),
                uv: [-1.0, -1.0],
                color: *color,
            };
            let q = [a - side, a + side, b + side, b - side].map(v);
            out.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
        }
    }
}

/// Ein Wert, den man zur Laufzeit verstellen kann.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Var {
    pub value: f32,
    pub min: f32,
    pub max: f32,
}

/// Was eine Eingabe in der Konsole ausgeloest hat.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Befehl des Spiels mit seinen Angaben
    Run(String, Vec<String>),
    /// Wert geaendert
    Set(String, f32),
    /// schon erledigt (Hilfe, Anzeige, Fehler)
    Done,
}

/// Konsole: Werte verstellen und Befehle des Spiels aufrufen.
#[derive(Clone, Debug, Default)]
pub struct Console {
    vars: BTreeMap<String, Var>,
    commands: BTreeMap<String, String>,
    pub log: VecDeque<String>,
    pub input: String,
    history: Vec<String>,
    at: usize,
    pub open: bool,
}

/// So viele Zeilen bleiben stehen
const LOG_LINES: usize = 200;

impl Console {
    /// Wert anmelden (Name ohne Leerzeichen).
    pub fn var(&mut self, name: &str, value: f32, min: f32, max: f32) {
        self.vars.insert(name.to_string(), Var { value, min, max });
    }

    /// Wert lesen; unbekannte sind 0.
    pub fn get(&self, name: &str) -> f32 {
        self.vars.get(name).map_or(0.0, |v| v.value)
    }

    /// Befehl anmelden.
    pub fn command(&mut self, name: &str, help: &str) {
        self.commands.insert(name.to_string(), help.to_string());
    }

    pub fn print(&mut self, line: impl Into<String>) {
        self.log.push_back(line.into());
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    /// Zeile ausfuehren.
    pub fn run(&mut self, line: &str) -> Command {
        let line = line.trim();
        if line.is_empty() {
            return Command::Done;
        }
        self.print(format!("> {line}"));
        self.history.push(line.to_string());
        self.at = self.history.len();
        let mut words = line.split_whitespace().map(str::to_string);
        let name = words.next().unwrap_or_default();
        let args: Vec<String> = words.collect();
        if name == "hilfe" {
            self.help();
            return Command::Done;
        }
        if self.commands.contains_key(&name) {
            return Command::Run(name, args);
        }
        let Some(var) = self.vars.get_mut(&name) else {
            self.print(format!("unbekannt: {name} (hilfe zeigt alles)"));
            return Command::Done;
        };
        let Some(text) = args.first() else {
            let text = format!("{name} = {} ({} bis {})", var.value, var.min, var.max);
            self.print(text);
            return Command::Done;
        };
        match text.replace(',', ".").parse::<f32>() {
            Ok(v) if v.is_finite() => {
                var.value = v.clamp(var.min, var.max);
                Command::Set(name, var.value)
            }
            _ => {
                self.print(format!("keine Zahl: {text}"));
                Command::Done
            }
        }
    }

    fn help(&mut self) {
        let commands = self.commands.iter().map(|(n, h)| format!("{n}: {h}"));
        let vars = self.vars.iter().map(|(n, v)| format!("{n} = {}", v.value));
        for l in commands.chain(vars).collect::<Vec<_>>() {
            self.print(l);
        }
    }

    /// Eingabezeile ausfuehren und leeren.
    pub fn enter(&mut self) -> Command {
        let line = std::mem::take(&mut self.input);
        self.run(&line)
    }

    /// Text eintippen (aus `KeyEvent::text`); Steuerzeichen fallen weg.
    pub fn type_text(&mut self, text: &str) {
        self.input.extend(text.chars().filter(|c| !c.is_control()));
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    /// Frueher Eingegebenes: -1 = aelter, 1 = neuer.
    pub fn recall(&mut self, dir: i32) {
        let at = self.at as i64 + dir as i64;
        self.at = at.clamp(0, self.history.len() as i64) as usize;
        self.input = self.history.get(self.at).cloned().unwrap_or_default();
    }

    /// Angefangenes Wort ergaenzen, soweit eindeutig.
    pub fn complete(&mut self) {
        let start = self.input.clone();
        let names = self.commands.keys().chain(self.vars.keys());
        let hits: Vec<&String> = names.filter(|n| n.starts_with(&start)).collect();
        let Some(first) = hits.first() else {
            return;
        };
        let mut len = first.len();
        for h in &hits {
            let same = first.bytes().zip(h.bytes()).take_while(|(a, b)| a == b);
            len = len.min(same.count());
        }
        if let Some(s) = first.get(..len) {
            self.input = s.to_string();
        }
    }

    /// Obere Bildhaelfte.
    pub fn draw(&self, c: &mut Canvas) {
        if !self.open {
            return;
        }
        let (size, h) = (16.0, c.h * 0.5);
        let w = c.w;
        c.rect(0.0, 0.0, w, h, [0.0, 0.0, 0.0, 0.82]);
        let text = format!("> {}_", self.input);
        c.text(10.0, h - 26.0, &text, size, [1.0; 4], Align::Left);
        let rows = ((h - 34.0) / 20.0) as usize;
        for (i, l) in self.log.iter().rev().take(rows).enumerate() {
            let y = h - 50.0 - i as f32 * 20.0;
            c.text(10.0, y, l, size, [0.75, 0.85, 0.75, 1.0], Align::Left);
        }
    }
}

/// Zeitmessung benannter Abschnitte eines Bildes (Prozessor).
#[derive(Debug, Default)]
pub struct Scopes {
    /// Name, geglaettete Millisekunden
    times: Vec<(&'static str, f32)>,
}

impl Scopes {
    /// Abschnitt messen.
    pub fn measure<T>(&mut self, name: &'static str, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let out = f();
        self.add(name, start.elapsed().as_secs_f32() * 1000.0);
        out
    }

    /// Selbst gemessene Zeit (ms) eintragen.
    pub fn add(&mut self, name: &'static str, ms: f32) {
        match self.times.iter_mut().find(|t| t.0 == name) {
            Some(t) => t.1 += (ms - t.1) * 0.1,
            None => self.times.push((name, ms)),
        }
    }

    /// Zeilen fuer `profile::draw_overlay`, Langsames zuerst.
    pub fn lines(&self) -> Vec<String> {
        let mut t = self.times.clone();
        t.sort_by(|a, b| b.1.total_cmp(&a.1));
        t.iter().map(|(n, ms)| format!("{n}: {ms:.2} ms")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_sets_values_and_passes_commands_on() {
        let mut c = Console::default();
        c.var("schwerkraft", 9.8, 0.0, 20.0);
        c.command("setze", "stellt etwas hin");
        assert_eq!(
            c.run("schwerkraft 3,5"),
            Command::Set("schwerkraft".into(), 3.5)
        );
        assert_eq!(
            c.run("schwerkraft 99"),
            Command::Set("schwerkraft".into(), 20.0)
        );
        assert_eq!(c.get("schwerkraft"), 20.0);
        let args = vec!["kiste".to_string(), "3".to_string()];
        assert_eq!(c.run("setze kiste 3"), Command::Run("setze".into(), args));
        assert_eq!(c.run("quatsch"), Command::Done);
        assert!(c.log.back().unwrap().contains("unbekannt"));
        c.input = "se".into();
        c.complete();
        assert_eq!(c.input, "setze");
        c.recall(-1);
        assert_eq!(c.input, "quatsch");
        c.run("hilfe");
        assert!(c.log.iter().any(|l| l.contains("stellt etwas hin")));
    }

    #[test]
    fn lines_become_thin_quads_and_hide_behind_the_camera() {
        let mut l = Lines::default();
        l.line(
            Vec3::new(-1.0, 0.0, -5.0),
            Vec3::new(1.0, 0.0, -5.0),
            [1.0; 4],
        );
        l.line(Vec3::Z * 5.0, Vec3::Z * 6.0, [1.0; 4]);
        let mut out = Vec::new();
        let vp = crate::render::perspective(1.0, 1.0, 0.1, 100.0);
        l.draw(&vp, (100.0, 100.0), 2.0, &mut out);
        assert_eq!(out.len(), 6);
        let ys: Vec<f32> = out.iter().map(|v| v.pos[1]).collect();
        assert!(ys.iter().all(|y| (48.9..51.1).contains(y)));
        l.clear();
        l.boxed(Vec3::ZERO, Vec3::ONE, [1.0; 4]);
        assert_eq!(l.list.len(), 12);
        let mut s = Scopes::default();
        assert_eq!(s.measure("a", || 5), 5);
        assert_eq!(s.lines().len(), 1);
    }
}
