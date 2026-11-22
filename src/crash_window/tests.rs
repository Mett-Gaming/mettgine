use super::*;

#[test]
fn crash_mode_is_recognized_from_the_arguments() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(parse_args(&a(&["kasse0.exe", "1994"])), None);
    let c = parse_args(&a(&["kasse0.exe", ARG_REPORT, "C:/x/crash.txt"])).unwrap();
    assert_eq!(c.report, PathBuf::from("C:/x/crash.txt"));
    assert!(c.hint.is_none());
    let c = parse_args(&a(&["k", ARG_REPORT, "r.txt", ARG_HINT, "Vulkan fehlt"])).unwrap();
    assert_eq!(c.hint.as_deref(), Some("Vulkan fehlt"));
}

#[test]
fn window_layout_fits_and_buttons_do_not_overlap() {
    for (wd, ht, s) in [(744, 521, 1.0), (1116, 782, 1.5), (440, 280, 1.0)] {
        let l = layout(wd, ht, s);
        for (x, y, w, h) in l {
            assert!(x >= 0 && y >= 0 && w > 0 && h > 0);
            assert!(
                x + w <= wd && y + h <= ht + 1,
                "ragt heraus: {:?}",
                (x, y, w, h)
            );
        }
        for k in 3..6 {
            assert!(l[k].0 + l[k].2 <= l[k + 1].0, "Knoepfe ueberlappen");
        }
        assert!(l[2].1 + l[2].3 <= l[3].1, "Bericht ueberdeckt die Knoepfe");
    }
}
