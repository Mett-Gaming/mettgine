//! Was ein Spiel pro Bild uebergibt: Modelle, Figuren, Licht und Kamera.

use super::SHADOW_SIZE;
use glam::{Mat4, Vec3, Vec4};

/// Ein Modell fuer dieses Bild.
#[derive(Clone, Copy, Debug)]
pub struct MeshDraw {
    /// Index in die Modelle, die `SceneRenderer::new` bekommen hat
    pub mesh: usize,
    pub model: Mat4,
    /// rgb = Farbmultiplikator, a = Eigenleuchten
    pub tint: Vec4,
    /// pulsierendes Aufleuchten (Warnung), 0..1
    pub blink: f32,
    /// wirft Schatten
    pub shadow: bool,
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
    /// Index in die Figuren, die `SceneRenderer::new` bekommen hat
    pub skin: usize,
    pub model: Mat4,
    /// rgb = Farbmultiplikator, a = Eigenleuchten
    pub tint: Vec4,
    pub first_bone: u32,
    /// pulsierendes Aufleuchten, 0..1
    pub blink: f32,
}

/// Licht, Kamera und Blenden eines Bildes. Farben in Anzeigewerten (0..1).
#[derive(Clone, Copy, Debug)]
pub struct SceneParams {
    /// Kamera (Projektion mit nach oben zeigendem Bild, siehe [`perspective`])
    pub view_proj: Mat4,
    pub cam_pos: Vec3,
    /// laufende Zeit in Sekunden (fuer Blinken)
    pub time: f32,
    /// Richtung zur Sonne (wird normiert)
    pub sun_dir: Vec3,
    pub sun_color: Vec3,
    /// Bereich der Schattenkarte: Mittelpunkt und halbe Kantenlaenge (Meter)
    pub shadow_center: Vec3,
    pub shadow_radius: f32,
    /// 0 = keine Schatten (Pass faellt weg), 1 = volle Schatten
    pub shadow_strength: f32,
    pub sky_top: Vec3,
    /// Horizont- und Nebelfarbe
    pub horizon: Vec3,
    /// Nebeldichte pro Meter (ab 25 m Abstand)
    pub fog_density: f32,
    /// Umgebungslicht von unten
    pub ground: Vec3,
    /// Schwarzblende 0..1
    pub fade: f32,
    /// Rotblitz 0..1
    pub red: f32,
    /// Saettigung (1 = normal)
    pub saturation: f32,
}

/// Perspektive (`fov_y` in Radiant). naga uebersetzt die Shader mit der y-Drehung von WebGPU:
/// NDC y = +1 ist oben im Bild, wie bei OpenGL. Darum genuegt `Mat4::perspective_rh` ohne
/// Spiegeln; Nachbearbeitung, UI und Schattenkarte rechnen mit derselben Regel.
pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    Mat4::perspective_rh(fov_y, aspect, near, far)
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
