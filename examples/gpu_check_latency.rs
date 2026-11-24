//! Prueft Reflex, Anti-Lag und Bildbegrenzung auf einer echten Grafikkarte, ohne sichtbares
//! Fenster. Mit `METTGINE_GPU=<Nummer oder Name>` laeuft es auf einer anderen Karte, mit
//! `METTGINE_LATENCY=wait` ohne die Wege der Hersteller.
//!
//! Aufruf: `cargo run --release --example gpu_check_latency`

use glam::{vec3, Mat4, Vec3};
use mettgine::render::*;
use mettgine::shapes;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Bilder je Messung
const FRAMES: u32 = 180;

fn check(window: &Window) -> Result<()> {
    let font = mettgine::ui::font_atlas();
    let meshes = [shapes::cube(), shapes::plane(20.0)];
    let mut r = Renderer::new(
        window,
        &Config {
            low_latency: true,
            diagnostics: true,
            ..Config::new(&meshes, &[], (&font, 128, 128))
        },
    )?;
    println!("{}: {:?}", r.gpu_name(), r.latency_tech());
    r.set_vsync(false);
    for (mode, fps) in [
        (LowLatency::Off, 0.0),
        (LowLatency::Off, 120.0),
        (LowLatency::On, 0.0),
        (LowLatency::On, 120.0),
        (LowLatency::Boost, 90.0),
    ] {
        r.set_low_latency(mode);
        r.set_fps_limit(fps);
        // Einschwingen, dabei einmal die Swapchain neu bauen
        let (mut start, mut n) = (Instant::now(), 0);
        for i in 0..FRAMES + 60 {
            if i == 20 {
                let s = window.inner_size();
                r.resize(s.width, s.height);
            }
            if i == 60 {
                (start, n) = (Instant::now(), 0);
            }
            r.begin_frame()?;
            let eye = vec3((i as f32 * 0.02).sin() * 6.0, 3.0, 6.0);
            r.draw(&FrameInput {
                meshes: &[
                    MeshDraw::new(0, Mat4::from_translation(Vec3::Y)),
                    MeshDraw::new(1, Mat4::IDENTITY),
                ],
                ..FrameInput::looking(eye, Vec3::Y, r.aspect())
            })?;
            n += 1;
        }
        let rate = n as f32 / start.elapsed().as_secs_f32();
        println!(
            "{mode:?}, Grenze {fps}: {rate:.1} B/s, {:?}",
            r.latency_report()
        );
        if fps > 0.0 && !(fps * 0.85..=fps * 1.08).contains(&rate) {
            return Err(format!("Grenze {fps} nicht eingehalten: {rate:.1} B/s").into());
        }
        if mode != LowLatency::Off
            && r.latency_tech() == LatencyTech::Reflex
            && r.latency_report().is_none()
        {
            return Err("Reflex liefert keine Messwerte".into());
        }
    }
    let s = r.stats();
    println!(
        "Grafikspeicher {} von {} MB",
        s.memory_mb, s.memory_budget_mb
    );
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
        let attrs = Window::default_attributes().with_visible(false);
        let window = el.create_window(attrs).expect("Fenster");
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
