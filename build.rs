//! Kompiliert alle `shaders/*.wgsl` mit naga zu SPIR-V (kein Vulkan SDK noetig), siehe
//! `game_shaders.rs`. Dateien mit `_` am Anfang sind nur zum Einbinden gedacht.

include!("game_shaders.rs");

fn main() {
    println!("cargo:rerun-if-changed=game_shaders.rs");
    for entry in std::fs::read_dir("shaders").expect("Ordner shaders/ fehlt") {
        let path = entry.expect("shaders/ lesen").path();
        let is_wgsl = path.extension().and_then(|e| e.to_str()) == Some("wgsl");
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("_");
        if is_wgsl && !name.starts_with('_') {
            mettgine_shader(&format!("shaders/{name}"), &["shaders"]);
        }
    }
}
