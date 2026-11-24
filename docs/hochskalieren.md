# Hochskalieren und Entrauschen mit Techniken der Hersteller

Alles wahlfrei, in drei Schichten: Cargo-Feature, `Config::upscalers`, `Features::upscaler`. Fehlt eine davon, die DLL oder die passende Karte, gilt die eigene Technik (`anti_alias`, `render_scale`). Zum Bauen ist kein SDK nötig, im Archiv liegt keine fremde DLL.

```toml
mettgine = { path = "mettgine", features = ["fsr", "xess", "streamline"] }
```

```rust
let config = Config { upscalers: &[Upscaler::Dlss, Upscaler::Fsr, Upscaler::Xess], ..Config::new(..) };
renderer.upscalers();   // was hier wirklich geht: [(Dlss, "DLSS 310.9.16"), ..]
Features {
    upscaler: Upscaler::Dlss,
    render_scale: Quality::Performance.scale(),   // 0,5; Native = nur Kantenglättung (DLAA)
    ray_reconstruction: true,                      // DLSS entrauscht auch die Strahlen
    ..Features::all()
}
```

| Technik | Feature | DLLs neben der exe | Karten |
|---|---|---|---|
| FSR | `fsr` | `amd_fidelityfx_loader_vk.dll`, `amd_fidelityfx_upscaler_vk.dll` | alle; welches Modell läuft, entscheidet AMDs DLL (FSR 4 nur auf neueren Radeon) |
| XeSS | `xess` | `libxess.dll` | viele |
| DLSS, DLAA | `streamline` | `sl.interposer.dll`, `sl.common.dll`, `sl.dlss.dll`, `nvngx_dlss.dll` | NVIDIA RTX |
| Ray Reconstruction | `streamline` | dazu `sl.dlss_d.dll`, `nvngx_dlssd.dll` | NVIDIA RTX |

**Voraussetzung für gute Bilder:** Kennungen für bewegte Dinge (`MotionIds`, siehe [moderne-grafik.md](moderne-grafik.md)). Partikel und Halbtransparentes melden sich selbst über eine Maske.

**Mit DLSS** lädt die Engine Vulkan über Streamlines Interposer. Das geht je Prozess nur für einen Renderer. Nachladen aus dem Netz ist abgeschaltet.

## Lizenzen

Die DLLs gehören den Herstellern und dürfen nur zusammen mit einem Spiel und ihren Lizenztexten weitergegeben werden. Vor einer Weitergabe entscheidet die Person:

- **NVIDIA** (RTX SDKs, 14.3.2024): DLSS nennen und NVIDIAs Marke auf Startbild und in den Credits zeigen, die Markennutzung vorher vorlegen, vor kommerzieller Veröffentlichung melden, nicht unter eine Open-Source-Lizenz stellen.
- **AMD** (FidelityFX SDK 2.4): binär mit Lizenztext.
- **Intel** (Simplified Software License): unverändert mit Lizenztext.

## Messwerte

RTX 5090, 1280 x 720, Abstand zum Bild mit vierfacher Abtastung (kleiner = besser), halbe Auflösung:

| | Standbild | Kamerafahrt | Zeit im Bild |
|---|---|---|---|
| eigenes TAA | 4,2 | 4,5 | 0,07 ms |
| FSR 3.1.6 | 3,6 | 4,1 | 0,15 ms |
| XeSS | 2,6 | 3,5 | 0,54 ms |
| DLSS | 2,1 | 2,5 | 0,30 ms |

Pathtracing mit einem Strahl je Bildpunkt, halbe Auflösung, Abstand zum langen Mittel: eigenes Entrauschen 4,3 (Fahrt 5,8), Ray Reconstruction 1,8 (Fahrt 2,4).

## Prüfen

```powershell
$env:METTGINE_VENDOR = "<Ordner mit DLLs>;<weiterer Ordner>"   # nur zum Entwickeln
cargo run --release --features full --example gpu_check_upscale <ordner>
```

Ungeprüft: Spielgefühl, eigenständige Radeon- und Arc-Karten, FSR 4.
