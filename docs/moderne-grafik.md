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
| `upscaler`, `ray_reconstruction` | Techniken der Hersteller, siehe [hochskalieren.md](hochskalieren.md) |
| `ray_tracing` | Schatten, Verdeckung, Streulicht, Spiegelungen aus Strahlen |
| `path_tracing` | das ganze Bild aus Strahlen |

## Bewegung

TAA und Hochskalieren müssen wissen, wo ein Ding im letzten Bild war. Dafür gibt das Spiel Kennungen mit, parallel zu seinen Zeichenaufrufen:

```rust
ids: MotionIds { meshes: &[1, 2, 3], skins: &[1], ..Default::default() },   // in SceneInput; 0 = keine
```

Die Engine merkt sich Lage und Knochen ein Bild lang. Ohne Kennung gilt ein Ding als unbewegt und zieht Schlieren. Der Himmel ist immer dabei. Ein gesetztes `ObjectDraw::previous` hat Vorrang. Partikel und Halbtransparentes haben keine Bewegung.

## Raytracing

Es läuft direkt über Vulkan (`VK_KHR_acceleration_structure`, `VK_KHR_ray_query`) und damit auf jeder Karte, die diese Erweiterungen anbietet. Einschalten mit `Config::ray_tracing`; ob es klappt, sagt `Renderer::ray_tracing()`. Sonst gilt die übliche Grafik. Ausprobiert wurde es bisher nur auf einer NVIDIA-Karte.

Entrauscht wird mit eigenen Durchgängen: über mehrere Bilder mitteln (`p_accum`), dann kantentreu glätten (`p_atrous`).

**Grenzen:**
- Höchstens 128 animierte Figuren je Bild sind für Strahlen sichtbar; sie werden jedes Bild auf der Karte ausgerechnet.
- Aufkleber, Halbtransparentes und Partikel werden im Pathtracing darübergezeichnet (außer beim Mitteln von Standbildern), in Spiegelungen fehlen sie.
- `ray_tracing.lamps` beleuchtet aus allen Lampen mit Schatten aus Strahlen (ReSTIR: Kandidaten, letztes Bild, Nachbarn); ein Gehäuse bis 25 cm um die Lampe wirft keinen Schatten.
- Nicht eingebaut: Opacity Micromaps (naga 24 kann nur Strahlabfragen ohne Rückfragen, und die Spiele haben kaum Blattwerk), ReSTIR für Streulicht, Hit-Positionen vom Treiber.
- Strahlen sehen höchstens 512 Farbtexturen; Normalenkarten und Rauheitskarten getroffener Flächen zählen nicht.
- Bei schnellen Bewegungen ziehen Streulicht und Spiegelungen kurz nach.

## Techniken der Hersteller

FSR, XeSS, DLSS und Ray Reconstruction: siehe [hochskalieren.md](hochskalieren.md).

Nicht eingebaut, weil es sie (Stand November 2026) nur für DirectX 12 gibt: FSR Ray Regeneration, XeSS Frame Generation, XeLL.

## Viel Geometrie

`Config::gpu_culling` und `Features::gpu_culling` sortieren Verdecktes auf der Grafikkarte aus: eine Tiefenpyramide und indirekte Zeichenbefehle in zwei Durchgängen, sodass nie etwas fehlt, das gerade erst sichtbar wird. Schatten und Halbtransparentes sind nicht dabei. Testfall mit 4000 feinen Kugeln hinter einer Wand: 9,8 → 0,27 ms (RTX 5090), Bild gleich. Genau aufeinanderliegende Flächen können flimmern, weil sich ihre Reihenfolge ändert.

Nicht eingebaut:
- **Mesh-Shader:** naga 24 kann keine; Aussortieren in Stücken eines Modells fehlt damit.
- **Async Compute:** SSAO und Bloom kosten im Testbild zusammen unter 0,1 ms; eine zweite Warteschlange wurde deshalb nicht ausprobiert.

## Prüfen

```powershell
cargo run --release --example gpu_check_modern <ordner>   # unsichtbar, legt Bilder ab
cargo run --release --features full --example modern      # zum Ausprobieren, Tab öffnet das Menü
```
