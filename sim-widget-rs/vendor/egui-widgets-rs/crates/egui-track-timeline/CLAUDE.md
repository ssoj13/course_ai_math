# CLAUDE.md — `crates/egui-track-timeline`

Conventions: repo-root `CLAUDE.md`. egui 0.34, edition 2024, MSRV 1.88
(let-chains are used).

## What
A **generic, multi-track NLE-style timeline widget** for egui. Stacked tracks of
clips drawn as bars over a horizontal frame axis, with a draggable playhead,
adaptive frame grid, zoom + horizontal pan, alternating row backgrounds, an
optional dimmed work-area overlay, and the four classic edit gestures:
**move**, **trim-in**, **trim-out**, **slide**.

Carries **no application types**. The caller builds a flat model each frame and
applies returned action intents to its own data. Used by any egui app that needs
a clip/track timeline (video editor, animation/NLE, sequencer).

## Gathered from
`playa/crates/playa-ui/src/widgets/timeline/{timeline.rs, timeline_ui.rs,
timeline_helpers.rs}` and the timeline variants of
`playa-ui/.../widgets/dnd.rs::GlobalDragState`. Decoupled from
`playa_engine` (`Comp`/`Layer`/`Project`/`Attrs`), `playa_time`, `uuid`, and the
event-bus dispatch closure.

## Model (input — caller rebuilds each frame; widget never owns it)
- `TimelineModel { tracks: Vec<Track>, playhead: i64, fps: f32, selection: Vec<ClipId>, work_area: Option<WorkArea> }`
- `Track { name: String, clips: Vec<Clip> }` — index 0 = topmost row.
- `Clip { id: ClipId (=u64), start, duration, trim_in, trim_out, label, color: Option<Color32> }`
  - Full bar = `start .. start+duration`. Visible (played) bar = full minus
    `trim_in` (head) / `trim_out` (tail). No explicit color → stable hash of `label`.
- `WorkArea { start, end }` — frames outside `[start,end)` are dimmed.

## Keyframe lanes (keys at any time)
- `Track { expanded, lanes: Vec<PropLane> }`; `PropLane { name, keys: Vec<Keyframe>, color }`.
- `Keyframe { frame: f64, value: f64, selected }` - a key's TIME in frames, a whole frame or
  between two. Diamonds are drawn and hit-tested at the exact x (`hit_key` returns the key's own
  `frame`, never one recomputed from the pointer).
- `KeyPos { track, lane, frame: f64 }` - positional address; `frame` is copied from
  `Keyframe::frame`, `Eq`/`Hash` compare its bits. Exported at the crate root.
- Actions: `AddKey { track, lane, frame: f64 }`, `SelectKeys`, `ClearKeySelection`,
  `MoveKeys { keys, delta: f64 }`, `DeleteKeys`. `TimelineAction` is `PartialEq`, not `Eq`.
- `TimelineConfig::snap_keys_to_frames` (default `true`): AddKey lands on the nearest whole frame
  and a drag's delta is whole frames, so a key between frames keeps its fraction; `false` makes
  both exact. Pure helpers `key_time` / `key_delta` (tested).

## View state (widget-owned; mutated in place; serde-persistable)
- `TimelineView { zoom, pan_offset, drag: Option<DragState> }` (`drag` is `#[serde(skip)]`).
- `TimelineConfig { row_height, pixels_per_frame, edge_threshold, show_work_area }` (static layout).
- `DragState` = `Pan | Move | TrimStart | TrimEnd | Slide` — in-widget gesture FSM,
  keyed by `(track_idx, clip_idx)` (was layer-idx/UUID in playa).

## Actions (output — caller applies to its own model)
`TimelineAction`:
- `Seek { frame }` — ruler scrub or empty-space click.
- `Select { id, additive (Ctrl/Cmd), range (Shift) }` / `ClearSelection`.
- `MoveClip { id, new_start, new_track }`.
- `TrimStart { id, delta }` / `TrimEnd { id, delta }` (delta>0 trims more material off that edge).
- `Slide { id, delta }` — caller applies `start+=delta, trim_in-=delta, trim_out+=delta` (clamp ≥0).

Pan/zoom are **not** actions — they mutate `TimelineView` directly.

## Entry point
`TrackTimeline::new(config).show(ui, &mut view, &model) -> Vec<TimelineAction>`.

## Files
- `src/egui_track_timeline.rs` — lib root: crate docs + re-exports.
- `src/model.rs` — `TimelineModel`/`Track`/`Clip`/`WorkArea`/`ClipId`.
- `src/actions.rs` — `TimelineAction`.
- `src/view.rs` — `TimelineConfig`/`TimelineView`/`DragState`, coord math, zoom clamp.
- `src/paint.rs` — `ClipGeom`, `Tool`, tool detection, ruler/clip/playhead/work-area painters.
- `src/widget.rs` — `TrackTimeline::show` (input + drag FSM + paint glue).

## Gotchas / decisions
- **Standalone build**: bare `[workspace]` at top of `Cargo.toml` makes this its
  own workspace root (own `Cargo.lock`), so the parent workspace member list is
  untouched. `cargo check` from this dir.
- **egui 0.34.3 API**: there is **no** `InputState::raw_scroll_delta` (newer than
  some docs); use `smooth_scroll_delta` (Vec2). `rect_stroke` needs
  `epaint::StrokeKind::Middle`. `Shape::convex_polygon` takes a `Stroke`
  (`Stroke::NONE`), not a `(f32, Color32)` tuple.
- **No name/outline column**: this widget paints only the bar canvas + ruler.
  A track-name outline panel (with per-layer visibility/opacity/blend/mask
  controls in playa) is the host's responsibility — kept out to stay generic.
- **`fps`** is a model field for the caller's own labeling; the widget currently
  works purely in frame space (ruler shows frame numbers).
- **Ruler labels are typography-driven**: `draw_ruler` measures the widest visible frame
  number in the configured monospace font, adds the shared large spacing token, and rounds
  the required frame interval UP on the shared 1/2/5 ladder. Minor ticks use a separate
  grid so valid combinations such as tick 200 / label 500 remain representable.

## Dropped during decoupling (playa-specific, not generalizable cleanly)
- Frame-cache **status strip** + `FrameStatus` colors (engine cache concept).
- **Bookmarks** on the ruler (Shift+digit set / digit jump / Ctrl-click clear).
- **Diagonal hatch texture** for "file comp" bars (engine node-type concept).
- **Project→Timeline external drag-drop** (`GlobalDragState::ProjectItem`,
  `ProjectDragSnapOverlay`, cycle detection) — cross-panel DnD belongs to the host.
- **Double-click "dive into source comp"**, solo, blend modes, opacity sliders,
  track-matte/mask refs, work-area lock, snap-to-frame, layout rename dialog,
  transport toolbar — all host/engine concerns.
- **Speed / rational-time** trim math (`playa_time::Speed`) — slide uses a plain
  1:1 frame delta; the caller applies speed scaling if it has any.
- Multi-clip group ghost preview when dragging a multi-selection — current ghost
  previews only the grabbed clip; the host can extend if needed.

## TODO / future
- [ ] Optional hover highlight (plumb `hovered` through `show`; `draw_clip`
      already supports it).
- [ ] Multi-selection group-move ghosts.
- [x] Clip edge snapping (`snap_threshold`, points) and key whole-frame snapping
      (`snap_keys_to_frames`).
