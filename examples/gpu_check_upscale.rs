//! Prueft das Hochskalieren mit Techniken der Hersteller auf einer echten Grafikkarte, ohne
//! sichtbares Fenster: vergleicht mit einem Bild in voller Aufloesung, ruhig und bei Kamerafahrt.
//! Die DLLs liegen neben der exe oder in einem Ordner aus `METTGINE_VENDOR`.
//!
//! Aufruf: `cargo run --release --features full --example gpu_check_upscale <ordner>`

use glam::{Mat4, Vec3};
use mettgine::particles::ParticleInst;
use mettgine::render::*;
use mettgine::shapes;
use std::path::{Path, PathBuf};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

const W: u32 = 1280;
const H: u32 = 720;
const FRAMES: u32 = 60;

struct World {
    objects: Vec<ObjectDraw>,
    ids: Vec<u32>,
}

/// Feines Muster: Kanten und Linien, an denen schlechtes Hochskalieren auffaellt.
fn pattern() -> TextureData {
    let n = 256u32;
    let rgba = (0..n * n)
        .flat_map(|i| {
            let (x, y) = (i % n, i / n);
            let line = x % 32 < 2 || y % 32 < 2 || (x + y) % 64 < 2;
            if line {
                [30, 30, 40, 255]
            } else {
                [200, 180, 150, 255]
            }
        })
        .collect();
    TextureData {
        width: n,
        height: n,
        rgba,
        srgb: true,
        mips: true,
    }
}

fn build(r: &mut Renderer) -> Result<World> {
    let tex = r.add_texture(&pattern())?;
    let plane = r.add_model(&ModelData::from_mesh(&shapes::plane(40.0)).box_uvs(0.5))?;
    let cube = r.add_model(&ModelData::from_mesh(&shapes::cube()).box_uvs(1.0))?;
    let ball = r.add_model(&ModelData::from_mesh(&shapes::sphere(32)))?;
    let wall = r.add_material(&Material {
        base_texture: Some(tex),
        ..Default::default()
    })?;
    let mirror = r.add_material(&Material {
        metallic: 1.0,
        roughness: 0.2,
        ..Default::default()
    })?;
    let mut objects = vec![ObjectDraw::new(plane, wall, Mat4::IDENTITY)];
    for i in 0..40 {
        let (x, z) = ((i % 8) as f32 * 1.6 - 5.6, (i / 8) as f32 * -2.2);
        let h = 0.6 + (i * 7 % 5) as f32 * 0.35;
        let m = Mat4::from_translation(Vec3::new(x, h * 0.5, z))
            * Mat4::from_rotation_y(i as f32 * 0.4)
            * Mat4::from_scale(Vec3::new(0.15 + (i % 3) as f32 * 0.3, h, 0.4));
        objects.push(ObjectDraw::new(cube, wall, m));
    }
    for i in 0..4 {
        let m = Mat4::from_translation(Vec3::new(i as f32 * 2.0 - 3.0, 0.4, 2.5))
            * Mat4::from_scale(Vec3::splat(0.8));
        objects.push(ObjectDraw::new(ball, mirror, m));
    }
    let ids = (1..=objects.len() as u32).collect();
    Ok(World { objects, ids })
}

#[derive(Clone, Copy)]
struct Case {
    upscaler: Upscaler,
    scale: f32,
    aa: AntiAlias,
    moving: bool,
    paths: Option<PathTracing>,
    rr: bool,
}

fn shot(r: &mut Renderer, w: &World, c: &Case, n: u32, path: Option<&Path>) -> Result<()> {
    if let Some(p) = path {
        r.capture_next(p.to_path_buf());
    }
    // die Fahrt endet immer an derselben Stelle
    let back = if c.moving {
        (FRAMES - 1 - n) as f32 * 0.03
    } else {
        0.0
    };
    let eye = Vec3::new(1.0 + back, 2.2, 6.5 + back * 0.3);
    let target = Vec3::new(back, 0.6, -2.0);
    let scene = SceneInput {
        objects: &w.objects,
        ids: MotionIds {
            objects: &w.ids,
            ..Default::default()
        },
        features: Features {
            tonemap: Tonemap::Aces,
            anti_alias: c.aa,
            render_scale: c.scale,
            upscaler: c.upscaler,
            path_tracing: c.paths,
            ray_reconstruction: c.rr,
            sun_shadow_size: 2048,
            ..Features::default()
        },
        cut: n == 0,
        ..SceneInput::new(Camera::looking(eye, target, W as f32 / H as f32))
    };
    r.queue_particles(&[ParticleInst {
        pos: [2.4, 0.6, 3.4],
        size: 0.4,
        color: [0.9, 0.9, 0.9, 0.5],
        extra: [0.3, 0.0, 0.8, 0.0],
    }]);
    r.draw(&FrameInput {
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

fn check(window: &Window, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let font = mettgine::ui::font_atlas();
    let wanted = [Upscaler::Fsr, Upscaler::Xess, Upscaler::Dlss];
    let config = Config {
        upscalers: &wanted,
        ray_tracing: true,
        ..Config::new(&[], &[], (&font, 128, 128))
    };
    let mut r = Renderer::new(window, &config)?;
    let found = r.upscalers();
    println!("{}: {found:?}", r.gpu_name());
    let world = build(&mut r)?;
    let mut case = Case {
        upscaler: Upscaler::Own,
        scale: 1.0,
        aa: AntiAlias::Taa,
        moving: false,
        paths: None,
        rr: false,
    };
    while r.splash_active() {
        shot(&mut r, &world, &case, 0, None)?;
    }
    let run = |r: &mut Renderer, name: &str, c: &Case| -> Result<Vec<u8>> {
        let path = dir.join(format!("hoch_{name}.bmp"));
        for n in 0..FRAMES {
            let last = (n == FRAMES - 1).then_some(path.as_path());
            shot(r, &world, c, n, last)?;
        }
        println!("  {name}: GPU {:.2} ms", r.stats().gpu_ms);
        pixels(&path)
    };
    // Vergleichsbild: doppelte Aufloesung, geglaettet und verkleinert
    r.set_render_scale(2.0);
    let best = run(&mut r, "voll", &case)?;
    r.set_render_scale(1.0);
    for scale in [1.0, 0.5] {
        for moving in [false, true] {
            let tag = format!("{scale}_{}", if moving { "fahrt" } else { "still" });
            (case.moving, case.scale) = (moving, scale);
            (case.upscaler, case.aa) = (Upscaler::Own, AntiAlias::None);
            let plain = difference(&best, &run(&mut r, &format!("roh_{tag}"), &case)?);
            case.aa = AntiAlias::Taa;
            let own = difference(&best, &run(&mut r, &format!("eigen_{tag}"), &case)?);
            println!("{tag}: roh {plain:.3}, eigen {own:.3}");
            for (u, _) in &found {
                case.upscaler = *u;
                let d = difference(&best, &run(&mut r, &format!("{u:?}_{tag}"), &case)?);
                println!("{tag}: {u:?} {d:.3}");
                if scale < 1.0 && !moving && d > own {
                    return Err(format!("{u:?} ist schlechter als das eigene TAA").into());
                }
            }
        }
    }
    if r.ray_tracing() && found.iter().any(|f| f.0 == Upscaler::Dlss) {
        check_denoising(&mut r, &run)?;
    }
    Ok(())
}

/// Pathtracing: eigenes Entrauschen gegen Ray Reconstruction, Vergleich mit langem Mitteln.
fn check_denoising(
    r: &mut Renderer,
    run: &dyn Fn(&mut Renderer, &str, &Case) -> Result<Vec<u8>>,
) -> Result<()> {
    let mut case = Case {
        upscaler: Upscaler::Own,
        scale: 1.0,
        aa: AntiAlias::None,
        moving: false,
        paths: Some(PathTracing {
            samples: 32,
            accumulate: true,
            ..Default::default()
        }),
        rr: false,
    };
    let best = run(r, "pfade_lang", &case)?;
    case.paths = Some(PathTracing::default());
    for (scale, moving) in [(1.0, false), (1.0, true), (0.5, false), (0.5, true)] {
        (case.scale, case.moving) = (scale, moving);
        let tag = format!("pfade_{scale}_{moving}");
        (case.upscaler, case.aa, case.rr) = (Upscaler::Own, AntiAlias::Taa, false);
        let own = difference(&best, &run(r, &format!("{tag}_eigen"), &case)?);
        case.upscaler = Upscaler::Dlss;
        let dlss = difference(&best, &run(r, &format!("{tag}_dlss"), &case)?);
        case.rr = true;
        let rr = difference(&best, &run(r, &format!("{tag}_rr"), &case)?);
        println!("{tag}: eigen {own:.3}, DLSS {dlss:.3}, Ray Reconstruction {rr:.3}");
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
        match check(&window, &self.dir) {
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
