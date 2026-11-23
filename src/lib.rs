//! Mettgine: die Engine der Mett-Gaming-Spiele (entstanden aus KASSE 0).
//!
//! - `render`: der eine Vulkan-Renderer: Welt-Boxen mit Lampen, Sonne mit Schattenkarte,
//!   Modelle, animierte Figuren, Hologramme, Partikel, zweite Kamera, UI, Nachbearbeitung.
//!   Die Shader liegen in `shaders/`. Dazu `particles`, `cull`, `splash`.
//! - `collide`: Kollision und Strahlen. `profile`: Messanzeige.
//! - `audio`, `input`, `canvas`, `text`, `settings`: Klang, Eingabe, Menues, Einstellungen.
//! - `presence`: Status fuer den Mett Launcher (Discord-Anzeige).
//! - `net`: Mehrspieler-Verbindung und Abgleich.
//! - `anim`: Knochen-Animation (.k0a v2), Ueberblenden, Kopf-Blickziel, Verformungen.
//! - `gpu`: Datenformate zwischen Spiel und Renderer.
//! - `ui`: Text und einfache Formen fuer Menues und HUD.
//! - `crash`, `crash_window`: Protokoll, Absturzberichte und Absturzfenster.

pub mod anim;
pub mod audio;
pub mod canvas;
pub mod collide;
pub mod crash;
pub mod crash_window;
pub mod cull;
pub mod gpu;
pub mod input;
pub mod net;
pub mod particles;
pub mod presence;
pub mod profile;
pub mod render;
pub mod settings;
mod splash;
pub mod text;
pub mod ui;
