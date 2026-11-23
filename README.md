# Mettgine

Die Engine der Mett-Gaming-Spiele, entstanden aus [KASSE 0](https://github.com/Mett-Gaming/kasse0). Rust und Vulkan, ohne großes Framework drumherum.

| Modul | Inhalt |
|---|---|
| `render` | Der eine Renderer für alle Spiele (Vulkan): Sonne mit Schattenkarte oder Räume mit Lampen, Welt-Boxen, Modelle (instanziert), animierte Figuren, Hologramme, Partikel, zweite Kamera, UI, saubere oder Videoband-Nachbearbeitung, Screenshots. Sortiert Unsichtbares aus, Puffer wachsen mit |
| `particles` | Partikel-Rechnung (Staub, Rauch, Funken), gezeichnet von `render` |
| `cull` | Sichtfeld und Hüllkugeln |
| `collide` | Kollision ohne Physik-Bibliothek: Kisten, Kapseln, Strahlen, Sichtlinien, Stufen |
| `splash` | Startbild „Gemacht mit Mettgine“: erscheint in jedem Spiel von selbst, nicht überspringbar, immer in Fensterauflösung. Spiele prüfen mit `splash_active()`, ob sie Eingaben annehmen |
| `profile` | Messanzeige: Prozessor- und Grafikzeiten, Ruckler, Zeichenbefehle |
| `anim` | Knochen-Animation (`.k0a` v2): Überblenden, Einmal-Clips, Kopf-Blickziel, Zucken, Strecken, Kopfdrehung, Stop-Motion |
| `gpu` | Datenformate zwischen Spiel und Renderer: Welt-Boxen, Lichter, Modelle, Figuren (`.k0s`) |
| `ui` | Bitmap-Text und Formen für Menüs und HUD |
| `canvas`, `text` | Zeichenfläche für Menüs mit Schrift aus TTF/OTF |
| `input` | Tastatur, Maus, Controller |
| `audio` | Effekte, Musik, Klang-Bausteine (`synth`), Raumklang (`spatial`) |
| `net` | Mehrspieler-Verbindungen; `net::sync` für Abgleich, Zwischenbilder und Vorhersage |
| `settings` | Einstellungen lesen und speichern |
| `crash`, `crash_window` | Protokoll, Absturzberichte (auch bei Treiberabstürzen) und Absturzfenster |

Die Shader liegen in `shaders/` und werden beim Bauen mit naga zu SPIR-V übersetzt, ein Vulkan SDK braucht man nicht. Die vorgegebenen Welt-Materialien (Poster, Schilder, Monitor) stammen aus KASSE 0 und liegen in `_world_kasse0.wgsl`; eigene bringt ein Spiel mit `Config::world` mit.

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

Die Schrift-Atlanten und Modelle gehören den Spielen, nicht der Engine. Die Engine nutzt `font8x8` (MIT/Apache-2.0) für den Bitmap-Text und `png` (MIT/Apache-2.0) für das Startbild.

Startbild (`assets/splash.png`): Schriftzug aus Archivo Black (OFL), Mettbrötchen selbst gezeichnet, Dino aus dem Logo des Mett Launchers (Herkunft und Lizenz des Fotos noch nicht belegt).
