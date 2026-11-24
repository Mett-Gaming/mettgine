//! Kompiliert alle `shaders/*.wgsl` mit naga zu SPIR-V (kein Vulkan SDK noetig).
//! Jede Datei braucht die Entry Points `vs_main` und `fs_main`; erzeugt werden
//! `<name>.vert.spv` und `<name>.frag.spv` in OUT_DIR, fuer weitere Entry Points
//! `<name>.<entry>.spv`.

use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=shaders");

    for entry in fs::read_dir("shaders").expect("Ordner shaders/ fehlt") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("wgsl") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
        if name.starts_with('_') {
            continue; // nur zum Einbinden gedacht
        }
        let src = read_with_includes(&path, 0);

        let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| {
            panic!(
                "{}",
                e.emit_to_string_with_path(&src, path.to_str().unwrap())
            )
        });
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
            fs::write(out.join(format!("{name}.{suffix}.spv")), bytes).unwrap();
        }
    }
}

/// Minimaler Praeprozessor: `#include "datei.wgsl"`, auch verschachtelt.
fn read_with_includes(path: &std::path::Path, depth: usize) -> String {
    assert!(depth < 8, "{}: Includes im Kreis", path.display());
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{} fehlt: {e}", path.display()));
    text.lines()
        .map(|line| match line.trim().strip_prefix("#include ") {
            Some(inc) => {
                let file = PathBuf::from("shaders").join(inc.trim_matches('"'));
                read_with_includes(&file, depth + 1)
            }
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
