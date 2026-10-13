# egui-viewport-overlays

Записная книжка для этого крейта. Дополняй при изменениях.

## Что это
Generic 3-D viewport **overlays** for egui 0.34, painted on top of an
already-rendered viewport rect: axis orientation indicator, world-AABB and
camera-frustum wireframes, ground-plane grid, camera letterbox **mask** +
**reticles** (crosshair / rule-of-thirds). The widget owns NO scene and NO GPU —
it only takes a `Rect` + caller-supplied view / projection matrices and draws
with `egui::Painter` primitives.

Free functions (stateless painters), not a `show(...) -> Action` panel — there is
no interaction, only drawing. `OverlayState` is a caller-owned visibility-toggle
struct (serde) for the host's settings UI.

## Откуда взято
Gathered from `vfx-rs/crates/view/vfx-view/src/panels/overlays.rs` (which
references usdviewq `stageView.py`). The ONLY host coupling was
`usd_gf::{Matrix4d, Vec3d}`.

## Decoupling decisions
- **USD math → tiny in-crate `Mat4` / `Vec3`.** `usd_gf::Matrix4d`/`Vec3d` are
  replaced by a row-major `Mat4([[f64;4];4])` + `Vec3{x,y,z}` (the public API
  stays USD row-vector; glam's column-vector convention would invert it). The
  one exception is NDC -> screen: `project_to_screen` applies
  `egui_gizmo::viewport_matrix` (glam `DMat3`), so the overlays and the gizmo
  share that formula instead of keeping two copies (2026-09-26). The
  ported `Mat4` keeps the **row-vector** convention verbatim: `mul` is standard
  matrix multiply, `transform_point` is `[p,1]*M` + perspective divide, `row`/
  `column` accessors match `GfMatrix4d`, and `inverse` is a cofactor/adjugate
  inverse. Five unit tests guard the math (`mul`/`inverse` round-trip,
  singular = None, identity transform). Projection placement is therefore
  bit-identical to vfx-view.
- **Caller passes row-major matrices.** Build a `Mat4` with `Mat4::from_rows([[f64;4];4])`
  in the SAME `m[row][col]` layout `GfMatrix4d` stores. vfx-view converts its
  `usd_gf::Matrix4d` at the call boundary.
- **Mask / reticle colours are parameters; axis / grid colours are intrinsic.**
  Axis R/G/B and grid greys are intrinsic to the overlay's meaning (like a colour
  picker's wheel hues — exempt from the `egui-widgets-config` token migration),
  so they stay as `const`. Mask / reticle colours are caller args to match the
  host theme.

## Public API (src/egui_viewport_overlays.rs)
- `struct Vec3 { x, y, z: f64 }` + `new`.
- `struct Mat4(pub [[f64;4];4])` (row-major) + `from_rows`.
- `struct OverlayState { show_axes, show_selection_bbox, show_scene_bbox,
  show_camera_mask, show_reticles: bool, camera_aspect: f64 }` (`Default`, `new`, serde).
- `fn draw_axes(ui, rect, view: &Mat4)`
- `fn draw_bbox(ui, rect, min: Vec3, max: Vec3, view, proj, color)`
- `fn draw_grid(ui, rect, view, proj, extent, spacing, major_every, is_z_up)`
- `fn draw_camera_mask(ui, rect, aspect, mask_color)` / `draw_camera_mask_outline(...)`
- `fn draw_reticles(ui, rect, reticle_color)`
- `fn draw_camera_oracle(ui, rect, cam_view, cam_proj, vp_view, vp_proj)`
- `fn contrasting_bbox_color(clear) -> Color32`

## Gotchas
- **Row-vector convention** — when projecting, dot with `VP.column(j)` NOT
  `VP.row(j)`. Passing a glam/column-vector matrix here would mirror the scene.
- AABB / frustum edges are culled per-corner when behind the camera (`clip_w<=0`),
  so a partially-behind box draws only its visible edges (same as the source).

## TODO
- [ ] Demo tab in `egui-widgets-demo` (rotating cube + grid + axis indicator).
- [ ] Consume from vfx-view (delete the local `panels/overlays.rs`, convert
      `usd_gf::Matrix4d` → `Mat4::from_rows` at the call sites).
