//! Prueft Lampenschatten und Verdunkelung in Ecken ohne Spieldaten und ohne sichtbares Fenster.
//!
//! `gpu_check_lamps <ordner>` legt Bilder ab und vergleicht Helligkeiten: Unter einem Tisch muss
//! es dunkler werden, freier Boden gleich bleiben, und beim Vorbeigehen darf nichts springen.

use glam::{Mat4, Vec3, Vec4};
use mettgine::gpu::{GpuInstance, GpuLight};
use mettgine::render::{
    Config, FrameInput, LampShadows, Lighting, MeshDraw, Post, Renderer, Resolution, Rooms, View,
};
use std::path::{Path, PathBuf};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

const W: usize = 960;
const H: usize = 540;

fn block(c: [f32; 3], h: [f32; 3], material: f32) -> GpuInstance {
    GpuInstance {
        center: [c[0], c[1], c[2], 0.0],
        half: [h[0], h[1], h[2], 0.0],
        color: [0.75, 0.75, 0.75, 1.0],
        extra: [material, 0.0, 0.0, 0.0],
    }
}

/// Boden, Decke, ein Tisch und eine Reihe Lampen.
fn world() -> (Vec<GpuInstance>, Vec<GpuLight>) {
    let boxes = vec![
        block([0.0, -0.1, 0.0], [20.0, 0.1, 6.0], 0.0),
        block([0.0, 3.6, 0.0], [20.0, 0.1, 6.0], 0.0),
        block([0.0, 1.0, 0.0], [0.8, 0.05, 0.8], 0.0),
    ];
    let lights = (0..12)
        .map(|i| GpuLight {
            pos: [i as f32 * 3.0 - 18.0, 3.0, 0.0, 7.5],
            color: [1.0, 1.0, 1.0, 0.7],
            extra: [0.0; 4],
        })
        .collect();
    (boxes, lights)
}

fn input(eye: Vec3, at: Vec3, sway: Vec4, time: f32) -> FrameInput<'static> {
    FrameInput {
        view: View {
            view_proj: Mat4::perspective_rh(1.2, 16.0 / 9.0, 0.05, 80.0)
                * Mat4::look_at_rh(eye, at, Vec3::Y),
            pos: eye,
        },
        time,
        lighting: Lighting::Rooms(Rooms {
            dark: false,
            fog_color: Vec3::ZERO,
            fog_density: 0.0,
            decay: 0.0,
            sway,
        }),
        post: Post::Plain(0.0),
        ..Default::default()
    }
}

struct Picture(Vec<u8>, usize, usize);

impl Picture {
    /// Mittlere Helligkeit um einen Punkt (0..1 im Bild).
    fn at(&self, x: f32, y: f32) -> f32 {
        let (px, py) = ((x * self.1 as f32) as usize, (y * self.2 as f32) as usize);
        let mut sum = 0.0;
        for dy in 0..9 {
            for dx in 0..9 {
                let row = self.2 - 1 - (py + dy).min(self.2 - 1);
                let o = (row * self.1 + (px + dx).min(self.1 - 1)) * 4;
                sum += self.0[o] as f32 + self.0[o + 1] as f32 + self.0[o + 2] as f32;
            }
        }
        sum / (81.0 * 3.0)
    }
}

/// BMP mit 32 Bit je Bildpunkt lesen (Zeilen von unten, sonst umdrehen).
fn read(path: &Path) -> Result<Picture, String> {
    let b = std::fs::read(path).map_err(|e| format!("kein Bild: {e}"))?;
    let int = |o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let (w, h) = (int(18) as usize, int(22));
    let start = int(10) as usize;
    let rows = h.unsigned_abs() as usize;
    let mut px = b[start..].to_vec();
    if h < 0 {
        px = px.chunks(w * 4).rev().flatten().copied().collect();
    }
    Ok(Picture(px, w, rows))
}

fn run(window: &Window, dir: &Path) -> Result<(), String> {
    let e = |e: Box<dyn std::error::Error>| e.to_string();
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let font = mettgine::ui::font_atlas();
    let meshes = [mettgine::shapes::cube()];
    let config = Config {
        resolution: Resolution::Fixed(W as u32, H as u32),
        ..Config::new(&meshes, &[], (&font, 128, 128))
    };
    let mut r = Renderer::new(window, &config).map_err(e)?;
    let (boxes, lights) = world();
    r.upload_world(&boxes, &lights).map_err(e)?;
    let eye = Vec3::new(0.0, 1.7, 4.5);
    let at = Vec3::new(0.0, 0.6, 0.0);
    let calm = input(eye, at, Vec4::ZERO, 1.0);
    while r.splash_active() {
        r.draw(&calm).map_err(e)?;
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    // ein bewegtes Modell neben dem Tisch
    let cube = [MeshDraw::new(
        0,
        Mat4::from_translation(Vec3::new(2.2, 1.2, 0.0)) * Mat4::from_scale(Vec3::splat(0.6)),
    )];
    let shot = |r: &mut Renderer, name: &str, f: FrameInput| -> Result<Picture, String> {
        let path = dir.join(name);
        for i in 0..8 {
            if i == 7 {
                r.capture_next(path.clone());
            }
            let f = FrameInput { meshes: &cube, ..f };
            r.draw(&f).map_err(|e| e.to_string())?;
        }
        read(&path)
    };
    let off = shot(&mut r, "aus.bmp", calm)?;
    let shadows = LampShadows {
        count: 4,
        strength: 1.0,
        ..Default::default()
    };
    r.set_lamp_shadows(shadows).map_err(e)?;
    let on = shot(&mut r, "schatten.bmp", calm)?;
    println!("Schatten: {:?}", r.stats().passes);
    // unter dem Tisch, unter dem Wuerfel, freier Boden vorn
    let table = project(&calm, Vec3::new(0.0, 0.0, 0.0));
    let under_cube = project(&calm, Vec3::new(2.2, 0.0, 0.2));
    let free = project(&calm, Vec3::new(-1.6, 0.0, 2.6));
    let drop = |p: (f32, f32)| on.at(p.0, p.1) / off.at(p.0, p.1).max(1.0);
    println!(
        "Tisch {:.2}, Wuerfel {:.2}, frei {:.2}",
        drop(table),
        drop(under_cube),
        drop(free)
    );
    if drop(table) > 0.85 || drop(under_cube) > 0.92 {
        return Err("kein Schatten unter Tisch oder Wuerfel".into());
    }
    if drop(free) < 0.97 {
        return Err("freier Boden ist dunkler geworden".into());
    }
    // Decke und Tischplatte duerfen sich nicht selbst beschatten
    let top = project(&calm, Vec3::new(0.3, 1.05, 0.2));
    if drop(top) < 0.95 {
        return Err(format!(
            "Tischplatte beschattet sich selbst: {:.2}",
            drop(top)
        ));
    }
    let contact = LampShadows {
        contact: 0.8,
        ..Default::default()
    };
    r.set_lamp_shadows(contact).map_err(e)?;
    let corner = shot(&mut r, "ecken.bmp", calm)?;
    let same = corner.at(free.0, free.1) / off.at(free.0, free.1).max(1.0);
    println!("Ecken: freier Boden {same:.2}");
    if corner.0 == off.0 || same < 0.9 {
        return Err("Verdunkelung in Ecken wirkt nicht oder ueberall".into());
    }
    // schwankende Welt: laeuft durch
    r.set_lamp_shadows(shadows).map_err(e)?;
    let wave = Vec4::new(0.3, 0.15, 30.0, 20.0);
    for k in 0..60 {
        r.draw(&input(eye, at, wave, k as f32 * 0.016)).map_err(e)?;
    }
    // Vorbeigehen: fester Punkt im Bild, Kamera schaut senkrecht von oben mit
    let mut before: Option<f32> = None;
    let mut worst = 0.0f32;
    for k in 0..40 {
        let x = k as f32 * 0.1 - 2.0;
        let f = input(
            Vec3::new(x, 2.5, 2.5),
            Vec3::new(-4.0, 0.0, 0.0),
            Vec4::ZERO,
            1.0,
        );
        let p = project(&f, Vec3::new(-4.0, 0.0, 0.5));
        let path = dir.join("gang.bmp");
        r.draw(&f).map_err(e)?;
        r.capture_next(path.clone());
        r.draw(&f).map_err(e)?;
        let v = read(&path)?.at(p.0, p.1);
        if let Some(b) = before {
            worst = worst.max((v - b).abs());
        }
        before = Some(v);
    }
    println!("groesster Sprung beim Gehen: {worst:.1} von 255");
    if worst > 12.0 {
        return Err("Schatten springen beim Gehen".into());
    }
    // zurueck auf aus: genau wie vorher
    r.set_lamp_shadows(LampShadows::default()).map_err(e)?;
    if shot(&mut r, "wieder_aus.bmp", calm)?.0 != off.0 {
        return Err("nach dem Ausschalten sieht es anders aus".into());
    }
    Ok(())
}

fn project(f: &FrameInput, p: Vec3) -> (f32, f32) {
    let q = f.view.view_proj.project_point3(p);
    (q.x * 0.5 + 0.5, 0.5 - q.y * 0.5)
}

struct App(PathBuf, Option<Result<(), String>>);

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_visible(false)
            .with_decorations(false)
            .with_inner_size(winit::dpi::PhysicalSize::new(W as u32, H as u32));
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
        eprintln!("Aufruf: gpu_check_lamps <ordner>");
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
