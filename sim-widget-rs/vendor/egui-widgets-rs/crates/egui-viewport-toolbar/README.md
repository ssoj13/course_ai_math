# egui-viewport-toolbar

A collapsible chrome strip for the bottom (or top) edge of a viewport, plus the viewer
**backdrop picker** that usually lives in it. The strip's contract (claimed area, input
leakage) is documented in the crate rustdoc and `CLAUDE.md`; this README covers the
backdrop.

## Viewer backdrop

What shows under the image's alpha: a checkerboard, black, 50% grey or a custom colour.

| Piece | Crate | What |
|-------|-------|------|
| `Backdrop` (+ `BackdropKind`, `Checker`, `CheckerScale`) | `egui-display` | The persisted setting (serde, `Default` = checker), `cycle()` for a hotkey, `label()` / `icon()` |
| `show_backdrop(painter, image_rect, &backdrop, &view)` | `egui-display` | Viewport side: clears the image rect of the canvas to transparent and publishes the frame's `BackdropUniform` |
| `take_backdrop(&ctx)` -> `PresentPass::backdrop` | `egui-display` | Host side: the present pass composites the canvas OVER the backdrop |
| `paint_backdrop(painter, image_rect, &backdrop, &view)` | `egui-display` | SDR-only painter path for hosts without a `PresentPass` (plain eframe, the demo) |
| `backdrop_picker(ui, &backdrop) -> Option<Backdrop>` | `egui-viewport-toolbar` | The control: a mini-swatch button opening a drop-up menu |

### Semantics (every host keeps them)

- **Composited in the present pass, in linear light.** The canvas holds the image
  premultiplied (extended sRGB, 1.0 = SDR white). Where its alpha is below 1 the present
  shader decodes it, goes OVER the backdrop in linear display light relative to SDR
  reference white, re-encodes, and then applies the output's own encoding (sRGB, PQ in
  absolute nits after BT.709 -> BT.2020, HLG, scRGB at 80 nits per 1.0). Right on every
  `Output`; on HDR the backdrop sits at SDR-white-relative levels, not at the peak.
- **Colours** are sRGB swatches relative to SDR white (what the colour picker shows). The
  default checker is 0.18 / 0.25 of reference white in linear light (codes 0.461 /
  0.537); "50% Gray" is code 0.5 (0.214 linear).
- **Only under the image**, never in data: no cache, export, scope or OCIO sees it. A
  screenshot "as displayed" contains it, because that is what was displayed.
- **Checker scale.** Default `CheckerScale::Screen`: a cell is `size_px` (default 16)
  physical pixels at any zoom, pinned to the viewport's corner, so it never crawls or
  shimmers while the image pans or zooms. `CheckerScale::Image` (the menu's "Zoom with
  image") makes a cell `size_px` image pixels, pinned to the image corner.
- **One backdrop per frame** (one published uniform; the last `show_backdrop` wins).
- **Overlays are fine.** Anything egui draws over the hole after the image (viewport chrome,
  OFX interact overlays, HUD text) keeps canvas alpha as "over" coverage, so the backdrop goes
  under overlay + image exactly once. Outside the cleared rect the canvas is presented as it
  is: the composite is clipped to the rect `show_backdrop` published.

### Wiring a PresentPass host

```rust
// Viewport, in paint order BEFORE the image (paint callback or textured mesh):
let view = egui_display::BackdropView::new(viewport_rect.min, ui.ctx().pixels_per_point())
    .with_image(image_rect.min, zoom); // zoom = screen points per image pixel
egui_display::show_backdrop(&painter, image_rect, &settings.backdrop, &view);
// ... draw the image ...

// Toolbar (any Ui; inside ViewportToolbar::show's closure):
if let Some(b) = egui_viewport_toolbar::backdrop_picker(ui, &settings.backdrop) {
    settings.backdrop = b; // persist the settings now
}

// Host, after the UI ran and before PresentPass::draw (and before a capture):
present.backdrop = egui_display::take_backdrop(&ctx);
```

Requirements: the image must blend over what is beneath it (straight-alpha
`ALPHA_BLENDING`, or egui's premultiplied blend), and the egui-wgpu renderer must target
`CANVAS_FORMAT` without MSAA (what a `PresentPass` host does). Nothing opaque may be
painted between `show_backdrop` and the image inside the image rect.

The menu opens ABOVE the button (a bottom toolbar) and flips below by itself when there
is no room. Black and 50% Gray close it; Checker (cell size slider, "Zoom with image")
and Custom (inline swatch opening the `egui-colorpicker` picker) keep it open. A click
outside the menu and its colour picker, or Esc, closes it. Hotkey:
`settings.backdrop.cycle()` steps Checker -> Black -> 50% Gray -> Custom and keeps every
parameter.

## Integration notes

### Playa

Wired (see Playa's git log): `ViewportSettings.backdrop` replaces the old matte, the
viewport calls `show_backdrop`, the toolbar carries `backdrop_picker`,
`viewport.cycle_backdrop` is the hotkey and `display_host` sets `PresentPass::backdrop`.

### exr-view

exr-view presents through `egui_display::PresentPass` too (`app.rs`, `present.rs`), so it
wires the same way, with no shader change:

1. **Persistence.** `#[serde(default)] pub backdrop: egui_display::Backdrop` in
   `persist::Config`, carried into the viewer controls (`controls.backdrop`).
2. **Viewport** (`viewport.rs`, the "Background + checker" block). Keep the gray-28
   viewport fill, delete `draw_checker`, and after `img_rect` is computed (or the
   display-window `canvas` rect, if the backdrop should cover the whole display window),
   before the tiles' paint callback:
   ```rust
   let view = egui_display::BackdropView::new(rect.min, ui.ctx().pixels_per_point())
       .with_image(img_rect.min, img.zoom);
   egui_display::show_backdrop(&painter, img_rect, &controls.backdrop, &view);
   ```
   The tiles' premultiplied blend (`tiles/render.rs`) leaves their alpha in the canvas;
   tiles still streaming show the backdrop.
3. **Host** (`app.rs`, where `PresentPass::draw` is recorded): set
   `present.backdrop = egui_display::take_backdrop(ctx)` after the UI pass.
4. **Toolbar.** exr-view's toolbar is its own top panel (`App::ui_toolbar`): add the
   `egui-viewport-toolbar` dependency and call `backdrop_picker` there; the menu opens
   downward below the top panel.
