use super::*;

#[test]
fn date_stamp_is_correct() {
    assert_eq!(stamp(0), "1970-01-01_00-00-00");
    assert_eq!(stamp(1_700_000_000), "2023-11-14_22-13-20");
    assert_eq!(stamp(951_782_400), "2000-02-29_00-00-00");
}

#[test]
fn reports_contain_error_context_and_log() {
    let dir = std::env::temp_dir().join(format!("kasse0-crashtest-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    init_in(Some(dir.clone()), "kasse0", "KASSE 0", "test", false);
    log("Hallo Protokoll");
    context("Stufe", "falsch");
    let path = write_report("Testabsturz", "etwas ging schief").expect("kein Bericht");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("Testabsturz") && text.contains("etwas ging schief"));
    assert!(text.contains("Stufe: falsch"), "Zustand fehlt");
    assert!(text.contains("Hallo Protokoll"), "Protokoll fehlt");
    let latest = fs::read_to_string(dir.join("logs").join("latest.log")).unwrap();
    assert!(latest.contains("Hallo Protokoll"));
    // alte Berichte werden aufgeraeumt
    for k in 0..15 {
        fs::write(
            dir.join("crashes").join(format!("crash-0000-{k:02}.txt")),
            "x",
        )
        .unwrap();
    }
    write_report("noch einer", "x");
    let n = fs::read_dir(dir.join("crashes")).unwrap().count();
    assert!(n <= KEEP_REPORTS, "{n} Berichte behalten");
    let _ = fs::remove_dir_all(&dir);
}
