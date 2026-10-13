# egui-toast — local notebook

## What it is
Transient, auto-expiring **toast** notifications: top-right anchored floating
stack of rounded, accent-tinted boxes that appear, linger a few seconds, fade,
and remove themselves. The *ephemeral* complement to `egui-logbuffer` (which is a
*persistent* browsable panel). Hermetic, egui-only, no app types.

## Public API
- `enum ToastLevel { Info, Success, Warn, Error }` — `.accent() -> Color32`.
- `struct Toast { pub text: String, pub level: ToastLevel, /* created_at, ttl */ }`
  - `Toast::new(text: impl Into<String>, level) -> Self` (default ttl ~4 s)
  - `.ttl(secs: f64) -> Self` (builder)
- `struct ToastStack { /* toasts: Vec<Toast> */ }` — `Default` = empty.
  - `ToastStack::new()`
  - `push(&mut self, text: impl Into<String>, level)`
  - `push_toast(&mut self, Toast)`
  - `is_empty(&self) -> bool`
  - `show(&mut self, ctx: &egui::Context)` — call ONCE per frame.

The stack is **caller-owned state** (keep one in your app struct). `show` is the
mutator: stamp → paint → fade → dismiss/expire → request_repaint.

## Timing model (the "why" — important)
- A toast's lifetime starts on the **first frame it is displayed**, not at
  `push`. `created_at` is `None` until `show` stamps it from
  `ctx.input(|i| i.time)`. Rationale: toasts queued while the window was
  unfocused / not repainting must not silently expire before the user ever sees
  them.
- We use the **egui frame clock** (`ctx.input(|i| i.time)`), never `std::time` —
  consistent with the rest of this widget set and correct under egui's time
  source.
- Fade: full opacity until the last `FADE` (0.5 s), then linear to 0. Alpha
  scales the box bg, border, and text colours.
- While any toast is live, `show` calls `ctx.request_repaint()` so expiry + fade
  animate even when nothing else drives the UI.

## Layout
- Each toast is its own `egui::Area` on `Order::Foreground`, anchored
  `Align2::RIGHT_TOP` with a growing `y_offset` (we add each toast's measured
  `rect.height() + gap`) so they stack downward from the top-right corner.
- Box = `Frame::default()` with `.fill` (dark), `.stroke` (level accent),
  `.corner_radius(6.0)`, `.inner_margin(Margin::symmetric(10, 8))`, max width 280.
- Click anywhere on a toast dismisses it (`resp.interact(Sense::click())`).

## Gotchas
- `show` must run once per frame; calling it 0 times freezes the lifecycle
  (nothing stamps/expires), calling it twice double-stamps the y-offset layout.
- Area `Id`s are positional (`("egui_toast", idx)`); fine because the stack is
  re-laid-out wholesale each frame. Don't try to persist per-toast Area state.
- Removal (click + expiry) is one `retain` pass over a `keep[]` flag vector built
  during paint — keep that in sync if you add more per-toast interactions.
- `Margin::symmetric` takes `i8` in egui 0.34 (literals infer fine).
```
```
