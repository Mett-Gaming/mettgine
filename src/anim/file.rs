//! Liest `.k0a` v2 (Kennung `K0A2`). Beschaedigte Dateien ergeben einen Fehler, keinen Abbruch.

use super::{AnimSet, ClipData, Local};
use glam::{Mat4, Quat, Vec3, Vec4};

/// Liest der Reihe nach; hinter dem Ende gibt es Fehler.
struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.rest.len() {
            return Err("K0A2-Animation zu kurz".into());
        }
        let (a, b) = self.rest.split_at(n);
        self.rest = b;
        Ok(a)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> Result<usize, String> {
        Ok(u32::from_le_bytes(self.array()?) as usize)
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    fn floats<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut out = [0.0; N];
        for f in &mut out {
            *f = self.f32()?;
        }
        Ok(out)
    }

    /// Laenge als Byte, dann der Text.
    fn name(&mut self) -> Result<String, String> {
        let len = self.u8()? as usize;
        Ok(String::from_utf8_lossy(self.take(len)?).into_owned())
    }

    /// Drei Zeilen mit je vier Zahlen.
    fn mat34(&mut self) -> Result<Mat4, String> {
        let m = self.floats::<12>()?;
        let col = |c: usize, w: f32| Vec4::new(m[c], m[4 + c], m[8 + c], w);
        Ok(Mat4::from_cols(
            col(0, 0.0),
            col(1, 0.0),
            col(2, 0.0),
            col(3, 1.0),
        ))
    }

    fn local(&mut self) -> Result<Local, String> {
        let f = self.floats::<10>()?;
        Ok(Local {
            rot: Quat::from_xyzw(f[0], f[1], f[2], f[3]).normalize(),
            pos: Vec3::new(f[4], f[5], f[6]),
            scale: Vec3::new(f[7], f[8], f[9]),
        })
    }

    fn clip(&mut self, bones: usize) -> Result<ClipData, String> {
        let fps = self.f32()?;
        let frames = self.u32()?;
        let looped = self.u8()? != 0;
        if frames == 0 || !fps.is_finite() || fps <= 0.0 {
            return Err("K0A2-Animation: Clip ohne Bilder".into());
        }
        let count = frames.checked_mul(bones).ok_or("K0A2-Animation zu gross")?;
        // 40 Byte je Knochen und Bild: vorher pruefen statt riesigen Speicher anzufordern
        if count.checked_mul(40).is_none_or(|n| n > self.rest.len()) {
            return Err("K0A2-Animation zu kurz".into());
        }
        let locals = (0..count).map(|_| self.local()).collect::<Result<_, _>>()?;
        Ok(ClipData {
            fps,
            frames,
            looped,
            locals,
        })
    }
}

/// Liest eine Animationsdatei (`.k0a` v2).
pub fn parse_anim(b: &[u8]) -> Result<AnimSet, String> {
    let mut r = Reader { rest: b };
    if r.take(4)? != b"K0A2" {
        return Err("keine K0A2-Animation".into());
    }
    let (bones, count) = (r.u32()?, r.u32()?);
    let (mut names, mut parents, mut rest_inv) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..bones {
        names.push(r.name()?);
        let p = r.i32()?;
        if p >= i as i32 {
            return Err("K0A2-Animation: Eltern muessen vor ihren Kindern stehen".into());
        }
        parents.push(p);
        rest_inv.push(r.mat34()?.inverse());
    }
    let (mut clips, mut clip_names) = (Vec::new(), Vec::new());
    for _ in 0..count {
        clip_names.push(r.name()?);
        clips.push(r.clip(bones)?);
    }
    Ok(AnimSet::new(names, parents, rest_inv, clips, clip_names))
}
