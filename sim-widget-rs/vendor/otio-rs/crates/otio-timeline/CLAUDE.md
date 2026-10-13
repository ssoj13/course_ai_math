# CLAUDE.md — `crates/otio-timeline`

> Dev conventions (Alex Joss): see `../../CONVENTIONS.md`.

## What
An **egui NLE timeline widget** over `otio-edit` — the "face" of the timeline
(DaVinci/Premiere/After-Effects style): track lanes, clip rectangles, a time
ruler, a playhead, and mouse editing with undo/redo. The "brain" (what an edit
does, undo, layout) lives entirely in `otio-edit`/`otio-core`; this crate holds
ONLY UI state + rendering. Lib root `src/otio_timeline.rs`.

## Packaging — workspace member, egui gated behind `widget`
egui (and eframe for the example) are heavy, so the WHOLE crate is gated behind an
optional **`widget`** feature (`#![cfg(feature = "widget")]` at the lib root; egui +
the otio model crates are optional deps). With the default (no features) the crate
is EMPTY and pulls nothing — so it can be a normal member of the otio-rs workspace
WITHOUT egui entering a default/core build (`cargo tree` shows 0 egui by default).
A UI host opts in with `--features widget`; downstream, egui-widgets-rs's
`egui-otio-timeline` re-export enables it. The eframe demo example is behind `demo`
(implies `widget`): `cargo run -p otio-timeline --example demo --features demo`.

## API (how a host app uses it)
```rust
let mut state = otio_timeline::TimelineState::default(); // once, owned by the app
// Source-compatible mutating wrapper:
otio_timeline::timeline_ui(ui, &mut editor, &mut state);

// SSOT/command-oriented host (Editor is immutable while the widget runs):
let response = otio_timeline::timeline_ui_commands(ui, &editor, &mut state);
let response = otio_timeline::apply_timeline_response(&mut editor, &mut state, response);
```
- `timeline_ui_commands[_ex]` reads `&Editor`, mutates only UI capture state, and returns
  a revision-stamped `TimelineResponse { base_revision, commands, ... }`. `TimelineCommand`
  covers edit verbs, transport/seek/scrub, cancellation, and surfaced errors. It does NOT
  own or advance a playback clock; an SSOT host advances its transport independently.
- `apply_timeline_response` consumes `commands` (successful work moves to `applied_commands`,
  refused work to `rejected_commands`) and rejects a stale NodeId-bearing batch before any
  command runs. This prevents deterministic NodeId collisions across replaced Editors from
  editing the wrong projection and makes applying the returned response again a no-op.
- `timeline_ui` / `timeline_ui_ex` remain source-compatible mutating wrappers. They apply
  emitted commands before advancing the legacy local clock, so Pause adds no final `dt` and
  Play begins in the accepted toggle frame, then request repaint.
- `TimelineState` — UI-only: `px_per_sec` (zoom), `scroll_x`, `track_h` (lane
  height), `playhead`, `rate` (frame snap), `playing`, `selection: HashSet<NodeId>`,
  `tool` (**Edit / Blade**), `last_error`, + private layout/gesture/projection capture.
- Selection is `NodeId`-keyed and survives undo/redo, but is cleared when the whole
  Editor/projection is replaced (fresh Editors have distinct revision epochs).
- Extensions via `timeline_ui_ex` / `timeline_ui_commands_ex` and `TimelineOpts`:
  `thumbnail: Option<&dyn Fn(NodeId)->Option<egui::TextureId>>` paints a host texture as
  each clip body's background (Gaps skipped). `NodeId` is re-exported.

## Tools & gestures (DaVinci-style: one context tool + a blade)
- **Edit** (default): the grab ZONE picks the verb. Drag a clip BODY → `ops::move_by`
  (reposition, leaves a gap; neighbours untouched). Drag the left edge →
  `ops::trim_in_record`: the edge follows the pointer, a preceding Gap is inserted/grown or
  consumed, and the clip's record end plus following placement stay fixed. Drag the right
  edge → `ops::trim_out` on that clip. The old `ops::trim_in` remains the source-only API.
  **Ctrl is the "alternate edit" modifier**: Ctrl+body → `ops::slide`, and
  Ctrl+edge on a SHARED cut → `ops::roll` (both edges move; a shared cut = neighbour is a
  clip, via `prev_clip`/`next_is_clip`). A plain click selects; an empty-space drag
  rubber-band selects. The clip hit-test extends `HIT_PAD` (4 px) beyond each edge so a
  near-miss still grabs the clip instead of starting a marquee.
- **Viewport navigation**: MMB drag pans the whole timeline without starting an edit, scrub, or
  marquee. Ctrl+wheel zooms around the pointer's timeline time; plain wheel scrolls horizontally.
  Navigation is hover-scoped and frozen while a primary edit drag owns the viewport.
- **Context menu**: RMB selects the item under the pointer and opens Select All / Deselect All /
  Delete / Ripple Delete / Fit All / Mark In / Mark Out. Delete and fit share the keyboard helpers,
  so menu and shortcut semantics cannot drift.
- **In/out marks** (record-side of a three-point edit): `I`/`O` set marks at the playhead
  (public `mark_in`/`mark_out`), rendered as a translucent band. `clear_marks` /
  `clear_marked_range(editor, state, ripple)` clear them / ripple-lift the marked clips.
- **Blade**: click → `ops::razor` (split at the cursor time).
- **Cursors**: the native OS arrows are hidden (`CursorIcon::None`) and replaced by drawn
  Phosphor glyphs on a foreground layer — ↔ (`ARROWS_HORIZONTAL`) over an edge / while
  trimming, 4-way move arrows (`ARROWS_OUT_CARDINAL`) over a body / while moving, scissors
  in Blade. The edge-grab band is `EDGE_GRAB` (9 px) so the ↔ zone is easy to hit. The HOST
  uses `egui_widgets_config::icons` and registers the font with
  `egui_widgets_config::add_icon_font` so the glyphs render
  (the demo does this in `main`).
- **Delete / Backspace**: on the selection — a CLIP lifts (`ops::lift_clip`, leaves a
  gap), Shift ripples it (`ops::ripple_delete_clip`, closes up); a selected **GAP**
  always ripples (closes the empty space, Premiere-style). Gated on hover so it
  doesn't eat Delete from other widgets.
- **Snapping**: on release, the committed moving edge (leading edge for Move/Slide,
  dragged left edge for record-side TrimIn, out edge for TrimOut, selected roll edge) snaps
  to clip edges, track start, and playhead (`snap::snap_targets` + `snap::snap`, 8 px hysteresis).
  Cross-track Move uses the destination lane's targets. **Hold Alt to bypass.**
- Applied ONCE on drag release (one undo step per gesture). A missing pointer position
  emits an explicit `Cancel` instead of dropping the edit. Apply failures surface in
  `TimelineResponse.errors`, terminal `TimelineCommand::Error` events, and legacy
  `state.last_error`; pending commands are consumed even on rejection.

## How it connects to otio-edit (layering)
- **Draw**: `state.layout.track_layout(editor, track)` → `[(NodeId, TimeRange)]`;
  clip rect `x = start*px_per_sec - scroll_x`, width `= dur*px_per_sec`. Tracks
  enumerated from the Document (Timeline → `tracks` Stack → children). Colour by
  canonical schema (Clip / Gap / Transition).
- **Refresh**: immediate-mode; `LayoutCache` recomputes a track only when
  `editor.revision()` advanced. A distinct revision epoch resets transient state when a
  host replaces the entire Editor/projection, even when deterministic NodeIds repeat.
- The widget NEVER mutates the Document directly — every change goes through
  `otio_edit::ops::*` so undo is recorded. Keep it that way.

## Status
- [x] Track lanes + headers, clip rects, ruler, playhead; pointer-centered zoom (Ctrl+wheel),
  pan (MMB drag), scroll (wheel), and off-screen culling.
- [x] Tools: Move / Slide / Trim (+ Roll on a shared edge) / Razor; select (click).
- [x] Delete (lift a clip / close a selected gap) + Shift-Delete (ripple a clip).
- [x] Snapping to clip edges / track start / playhead (Alt bypass).
- [x] Undo/redo (demo wires Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z).
- [x] Live drag ghost: a translucent clip preview at the exact snapped target the drop
  will commit (Move/Slide = full-body ghost, Trim = resized edge, Roll = edge band),
  drawn alongside the pointer guide line. Shares `snap_drag_delta` with the release
  path so the ghost and the committed edit agree to the pixel.
- [x] Marquee (rubber-band) multi-select: dragging from empty space selects every clip
  the rectangle touches (Shift adds to the current selection).
- [x] Cross-track drag: a Move dropped on a different SAME-KIND lane (video→video,
  audio→audio; a foreign-kind drop just repositions on the source track) moves the clip
  via `ops::move_clip_cross_track` — a same-duration gap is left at the source (downstream
  stays put) and the destination is overwritten at the drop time. The ghost previews on
  the destination lane. The sample has V1+V2 (Video) + A1 (Audio) so both the move and the
  kind-guard are demonstrable.
- [x] Timecode ruler: HH:MM:SS:FF at `rate`, offset by the timeline's `global_start_time`
  (`fmt_tc` / `timeline_global_start`).
- [x] Playback transport: command API emits toggle/seek/scrub start/stop and owns no
  advancing clock. Legacy wrappers still advance `state.playhead`, loop within in/out marks,
  stop at content end, and pause/resume around ruler scrubbing for source compatibility.
- [x] Track height: public `TimelineState::track_h` (clamped `TRACK_H_MIN..=TRACK_H_MAX`)
  drives all lane geometry; the demo has a "Track H" slider. The widget self-sizes its
  total height to the track count, so vertical overflow is the host ScrollArea's job.
- [x] Per-clip thumbnails: `timeline_ui_ex` + `TimelineOpts::thumbnail` provider.
- [x] Three-point RECORD side: in/out marks (`I`/`O`, `mark_in`/`mark_out`) + band render +
  `clear_marks` / `clear_marked_range` (ripple/lift the marked span).
- [ ] **Not yet**: the SOURCE side of a three-point/overwrite edit — a single timeline
  widget has no source monitor, so filling the marked range from a source clip belongs to a
  host with a viewer; intentionally out of this widget's scope for now.

## Gotchas
- `rate` (default 24) is the frame grid edits snap to; a real app should read it
  from the timeline/clip instead of assuming 24. The widget clamps a NaN/≤0 `rate`
  and a NaN/out-of-range `px_per_sec` on entry (public fields, untrusted).
- egui 0.34 API used: `CornerRadius`, `StrokeKind`, `Sense::click_and_drag`,
  `drag_started`/`drag_stopped`. Note `Sense::drag()` does NOT emit clicks in 0.34.
- Snap targets include the dragged clip's own edges → near the origin the edge
  snaps back to its start (a "back to start" detent); harmless, kept for v1.

## Verify
`cargo clippy -p otio-timeline --features widget --all-targets -- -D warnings`,
`cargo test -p otio-timeline --features widget`, and
`cargo run -p otio-timeline --example demo --features demo` for the window.
