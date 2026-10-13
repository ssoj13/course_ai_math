# CLAUDE.md — `crates/egui-timeline`

Conventions: repo-root `CLAUDE.md`.

## What
Reusable, generic egui time-track widget. No app types — usable by exr-view and
any other egui app. Used by exr-view's transport bar (cache-tinted; replaced the
hand-rolled slider + on-viewport scrubber).

## API
`Timeline::new().height(h).minor_ticks(b).accent(c)` → builder.  
`Timeline::show(ui, pos, count, tint, label) -> TimelineResponse { seek, response }`:
- `pos`/`count` — indexed slot set (positions into a present-frame sequence).
- `tint(i) -> Option<Color32>` — per-slot fill (cache "loaded" bar); run-length
  coalesced so a 1000-frame fill is a few draws.
- `label(i) -> String` — tick / playhead text (caller maps slot → real frame number).
- `seek` — new position when the user clicks/drags the track.

Colours come from the active `egui::Visuals` + configurable accent. egui 0.34.

## Gotchas
- Works in **slot/position space**, not raw frame numbers — sequence gaps are
  handled by the caller's `label`/`tint` mapping.
- No let-chains (workspace MSRV 1.85).
