use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum TestClip {
    #[default]
    Stand,
    Walk,
    Grab,
}

impl ClipId for TestClip {
    fn index(self) -> usize {
        self as usize
    }
    fn locomotion(self) -> bool {
        self == TestClip::Walk
    }
    fn looping(self) -> bool {
        self != TestClip::Grab
    }
}

/// Kleine Datei: zwei Knochen, `clips` Clips mit je einem Bild.
fn file(clips: u32, frames: u32) -> Vec<u8> {
    let mut b = b"K0A2".to_vec();
    b.extend(2u32.to_le_bytes());
    b.extend(clips.to_le_bytes());
    for (name, parent, y) in [("Root", -1i32, 0.0f32), ("Head", 0, 1.5)] {
        b.push(name.len() as u8);
        b.extend(name.as_bytes());
        b.extend(parent.to_le_bytes());
        for f in [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, y, 0.0, 0.0, 1.0, 0.0] {
            b.extend(f32::to_le_bytes(f));
        }
    }
    for _ in 0..clips {
        b.push(1);
        b.push(b'c');
        b.extend(30.0f32.to_le_bytes());
        b.extend(frames.to_le_bytes());
        b.push(1);
        for _ in 0..frames {
            for y in [0.0f32, 1.5] {
                for f in [0.0, 0.0, 0.0, 1.0, 0.0, y, 0.0, 1.0, 1.0, 1.0] {
                    b.extend(f32::to_le_bytes(f));
                }
            }
        }
    }
    b
}

#[test]
fn broken_files_are_errors_and_clipless_files_stand_still() {
    let good = file(1, 2);
    for cut in 0..good.len() {
        assert!(parse_anim(&good[..cut]).is_err(), "bei {cut} Byte");
    }
    assert!(parse_anim(&file(1, 0)).is_err(), "Clip ohne Bilder");
    for b in [good, file(0, 0)] {
        let a = parse_anim(&b).unwrap();
        let mut out = Vec::new();
        a.sample(TestClip::Grab, 3.7, &mut out);
        assert_eq!(out.len(), 2);
        assert!(out
            .iter()
            .all(|m| m.abs_diff_eq(glam::Mat4::IDENTITY, 1e-5)));
    }
    assert_eq!(
        parse_anim(&file(0, 0)).unwrap().duration(TestClip::Walk),
        0.0
    );
}

#[test]
fn animator_crossfades_and_counts_one_shots_from_their_start() {
    let mut an = Animator::default();
    an.update(TestClip::Walk, 5.0, 0.1, 0.2);
    an.update(TestClip::Walk, 5.1, 0.1, 0.2);
    an.update(TestClip::Grab, 5.2, 0.1, 0.2);
    assert!(an.blend().is_some(), "kein Ueberblenden");
    assert!(an.clip_time() < 1e-4, "Einmal-Clip startet nicht bei 0");
    for k in 0..5 {
        an.update(TestClip::Grab, 5.3 + k as f32 * 0.1, 0.1, 0.2);
    }
    assert!(an.blend().is_none(), "Ueberblenden endet nicht");
    assert!(an.finished(0.4));
    let _ = TestClip::Stand;
}
