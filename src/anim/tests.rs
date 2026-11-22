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
