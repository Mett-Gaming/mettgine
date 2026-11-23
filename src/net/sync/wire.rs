//! Nachrichten packen und lesen (Byte-Reihenfolge: little endian). Lesen kann nie abstuerzen:
//! Fehlt etwas, kommt `None`.

use glam::Vec3;
use std::f32::consts::TAU;

/// Schreibt eine Nachricht. Wiederverwendbar ueber [`Writer::clear`].
#[derive(Clone, Debug, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn clear(&mut self) {
        self.buf.clear();
    }
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn bool(&mut self, v: bool) {
        self.u8(v as u8);
    }
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn vec3(&mut self, v: Vec3) {
        v.to_array().iter().for_each(|c| self.f32(*c));
    }
    /// Zahl zwischen `min` und `max` in 2 Byte (begrenzt; NaN wird `min`).
    pub fn ranged(&mut self, v: f32, min: f32, max: f32) {
        let t = ((v - min) / (max - min)).clamp(0.0, 1.0);
        self.u16((t * 65535.0).round() as u16);
    }
    /// Winkel (Radiant, beliebig oft herum) in 2 Byte.
    pub fn angle(&mut self, v: f32) {
        let t = (v / TAU).rem_euclid(1.0);
        self.u16((t * 65536.0).round() as u32 as u16);
    }
    /// Text bis 255 Byte; laengerer wird an einer Zeichengrenze gekuerzt.
    pub fn str(&mut self, s: &str) {
        let mut n = s.len().min(255);
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        self.u8(n as u8);
        self.buf.extend_from_slice(&s.as_bytes()[..n]);
    }
}

/// Liest eine Nachricht.
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }
    /// Noch nicht gelesene Byte.
    pub fn left(&self) -> usize {
        self.buf.len()
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if n > self.buf.len() {
            return None;
        }
        let (a, b) = self.buf.split_at(n);
        self.buf = b;
        Some(a)
    }
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }
    pub fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|b| b[0])
    }
    pub fn bool(&mut self) -> Option<bool> {
        self.u8().map(|v| v != 0)
    }
    pub fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_le_bytes)
    }
    pub fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }
    /// Nur endliche Zahlen (NaN und unendlich gelten als kaputte Nachricht).
    pub fn f32(&mut self) -> Option<f32> {
        self.array()
            .map(f32::from_le_bytes)
            .filter(|v| v.is_finite())
    }
    /// wie [`Reader::f32`].
    pub fn f64(&mut self) -> Option<f64> {
        self.array()
            .map(f64::from_le_bytes)
            .filter(|v| v.is_finite())
    }
    pub fn vec3(&mut self) -> Option<Vec3> {
        Some(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    pub fn ranged(&mut self, min: f32, max: f32) -> Option<f32> {
        self.u16().map(|v| min + (max - min) * (v as f32 / 65535.0))
    }
    /// Winkel in 0..2 Pi.
    pub fn angle(&mut self) -> Option<f32> {
        self.u16().map(|v| v as f32 / 65536.0 * TAU)
    }
    pub fn str(&mut self) -> Option<&'a str> {
        let n = self.u8()? as usize;
        std::str::from_utf8(self.take(n)?).ok()
    }
}
