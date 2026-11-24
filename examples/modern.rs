//! Moderne Grafik zum Ausprobieren: Physik-Kisten, Lampen, Menue mit allen Schaltern.
//!
//! `cargo run --release --features full --example modern [modell.glb]`
//! WASD/Pfeile laufen, Maus umsehen, Leertaste wirft eine Kiste, Tab oeffnet das Menue, Esc beendet.

use glam::{Mat4, Vec3, Vec4};
use mettgine::app::{self, AppConfig, Game, Stage};
use mettgine::asset::gltf;
use mettgine::camera::FirstPerson;
use mettgine::canvas::Canvas;
use mettgine::input::{Bind, Key, Pad, Stick};
use mettgine::menu::{Event, Item, Kind, Menu, Style};
use mettgine::physics::{BodyDesc, BodyId, Motion, Physics, Shape};
use mettgine::profile::{draw_overlay, Meter};
use mettgine::random::Random;
use mettgine::render::*;
use mettgine::scene::Transform;
use mettgine::text::FontAtlas;
use mettgine::time::FixedStep;
use winit::window::Window;

const FORWARD: &[Bind] = &[
    Bind::Key(Key::KeyW),
    Bind::Key(Key::ArrowUp),
    Bind::Stick(Stick::Up),
];
const BACK: &[Bind] = &[
    Bind::Key(Key::KeyS),
    Bind::Key(Key::ArrowDown),
    Bind::Stick(Stick::Down),
];
const LEFT: &[Bind] = &[
    Bind::Key(Key::KeyA),
    Bind::Key(Key::ArrowLeft),
    Bind::Stick(Stick::Left),
];
const RIGHT: &[Bind] = &[
    Bind::Key(Key::KeyD),
    Bind::Key(Key::ArrowRight),
    Bind::Stick(Stick::Right),
];
const THROW: &[Bind] = &[Bind::Key(Key::Space), Bind::Pad(Pad::South)];
const MENU: &[Bind] = &[Bind::Key(Key::Tab), Bind::Pad(Pad::Start)];
const QUIT: &[Bind] = &[Bind::Key(Key::Escape)];

/// Schalter im Menue.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Opt {
    Shadows,
    Lamps,
    Ssao,
    Bloom,
    Fog,
    Aa,
    Tonemap,
    Scale,
    Sharpen,
    RayShadows,
    RayAo,
    RayIndirect,
    RayMirror,
    Paths,
    Night,
}

fn menu() -> Menu<Opt> {
    let on = |id, text: &str, v| Item::new(id, text, Kind::Toggle(v));
    let choice = |id, text: &str, names: &[&str], index| {
        let names = names.iter().map(|n| n.to_string()).collect();
        Item::new(id, text, Kind::Choice { index, names })
    };
    let slider = |id, text: &str, value, min| {
        let (max, step) = (1.0, 0.05);
        let kind = Kind::Slider {
            value,
            min,
            max,
            step,
        };
        Item::new(id, text, kind)
    };
    Menu::new(vec![
        on(Opt::Shadows, "Sonnenschatten", true),
        on(Opt::Lamps, "Lampenschatten", true),
        on(Opt::Ssao, "Verdeckung", true),
        on(Opt::Bloom, "Überstrahlen", true),
        on(Opt::Fog, "Nebel", false),
        choice(Opt::Aa, "Kantenglättung", &["aus", "FXAA", "TAA"], 2),
        choice(Opt::Tonemap, "Farbkurve", &["hart", "ACES", "AgX"], 2),
        slider(Opt::Scale, "Auflösung", 1.0, 0.35),
        slider(Opt::Sharpen, "Nachschärfen", 0.0, 0.0),
        on(Opt::RayShadows, "Strahlen: Schatten", false),
        on(Opt::RayAo, "Strahlen: Verdeckung", false),
        on(Opt::RayIndirect, "Strahlen: Streulicht", false),
        on(Opt::RayMirror, "Strahlen: Spiegelung", false),
        on(Opt::Paths, "Pathtracing", false),
        on(Opt::Night, "Nacht", false),
    ])
}

struct Demo {
    menu: Menu<Opt>,
    open: bool,
    features: Features,
    night: bool,
    cam: FirstPerson,
    physics: Physics,
    step: FixedStep,
    boxes: Vec<(BodyId, usize)>,
    cube: Option<ModelId>,
    floor: Option<ModelId>,
    materials: Vec<MaterialId>,
    extra: Option<gltf::Uploaded>,
    lights: Vec<Light>,
    canvas: Canvas,
    meter: Meter,
    time: f32,
}

impl Demo {
    fn new() -> app::Result<Self> {
        let mut physics = Physics::default();
        let ground = Shape::Box(Vec3::new(40.0, 0.5, 40.0));
        let at = Transform::at(Vec3::Y * -0.5);
        physics.add(&BodyDesc::new(ground, Motion::Fixed, at))?;
        let mut rnd = Random::new(5);
        let lights = (0..120)
            .map(|i| Light {
                pos: Vec3::new(
                    rnd.range(-18.0, 18.0),
                    rnd.range(0.4, 2.5),
                    rnd.range(-18.0, 18.0),
                ),
                color: Vec3::new(rnd.f32(), rnd.f32(), rnd.f32()) * 3.0,
                range: 5.0,
                kind: LightKind::Point,
                shadow: i % 12 == 0,
            })
            .collect();
        // die eigene Bitmap-Schrift reicht hier nicht: Systemschrift nehmen
        let font = std::fs::read("C:/Windows/Fonts/arial.ttf")?;
        let mut demo = Self {
            menu: menu(),
            open: false,
            features: Features::all(),
            night: false,
            cam: FirstPerson {
                pos: Vec3::new(0.0, 2.0, 10.0),
                ..Default::default()
            },
            physics,
            step: FixedStep::new(1.0 / 60.0),
            boxes: Vec::new(),
            cube: None,
            floor: None,
            materials: Vec::new(),
            extra: None,
            lights,
            canvas: Canvas::new(FontAtlas::bake(&font)?),
            meter: Meter::new(),
            time: 0.0,
        };
        for i in 0..60 {
            let p = Vec3::new(
                rnd.range(-6.0, 6.0),
                2.0 + i as f32 * 0.6,
                rnd.range(-6.0, 6.0),
            );
            demo.drop_box(p, Vec3::ZERO)?;
        }
        Ok(demo)
    }

    fn drop_box(&mut self, at: Vec3, speed: Vec3) -> app::Result<()> {
        let shape = Shape::Box(Vec3::splat(0.5));
        let id = self
            .physics
            .add(&BodyDesc::new(shape, Motion::Free, Transform::at(at)))?;
        self.physics.set_velocity(id, speed);
        self.boxes.push((id, self.boxes.len() % 6));
        Ok(())
    }

    fn apply(&mut self, e: Event<Opt>) {
        let f = &mut self.features;
        match e {
            Event::Toggled(o, on) => match o {
                Opt::Shadows => f.sun_shadow_size = if on { 2048 } else { 0 },
                Opt::Lamps => f.lamp_shadows = on,
                Opt::Ssao => f.ssao = on.then(Ssao::default),
                Opt::Bloom => f.bloom = on.then(Bloom::default),
                Opt::Fog => {
                    f.fog = on.then_some(HeightFog {
                        color: Vec3::new(0.5, 0.55, 0.6),
                        density: 0.05,
                        falloff: 0.2,
                    })
                }
                Opt::RayShadows => f.ray_tracing.sun_shadows = on,
                Opt::RayAo => f.ray_tracing.occlusion = on,
                Opt::RayIndirect => f.ray_tracing.indirect = on,
                Opt::RayMirror => f.ray_tracing.reflections = on,
                Opt::Paths => f.path_tracing = on.then(PathTracing::default),
                Opt::Night => self.night = on,
                _ => {}
            },
            Event::Chosen(Opt::Aa, i) => {
                f.anti_alias = [AntiAlias::None, AntiAlias::Fxaa, AntiAlias::Taa][i]
            }
            Event::Chosen(Opt::Tonemap, i) => {
                f.tonemap = [Tonemap::None, Tonemap::Aces, Tonemap::Agx][i]
            }
            Event::Slid(Opt::Scale, v) => f.render_scale = v,
            Event::Slid(Opt::Sharpen, v) => f.sharpen = v,
            Event::Back => self.open = false,
            _ => {}
        }
    }

    fn load(&mut self, r: &mut Renderer) -> app::Result<()> {
        use mettgine::shapes;
        self.cube = Some(r.add_model(&ModelData::from_mesh(&shapes::cube()))?);
        self.floor = Some(r.add_model(&ModelData::from_mesh(&shapes::plane(80.0)))?);
        let mut rnd = Random::new(9);
        for i in 0..7 {
            let c = Vec3::new(
                rnd.range(0.2, 1.0),
                rnd.range(0.2, 1.0),
                rnd.range(0.2, 1.0),
            );
            self.materials.push(r.add_material(&Material {
                base_color: if i == 6 {
                    Vec4::new(0.4, 0.4, 0.42, 1.0)
                } else {
                    c.extend(1.0)
                },
                metallic: (i % 3 == 0 && i < 6) as u8 as f32,
                roughness: if i == 6 { 0.35 } else { 0.1 + i as f32 * 0.14 },
                ..Default::default()
            })?);
        }
        if let Some(path) = std::env::args().nth(1) {
            self.extra = Some(gltf::parse(&std::fs::read(path)?)?.upload(r)?);
        }
        Ok(())
    }

    fn objects(&self) -> Vec<ObjectDraw> {
        let mut out = Vec::new();
        let (Some(cube), Some(floor)) = (self.cube, self.floor) else {
            return out;
        };
        out.push(ObjectDraw::new(floor, self.materials[6], Mat4::IDENTITY));
        for (id, m) in &self.boxes {
            if let Some(t) = self.physics.transform(*id) {
                out.push(ObjectDraw::new(cube, self.materials[*m], t.matrix()));
            }
        }
        if let Some(e) = &self.extra {
            out.extend(e.draws(Mat4::from_translation(Vec3::new(0.0, 0.0, 4.0)), 0));
        }
        out
    }
}

impl Game for Demo {
    fn renderer(&mut self, window: &Window) -> app::Result<Renderer> {
        let font = &self.canvas.font;
        let config = Config {
            ray_tracing: true,
            ..Config::new(&[], &[], (&font.pixels, font.width, font.height))
        };
        Renderer::new(window, &config)
    }

    fn ready(&mut self, stage: &mut Stage) {
        stage.set_grab(true);
        if let Err(e) = self.load(&mut stage.renderer) {
            mettgine::crash::log(&format!("Laden fehlgeschlagen: {e}"));
            stage.quit();
        }
    }

    fn mouse_motion(&mut self, _: &mut Stage, dx: f32, dy: f32) {
        self.cam.turn(dx, dy, 0.002);
    }

    fn frame(&mut self, stage: &mut Stage, dt: f32) {
        self.time += dt;
        if stage.input.pressed(QUIT) && !self.open {
            stage.quit();
        }
        if stage.input.pressed(MENU) {
            self.open = !self.open;
        }
        if self.open {
            let cursor = stage.cursor();
            for e in self.menu.update(&stage.input, cursor, dt) {
                self.apply(e);
            }
        } else {
            let axis = |a, b| stage.input.held(a) as i8 as f32 - stage.input.held(b) as i8 as f32;
            self.cam.pos += self.cam.walk(axis(RIGHT, LEFT), axis(FORWARD, BACK)) * dt * 5.0;
            if stage.input.pressed(THROW) {
                let dir = self.cam.forward();
                let _ = self.drop_box(self.cam.pos + dir, dir * 14.0);
            }
        }
        stage.set_grab(!self.open);
        self.step.add(dt);
        while self.step.step() {
            self.physics.step(self.step.dt());
        }
        let objects = self.objects();
        let mut camera = Camera::looking(Vec3::ZERO, Vec3::NEG_Z, stage.aspect());
        camera.pos = self.cam.pos;
        camera.view = self.cam.matrix();
        let mut scene = SceneInput {
            objects: &objects,
            lights: if self.night { &self.lights } else { &[] },
            features: self.features,
            dt,
            ..SceneInput::new(camera)
        };
        if self.night {
            scene.sun = None;
            scene.sky = Sky::Gradient {
                zenith: Vec3::new(0.004, 0.006, 0.02),
                horizon: Vec3::new(0.02, 0.02, 0.04),
                ground: Vec3::splat(0.004),
            };
        }
        self.meter.push(dt, &stage.renderer.stats());
        let (w, h) = stage.renderer.ui_area();
        self.canvas.begin(w, h);
        draw_overlay(&mut self.canvas, 8.0, 8.0, 14.0, &self.meter.lines());
        if self.open {
            let c = &mut self.canvas;
            c.rect(w - 460.0, 40.0, 440.0, 16.0 * 34.0, [0.0, 0.0, 0.0, 0.75]);
            self.menu
                .draw(c, w - 450.0, 50.0, 420.0, 34.0, &Style::default());
        }
        stage.draw(&FrameInput {
            time: self.time,
            post: Post::Plain(0.0),
            ui: &self.canvas.verts,
            scene: Some(&scene),
            ..Default::default()
        });
        // unsichtbarer Lauf zum Pruefen
        if std::env::var_os("METTGINE_HIDDEN").is_some() && self.time > 8.0 {
            stage.quit();
        }
    }
}

fn main() -> app::Result<()> {
    let config = AppConfig::new("mettgine-demo", "Mettgine", env!("CARGO_PKG_VERSION"));
    app::run(config, Demo::new)
}
