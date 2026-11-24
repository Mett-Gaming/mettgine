//! Was das Geruest fuer das Spiel bereithaelt: Fenster, Renderer, Eingabe, Maus.

use crate::input::Input;
use crate::launcher::Achievements;
use crate::render::Renderer;
use winit::window::{CursorGrabMode, Fullscreen, Window};

/// Groesse des Bildes in Pixeln.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewSize {
    pub width: u32,
    pub height: u32,
}

/// Fenster und Renderer des laufenden Spiels.
pub struct Stage {
    // Reihenfolge wichtig: Renderer (Surface) vor dem Fenster freigeben.
    pub renderer: Renderer,
    pub window: Window,
    /// Tastatur, Maus und Controller
    pub input: Input,
    /// Erfolge fuer den Launcher; ohne Launcher passiert nichts
    pub achievements: Achievements,
    cursor: (f32, f32),
    grabbed: bool,
    overlay: bool,
    quit: bool,
}

impl Stage {
    pub(super) fn new(renderer: Renderer, window: Window, achievements: Achievements) -> Self {
        Self {
            renderer,
            window,
            input: Input::new(),
            achievements,
            cursor: (0.0, 0.0),
            grabbed: false,
            overlay: false,
            quit: false,
        }
    }

    /// Spiel nach diesem Bild beenden.
    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn quitting(&self) -> bool {
        self.quit
    }

    /// Fenstergroesse in Pixeln.
    pub fn size(&self) -> ViewSize {
        let s = self.window.inner_size();
        ViewSize {
            width: s.width,
            height: s.height,
        }
    }

    /// Seitenverhaeltnis des Fensters (Breite / Hoehe).
    pub fn aspect(&self) -> f32 {
        let s = self.size();
        s.width.max(1) as f32 / s.height.max(1) as f32
    }

    /// Mauszeiger in Einheiten der Oberflaeche ([`Renderer::ui_area`]).
    pub fn cursor(&self) -> (f32, f32) {
        self.cursor
    }

    pub(super) fn moved(&mut self, x: f64, y: f64) {
        let s = self.size();
        let (w, h) = self.renderer.ui_area();
        self.cursor = (
            x as f32 / s.width.max(1) as f32 * w,
            y as f32 / s.height.max(1) as f32 * h,
        );
    }

    /// Ist die Maus gefangen und unsichtbar (Kamera-Steuerung)?
    pub fn grabbed(&self) -> bool {
        self.grabbed
    }

    /// Maus fangen und verstecken oder wieder freigeben.
    /// Solange das Overlay des Launchers offen ist, gilt der Wunsch erst danach.
    pub fn set_grab(&mut self, grab: bool) {
        if self.overlay {
            self.grabbed = grab;
            return;
        }
        if grab {
            self.grabbed = self
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok();
        } else {
            let _ = self.window.set_cursor_grab(CursorGrabMode::None);
            self.grabbed = false;
        }
        self.window.set_cursor_visible(!grab);
    }

    /// Ist das Overlay des Launchers offen? Das Spiel bekommt dann keine Eingaben; im Einzelspieler
    /// sollte es anhalten.
    pub fn overlay_open(&self) -> bool {
        self.overlay
    }

    pub(super) fn set_overlay(&mut self, open: bool) {
        if self.overlay == open {
            return;
        }
        self.input.release_all();
        if open {
            let _ = self.window.set_cursor_grab(CursorGrabMode::None);
            self.window.set_cursor_visible(true);
        }
        self.overlay = open;
        if !open {
            self.set_grab(self.grabbed);
        }
    }

    pub fn fullscreen(&self) -> bool {
        self.window.fullscreen().is_some()
    }

    /// Randloses Vollbild an oder aus.
    pub fn set_fullscreen(&mut self, on: bool) {
        if self.fullscreen() != on {
            self.window
                .set_fullscreen(on.then_some(Fullscreen::Borderless(None)));
        }
    }

    /// Laeuft noch das Startbild der Engine?
    pub fn splash_active(&self) -> bool {
        self.renderer.splash_active()
    }
}
