# Erfolge und Overlay: Anleitung für Spiele

Das Spiel sagt nur „Erfolg X ist erreicht“. Speichern, Einblenden, Ton, Liste und Overlay macht der Mett Launcher. Ohne Launcher oder mit einem alten Launcher passiert nichts, das Spiel läuft wie immer.

## In vier Schritten
1. **Erfolge beschreiben** in `erfolge/erfolge.json` (siehe unten).
2. **Ins Paket legen:** Der Ordner `erfolge` muss im Zip **neben der exe** liegen. Den Schritt im Bau-Ablauf (`.github/workflows`) und im lokalen `dist/` ergänzen. Änderungen in `.github/` braucht die Person vorher.
3. **Im Spiel melden** (siehe unten).
4. **Prüfen:** Test, dass jede im Code benutzte Id in der Datei steht (Vorbild: `src/achievements.rs` in KASSE 0).

## Die Datei
```json
{
  "schema": 1,
  "achievements": [
    { "id": "erste_kasse", "name": "Erste Kasse", "text": "Bezahl zum ersten Mal.", "icon": "erste_kasse.png" },
    { "id": "stammkunde", "name": "Stammkunde", "text": "Mach 100 Einkäufe.", "goal": 100 },
    { "id": "geheim", "name": "Nachtschicht", "text": "Bleib bis Mitternacht.", "hidden": true }
  ]
}
```

| Feld | Pflicht | Regel |
|---|---|---|
| `id` | ja | Kleinbuchstaben, Ziffern, `_`, höchstens 64 Zeichen. **Nie wieder ändern**, nie für etwas anderes benutzen. |
| `name` | ja | höchstens 80 Zeichen |
| `text` | nein | höchstens 200 Zeichen, sagt, was zu tun ist |
| `icon` | nein | PNG im Ordner `erfolge`, höchstens 256 KB, quadratisch, am besten 128 × 128. Ohne Bild erscheint ein Pokal. |
| `goal` | nein | Ziel für Fortschritt, sonst 1 |
| `hidden` | nein | Name und Text erst nach dem Freischalten |

Höchstens 200 Erfolge je Spiel. Ist die Datei kaputt, hat das Spiel keine Erfolge; alles andere läuft weiter. Bilder brauchen eine freie Lizenz und einen Eintrag in `CREDITS.md`.

## Im Code (Mettgine)
```rust
fn frame(&mut self, stage: &mut Stage, dt: f32) {
    if erste_kasse_bezahlt {
        stage.achievements.unlock("erste_kasse");
    }
    stage.achievements.progress("stammkunde", einkaeufe_insgesamt);
}
```
- Beides darf **jedes Bild** aufgerufen werden: Es wartet nie und sendet nur Neues.
- `progress` bekommt den **ganzen Stand**, nicht den Zuwachs. Er sinkt nie. Beim Ziel wird von selbst freigeschaltet, bei 25, 50 und 75 % erscheint eine kleine Einblendung (ab Ziel 4).
- Zähler über mehrere Sitzungen **speichert das Spiel** (`settings` oder `save`). Beim Start einmal alles schon Erreichte melden (`Game::ready`): So geht nichts verloren, was ohne Launcher gespielt wurde.
- Wer `Achievements` in eigenen Strukturen braucht: klonen (`stage.achievements.clone()`), das ist billig.
- Mehrspieler: Jeder Rechner meldet nur für den **eigenen** Spieler.
- Cheats, Konsole, Kurztests und Vorführungen melden nichts.

Im Protokoll des Launchers steht, wenn ein Spiel eine unbekannte Id meldet.

## Overlay
- Einblendungen und Leiste erscheinen ohne Zutun des Spiels, bei jedem Vulkan-Spiel.
- Umschalt+Tab (einstellbar) bzw. Select+Start öffnet das Feld. `app::run` erledigt das: Maus, Tasten und Controller gehen dann nicht ans Spiel, die Maus wird frei und danach wieder gefangen.
- `stage.overlay_open()` sagt, ob es offen ist. **Im Einzelspieler anhalten**, im Mehrspieler weiterlaufen.
- Freie Ecken: Einblendungen stehen unten rechts, die Leiste oben links (beides einstellbar). Wichtiges dort nicht dauerhaft anzeigen.

## Ohne Mettgine (Protokoll)
Nur wenn `METT_LAUNCHER_LINK` gesetzt ist: TCP zu `METT_PRESENCE_ADDR` (nur localhost), je Zeile ein JSON-Objekt, höchstens 4096 Byte.

| Richtung | Zeile | Bedeutung |
|---|---|---|
| Spiel → Launcher | `{"token":"<METT_PRESENCE_TOKEN>","link":1}` | erste Zeile |
| Spiel → Launcher | `{"unlock":"id","n":1}` | freischalten |
| Spiel → Launcher | `{"progress":"id","value":40,"n":2}` | Stand melden |
| Launcher → Spiel | `{"ack":2}` | gespeichert; ohne Antwort nach 5 s noch einmal senden |
| Spiel → Launcher | `{"overlay":"toggle"}` | Tastenkürzel gedrückt (`METT_OVERLAY_KEY`, z. B. `shift+tab`) |
| Launcher → Spiel | `{"overlay":true}` / `false` | Feld offen bzw. zu |
| Spiel → Launcher | `{"ui":["move",x,y]}`, `click` ebenso | Maus, 0 bis 1 im Fenster |
| Spiel → Launcher | `{"ui":["scroll",1]}`, `["up"]`, `["down"]`, `["ok"]`, `["back"]` | Bedienung |

Höchstens 300 Erfolgsmeldungen in zehn Sekunden. Die Zeilen von `presence` laufen über eine eigene Verbindung.

## Selbst ausprobieren
`cargo run --release --example erfolge` meldet drei Erfolge, sobald ein Launcher da ist.

## Grenzen
- Gespeichert wird nur auf diesem Rechner. Konten kommen später.
- Der Launcher muss laufen. Schließt man ihn während eines Spiels, fragt er, ob er im Hintergrund bleiben soll.
