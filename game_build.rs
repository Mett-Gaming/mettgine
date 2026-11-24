// Hilfe fuer das build.rs eines Spiels, dort eingebunden mit
// `include!("mettgine/game_build.rs");`. Gehoert nicht zur Engine selbst.

/// Setzt die Version des Spiels (`env!("<variable>")`): in der CI die Versionsmarke, sonst
/// `git describe`, sonst Cargo.toml. Dazu moderne Windows-Bedienelemente fuer das Absturzfenster.
fn mettgine_game(variable: &str) {
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-changed=mettgine/game_build.rs");
    let version = std::env::var("GITHUB_REF_NAME")
        .ok()
        .filter(|r| r.starts_with('v'))
        .or_else(|| {
            std::process::Command::new("git")
                .args(["describe", "--tags", "--always"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    println!("cargo:rustc-env={variable}={version}");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bins=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
