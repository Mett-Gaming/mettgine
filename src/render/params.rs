//! Was ein Spiel uebergibt: beim Aufbau ([`Config`]) und pro Bild ([`FrameInput`]).

use super::{UiVertex, WorldShaders, SHADOW_SIZE};
use crate::gpu::{MeshData, SkinData};
use glam::{Mat4, Vec2, Vec3, Vec4};

/// Aufloesung, in der die Szene gezeichnet wird.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Resolution {
    /// relativ zum Fenster (0.25..2)
    Scale(f32),
    /// feste Bildpunkte, hochgezogen aufs Fenster
    Fixed(u32, u32),
}

/// Virtuelle Flaeche der Oberflaeche.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiSize {
    /// feste Hoehe, die Breite folgt dem Fenster; wird in Fensteraufloesung gezeichnet
    Height(f32),
    /// festes Raster; mit [`Post::Vhs`] eine eigene Ebene in genau dieser Aufloesung
    Fixed(u32, u32),
}

/// Aufbau des Renderers. Vorgaben: [`Config::new`].
#[derive(Clone, Copy)]
pub struct Config<'a> {
    /// alle Modelle (Index = [`MeshDraw::mesh`])
    pub meshes: &'a [MeshData],
    /// alle Figuren (Index = [`SkinDraw::skin`])
    pub skins: &'a [SkinData],
    /// Schrift-Atlas: R8-Pixel, Breite, Hoehe
    pub font: (&'a [u8], u32, u32),
    /// Schild-Atlas der Welt-Boxen: R8-Pixel, Breite, Hoehe
    pub signs: Option<(&'a [u8], u32, u32)>,
    pub resolution: Resolution,
    pub ui: UiSize,
    /// Aufloesung der zweiten Kamera (Monitor, Spiegel); None = keine
    pub second_camera: Option<(u32, u32)>,
    /// weich skalieren (Szene aufs Fenster, Schrift); false = harte Bildpunkte
    pub smooth: bool,
    /// bei gleicher Tiefe gewinnt das zuletzt Gezeichnete
    pub last_wins: bool,
    /// Materialien der Welt-Boxen
    pub world: WorldShaders<'a>,
}

impl<'a> Config<'a> {
    /// Fensteraufloesung, weich, Oberflaeche 720 hoch, keine zweite Kamera.
    pub fn new(
        meshes: &'a [MeshData],
        skins: &'a [SkinData],
        font: (&'a [u8], u32, u32),
    ) -> Config<'a> {
        Config {
            meshes,
            skins,
            font,
            signs: None,
            resolution: Resolution::Scale(1.0),
            ui: UiSize::Height(super::UI_HEIGHT),
            second_camera: None,
            smooth: true,
            last_wins: true,
            world: WorldShaders::default(),
        }
    }
}

/// Ein Modell fuer dieses Bild.
#[derive(Clone, Copy, Debug)]
pub struct MeshDraw {
    /// Index in [`Config::meshes`]
    pub mesh: usize,
    pub model: Mat4,
    /// rgb = Farbmultiplikator, a = Eigenleuchten
    pub tint: Vec4,
    /// pulsierendes Aufleuchten (Warnung), 0..1
    pub blink: f32,
    /// wirft Schatten (nur mit [`Lighting::Sun`])
    pub shadow: bool,
}

impl Default for MeshDraw {
    fn default() -> Self {
        Self::new(0, Mat4::IDENTITY)
    }
}

impl MeshDraw {
    pub fn new(mesh: usize, model: Mat4) -> Self {
        Self {
            mesh,
            model,
            tint: Vec4::new(1.0, 1.0, 1.0, 0.0),
            blink: 0.0,
            shadow: true,
        }
    }
    /// Farbmultiplikator setzen (Eigenleuchten bleibt).
    pub fn tint(mut self, rgb: Vec3) -> Self {
        self.tint = rgb.extend(self.tint.w);
        self
    }
    /// Eigenleuchten (0 = aus, 1 = volle Eigenfarbe dazu).
    pub fn glow(mut self, g: f32) -> Self {
        self.tint.w = g;
        self
    }
    pub fn blink(mut self, b: f32) -> Self {
        self.blink = b;
        self
    }
    pub fn no_shadow(mut self) -> Self {
        self.shadow = false;
        self
    }
}

/// Eine animierte Figur: ihre Knochen stehen ab `first_bone` in den Knochen des Bildes.
#[derive(Clone, Copy, Debug)]
pub struct SkinDraw {
    /// Index in [`Config::skins`]
    pub skin: usize,
    pub model: Mat4,
    /// rgb = Farbmultiplikator, a = Eigenleuchten
    pub tint: Vec4,
    pub first_bone: u32,
    /// pulsierendes Aufleuchten, 0..1
    pub blink: f32,
    /// halbtransparent (Hologramm), ohne Schatten
    pub ghost: bool,
    /// Zerreissen (0..1)
    pub distort: f32,
}

impl Default for SkinDraw {
    fn default() -> Self {
        Self {
            skin: 0,
            model: Mat4::IDENTITY,
            tint: Vec4::new(1.0, 1.0, 1.0, 0.0),
            first_bone: 0,
            blink: 0.0,
            ghost: false,
            distort: 0.0,
        }
    }
}

/// Eine Kamera.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Projektion mal Ansicht (nach oben zeigendes Bild, siehe [`perspective`])
    pub view_proj: Mat4,
    pub pos: Vec3,
}

/// Sonne, Himmel und Schattenkarte. Farben in Anzeigewerten (0..1).
#[derive(Clone, Copy, Debug)]
pub struct Sun {
    /// Richtung zur Sonne (wird normiert)
    pub dir: Vec3,
    pub color: Vec3,
    /// Bereich der Schattenkarte: Mittelpunkt und halbe Kantenlaenge (Meter)
    pub shadow_center: Vec3,
    pub shadow_radius: f32,
    /// 0 = keine Schatten, 1 = volle Schatten
    pub shadow_strength: f32,
    pub sky_top: Vec3,
    /// Horizont- und Nebelfarbe
    pub horizon: Vec3,
    /// Nebeldichte pro Meter (ab 25 m Abstand)
    pub fog_density: f32,
    /// Umgebungslicht von unten
    pub ground: Vec3,
}

/// Raeume mit Lampen (siehe [`super::Renderer::upload_world`]).
#[derive(Clone, Copy, Debug)]
pub struct Rooms {
    /// fast kein Umgebungslicht, schwaches Licht an der Kamera
    pub dark: bool,
    pub fog_color: Vec3,
    /// Nebeldichte pro Meter
    pub fog_density: f32,
    /// Verfall der Welt-Materialien (0..1)
    pub decay: f32,
    /// Schwanken der Boxen: seitlich, hoch, Ende z, Ende x
    pub sway: Vec4,
}

/// Beleuchtung eines Bildes.
#[derive(Clone, Copy, Debug)]
pub enum Lighting {
    Sun(Sun),
    Rooms(Rooms),
}

/// Saubere Nachbearbeitung: weiche Lichter, Vignette, Blenden.
#[derive(Clone, Copy, Debug)]
pub struct Clean {
    /// Schwarzblende 0..1
    pub fade: f32,
    /// Rotblitz 0..1
    pub red: f32,
    /// Saettigung (1 = normal)
    pub saturation: f32,
}

/// Videoband-Nachbearbeitung.
#[derive(Clone, Copy, Debug)]
pub struct Vhs {
    /// Staerke (0 = aus, 1 = normal)
    pub strength: f32,
    /// dunkler, gruenstichiger Zustand
    pub dark: bool,
    /// Bildstoerung 0..1
    pub glitch: f32,
    /// Schwarzblende 0..1
    pub fade: f32,
    /// Rotblitz 0..1
    pub red: f32,
    /// Farbumkehr 0..1
    pub invert: f32,
    /// Ueberbelichtung 0..1
    pub flash: f32,
}

/// Nachbearbeitung eines Bildes.
#[derive(Clone, Copy, Debug)]
pub enum Post {
    Clean(Clean),
    Vhs(Vhs),
}

/// Ein Bild.
#[derive(Clone, Copy)]
pub struct FrameInput<'a> {
    pub view: View,
    /// zweite Kamera; None = ihr Bild zeigt nur die Nebelfarbe
    pub second_camera: Option<View>,
    /// laufende Zeit in Sekunden
    pub time: f32,
    pub lighting: Lighting,
    pub post: Post,
    pub meshes: &'a [MeshDraw],
    /// halbtransparente Modelle (Hologramme), in dieser Reihenfolge gezeichnet
    pub ghosts: &'a [MeshDraw],
    pub skins: &'a [SkinDraw],
    /// Figuren, die nur die zweite Kamera sieht
    pub camera_skins: &'a [SkinDraw],
    /// alle Knochen-Matrizen ([`SkinDraw::first_bone`] zeigt hinein)
    pub bones: &'a [Mat4],
    /// Dreiecke in Einheiten der virtuellen Flaeche ([`Config::ui`])
    pub ui: &'a [UiVertex],
}

impl Default for Sun {
    /// Klarer Tag: Sonne schraeg von oben, Schatten im Umkreis von 30 m um den Ursprung.
    fn default() -> Self {
        Self {
            dir: Vec3::new(0.4, 1.0, 0.3),
            color: Vec3::new(1.0, 0.95, 0.85),
            shadow_center: Vec3::ZERO,
            shadow_radius: 30.0,
            shadow_strength: 0.8,
            sky_top: Vec3::new(0.35, 0.55, 0.9),
            horizon: Vec3::new(0.75, 0.83, 0.92),
            fog_density: 0.004,
            ground: Vec3::new(0.25, 0.23, 0.2),
        }
    }
}

impl Default for Clean {
    /// Keine Blenden, normale Saettigung.
    fn default() -> Self {
        Self {
            fade: 0.0,
            red: 0.0,
            saturation: 1.0,
        }
    }
}

impl Default for FrameInput<'_> {
    /// Leeres Bild bei Tageslicht, Blick vom Ursprung nach -z.
    fn default() -> Self {
        Self {
            view: View {
                view_proj: perspective(1.1, 16.0 / 9.0, 0.1, 500.0),
                pos: Vec3::ZERO,
            },
            second_camera: None,
            time: 0.0,
            lighting: Lighting::Sun(Sun::default()),
            post: Post::Clean(Clean::default()),
            meshes: &[],
            ghosts: &[],
            skins: &[],
            camera_skins: &[],
            bones: &[],
            ui: &[],
        }
    }
}

impl FrameInput<'_> {
    /// Leeres Bild bei Tageslicht, Blick von `eye` auf `target` (Bildwinkel 60 Grad).
    /// `aspect` = Breite / Hoehe des Fensters.
    pub fn looking(eye: Vec3, target: Vec3, aspect: f32) -> Self {
        let proj = perspective(std::f32::consts::FRAC_PI_3, aspect, 0.1, 500.0);
        Self {
            view: View {
                view_proj: proj * Mat4::look_at_rh(eye, target, Vec3::Y),
                pos: eye,
            },
            ..Self::default()
        }
    }
}

/// Perspektive (`fov_y` in Radiant). naga uebersetzt die Shader mit der y-Drehung von WebGPU:
/// NDC y = +1 ist oben im Bild, wie bei OpenGL. Darum genuegt `Mat4::perspective_rh` ohne
/// Spiegeln; Nachbearbeitung, UI und Schattenkarte rechnen mit derselben Regel.
pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    Mat4::perspective_rh(fov_y, aspect, near, far)
}

/// Wo ein Weltpunkt im Bild liegt: x nach rechts, y nach unten, 0..1 = im Bild. `None` = hinter
/// der Kamera. Mal [`super::Renderer::ui_area`] ergibt die Stelle auf der Oberflaeche.
pub fn project(view_proj: &Mat4, p: Vec3) -> Option<Vec2> {
    let clip = *view_proj * p.extend(1.0);
    if clip.w <= 0.05 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5))
}

/// Ortho-Kamera der Sonne, die einen Wuerfel um `center` (halbe Kante `radius`) abdeckt.
/// Auf das Texelraster eingerastet, damit Schattenkanten beim Bewegen nicht flimmern.
pub fn light_view_proj(sun_dir: Vec3, center: Vec3, radius: f32) -> Mat4 {
    let dir = sun_dir.normalize();
    let up = if dir.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
    let eye = center + dir * radius * 3.0;
    let view = Mat4::look_at_rh(eye, center, up);
    let texel = 2.0 * radius / SHADOW_SIZE as f32;
    let c = view.transform_point3(center);
    let snap = Vec3::new(
        (c.x / texel).round() * texel - c.x,
        (c.y / texel).round() * texel - c.y,
        0.0,
    );
    let view = Mat4::from_translation(snap) * view;
    let proj = Mat4::orthographic_rh(-radius, radius, -radius, radius, 0.1, radius * 6.0);
    proj * view
}
