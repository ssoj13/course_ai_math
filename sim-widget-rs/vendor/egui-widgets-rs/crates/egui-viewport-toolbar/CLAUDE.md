# CLAUDE.md — `crates/egui-viewport-toolbar`

Conventions: repo-root `CLAUDE.md`.

## What
A collapsible chrome strip on a caller-supplied viewport `Rect`. It defaults to the bottom edge;
callers can choose the top edge. Click the toggle to roll it up or out, or drag the same toggle
vertically to place it within the viewport.
It rolls up to the LEFT into a small square toggle button and expands back,
animated. Generic — no app types: the HOST draws the controls inside through a
closure (rv-rs puts its A/B-compare + RGBA-channel controls there; another app
puts something else entirely).

## Why it lives here (and why it is not one of the other viewport crates)
Checked before writing, nothing was reusable: `egui-viewport-hud` is a stateless
painter for corner stat plates, `egui-viewport-overlays` draws 3-D axis/grid/
reticle chrome, and `egui-viewport` is a whole 2-D viewport with its own pan/zoom
(rv-rs has its own GPU present path and does not use it). None of them own a
control strip or a collapse animation.

## API
- `ToolbarState { expanded: bool, vertical_offset: f32 }` (serde, `#[serde(default)]`)
  is the persisted state. The offset is in points inward from the chosen edge and is clamped
  to the viewport when drawn; old configs without it load at zero. The host owns this state
  and passes it back each frame. Defaults to expanded at the edge.
- `ViewportToolbar { height, margin, button_width, id_salt, expand_tooltip,
  collapse_tooltip }` — layout constants, `Copy`, cheap to build each frame.
- `.show(ui, viewport, &mut state, |ui| { ...controls... }) -> ToolbarResponse`
- `ToolbarResponse { rect, contains_pointer, toggled, moved, layer }`. Save the host state after
  either `toggled` or `moved`. A drag release does not toggle. `layer` is the strip Area's
  `LayerId`: a host with per-panel input routing registers the strip with it
  (`egui_hotkeys::register_at`), since the pointer over the strip hits that layer, not the host's.
- `backdrop_picker(ui, &Backdrop) -> Option<Backdrop>` (`src/backdrop.rs`): the viewer
  backdrop control, a mini-swatch button + drop-up menu. Model and rendering live in
  `egui-display` (`Backdrop`, `show_backdrop` + `PresentPass::backdrop`, `paint_backdrop`
  for eframe hosts); semantics and the host wiring are in `README.md`. Tests:
  `tests/backdrop.rs` (kittest).

## Claimed area (the contract — tests pin it)
Exactly what it paints, never more:

| state     | claimed                                                      |
|-----------|--------------------------------------------------------------|
| expanded  | viewport width inset by `margin`, at the chosen vertical position |
| collapsed | only the `button_width` x `height` toggle at that position    |
| animating | the strip at its current eased width (LEFT edge fixed)        |
| no room   | `Rect::NOTHING` — draws nothing, claims nothing               |

## Gotchas
- **INPUT LEAKAGE IS THE WHOLE POINT OF THIS CRATE.** It floats over a viewport
  that binds raw pointer gestures (rv-rs: LMB drag = scrub, MMB = pan, RMB =
  drag-zoom, wheel = zoom, Ctrl+LMB = compare wipe). Two halves, and only one is
  automatic:
  - Clicks/drags: handled here. The strip lives in its own `egui::Area` at
    `Order::Middle` and allocates its whole rect with `Sense::click_and_drag()`
    BEFORE drawing into it, so egui's hit-test resolves presses (including on the
    strip's empty background) to the toolbar, not to the host's viewport widget.
  - Wheel / keys: **the host must gate on `contains_pointer`.** Scroll is not
    routed by hit-testing — a host reading `i.smooth_scroll_delta` sees a wheel
    turn wherever the pointer is. This is not fixable from inside the widget.
- The background claim is load-bearing and mutation-checked: deleting that one
  `allocate_rect` line reddens `pointer_over_the_strip_does_not_reach_the_host_viewport`
  with "toolbar leaked a pointer through to the host viewport". Do not "simplify"
  it away.
- The toggle button uses a CHILD id (`id.with("toggle")`), never the Area's own —
  re-using an Area id inside itself is egui's "Double use of widget id" (the bug
  `egui-toast` already hit).
- Overflow SCROLLS horizontally, it does not wrap: this is chrome over a picture
  the host already sized, so growing vertically would eat the image and clipping
  would strand controls out of reach. Budget a few points of `height` for the
  scrollbar if your content can overflow.
- Caret glyphs come from `egui-widgets-config::icons` (phosphor). The host must
  register the phosphor font once (`egui_phosphor::add_to_fonts`), as everywhere
  else in this repo.
- The animated width is clamped to `[button_width, expanded_w]`: the eased value
  interpolates from wherever it was, so a viewport resize mid-roll could otherwise
  leave the strip briefly wider than the viewport it sits in.
- Tests are headless via `Context::run_ui` (no GPU). Its root `Ui` is on
  `LayerId::background()` — the same layer a host CentralPanel viewport would be,
  which is exactly the layer the Area has to beat, so the leakage test is real.
  Hover needs the previous frame's widget rects, hence `Host::settle(n, ..)`.
- **Backdrop menu open state is NOT egui's popup memory.** egui keeps ONE open memory popup
  per viewport, so the colour picker (`Popup::menu` from the Custom swatch) opening inside
  the menu closed the menu. The menu is `Popup::open_bool` over `MenuOpen` in temp data
  (with the pass it was last drawn on, so an unshown control comes back closed).
- **Its close-on-outside is hand-rolled** (`IgnoreClicks` + a click on a layer below
  `Order::Foreground`): egui's `CloseOnClickOutside` would treat a click in the picker's
  separate area as outside. Mutation-checked by
  `only_a_click_outside_the_menu_and_its_colour_picker_closes_it`.
- Never call `ui.ctx()` lock-taking methods (`layer_id_at`) inside `ui.input(|i| ..)`:
  egui's context lock is not re-entrant; it deadlocks (epaint's 10 s RwLock panic in tests).
- egui_kittest needs `.with_step_dt(1/60)` here: the default quarter-second step turns the
  colour picker's 0.5 s idle close into two frames.
