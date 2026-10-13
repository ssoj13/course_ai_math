# CLAUDE.md — `egui-gizmo` (WIP working notebook)

> This is the working notebook for an in-progress crate. Keep it current: record
> the goal, the design, decisions, gotchas, and the roadmap. Append as the design
> firms up; compress stale notes into the changelog.

---

## THE TASK

Build a **production-grade, hybrid 2D/3D transform gizmo** for egui — the on-canvas
manipulator artists grab to move / rotate / scale things. The bar is the gizmos in
**Maya, Houdini, and Nuke**: precise, legible, predictable, and **fully
customizable** so it fits any task (3D scene editing, 2D compositing transforms,
bone/rig handles, light manipulators, custom tool handles).

Non-negotiables:
- **Hybrid 2D + 3D** from one core. A 3D perspective scene and a flat 2D
  composite (Nuke-style) are the same widget with a different projection + handle
  set — not two separate code paths.
- **Flexible / customizable for ANY task**: the handle set, colors, sizes,
  hotkeys, snapping, spaces, and even custom handle shapes are caller-configurable.
- **Hermetic**: depends only on `egui` (+ `glam` for math). No host engine types;
  the gizmo never owns the object — it reports a delta `Transform` and the caller
  applies it. (Same immediate-mode contract as the rest of `egui-widgets-rs`.)
- **Think it all through** before coding: this doc is that thinking.

---

## What "professional" means (behaviors to match)

From Maya / Houdini / Nuke, the table stakes:

1. **Three modes + universal**: Translate (T/W), Rotate (R/E), Scale (S/R), and a
   combined "universal" manipulator showing all three at once (Houdini handle).
2. **Axis + plane handles**: per-axis arrows (X/Y/Z), planar quads (XY/YZ/ZX),
   screen-space center handle (free translate / trackball rotate / uniform scale).
3. **Spaces**: World, Local (object), Parent, View/Screen, and a **custom** space
   (caller-supplied basis). Toggleable live (Maya's world/object toggle).
4. **Pivot control**: the gizmo origin can be the object pivot, the bounding-box
   center, a custom point, or the median of a multi-selection. Editable pivot mode.
5. **Snapping & precision**: grid/increment snap (translate), angle snap (rotate,
   e.g. 15°), relative vs absolute; modifier keys for slow/precise drag; numeric
   entry (type a value while dragging — Houdini-style).
6. **Legibility**: handles keep a **constant screen size** regardless of camera
   distance (3D) or zoom (2D); hovered/active handle highlights; the inactive axes
   dim; a readout (delta value) follows the cursor.
7. **Correct hit-testing & occlusion**: closest handle wins; in 3D, handles facing
   away are de-emphasized; planar handles only active when the plane faces the
   camera enough.
8. **Multi-select**: one gizmo drives N objects around a shared pivot; per-object
   vs shared-pivot transform application is the caller's choice (we report the
   delta + pivot).
9. **2D / Nuke specifics**: translate/rotate/scale **+ shear + pivot offset** on a
   2D plane; corner + edge handles of a bounding box; aspect-locked scale;
   center/rotate-around-pivot; the classic Nuke "Transform" + "CornerPin" handles.

---

## Architecture (separation of concerns)

Mirror the structure that makes `transform-gizmo` (vendored in `_ref/`) good, but
own it so we can extend to 2D and custom handles:

```
egui-gizmo/
  src/
    egui_gizmo.rs   # lib root: public API, re-exports
    math/           # pure math — NO egui. Ray/plane intersection, screen<->world,
                    #   constant-screen-size scaling, transform compose/decompose.
    space.rs        # TransformSpace (World/Local/Parent/View/Custom) + pivot model.
    handle/         # handle definitions: Arrow, Plane, Rotation ring, Scale box,
                    #   Screen, Shear, CornerPin, Custom(trait). Each: hit-test +
                    #   drag->delta + paint.
    mode.rs         # GizmoMode (Translate/Rotate/Scale/Universal) -> handle set.
    interaction.rs  # input state machine: hover -> press -> drag -> release,
                    #   axis/plane constraint, snapping, numeric entry, modifiers.
    paint.rs        # egui Painter rendering of handles (2D + 3D projected).
    config.rs       # GizmoConfig: colors, sizes, visuals, enabled handles, snap.
    result.rs       # GizmoResult: did-interact, the delta Transform, active handle.
```

Key principle: a **camera/projection abstraction** unifies 2D and 3D. The gizmo
works in a generic `GizmoViewport { view_proj: Mat4, viewport_rect, mode_2d: bool }`.
- **3D**: caller passes its real `view_proj` (perspective/ortho) — handles project
  to screen, rays cast from the cursor.
- **2D**: caller passes an ortho `view_proj` built from pan/zoom (pairs naturally
  with `egui-viewport`); the Z handles are hidden, shear/cornerpin handles appear.

This is the "hybrid" hinge: **one math core, projection decides the dimension.**

---

## Public API sketch (to refine — NOT final)

```rust
// What the gizmo manipulates (compose/decompose with glam).
pub struct Transform { pub translation: Vec3, pub rotation: Quat, pub scale: Vec3 }

pub enum GizmoMode { Translate, Rotate, Scale, Universal }
pub enum TransformSpace { World, Local, Parent, View, Custom(Mat3) }

pub struct GizmoConfig {
    pub modes: EnumSet<GizmoMode>,       // which handles to show (universal = all)
    pub space: TransformSpace,
    pub pivot: PivotMode,                // Origin | BoundsCenter | Custom(Vec3) | Median
    pub snapping: Snapping,              // grid/angle/scale increments + modifiers
    pub visuals: GizmoVisuals,           // colors, handle size (screen px), opacity
    pub viewport_2d: bool,               // 2D mode: hide Z, enable shear/cornerpin
    pub custom_handles: Vec<Box<dyn Handle>>, // user-defined handles
}

pub struct GizmoViewport { pub view_proj: Mat4, pub rect: egui::Rect }

pub struct Gizmo { /* persistent interaction state */ }
impl Gizmo {
    pub fn new(config: GizmoConfig) -> Self;
    /// Drive the gizmo for `targets` (their current transforms). Returns the
    /// interaction result; the caller applies `result.delta` to its objects.
    pub fn interact(&mut self, ui: &mut egui::Ui, vp: &GizmoViewport,
                    targets: &[Transform]) -> GizmoResult;
}

pub struct GizmoResult {
    pub active: bool,                    // is the user dragging a handle?
    pub mode: GizmoMode,
    pub delta: Transform,                // incremental transform this frame
    pub absolute: Transform,            // resulting transform (pivot-aware)
    pub handle: Option<HandleId>,        // which handle is hot
}

// Extensibility: a custom handle is just hit-test + drag-math + paint.
pub trait Handle {
    fn id(&self) -> HandleId;
    fn hit(&self, ctx: &HandleCtx, pointer: Pos2) -> Option<f32>;     // depth/priority
    fn drag(&self, ctx: &HandleCtx, drag: DragInfo) -> Transform;     // -> delta
    fn paint(&self, ctx: &HandleCtx, painter: &egui::Painter, state: HandleState);
}
```

Design rules:
- The gizmo **mutates only its own interaction state**; object transforms are
  caller-owned. `interact` returns deltas (immediate-mode contract).
- Everything visual/behavioral routes through `GizmoConfig` so it is customizable
  without forking. Custom handles via the `Handle` trait → "for any task."
- Snapping, numeric entry, and modifier behavior live in `interaction.rs`, not in
  each handle, so they apply uniformly.

---

## A handle must MEASURE what it represents (inherited upstream bug, fixed 2026-07-30)

Upstream `transform-gizmo` 0.9 read the scale factor for **every** handle from
`distance_from_origin_2d` — the plain screen distance between the cursor and the projected gizmo
origin. That measure knows nothing about which handle is held, so all three arrows behaved like one
radial slider wearing three hats:

- dragging the X arrow straight UP, perpendicular to X, still grew the distance from the centre and
  so still scaled X;
- circling the cursor around the origin at constant radius scaled nothing at any angle (this is what
  users report as "the scale manipulator has weird rotating behaviour");
- pushing an arrow back THROUGH the origin kept growing the factor, because distance is unsigned.

Fixed in `scale.rs` by projecting the cursor ray onto the handle's own axis (`ray_to_ray` — the same
solve `translation.rs` already used for its arrows) and taking the ratio of the position along that
axis now vs. at pick time. Guard: `perpendicular_cursor_motion_does_not_move_along_the_axis`.

**The general rule for any new handle kind:** the quantity you divide by must be the quantity the
handle edits.
- **axis** handle → project onto that axis. Perpendicular motion must be a no-op.
- **plane** / **View uniform** handle → radial distance IS the edited quantity; keep it. These were
  deliberately left radial.
- degenerate case: an axis within ~2.5 deg of the view forward projects to a dot and the ray/axis
  solve is ill-conditioned — fall back to radial rather than emit a jumping factor.

Two traps that follow from it: `draw()` has no ray, so it cannot redo an axis projection — stash the
factor `update()` actually applied and report THAT (`ScaleState::last_factor`), and only draw the
radial HUD ruler for handles whose factor is radial, or the HUD shows a measurement the drag does not
use.

Not every "the manipulator is wrong" report is ours, though: playa's "scale squashes crookedly"
turned out to be its own renderer inverse-matrix bug (`S^-1`/`R^-1` transposed), with this crate
handing over a perfectly correct per-axis factor. Check what the host does with the factor before
digging here.

## Result application — what the core guarantees

`target_start_transforms` is snapshotted ONCE, on `drag_started` (or MMB `force_activate`), never
per frame — so a factor is always applied to the pose as it was at grab time and drags cannot
compound. `update_scale` is then literally `start_transform.scale * factor` for
`GizmoOrientation::Local`. Keep it that way: a host that writes the result back and re-feeds it next
frame depends on this being non-accumulating.

Shear and corner-pin are NOT TRS-representable, so their results leave the target `Transform`
untouched and travel on `GizmoResult::kind`; the gizmo persists them itself
(`Gizmo::shear` / `Gizmo::corner_pin`) and the host seeds/reads them via `set_shear_2d` /
`shear_2d`. A host that does not seed will show an un-sheared box over a sheared object.

## Coordinate-space notes (learned from the playa ref)

`_ref/gizmo/playa_gizmo.rs` drove a **2D compositor** in *frame space*: origin at
the comp center, **+Y up**, positions in pixels. egui screen space is **+Y down**,
origin top-left. The view matrix (zoom + pan) maps frame→screen. Our 2D mode must:
- take the caller's 2D view transform (pan/zoom) as the ortho `view_proj`,
- flip Y once, in the projection, not scattered through the handles,
- keep the pivot in frame space; report deltas in frame space too.
See `_ref/gizmo/playa_coords.rs` for the exact frame↔screen conversions used, and
`playa_tool.rs` for the tool-mode enum (Select/Move/Rotate/Scale) it switched on.

---

## Vendored reference (`_ref/gizmo/`) — study, don't depend

- `transform-gizmo/` — upstream `transform-gizmo` 0.9 (the math + handle core,
  engine-agnostic). **Best base for the 3D side.** Note it already targets
  `egui 0.34.3` (via ecolor/emath/epaint 0.34) and `glam 0.32`, `mint`,
  `enum_dispatch`, `enumset` — so we can either fork it or depend on it and extend.
- `transform-gizmo-egui/` — its thin egui binding (`Gizmo`, `GizmoExt`,
  `GizmoConfig`, `GizmoVisuals`) — the integration pattern playa used.
- `playa_gizmo.rs` / `playa_coords.rs` / `playa_tool.rs` — how a real 2D
  compositor wired it up (frame-space mapping, layer transform events).

Decision pending (see Open questions): **fork** `transform-gizmo` into this crate
for full control over the 2D/custom-handle extensions, **or** depend on it and add
a 2D layer on top. Forking is likely necessary for the Nuke-style 2D handles
(shear, corner-pin) and the `Handle` trait, which upstream does not expose.

---

## Roadmap (phased)

- [ ] **P0 — spike**: stand up `transform-gizmo-egui` against our egui 0.34 in a
      throwaway example; confirm the interaction feel; capture what to keep/replace.
- [x] **P1 — 3D core**: own `Transform`/space/pivot + translate/rotate/scale/
      universal handles with constant-screen-size, hover/active, snapping. Return
      deltas. (Done via the fork.) Open-handle refactor: `enum_dispatch SubGizmo` →
      `Box<dyn SubGizmoControl>` (clone_box + Debug supertrait + From shim); the
      `enum_dispatch` dep is gone. New handle kinds (shear/corner-pin/custom) no
      longer need a fixed variant set. Verified no regression on the cube example.
- [ ] **P2 — 2D mode**: ortho projection from pan/zoom (pairs with `egui-viewport`),
      bounding-box handles, aspect-lock scale, pivot offset, **shear**, Nuke
      **corner-pin**. Y-flip handled once in the projection.
- [ ] **P3 — customization**: `Handle` trait + `GizmoConfig` (colors/sizes/hotkeys/
      enabled handles), custom spaces, numeric entry, modifier precision.
- [ ] **P-HUD — drag-time ghost/HUD** (APPROVED, full pro set; do AFTER P2b):
      feedback drawn only while a handle is active, snap-aware, toggleable via new
      `GizmoVisuals` fields. Rotate = graduated dial (ticks 10°/5° + degree labels)
      + filled swept wedge + start/current radii + snap-tick highlight. Move =
      active axis extended across the viewport + start ghost + delta trail +
      distance readout. Scale = origin-tick→current ruler + ×factor readout. Numeric
      readout near cursor in ALL modes (snapped). Hooks into the active subgizmo's
      draw path (has start+delta); reuse `world_to_screen` + `painter.text`.
      Renderer-agnostic (epaint).
- [~] **P4 — polish/prod**. Audited 2026-06-19 against the actual source; most of
      P4 was already satisfied (inherited from the fork or built earlier), so do NOT
      re-implement these — they are DONE:
      - [x] **occlusion / facing logic** — already present per handle type, no extra
            code needed:
            * Rotation rings draw a FRONT-facing semicircle that grows to a full
              circle only when edge-on (`subgizmo/rotation.rs` `draw` idle branch
              `arc(FRAC_PI_2 - angle, FRAC_PI_2 + angle)` with `arc_angle()` ∈
              [π/2, π]) — i.e. the back half facing away from the camera is culled,
              exactly the Maya look.
            * Translate/scale axes + planes fade by camera angle
              (`subgizmo/common.rs` `arrow_visibility` / `plane_visibility`, hidden
              below `1e-4`). `.abs()` means toward- and away-pointing axes fade
              identically (correct: an away-pointing axis is still grabbable).
      - [x] **multi-select shared pivot** — fully wired, both pivot modes:
            `Gizmo::interact(&[Transform]) -> Option<(GizmoResult, Vec<Transform>)>`
            takes N targets, derives the pivot as their CENTROID
            (`config.rs::update_for_targets`, median pivot), and replays the handle
            delta onto EACH target (`gizmo.rs::update_transforms_with_result`).
            `update_rotation_quat` honours `TransformPivotPoint::{MedianPoint,
            IndividualOrigins}` — rotate each target about the shared centroid, or
            in place. Nuance: the manipulator's own orientation uses the LAST
            target's rotation (quaternion averaging is non-trivial; Maya likewise
            orients to the active object). API-complete; the demo currently drives a
            single target, so the N>1 path compiles + is correct but is not yet
            *shown* interactively.
      - [x] **readout HUD** — the P-HUD pass (dial/guides/ruler + numeric readout).
      - [ ] **accessibility (keyboard nudge)** — STILL OPEN: arrow-key / numeric
            nudge of the active handle. The only genuinely unbuilt P4 item.
      - [ ] **multi-object demo showcase** (optional polish) — drive 2-3 cubes from
            one gizmo in the demo to *visually* prove the multi-select path above.
- [x] Added `egui-gizmo` to the parent workspace `members` and wired the demo tab
      (3D cube; 2D space via the `2`/`3` viewport keys) — see P4-1 + the demo's
      `page_gizmo`. (2D image-transform sample still single-target.)

## DECISION (3-architect consensus, 2026-06 — unanimous): FORK
Depending on `transform-gizmo-egui` is a DEAD END for our goals, proven from the
source: the handle set is a `pub(crate)` `enum_dispatch` enum (`SubGizmo`,
`SubGizmoControl` private) so custom / shear / corner-pin / `Handle`-trait handles
CANNOT be added; `GizmoResult` is a closed enum and `Transform` is TRS-only, so
shear (affine) and corner-pin (projective homography) have nowhere to land; `mint`
leaks through every public type; `GizmoConfig` is `Copy` (no `Vec<Box<dyn Handle>>`).
So we **fork-vendor** the core into `src/` and own it.

### Plan (consensus)
- Vendor `_ref/gizmo/transform-gizmo/src/*` into `src/`; inline the ~110-line egui
  binding from `transform-gizmo-egui`. Diet deps to **egui + glam ONLY**: drop
  `mint` (internalise to glam f64), `ahash` (→ counter/std hasher), `enum_dispatch`
  (→ `Box<dyn Handle>`), `enumset` (→ bitflags/Vec), `ecolor/emath/epaint` (→
  `egui::{..., epaint::{Mesh,Tessellator}}`).
- **glam f32 PUBLIC, glam f64 INTERNAL.** Convert once at `interact` entry / result
  exit. Never thread f32 through the math (precision: inverse/scale_factor need f64).
- Replace closed `enum_dispatch SubGizmo` → open `pub trait Handle` +
  `Vec<Box<dyn Handle>>` owned by `Gizmo` (NOT in `GizmoConfig`, which stays POD-`Copy`).
- Generalise the RESULT before any 2D handle: `GizmoResult.kind` enum with room for
  `Shear`/`CornerPin([Vec2;4])` (only TRS wired in P0). Keep PER-TARGET
  `Vec<Transform>` output (multi-select correctness). Expose `is_hovered`/pick-preview
  so the host can suppress camera orbit when grabbing a handle.
- 2D for T/R/S is free: caller passes an ortho `view_proj` (proven by `playa_gizmo.rs`).
  Y-flip once in the projection.

### CRITICAL pitfalls (cost days if missed)
1. **Matrix transpose trap.** Upstream stores `mint::RowMatrix4<f64>` (row-major);
   `DMat4::from(mint::RowMatrix4)` TRANSPOSES. Our public glam `Mat4` is
   column-major — widen element-wise to `DMat4` with NO transpose, or you
   double-transpose. And `view_forward/up/right` read ROWS (`.z/.y/.x` of the
   row-major matrix) → on a glam column-major `DMat4` use `m.row(2/1/0).xyz()`,
   NOT `z_axis/y_axis/x_axis`. Unit-test against a known view matrix.
2. f64 internal, f32 only at the boundary (`screen_to_world` inverse, `scale_factor`).
3. epaint `Tessellator` is INTERNAL egui API — isolate every call in `paint.rs`,
   pin egui 0.34, so a future bump touches one file.
4. Manifest: glam `0.30`→`0.32`. Keep `rust-version = 1.88` (let-chains are 1.88-stable;
   upstream's declared 1.92 is conservative — the one let-chain compiles on 1.88).
5. License: retain MIT/Apache + Urho Laukkarinen copyright on vendored files
   (NOTICE / per-file header). Our new 2D/Handle code is BSD-3.
6. Replace `update_transforms_with_result(...)[0]` indexing with `.into_iter().next()`.
7. Multi-select rotation is overwritten upstream (`update_for_targets`) — flag for P4.

### P0 deliverable (this increment)
Forked crate (egui + glam only) COMPILES; a runnable `examples/` eframe window shows
a draggable 3D **translate/rotate/scale** gizmo over a cube, returning a delta
`Transform` via the final glam-f32 public facade. Internal `enum_dispatch` MAY stay
for P0 (it's internal; the public facade is forward-compatible) — the `Handle`-trait
refactor + 2D/shear/corner-pin handles are P1–P3, additive thanks to the general
result/facade shape.

## Open questions (decide early)
- Math lib: `glam` (rest of our stack-friendly) vs `mint`/`emath` (upstream uses a
  mix). Pick one public-facing type set; convert at the boundary.
- Rotation UX: trackball vs axis-angle vs screen-tangent for the screen ring.
- How much of egui's interaction (`Response`, `interact`) to reuse vs custom hit-test
  (custom is needed for depth-sorted 3D handles).

## 3D bounding box — `GizmoTool::Box3D` (added 2026-06-24)
Volumetric twin of the planar `Box`. New `subgizmo/bbox3d.rs` (`Bbox3DSubGizmo`):
**26 grips** = sign vector ∈ {-1,0,1}³\{0} → 6 faces (1 axis) + 12 edges (2) + 8
corners (3). Each scales the target about the OPPOSITE anchor along its non-zero
axes; emits the existing `GizmoResult::BoxScale{pivot, scale:DVec3}` (the 2D box
just pinned `scale.z=1`), so the result path is unchanged. Drag math: a single
per-handle drag subspace (`local_hit`), frozen at pick like `bbox.rs`, then a
per-axis ratio in the frozen local frame: a FACE (1 axis) drags along that world
axis line (`closest_on_line`), an EDGE (2 axes) on the plane those axes span
(normal = the unscaled axis), a CORNER (3 axes) on the frozen camera-facing plane
(`eye_to_model_dir`). One grip (the
+++ corner) draws the shared 12-edge wireframe. Half-extent mirrors
`bbox_half_extent.x` onto Z (cubic ±0.5 reference; real size rides the target
`scale`) — NO new config field. Wiring: `GizmoMode::BoundingBox3D` (config.rs,
+ `axes()`=XYZ + `kind()`=Scale), `GizmoTool::Box3D` (+ ALL + `to_inner`),
`gizmo.rs::add_bbox3d`. Builds clean; consumed by colmap-rs `colmap-viewer` crop
box. NOT yet eyes-on verified for feel.

## Status — P0 DONE (forked, on screen, verified)
The fork-vendor is complete and runs: `cargo run --example cube` shows a draggable
3D translate/rotate/scale gizmo over a cube, verified by screenshot (matrices align
end-to-end — both transpose pitfalls handled correctly: `widen()` is column-preserving;
`view_forward/up/right` read `.row(2/1/0)`). Deps are `egui` + `glam` publicly;
`enum_dispatch`/`enumset` kept INTERNAL (P1 removal with the `Handle`-trait). `mint`
gone. Public facade is glam-f32 (`Transform`/`GizmoConfig{view,projection,viewport,
modes,orientation,snapping,visuals}`/`GizmoModes`/`GizmoResult`/`Gizmo::{new,update_config,
interact,is_hovered}`). `lib.rs` = `src/egui_gizmo.rs`. Now a workspace member
(`_ref` excluded from the build); has a demo tab; consumed by playa.

### Changelog
- `Gizmo::draw` now draws ALL subgizmos every frame (was: only the active one mid-drag). The Box3D
  wireframe is emitted by a single handle (+++ corner); the old declutter hid the whole box while
  dragging any other grip. (colmap-viewer crop gizmo — "don't hide the gizmo during drag".)
- P0: fork-vendored transform-gizmo 0.9 core; mint→glam f64 internal; glam-f32 facade;
  `widen()` no-transpose + `view_*` row-reads; inlined egui mesh paint (isolated in
  `paint()`); `examples/cube.rs`.
- Default `GizmoVisuals::stroke_width` 4.0 → **1.5** (thinner lines, user request).
- example: Maya keys Q/W/E/R/T (select/move/rotate/scale/universal), X = Local/Global
  toggle, `-`/`=` = gizmo size; default orientation **Local** (gizmo sticks to the
  object — fixes the "diverges when rotated" Global behaviour). Note: Global scale of a
  rotated object is a shear (not TRS-representable) — a real upstream limitation, P2.
- example: orbit reads RAW `pointer.delta()` (not a `ui.interact` region) so the
  gizmo's hover-interact no longer eats the background drag.
- Configurability: facade `GizmoConfig` now takes `space: GizmoSpace` (3D/2D) +
  `features: GizmoFeatures` (granular mode/handle-type/per-axis toggles, replaces
  `GizmoModes`); `GizmoVisuals` gains `arrow_scale`/`plane_scale`/`ring_scale`/
  `scale_box_scale` (default 1.0) threaded into `subgizmo::common` geometry; the
  `cube` example gained an aligned left control panel (Grid sliders + checkboxes).
- Center handle + per-group alpha: `GizmoVisuals` gains `CenterShape` (Ring/Box),
  `center_color` (turquoise), `center_stroke_width` (2.0), and four group alphas
  (`translate/rotate/scale/center_alpha`, default 1.0). `draw_center()` in common.rs
  replaces the two View-plane `draw_circle` calls in translation.rs + scale.rs;
  `draw_arrow`/`draw_plane` gain a `group_alpha` param; rotation.rs multiplies ring
  color by `rotate_alpha`. Panel: Center section (Ring/Box toggle, stroke slider,
  colour button) + Opacity section (four sliders).
- Center defaults tuned: `center_color` = light blue `rgb(124,197,247)`; new
  `center_scale` (default `0.7` = 30% smaller than `inner_circle_radius`, applied in
  `draw_center`) exposed as a "size" slider in the panel's Center section. Example
  panel wrapped in a `ScrollArea` and the window defaults to 860×920 so every
  section stays reachable; feature checkboxes use native box-left `ui.checkbox`
  so they all left-align.
- P1 open-handle refactor: dropped `enum_dispatch`; `SubGizmo` is now
  `type SubGizmo = Box<dyn SubGizmoControl>` (subgizmo.rs). `SubGizmoControl` gained
  a `Debug` supertrait + `clone_box` so `Box<dyn>` stays `Clone`/`Debug` (the owning
  `Gizmo` derives both); construction stays `.into()` via a `From<SubGizmoConfig<T>>`
  shim. `gizmo.rs` unchanged except dropping the now-unused trait import. Cube
  example verified identical (no regression). NEXT: P2 (2D handles — shear, Nuke
  corner-pin, bbox) can now add handle kinds freely + a generalised `GizmoResult`.
- Center DEFAULTS changed (user request): `center_shape` = **Box**, `center_scale`
  **0.5**, `center_stroke_width` **1.4** (colour stays light blue `124,197,247`).
- Maya **MMB translate**: `GizmoInteraction` gains `mmb_pressed`/`mmb_down`/`shift`
  (facade reads `PointerButton::Middle` + `modifiers.shift`); plain MMB force-grabs
  the `View` handle (free screen move), Shift+MMB picks the enabled axis whose
  screen-projected dir is most parallel to the drag and locks it. Reuses the
  translation subgizmos via new `SubGizmoControl::{mmb_translate_dir,seed_drag_start}`
  (seed projects onto the axis line/plane so a far-cursor grab starts jump-free);
  mutually exclusive with the LMB drag. No-op when translate handles are disabled.
- **P2b step 1 — 2D bbox scale handles**: new `subgizmo/bbox.rs` (8 grips: 4 corners +
  4 edge mids on the local z=0 plane, pivot = opposite anchor; corners scale both
  axes, edges only the perpendicular). Emits inner `GizmoResult::BoxScale{pivot,scale}`
  → `T(piv)·S·T(-piv)·SRT(start)` arm in `update_transforms_with_result`. Gated by new
  inner `GizmoMode::BoundingBox` (2D-only via facade `GizmoFeatures::bbox`); 8 ids from
  the per-handle params hash. Config gains `bbox_half_extent: Vec2` (default 0.5),
  threaded facade→`config::GizmoConfig`. Aspect-lock = **Ctrl** (`GizmoInteraction.ctrl`
  → `PreparedGizmoConfig.aspect_lock`, set in `Gizmo::update` before subgizmos read it).
  NOTE: the drag math runs in a FIXED start-of-drag model frame stored in `BboxState`
  (live model feeds back as the pivot-scaled centre moves → would oscillate); pick uses
  a SCREEN-px tolerance `focus_distance/scale_factor` (the plan's raw `focus_distance`
  compare is world-units, dimensionally wrong). Demo `cube.rs`: `bbox (2D)` checkbox +
  `bbox_half_extent = 0.8` to match the quad. Shear / corner-pin / public result-gen = later steps.
- **P2b steps 2+3 — result-gen + 2D shear handles**: public `GizmoResult` drops `Eq`,
  gains `kind: GizmoResultKind { Transform, Shear{axis,amount} }` + `ShearAxis{X,Y}`
  (NON-TRS data channel; TRS callers keep reading only `new_transforms`). Public
  `GizmoMode::Shear` added (all matches are `matches!`/construction → stayed valid).
  Inner `GizmoResult::Shear{axis: ShearAxis2, amount: f64}` (`ShearAxis2{X,Y}` lives in
  `subgizmo/shear.rs`); its `update_transforms_with_result` arm returns the target
  **unchanged** (shear isn't TRS — the factor rides on the result). Facade `kind_of()`
  next to `mode_of()` narrows `ShearAxis2`→`ShearAxis` + f64→f32; `mode_of` maps
  `Shear→GizmoMode::Shear`. New `subgizmo/shear.rs` (template = bbox.rs): 2 grips —
  X at top-edge mid `(0,hy,0)`, Y at right-edge mid `(hx,0,0)`; FROZEN start-frame drag
  (`ShearState`), screen-px pick (`grab_px` dup'd from bbox), local-plane intersect.
  `amount` about the PIVOT (box centre): X = `(cur.x-start.x)/hy`, Y = `(cur.y-start.y)/hx`.
  Draw = screen-constant thin bar (wide for X, tall for Y) like the bbox grip. Gating:
  facade `GizmoFeatures::shear` (true in `all()`, false in `single()`, 2D-only in
  `to_inner`) → inner `config::GizmoMode::Shear` (kind=Scale, axes=X|Y) → `add_shear()`
  builder. Demo `cube.rs`: `shear (2D)` checkbox + `CubeApp::shear: Vec2`; reads
  `result.kind` (replace, amount is absolute-from-start); `draw_quad` applies
  `x'=x+sx·y; y'=y+sy·x` to local corners about the centre. `cargo build --example cube`
  + `cargo clippy --all-targets` = 0/0. Corner-pin = step 4 (TODO noted on `GizmoResultKind`).
- **P2b step 4 — 2D corner-pin handles (Nuke-style, PERSISTENT state)**: new
  `subgizmo/corner_pin.rs` — 4 independently-draggable corners (fixed order SW,SE,NE,NW;
  `CornerPinParams{corner:usize}`), template = bbox/shear. Outcome is a HOMOGRAPHY
  (not TRS): inner `GizmoResult::CornerPin{corners:[DVec2;4]}` → `update_transforms_with_result`
  arm returns the target **unchanged**; corners ride on public `GizmoResultKind::CornerPin
  {corners:[glam::Vec2;4]}` (frame space, +Y up). Public `GizmoMode::CornerPin`; `kind_of`
  narrows f64 world-XY→f32; `mode_of`→`CornerPin`; config `GizmoMode::CornerPin` (kind=**Translate**
  — corners are positional, axes=X|Y). **Persistent shared state** (the risky part): a
  homography is not a Transform, so `gizmo::Gizmo` OWNS `corner_pin: Option<[DVec3;4]>` +
  `PreparedGizmoConfig.corner_pin:[DVec3;4]` (every corner reads all four each frame).
  SEED (when `None`): `Gizmo::update` sets `config.corner_pin = self.corner_pin.unwrap_or_else(
  ||seed_corner_pin(&config))` right after `aspect_lock` (model matrix is current — `update_for_targets`
  ran); `seed_corner_pin` = local `(±hx,±hy,0)`→world via model matrix. PERSIST (after drag):
  on `GizmoResult::CornerPin` store `self.corner_pin = Some(corners→DVec3 z=0)` so the next frame
  re-feeds it (un-warped TRS would snap corners back). RESET: in `update_config`, where
  `modes_changed` clears subgizmos, also `self.corner_pin = None` (toggle/target switch re-seeds).
  COORD CHOICE: corners are WORLD positions in the gizmo z=0 plane → projected with
  `config.view_projection` (NOT `mvp` — mvp folds in the model, corners are already world);
  reported as world XY (`DVec2::new(c.x,c.y)`) which equals frame space in the demo's ortho-down-Z.
  Drag intersects the FROZEN start-of-drag z=0 plane (`CornerPinState{start_model}`); pick = screen
  dist to projected corner, tol = bbox `grab_px` (dup'd). Draw = quad outline polyline (one instance,
  corner 0) + per-corner screen-constant translucent grip. Gating: `GizmoFeatures::corner_pin`
  (true `all()`, false `single()`, 2D-only `to_inner`) → `add_corner_pin()` builder (4 instances).
  Demo `cube.rs`: `corner pin (2D)` checkbox (clears warp on toggle-off) + `CubeApp::corner_pin:
  Option<[Vec2;4]>` (None→3D reset); `draw_quad` draws the quad straight through the 4 corners
  (frame XY, projected with `view_proj` alone) when `Some`, else the TRS/shear path. `cargo build
  --example cube` + `cargo clippy --all-targets` = 0/0. **P2b COMPLETE** (bbox + shear + corner-pin).
- **Tool-mode refactor**: collapsed the `GizmoMode` mode-bools + `Tool2d` exclusive-2D enum into ONE
  public `GizmoTool` {Move,Rotate,Scale,Universal,Box,Shear,CornerPin} (`ALL`+`supports(space)`;
  only CornerPin is 2D-only). `GizmoConfig.tool` replaces `tool_2d`; `GizmoFeatures` is now pure
  *refinements* (axes/planes/screen/uniform/arcball + per-axis masks — mode bools dropped, `single()`
  gone). `to_inner(space,tool)` dispatches per-tool (Move/Rotate/Scale add only their group, Universal
  = union, Box/Shear add the single handle in BOTH spaces — 2D guard dropped now that `box_model`
  makes them valid in 3D). Shear handle now reports ABSOLUTE `base+drift` (`base=config.shear`) and
  draws/picks through `box_model()` so a re-grab continues + the grip tracks the sheared box. Demo
  `cube.rs`: Tool selector (enabled-guarded), Refinements group, hotkeys 2/3=space, Q/W=Move E=Rotate
  R=Scale T=Universal, Tab cycles supported tools (raw-event fallback if focus eats it); camera now
  ORBITS the selection centre (`look_at(target±dir, target)`). `cargo build --example cube` +
  `cargo clippy --all-targets` = 0/0 (rustc 1.96 Windows toolchain).
- **Tool-mode review fixes** (3 reviewers): (1) CRITICAL shear runaway — `shear.rs` read the
  growing `config.shear` LIVE each frame and added it to a from-start drift → double-count;
  fixed by capturing `start_base` ONCE at `pick`. (2) HIGH dead-state — selecting CornerPin then
  switching to 3D left an unsupported tool active (empty gizmo); `reset_2d_state` now clamps via
  `next_tool` when `!tool.supports(space)`. (3) `to_inner` early-returns empty for an unsupported
  tool (single source vs `supports`); added a `to_inner` mapping unit test. GOTCHAS to remember:
  **Universal is NOT a clean union** — `gizmo.rs::add_scale` de-dups planar/uniform SCALE handles
  against the translate-plane/view twins, so under Universal `planes`/`uniform_scale` refinements
  are no-ops (only the standalone `Scale` tool shows them). **Box in 3D is PLANAR** (the object's
  local z=0 box, drag-corners scales X/Y) — a true 8-corner 3D box is a future refinement.
- **Highlight fade (2026-07)** — the per-subgizmo focus highlight now EASES in/out instead of
  hard-switching. Added `focus_factor: f32` to `SubGizmoConfig` + `set_focus_factor`/`focus_factor`
  on the `SubGizmoControl` trait; the egui facade (`egui_gizmo.rs::interact`) drives it via
  `ctx.animate_bool_with_time` (0.12 s) through `Gizmo::set_focus_factors`, keeping the frame
  clock OUT of the glam core. `gizmo_color`/`draw_center`/`draw_arrow`/`draw_plane` AND every
  custom sub-gizmo (bbox/bbox3d/shear/corner-pin/arcball) now LERP inactive↔highlight colour+alpha
  by the factor. Endpoints are byte-identical (factor 0 == old !focused, 1 == old focused), so the
  static look is unchanged — only the transition is new.

### Notes for callers (answers to recurring questions)
- **2D?** TRS in 2D is FREE — pass an ORTHO `view`/`projection` (the hybrid hinge,
  proven by `playa_gizmo.rs`); translate/rotate/scale work. 2D-SPECIFIC handles
  (shear, Nuke corner-pin, bbox edge/corner) are **P2**, not yet implemented.
- **wgpu?** NO. The gizmo builds an `egui::Mesh` and paints it via `egui::Painter`
  — renderer-agnostic. It works on BOTH the glow and wgpu eframe backends and never
  touches wgpu itself. (The `examples/cube.rs` runs on eframe's default glow backend.)
- **Drag HUD ("P-HUD")**: while a handle is actively dragged the gizmo paints a
  heads-up overlay — gated `subgizmo.active && visuals.show_drag_hud` + a per-feature
  toggle (`hud_readout`/`dial_ticks`/`axis_guides`). Added a TEXT CHANNEL: `GizmoLabel`
  + `GizmoDrawData::labels` (pre-projected screen pos, NO index offset on concat);
  `paint()` now builds one `Painter`, adds the mesh, then draws labels, and early-returns
  only when BOTH mesh AND labels are empty (label-only frames paint). Label colour reuses
  the EXACT premultiplied-linear `Rgba::from_rgba_premultiplied` mesh conversion (a straight
  `Color32` would mis-tint). Each state struct stashes `last_cursor` in `update()` so the
  cursor-less `draw()` can anchor the readout. Geometry: rotate = dial tick ring (major/minor,
  snap-multiples emphasised) + degree labels (via `view_projection*rotation_matrix`) + sweep
  alpha routed through `hud_sweep_alpha`, readout `{:.1}°` from `current_delta`; move = WORLD
  axis guide (`gizmo_normal`, IDENTITY model through `view_projection`, Axis handles only) +
  view-facing start ghost + delta trail (`start_point`/`last_point` are WORLD even in Local —
  the inverse-rotation only re-expresses reported deltas), readout `{:.3}` = `current_delta.length()`;
  scale = SCREEN-space ruler (new `ShapeBuilder::line_segment_2d`; `ScaleState` only has the
  px `start_delta`, no world start point — do NOT touch the pick/update math), readout `{:.3}×`.
  New `GizmoVisuals` fields (public via re-export) default on-but-subtle grey. `cargo build
  --example cube` + `cargo clippy --all-targets` (src = 0 warnings; 2 PRE-EXISTING `Stroke::new`
  f32/f64 warnings in `cube.rs`'s own edge drawing, untouched) + `cargo test --lib` = pass.
