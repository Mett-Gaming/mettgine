//! Prueft die Bewegungsvektoren auf einer echten Grafikkarte, ohne sichtbares Fenster: Bewegte
//! Modelle und Figuren duerfen mit TAA und Kennungen weniger Schlieren ziehen als ohne. Mit
//! Raytracing muessen Figuren ausserdem von Strahlen getroffen werden.
//!
//! Aufruf: `cargo run --release --example gpu_check_motion <ordner>`

use glam::{Mat4, Vec3};
use mettgine::gpu::{MeshVertex, SkinData, SkinVertex};
use mettgine::render::*;
use mettgine::shapes;
use std::path::{Path, PathBuf};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

const W: u32 = 1280;
const H: u32 = 720;
const FRAMES: u32 = 30;

/// Wuerfel mit kleinen Farbfeldern, oben an einem zweiten Knochen.
fn figure() -> SkinData {
    let cube = shapes::cube();
    let v = |(i, v): (usize, &MeshVertex)| SkinVertex {
        pos: v.pos,
        normal: v.normal,
        color: [(i % 3) as f32 * 0.5, (i % 2) as f32, 0.4],
        joints: [0.0, 1.0, 0.0, 0.0],
        weights: if v.pos[1] > 0.0 {
            [0.0, 1.0, 0.0, 0.0]
        } else {
            [1.0, 0.0, 0.0, 0.0]
        },
    };
    SkinData {
        vertices: cube.vertices.iter().enumerate().map(v).collect(),
    }
}

fn shot(r: &mut Renderer, aa: AntiAlias, ids: bool, n: u32, path: Option<&Path>) -> Result<()> {
    if let Some(p) = path {
        r.capture_next(p.to_path_buf());
    }
    let t = n as f32;
    let x = (t * 0.12) % 8.0 - 4.0;
    let mesh = MeshDraw::new(
        0,
        Mat4::from_translation(Vec3::new(x, 1.0, 0.0)) * Mat4::from_rotation_y(t * 0.05),
    )
    .tint(Vec3::new(0.9, 0.2, 0.1));
    let bones = [
        Mat4::IDENTITY,
        Mat4::from_translation(Vec3::new((t * 0.3).sin() * 0.6, 0.0, 0.0)),
    ];
    let skin = SkinDraw {
        model: Mat4::from_translation(Vec3::new(-x, 0.6, 2.0)),
        ..Default::default()
    };
    let eye = Vec3::new(0.0, 2.5, 8.0);
    let camera = Camera::looking(eye, Vec3::new(0.0, 1.0, 0.0), W as f32 / H as f32);
    let scene = SceneInput {
        features: Features {
            anti_alias: aa,
            ..Features::default()
        },
        ids: if ids {
            MotionIds {
                meshes: &[1],
                skins: &[1],
                ..Default::default()
            }
        } else {
            MotionIds::default()
        },
        cut: n == 0,
        ..SceneInput::new(camera)
    };
    r.draw(&FrameInput {
        meshes: &[mesh],
        skins: &[skin],
        bones: &bones,
        post: Post::Plain(0.0),
        scene: Some(&scene),
        ..Default::default()
    })
}

fn pixels(path: &Path) -> Result<Vec<u8>> {
    Ok(std::fs::read(path)?[54..].to_vec())
}

fn difference(a: &[u8], b: &[u8]) -> f64 {
    let d = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as f64);
    d.sum::<f64>() / a.len() as f64
}

/// Bild aus Strahlen, mit oder ohne Figur.
fn ray_shot(r: &mut Renderer, rays: bool, figure: bool, n: u32, path: Option<&Path>) -> Result<()> {
    if let Some(p) = path {
        r.capture_next(p.to_path_buf());
    }
    let floor = MeshDraw::new(1, Mat4::IDENTITY);
    let t = n as f32 * 0.1;
    let bones = [
        Mat4::IDENTITY,
        Mat4::from_translation(Vec3::new(t.sin() * 0.4, 0.5, 0.0)),
    ];
    let skin = SkinDraw {
        model: Mat4::from_translation(Vec3::new(0.0, 0.6, 0.0)),
        ..Default::default()
    };
    let skins = [skin];
    let eye = Vec3::new(1.0, 2.0, 4.0);
    let camera = Camera::looking(eye, Vec3::new(0.0, 0.8, 0.0), W as f32 / H as f32);
    let scene = SceneInput {
        features: Features {
            ray_tracing: RayTracing {
                reflections: rays,
                sun_shadows: rays,
                ..Default::default()
            },
            path_tracing: (!rays).then(PathTracing::default),
            ..Features::default()
        },
        plain_roughness: 0.1,
        cut: n == 0,
        ..SceneInput::new(camera)
    };
    r.draw(&FrameInput {
        meshes: &[floor],
        skins: if figure { &skins } else { &[] },
        bones: &bones,
        post: Post::Plain(0.0),
        scene: Some(&scene),
        ..Default::default()
    })
}

fn check_rays(window: &Window, dir: &Path) -> Result<()> {
    let font = mettgine::ui::font_atlas();
    let (meshes, skins) = ([shapes::cube(), shapes::plane(20.0)], [figure()]);
    let config = Config {
        ray_tracing: true,
        ..Config::new(&meshes, &skins, (&font, 128, 128))
    };
    let mut r = Renderer::new(window, &config)?;
    if !r.ray_tracing() {
        println!("kein Raytracing: uebersprungen");
        return Ok(());
    }
    while r.splash_active() {
        ray_shot(&mut r, true, false, 0, None)?;
    }
    for rays in [true, false] {
        let mut images = Vec::new();
        for figure in [false, true] {
            let path = dir.join(format!("figur_{rays}_{figure}.bmp"));
            for n in 0..FRAMES {
                let last = (n == FRAMES - 1).then_some(path.as_path());
                ray_shot(&mut r, rays, figure, n, last)?;
            }
            images.push(pixels(&path)?);
        }
        let d = difference(&images[0], &images[1]);
        println!("Strahlen {rays}: Figur aendert das Bild um {d:.3}");
        if d < 1.5 {
            return Err("Figur fehlt im Bild aus Strahlen".into());
        }
    }
    Ok(())
}

fn check(window: &Window, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let font = mettgine::ui::font_atlas();
    let (meshes, skins) = ([shapes::cube()], [figure()]);
    let mut r = Renderer::new(window, &Config::new(&meshes, &skins, (&font, 128, 128)))?;
    while r.splash_active() {
        shot(&mut r, AntiAlias::None, false, 0, None)?;
    }
    let mut images = Vec::new();
    for (name, aa, ids) in [
        ("ohne", AntiAlias::None, false),
        ("taa", AntiAlias::Taa, false),
        ("taa_kennung", AntiAlias::Taa, true),
    ] {
        let path = dir.join(format!("bewegung_{name}.bmp"));
        for n in 0..FRAMES {
            let last = (n == FRAMES - 1).then_some(path.as_path());
            shot(&mut r, aa, ids, n, last)?;
        }
        images.push(pixels(&path)?);
    }
    let (without, with) = (
        difference(&images[0], &images[1]),
        difference(&images[0], &images[2]),
    );
    println!("Abstand zum Bild ohne TAA: ohne Kennung {without:.4}, mit Kennung {with:.4}");
    if with > without * 0.8 {
        return Err("Kennungen verringern die Schlieren nicht".into());
    }
    Ok(())
}

struct App {
    dir: PathBuf,
    done: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if std::mem::replace(&mut self.done, true) {
            return;
        }
        let attrs = Window::default_attributes()
            .with_visible(false)
            .with_inner_size(winit::dpi::PhysicalSize::new(W, H));
        let window = el.create_window(attrs).expect("Fenster");
        match check(&window, &self.dir).and_then(|()| check_rays(&window, &self.dir)) {
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
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let mut app = App {
        dir: dir.into(),
        done: false,
    };
    let el = EventLoop::new().expect("Ereignisschleife");
    el.run_app(&mut app).expect("Ablauf");
}
