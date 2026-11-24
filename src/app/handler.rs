//! Verbindung zu winit: baut Fenster und Renderer auf und reicht Ereignisse weiter.

use super::{AppConfig, Game, Result, Stage};
use crate::crash;
use crate::launcher::{self, Achievements, Overlay, Route};
use crate::presence::Presence;
use crate::time::FrameClock;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId};

/// Abstand der Meldungen an Absturzberichte und Launcher (Sekunden)
const REPORT_EVERY: f32 = 1.0;

struct Handler<G> {
    // Reihenfolge wichtig: die Buehne (Renderer, Fenster) vor dem Spiel freigeben.
    stage: Option<Stage>,
    game: G,
    config: AppConfig,
    clock: FrameClock,
    presence: Presence,
    achievements: Achievements,
    overlay: Overlay,
    report_in: f32,
}

pub(super) fn run<G: Game>(config: AppConfig, game: G) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let (achievements, overlay) = launcher::connect();
    let mut handler = Handler {
        stage: None,
        game,
        clock: FrameClock::new(config.max_dt),
        presence: Presence::from_launcher(),
        achievements,
        overlay,
        report_in: 0.0,
        config,
    };
    event_loop.run_app(&mut handler)?;
    Ok(())
}

impl<G: Game> Handler<G> {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Option<Stage> {
        let full = self.game.starts_fullscreen();
        let attrs = Window::default_attributes()
            .with_title(&self.config.title)
            .with_visible(self.config.visible)
            .with_inner_size(LogicalSize::new(self.config.size.0, self.config.size.1))
            .with_fullscreen(full.then_some(Fullscreen::Borderless(None)));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => w,
            Err(e) => {
                crash::fatal(
                    "Fenster",
                    &format!("Fenster konnte nicht erstellt werden: {e}"),
                    "Das Spielfenster konnte nicht geöffnet werden.",
                );
                return None;
            }
        };
        match self.game.renderer(&window) {
            Ok(renderer) => {
                crash::context("GPU", renderer.gpu_name());
                crash::log(&format!("GPU: {}", renderer.gpu_name()));
                Some(Stage::new(renderer, window, self.achievements.clone()))
            }
            Err(e) => {
                crash::fatal(
                    "Vulkan",
                    &format!("Vulkan-Initialisierung fehlgeschlagen: {e}"),
                    &format!(
                        "Die Grafik konnte nicht gestartet werden. {} braucht eine Grafikkarte \
                         mit Vulkan 1.3. Bitte den Grafiktreiber aktualisieren.",
                        self.config.title
                    ),
                );
                None
            }
        }
    }

    /// Einmal pro Sekunde: Absturzbericht und Launcher auf den neuesten Stand bringen.
    fn report(&mut self, dt: f32) {
        self.report_in -= dt;
        if self.report_in > 0.0 {
            return;
        }
        self.report_in = REPORT_EVERY;
        self.game.crash_context();
        crash::context("Bild", format!("{:.1} ms", dt * 1000.0));
        if let Some(s) = self.game.status() {
            self.presence.set(s);
        }
    }

    fn redraw(&mut self) {
        let dt = self.clock.tick();
        self.report(dt);
        if let Some(stage) = &mut self.stage {
            stage.set_overlay(self.overlay.open());
            stage.renderer.input_sampled();
            self.game.frame(stage, dt);
            stage.input.begin_frame();
            self.overlay.frame(&mut stage.input);
        }
    }
}

/// Eingabe, die waehrend des Startbilds verfaellt?
fn is_input(e: &WindowEvent) -> bool {
    matches!(
        e,
        WindowEvent::KeyboardInput { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
    )
}

impl<G: Game> ApplicationHandler for Handler<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.stage.is_some() {
            return;
        }
        let Some(mut stage) = self.open(event_loop) else {
            event_loop.exit();
            return;
        };
        self.game.ready(&mut stage);
        self.stage = Some(stage);
        self.clock.restart();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(stage) = &mut self.stage else {
            return;
        };
        if self.config.block_input_on_splash && is_input(&event) && stage.splash_active() {
            return;
        }
        let size = stage.size();
        if self.overlay.event(&event, (size.width, size.height)) == Route::Swallow {
            return;
        }
        stage.input.event(&event);
        match &event {
            WindowEvent::CloseRequested => stage.quit(),
            WindowEvent::Resized(size) => stage.renderer.resize(size.width, size.height),
            WindowEvent::CursorMoved { position, .. } => stage.moved(position.x, position.y),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
        if let Some(stage) = &mut self.stage {
            if !matches!(event, WindowEvent::RedrawRequested) {
                self.game.event(stage, &event);
            }
            if stage.quitting() {
                event_loop.exit();
            }
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let (DeviceEvent::MouseMotion { delta }, Some(stage)) = (event, &mut self.stage) {
            if stage.overlay_open() {
                return;
            }
            let (dx, dy) = (delta.0 as f32, delta.1 as f32);
            stage.input.mouse_moved(dx, dy);
            if stage.grabbed() {
                self.game.mouse_motion(stage, dx, dy);
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Vor dem Einsammeln der Eingaben warten; sie treffen danach noch vor dem Zeichnen ein
        if let Some(s) = &mut self.stage {
            if let Err(e) = s.renderer.begin_frame() {
                crash::log(&format!("Warten auf das naechste Bild: {e}"));
            }
        }
        // Unsichtbare Fenster bekommen von Windows keine Zeichen-Ereignisse
        if !self.config.visible {
            self.redraw();
            if self.stage.as_ref().is_some_and(Stage::quitting) {
                event_loop.exit();
            }
        } else if let Some(s) = &self.stage {
            s.window.request_redraw();
        }
    }
}
