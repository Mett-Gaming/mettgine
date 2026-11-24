# HDR-Ausgabe

Wahlfrei mit `Config::hdr_output`. Ohne das bleibt alles wie bisher.

```rust
let config = Config { hdr_output: true, ..Config::new(&modelle, &[], schrift) };
renderer.set_hdr_levels(HdrLevels { paper_white: 200.0, peak: 1000.0 });
renderer.set_hdr_output(false);   // jederzeit umschaltbar
renderer.hdr_active();            // zeigt der Bildschirm gerade wirklich HDR?
```

Das Bild entsteht in einem Zwischenbild mit Anzeigewerten (1 = Papierweiß, Lichter darüber) und wird am Schluss für den Bildschirm umgerechnet (`p_hdr.wgsl`): HDR10 (PQ, Rec. 2020), sonst scRGB, sonst SDR. Die Oberfläche mischt dadurch genau wie in SDR.

| Bild | in HDR |
|---|---|
| Moderne Grafik, `Post::Plain` oder `Post::Clean` | helle Stellen gehen bis `peak`; unter 75 % Anzeigewert ist das Bild gleich |
| Einfache Grafik | sieht aus wie SDR, nur Leuchtendes darf heller werden |
| `Post::Vhs` | wie SDR auf Papierweiß |
| Bildschirmfotos | nach SDR umgerechnet, Lichter werden weiß |

Geprüft wird mit `METTGINE_HDR=1` und `gpu_check_modern`. Wie es am Bildschirm aussieht, hat noch niemand beurteilt.
