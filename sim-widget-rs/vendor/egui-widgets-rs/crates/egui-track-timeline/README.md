# egui-track-timeline

A generic clip/keyframe Timeline that borrows host view/model state and returns edit intents. The host owns scene evaluation, selection mapping, Undo, playback, caching, and I/O.

## Keep the ruler fixed while rows scroll

Use `TrackTimeline::show_pinned_ruler(ui, view, model, top)` when a vertical ScrollArea contains the logical layer canvas. Capture `top` from the viewport before entering its scrolling content. It is a screen-space Y coordinate, not a frame number or logical scroll offset.

```rust,ignore
let ruler_top = ui.cursor().min.y;
egui::ScrollArea::vertical().show(ui, |rows_ui| {
    let response = egui_track_timeline::TrackTimeline::default()
        .show_pinned_ruler(rows_ui, view, model, ruler_top);
    // Apply response.actions through the host's command/Undo system.
});
```

The widget preserves logical layout allocation and content height, places the ruler at `top`, and clips hidden track/key hit regions below it. `TimelineResponse::track_tops` continues to describe scrolling row positions; `ruler_rect` describes the pinned screen-space ruler. Existing ruler scrubbing and work-range handles use that rectangle.

If the host draws a separate names/property outline, pin its header to the same Y coordinate and clip its rows below `ruler_rect.bottom() + 4.0`. Restore the previous clip rectangle before drawing/registering host splitters or overlays. Use the returned track geometry so names and time bars share one vertical scroll offset.

Normal `show` keeps the ruler in logical flow. Both entry points share the same rendering/gesture implementation. The pinned-ruler API does not provide playback caching or scene-specific hotkeys.

## Preserve input ownership

Pass the actual clipped child UI for the Timeline. Logical geometry can extend outside a dock panel, but those offscreen regions must not initiate gestures. Foreground overlays and adjacent panels retain their pointer input. Hidden bars underneath the pinned ruler cannot capture scrubbing.

## Verify the contract

```sh
cargo test -p egui-track-timeline
```

The pinned-ruler regression checks fixed ruler position and content height, moving row positions, a Seek action on the ruler, and no hidden-bar Select/Slide action. The adjacent-panel/foreground-overlay regression checks clipping and layer ownership.
