//! Dichte Testbilder wie in den Spielen, mit Bildzeit: ein Markt (viele feste Waren, Boxen, Lampen,
//! Figuren, Videoband) und eine Strasse (viele Modelle, Sonne mit Schatten). Unsichtbares Fenster.
//!
//! `gpu_bench <ordner> [shader-ordner]` legt Bilder ab bzw. vergleicht mit vorhandenen und nennt die Bildzeiten.

use glam::{Mat4, Vec3, Vec4};
use mettgine::gpu::{GpuInstance, GpuLight, MeshData, MeshVertex, SkinData, SkinVertex};
use mettgine::render::*;
use std::path::{Path, PathBuf};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Kleiner Koerper aus `n` Seiten.
fn solid(n: usize, color: [f32; 3]) -> MeshData {
    let at = |i: usize, y: f32| {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        Vec3::new(a.cos() * 0.5, y, a.sin() * 0.5)
    };
    let mut vertices = Vec::new();
    for i in 0..n {
        let (a, b, c, d) = (at(i, 0.0), at(i + 1, 0.0), at(i + 1, 1.0), at(i, 1.0));
        for t in [[a, c, b], [a, d, c], [d, Vec3::Y, c]] {
            let normal = (t[1] - t[0]).cross(t[2] - t[0]).normalize().to_array();
            vertices.extend(t.map(|p| MeshVertex {
                pos: p.to_array(),
                normal,
                color,
            }));
        }
    }
    MeshData {
        vertices,
        min: Vec3::new(-0.5, 0.0, -0.5),
        max: Vec3::new(0.5, 1.0, 0.5),
    }
}

fn person() -> SkinData {
    let m = solid(24, [0.6, 0.5, 0.4]);
    let v = |v: &MeshVertex| SkinVertex {
        pos: [v.pos[0] * 0.6, v.pos[1] * 1.8, v.pos[2] * 0.6],
        normal: v.normal,
        color: v.color,
        joints: [0.0, 1.0, 0.0, 0.0],
        weights: [1.0 - v.pos[1], v.pos[1], 0.0, 0.0],
    };
    SkinData {
        vertices: m.vertices.iter().map(v).collect(),
    }
}

fn at(x: f32, y: f32, z: f32, s: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z)) * Mat4::from_scale(Vec3::splat(s))
}

fn draw_of(mesh: usize, model: Mat4) -> MeshDraw {
    MeshDraw {
        mesh,
        model,
        tint: Vec4::new(0.9, 0.8, 0.7, 0.0),
        ..Default::default()
    }
}

/// Regale mit 18000 Waren, 1500 Boxen, 40 Lampen.
fn market() -> (Vec<MeshDraw>, Vec<GpuInstance>, Vec<GpuLight>) {
    let (mut goods, mut boxes, mut lights) = (Vec::new(), Vec::new(), Vec::new());
    let mut add = |c: [f32; 3], h: [f32; 3], material: f32| {
        boxes.push(GpuInstance {
            center: [c[0], c[1], c[2], 0.0],
            half: [h[0], h[1], h[2], 0.0],
            color: [0.7, 0.68, 0.62, 1.0],
            extra: [material, 0.0, 0.9, 0.0],
        });
    };
    add([20.0, -0.1, 20.0], [22.0, 0.1, 22.0], 1.0);
    add([20.0, 4.1, 20.0], [22.0, 0.1, 22.0], 2.0);
    for aisle in 0..12 {
        for part in 0..25 {
            for shelf in 0..5 {
                let (x, z) = (aisle as f32 * 3.2 + 2.0, part as f32 * 1.4 + 3.0);
                let y = shelf as f32 * 0.45 + 0.1;
                add([x, y, z], [0.45, 0.02, 0.68], 0.0);
                for k in 0..12 {
                    let (gx, gz) = (x + (k % 2) as f32 * 0.3 - 0.15, z + k as f32 * 0.1 - 0.6);
                    goods.push(draw_of((aisle + shelf * 3) % 8, at(gx, y + 0.03, gz, 0.18)));
                }
            }
        }
    }
    for i in 0..40 {
        lights.push(GpuLight {
            pos: [
                (i % 8) as f32 * 5.0 + 2.0,
                3.8,
                (i / 8) as f32 * 8.0 + 3.0,
                8.0,
            ],
            color: [1.0, 0.96, 0.9, 0.8],
            extra: [i as f32, 0.2, 0.0, 0.0],
        });
    }
    (goods, boxes, lights)
}

struct Scene {
    meshes: Vec<MeshDraw>,
    skins: Vec<SkinDraw>,
    bones: Vec<Mat4>,
}

fn people(count: usize) -> Scene {
    let skins = (0..count)
        .map(|i| SkinDraw {
            skin: 0,
            model: at(
                (i % 9) as f32 * 4.3 + 3.5,
                0.0,
                (i / 9) as f32 * 5.0 + 4.0,
                1.0,
            ),
            first_bone: i as u32 * 2,
            ..Default::default()
        })
        .collect();
    let bone = |i: usize| Mat4::from_rotation_z((i as f32).sin() * 0.1);
    Scene {
        meshes: Vec::new(),
        skins,
        bones: (0..count * 2).map(bone).collect(),
    }
}

fn input<'a>(s: &'a Scene, street: bool, k: usize) -> FrameInput<'a> {
    let eye = if street {
        Vec3::new(0.0, 4.0, 12.0)
    } else {
        Vec3::new(5.2, 1.7, 2.0 + k as f32)
    };
    let to = if street {
        Vec3::new(0.0, 1.0, -20.0)
    } else {
        eye + Vec3::new(0.15, -0.05, 1.0)
    };
    let aspect = 16.0 / 9.0;
    let lighting = if street {
        Lighting::Sun(Sun {
            dir: Vec3::new(0.4, 1.0, 0.3),
            color: Vec3::new(1.0, 0.95, 0.85),
            shadow_center: Vec3::new(0.0, 0.0, -15.0),
            shadow_radius: 35.0,
            shadow_strength: 0.8,
            sky_top: Vec3::new(0.35, 0.55, 0.9),
            horizon: Vec3::new(0.75, 0.83, 0.92),
            fog_density: 0.004,
            ground: Vec3::new(0.25, 0.23, 0.2),
        })
    } else {
        Lighting::Rooms(Rooms {
            dark: k == 1,
            fog_color: Vec3::new(0.1, 0.11, 0.1),
            fog_density: 0.02,
            decay: 0.3,
            sway: Vec4::new(0.3, 0.15, 40.0, 40.0) * (k as f32 * 0.5),
        })
    };
    let post = if street {
        Post::Clean(Clean {
            fade: 0.0,
            red: 0.0,
            saturation: 1.0,
        })
    } else {
        Post::Vhs(Vhs {
            strength: 1.0,
            dark: k == 1,
            glitch: 0.0,
            fade: 0.0,
            red: 0.0,
            invert: 0.0,
            flash: 0.0,
        })
    };
    FrameInput {
        view: View {
            view_proj: perspective(1.2, aspect, 0.05, 120.0) * Mat4::look_at_rh(eye, to, Vec3::Y),
            pos: eye,
        },
        second_camera: None,
        time: 5.0 + k as f32,
        lighting,
        post,
        meshes: &s.meshes,
        ghosts: &[],
        skins: &s.skins,
        camera_skins: &[],
        bones: &s.bones,
        ui: &[],
        ..Default::default()
    }
}

fn run(window: &Window, dir: &Path) -> std::result::Result<(), String> {
    let e = |e: Box<dyn std::error::Error>| e.to_string();
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let font = vec![255u8; 128 * 128];
    let meshes: Vec<MeshData> = (0..8)
        .map(|i| {
            solid(
                6 + i * 3,
                [0.3 + i as f32 * 0.08, 0.5, 0.7 - i as f32 * 0.05],
            )
        })
        .collect();
    let skins = [person()];
    let own: Option<Vec<Vec<u8>>> = std::env::args_os().nth(2).and_then(|d| {
        ["world.vert.spv", "world.frag.spv"]
            .iter()
            .map(|f| std::fs::read(Path::new(&d).join(f)).ok())
            .collect()
    });
    let world = own
        .as_ref()
        .map_or(WorldShaders::default(), |b| WorldShaders {
            vert: &b[0],
            frag: &b[1],
        });
    let config = Config {
        world,
        last_wins: false,
        ..Config::new(&meshes, &skins, (&font, 128, 128))
    };
    let mut r = Renderer::new(window, &config).map_err(e)?;
    let (goods, boxes, lights) = market();
    r.upload_world(&boxes, &lights).map_err(e)?;
    r.upload_statics(&goods).map_err(e)?;
    let shoppers = people(40);
    let mut street = people(30);
    street.meshes = (0..3000)
        .map(|i| {
            draw_of(
                i % 8,
                at(
                    (i % 50) as f32 * 1.6 - 40.0,
                    0.0,
                    (i / 50) as f32 * -1.5,
                    1.0,
                ),
            )
        })
        .collect();
    while r.splash_active() {
        r.draw(&input(&shoppers, false, 0)).map_err(e)?;
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    for (name, scene, is_street, k) in [
        ("markt0", &shoppers, false, 0),
        ("markt1", &shoppers, false, 1),
        ("strasse", &street, true, 0),
    ] {
        if is_street {
            r.upload_world(&[], &[]).map_err(e)?;
            r.upload_statics(&[]).map_err(e)?;
        }
        let (mut cpu, mut gpu) = (0.0f32, 0.0f32);
        for i in 0..240 {
            r.draw(&input(scene, is_street, k)).map_err(e)?;
            if i >= 40 {
                cpu += r.stats().cpu_ms;
                gpu += r.stats().gpu_ms;
            }
        }
        let (want, new) = (
            dir.join(format!("{name}.bmp")),
            dir.join(format!("{name}.neu.bmp")),
        );
        r.capture_next(new.clone());
        r.draw(&input(scene, is_street, k)).map_err(e)?;
        let s = r.stats();
        println!(
            "{name}: CPU {:.3} ms, GPU {:.3} ms, Befehle {}",
            cpu / 200.0,
            gpu / 200.0,
            s.draw_calls
        );
        let px = std::fs::read(&new).map_err(|e| e.to_string())?;
        match std::fs::read(&want) {
            Ok(old) if old == px => std::fs::remove_file(&new).map_err(|e| e.to_string())?,
            Ok(_) => return Err(format!("{name} weicht ab")),
            Err(_) => std::fs::rename(&new, &want).map_err(|e| e.to_string())?,
        }
    }
    Ok(())
}

struct App(PathBuf, Option<std::result::Result<(), String>>);

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_visible(false)
            .with_decorations(false)
            .with_inner_size(winit::dpi::PhysicalSize::new(1920, 1080));
        self.1 = Some(
            el.create_window(attrs)
                .map_err(|e| e.to_string())
                .and_then(|w| run(&w, &self.0)),
        );
        el.exit();
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

fn main() {
    let Some(dir) = std::env::args_os().nth(1) else {
        eprintln!("Aufruf: gpu_bench <ordner>");
        std::process::exit(2);
    };
    let mut app = App(dir.into(), None);
    let run = EventLoop::new().and_then(|l| l.run_app(&mut app));
    match (run, app.1) {
        (Ok(()), Some(Ok(()))) => println!("Alles in Ordnung."),
        (run, result) => {
            eprintln!("FEHLER: {run:?} {result:?}");
            std::process::exit(1);
        }
    }
}
