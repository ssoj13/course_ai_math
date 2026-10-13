# egui-scopes

Working notebook. Keep current.

## What
GPU video **scopes** for egui + wgpu — **histogram**, **waveform**, **vectorscope**,
**RGB parade** and **false colour** over an input texture, computed on the GPU
(compute passes → draw) and presented as an `egui::Image`. Harvested from the exr /
openvdb viewers; the parade + false-colour scopes were added on top.

## The five scopes
- **histogram / waveform / vectorscope** — the original three; scatter-into-bins scopes
  sharing `scope::ScopePanel` (below).
- **RGB parade** (`parade.rs`, `GpuParadePanel`) — three single-channel waveforms laid
  side by side (R | G | B thirds). REUSES the waveform's clear/scatter/reduce passes
  verbatim (same bins layout); only its draw pass differs, so the two cannot drift.
- **false colour** (`falsecolor.rs`, `GpuFalseColorPanel`) — a per-pixel exposure LUT,
  NOT a scatter scope: a single compute pass maps each source texel's luma → an ARRI
  ALEXA false-colour band and writes it at SOURCE resolution (egui scales it, so it
  reports no bucket crop). Stops (on the sRGB-encoded signal / IRE): purple `<2.5`
  (black clip), blue `2.5–4`, green `38–42` (18% grey), pink `52–56` (skin), yellow
  `97–99`, red `>99` (white clip); neutral grey elsewhere. See the module doc for why
  the linear source is taken through the sRGB OETF first.

## Architecture — ONE shared engine (`scope::ScopePanel`)
The four scatter scopes (histogram / waveform / vectorscope / parade) are the SAME
pipeline shape, so they share `scope.rs` instead of each carrying its own
clear/scatter/reduce/draw plumbing:
- **`ScopeSpec`** — the per-scope recipe (a `const`): `label`, `bins_bytes` (the
  accumulation-buffer size), `clear_wgsl` / `scatter_wgsl` / `reduce_wgsl: Option<&str>`
  / `draw_wgsl`, and the `clear_groups` / `reduce_groups` dispatch counts. The reduce
  pass is OPTIONAL.
- **`ScopePanel`** — owns the pipelines (`ScopePipes`, built lazily from the spec +
  device) + the output texture id. `tick()` runs clear → scatter → [reduce?] → draw;
  `release()` frees GPU state; `texture_id()` feeds the egui Image.
- Each scope (`histogram.rs` / `waveform.rs` / `vectorscope.rs`) is now THIN: the
  verbatim WGSL consts + a `*_SPEC: ScopeSpec` + `pub struct Gpu*Panel(ScopePanel)`
  delegating `impl GpuPanel`. Dispatch preserved verbatim (scatter ÷16, draw ÷8;
  histogram clear 17 / reduce 4, waveform 771 / 4, vectorscope 1024 / no reduce).

## Why
The three scopes were ~450 lines of duplicated pipeline plumbing. `ScopeSpec` +
`ScopePanel` deduplicate that to a single codepath — a scope is now just its WGSL +
sizes. Behaviour is byte-identical to the pre-dedup version.

## Gotchas
- wgpu 29 / egui-wgpu 0.34 sample only FILTERABLE formats: the displayed scope
  texture is Rgba8/Rgba16F; the source is read via `textureLoad` (Rgba32Float is fine
  as a COMPUTE source, not as a sampled texture).
- The reduce pass is `Option` — a reduce-less scope (vectorscope) passes
  `reduce_wgsl: None` + `reduce_groups: 0` and fits the same shape; don't special-case it.
- Colours (histogram hues etc.) are intrinsic to the WGSL, not config tokens.
- Vectorscope: scatter, dot readout and R/Y/G/C/B/M markers share ONE chroma transform (WGSL prelude
  `chroma_wgsl!`, spliced via `concat!`; test `wgsl_prelude_matches_rust_consts` pins the Rust twins).
  Pure red sits up-left at ~109 deg, NOT at the HSV 0 deg (2026-10-01 fix from exv-view: markers were
  at HSV angles, the draw pass read the bins vertically flipped vs the scatter, `dot_rgb` now clamped
  against u32 sum wrap above ~16.8 Mpx).

## Verify
`cargo test -p egui-scopes` (headless naga validation of every scope's WGSL — wgpu
only validates at pipeline creation, so this catches shader typos without a GPU),
then `cargo clippy -p egui-scopes --all-targets -- -D warnings`; the demo's Scopes
page drives all five live.
