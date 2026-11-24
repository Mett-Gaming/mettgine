//! Erfolge ausprobieren (siehe `docs/erfolge.md`): meldet drei Erfolge, sobald ein Launcher da
//! ist. Esc beendet; mit `ERFOLGE_SEKUNDEN` von selbst.

use mettgine::app::{self, AppConfig, Game, Stage};
use mettgine::input::{Bind, Key};
use mettgine::render::{FrameInput, MeshDraw, Renderer};
use mettgine::shapes;
use winit::window::Window;

const QUIT: &[Bind] = &[Bind::Key(Key::Escape)];

struct Demo {
    time: f32,
    limit: Option<f32>,
}

impl Game for Demo {
    fn renderer(&mut self, window: &Window) -> app::Result<Renderer> {
        Renderer::simple(window, &[shapes::cube(), shapes::plane(20.0)])
    }

    fn frame(&mut self, stage: &mut Stage, dt: f32) {
        // Bei offenem Overlay anhalten
        if !stage.overlay_open() {
            self.time += dt;
        }
        if stage.input.pressed(QUIT) || self.limit.is_some_and(|l| self.time > l) {
            stage.quit();
        }
        if self.time > 1.0 {
            stage.achievements.unlock("gestartet");
        }
        stage.achievements.progress("sekunden", self.time as u32);
        if self.time > 4.0 {
            stage.achievements.unlock("geduldig");
        }
        let spin = glam::Mat4::from_rotation_y(self.time);
        let eye = glam::vec3(0.0, 3.0, 6.0);
        stage.draw(&FrameInput {
            time: self.time,
            meshes: &[
                MeshDraw::new(0, spin * glam::Mat4::from_translation(glam::Vec3::Y))
                    .tint(glam::vec3(0.9, 0.3, 0.2)),
                MeshDraw::new(1, glam::Mat4::IDENTITY).tint(glam::vec3(0.4, 0.6, 0.3)),
            ],
            ..FrameInput::looking(eye, glam::Vec3::Y, stage.aspect())
        });
    }
}

fn main() -> app::Result<()> {
    let limit = std::env::var("ERFOLGE_SEKUNDEN").ok();
    let limit = limit.and_then(|s| s.parse().ok());
    let config = AppConfig::new("erfolge", "Erfolge", env!("CARGO_PKG_VERSION"));
    app::run(config, || Ok(Demo { time: 0.0, limit }))
}
