# Spieldaten und Pakete: Anleitung für Spiele

Modelle, Figuren und Schriften liegen neben der exe in `daten/*.mpak`, nicht mehr in ihr. Ein zentrales Werkzeug der Engine (`mettpack`) prüft, baut und packt, lokal und auf GitHub gleich. Der Launcher lädt bei Aktualisierungen nur, was sich geändert hat.

## Einrichten
1. **Daten lesen** statt einbetten. Namen gelten ab `assets/`:
   ```rust
   use mettgine::asset::data;
   let mesh: MeshData = data::load("props/van.k0m");   // auch SkinData, AnimSet, String, Vec<u8>
   let font = data::read("fonts/schrift.ttf");         // rohe Bytes
   ```
   Beim Start laden, nicht im Bild-Takt. Fehlt etwas oder ist es beschädigt, erscheint eine verständliche Meldung und das Spiel endet.
2. **`spiel.toml`** ins Hauptverzeichnis (siehe unten).
3. **`Cargo.toml`:** in `members` und `default-members` zusätzlich `"mettgine/mpak"` und `"mettgine/mettpack"`.
4. **GitHub-Ablauf** (`.github/workflows/build.yml`; Änderungen dort braucht die Person vorher):
   ```yaml
   name: Build
   on:
     push: { branches: [main], tags: ["v*"] }
     pull_request:
     workflow_dispatch:
   permissions:
     contents: write
   jobs:
     spiel:
       uses: Mett-Gaming/mettgine/.github/workflows/spiel.yml@v0.21.0
       with: { titel: "MEIN SPIEL" }
   ```
   Die Marke hinter `@` bei neuen Engine-Fassungen nur anheben, wenn sich der Ablauf geändert hat.

Shader bleiben in der exe.

## spiel.toml
```toml
name = "HACKEPETER"          # Ordner im Zip und Anfang des Zip-Namens
package = "hackepeter"       # Name in Cargo.toml
exe = "HACKEPETER.exe"
ausgelassen = ["fonts/OFL.txt"]   # unter assets/, wird nicht ausgeliefert

[daten]                      # Paket -> Muster unter assets/, * steht für beliebige Zeichen
figuren = ["people/*"]
schriften = ["fonts/*.ttf"]

[schriften]                  # Schrift -> Beilage mit ihrer Lizenz
"fonts/ArchivoBlack-Regular.ttf" = "Lizenzen/ArchivoBlack-OFL.txt"

[beilagen]                   # Ziel im Paket -> Datei oder Ordner
"LIESMICH.txt" = "packaging/LIESMICH.txt"
"Lizenzen/ArchivoBlack-OFL.txt" = "assets/fonts/OFL.txt"
erfolge = "erfolge"
```
Jede Datei unter `assets/` gehört in genau ein Paket oder ist ausgelassen. DLLs brauchen einen Eintrag in `freigegebene_dlls` und vorher die Freigabe der Person.

## Befehle
| Befehl | Was er tut |
|---|---|
| `cargo run --release -p mettpack -- pruefen` | Beschreibung, Daten, Lizenzen, Erfolge prüfen |
| `… -- bauen` | dazu das Spiel bauen |
| `… -- packen --version v1.2.3` | dazu `dist/` füllen: Ordner, Zip und Dateiliste |

Ohne `--version` zählt die Marke in der CI, sonst `git describe`.

## Entwickeln
`cargo run` liest direkt aus `assets/`: geänderte Datei, Neustart, fertig. Gesucht wird in dieser Reihenfolge: `METTGINE_DATEN` (Ordner mit Paketen oder losen Dateien), `daten/` neben der exe, `assets/` im Projekt (erkannt an `spiel.toml`).

## Veröffentlichung
Neben dem Zip gehört `<Zip ohne .zip>.dateien.json` in die Veröffentlichung. Daran erkennt der Launcher (ab v0.7.0), was sich geändert hat, und lädt nur diese Stücke aus dem Zip. Fehlt die Liste oder klappt etwas nicht, lädt er das ganze Zip. Nach dem Veröffentlichen nichts mehr austauschen.

## Format `.mpak`
Siehe `mpak/src/lib.rs`. Jeder Eintrag ist einzeln gepackt (zlib) und hat eine Prüfsumme; im Zip liegt die Datei unverändert. Gleicher Inhalt ergibt dieselbe Datei.
