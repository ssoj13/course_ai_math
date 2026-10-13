# egui-spinner — local notebook

## What it is
A tiny **indeterminate** "busy" throbber: a rotating arc painted into an
exact-size rect. The indeterminate counterpart to the determinate
`egui-progressbar`. Hermetic, egui-only, no app types.

Public API (free functions, immediate-mode):
- `spinner(ui: &mut egui::Ui) -> egui::Response`
  — default size (`ui.spacing().interact_size.y`), default colour
  (`ui.visuals().weak_text_color()`).
- `spinner_sized(ui, diameter: f32, color: Option<egui::Color32>) -> egui::Response`
  — explicit diameter; `color = None` ⇒ weak-text colour.

## Decision: wrap vs reimplement `egui::Spinner`
egui 0.34 **HAS** a built-in `egui::Spinner`. This crate is a deliberately thin
**reimplementation**, NOT a wrapper, chosen for:
- **Consistent themed defaults** with the rest of this widget set: it sizes off
  `interact_size.y` and colours off `weak_text_color()`, so a spinner placed next
  to the library's other widgets matches without per-call styling.
- **Terse free-function API** (`spinner(ui)`) mirroring the library's style,
  vs egui's builder (`egui::Spinner::new()`).

Honest note: semantically egui's built-in fully suffices — `ui.add(egui::Spinner::new())`
gives the stock look. If we later decide the convenience is not worth a crate,
this is a 2-line wrapper over `egui::Spinner` and could collapse to one. Kept as a
self-painted arc so the *look* (3/4 arc, ~1.4 s/turn) is ours and stable across
egui's own restyling.

## How it animates (the why)
- Angle advances with the egui time source: `ui.input(|i| i.time)` (NOT
  `std::time`) so it stays in egui's frame clock.
- We call `ui.ctx().request_repaint()` every frame the spinner is visible —
  otherwise the arc only moves when some *other* event repaints, and it
  stutters/freezes.
- `allocate_exact_size` reserves a square rect; we early-out when
  `!is_rect_visible` (scrolled offscreen) to skip both the repaint request and
  the paint.

## Gotchas
- The arc is a `Shape::line` polyline (32 segments). Fine for throbber sizes;
  bump segments only if used very large.
- Stroke width scales with diameter (`0.12 * d`, min 1.0) so it stays
  proportionate from tiny status-row spinners up.
```
```
