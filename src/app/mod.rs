//! Spielgeruest: Fenster, Ereignisse, Bildtakt, Absturzfenster. Ein Spiel setzt [`Game`] um und
//! startet mit [`run`]; mehr braucht es nicht (siehe `examples/minimal.rs`).
//!
//! Wer alles selbst machen will, benutzt `render::Renderer` weiterhin direkt.

mod handler;
mod stage;

pub use stage::{Stage, ViewSize};

use crate::presence::Status;
use crate::render::{FrameInput, Renderer};
use crate::{crash, crash_window};
use winit::event::WindowEvent;
use winit::window::Window;

/// Fehler, die ein Spiel beim Start melden kann.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Beschreibung eines Spiels. Vorgaben: [`AppConfig::new`].
#[derive(Clone, Debug)]
pub struct AppConfig {
    /// Kurzname fuer Ordner und Protokoll (klein, ohne Leerzeichen)
    pub id: &'static str,
    /// Fenstertitel und Name in Fehlermeldungen
    pub title: String,
    pub version: &'static str,
    /// Fenstergroesse beim Start (ohne Bildschirm-Skalierung)
    pub size: (f64, f64),
    /// Laengste Bildzeit in Sekunden, siehe [`crate::time::FrameClock`]
    pub max_dt: f32,
    /// Eingaben verfallen, solange das Startbild laeuft
    pub block_input_on_splash: bool,
    /// Fenster anzeigen (aus: Pruefprogramme; auch mit `METTGINE_HIDDEN=1`)
    pub visible: bool,
}

impl AppConfig {
    /// Fenster 1280 x 720, hoechstens 0,1 s je Bild.
    pub fn new(id: &'static str, title: &str, version: &'static str) -> Self {
        Self {
            id,
            title: title.to_string(),
            version,
            size: (1280.0, 720.0),
            max_dt: 0.1,
            block_input_on_splash: true,
            visible: std::env::var_os("METTGINE_HIDDEN").is_none(),
        }
    }
}

/// Ein Spiel. Nur [`Game::renderer`] und [`Game::frame`] sind Pflicht.
pub trait Game {
    /// Startet das Spiel im randlosen Vollbild?
    fn starts_fullscreen(&self) -> bool {
        false
    }

    /// Renderer fuer das frische Fenster bauen (`Renderer::new` mit den eigenen Modellen).
    fn renderer(&mut self, window: &Window) -> Result<Renderer>;

    /// Fenster und Renderer stehen, gleich beginnt das erste Bild.
    fn ready(&mut self, _stage: &mut Stage) {}

    /// Ein Bild: `dt` Sekunden sind vergangen. Hier rechnen und [`Stage::draw`] aufrufen.
    fn frame(&mut self, stage: &mut Stage, dt: f32);

    /// Fenster-Ereignis (Tasten, Maus, Fokus). [`Stage::input`] ist schon auf dem neuen Stand.
    fn event(&mut self, _stage: &mut Stage, _event: &WindowEvent) {}

    /// Mausbewegung in rohen Schritten (Kamera); nur waehrend die Maus gefangen ist.
    fn mouse_motion(&mut self, _stage: &mut Stage, _dx: f32, _dy: f32) {}

    /// Einmal pro Sekunde: Angaben fuer Absturzberichte sammeln (`crash::context`).
    fn crash_context(&self) {}

    /// Einmal pro Sekunde: Status fuer den Launcher. `None` = nichts melden.
    fn status(&self) -> Option<Status> {
        None
    }
}

/// Startet ein Spiel und kehrt zurueck, wenn es beendet ist. Richtet Protokoll und
/// Absturzberichte ein; als Absturzfenster gestartet, zeigt es nur dieses. `make` baut das
/// Spiel erst danach, damit auch Fehler beim Aufbau im Absturzbericht landen.
pub fn run<G: Game>(config: AppConfig, make: impl FnOnce() -> Result<G>) -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(c) = crash_window::parse_args(&args) {
        crash_window::run(&c, &config.title);
        return Ok(());
    }
    crash::init(config.id, &config.title, config.version);
    handler::run(config, make()?)
}

impl Stage {
    /// Zeichnet ein Bild. Scheitert es, erscheint das Absturzfenster und das Spiel endet.
    pub fn draw(&mut self, input: &FrameInput) {
        if let Err(e) = self.renderer.draw(input) {
            crash::fatal(
                "Zeichnen",
                &format!("Bild konnte nicht gezeichnet werden: {e}"),
                "Beim Zeichnen ist ein Grafikfehler aufgetreten. Ein Neustart hilft oft, sonst \
                 den Grafiktreiber aktualisieren.",
            );
            self.quit();
        }
    }
}
