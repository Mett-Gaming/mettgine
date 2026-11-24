//! Zeichnet Testbilder mit den Welt-Materialien (Boxen, Lichter, Monitor, Hologramme, Figuren,
//! Oberflaeche, VHS) ohne Spieldaten und ohne sichtbares Fenster.
//!
//! `gpu_check_world <ordner>` legt die Bilder als BMP ab; liegen dort schon welche, muessen die
//! neuen Byte fuer Byte gleich sein (Vergleich vor und nach einem Umbau).

use glam::{Mat4, Vec3, Vec4};
use mettgine::gpu::{GpuInstance, GpuLight, MeshData, MeshVertex, SkinData, SkinVertex};
use mettgine::render::{
    Config, FrameInput, Lighting, MeshDraw, Post, Renderer, Resolution, Rooms, SkinDraw, UiSize,
    UiVertex, Vhs, View,
};
use std::path::{Path, PathBuf};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

fn vert(pos: [f32; 3], normal: [f32; 3]) -> MeshVertex {
    MeshVertex {
        pos,
        normal,
        color: [0.8, 0.6, 0.3],
    }
}

/// Pyramide aus vier Dreiecken.
fn pyramid() -> MeshData {
    let base = [
        [-0.5, 0.0, -0.5],
        [0.5, 0.0, -0.5],
        [0.5, 0.0, 0.5],
        [-0.5, 0.0, 0.5],
    ];
    let mut vertices = Vec::new();
    for i in 0..4 {
        let (a, b) = (base[i], base[(i + 1) % 4]);
        let n = [(a[0] + b[0]) * 0.7, 0.5, (a[2] + b[2]) * 0.7];
        vertices.extend([vert(a, n), vert(b, n), vert([0.0, 1.0, 0.0], n)]);
    }
    MeshData {
        vertices,
        min: Vec3::new(-0.5, 0.0, -0.5),
        max: Vec3::new(0.5, 1.0, 0.5),
    }
}

fn skin() -> SkinData {
    let v = |x: f32, y: f32| SkinVertex {
        pos: [x, y, 0.0],
        normal: [0.0, 0.0, -1.0],
        color: [0.4, 0.7, 0.9],
        joints: [0.0, 1.0, 0.0, 0.0],
        weights: [1.0 - y * 0.5, y * 0.5, 0.0, 0.0],
    };
    let (a, b, c, d) = (v(-0.4, 0.0), v(0.4, 0.0), v(0.4, 2.0), v(-0.4, 2.0));
    SkinData {
        vertices: vec![a, b, c, a, c, d],
    }
}

/// Raum mit allen Materialien 0 bis 7.
fn world() -> (Vec<GpuInstance>, Vec<GpuLight>) {
    let b = |c: [f32; 4], h: [f32; 3], color: [f32; 4], extra: [f32; 4]| GpuInstance {
        center: c,
        half: [h[0], h[1], h[2], 0.0],
        color,
        extra,
    };
    let mut v = vec![
        b(
            [0.0, -0.1, 8.0, 0.0],
            [9.0, 0.1, 12.0],
            [0.7, 0.7, 0.65, 1.0],
            [1.0, 0.0, 0.0, 0.0],
        ),
        b(
            [0.0, 4.1, 8.0, 0.0],
            [9.0, 0.1, 12.0],
            [0.8, 0.8, 0.8, 1.0],
            [2.0, 0.0, 0.0, 0.0],
        ),
    ];
    for m in 0..8 {
        let x = m as f32 * 1.5 - 5.25;
        let extra = [
            m as f32,
            m as f32 * 0.25,
            0.9,
            if m == 3 { 0.6 } else { 0.0 },
        ];
        v.push(b(
            [x, 1.4, 6.0 + (m % 3) as f32, m as f32 * 0.1],
            [0.6, 0.7, 0.2],
            [0.8, 0.3, 0.25, 1.0],
            extra,
        ));
    }
    let light = |x: f32, z: f32, f: f32| GpuLight {
        pos: [x, 3.6, z, 9.0],
        color: [1.0, 0.95, 0.85, 0.9],
        extra: [x, f, 0.0, 0.0],
    };
    (
        v,
        vec![
            light(-3.0, 4.0, 0.0),
            light(3.0, 8.0, 0.8),
            light(0.0, 13.0, 0.3),
        ],
    )
}

fn params(k: usize) -> FrameInput<'static> {
    let eye = Vec3::new(0.4, 1.7, -1.5);
    let cam = |eye: Vec3, at: Vec3, fov: f32| {
        Mat4::perspective_rh(fov, 16.0 / 9.0, 0.05, 80.0) * Mat4::look_at_rh(eye, at, Vec3::Y)
    };
    let dark = k == 2;
    let view = |eye: Vec3, at: Vec3, fov: f32| View {
        view_proj: cam(eye, at, fov),
        pos: eye,
    };
    FrameInput {
        view: view(eye, Vec3::new(0.0, 1.3, 7.0), 1.2),
        second_camera: (k != 1)
            .then(|| view(Vec3::new(4.0, 3.5, 14.0), Vec3::new(0.0, 0.5, 6.0), 1.0)),
        time: 12.34 + k as f32,
        lighting: Lighting::Rooms(Rooms {
            dark,
            fog_color: Vec3::new(0.12, 0.13, 0.12),
            fog_density: if dark { 0.09 } else { 0.02 },
            decay: k as f32 * 0.45,
            sway: Vec4::new(0.3, 0.15, 20.0, 9.0) * (k as f32 * 0.5),
        }),
        post: Post::Vhs(Vhs {
            strength: [1.0, 0.0, 1.0][k],
            dark,
            glitch: if dark { 0.7 } else { 0.0 },
            fade: 0.1,
            red: if dark { 0.5 } else { 0.0 },
            invert: if dark { 0.3 } else { 0.0 },
            flash: 0.05,
        }),
        meshes: &[],
        ghosts: &[],
        skins: &[],
        camera_skins: &[],
        bones: &[],
        ui: &[],
        scene: None,
    }
}

fn run(window: &Window, dir: &Path) -> Result<(), String> {
    let e = |e: Box<dyn std::error::Error>| e.to_string();
    let pattern = |w: u32, h: u32| -> Vec<u8> {
        (0..w * h)
            .map(|i| ((i % w / 3 + i / w / 5) % 2 * 255) as u8)
            .collect()
    };
    let (font, signs) = (pattern(128, 128), pattern(512, 64));
    let (meshes, skins) = ([pyramid()], [skin()]);
    let config = Config {
        signs: Some((&signs, 512, 64)),
        resolution: Resolution::Fixed(480, 270),
        ui: UiSize::Fixed(480, 270),
        second_camera: Some((160, 90)),
        smooth: false,
        last_wins: false,
        ..Config::new(&meshes, &skins, (&font, 128, 128))
    };
    let mut r = Renderer::new(window, &config).map_err(e)?;
    let (boxes, lights) = world();
    r.upload_world(&boxes, &lights).map_err(e)?;
    let at = |x: f32, z: f32| Mat4::from_translation(Vec3::new(x, 0.0, z));
    let mesh = |model: Mat4, w: f32| MeshDraw {
        model,
        tint: Vec4::new(1.0, 0.9, 0.8, w),
        ..Default::default()
    };
    let statics: Vec<_> = (0..30)
        .map(|i| mesh(at(i as f32 * 0.5 - 7.0, 9.0 + (i % 4) as f32), 0.0))
        .collect();
    r.upload_statics(&statics).map_err(e)?;
    let meshes: Vec<_> = (0..60)
        .map(|i| mesh(at((i % 9) as f32 - 4.0, i as f32 * 0.6 - 12.0), 0.1))
        .collect();
    let ghosts = [mesh(at(1.5, 4.0), 0.4), mesh(at(0.0, -9.0), 0.4)];
    let person = |x: f32, ghost: bool, distort: f32| SkinDraw {
        skin: 0,
        model: at(x, 3.5),
        tint: Vec4::new(1.0, 1.0, 1.0, 0.1),
        first_bone: 0,
        ghost,
        distort,
        ..Default::default()
    };
    let people = [
        person(-1.5, false, 0.0),
        person(-0.5, true, 0.0),
        person(2.4, false, 0.8),
        person(60.0, false, 0.0),
    ];
    let monitor = [person(0.8, false, 0.0)];
    let bones = [Mat4::IDENTITY, Mat4::from_rotation_z(0.3)];
    let ui: Vec<_> = [
        (20.0, 20.0),
        (120.0, 20.0),
        (120.0, 60.0),
        (300.0, 200.0),
        (420.0, 200.0),
        (420.0, 250.0),
    ]
    .iter()
    .enumerate()
    .map(|(i, &(x, y))| UiVertex {
        pos: [x, y],
        uv: if i < 3 {
            [-1.0, -1.0]
        } else {
            [x / 480.0, y / 270.0]
        },
        color: [0.9, 0.8, 0.2, 0.85],
    })
    .collect();
    let shot = |r: &mut Renderer, k: usize, path: Option<PathBuf>| {
        if let Some(p) = path {
            r.capture_next(p);
        }
        r.draw(&FrameInput {
            meshes: &meshes,
            ghosts: &ghosts,
            skins: &people,
            camera_skins: &monitor,
            bones: &bones,
            ui: &ui,
            ..params(k)
        })
        .map_err(e)
    };
    while r.splash_active() {
        shot(&mut r, 0, None)?;
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for k in 0..3 {
        let want = dir.join(format!("welt{k}.bmp"));
        let new = dir.join(format!("welt{k}.neu.bmp"));
        for i in 0..3 {
            shot(&mut r, k, (i == 2).then(|| new.clone()))?;
        }
        let px = std::fs::read(&new).map_err(|e| format!("kein Bild: {e}"))?;
        let colors = px[54..]
            .chunks(4)
            .collect::<std::collections::HashSet<_>>()
            .len();
        println!("Bild {k}: {colors} Farben, {:?}", r.stats());
        if colors < 100 {
            return Err(format!("Bild {k} ist fast leer"));
        }
        match std::fs::read(&want) {
            Ok(old) if old == px => std::fs::remove_file(&new).map_err(|e| e.to_string())?,
            Ok(_) => return Err(format!("Bild {k} weicht ab ({})", new.display())),
            Err(_) => std::fs::rename(&new, &want).map_err(|e| e.to_string())?,
        }
    }
    Ok(())
}

struct App(PathBuf, Option<Result<(), String>>);

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_visible(false)
            .with_decorations(false)
            .with_inner_size(winit::dpi::PhysicalSize::new(960, 540));
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
        eprintln!("Aufruf: gpu_check_world <ordner>");
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
