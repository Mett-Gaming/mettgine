# Mettgine

Die Engine der Mett-Gaming-Spiele, entstanden aus [KASSE 0](https://github.com/Mett-Gaming/kasse0). Rust und Vulkan, ohne großes Framework drumherum.

| Modul | Inhalt |
|---|---|
| `render` | Vulkan-Renderer: Welt-Boxen, Modelle (instanziert), animierte Figuren (Skinning), Hologramme, zweite Kamera (Überwachungsmonitor), UI-Ebene, VHS-Nachbearbeitung, Screenshots |
| `anim` | Knochen-Animation (`.k0a` v2): Überblenden, Einmal-Clips, Kopf-Blickziel, Zucken, Strecken, Kopfdrehung, Stop-Motion |
| `gpu` | Datenformate zwischen Spiel und Renderer: Welt-Boxen, Lichter, Modelle, Figuren (`.k0s`) |
| `ui` | Bitmap-Text und Formen für Menüs und HUD |
| `crash`, `crash_window` | Protokoll, Absturzberichte (auch bei Treiberabstürzen) und Absturzfenster |

Die Shader liegen in `shaders/` und werden beim Bauen mit naga zu SPIR-V übersetzt, ein Vulkan SDK braucht man nicht. Die Welt-Materialien (Poster, Schilder, Monitor) stammen noch aus KASSE 0.

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

## Arbeiten ohne Pull-Requests

Gearbeitet wird direkt auf `main`, mit Commits, Merges und Pushes. Das geht auch aus einem Spiel heraus, in dem die Engine als Submodule liegt.

**Spiel klonen oder aktualisieren** (die Engine landet dabei auf ihrem `main`-Zweig, nicht auf einem losen Commit):

```powershell
git clone --recursive https://github.com/Mett-Gaming/<spiel>.git
git pull
git submodule update --init --remote --merge
```

**Die Engine aus dem Spiel heraus ändern:**

1. Im Ordner `mettgine/` ändern, dann dort committen.
2. Dort Änderungen vom Server holen und einarbeiten: `git fetch origin` und `git merge origin/main`, danach `git push origin main`.
3. Im Spiel-Repo den neuen Engine-Stand festhalten: `git add mettgine` und committen (zusammen mit den Spieländerungen, die ihn brauchen), dann wie gewohnt pushen.

So bekommt jedes andere Spiel die Änderung beim nächsten `git submodule update --remote --merge`. Vorher dort `cargo test` laufen lassen, falls sich eine Schnittstelle geändert hat.

**Versionen:** Wer eine Engine-Änderung pusht, setzt ein Tag `vX.Y.Z` (neue Funktion: MINOR + 1, Korrektur: PATCH + 1), damit Spiele sich auf einen Stand beziehen können.

## Lizenzen

Die Schrift-Atlanten und Modelle gehören den Spielen, nicht der Engine. Die Engine nutzt `font8x8` (MIT/Apache-2.0) für den Bitmap-Text.
