# egui-colorpicker — Maya/Nuke-style HDR colour picker

Local working notes for this crate. Keep current — append findings, prune stale.
Author convention: Alex Joss <joss13@gmail.com>.

## What / why

A self-contained HDR colour picker popup + Nuke-style inline RGBA editors for
egui. Used by `egui-attr-grid` (the `color` hint of a Vec3 / Vec4 row). Stores colour **linearly** as `[f32;4]`; HDR values (any
channel > 1.0) flow through unchanged. (Gathered from gitnexus-rs — the richer of
two variants found across the cglibs projects.)

**Dependency direction (important):** `egui-attr-grid` depends on
`egui-colorpicker`, NOT the other way round. This crate may depend ONLY on `egui`
(+ `serde`). Never add an `egui-attr-grid` dep here — it would be a cycle. That is
why the in-popup LDR|HDR segmented toggle is rebuilt inline with two
`selectable_label`s instead of reusing `egui_attr_grid::segmented`.

## Public API (`src/lib.rs`)

- `color_button` / `color_button_with(ui, &mut [f32;4], &mut PickerConfig)` — the
  round swatch chip that opens the picker popup. `color_button_with` reports
  `changed()` itself (it snapshots before/after, since the swatch's own click
  Sense never reports popup edits).
- `color_edit_rgba` / `color_edit_rgba_ex(ui, &mut [f32;4], swatch_left, decimals)`
  — inline `R G B A` DragValues + swatch; `decimals` is DISPLAY-only rounding.
- `color_edit_rgb(ui, &mut [f32;3])` — RGB variant (alpha pinned 1.0).
- `PickerConfig { hdr_max, alpha_enabled, display_transform, eyedropper, .. }`.
  `display_transform` defaults to sRGB OETF — pass your tonemap to keep the
  "display" preview honest.
- `color::{linear_to_srgb, srgb_to_linear, rgb_to_hsv, hsv_to_rgb, hex_*}`.

## LDR | HDR toggle (popup top)

A persistent global mode (egui memory id `egui_colorpicker_hdr_full`), rendered as
an explicit two-segment `LDR | HDR` strip at the top of `picker_popup_contents`:
- **LDR** → `cfg.hdr_max = 1.0` (every channel capped at 1.0; classic display colour).
- **HDR** → `cfg.hdr_max = HDR_FULL_MAX (16.0)` (emissive / bloom colours > 1.0).
It OVERRIDES the caller's `cfg.hdr_max` for the popup. The sRGB column always
clamps `[0,1]` regardless (sRGB encoding is undefined above 1.0). One choice,
shared by every picker, persisted across restart.

## Gotchas

- Popup close paths: 0.5 s idle-no-hover timer (with a bottom countdown bar), Esc,
  or re-click the swatch. A transparent `Order::Middle` backstop rect blocks
  click-through to dock content beneath the popup.
- Colours are LINEAR end-to-end; only the sRGB column + hex are display-encoded.

## Changelog

- 2026-10-02 — the popup no longer closes itself while open: the idle timer tested
  `response.hovered() || popup.contains_pointer()`, and egui reports both false when another
  area (the popup over its own swatch near a screen edge, the click-through backstop) is on
  top at the pointer, so the picker closed 0.5 s after opening under a resting pointer. It
  now tests the pointer position against the swatch and popup rects. Kittest:
  `tests/popup.rs` (red before the fix).

- 2026-07-12 — fixed the "white corners" glitch on ROUNDED swatches (the popup's working/display
  preview rects + the palette swatches). Root cause: `paint_checker` filled its light-gray base as
  a SQUARE (rounding 0) while the colour rect on top was rounded, so the base spilled past the
  rounded corners. `paint_checker` now takes a `rounding` param — rounds the light base to match
  and clips dark cells to the rounded footprint. Same class of fix the round swatch already had
  (inscribe the checker); the rounded-RECT swatches were missed. Callers: previews `4.0`, palette
  `2.0`, circle `0.0`.

- 2026-06-19 — exported `pub const EDITOR_MARGIN: f32 = 12.0` (the shared
  inline-editor right inset). `egui-attr-table` now `use`s it instead of
  hand-copying the literal, so the colour swatch and the table's slider value
  boxes can never silently drift apart. Const lives HERE (leaf) to respect the
  one-way `egui-attr-table` → `egui-colorpicker` dep.

- 2026-06-04 — LDR|HDR checkbox → explicit segmented `LDR | HDR` toggle (the cap
  control existed but was an easy-to-miss `"HDR (full)"` checkbox).
