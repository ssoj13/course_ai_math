# egui-otio-timeline

Working notebook for this crate. Keep current.

## What it is
A **thin re-export** of the OTIO timeline widget that lives in the sibling
`otio-rs` repo. `otio-timeline` already IS the complete egui NLE timeline widget
over `otio-edit` (Move/Slide/Trim/Razor tools, Roll on shared edges, Delete /
Shift-Delete incl. Premiere-style close-a-selected-gap, snapping to edges/playhead
with Alt-bypass, undo/redo), so this crate only:
- `pub use otio_timeline;` — the widget (`timeline_ui`, `TimelineState`, `Tool`, …)
- `pub use otio_edit;` — the edit engine (`Editor`, `ops`, undo/redo)

No wrapper logic. Same pattern as `egui-nodes-view` re-exporting nodes-rs.

## Why it exists
So a host consumes ONE crate from this widget library (`egui-otio-timeline`)
instead of reaching into `otio-rs` directly. The demo uses it via
`egui_otio_timeline::{otio_edit, otio_timeline}`.

## The `widget` feature (important)
`otio-timeline` gates its ENTIRE widget behind its own `widget` feature — egui is
heavy and there it is an OPTIONAL dep, so `otio-rs`'s default/core workspace build
pulls NO egui (the crate is empty without the feature). This re-export enables
`features = ["widget"]` on the git-ref dep, so downstream gets the real widget.
If you ever see "unresolved import otio_timeline::…", check that feature is still on.

## Relationship to the other timeline crates
- `egui-timeline` / `egui-track-timeline` / `egui-keyframes` — this repo's OWN
  timeline widgets (frame rulers, keyframe lanes), egui-only, no OTIO model.
- `egui-otio-timeline` — backed by the real **OpenTimelineIO** model + edit engine
  (otio-rs): tracks/clips/gaps, source ranges, ripple/roll/slide/trim/razor ops.

## Gotchas
- **git-ref, not path**: deps come from `ssh://git@github.com/ssoj13/otio-rs.git`
  (branch=main). After pushing otio-rs, bump here with
  `cargo update -p otio-timeline -p otio-edit` and commit the Cargo.lock.
- **egui-only, no wgpu** — unlike `egui-nodes-view`, this stays a light,
  wasm-friendly re-export; it pulls only the otio-rs model/edit tree + egui.
- otio-rs consumer gaps (roll/delete/snap not yet wired in the widget UI, plus
  packaging notes) are parked in `otio-rs`'s own docs / this repo's `TODO.md`.
