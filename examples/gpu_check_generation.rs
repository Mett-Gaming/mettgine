//! Prueft die Bilderzeugung (DLSS Frame Generation) auf einer echten Grafikkarte: ein- und
//! ausschalten, Groesse aendern, mit und ohne DLSS. Das Fenster liegt ausserhalb des Bildschirms und
//! braucht den Fokus: Ohne ihn erzeugt die Technik nichts, geprueft wird dann nur der Ablauf.
//!
//! Aufruf: `cargo run --release --features full --example gpu_check_generation`

use glam::{Mat4, Vec3};
use mettgine::render::*;
use mettgine::shapes;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

fn shot(r: &mut Renderer, upscaler: Upscaler, n: u32) -> Result<()> {
    r.begin_frame()?;
    let t = n as f32 * 0.02;
    let cube = Mat4::from_translation(Vec3::new(t.sin() * 2.0, 1.0, 0.0));
    let eye = Vec3::new(t.cos() * 2.0, 2.5, 7.0);
    let scene = SceneInput {
        features: Features {
            upscaler,
            render_scale: if upscaler == Upscaler::Own { 1.0 } else { 0.5 },
            ..Features::default()
        },
        ids: MotionIds {
            meshes: &[1, 0],
            ..Default::default()
        },
        cut: n == 0,
        ..SceneInput::new(Camera::looking(eye, Vec3::Y, r.aspect()))
    };
    r.draw(&FrameInput {
        meshes: &[MeshDraw::new(0, cube), MeshDraw::new(1, Mat4::IDENTITY)],
        post: Post::Plain(0.0),
        scene: Some(&scene),
        ..Default::default()
    })?;
    // etwa 70 gerechnete Bilder pro Sekunde
    std::thread::sleep(Duration::from_millis(12));
    Ok(())
}

fn check(window: &Window) -> Result<()> {
    let font = mettgine::ui::font_atlas();
    let meshes = [shapes::cube(), shapes::plane(20.0)];
    let config = Config {
        upscalers: &[Upscaler::Dlss],
        frame_generation: true,
        low_latency: true,
        ..Config::new(&meshes, &[], (&font, 128, 128))
    };
    let mut r = Renderer::new(window, &config)?;
    let found = r.upscalers();
    println!(
        "{}: hoechstens {} erzeugte Bilder, {:?}",
        r.gpu_name(),
        r.frame_generation_max(),
        found
    );
    r.set_vsync(false);
    while r.splash_active() {
        shot(&mut r, Upscaler::Own, 0)?;
    }
    if r.frame_generation_max() == 0 {
        println!("keine Bilderzeugung: uebersprungen");
        return Ok(());
    }
    let most = r.frame_generation_max();
    for (frames, upscaler) in [
        (1, Upscaler::Own),
        (0, Upscaler::Own),
        (1, Upscaler::Dlss),
        (most, Upscaler::Dlss),
        (0, Upscaler::Dlss),
    ] {
        r.set_frame_generation(frames);
        let start = Instant::now();
        let mut best = 0;
        for n in 0..180 {
            if n == 60 {
                let s = window.inner_size();
                r.resize(s.width, s.height);
            }
            shot(&mut r, upscaler, n)?;
            if let Some(g) = r.frame_generation() {
                best = best.max(g.presented);
            }
        }
        println!(
            "{frames} erzeugt, {upscaler:?}: {:.1} gerechnete B/s, {:?}, hoechstens {best} gezeigt",
            180.0 / start.elapsed().as_secs_f32(),
            r.frame_generation()
        );
        let status = r.frame_generation().map_or(0, |g| g.status);
        if status != 0 {
            return Err(format!("Bilderzeugung meldet Fehler {status}").into());
        }
        if frames > 0 && best < 2 {
            println!("  nichts erzeugt (Fenster nicht im Vordergrund?): nicht beurteilt");
        }
    }
    Ok(())
}

#[derive(Default)]
struct App {
    done: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if std::mem::replace(&mut self.done, true) {
            return;
        }
        let attrs = Window::default_attributes()
            .with_active(true)
            .with_decorations(false)
            .with_position(winit::dpi::PhysicalPosition::new(-9000, 100))
            .with_inner_size(winit::dpi::PhysicalSize::new(1280, 720));
        let window = el.create_window(attrs).expect("Fenster");
        window.focus_window();
        match check(&window) {
            Ok(()) => println!("Alles in Ordnung."),
            Err(e) => {
                eprintln!("FEHLER: {e}");
                std::process::exit(1);
            }
        }
        el.exit();
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, _e: WindowEvent) {}
}

fn main() {
    let el = EventLoop::new().expect("Ereignisschleife");
    el.run_app(&mut App::default()).expect("Ablauf");
}
