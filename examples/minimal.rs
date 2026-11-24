//! Das kleinste Spiel: ein Boden, ein Wuerfel, Kamera mit Tasten oder Stick, Esc beendet.

use mettgine::app::{self, AppConfig, Game, Stage};
use mettgine::input::{Bind, Key, Pad, Stick};
use mettgine::render::{FrameInput, MeshDraw, Renderer};
use mettgine::shapes;
use winit::window::Window;

const LEFT: &[Bind] = &[Bind::Key(Key::ArrowLeft), Bind::Stick(Stick::Left)];
const RIGHT: &[Bind] = &[Bind::Key(Key::ArrowRight), Bind::Stick(Stick::Right)];
const QUIT: &[Bind] = &[Bind::Key(Key::Escape), Bind::Pad(Pad::Select)];

#[derive(Default)]
struct Minimal {
    angle: f32,
    time: f32,
}

impl Game for Minimal {
    fn renderer(&mut self, window: &Window) -> app::Result<Renderer> {
        Renderer::simple(window, &[shapes::cube(), shapes::plane(20.0)])
    }

    fn frame(&mut self, stage: &mut Stage, dt: f32) {
        if stage.input.pressed(QUIT) {
            stage.quit();
        }
        self.time += dt;
        self.angle += dt * stage.input.held(RIGHT) as u8 as f32;
        self.angle -= dt * stage.input.held(LEFT) as u8 as f32;
        let eye = glam::vec3(self.angle.sin() * 6.0, 3.0, self.angle.cos() * 6.0);
        let cube =
            glam::Mat4::from_rotation_y(self.time) * glam::Mat4::from_translation(glam::Vec3::Y);
        stage.draw(&FrameInput {
            time: self.time,
            meshes: &[
                MeshDraw::new(0, cube).tint(glam::vec3(0.9, 0.3, 0.2)),
                MeshDraw::new(1, glam::Mat4::IDENTITY).tint(glam::vec3(0.4, 0.6, 0.3)),
            ],
            ..FrameInput::looking(eye, glam::Vec3::Y, stage.aspect())
        });
    }
}

fn main() -> app::Result<()> {
    let config = AppConfig::new("minimal", "Minimal", env!("CARGO_PKG_VERSION"));
    app::run(config, || Ok(Minimal::default()))
}
