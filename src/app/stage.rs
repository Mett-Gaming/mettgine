//! Was das Geruest fuer das Spiel bereithaelt: Fenster, Renderer, Eingabe, Maus.

use crate::input::Input;
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
    cursor: (f32, f32),
    grabbed: bool,
    quit: bool,
}

impl Stage {
    pub(super) fn new(renderer: Renderer, window: Window) -> Self {
        Self {
            renderer,
            window,
            input: Input::new(),
            cursor: (0.0, 0.0),
            grabbed: false,
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
    pub fn set_grab(&mut self, grab: bool) {
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
