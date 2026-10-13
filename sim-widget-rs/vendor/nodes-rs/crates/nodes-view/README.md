# nodes-view

Wgpu-first graph viewport renderer. Turns a `nodes-core` `LayoutCache` into GPU
instances and draws the graph on a 3D working plane through a real perspective camera.
**No egui types in the public API.**

- SDF node bodies (default) or real 3D cards (Mesh3D via `wgpu-node`).
- Tube wires (`wgpu-tube`), graph-plane glyphs (`wgpu-text`).
- One GPU object-id pick buffer as the sole hit-test authority (`wgpu-pick`).
- Frustum cull + screen-size LOD (`compute_visible_nodes`), shared with the editor's
  widget layer.
- All colours from `ui_theme::Theme` (SSOT).

See `CLAUDE.md` for internals and [`../../docs/mdbook`](../../docs/mdbook) → Whitepaper ›
Rendering & picking / Camera, and Developer Guide › Renderer internals.
