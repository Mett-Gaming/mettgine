//! Was ein Spiel fuer die moderne Grafik uebergibt. Alles hier ist wahlfrei: Ohne
//! [`super::super::FrameInput::scene`] entsteht nichts davon.

use super::super::View;
use super::model::{ObjectDraw, TextureId};
use glam::{Mat4, Vec3};

/// Kamera der modernen Grafik.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub pos: Vec3,
    /// Welt -> Kamera (Blick nach -z)
    pub view: Mat4,
    /// Bildwinkel senkrecht (Radiant)
    pub fov_y: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    /// Blick von `pos` auf `target`, Bildwinkel 60 Grad, 0,1 bis 500 m.
    pub fn looking(pos: Vec3, target: Vec3, aspect: f32) -> Self {
        Self {
            pos,
            view: Mat4::look_at_rh(pos, target, Vec3::Y),
            fov_y: std::f32::consts::FRAC_PI_3,
            aspect,
            near: 0.1,
            far: 500.0,
        }
    }

    pub fn proj(&self) -> Mat4 {
        super::super::perspective(self.fov_y, self.aspect, self.near, self.far)
    }

    pub fn view_proj(&self) -> Mat4 {
        self.proj() * self.view
    }

    pub(crate) fn to_view(self) -> View {
        View {
            view_proj: self.view_proj(),
            pos: self.pos,
        }
    }
}

/// Sonne der modernen Grafik.
#[derive(Clone, Copy, Debug)]
pub struct SunLight {
    /// Richtung zur Sonne
    pub dir: Vec3,
    /// Farbe mal Staerke (linear; klarer Tag etwa 3 bis 6)
    pub color: Vec3,
    /// Schatten bis zu dieser Entfernung (Meter); 0 = keine
    pub shadow_distance: f32,
}

/// Art einer Lampe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightKind {
    /// nach allen Seiten
    Point,
    /// Kegel in Richtung `dir`; Winkel von der Achse (Radiant), dazwischen weich
    Spot { dir: Vec3, inner: f32, outer: f32 },
}

/// Eine Lampe.
#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub pos: Vec3,
    /// Farbe mal Staerke (linear)
    pub color: Vec3,
    /// reicht so weit (Meter)
    pub range: f32,
    pub kind: LightKind,
    /// wirft Schatten (begrenzte Zahl, naehere zuerst)
    pub shadow: bool,
}

/// Himmel und Umgebungslicht.
#[derive(Clone, Copy, Debug)]
pub enum Sky {
    /// Verlauf, Farben linear
    Gradient {
        zenith: Vec3,
        horizon: Vec3,
        ground: Vec3,
    },
    /// Rundumbild (Laenge x Breite) mit Verkleinerungsstufen, mal `strength`
    Image { texture: TextureId, strength: f32 },
}

impl Default for Sky {
    fn default() -> Self {
        Self::Gradient {
            zenith: Vec3::new(0.11, 0.24, 0.55),
            horizon: Vec3::new(0.42, 0.5, 0.62),
            ground: Vec3::new(0.12, 0.11, 0.1),
        }
    }
}

/// Uebertragung von hellen Werten auf den Bildschirm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tonemap {
    /// hart abschneiden
    None,
    /// filmisch, kraeftig
    Aces,
    /// farbtreu, weiche Lichter
    Agx,
}

/// Nachtraegliche Kantenglaettung.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AntiAlias {
    None,
    /// schnell, leicht weich
    Fxaa,
    /// ueber mehrere Bilder, ruhig und scharf
    Taa,
}

/// Verdeckung in Ecken (SSAO).
#[derive(Clone, Copy, Debug)]
pub struct Ssao {
    /// Reichweite (Meter)
    pub radius: f32,
    /// 0..2, 1 = normal
    pub strength: f32,
}

impl Default for Ssao {
    fn default() -> Self {
        Self {
            radius: 0.6,
            strength: 1.0,
        }
    }
}

/// Ueberstrahlen heller Stellen.
#[derive(Clone, Copy, Debug)]
pub struct Bloom {
    /// Anteil (0..1)
    pub strength: f32,
    /// erst ab dieser Helligkeit (nach Belichtung)
    pub threshold: f32,
}

impl Default for Bloom {
    fn default() -> Self {
        Self {
            strength: 0.06,
            threshold: 1.0,
        }
    }
}

/// Nebel, der nach oben duenner wird.
#[derive(Clone, Copy, Debug)]
pub struct HeightFog {
    pub color: Vec3,
    /// Dichte pro Meter auf Hoehe 0
    pub density: f32,
    /// Abnahme pro Meter Hoehe (0 = ueberall gleich)
    pub falloff: f32,
}

/// Belichtung.
#[derive(Clone, Copy, Debug)]
pub enum Exposure {
    /// fester Faktor
    Fixed(f32),
    /// passt sich der Bildhelligkeit an; Faktor bleibt zwischen `min` und `max`
    Auto { min: f32, max: f32, speed: f32 },
}

/// Schaltbare Grafik. Vorgabe: alles aus.
#[derive(Clone, Copy, Debug)]
pub struct Features {
    pub tonemap: Tonemap,
    pub exposure: Exposure,
    pub anti_alias: AntiAlias,
    pub ssao: Option<Ssao>,
    pub bloom: Option<Bloom>,
    pub fog: Option<HeightFog>,
    /// Kantenlaenge einer Schattenstufe (Texel); 0 = keine Sonnenschatten
    pub sun_shadow_size: u32,
    /// Schatten von Lampen
    pub lamp_shadows: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            tonemap: Tonemap::None,
            exposure: Exposure::Fixed(1.0),
            anti_alias: AntiAlias::None,
            ssao: None,
            bloom: None,
            fog: None,
            sun_shadow_size: 0,
            lamp_shadows: false,
        }
    }
}

impl Features {
    /// Alles an, in guter Qualitaet.
    pub fn all() -> Self {
        Self {
            tonemap: Tonemap::Agx,
            exposure: Exposure::Fixed(1.0),
            anti_alias: AntiAlias::Taa,
            ssao: Some(Ssao::default()),
            bloom: Some(Bloom::default()),
            fog: None,
            sun_shadow_size: 2048,
            lamp_shadows: true,
        }
    }
}

/// Moderne Szene eines Bildes. Modelle und Figuren aus `FrameInput` werden mitgezeichnet
/// (mit [`SceneInput::plain_roughness`]); Beleuchtung und Nebel kommen von hier.
#[derive(Clone, Copy)]
pub struct SceneInput<'a> {
    pub camera: Camera,
    pub objects: &'a [ObjectDraw],
    pub sun: Option<SunLight>,
    pub lights: &'a [Light],
    pub sky: Sky,
    /// Umgebungslicht-Faktor (1 = wie der Himmel)
    pub ambient: f32,
    pub features: Features,
    /// Rauheit der einfachen Modelle und Figuren
    pub plain_roughness: f32,
    /// vergangene Zeit seit dem letzten Bild (Sekunden), fuer die Belichtung
    pub dt: f32,
    /// Kamerasprung: Verlauf (TAA, Belichtung) verwerfen
    pub cut: bool,
}

impl SceneInput<'_> {
    /// Leere Szene bei Tag, alle Zusaetze aus.
    pub fn new(camera: Camera) -> Self {
        Self {
            camera,
            objects: &[],
            sun: Some(SunLight {
                dir: Vec3::new(0.4, 1.0, 0.3),
                color: Vec3::new(2.4, 2.2, 1.95),
                shadow_distance: 80.0,
            }),
            lights: &[],
            sky: Sky::default(),
            ambient: 1.0,
            features: Features::default(),
            plain_roughness: 0.75,
            dt: 1.0 / 60.0,
            cut: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_off_by_default() {
        let f = Features::default();
        assert!(f.ssao.is_none() && f.bloom.is_none() && f.fog.is_none());
        assert!(f.sun_shadow_size == 0 && !f.lamp_shadows);
        assert_eq!(f.anti_alias, AntiAlias::None);
    }
}
