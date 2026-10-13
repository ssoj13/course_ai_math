# egui-keyframes

Reusable, **generic** keyframe / animation-curve panel widget for egui 0.34.
After Effects-style: a list of animatable properties, each with an inline value
editor (left column) and a keyframe track painted over a shared time ruler
(right column), plus a draggable playhead.

## What it is / why
A standalone, app-agnostic version of the playa "AE attributes + keyframes"
panel. The original was welded to `playa_engine` types (`Attrs`/`AttrValue`,
`Effect`/`EffectType`, `uuid::Uuid`, serde_json-encoded engine values) and used
`egui_extras::TableBuilder`. This crate keeps the *visual + interaction* design
but retargets it onto a flat, plain-scalar model so any egui app can reuse it.

## Model (caller passes a snapshot each frame; widget never owns it)
- `KeyframePanel { properties: Vec<Property>, time, start, end }`
  - `time` = playhead; `start`/`end` = visible window mapped onto the ruler.
- `Property { id: u64, name, kind: PropKind, value: [f32;4], range: Option<(f32,f32)>, speed, keys: Vec<Key> }`
  - `id` is a `u64` (not Uuid) — caller's stable identifier (e.g. hashed path).
  - `value` holds up to 4 scalar components; only `kind.arity()` are meaningful.
    For scalar kinds `value[0]` is the current value echoed onto the track.
- `Key { time: f64, value: f32 }` — plain `f32`, no app value types.
- `PropKind { Bool, Float, Vec2, Vec3, Vec4 }` — selects the inline editor only.

## Action enum (returned intents; caller applies them)
`KeyAction`:
- `AddKey { prop, time, value }`   — double-click empty track (value = current).
- `MoveKey { prop, index, time, value }` — drag a diamond horizontally.
- `DeleteKey { prop, index }`      — right-click a diamond.
- `SetValue { prop, component, value }` — inline value editor changed.
- `Seek { time }`                  — click/drag the ruler (playhead scrub).

## Entry point
`pub fn show(ui, panel: &KeyframePanel, state: &mut KeyframesState) -> Vec<KeyAction>`
`KeyframesState { name_column_width: f32 }` — only persistent state (splitter
position). Derives serde when the default `serde` feature is on.

## Deps
- `egui = "0.34"` (runtime). `serde` optional (default on) — only for `KeyframesState`/`PropKind`.
- NO uuid, NO serde_json, NO egui_extras, NO playa crates.

## Build
Own bare `[workspace]` table in Cargo.toml => builds standalone:
`cargo check` (or `--no-default-features` to drop serde). Edition 2024, rust 1.88.
`[lib] path = "src/egui_keyframes.rs"`; model in `src/model.rs`.

## Gotchas / design notes
- Hand-painted layout (not a table): the track column is one continuous painting
  surface so the ruler + playhead line up across all rows. Rows are laid out with
  explicit rects + `new_child` for the left column; `advance_cursor_after_rect`
  tells egui how much vertical space was consumed.
- egui 0.34 API: `Painter::rect_filled(rect, corner_radius, fill)`,
  `CornerRadius: From<f32>`/`CornerRadius::ZERO`; diamonds via
  `Shape::convex_polygon`. let-chains (`if x && let Some(p) = ...`) require 1.88.
- Time is an opaque `f64`; the widget linearly maps `[start,end] <-> track x`
  with a small inset so edge diamonds aren't clipped. Seconds vs frames is the
  caller's choice.
- Keyframe drag only changes **time** (the generic track has no value axis) —
  `MoveKey` preserves the original `value`. Keyframes are expected sorted by time;
  unsorted input still renders, indices are reported as-is.

## Dropped from the playa source (couldn't generalize cleanly)
- **Effects stack UI** (`EffectAction`, add/remove/reorder/toggle effects,
  per-effect attr tables): tied to `EffectType`/`Effect` schemas — out of scope
  for a generic keyframe panel.
- **Rich `AttrValue` variants**: `Str`/combobox, `Mat3`/`Mat4`, `Json`, `Uuid`,
  `List`/`Map`/`Set`, `Int8`/`UInt` — collapsed to numeric scalars + bool. The
  generic model keeps `f32` components; non-numeric/structured attrs are not
  representable here by design.
- **Schema-driven ordering & ui_options** (combobox option lists, slider
  min/max/step from a schema): replaced by per-`Property` `range`/`speed`. Caller
  pre-sorts `properties`.
- **Mixed-value (multi-selection) dimming** (`mixed_keys`, weak text): not part of
  the single-snapshot generic model.

## Gathered from
`playa/crates/playa-ui/src/widgets/ae/ae_ui.rs` (decoupled from playa-engine).
