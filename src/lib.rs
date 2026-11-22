//! Mettgine: die Engine der Mett-Gaming-Spiele (entstanden aus KASSE 0).
//!
//! - `render`: Vulkan-Renderer (Welt-Boxen, Modelle, animierte Figuren, Hologramme,
//!   Ueberwachungskamera, UI-Ebene, VHS-Nachbearbeitung). Die Shader liegen in `shaders/`;
//!   die Welt-Materialien (Poster, Schilder, Monitor) stammen noch aus KASSE 0.
//! - `anim`: Knochen-Animation (.k0a v2), Ueberblenden, Kopf-Blickziel, Verformungen.
//! - `gpu`: Datenformate zwischen Spiel und Renderer.
//! - `ui`: Text und einfache Formen fuer Menues und HUD.
//! - `crash`, `crash_window`: Protokoll, Absturzberichte und Absturzfenster.

pub mod anim;
pub mod crash;
pub mod crash_window;
pub mod gpu;
pub mod render;
pub mod ui;
