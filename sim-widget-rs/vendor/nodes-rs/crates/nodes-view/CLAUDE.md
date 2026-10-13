# nodes-view — wgpu graph renderer + GPU picker (CLAUDE working notes)

> Keep this file current. This crate is the wgpu-first viewport: it draws the graph (grid,
> node bodies/titles/pins, bezier/tube wires, graph-plane text) and owns the **GPU object-id
> pick** buffer. No egui types in the public API; no interaction state (that's `nodes-interact`
> / `nodes-editor`). It sits on `nodes-core` (data model + `chrome`/`graph_rect` geometry) and
> `wgpu-text` (glyphs).

## Layout
```
src/
  camera/        3D perspective Camera facade over cam-controls (fov_y, reversed-Z infinite-far)
  camera_control.rs  CamControl: the ONE camera gesture brain (lens Flat↔Perspective + FOV ease + orbit)
  nav.rs         Camera-navigation SSOT: NavInput/FlyAxes + apply_nav/apply_fly (see below)
  engine/
    mod.rs           GraphRenderer: passes, offscreen targets, request_pick/last_gpu_pick
    shape2d.rs       Shape2d SDF kinds, ShapeStyle, PickLayer, PickTarget, ShapeInstance
    shape_scene.rs   build_shape_instances: node/title/pin/field/chrome instances + IdRegistry
    shape_pipeline.rs ShapeInstanceGpu packing (instance_to_gpu), color + id pipelines
    shape.wgsl       SDF vertex/fragment; color (fs_main) + id (fs_id) fragments
    pick_index.rs    CPU uniform-grid PickIndex (pick_target_at) — nodes-egui ONLY now
    wire_scene.rs    tube/bezier wire instances
    grid.rs / text_labels.rs / *_pipeline.rs / *.wgsl
    text_renderer.rs graph-plane glyphs via wgpu-text
  viewport/        visible-region + node_visibility (frustum-vs-AABB cull + LOD SSOT)
  gizmo/ present/   overlays, swapchain present
```

## Camera navigation = the SINGLE mapping authority (`nav.rs`, added 2026-07-10)
Both frontends (winit `nodes-editor` + embeddable egui `nodes-egui`) drove the camera
separately, so `nodes-editor` had FPS/Space FLY and `nodes-egui` did not. Fixed by one
frontend-agnostic seam here (Camera lives in this crate, so no dep cycle):
- `NavInput { fly: Option<FlyAxes>, orbit, pan_uv, zoom }` = decoded per-frame nav.
- `apply_nav(cam, &NavInput, dt, vp_px: Vec2)` = dispatch fly/orbit/pan/zoom + ONE
  `update_dynamics`.
- `apply_fly(cam, &FlyAxes, dt, vp: ViewportSize)` = fly intent sequence (Look/Thrust/
  Roll/Boost) ALONE, no `update_dynamics` (for a frontend that already ticks it).
Each frontend keeps its own INPUT reading + px->value conversion (feel unchanged) +
platform bits (winit cursor-grab); HOW nav maps to the camera lives ONLY here.
`nodes-egui` now routes all nav through `apply_nav` and gains fly (hold RMB + WASD/QE,
additive: plain RMB-drag still zooms) -> every egui embedder (incl. rv-rs) gets it.
`nodes-editor::step_camera` reuses `apply_fly` (identical intents). 4 unit tests in `nav.rs`.

## Camera gesture brain = `CamControl` (ONE shared brain, added 2026-07-12)
`camera_control.rs` (`CamControl` + `LensMode {Flat,Perspective}` + FOV consts) is the ONE
host-agnostic camera GESTURE policy both frontends drive (winit `nodes-editor` + egui `nodes-egui`),
so the persp↔ortho lens + orbit handling has a single implementation instead of drifting copies:
- **Lens intent** (`LensMode` + `persp_fov`); **`step(cam,dt,anim,secs)`** = THE single per-frame
  `fov_y` writer (eases toward the intent's target via `Camera::adjust_fov`, with a PURE top-down
  flatten GATE — no latch, self-corrects if you orbit away mid-flatten).
- **`orbit(cam, delta_px, sensitivity)`** ensures the orbit backend, opens perspective, and scales
  the angular step by `tan(fov/2)/tan(FOV_PERSP/2)` so a pixel maps to the SAME on-screen sweep at
  any FOV (kills the near-ortho "wild spin"); returns applied `(yaw,pitch)` for host coast-seeding.
- **`ensure_orbit`** + **`set_persp_fov`/`nudge_persp_fov`** (Ctrl+Alt dolly-zoom drives the
  perspective TARGET, not `fov_y` directly).
Per-host bits stay per-host: cursor grab + release-inertia coast. This **replaced/deleted** the old
`fly.rs` `PerspectiveFly` (egui's separate FOV machine) AND the editor's `LensMode`/`fov_anim`/
`pending_flatten_fov`. Exported at the crate root: `CamControl, LensMode, FOV_FLAT, FOV_PERSP,
FOV_ORBIT_TRIGGER`.

**`Camera::adopt_orbit_backend()`** (`camera/flat.rs`): switch a fly backend (Fps/Space) to the
Houdini orbit rig and reseat the pivot on the graph-plane point under the view (eye frozen). Fly
backends map an `Orbit` intent to free mouse-LOOK, and RMB-fly release KEEPS the Space backend to
coast — so without this, Alt-orbit / `H` after a flight free-looked into a weird pose. `CamControl`
calls it before every board orbit / framing gesture (each gesture guarantees the backend it needs).

## Rendering passes (`engine/mod.rs::render_scene`)
1. Color shapes (MSAA) -> color target.
2. Tubes/wires.
3. **Pick id pass** — single-sampled `R32Uint` id target (`wgpu_pick::IdTarget`), its own
   `id_depth`. Draws the SAME instance buffer as the color pass (`fs_id` returns `pick_id`).
4. Graph-plane text (titles + port names) on the resolved color target, depth-tested vs `id_depth`.
Then `IdPicker::submit` (copy the requested pixel) + `queue.submit` + `IdPicker::poll` — the
readback is **synchronous same-frame** (this-frame registry <-> this-frame id texture <-> poll).

## GPU object-id picking = the SINGLE hit-test authority (unified 2026-07-08)
Everything on the graph plane is classified by ONE pick buffer. `PickTarget`:
`NodeBody`, `NodeTitle`, `Pin{node,side,index}`, `ViewModeToggle`, `BypassFlag`, `DisplayFlag`,
and **`NodeField{node,port}`** (inline editable field). `PickLayer` (draw/pick z order):
`Body < Title < Outline < Field < Pin` (fields beat body/title; pins always win on the border).

- `shape_scene::build_shape_instances` fills `wgpu_pick::IdRegistry<PickTarget>`: every
  `pickable` instance gets `pick_id = registry.alloc(target)`. `last_gpu_pick` maps the read
  pixel back to a `PickTarget`.
- **Invisible pick-only quads**: inline fields (`NodeField`) and the title chrome flags
  (`ViewModeToggle`/`BypassFlag`/`DisplayFlag`) are emitted as fully-transparent quads that must
  still write their id. `instance_to_gpu` sets the free `p1.w` slot to `1.0` for those pick
  targets; `shape.wgsl` `fs_main` DISCARDS them (no color, no depth write -> can't occlude the
  widget/text drawn later), while `fs_id` gates them on GEOMETRIC coverage (ignoring color alpha)
  so an alpha-0 rect still stamps its id inside its bounds. Their VISUALS are drawn by the
  editor's widget layer (field value box, chrome checkboxes) — GPU pick classifies without a
  second draw.
- **Geometry is shared, never re-derived**: field rects come from
  `nodes_core::chrome::inline_field_rects` (same fn the editor uses to place widgets); chrome
  rects from `nodes_core::graph_rect::node_regions`. Pick can't drift from the visual.
- Editor consumes pick via `request_pick(px,py)` (on `CursorMoved`) -> `last_gpu_pick()` (read in
  `draw_frame`); it is the ONLY hit-test the editor uses (press, drag, wire, chrome, fields).

## CPU `PickIndex` (`pick_index.rs`, `pick_target_at`)
Same instances, uniform-grid CPU hit-test with layer/draw-order resolution. **Retained ONLY for
the `nodes-egui` bridge** (which still CPU-picks). The wgpu editor no longer calls it. Removing
it is gated on migrating nodes-egui to GPU pick.

## Theming (SSOT `ui_theme::Theme`)
All graph colors come from the shared `ui-theme` palette — there are NO hard-coded color consts in
`shape_scene`/`grid`/`wire_scene` anymore. `GraphRenderer` holds a `theme` (`set_theme` /
`impl ui_theme::Themed`); `render_scene` passes `&self.theme` into `ShapeSceneParams.theme`,
`grid::build_grid_vertices`, and `append_marquee_shape`. Node body/outline read `node_body`/
`node_body_frame`/`node_outline`/`node_outline_hot`; pins read `pin_stroke`/`pin_stroke_hot`; grid
reads `grid_minor`/`grid_major`/`grid_axis`; marquee/wire-preview read `wire_preview_*`. Category
header fills still come through `nodes_core::category_*` (which now delegates to `ui_theme`).
Per-TYPE pin/wire colors stay in `ValueTypes` (a runtime registry, not the static palette).

## Node visibility: frustum cull + screen-size LOD (SSOT, 2026-07-09)
`viewport/node_visibility.rs` is the ONE culling/LOD policy for every graph layer. Pure fn
`compute_visible_nodes(layout, camera, viewport, VisibilityConfig) -> VisibleNodes`:
- **Cull**: PROJECT each node's world AABB (rotated card corners via `LayoutNode::xform`, ±`z_extrude`
  along the plane normal for Mesh3D) into CLIP space and drop it only if provably outside one clip
  edge (or fully behind, all corners `w<=0`). Conservative — never culls a straddling box. This is
  why it works at grazing angles where the old `visible_graph_uv_rect` (screen→plane unproject)
  collapsed. `Frustum::from_view_proj` stores the matrix; no fragile plane extraction (safe for the
  reversed-Z infinite-far projection).
- **LOD** (`NodeLod`): from projected node HEIGHT in px. `Full` → `NoText` (<`lod_no_text_px`≈18) →
  `BodyOnly` (<`lod_body_px`≈8). `draws_text()` gates plane labels; `draws_detail()` gates
  pins/inline-fields/chrome/widgets. Mesh3D bodies have no LOD (drawn iff visible).
- **Wiring**: `render_scene` (and `sync_interaction_pick`) build ONE `VisibleNodes` and hand it to
  `shape_scene`, `wire_scene` (wire kept if EITHER endpoint visible), `text_labels`, and
  `node_scene` (Mesh3D). The editor's node-widget layer calls the SAME fn (see nodes-editor) —
  computing twice is intentional and cheap (idiomatic, like `build_shape_instances`), and guarantees
  no "half node" (body culled but fields kept), the exact bug that had disabled the old cull.
- **Toggle**: `GraphRenderer::set_cull_enabled` (default true); editor menu "Frustum cull + LOD"
  + `settings.render.frustum_cull`. Off ⇒ `VisibleNodes` reports all-visible/Full.
- `VisibilityConfig::for_backend(cull, mesh3d)` is the SSOT backend→`z_extrude` map
  (`MESH3D_Z_EXTRUDE`), used by BOTH the renderer and the editor so their sets are identical.

## Pin world positions (rotated nodes)
`LayoutCache::pin_positions` are LOCAL (unrotated). Any WORLD-space consumer (tube wires,
`last_layout_pin` for wire-drag preview) MUST go through `LayoutCache::pin_world(node,side,idx)`,
which applies the node `NodeXform` — otherwise wires miss the sockets of rotated nodes (the pin
dots rotate in-shader about the node center). `cache.edges` is finalized the same way but is NOT
read by the tube path (kept for 2D-line consumers).

## Gotchas
- **Sticky `Note` node renders as a SOLID card** (`shape_scene`, 2026-07-12): `is_note_node` gets a
  self-contained early block (opaque `node_body` RoundedRect + highlight-aware outline, `pick=NodeBody`,
  no title strip / pins) then `continue` — deliberately distinct from a `Frame` (translucent
  `node_body_frame` + hollow-border pick). The note's multi-line TEXT is NOT drawn here: the editor's
  widget layer places a `wgpu_widgets::MultilineEdit` over the body (render + edit). No new `PickTarget`
  variant — a note-body pick focuses that text widget (avoids the "breaking across every match" churn).
- MRT rule: the id target must be the SAME size as the color target every frame —
  `ensure_offscreen_target` resizes color/depth/id together.
- Adding a `PickTarget` variant is a breaking change for every match: `pick_index::resolve`,
  `nodes-editor::handle_primary_press`, `nodes-egui::graph_viewport`, benches/tests. The compiler
  enforces exhaustiveness — chase every arm.
- `shape.wgsl` shares `shape_sdf` across color/id/pick-only paths so all three test identical
  geometry; the `p1.w` pick-only flag is the ONLY divergence.
- Camera is genuine 3D perspective (reversed-Z, `fov_y`); there is no ortho width anymore.
- Run cargo on Windows via **pwsh** (project stable toolchain).
