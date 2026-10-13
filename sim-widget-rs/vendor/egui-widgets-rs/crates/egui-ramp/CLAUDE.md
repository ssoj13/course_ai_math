# CLAUDE.md — `egui-ramp`

> Working notebook for this crate. Keep it current: what it is, the public API,
> the interaction model, and the gotchas a fresh session needs.

## What this is
A hermetic, **generic** ramp editor crate for egui 0.36:

1. **Value-remap CURVE** (`curve_edit`) — a framed box over `CurveConfig::x_range`
   × `y_range` (both `[0,1]` by default) with draggable keys. The data is
   `curves::Track<T>` (re-exported; `T: curves::Real` = f32 or f64): keys with two
   tangent `Side`s each (`Tan`: Smooth, Linear, Constant, Flat, CatmullRom, User).
2. **Curve SET editor** (`curves_edit`) — the same plot for several tracks (an RGB
   curve, an OpenFX Parametric parameter, used by ofx-rs `ofx-host-egui`): channel
   selector (`segmented_idx` strip for <= `STRIP_MAX` = 4 tracks, dropdown beyond; a strip that does not fit is a dropdown),
   a help icon beside it carrying `CURVE_HINT` (the plot has no tooltip: it covered the keys), numeric x / y fields
   of the selected key, a Reset ICON button (caller defaults), and an `EditPhase` (Idle / Live / Done) so a caller with
   undo commits one value per gesture.
3. **Colour GRADIENT / ramp** (`gradient_edit`) — a horizontal bar painted from
   `Gradient::sample`, with draggable colour-stop markers underneath and a colour
   picker for the selected stop. `sample` = one `Track<f32>` per premultiplied
   linear channel (r, g, b, a) built from the stops with the chosen `Tan`.

No app types. Runtime deps: `egui`, `egui-widgets-config` (shared metrics, icons, palette, `AttrMetrics`, `segmented_idx`, `icon_button_tip`; no attr-grid dependency),
`curves` (ssh git, branch main; the `Track` model, KEYS.md in curves-rs/docs),
`egui-colorpicker`. Dev-dep: `egui_kittest` (gesture tests).

Single source file: `src/egui_ramp.rs` (set via `[lib] path`), tests at its end.

## Public API
- re-exports `Extrap, Key, Real, Side, SideId, Tan, Track` (`Key` = curves' `Key`)
- `curve_edit<T: Real>(ui, &mut Track<T>, &CurveConfig) -> Response` (`.changed()` on edit frames)
- `curves_edit<T: Real>(ui, id: Id, CurveSet { curves, labels, colors, defaults }, &CurveConfig,
  background: Option<Background>) -> CurveOutput { response, phase }`
- `CurveConfig { x_range, y_range (f64), width: Option, height, grid, pin_ends (ramp: true),
  min_knots (2), new_tan (Smooth), color (None = palette accent), handle_size, handle_shape, tangent_line,
  handle_color, metrics: AttrMetrics }`; `GradientConfig { interp, metrics, .. }`
- `EditPhase`, `CURVE_HINT`, `Background = &mut dyn FnMut(&Painter, Rect)` (drawn under the curves)
- `Gradient`, `GradientStop`, `gradient_edit`, `GradientConfig { height, marker_size, interp: Tan, metrics }`, `GRADIENT_KINDS`

## Curve gestures (one set for both curve editors; `CURVE_HINT` lists them)
- hover a key: bigger + grab cursor; drag it: `move_key` between its neighbours (gap 1e-4
  of the domain) + `set_value`; `pin_ends` keeps the ends at the domain bounds;
  **Ctrl**-drag: a tenth of the motion; **Shift**-drag: one axis (whichever moved more);
- click a key: select (bright fill, selection-coloured ring; its tangent handles appear); **Shift**-click: toggle
  the segment after the key (before it, for the last key) Linear <-> `new_tan` with
  `set_seg` (the key is drawn as a square while that segment is linear);
- drag a handle (selected key only): `set_handle(i, side, (dt, dv))`; the other side follows
  while the key is unbroken; **Shift**-drag does `set_broken(i, true)` first;
- a press takes the NEARER of a key and a handle (tie: key), else a handle lying over its key
  would make the key ungrabbable;
- **double-click** off the keys: `add(t, eval(t), new_tan)` ON the curve, selected;
- **right-click** a key, or **Delete** / Backspace with the plot focused: delete
  (never below `min_knots`).
No multi-select (the track's `shift(&[usize], dt)` is there if a host wants it).

## Where transient state lives
egui memory: the plot's `PlotState { curve, selected, drag }` keyed by the plot
response id; `curves_edit`'s active channel and numeric-drag flag keyed by its `id`;
the gradient's selected stop by `ui.id().with("egui_ramp_gradient_sel")`.

## Gotchas / decisions
- The track validates every edit (finite, strictly increasing times, ...) and stays
  unchanged on refusal: the editor treats a refused move/add/remove as no edit
  (`.ok()` / `is_ok()` with a comment), it never half-applies.
- Keys stay SORTED and strictly increasing; the editor clamps a dragged time between
  its neighbours so the key index never changes mid-drag.
- A key drag is latched on the key under `pointer.press_origin()` (egui reports drag
  start only after its threshold) and moves by accumulated `drag_delta`, so
  Ctrl / Shift can change mid-drag.
- Plot accessibility label "curve plot" (kittest finds the rect by it).
- Gradient: stops sorted; marker drag may pass neighbours, re-sorted after. Equal stop
  positions are nudged apart by `TIE` (1e-6) when the channel tracks are built, which
  keeps the old "two stops at one position = hard edge" behaviour.
- Colour blend is LINEAR-space (premultiplied linear channels). The bar builds the four
  tracks once per frame (`Gradient::ramp`), not per slice. Catmull-Rom may overshoot: each
  channel is clamped to 0..=1 in `ramp_at`.
- The curve polyline is sampled (<= 256) so curved segments show; holds draw as steep lines.
- Changelog 2026-10-06: moved from `Curve/Knot/CurveKind` to `Track/Key/Tan` (curves 169ded7);
  handles only for the selected key; Shift-click toggles a segment; see CHANGELOG.md.

- Markers: curve keys / handles map into the plot frame inset by `marker_pad` (`Plot::inner`), gradient stops into the
  bar inset by the marker half-width (`marker_rect`), so end markers are whole and clickable inside the allocated rect;
  the painter clip stays the plain rect. The `Background` callback gets `Plot::inner`.
- Tests: harnesses use `with_max_steps(8)` + `run()`; `run_steps(n)` only to let time pass (double-click window).
  `WIDGET_SHOTS=dir cargo test -p egui-ramp -- --ignored shots` writes ramp-style-*.png.

## TODO / ideas
- [ ] Optional `serde` feature for `Gradient` if persistence is wanted.
- [ ] Multi-select + `Track::shift` (dope-sheet style) if a host needs it.
- [ ] ofx-host-egui `curves.rs` + `ofx-host::parametric_curve` migration to `Track` (consumer repo).
