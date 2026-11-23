//! Prueft den Szenen-Renderer auf einer echten Grafikkarte, ohne Spieldaten und ohne sichtbares
//! Fenster: zeichnet kleine Testszenen und vergleicht die Bildpunkte selbst.
//!
//! Aufruf: `cargo run --release --example gpu_check` (Ende mit Fehler, wenn etwas nicht stimmt).

use glam::{Mat4, Vec3, Vec4};
use mettgine::gpu::{MeshData, MeshVertex, SkinData, SkinVertex};
use mettgine::particles::ParticleInst;
use mettgine::scene::{perspective, MeshDraw, SceneParams, SceneRenderer, SkinDraw, UiVertex};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

const W: u32 = 320;
const H: u32 = 180;

/// Wuerfel mit halber Kante `h` als Dreiecke.
fn cube(h: f32) -> MeshData {
    let mut vertices = Vec::new();
    for axis in 0..3 {
        for side in [-1.0f32, 1.0] {
            let mut n = [0.0; 3];
            n[axis] = side;
            for (a, b) in [(-1, -1), (1, -1), (1, 1), (-1, -1), (1, 1), (-1, 1)] {
                let mut pos = [0.0; 3];
                pos[axis] = side * h;
                pos[(axis + 1) % 3] = a as f32 * h;
                pos[(axis + 2) % 3] = b as f32 * h;
                vertices.push(MeshVertex {
                    pos,
                    normal: n,
                    color: [0.9, 0.3, 0.2],
                });
            }
        }
    }
    MeshData {
        vertices,
        min: Vec3::splat(-h),
        max: Vec3::splat(h),
    }
}

fn params() -> SceneParams {
    let cam_pos = Vec3::new(0.0, 4.0, -10.0);
    let view = Mat4::look_at_rh(cam_pos, Vec3::new(0.3, 0.5, 4.0), Vec3::Y);
    SceneParams {
        view_proj: perspective(1.1, W as f32 / H as f32, 0.1, 200.0) * view,
        cam_pos,
        time: 1.0,
        sun_dir: Vec3::new(0.4, 1.0, -0.3),
        sun_color: Vec3::new(1.0, 0.95, 0.8),
        shadow_center: Vec3::new(0.0, 0.0, 5.0),
        shadow_radius: 20.0,
        shadow_strength: 0.8,
        sky_top: Vec3::new(0.3, 0.5, 0.9),
        horizon: Vec3::new(0.7, 0.8, 0.9),
        fog_density: 0.005,
        ground: Vec3::new(0.2, 0.2, 0.2),
        fade: 0.0,
        red: 0.0,
        saturation: 1.0,
    }
}

/// Ein Testbild: alles, was der Renderer braucht.
#[derive(Default)]
struct Shot {
    meshes: Vec<MeshDraw>,
    skins: Vec<SkinDraw>,
    bones: Vec<Mat4>,
    particles: Vec<ParticleInst>,
    ui: Vec<UiVertex>,
}

impl Shot {
    /// Boden, viele Wuerfel rundum (die meisten ausserhalb des Bildes), eine Figur.
    fn scene() -> Self {
        let mut s = Shot::default();
        let floor = Mat4::from_translation(Vec3::new(0.0, -1.0, 0.0))
            * Mat4::from_scale(Vec3::new(60.0, 1.0, 60.0));
        s.meshes
            .push(MeshDraw::new(0, floor).tint(Vec3::splat(0.6)));
        for i in 0..400 {
            let a = i as f32 * 2.399;
            let r = 1.5 + i as f32 * 0.09;
            let pos = Vec3::new(a.cos() * r, 0.6, a.sin() * r);
            let m = Mat4::from_translation(pos) * Mat4::from_rotation_y(a);
            s.meshes
                .push(MeshDraw::new(i % 2, m * Mat4::from_scale(Vec3::splat(0.5))));
        }
        s.bones = vec![Mat4::from_translation(Vec3::new(1.0, 1.0, 0.0)); 2];
        s.skins.push(SkinDraw {
            skin: 0,
            model: Mat4::from_translation(Vec3::new(-3.0, 0.5, -1.0)),
            tint: Vec4::new(0.2, 1.0, 0.3, 0.2),
            first_bone: 0,
            blink: 0.0,
        });
        s
    }
}

struct Check {
    renderer: SceneRenderer,
    path: std::path::PathBuf,
}

impl Check {
    fn draw(&mut self, s: &Shot) -> Result<(), String> {
        self.renderer.queue_particles(&s.particles);
        self.renderer
            .draw(&params(), &s.meshes, &s.skins, &s.bones, &s.ui)
            .map_err(|e| format!("Zeichnen: {e}"))
    }

    /// Ein Bild zeichnen und als BGR-Bildpunkte liefern.
    fn one(&mut self, s: &Shot) -> Result<Vec<[u8; 3]>, String> {
        let _ = std::fs::remove_file(&self.path);
        self.renderer.capture_next(self.path.clone());
        self.draw(s)?;
        let bmp = std::fs::read(&self.path).map_err(|e| format!("kein Bild: {e}"))?;
        let _ = std::fs::remove_file(&self.path);
        Ok(bmp[54..].chunks(4).map(|p| [p[0], p[1], p[2]]).collect())
    }

    /// Zeichnet dreimal (alle Bilder im Flug) und liefert das letzte Bild.
    fn shot(&mut self, s: &Shot) -> Result<Vec<[u8; 3]>, String> {
        self.draw(s)?;
        self.draw(s)?;
        self.one(s)
    }

    /// Startbild abspielen; prueft ein Bild mittendrin.
    fn check_splash(&mut self) -> Result<(), String> {
        let start = std::time::Instant::now();
        let mut seen = false;
        while self.renderer.splash_active() {
            let now = start.elapsed().as_secs_f32();
            ensure(now < 30.0, "Startbild endet nicht")?;
            if !seen && now > 1.6 {
                seen = true;
                let px = self.one(&Shot::default())?;
                let cream = px.iter().filter(|p| p.iter().all(|c| *c > 190)).count();
                ensure(cream > 300, &format!("Schriftzug fehlt ({cream})"))?;
                let back = px
                    .iter()
                    .filter(|p| p[2].abs_diff(18) < 2 && p[0].abs_diff(11) < 2);
                ensure(
                    back.count() > px.len() / 2,
                    "Startbild: falscher Hintergrund",
                )?;
            } else {
                self.draw(&Shot::default())?;
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        println!("Startbild: {:.1} s", start.elapsed().as_secs_f32());
        ensure(
            seen && start.elapsed().as_secs_f32() > 2.0,
            "Startbild zu kurz",
        )
    }

    fn run(&mut self) -> Result<(), String> {
        self.check_splash()?;
        let scene = Shot::scene();
        let on = self.shot(&scene)?;
        let st = self.renderer.stats();
        println!("Messwerte: {st:?}");
        ensure(st.culled > 100, "es wird nichts aussortiert")?;
        ensure(st.dropped == 0, "Objekte fehlen")?;
        ensure(st.gpu_ms > 0.0, "keine GPU-Zeiten")?;
        self.renderer.set_culling(false);
        let off = self.shot(&scene)?;
        ensure(
            self.renderer.stats().culled == 0,
            "Aussortieren nicht abschaltbar",
        )?;
        ensure(on == off, "Aussortieren veraendert das Bild")?;
        self.renderer.set_culling(true);
        let greens = on
            .iter()
            .filter(|p| p[1] as i32 > p[2] as i32 + 40 && p[1] as i32 > p[0] as i32 + 40);
        ensure(greens.count() > 50, "Figur fehlt")?;

        self.check_msaa(&scene, &on)?;
        self.check_particles(&scene, &on)?;
        self.check_growth()?;
        self.check_font()
    }

    fn check_msaa(&mut self, scene: &Shot, plain: &[[u8; 3]]) -> Result<(), String> {
        self.renderer.set_msaa(4);
        let n = self.renderer.msaa();
        let smooth = self.shot(scene)?;
        println!(
            "Kantenglaettung: {n}x, Farben {} -> {}",
            colors(plain),
            colors(&smooth)
        );
        ensure(n == 1 || smooth != plain, "Kantenglaettung ohne Wirkung")?;
        let diff = plain
            .iter()
            .zip(&smooth)
            .filter(|(a, b)| (0..3).any(|k| a[k].abs_diff(b[k]) > 12))
            .count();
        ensure(
            diff < plain.len() / 4,
            "Kantenglaettung veraendert mehr als Kanten",
        )?;
        self.renderer.set_msaa(1);
        ensure(
            self.shot(scene)? == plain,
            "nach dem Abschalten anders als vorher",
        )
    }

    fn check_particles(&mut self, scene: &Shot, plain: &[[u8; 3]]) -> Result<(), String> {
        // Mitte des Bildes, kurz vor der Kamera
        let at = |additive: f32, color: [f32; 4]| ParticleInst {
            pos: Vec3::new(0.05, 3.3, -8.0).to_array(),
            size: 0.4,
            color,
            extra: [0.3, additive, 0.5, 0.0],
        };
        let mid = |px: &[[u8; 3]]| px[(H / 2 * W + W / 2) as usize];
        for msaa in [1, 4] {
            self.renderer.set_msaa(msaa);
            let mut s = Shot {
                particles: vec![at(0.0, [0.0, 0.0, 0.0, 1.0])],
                ..Shot::scene()
            };
            let dark = mid(&self.shot(&s)?);
            ensure(
                dark.iter().all(|c| *c < 40),
                &format!("Rauch deckt nicht: {dark:?}"),
            )?;
            s.particles = vec![at(1.0, [0.5, 0.5, 0.5, 1.0])];
            let (before, after) = (mid(plain), mid(&self.shot(&s)?));
            let brighter = (0..3).all(|k| after[k] > before[k] || before[k] > 220);
            ensure(
                brighter,
                &format!("Funken hellen nicht auf: {before:?} {after:?}"),
            )?;
        }
        self.renderer.set_msaa(1);
        ensure(self.shot(scene)? == plain, "Partikel bleiben haengen")
    }

    /// Weit mehr Objekte als der Startplatz.
    fn check_growth(&mut self) -> Result<(), String> {
        let mut s = Shot::scene();
        let one = s.meshes[1];
        s.meshes.resize(30_000, one);
        s.bones.resize(20_000, Mat4::IDENTITY);
        s.particles = vec![ParticleInst::default(); 5_000];
        s.ui = vec![
            UiVertex {
                uv: [-1.0; 2],
                ..Default::default()
            };
            150_000
        ];
        self.shot(&s)?;
        ensure(self.renderer.stats().dropped == 0, "Puffer wachsen nicht")
    }

    /// Stark verkleinertes Schachbrett-Zeichen: mit Stufen gleichmaessig grau.
    fn check_font(&mut self) -> Result<(), String> {
        let v = |x: f32, y: f32| UiVertex {
            pos: [x * 100.0, y * 100.0],
            uv: [x, y],
            color: [1.0; 4],
        };
        let c = [(0., 0.), (1., 0.), (1., 1.), (0., 0.), (1., 1.), (0., 1.)];
        let s = Shot {
            ui: c.iter().map(|&(x, y)| v(x, y)).collect(),
            ..Shot::scene()
        };
        let plain = self.shot(&s)?;
        self.renderer.set_font_mipmaps(true);
        let soft = self.shot(&s)?;
        // Ecke oben links (BMP: unterste Zeile zuerst), 20x20 Bildpunkte
        let spread = |px: &[[u8; 3]]| {
            let it =
                (2..20u32).flat_map(|y| (2..20u32).map(move |x| ((H - 1 - y) * W + x) as usize));
            let v: Vec<u8> = it.map(|i| px[i][1]).collect();
            v.iter().max().unwrap() - v.iter().min().unwrap()
        };
        println!("Schrift: Streuung {} -> {}", spread(&plain), spread(&soft));
        ensure(
            spread(&soft) <= spread(&plain),
            "Schriftstufen ohne Wirkung",
        )?;
        ensure(soft != plain, "Schriftstufen ohne Wirkung")
    }
}

fn colors(px: &[[u8; 3]]) -> usize {
    px.iter().collect::<std::collections::HashSet<_>>().len()
}

fn ensure(ok: bool, what: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(what.to_string())
    }
}

struct App(Option<Result<(), String>>);

impl App {
    fn check(el: &ActiveEventLoop) -> Result<(), String> {
        let attrs = Window::default_attributes()
            .with_visible(false)
            .with_decorations(false)
            .with_inner_size(winit::dpi::PhysicalSize::new(W, H));
        let window = el.create_window(attrs).map_err(|e| e.to_string())?;
        let vert = |x: f32, y: f32| SkinVertex {
            pos: [x, y, 0.0],
            normal: [0.0, 0.0, -1.0],
            color: [1.0; 3],
            joints: [0.0, 1.0, 0.0, 0.0],
            weights: [0.5, 0.5, 0.0, 0.0],
        };
        let skin = SkinData {
            vertices: vec![vert(-1.0, 0.0), vert(1.0, 0.0), vert(0.0, 2.0)],
        };
        let n = 512u32;
        let font: Vec<u8> = (0..n * n)
            .map(|i| ((i % n + i / n) % 2 * 255) as u8)
            .collect();
        let meshes = [cube(1.0), cube(0.8)];
        let renderer = SceneRenderer::new(&window, &meshes, &[skin], (&font, n, n), 1.0)
            .map_err(|e| format!("Renderer: {e}"))?;
        println!("{}", renderer.gpu_name());
        let path =
            std::env::temp_dir().join(format!("mettgine_gpu_check_{}.bmp", std::process::id()));
        let result = Check { renderer, path }.run();
        drop(window);
        result
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.0 = Some(Self::check(el));
        el.exit();
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

fn main() {
    let mut app = App(None);
    let run = EventLoop::new().and_then(|l| l.run_app(&mut app));
    match (run, app.0) {
        (Ok(()), Some(Ok(()))) => println!("Alles in Ordnung."),
        (run, result) => {
            eprintln!("FEHLER: {run:?} {result:?}");
            std::process::exit(1);
        }
    }
}
