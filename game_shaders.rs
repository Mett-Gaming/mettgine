// WGSL zu SPIR-V uebersetzen, im build.rs der Engine und der Spiele eingebunden. Ein Spiel braucht
// dafuer `naga` (Version wie in der Engine) als Bau-Abhaengigkeit:
//
//     include!("mettgine/game_shaders.rs");
//     mettgine_shader("shaders/welt.wgsl", &["shaders", "mettgine/shaders"]);
//
// Erzeugt `OUT_DIR/<name>.vert.spv` und `<name>.frag.spv`, fuer weitere Einstiege
// `<name>.<einstieg>.spv`.

/// Uebersetzt eine Shader-Datei; `#include "datei.wgsl"` wird in `includes` gesucht.
fn mettgine_shader(path: &str, includes: &[&str]) {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    for dir in includes {
        println!("cargo:rerun-if-changed={dir}");
    }
    let name = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("Shader ohne Namen")
        .to_owned();
    let src = read_with_includes(std::path::Path::new(path), includes, 0);
    let module = naga::front::wgsl::parse_str(&src)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string_with_path(&src, path)));
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&src)));
    for ep in &module.entry_points {
        let (stage, entry_point) = (ep.stage, ep.name.as_str());
        let suffix = match entry_point {
            "vs_main" => "vert",
            "fs_main" => "frag",
            other => other,
        };
        let mut options = naga::back::spv::Options::default();
        if name.starts_with("rt_") {
            options.lang_version = (1, 4); // Strahlabfragen
        }
        let pipeline = naga::back::spv::PipelineOptions {
            shader_stage: stage,
            entry_point: entry_point.to_string(),
        };
        let words = naga::back::spv::write_vec(&module, &info, &options, Some(&pipeline))
            .unwrap_or_else(|e| panic!("{name} {entry_point}: {e}"));
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        std::fs::write(out.join(format!("{name}.{suffix}.spv")), bytes).expect("Shader schreiben");
    }
}

/// Minimaler Praeprozessor: `#include "datei.wgsl"`, auch verschachtelt.
fn read_with_includes(path: &std::path::Path, dirs: &[&str], depth: usize) -> String {
    assert!(depth < 8, "{}: Includes im Kreis", path.display());
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} fehlt: {e}", path.display()));
    text.lines()
        .map(|line| match line.trim().strip_prefix("#include ") {
            Some(inc) => {
                let file = inc.trim_matches('"');
                let found = dirs
                    .iter()
                    .map(|d| std::path::Path::new(d).join(file))
                    .find(|p| p.exists())
                    .unwrap_or_else(|| panic!("{}: {file} nicht gefunden", path.display()));
                read_with_includes(&found, dirs, depth + 1)
            }
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
