# CLAUDE.md — `crates/egui-slider`

> Working notebook for this subdir. Keep current: layout, gotchas, TODOs.
> Author: Alex Joss. Commit on `main`, NO AI co-author trailer.

## What / why
A **slim, custom-painted** horizontal slider: a thin rail + a **SHORT flat handle** +
an optional trailing fill + an optional numeric `DragValue` box. Lib root
`src/egui_slider.rs`. Generic over `egui::emath::Numeric`, no app types.

Born from a review-rs ask: `egui::Slider`'s handle looks tall/full-height in dense
toolbars and can't be made shorter.

## Key decision — paint it, don't wrap egui::Slider
First tried wrapping `egui::Slider` (handle_shape=Rect, trailing_fill, thin rail via
`spacing.slider_rail_height`). **Dead end for the handle height:** `Slider::add_contents`
sets its thickness to `max(text_style_height(Body), interact_size.y)` and sizes the
handle to ~0.8x that — so the handle can NEVER be shorter than the body font, no matter
the spacing. To get a genuinely short handle we paint the rail + handle ourselves and run
the drag math (`value_to_t` / `t_to_value`, log-aware). Interaction is a plain
`Sense::click_and_drag` on the track (click-to-jump + drag). The value box is a real
`egui::DragValue` editing the same value; the returned `Response` is the union.

## API
```rust
ui.add(SlimSlider::new(&mut v, 2.0..=2000.0).logarithmic(true).suffix(" px"));
ui.add(SlimSlider::new(&mut v, 24.0..=240.0).handle_height(6.0));  // short handle
egui_slider::slim_slider(ui, &mut v, 0.0..=1.0);                    // terse free fn
```
Builder knobs: `rail_height`, **`handle_height`** (the key one), `handle_width`,
`handle_corner_radius` (0 = sharp, big = pill), `row_height` (hit-strip / value-box
height), `trailing_fill`, `value_field`, `field_width`, `width` (track px),
`logarithmic`, `step_by`, `suffix`, `accent`, plus (2026-10, for attribute rows):
**`fill(true)`** (rail = available width minus value box, box flush with the cell's right
edge: every row ends at one edge), **`default_value(v)`** (Ctrl/Cmd-click on the rail resets),
**`integer()`**, **`max_decimals(n)`**, **`clamping(..)`** (default `Edits`, see Gotchas), keyboard arrows + AccessKit Increment/Decrement/SetValue (a focus-lock filter stops egui moving focus to the value box on an arrow press), and **`labelled_by(id)`** (names the slider by a row label for AccessKit). The widget also sets
`widget_info(WidgetInfo::slider)` + min/max/step, so kittest finds it by role and label like
`egui::Slider`. The demo page is a live playground whose config controls are themselves
SlimSliders (dogfooding).
Defaults: rail 4, handle 10×7 px, trailing fill on, value box on. Row height and box width
default to the STYLE (`interact_size.y` / `.x`), not constants, so a host compacting its
controls (`AttrMetrics::apply`) compacts sliders too; `MIN_TRACK` (24 px) bounds a fill rail.

## Gotchas
- **One snap + clamp rule** (`Snap`): the rail, the arrow keys and the value box all write through it
  (egui `Slider::set_value`'s: clamp, step counted from the range start, integers, clamp). The box is
  a `DragValue::from_get_set` whose setter applies it, so a typed 30.4 on step 1 is 30 and the text
  being edited stays in step with the value. **Showing never writes the value; only edits are
  clamped.** Clamping modes are egui's `SliderClamping`, but the DEFAULT differs from egui's:
  `Edits` (default) clamps edits (rail drag/click, typed value, arrows, AccessKit set) and draws a
  STORED out-of-range value as it is, with no `changed()`: a host's scene/document data survives
  being displayed. `Always` is an explicit opt-in that also clamps a stored value on every show
  (and marks the response changed): only for widgets that own the value. `Never` is soft (the box
  accepts anything). Do not add per-widget workarounds; opt in to `Always` with a why-comment.
- The value box width is the style's `interact_size.x` grown to the widest text it can show (range
  ends, current value, suffix); `field_width` fixes it.
- Colours track the egui theme: rail = `visuals.widgets.inactive.bg_fill`, handle =
  `style.interact(&resp)` visuals (so it brightens on hover/drag), trailing fill =
  `accent` or `visuals.selection.bg_fill`.
- Log scale needs a strictly positive range (`lo > 0`); otherwise it falls back to linear.
- Integer `Num` (`Numeric::INTEGRAL`) rounds on set; `step_by` quantises.
- Registered in `egui-widgets-demo` (Page::Slider), Editors group.

## TODO
- [ ] Optional tick marks / discrete-notch look.
- [ ] Vertical orientation (currently horizontal-only).
- [ ] Adopt in review-rs / otio-timeline toolbars (Zoom / Track H) once consumed.

## Verify
`cargo clippy -p egui-slider --all-targets -- -D warnings` and the demo's Slider page.
