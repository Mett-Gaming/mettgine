//! Prueft die moderne Grafik auf einer echten Grafikkarte, ohne sichtbares Fenster: zeichnet eine
//! Testszene mit einzelnen und allen Zusaetzen, vergleicht Bildpunkte und legt die Bilder als BMP
//! in `<ordner>` ab.
//!
//! Aufruf: `cargo run --release --example gpu_check_modern <ordner>`, mit `METTGINE_HDR=1` ueber
//! die HDR-Ausgabe (die Bilder sind dann umgerechnet)

use glam::{Mat4, Vec3, Vec4};
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

/// Schachbrett mit Fugen: Farbe, Normalen.
fn tiles() -> (TextureData, TextureData) {
    let n = 256u32;
    let (mut color, mut normal) = (Vec::new(), Vec::new());
    for y in 0..n {
        for x in 0..n {
            let (cx, cy) = (x % 64, y % 64);
            let dark = (x / 64 + y / 64) % 2 == 0;
            let joint = cx < 3 || cy < 3;
            let c: [u8; 3] = match (joint, dark) {
                (true, _) => [60, 58, 55],
                (_, true) => [150, 60, 40],
                _ => [210, 200, 180],
            };
            color.extend([c[0], c[1], c[2], 255]);
            let edge = |v: u32| match v {
                0..=2 => -90i32,
                61..=63 => 90,
                _ => 0,
            };
            normal.extend([(128 + edge(cx)) as u8, (128 + edge(cy)) as u8, 220, 255]);
        }
    }
    let t = |rgba, srgb| TextureData {
        width: n,
        height: n,
        rgba,
        srgb,
        mips: true,
    };
    (t(color, true), t(normal, false))
}

/// Gitter mit Loechern.
fn grid() -> TextureData {
    let n = 64u32;
    let rgba = (0..n * n)
        .flat_map(|i| {
            let (x, y) = (i % n, i / n);
            let solid = x % 16 < 5 || y % 16 < 5;
            [70, 130, 60, if solid { 255 } else { 0 }]
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

struct World {
    objects: Vec<ObjectDraw>,
    lights: Vec<Light>,
    logo: TextureId,
    decals: Vec<DecalDraw>,
    meshes: Vec<MeshDraw>,
}

fn build(r: &mut Renderer) -> Result<World> {
    let (color, normal) = tiles();
    let (color, normal) = (r.add_texture(&color)?, r.add_texture(&normal)?);
    let grid = r.add_texture(&grid())?;
    let plane = r.add_model(&ModelData::from_mesh(&shapes::plane(60.0)).box_uvs(0.5))?;
    let sphere = r.add_model(&ModelData::from_mesh(&shapes::sphere(32)))?;
    let cube = r.add_model(&ModelData::from_mesh(&shapes::cube()).box_uvs(1.0))?;
    let floor = r.add_material(&Material {
        base_texture: Some(color),
        normal_texture: Some(normal),
        roughness: 0.45,
        ..Default::default()
    })?;
    let stone = r.add_material(&Material {
        base_color: Vec4::new(0.55, 0.53, 0.5, 1.0),
        roughness: 0.9,
        ..Default::default()
    })?;
    let fence = r.add_material(&Material {
        base_texture: Some(grid),
        alpha: Alpha::Mask(0.5),
        double_sided: true,
        ..Default::default()
    })?;
    let glass = r.add_material(&Material {
        base_color: Vec4::new(0.5, 0.8, 1.0, 0.35),
        roughness: 0.05,
        alpha: Alpha::Blend,
        ..Default::default()
    })?;
    let lamp = r.add_material(&Material {
        emissive: Vec3::new(8.0, 5.0, 2.0),
        ..Default::default()
    })?;
    let at = |x: f32, y: f32, z: f32, s: Vec3| {
        Mat4::from_translation(Vec3::new(x, y, z)) * Mat4::from_scale(s)
    };
    let mut objects = vec![
        ObjectDraw::new(plane, floor, Mat4::IDENTITY),
        ObjectDraw::new(cube, stone, at(-6.0, 1.5, -6.0, Vec3::new(2.0, 3.0, 2.0))),
        ObjectDraw::new(cube, stone, at(6.5, 2.0, -8.0, Vec3::new(3.0, 4.0, 1.0))),
        ObjectDraw::new(cube, stone, at(0.0, 0.5, -9.0, Vec3::new(8.0, 1.0, 1.0))),
        ObjectDraw::new(cube, fence, at(-3.0, 1.0, 0.5, Vec3::new(2.0, 2.0, 0.1))),
        ObjectDraw::new(sphere, glass, at(3.6, 1.0, 1.0, Vec3::splat(1.6))),
        ObjectDraw::new(sphere, lamp, at(4.5, 2.4, -2.0, Vec3::splat(0.4))),
    ];
    for m in 0..2 {
        for k in 0..6 {
            let material = r.add_material(&Material {
                base_color: if m == 1 {
                    Vec4::new(1.0, 0.77, 0.34, 1.0)
                } else {
                    Vec4::new(0.8, 0.1, 0.08, 1.0)
                },
                metallic: m as f32,
                roughness: 0.05 + k as f32 * 0.18,
                ..Default::default()
            })?;
            let place = at(
                k as f32 * 1.25 - 3.1,
                0.5 + m as f32 * 1.1,
                -2.5 - m as f32,
                Vec3::ONE,
            );
            objects.push(ObjectDraw::new(sphere, material, place));
        }
    }
    let mut lights = vec![
        Light {
            pos: Vec3::new(4.5, 2.4, -2.0),
            color: Vec3::new(14.0, 8.0, 3.0),
            range: 9.0,
            kind: LightKind::Point,
            shadow: true,
        },
        Light {
            pos: Vec3::new(-5.0, 5.0, 2.0),
            color: Vec3::new(20.0, 30.0, 60.0),
            range: 14.0,
            kind: LightKind::Spot {
                dir: Vec3::new(0.5, -1.0, -0.6),
                inner: 0.25,
                outer: 0.45,
            },
            shadow: true,
        },
    ];
    for i in 0..60 {
        let a = i as f32 * 2.4;
        let d = 3.0 + i as f32 * 0.35;
        lights.push(Light {
            pos: Vec3::new(a.cos() * d, 0.35, a.sin() * d - 4.0),
            color: Vec3::new(
                0.5 + 0.5 * a.sin(),
                0.5 + 0.5 * (a * 1.7).cos(),
                0.5 + 0.5 * (a * 0.6).sin(),
            ) * 0.7,
            range: 2.0,
            kind: LightKind::Point,
            shadow: false,
        });
    }
    let stain = r.add_material(&Material {
        base_texture: Some(grid),
        base_color: Vec4::new(1.0, 0.2, 1.0, 0.9),
        alpha: Alpha::Blend,
        ..Default::default()
    })?;
    Ok(World {
        decals: vec![DecalDraw {
            material: stain,
            transform: at(-0.5, 0.0, 3.5, Vec3::new(1.5, 1.0, 1.5)),
            tint: Vec4::ONE,
        }],
        logo: color,
        objects,
        lights,
        meshes: vec![
            MeshDraw::new(0, at(1.5, 0.4, 1.5, Vec3::splat(0.8))).tint(Vec3::new(0.2, 0.7, 0.3))
        ],
    })
}

/// Ein Testbild: Name und Einstellungen.
struct Case {
    name: &'static str,
    features: Features,
    night: bool,
}

fn cases() -> Vec<Case> {
    let off = Features::default();
    let day = |name, features| Case {
        name,
        features,
        night: false,
    };
    vec![
        day("aus", off),
        day(
            "schatten",
            Features {
                sun_shadow_size: 2048,
                ..off
            },
        ),
        day(
            "verdeckung",
            Features {
                ssao: Some(Ssao::default()),
                ..off
            },
        ),
        day(
            "agx",
            Features {
                tonemap: Tonemap::Agx,
                ..off
            },
        ),
        day(
            "fxaa",
            Features {
                anti_alias: AntiAlias::Fxaa,
                ..off
            },
        ),
        day(
            "taa",
            Features {
                anti_alias: AntiAlias::Taa,
                ..off
            },
        ),
        day(
            "nebel",
            Features {
                fog: Some(HeightFog {
                    color: Vec3::new(0.6, 0.65, 0.7),
                    density: 0.08,
                    falloff: 0.3,
                }),
                ..off
            },
        ),
        day("alles", Features::all()),
        day(
            "hochskaliert",
            Features {
                render_scale: 0.5,
                sharpen: 0.5,
                ..Features::all()
            },
        ),
        day(
            "halb_ohne_taa",
            Features {
                render_scale: 0.5,
                anti_alias: AntiAlias::None,
                ..Features::all()
            },
        ),
        day(
            "strahlen_schatten",
            Features {
                ssao: Some(Ssao::default()),
                ray_tracing: RayTracing {
                    sun_shadows: true,
                    occlusion: true,
                    ..Default::default()
                },
                ..Features::all()
            },
        ),
        day(
            "strahlen_alles",
            Features {
                ray_tracing: RayTracing::all(),
                ..Features::all()
            },
        ),
        day(
            "strahlen_pfade",
            Features {
                path_tracing: Some(PathTracing::default()),
                ..Features::all()
            },
        ),
        day(
            "strahlen_pfade_still",
            Features {
                path_tracing: Some(PathTracing {
                    samples: 4,
                    accumulate: true,
                    ..Default::default()
                }),
                anti_alias: AntiAlias::None,
                ..Features::all()
            },
        ),
        Case {
            name: "nacht",
            features: Features {
                exposure: Exposure::Auto {
                    min: 0.3,
                    max: 4.0,
                    speed: 3.0,
                },
                ..Features::all()
            },
            night: true,
        },
        day(
            "strahlen_lampen",
            Features {
                ray_tracing: RayTracing {
                    lamps: true,
                    ..Default::default()
                },
                ..Features::all()
            },
        ),
    ]
}

fn shot(r: &mut Renderer, w: &World, c: &Case, frame: u32, path: Option<&Path>) -> Result<()> {
    if let Some(p) = path {
        r.capture_next(p.to_path_buf());
    }
    let eye = Vec3::new(1.0, 3.2, 7.5);
    let mut scene = SceneInput {
        objects: &w.objects,
        decals: &w.decals,
        lights: &w.lights,
        features: c.features,
        cut: frame == 0,
        ..SceneInput::new(Camera::looking(
            eye,
            Vec3::new(0.2, 0.8, -2.0),
            W as f32 / H as f32,
        ))
    };
    if c.night {
        scene.sun = None;
        scene.sky = Sky::Gradient {
            zenith: Vec3::new(0.004, 0.006, 0.02),
            horizon: Vec3::new(0.02, 0.02, 0.04),
            ground: Vec3::new(0.004, 0.004, 0.004),
        };
    }
    let particle = |x: f32| ParticleInst {
        pos: [x, 0.3, 2.6],
        size: 0.5,
        color: [0.9, 0.9, 0.9, 0.5],
        extra: [0.3, 0.0, 0.8, 0.0],
    };
    r.queue_particles(&[particle(-0.6), particle(-1.0)]);
    r.draw(&FrameInput {
        time: frame as f32 / 60.0,
        meshes: &w.meshes,
        post: Post::Plain(0.0),
        images: &[UiImage::new(w.logo, [20.0, 20.0, 90.0, 90.0])],
        scene: Some(&scene),
        ..Default::default()
    })
}

/// Bildpunkte eines BMP (BGR).
fn pixels(path: &Path) -> std::result::Result<Vec<[u8; 3]>, String> {
    let bmp = std::fs::read(path).map_err(|e| format!("kein Bild: {e}"))?;
    Ok(bmp[54..].chunks(4).map(|p| [p[0], p[1], p[2]]).collect())
}

fn mean(px: &[[u8; 3]]) -> f64 {
    px.iter().flatten().map(|&c| c as f64).sum::<f64>() / (px.len() * 3) as f64
}

fn difference(a: &[[u8; 3]], b: &[[u8; 3]]) -> f64 {
    let d = |(x, y): (&[u8; 3], &[u8; 3])| (0..3).map(|k| x[k].abs_diff(y[k]) as f64).sum::<f64>();
    a.iter().zip(b).map(d).sum::<f64>() / (a.len() * 3) as f64
}

fn run(window: &Window, dir: &Path) -> std::result::Result<(), String> {
    let e = |e: Box<dyn std::error::Error>| e.to_string();
    let font = mettgine::ui::font_atlas();
    let meshes = [shapes::cube()];
    let config = Config {
        ray_tracing: true,
        hdr_output: std::env::var_os("METTGINE_HDR").is_some(),
        gpu_culling: true,
        ..Config::new(&meshes, &[], (&font, 128, 128))
    };
    let mut r = Renderer::new(window, &config).map_err(e)?;
    println!(
        "{}, Raytracing: {}, HDR: {}",
        r.gpu_name(),
        r.ray_tracing(),
        r.hdr_active()
    );
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let world = build(&mut r).map_err(e)?;
    let mut all = cases();
    if !r.ray_tracing() {
        all.retain(|c| !c.name.starts_with("strahlen"));
    }
    while r.splash_active() {
        shot(&mut r, &world, &all[0], 0, None).map_err(e)?;
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    let mut base = Vec::new();
    for c in &all {
        let path = dir.join(format!("modern_{}.bmp", c.name));
        for frame in 0..40 {
            let last = (frame == 39).then_some(path.as_path());
            shot(&mut r, &world, c, frame, last).map_err(e)?;
        }
        let px = pixels(&path)?;
        let s = r.stats();
        println!(
            "{:<11} Helligkeit {:>5.1}, gezeichnet {}, Befehle {}, GPU {:.2} ms, {:?}",
            c.name,
            mean(&px),
            s.drawn,
            s.draw_calls,
            s.gpu_ms,
            s.passes
                .iter()
                .filter(|p| !p.name.is_empty())
                .map(|p| format!("{} {:.2}", p.name, p.ms))
                .collect::<Vec<_>>()
        );
        if base.is_empty() {
            base = px;
        } else if difference(&base, &px) < 0.05 {
            return Err(format!("{} aendert nichts am Bild", c.name));
        }
    }
    // Viel auf einmal, dazu Groessenwechsel
    let many: Vec<ObjectDraw> = (0..6000)
        .map(|i| {
            let (x, z) = ((i % 100) as f32 - 50.0, (i / 100) as f32 * -1.5);
            let at = Mat4::from_translation(Vec3::new(x * 1.5, 0.5, z));
            ObjectDraw::new(world.objects[1].model, world.objects[1].material, at)
        })
        .collect();
    let lots: Vec<Light> = (0..1200)
        .map(|i| Light {
            pos: Vec3::new((i % 40) as f32 * 3.0 - 60.0, 1.5, (i / 40) as f32 * -3.0),
            ..world.lights[0]
        })
        .collect();
    let crowd = World {
        objects: many,
        lights: lots,
        logo: world.logo,
        decals: Vec::new(),
        meshes: Vec::new(),
    };
    for c in all
        .iter()
        .filter(|c| c.name == "alles" || c.name == "strahlen_alles")
    {
        for (f, size) in [(W, H), (640, 360), (W, H), (333, 777), (W, H)]
            .iter()
            .enumerate()
        {
            r.resize(size.0, size.1);
            for k in 0..20 {
                shot(&mut r, &crowd, c, f as u32 * 20 + k, None).map_err(e)?;
            }
        }
        let s = r.stats();
        println!(
            "viel/{}: CPU {:.2} ms, GPU {:.2} ms, Befehle {}",
            c.name, s.cpu_ms, s.gpu_ms, s.draw_calls
        );
    }
    check_budget(&mut r, &world).map_err(e)?;
    check_culling(&mut r, &world, dir)?;
    // Entfernen waehrend des Betriebs darf nichts kaputt machen
    r.remove_model(world.objects[0].model);
    r.remove_material(world.objects[1].material);
    for f in 0..5 {
        shot(&mut r, &world, &all[7], f, None).map_err(e)?;
    }
    Ok(())
}

/// Begrenztes Hochladen: kommt nach und nach an, Entfernen unterwegs stoert nicht.
fn check_budget(r: &mut Renderer, world: &World) -> Result<()> {
    r.set_upload_budget(1)?;
    let mut models = Vec::new();
    for _ in 0..40 {
        models.push(r.add_model(&ModelData::from_mesh(&shapes::sphere(64)))?);
    }
    let big = TextureData {
        width: 1024,
        height: 1024,
        rgba: vec![200; 1024 * 1024 * 4],
        srgb: true,
        mips: true,
    };
    let tex = r.add_texture(&big)?;
    r.remove_model(models[20]);
    r.remove_texture(tex)?;
    let objects = models.iter().map(|m| ObjectDraw {
        model: *m,
        ..world.objects[1]
    });
    let crowd = World {
        objects: objects.collect(),
        lights: Vec::new(),
        logo: world.logo,
        decals: Vec::new(),
        meshes: Vec::new(),
    };
    let all = cases();
    let rays = all.iter().find(|c| c.name == "strahlen_alles");
    let case = rays.filter(|_| r.ray_tracing()).unwrap_or(&all[0]);
    let mut frames = 0;
    while r.loading() {
        shot(r, &crowd, case, frames, None)?;
        frames += 1;
        if frames > 500 {
            return Err("Hochladen wird nie fertig".into());
        }
    }
    println!("Hochladen mit Grenze: {frames} Bilder");
    for k in 0..5 {
        shot(r, &crowd, case, k, None)?;
    }
    r.set_upload_budget(0)?;
    if frames < 5 {
        return Err("Grenze beim Hochladen wirkt nicht".into());
    }
    Ok(())
}

/// Aussortieren auf der Grafikkarte darf am Bild nichts aendern. Feine Kugeln hinter einer Wand.
fn check_culling(r: &mut Renderer, world: &World, dir: &Path) -> std::result::Result<(), String> {
    let e = |e: Box<dyn std::error::Error>| e.to_string();
    r.resize(W, H);
    let ball = r
        .add_model(&ModelData::from_mesh(&shapes::sphere(48)))
        .map_err(e)?;
    let wall = world.objects[1];
    let mut objects = vec![ObjectDraw {
        transform: Mat4::from_translation(Vec3::new(0.0, 3.0, 0.0))
            * Mat4::from_scale(Vec3::new(14.0, 6.0, 0.3)),
        ..wall
    }];
    objects.push(world.objects[0]);
    for i in 0..4000 {
        let (x, z) = ((i % 80) as f32 * 0.5 - 20.0, (i / 80) as f32 * -0.8 - 2.0);
        let at = Mat4::from_translation(Vec3::new(x, 0.4 + (i % 7) as f32 * 0.1, z));
        objects.push(ObjectDraw::new(
            ball,
            wall.material,
            at * Mat4::from_scale(Vec3::splat(0.4)),
        ));
    }
    let make = |objects| World {
        objects,
        lights: Vec::new(),
        logo: world.logo,
        decals: Vec::new(),
        meshes: Vec::new(),
    };
    // dasselbe ohne Wand: alles, was verdeckt war, muss im selben Bild erscheinen
    let open = make(objects[1..].to_vec());
    let crowd = make(objects);
    let mut images = Vec::new();
    for on in [false, true] {
        let case = Case {
            name: "aussortieren",
            features: Features {
                gpu_culling: on,
                ..Features::default()
            },
            night: false,
        };
        let path = dir.join(format!("modern_aussortieren_{on}.bmp"));
        for frame in 0..40 {
            let last = (frame == 39).then_some(path.as_path());
            shot(r, &crowd, &case, frame, last).map_err(e)?;
        }
        println!("aussortieren {on}: GPU {:.2} ms", r.stats().gpu_ms);
        images.push(pixels(&path)?);
        shot(r, &open, &case, 40, Some(&path)).map_err(e)?;
        images.push(pixels(&path)?);
    }
    if difference(&images[0], &images[1]) < 1.0 {
        return Err("die Wand verdeckt nichts".into());
    }
    if difference(&images[0], &images[2]) > 0.001 || difference(&images[1], &images[3]) > 0.001 {
        return Err("Aussortieren auf der Grafikkarte aendert das Bild".into());
    }
    Ok(())
}

struct App(PathBuf, Option<std::result::Result<(), String>>);

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_visible(false)
            .with_decorations(false)
            .with_inner_size(winit::dpi::PhysicalSize::new(W, H));
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
        eprintln!("Aufruf: gpu_check_modern <ordner>");
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
