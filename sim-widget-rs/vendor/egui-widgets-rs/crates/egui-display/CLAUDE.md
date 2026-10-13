# CLAUDE.md — `crates/egui-display`

Working notes for this crate; keep current. Conventions: repo-root `CLAUDE.md`.

## What
Window display output: an extended-sRGB float canvas (`present::CANVAS_FORMAT`, 1.0 = SDR
white) that egui and the image tiles draw into, then one transfer/gamut/dither pass into
the surface the user picked (`Output`: SDR 8/10-bit, HDR10 PQ, HLG, scRGB). `DisplayPrefs`
+ `settings_ui` are the Preferences half. Consumers: Playa (`playa-app/src/display_host.rs`),
exr-view (`app.rs`, `present.rs`). Both present through `PresentPass`.

## Viewer backdrop (`src/backdrop.rs`, `src/backdrop.wgsl`, the present shader)
What shows under the image's alpha, composited INSIDE the present pass.
- `Backdrop { kind: BackdropKind, checker: Checker, color }` — one struct, not a
  data-carrying enum, so cycling kinds never forgets the checker size / custom colour.
  Colours are sRGB swatches relative to SDR white. Default checker 0.18 / 0.25 of white in
  linear light; Gray50 = code 0.5.
- Viewport: `show_backdrop(painter, image_rect, ..)` = a paint callback that clears the
  rect of the canvas to (0,0,0,0) (blend off, `ClearToBackdrop`) + publishes the
  `BackdropUniform` in ctx temp data. Host: `present.backdrop = take_backdrop(&ctx)`.
- CANVAS CONTRACT (module docs of `backdrop.rs`): under the backdrop the canvas holds ENCODED
  ASSOCIATED light, `rgb = oetf(L_premul)`, alpha = coverage. Hosts (exr-view tiles, Playa) write
  that; egui's UI (`enc(S)*u`) agrees at u = 0 and 1, and differs for light partial-alpha UI over a
  transparent canvas (quantified in the docs). NEVER divide by alpha: emission (a=0, rgb>0) is legal.
- Present shader: where canvas alpha < 1 (NaN = opaque): `oetf(eotf(rgb) + backdrop_lin*(1-a))`,
  then the unchanged output switch. `backdrop: None`
  = the old behaviour exactly. CPU twins: `BackdropUniform::composite`, `encode_over`.
- `BackdropUniform` is 64 B: colours, grid, and `clip_px` = the cleared image rect in pixels
  (rounded like egui-wgpu's callback viewport). The composite runs only inside it, so a canvas
  that is not opaque elsewhere (a host clearing transparent, translucent UI fills) never shows
  the backdrop. `Params` = 16 B head + 64 B uniform + `bd_on` + 12 B tail = 96 B.
- Overlays over the hole (viewport chrome, OFX interacts, HUD text) are fine: egui's blend keeps
  canvas alpha as "over" coverage, so the backdrop goes under overlay + image exactly once.
  GPU test `overlay_over_the_hole_shows_the_backdrop_once` drives the real egui-wgpu renderer.
- `paint_backdrop`: SDR-only painter path (2x2 Repeat + Nearest tile) for eframe hosts.
- Checker parity is FLOAT (`s - 2 floor(s/2)`), matching the CPU `floor` on negative cells.

## Screenshots (`src/capture.rs`, `src/screenshot.rs`)
- `PresentPass::capture(device, queue, output, target) -> PendingCapture`; `.wait(device)`
  (any thread) -> `Capture`. Same module, layout and `params_for` as the swapchain draw,
  pipeline built for `capture_format(output)` (Rgba8Unorm / Rgb10a2Unorm / Rgba16Float).
  Readback via `gpu_mem::wait::{submit, map_read}` — never a blocking `Device::poll`.
- `screenshot` (no wgpu): `Capture { output, width, height, white_nits, peak_nits, pixels }`,
  `Pixels::{Rgba8, Rgba16 (10-bit widened by bit replication), RgbaF16}`, `primaries()`,
  `transfer()`, `cicp()`, `content_light()`, `write_png(W)` / `write_png_measured(W)`
  (an encoder into the host's writer). PQ/HLG PNG: 16-bit + cICP
  (9/16/0/1 or 9/18/0/1) + mDCV (peak) [+ cLLI for PQ, measured] written with png's
  `Writer::write_chunk` (png 0.18 parses but does not write them).
  NO file writing and NO EXR here (2026-10-11): `Capture::save` / `save_measured` /
  `write_exr` and the `exr` feature were removed so egui-display carries no exr-core
  (exr-core lives in exr-rs, which depends on this repo: a cycle that put two copies of
  exr-core/deflate-rs/imath-rs into an exr-rs build). Hosts own the destination (temp
  sibling + atomic rename) and, for scRGB, the half-float EXR writer.
- `ScreenshotMode { AsDisplayed (default), Sdr }`: the app persists it; `Sdr` captures
  `Output::Sdr8`.

## Export publication (`src/export.rs`)
PNG and video exports write a `tempfile` sibling of the destination (`temp_for`: same
directory so the rename is atomic, same extension, deleted on drop/error/cancel) and
`publish` renames it over the destination (`persist_noclobber` without overwrite).
`check_overwrite` refuses an existing output up front; `ensure_distinct` refuses encoding a
single frame onto its own input. This replaced ffmpeg-rs's `av_util_core::outfile`
(2026-10-10): that crate is LGPL-2.1+, and egui-display is BSD-3 and linked into BSD apps
(exr-rs), so it must not depend on ffmpeg-rs.

## Tests
Unit: backdrop (serde, cycle, levels, grid, composite, painter, show/take, WGSL layout),
`backdrop_oracle_on_every_output`, screenshot (cICP bytes + decode, cLLI),
10-bit widening. GPU (`tests/gpu.rs`, `#[ignore]`, run with `-- --ignored`):
`present_composites_the_backdrop_like_its_twin` (every output x every kind),
`capture_equals_the_swapchain_signal_on_every_output` (window pass SDR, capture other
outputs), `overlay_over_the_hole_shows_the_backdrop_once`, `write_reference_screenshots`
(writes PNGs of the non-scRGB outputs to `EGUI_DISPLAY_SHOTS`). Mutants checked: composite factor, capture encoding, clip test.

## Gotchas
- After restoring a file from a copy, touch it: cargo rebuilds by mtime.
- exr-view's tests build `DisplayPrefs` literals: adding a field there breaks them, which is
  why `ScreenshotMode` is a separate type the app persists.
