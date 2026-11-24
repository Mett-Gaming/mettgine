# Mettgine

Die Engine der Mett-Gaming-Spiele, entstanden aus [KASSE 0](https://github.com/Mett-Gaming/kasse0). Rust und Vulkan, ohne großes Framework drumherum.

| Modul | Inhalt |
|---|---|
| `render` | Der eine Renderer für alle Spiele (Vulkan): Sonne mit Schattenkarte oder Räume mit Lampen, Welt-Boxen, Modelle (instanziert), animierte Figuren, Hologramme, Partikel, zweite Kamera, UI, saubere oder Videoband-Nachbearbeitung, Screenshots. Sortiert Unsichtbares aus, Puffer wachsen mit |
| `app` | Spielgerüst: Fenster, Ereignisse, Bildtakt, Absturzfenster. Ein Spiel setzt `Game` um |
| `time`, `input` | fester Takt; Tastatur, Maus, Controller, umstellbare Tasten |
| `asset`, `jobs` | Spieldaten im Hintergrund laden (auch PNG, JPEG, glTF, Klang), Arbeit auf Kerne verteilen |
| `scene`, `camera`, `nav` | Objekte mit Hierarchie, Kameraführung, Wegfindung |
| `physics`, `ecs` | Physik (rapier) und Entity-Component-System (hecs), beide zuschaltbar |
| `menu`, `locale`, `save`, `random` | Menüs, Übersetzungen, Spielstände, Zufall |
| `debug`, `shapes` | Linien, Konsole, Messung; einfache Formen |
| `particles` | Partikel-Rechnung (Staub, Rauch, Funken), gezeichnet von `render` |
| `cull` | Sichtfeld und Hüllkugeln |
| `collide` | Kollision ohne Physik-Bibliothek: Kisten, Kapseln, Strahlen, Sichtlinien, Stufen |
| `splash` | Startbild „Gemacht mit Mettgine“: erscheint in jedem Spiel von selbst, nicht überspringbar, immer in Fensterauflösung. Spiele prüfen mit `splash_active()`, ob sie Eingaben annehmen |
| `profile` | Messanzeige: Prozessor- und Grafikzeiten, Ruckler, Zeichenbefehle |
| `anim` | Knochen-Animation (`.k0a` v2): Überblenden, Einmal-Clips, Kopf-Blickziel, Zucken, Strecken, Kopfdrehung, Stop-Motion |
| `gpu` | Datenformate zwischen Spiel und Renderer: Welt-Boxen, Lichter, Modelle, Figuren (`.k0s`) |
| `ui` | Bitmap-Text und Formen für Menüs und HUD |
| `canvas`, `text` | Zeichenfläche für Menüs mit Schrift aus TTF/OTF |
| `audio` | Effekte, Musik, Klang-Bausteine (`synth`), Raumklang (`spatial`), Mischpult (`mixer`) |
| `net` | Mehrspieler-Verbindungen; `net::sync` für Abgleich, Zwischenbilder und Vorhersage |
| `presence` | Meldet dem Mett Launcher, was gerade passiert (Discord-Anzeige) |
| `launcher` | Erfolge und Overlay des Mett Launchers, siehe [docs/erfolge.md](docs/erfolge.md) |
| `settings` | Einstellungen lesen und speichern |
| `crash`, `crash_window` | Protokoll, Absturzberichte (auch bei Treiberabstürzen) und Absturzfenster |

Die Shader liegen in `shaders/` und werden beim Bauen mit naga zu SPIR-V übersetzt, ein Vulkan SDK braucht man nicht. Eigene Welt-Materialien bringt ein Spiel mit `Config::world` mit und übersetzt sie in seinem `build.rs` mit `game_shaders.rs` (Beispiel: KASSE 0, `shaders/world.wgsl`).

## Schnell anfangen

```rust
use mettgine::app::{self, AppConfig, Game, Stage};
use mettgine::render::{FrameInput, MeshDraw, Renderer};

struct MeinSpiel;

impl Game for MeinSpiel {
    fn renderer(&mut self, window: &winit::window::Window) -> app::Result<Renderer> {
        Renderer::simple(window, &[mettgine::shapes::cube()])
    }
    fn frame(&mut self, stage: &mut Stage, _dt: f32) {
        stage.draw(&FrameInput {
            meshes: &[MeshDraw::new(0, glam::Mat4::IDENTITY)],
            ..FrameInput::looking(glam::vec3(3.0, 2.0, 4.0), glam::Vec3::ZERO, stage.aspect())
        });
    }
}

fn main() -> app::Result<()> {
    app::run(AppConfig::new("meinspiel", "Mein Spiel", "v0.1.0"), || Ok(MeinSpiel))
}
```

Mehr: `examples/minimal.rs` (unter 60 Zeilen) und `examples/modern.rs` (Physik, Lampen, alle Schalter).

## Zuschaltbare Teile

Vorgabe ist klein; ein Spiel nimmt dazu, was es braucht: `mettgine = { path = "mettgine", features = ["gltf", "physics"] }`

| Feature | bringt |
|---|---|
| `mett-net` (Vorgabe) | Mehrspieler über den Mett Launcher |
| `images` | PNG und JPEG als Texturen |
| `gltf` | Modelle im glTF-Format |
| `audio-files` | WAV, OGG Vorbis |
| `physics` | Physik (rapier3d) |
| `ecs` | Entity-Component-System (hecs) |
| `full` | alles |

Die **moderne Grafik** (Materialien, HDR, Schattenstufen, viele Lampen, Raytracing, Pathtracing, Hochskalieren) braucht kein Feature: Sie entsteht, sobald `FrameInput::scene` gesetzt ist. Siehe [docs/moderne-grafik.md](docs/moderne-grafik.md).

**Reflex, Anti-Lag, Bildbegrenzung und Diagnose** sind ebenfalls wahlfrei, siehe [docs/verzoegerung.md](docs/verzoegerung.md). **HDR-Ausgabe:** [docs/hdr.md](docs/hdr.md). **FSR, XeSS, DLSS:** [docs/hochskalieren.md](docs/hochskalieren.md).

## In einem Spiel benutzen

Mettgine wird als **Git-Submodule** eingebunden und als Pfad-Abhängigkeit gebaut:

```powershell
git submodule add -b main https://github.com/Mett-Gaming/mettgine.git mettgine
git config -f .gitmodules submodule.mettgine.update merge
```

```toml
# Cargo.toml des Spiels
[workspace]
members = [".", "mettgine"]
default-members = [".", "mettgine"]

[dependencies]
mettgine = { path = "mettgine" }
```

Das Spiel legt seine Animationen als Aufzählung fest und implementiert dafür `mettgine::anim::ClipId` (Position in der `.k0a`-Datei, welche Clips Fortbewegung sind, welche im Stand weiterlaufen). Absturzberichte richtet es beim Start ein:

```rust
if let Some(c) = mettgine::crash_window::parse_args(&args) {
    mettgine::crash_window::run(&c, "MEIN SPIEL");
    return Ok(());
}
mettgine::crash::init("meinspiel", "MEIN SPIEL", VERSION);
```

Mit `app::run` ist das schon erledigt. Version und Windows-Manifest setzt im `build.rs` des Spiels `include!("mettgine/game_build.rs");` mit `mettgine_game("MEINSPIEL_VERSION")`.

Einstellungen, Spielstände, Protokoll und Absturzberichte liegen in `%APPDATA%\.mettlauncher\<spiel>` (`crash::data_dir`).

### Zeichnen

```rust
let mut renderer = Renderer::new(&window, &Config::new(&meshes, &skins, font))?;
renderer.draw(&FrameInput { view, lighting: Lighting::Sun(sun), post: Post::Clean(clean), /* … */ })?;
```

Neue Fähigkeiten kommen in diesen Renderer (`Config`, `FrameInput`, `Lighting`, `Post`), nie in einen zweiten.

### Zuschaltbares

Alles Sichtbare ist aus, bis das Spiel es einschaltet:

```rust
renderer.set_msaa(4);              // Kantenglättung (1, 2, 4, 8)
renderer.set_font_mipmaps(true);   // kleine Schrift ruhiger
renderer.queue_particles(particles.instances(cam_pos)); // vor jedem draw

// Messanzeige
meter.push(dt, &renderer.stats());
mettgine::profile::draw_overlay(&mut canvas, 8.0, 8.0, 14.0, &meter.lines());

// Raumklang
audio.set_listener(Listener { pos, forward, ..Default::default() });
audio.play_at(&sound, Emitter::at(wo));
```

Ob der Szenen-Renderer auf der eigenen Grafikkarte richtig zeichnet, prüft ohne Spieldaten und ohne sichtbares Fenster:

```powershell
cargo run --release --example gpu_check
cargo run --release --example gpu_check_modern <ordner>
cargo run --release --example gpu_bench <ordner>          # dichte Bilder mit Bildzeit
cargo run --release --example gpu_check_world <ordner>   # Vergleichsbilder vor/nach einem Umbau
```

## Arbeiten ohne Pull-Requests

Die verbindlichen Regeln für KI-Agenten (und alle anderen) stehen in [`AGENTS.md`](AGENTS.md).

Gearbeitet wird direkt auf `main`, mit Commits, Merges und Pushes. Das geht auch aus einem Spiel heraus, in dem die Engine als Submodule liegt.

**Spiel klonen oder aktualisieren** (die Engine landet dabei auf ihrem `main`-Zweig, nicht auf einem losen Commit):

```powershell
git clone --recursive https://github.com/Mett-Gaming/<spiel>.git
git submodule foreach "git checkout main"   # einmal nach dem Klonen
git pull
git submodule update --init --remote --merge
```

Nach dem Klonen steht die Engine zunächst auf einem losen Commit; `git checkout main` im Submodule (einmalig) setzt sie auf den Zweig. Danach führt `update --remote --merge` neue Engine-Stände per Merge in diesen Zweig.

**Die Engine aus dem Spiel heraus ändern:**

1. Im Ordner `mettgine/` ändern, dann dort committen.
2. Dort Änderungen vom Server holen und einarbeiten: `git fetch origin` und `git merge origin/main`, danach `git push origin main`.
3. Im Spiel-Repo den neuen Engine-Stand festhalten: `git add mettgine` und committen (zusammen mit den Spieländerungen, die ihn brauchen), dann wie gewohnt pushen.

So bekommt jedes andere Spiel die Änderung beim nächsten `git submodule update --remote --merge`. Vorher dort `cargo test` laufen lassen, falls sich eine Schnittstelle geändert hat.

**Versionen:** Wer eine Engine-Änderung pusht, setzt ein Tag `vX.Y.Z` (neue Funktion: MINOR + 1, Korrektur: PATCH + 1), damit Spiele sich auf einen Stand beziehen können.

## Lizenzen

Die Schrift-Atlanten und Modelle gehören den Spielen, nicht der Engine. Die Engine nutzt `font8x8` (MIT/Apache-2.0) für den Bitmap-Text und `png` (MIT/Apache-2.0) für das Startbild. Zuschaltbar: `gltf`, `image`, `hecs` (MIT/Apache-2.0), `rapier3d` (Apache-2.0); Klangdateien über `lewton` (MIT/Apache-2.0) und `hound` (Apache-2.0).

Startbild (`assets/splash.png`): Schriftzug aus Archivo Black (OFL), Mettbrötchen selbst gezeichnet, Dino aus dem Logo des Mett Launchers (Herkunft und Lizenz des Fotos noch nicht belegt).
