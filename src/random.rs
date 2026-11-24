//! Zufall mit festem Startwert: gleicher Startwert, gleiche Folge, auf jedem Rechner.
//!
//! Fuer neue Spiele. Wer schon einen anderen Generator benutzt, sollte dabei bleiben: Sonst
//! aendern sich alle Welten.

/// Zufallsgenerator (xoshiro128++).
#[derive(Clone, Debug, PartialEq)]
pub struct Random {
    s: [u32; 4],
}

fn mix(z: &mut u64) -> u64 {
    *z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut x = *z;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Startwert aus der Uhr (fuer "irgendein" Spiel).
pub fn seed_from_clock() -> u64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    mix(&mut t.clone())
}

/// Fester Zufallswert 0..1 zu drei Zahlen (z. B. Weltkoordinaten).
pub fn hash(x: i32, y: i32, z: i32) -> f32 {
    let mut s = (x as u32 as u64) << 32 | y as u32 as u64;
    s ^= (z as u32 as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93);
    (mix(&mut s) >> 40) as f32 / (1u64 << 24) as f32
}

impl Random {
    pub fn new(seed: u64) -> Self {
        let mut z = seed;
        let (a, b) = (mix(&mut z), mix(&mut z));
        Self {
            s: [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32 | 1],
        }
    }

    /// Unabhaengige Folge fuer einen Teilbereich (z. B. je Raum).
    pub fn fork(&mut self, id: u64) -> Self {
        Self::new((self.u32() as u64) << 32 ^ self.u32() as u64 ^ id)
    }

    pub fn u32(&mut self) -> u32 {
        let s = &mut self.s;
        let out = s[0].wrapping_add(s[3]).rotate_left(7).wrapping_add(s[0]);
        let t = s[1] << 9;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(11);
        out
    }

    /// 0 (einschliesslich) bis 1 (ausschliesslich).
    pub fn f32(&mut self) -> f32 {
        (self.u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// `a` bis `b`.
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f32()
    }

    /// 0 bis `n - 1`, alle gleich wahrscheinlich; 0 bei `n` = 0.
    pub fn index(&mut self, n: usize) -> usize {
        ((self.u32() as u64 * n as u64) >> 32) as usize
    }

    /// Trifft mit Wahrscheinlichkeit `p` (0..1) ein.
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    pub fn pick<'a, T>(&mut self, list: &'a [T]) -> Option<&'a T> {
        list.get(self.index(list.len()))
    }

    /// Nach Gewichten waehlen; `None`, wenn kein Gewicht ueber 0 liegt.
    pub fn weighted(&mut self, weights: &[f32]) -> Option<usize> {
        let sum: f32 = weights.iter().map(|w| w.max(0.0)).sum();
        let mut at = self.f32() * sum;
        let mut last = None;
        for (i, w) in weights.iter().enumerate().filter(|(_, w)| **w > 0.0) {
            last = Some(i);
            if at < *w {
                return last;
            }
            at -= w;
        }
        last
    }

    pub fn shuffle<T>(&mut self, list: &mut [T]) {
        for i in (1..list.len()).rev() {
            list.swap(i, self.index(i + 1));
        }
    }

    /// Richtung auf der Einheitskugel.
    pub fn direction(&mut self) -> glam::Vec3 {
        let y = self.range(-1.0, 1.0);
        let a = self.range(0.0, std::f32::consts::TAU);
        let r = (1.0 - y * y).max(0.0).sqrt();
        glam::Vec3::new(a.cos() * r, y, a.sin() * r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_are_fixed_and_even() {
        let (mut a, mut b) = (Random::new(1994), Random::new(1994));
        assert!((0..100).all(|_| a.u32() == b.u32()));
        assert_ne!(Random::new(1).u32(), Random::new(2).u32());
        let mut r = Random::new(7);
        let mut hits = [0u32; 6];
        for _ in 0..60_000 {
            hits[r.index(6)] += 1;
        }
        assert!(hits.iter().all(|h| (9_500..10_500).contains(h)), "{hits:?}");
        assert_eq!(r.index(0), 0);
        let mean = (0..10_000).map(|_| r.f32()).sum::<f32>() / 10_000.0;
        assert!((mean - 0.5).abs() < 0.02);
        assert!((r.direction().length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn choices_respect_their_weights() {
        let mut r = Random::new(3);
        let mut n = [0; 3];
        for _ in 0..4000 {
            n[r.weighted(&[1.0, 0.0, 3.0]).unwrap()] += 1;
        }
        assert!(n[1] == 0 && n[2] > n[0] * 2);
        assert_eq!(r.weighted(&[0.0, 0.0]), None);
        assert_eq!(r.pick::<u8>(&[]), None);
        let mut v: Vec<u32> = (0..50).collect();
        r.shuffle(&mut v);
        assert_ne!(v, (0..50).collect::<Vec<_>>());
        v.sort_unstable();
        assert_eq!(v, (0..50).collect::<Vec<_>>());
        assert!((0..200).all(|i| (0.0..1.0).contains(&hash(i, -i, 5))));
    }
}
