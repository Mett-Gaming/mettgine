# Verzögerung, Bildbegrenzung, Diagnose

Alles wahlfrei. Ohne `Config::low_latency` und `Config::diagnostics` läuft der Renderer wie bisher.

```rust
let config = Config { low_latency: true, diagnostics: true, ..Config::new(&modelle, &[], schrift) };
renderer.set_low_latency(LowLatency::On);   // Off, On, Boost
renderer.set_fps_limit(120.0);              // 0 = unbegrenzt, geht auch ohne low_latency
```

| `latency_tech()` | Weg | Karten |
|---|---|---|
| `Reflex` | `VK_NV_low_latency2` | NVIDIA |
| `AntiLag` | `VK_AMD_anti_lag` | AMD |
| `PresentWait` | `VK_KHR_present_wait`: wartet, bis das vorige Bild zu sehen ist | je nach Treiber |
| `Basic` | wartet, bis die Karte mit dem vorigen Bild auf diesem Platz fertig ist | alle |

Fremde Bibliotheken sind nicht nötig. Gewartet wird **vor** den Eingaben: `app` ruft dazu `Renderer::begin_frame` auf. Wer die Schleife selbst schreibt, ruft es vor dem Auswerten der Eingaben auf. `latency_report()` liefert mit Reflex gemessene Zeiten.

Reflex übernimmt die Bildbegrenzung selbst, sonst gilt ein genauer Wartepunkt auf dem Prozessor (auch mit Anti-Lag: dessen Grenze hielt der Treiber im Versuch nicht ein).

**Diagnose:** `Config::diagnostics` trägt bei einem Ausfall der Grafikkarte den Grund des Treibers in den Absturzbericht ein (`VK_EXT_device_fault`) und füllt `FrameStats::memory_mb` (`VK_EXT_memory_budget`).

**Zum Testen:** `METTGINE_GPU=<Nummer oder Teil des Namens>` wählt die Grafikkarte, `METTGINE_LATENCY=wait` lässt Reflex und Anti-Lag weg.

```powershell
cargo run --release --example gpu_check_latency
```

Ausprobiert auf einer RTX 5090 (Reflex) und der Grafik eines Ryzen-Prozessors (Anti-Lag), unsichtbar. Das Gefühl im Spiel ist ungeprüft.
