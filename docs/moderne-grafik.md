# Moderne Grafik, Raytracing, Hochskalieren

Alles hier ist **wahlfrei**. Ohne `FrameInput::scene` entsteht nichts davon, und das Bild der einfachen Grafik bleibt Byte für Byte gleich.

## So geht es an

```rust
let modell = renderer.add_model(&daten)?;          // oder asset::gltf::parse(..)?.upload(&mut renderer)?
let material = renderer.add_material(&Material { roughness: 0.4, ..Default::default() })?;
let scene = SceneInput {
    objects: &[ObjectDraw::new(modell, material, lage)],
    features: Features::all(),                     // Vorgabe: alles aus
    ..SceneInput::new(Camera::looking(auge, ziel, stage.aspect()))
};
stage.draw(&FrameInput { post: Post::Plain(0.0), scene: Some(&scene), ..Default::default() });
```

Modelle (`MeshDraw`), Figuren (`SkinDraw`), feste Modelle und Welt-Boxen werden mitgezeichnet. Die eigenen Materialien der Welt-Boxen (`WorldShaders`) und die zweite Kamera gibt es nur in der einfachen Grafik.

| Schalter (`Features`) | Wirkung |
|---|---|
| `tonemap`, `exposure` | Helligkeit aufs Fenster bringen, fest oder automatisch |
| `sun_shadow_size`, `lamp_shadows` | Sonne mit vier Schattenstufen; Lampen im Schatten-Atlas (64 Felder: Kegel 1, Punktlampe 6) |
| `ssao`, `bloom`, `fog` | Verdeckung in Ecken, Überstrahlen, Höhennebel |
| `anti_alias` | FXAA oder TAA |
| `render_scale`, `sharpen` | Szene kleiner rechnen und mit TAA hochskalieren; nachschärfen |
| – (`SceneInput::decals`) | Aufkleber auf Flächen |
| `ray_tracing` | Schatten, Verdeckung, Streulicht, Spiegelungen aus Strahlen |
| `path_tracing` | das ganze Bild aus Strahlen |

## Raytracing

Es läuft direkt über Vulkan (`VK_KHR_acceleration_structure`, `VK_KHR_ray_query`) und damit auf jeder Karte, die diese Erweiterungen anbietet. Einschalten mit `Config::ray_tracing`; ob es klappt, sagt `Renderer::ray_tracing()`. Sonst gilt die übliche Grafik. Ausprobiert wurde es bisher nur auf einer NVIDIA-Karte.

Entrauscht wird mit eigenen Durchgängen: über mehrere Bilder mitteln (`p_accum`), dann kantentreu glätten (`p_atrous`).

**Grenzen:**
- Animierte Figuren erscheinen nicht in Spiegelungen und im Pathtracing. Ihre Schatten kommen weiter aus der Schattenkarte.
- Halbtransparentes und Partikel fehlen im Pathtracing.
- Strahlen sehen höchstens 512 Farbtexturen; Normalenkarten und Rauheitskarten getroffener Flächen zählen nicht.
- Bei schnellen Bewegungen ziehen Streulicht und Spiegelungen kurz nach.

## Techniken der Hersteller

Nicht eingebaut. Sie brauchen fremde, teils geschlossene Pakete je Hersteller, die mit jedem Spiel ausgeliefert und gepflegt werden müssten. Das passt nicht zu den Regeln der Engine: kein SDK zum Bauen nötig, Abhängigkeiten nur unter MIT, Apache-2.0, BSD, Zlib oder CC0.

| Technik | Was dafür nötig wäre | Was die Engine stattdessen hat |
|---|---|---|
| DLSS (Hochskalieren), Streamline | NVIDIA-Bibliotheken mit eigener Lizenz, Anmeldung der Anwendung, eigene Schicht um Instanz, Gerät und Swapchain | `render_scale` mit TAA |
| DLSS Frame Generation | Streamline samt Reflex; ersetzt das Anzeigen der Bilder | – |
| DLSS Ray Reconstruction | wie DLSS, zusätzlich Albedo, Spiegelanteil, Rauheit und Trefferabstand als Bilder | eigenes Entrauschen |
| FSR (Hochskalieren, Bilderzeugung) | FidelityFX SDK (C++); die Bilderzeugung ersetzt die Swapchain | `render_scale` mit TAA, `sharpen` |
| FSR Ray Regeneration | nur bestimmte AMD-Karten und Treiber | eigenes Entrauschen |
| XeSS | Bibliothek von Intel mit eigener Lizenz | `render_scale` mit TAA |

**Falls es doch dazukommen soll:** Tiefe, Bewegung, Normale mit Rauheit, Kameraversatz und Belichtung liegen schon als Bilder bzw. Werte vor (`render/modern/targets.rs`, `gpu::jitter`). Anzusetzen wäre an der Stelle von `Post::taa` in `render/modern/post.rs`, als eigenes Cargo-Feature. Vorher Lizenz und Weitergabe klären.

## Prüfen

```powershell
cargo run --release --example gpu_check_modern <ordner>   # unsichtbar, legt Bilder ab
cargo run --release --features full --example modern      # zum Ausprobieren, Tab öffnet das Menü
```
