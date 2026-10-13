# egui-viewport

Записная книжка для этого крейта. Дополняй при изменениях.

## Что это
Generic 2-D image viewport widget for egui 0.34. Pan / zoom / fit / home (100%),
pixel-coordinate + RGBA readout HUD, channel isolation (R/G/B/A/RGB) reported as
state, and click/drag scrub reporting. The widget owns NO image and NO GPU — the
caller uploads its image to an egui texture and passes the `TextureId` + size.

## Откуда взято
Gathered from `exr-rs/crates/exr-view/src/viewport.rs` and the byte-identical
`openvdb-rs/crates/vdb-view/src/viewport.rs`. Those were coupled to app types:
`ImageBackend`, `ColorPass`, `DisplayGpu`, `ViewControls`, `Playback`, and to
`wgpu`/`egui-wgpu`. All of that was stripped out — see "Decoupling" below.

## Public API (src/egui_viewport.rs)
- `enum Channel { Rgb, R, G, B, A }` — isolated channel intent (serde).
- `struct ViewportState { pan: Vec2, zoom: f32, fit_zoom: f32, channel: Channel,
  picked_pixel: Option<(u32,u32)> }` — persistent, caller-owned, serde-derived.
  - `Default`, `fit()`, `home()`, `invalidate_fit()`.
- `struct ViewportImage { texture: TextureId, size: [u32;2] }` + `new(...)`.
- `struct ScrubConfig { range: Option<(i32,i32)>, playhead: Option<f32>,
  playhead_alpha: f32 }` (`Default`).
- `struct ViewportResponse { response: egui::Response,
  hovered_pixel: Option<(u32,u32)>, scrub_to: Option<f32> }`.
- `fn show(ui, state: &mut ViewportState, image: ViewportImage,
  scrub: ScrubConfig, read_pixel: impl Fn(u32,u32)->Option<[f32;4]>)
  -> ViewportResponse`.

## Навигация
MMB drag = pan · RMB drag = zoom about centre · wheel = zoom to cursor ·
Ctrl + `+`/`=`/`-` = zoom step · `F` = fit · `H`/`0` = 1:1 · `R`/`G`/`B`/`A` =
isolate channel (toggle back to RGB). LMB: scrub (if `ScrubConfig` enabled) else
pin pixel readout.

## Decoupling decisions
- **GPU/colour pass → caller.** Source ran a wgpu colour pass and registered the
  texture inside `show`. Here the caller uploads + colour-transforms and passes a
  ready `TextureId` + pixel size. No wgpu / egui-wgpu dep.
- **Channel isolation = enum in state, NOT a tint.** A `Color32` tint cannot
  express "splat one channel to grayscale" (R-only / A-matte views) — it only
  scales channels. So the widget reports the `Channel` intent and the caller
  applies it in its own shader/colour pass, where full semantics exist. This also
  keeps the widget a pure input/paint mapper. (Caller reads `state.channel`.)
- **Pixel readout = `read_pixel` callback** instead of `ImageBackend::pixel_at`.
  Pass `|_,_| None` to disable the value readout (coordinate still shown + always
  reported via `ViewportResponse::hovered_pixel`).
- **Scrub/playhead = `ScrubConfig`** instead of `seq_pos`/`playhead_alpha`/
  `seq_range` + `Playback`. `show` returns `scrub_to: Option<f32>` (0..=1); the
  caller maps it to a frame and owns the seek.
- **`exposure`/`gamma`/`active_layer` dropped** — they belonged to the caller's
  colour pass, never to viewport navigation.

## Math preserved verbatim
`fit_zoom = min(rect/size)`, centre-pivot zoom (`pan *= scale`), cursor-pivot
zoom (`pan = delta + (pan - delta) * scale`), `screen_to_pixel`
(`floor((pos - img_min)/zoom)`), checker tiling, Flame scrub-cursor frame math.

## Gotchas / ловушки
- `egui = { features = ["serde"] }` is REQUIRED because `ViewportState.pan` is an
  `egui::Vec2` and we serde the whole state. Without it: E0277 on `Serialize`.
- `Cargo.toml` has a bare top-level `[workspace]` table so the crate is its own
  workspace root and builds standalone (the parent workspace's member list need
  not be edited). Remove that table if/when this is added to the parent members.
- Uses stable let-chains (`if a && let Some(x) = ...`) → needs rust 1.88+.
- The RMB-zoom reads RAW pointer state (`button_down` + `pointer.delta()`), not
  just `dragged_by(Secondary)`, because egui suppresses the claimed-drag signal
  for small/slow right-drags (context-menu arbitration). Don't "simplify" it.

## Status
- [x] Decoupled from exr/vdb app types + wgpu.
- [x] `cargo check` clean: 0 errors, 0 warnings (rust 1.88, edition 2024).
- [ ] No example/integration test yet (no eframe dep here; exercised by the
      `egui-widgets-demo` gallery if/when added to the parent workspace).
