//! Partikel: Staub, Rauch, Funken. Gerechnet auf dem Prozessor, gezeichnet vom
//! Szenen-Renderer (`Renderer::queue_particles`).
//!
//! Welche Effekte es gibt, beschreibt das Spiel mit [`Style`]; hier liegt nur die Mechanik.

use bytemuck::{Pod, Zeroable};
use glam::{Vec3, Vec4};

/// Ein gezeichnetes Partikel (Instanz-Daten, siehe `shaders/particle.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ParticleInst {
    pub pos: [f32; 3],
    /// halbe Kantenlaenge (Meter)
    pub size: f32,
    /// rgb, a = Deckkraft
    pub color: [f32; 4],
    /// x = Drehung (Radiant), y = Aufhellen statt Abdecken (0..1), z = Randweichheit (0..1)
    pub extra: [f32; 4],
}

/// Aussehen und Verhalten eines Effekts.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    /// Lebensdauer von..bis (s)
    pub life: (f32, f32),
    /// Anfangstempo von..bis (m/s)
    pub speed: (f32, f32),
    /// Streuung um die Richtung: 0 = genau, 1 = in alle Richtungen
    pub spread: f32,
    /// Beschleunigung (Schwerkraft, Auftrieb; m/s^2)
    pub accel: Vec3,
    /// Luftwiderstand (1/s)
    pub drag: f32,
    /// halbe Groesse am Anfang und am Ende (m)
    pub size: (f32, f32),
    /// Farbe am Anfang und am Ende (a = Deckkraft)
    pub color: (Vec4, Vec4),
    /// Drehung von..bis (Radiant/s)
    pub spin: (f32, f32),
    /// 0 = deckt ab (Rauch), 1 = hellt auf (Funken)
    pub additive: f32,
    /// 0 = harter Rand, 1 = ganz weich
    pub softness: f32,
    /// Boden (y): Partikel bleiben darueber und prallen gedaempft ab
    pub floor: Option<f32>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            life: (0.5, 1.0),
            speed: (0.5, 1.5),
            spread: 1.0,
            accel: Vec3::ZERO,
            drag: 0.0,
            size: (0.1, 0.1),
            color: (Vec4::ONE, Vec4::new(1.0, 1.0, 1.0, 0.0)),
            spin: (0.0, 0.0),
            additive: 0.0,
            softness: 0.8,
            floor: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Particle {
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
    rot: f32,
    spin: f32,
    style: u32,
}

/// Alle Partikel eines Spiels. Feste Obergrenze: Ist sie erreicht, kommen keine neuen dazu.
#[derive(Debug)]
pub struct Particles {
    items: Vec<Particle>,
    styles: Vec<Style>,
    max: usize,
    rng: u32,
    out: Vec<ParticleInst>,
}

/// Kennung eines Effekts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleId(u32);

impl Particles {
    /// Hoechstens `max` Partikel gleichzeitig; gleicher `seed` = gleicher Ablauf.
    pub fn new(max: usize, seed: u32) -> Self {
        Self {
            items: Vec::with_capacity(max),
            styles: Vec::new(),
            max,
            rng: seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453) | 1,
            out: Vec::with_capacity(max),
        }
    }

    /// Effekt anmelden (einmal beim Start).
    pub fn style(&mut self, s: Style) -> StyleId {
        self.styles.push(s);
        StyleId(self.styles.len() as u32 - 1)
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    fn range(&mut self, r: (f32, f32)) -> f32 {
        r.0 + (r.1 - r.0) * self.rand()
    }

    fn sphere_dir(&mut self) -> Vec3 {
        let z = self.rand() * 2.0 - 1.0;
        let a = self.rand() * std::f32::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * a.cos(), r * a.sin(), z)
    }

    /// `count` Partikel auf einmal, von `pos` in Richtung `dir`, zusaetzlich bewegt mit `base`
    /// (Tempo der Quelle).
    pub fn burst(&mut self, id: StyleId, pos: Vec3, dir: Vec3, base: Vec3, count: usize) {
        let Some(&s) = self.styles.get(id.0 as usize) else {
            return;
        };
        let dir = dir.normalize_or_zero();
        for _ in 0..count.min(self.max - self.items.len()) {
            let k = s.spread.clamp(0.0, 1.0);
            let random = self.sphere_dir();
            let d = (dir * (1.0 - k) + random * k).normalize_or(random);
            let speed = self.range(s.speed);
            let life = self.range(s.life).max(1e-3);
            let rot = self.rand() * std::f32::consts::TAU;
            let spin = self.range(s.spin);
            self.items.push(Particle {
                pos,
                vel: base + d * speed,
                age: 0.0,
                life,
                rot,
                spin,
                style: id.0,
            });
        }
    }

    /// Dauerquelle: `per_second` Partikel ueber `dt` Sekunden, gleichmaessig entlang `from`..`to`
    /// verteilt (keine Klumpen bei schneller Bewegung). `rest` merkt sich angebrochene Partikel.
    #[allow(clippy::too_many_arguments)]
    pub fn stream(
        &mut self,
        id: StyleId,
        from: Vec3,
        to: Vec3,
        dir: Vec3,
        per_second: f32,
        dt: f32,
        rest: &mut f32,
    ) {
        *rest += per_second.max(0.0) * dt.max(0.0);
        let n = *rest as usize;
        *rest -= n as f32;
        for i in 0..n {
            let t = (i as f32 + 0.5) / n as f32;
            self.burst(id, from.lerp(to, t), dir, Vec3::ZERO, 1);
        }
    }

    /// Einen Schritt rechnen.
    pub fn update(&mut self, dt: f32) {
        let styles = &self.styles;
        self.items.retain_mut(|p| {
            let s = &styles[p.style as usize];
            p.age += dt;
            if p.age >= p.life {
                return false;
            }
            p.vel += s.accel * dt;
            p.vel *= (-s.drag * dt).exp();
            p.pos += p.vel * dt;
            p.rot += p.spin * dt;
            if let Some(y) = s.floor {
                if p.pos.y < y {
                    p.pos.y = y;
                    p.vel.y = p.vel.y.abs() * 0.35;
                    p.vel.x *= 0.7;
                    p.vel.z *= 0.7;
                }
            }
            true
        });
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Zeichen-Daten, von hinten nach vorn sortiert (damit Rauch richtig ueberdeckt).
    pub fn instances(&mut self, cam_pos: Vec3) -> &[ParticleInst] {
        let styles = &self.styles;
        self.out.clear();
        self.out.extend(self.items.iter().map(|p| {
            let s = &styles[p.style as usize];
            let t = p.age / p.life;
            ParticleInst {
                pos: p.pos.to_array(),
                size: s.size.0 + (s.size.1 - s.size.0) * t,
                color: s.color.0.lerp(s.color.1, t).to_array(),
                extra: [p.rot, s.additive, s.softness, 0.0],
            }
        }));
        let dist = |p: &ParticleInst| Vec3::from(p.pos).distance_squared(cam_pos);
        self.out.sort_by(|a, b| dist(b).total_cmp(&dist(a)));
        &self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bursts_live_move_and_die() {
        let mut p = Particles::new(100, 1);
        let s = p.style(Style {
            life: (1.0, 1.0),
            speed: (2.0, 2.0),
            spread: 0.0,
            size: (0.1, 0.5),
            ..Default::default()
        });
        p.burst(s, Vec3::ZERO, Vec3::X, Vec3::ZERO, 10);
        p.update(0.5);
        let i = p.instances(Vec3::ZERO)[0];
        assert!((i.pos[0] - 1.0).abs() < 1e-4 && (i.size - 0.3).abs() < 1e-4);
        assert!((i.color[3] - 0.5).abs() < 1e-4, "blendet aus");
        p.update(0.6);
        assert!(p.is_empty());
    }

    #[test]
    fn capacity_order_and_repeatability() {
        let run = || {
            let mut p = Particles::new(50, 7);
            let s = p.style(Style::default());
            p.burst(s, Vec3::ZERO, Vec3::Y, Vec3::ZERO, 500);
            assert_eq!(p.len(), 50, "Obergrenze");
            p.update(0.1);
            p.instances(Vec3::new(0.0, 0.0, -5.0)).to_vec()
        };
        let a = run();
        assert_eq!(a, run(), "gleicher Seed, gleicher Ablauf");
        let d = |p: &ParticleInst| Vec3::from(p.pos).distance(Vec3::new(0.0, 0.0, -5.0));
        assert!(a.windows(2).all(|w| d(&w[0]) >= d(&w[1])), "hinten zuerst");
    }

    #[test]
    fn stream_rate_does_not_depend_on_frame_rate() {
        for steps in [10, 144] {
            let mut p = Particles::new(10_000, 3);
            let s = p.style(Style {
                life: (99.0, 99.0),
                ..Default::default()
            });
            let mut rest = 0.0;
            for _ in 0..steps {
                p.stream(
                    s,
                    Vec3::ZERO,
                    Vec3::X,
                    Vec3::Y,
                    100.0,
                    1.0 / steps as f32,
                    &mut rest,
                );
            }
            assert!((99..=100).contains(&p.len()), "{} bei {steps}", p.len());
        }
    }

    #[test]
    fn floor_keeps_particles_above_ground() {
        let mut p = Particles::new(50, 5);
        let s = p.style(Style {
            life: (9.0, 9.0),
            accel: Vec3::new(0.0, -9.8, 0.0),
            floor: Some(0.0),
            ..Default::default()
        });
        p.burst(s, Vec3::Y, Vec3::Y, Vec3::ZERO, 50);
        for _ in 0..200 {
            p.update(0.016);
            assert!(p.instances(Vec3::ZERO).iter().all(|i| i.pos[1] >= 0.0));
        }
    }
}
