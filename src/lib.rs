//! Mettgine: die Engine der Mett-Gaming-Spiele (entstanden aus KASSE 0).
//!
//! - `render`: der eine Vulkan-Renderer: Welt-Boxen mit Lampen, Sonne mit Schattenkarte,
//!   Modelle, animierte Figuren, Hologramme, Partikel, zweite Kamera, UI, Nachbearbeitung.
//!   Die Shader liegen in `shaders/`. Dazu `particles`, `cull`, `splash`.
//! - `app`: Spielgeruest (Fenster, Ereignisse, Bildtakt). `time`: fester Takt.
//! - `shapes`: einfache Formen als Modelle.
//! - `asset`, `jobs`: Spieldaten im Hintergrund laden, Arbeit verteilen.
//! - `scene` (Objekte mit Hierarchie), `camera`, `nav` (Wege), `random`, `save`, `locale`,
//!   `menu`, `debug` (Linien, Konsole, Messung); mit Features `ecs` und `physics`.
//! - `collide`: Kollision und Strahlen. `profile`: Messanzeige.
//! - `audio`, `input`, `canvas`, `text`, `settings`: Klang, Eingabe, Menues, Einstellungen.
//! - `presence`: Status fuer den Mett Launcher (Discord-Anzeige). `launcher`: Erfolge und Overlay.
//! - `net`: Mehrspieler-Verbindung und Abgleich.
//! - `anim`: Knochen-Animation (.k0a v2), Ueberblenden, Kopf-Blickziel, Verformungen.
//! - `gpu`: Datenformate zwischen Spiel und Renderer.
//! - `ui`: Text und einfache Formen fuer Menues und HUD.
//! - `crash`, `crash_window`: Protokoll, Absturzberichte und Absturzfenster.

/// Fenster-Bibliothek der Engine (Ereignisse in `app::Game::event`)
pub use winit;

pub mod anim;
pub mod app;
pub mod asset;
pub mod audio;
pub mod camera;
pub mod canvas;
pub mod collide;
pub mod crash;
pub mod crash_window;
pub mod cull;
#[cfg(feature = "ecs")]
pub use hecs as ecs;
pub mod debug;
pub mod gpu;
pub mod input;
pub mod jobs;
pub mod launcher;
pub mod locale;
pub mod menu;
pub mod nav;
pub mod net;
pub mod particles;
#[cfg(feature = "physics")]
pub mod physics;
pub mod presence;
pub mod profile;
pub mod random;
pub mod render;
pub mod save;
pub mod scene;
pub mod settings;
pub mod shapes;
mod splash;
pub mod text;
pub mod time;
pub mod ui;
