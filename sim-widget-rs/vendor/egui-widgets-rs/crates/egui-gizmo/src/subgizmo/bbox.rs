// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.
//
//! 2D bounding-box scale handles (P2b, step 1).
//!
//! Eight grips on a flat box living in the gizmo's LOCAL z=0 plane: four corners
//! and four edge mid-points. Dragging a grip scales the target about the OPPOSITE
//! anchor (corner → opposite corner; edge → opposite edge mid) — the familiar
//! Nuke / Maya bounding-box gesture. Corners scale both axes, edges only the axis
//! perpendicular to the edge. Holding the aspect-lock modifier (Ctrl, threaded in
//! via [`PreparedGizmoConfig::aspect_lock`]) forces a uniform factor.
//!
//! This step emits a TRS [`GizmoResult::BoxScale`] (scale about a world pivot);
//! shear / corner-pin / public result-gen are later steps.

use glam::{DMat4, DVec3};

use crate::config::PreparedGizmoConfig;
use crate::gizmo::{GizmoDrawData, GizmoResult, Ray};
use crate::math::{intersect_plane, world_to_screen};
use crate::shape::ShapeBuilder;
use crate::subgizmo::{SubGizmoConfig, SubGizmoKind};

use super::common::PickResult;

pub(crate) type BboxSubGizmo = SubGizmoConfig<Bbox>;

/// The eight bounding-box grips. Compass naming: N = +Y, E = +X (local plane).
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub(crate) enum BboxHandle {
    CornerNE,
    CornerNW,
    CornerSW,
    CornerSE,
    EdgeN,
    EdgeE,
    EdgeS,
    EdgeW,
}

impl BboxHandle {
    /// All eight handles, in a stable order (used by the builder).
    pub(crate) const ALL: [BboxHandle; 8] = [
        BboxHandle::CornerNE,
        BboxHandle::CornerNW,
        BboxHandle::CornerSW,
        BboxHandle::CornerSE,
        BboxHandle::EdgeN,
        BboxHandle::EdgeE,
        BboxHandle::EdgeS,
        BboxHandle::EdgeW,
    ];

    /// This grip's anchor in the gizmo LOCAL z=0 plane (half-extent `hx`/`hy`).
    fn anchor(self, hx: f64, hy: f64) -> DVec3 {
        match self {
            BboxHandle::CornerNE => DVec3::new(hx, hy, 0.0),
            BboxHandle::CornerNW => DVec3::new(-hx, hy, 0.0),
            BboxHandle::CornerSW => DVec3::new(-hx, -hy, 0.0),
            BboxHandle::CornerSE => DVec3::new(hx, -hy, 0.0),
            BboxHandle::EdgeN => DVec3::new(0.0, hy, 0.0),
            BboxHandle::EdgeE => DVec3::new(hx, 0.0, 0.0),
            BboxHandle::EdgeS => DVec3::new(0.0, -hy, 0.0),
            BboxHandle::EdgeW => DVec3::new(-hx, 0.0, 0.0),
        }
    }

    /// The pivot to scale about = the OPPOSITE anchor, i.e. the negated anchor.
    fn pivot(self, hx: f64, hy: f64) -> DVec3 {
        -self.anchor(hx, hy)
    }

    /// Which axes this grip scales: `(scale_x, scale_y)`. Corners scale both,
    /// edges scale only the axis perpendicular to the edge (the non-zero anchor
    /// coordinate).
    fn axes(self) -> (bool, bool) {
        match self {
            BboxHandle::CornerNE
            | BboxHandle::CornerNW
            | BboxHandle::CornerSW
            | BboxHandle::CornerSE => (true, true),
            BboxHandle::EdgeE | BboxHandle::EdgeW => (true, false),
            BboxHandle::EdgeN | BboxHandle::EdgeS => (false, true),
        }
    }
}

#[derive(Debug, Copy, Clone, Hash)]
pub(crate) struct BboxParams {
    pub handle: BboxHandle,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct BboxState {
    /// Gizmo model matrix captured at pick. The drag math runs entirely in THIS
    /// fixed frame: deriving `cur_local` from the LIVE model would feed the result
    /// back into the input (the gizmo centre moves as it scales about a pivot),
    /// producing oscillation. So we freeze the reference frame at drag start.
    start_model: DMat4,
    /// Cursor's ray∩local-plane hit at pick, in `start_model` LOCAL coords.
    start_local: DVec3,
    /// Opposite anchor projected to WORLD at pick — the pivot the result scales
    /// about. Fixed for the whole drag.
    pivot_world: DVec3,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct Bbox;

/// Local half-extent of the box, read from the (caller-configurable) config.
fn half_extent(config: &PreparedGizmoConfig) -> (f64, f64) {
    (
        config.bbox_half_extent.x as f64,
        config.bbox_half_extent.y as f64,
    )
}

/// Per-axis scale factor about `pivot` from `start`→`cur` (all in one local axis).
/// Returns `1.0` when the start lies on the pivot (degenerate, no leverage).
fn factor(cur: f64, start: f64, pivot: f64) -> f64 {
    let denom = start - pivot;
    if denom.abs() > 1e-6 {
        (cur - pivot) / denom
    } else {
        1.0
    }
}

/// Screen-pixel pick tolerance. `focus_distance` is in WORLD units
/// (`scale_factor * px`); dividing by `scale_factor` recovers the equivalent
/// screen tolerance, matching the arrow/plane handles' effective grab radius.
fn focus_px(config: &PreparedGizmoConfig) -> f32 {
    if config.scale_factor.abs() > 1e-6 {
        config.focus_distance / config.scale_factor
    } else {
        config.focus_distance
    }
}

/// Effective grab radius (screen px) for a grip: sized so clicking anywhere on the
/// visible square picks it (≈ its half-diagonal), but never below the default
/// handle grab radius. Keeps the pick zone matching the drawn square.
fn grab_px(config: &PreparedGizmoConfig) -> f32 {
    (config.visuals.bbox_handle_size * 0.7).max(focus_px(config))
}

impl SubGizmoKind for Bbox {
    type Params = BboxParams;
    type State = BboxState;
    type PickPreview = PickResult;

    fn pick_preview(subgizmo: &BboxSubGizmo, ray: Ray) -> PickResult {
        let config = subgizmo.config;
        let handle = subgizmo.handle;
        let (hx, hy) = half_extent(&config);

        // Anchor and plane come from the SHEARED box frame (`box_model`) so a
        // non-zero owned shear moves the grips onto the sheared box. Zero shear =>
        // `box_model == model_matrix` => identical to before.
        let box_model = config.box_model();
        let Some(anchor_screen) = world_to_screen(
            config.viewport,
            config.view_projection * box_model,
            handle.anchor(hx, hy),
        ) else {
            return PickResult {
                subgizmo_point: DVec3::ZERO,
                picked: false,
                t: 0.0,
            };
        };

        let normal = box_model.transform_vector3(DVec3::Z).normalize();
        let origin = box_model.transform_point3(DVec3::ZERO);
        let mut t = 0.0;
        let hit = if intersect_plane(normal, origin, ray.origin, ray.direction, &mut t) {
            ray.origin + ray.direction * t
        } else {
            DVec3::ZERO
        };

        let picked = ray.screen_pos.distance(anchor_screen) <= grab_px(&config);
        PickResult {
            subgizmo_point: hit,
            picked,
            t,
        }
    }

    fn pick(subgizmo: &mut BboxSubGizmo, ray: Ray) -> Option<f64> {
        let config = subgizmo.config;
        let handle = subgizmo.handle;
        let (hx, hy) = half_extent(&config);

        // Anchor/plane/pivot all derive from the SHEARED box frame so the grips
        // sit on the sheared box (zero shear => identical to the un-sheared frame).
        let box_model = config.box_model();
        let anchor_screen = world_to_screen(
            config.viewport,
            config.view_projection * box_model,
            handle.anchor(hx, hy),
        )?;

        // Local plane = sheared box z=0: normal is the box +Z in world, origin the
        // box origin. `point_on_plane`-style intersection (translation.rs).
        let normal = box_model.transform_vector3(DVec3::Z).normalize();
        let origin = box_model.transform_point3(DVec3::ZERO);
        let mut t = 0.0;
        if !intersect_plane(normal, origin, ray.origin, ray.direction, &mut t) {
            return None;
        }
        let hit_world = ray.origin + ray.direction * t;

        // Freeze the reference frame for the whole drag (see `BboxState`). Freezing
        // the SHEARED box frame keeps the scale math in the sheared box's local
        // coords so the grips stay anchored to the sheared edges during the drag.
        let model = box_model;
        subgizmo.state.start_model = model;
        subgizmo.state.start_local = model.inverse().transform_point3(hit_world);
        subgizmo.state.pivot_world = model.transform_point3(handle.pivot(hx, hy));

        if ray.screen_pos.distance(anchor_screen) <= grab_px(&config) {
            Some(t)
        } else {
            None
        }
    }

    fn update(subgizmo: &mut BboxSubGizmo, ray: Ray) -> Option<GizmoResult> {
        let state = subgizmo.state;
        let handle = subgizmo.handle;
        let (hx, hy) = half_extent(&subgizmo.config);
        let pivot_local = handle.pivot(hx, hy);

        // Intersect against the FIXED start-of-drag plane (see `BboxState`).
        let normal = state.start_model.transform_vector3(DVec3::Z).normalize();
        let origin = state.start_model.transform_point3(DVec3::ZERO);
        let mut t = 0.0;
        if !intersect_plane(normal, origin, ray.origin, ray.direction, &mut t) {
            return None;
        }
        let hit_world = ray.origin + ray.direction * t;
        let cur_local = state.start_model.inverse().transform_point3(hit_world);

        let (do_x, do_y) = handle.axes();
        let mut sx = if do_x {
            factor(cur_local.x, state.start_local.x, pivot_local.x)
        } else {
            1.0
        };
        let mut sy = if do_y {
            factor(cur_local.y, state.start_local.y, pivot_local.y)
        } else {
            1.0
        };

        // Aspect-lock (Ctrl): force a single magnitude on both axes, keeping each
        // axis' direction (so mirror-drags past the pivot still flip correctly).
        if subgizmo.config.aspect_lock {
            let s = sx.abs().max(sy.abs());
            sx = s.copysign(sx);
            sy = s.copysign(sy);
        }

        Some(GizmoResult::BoxScale {
            pivot: state.pivot_world,
            scale: DVec3::new(sx, sy, 1.0),
        })
    }

    fn draw(subgizmo: &BboxSubGizmo) -> GizmoDrawData {
        let config = subgizmo.config;
        let handle = subgizmo.handle;
        let (hx, hy) = half_extent(&config);

        // Neutral (screen) colour with the standard focus highlight, then the
        // shared `scale_alpha` group fade (this is a scale-family handle).
        let base = config.visuals.s_color;
        // Lerp colour + alpha toward the highlight by the eased focus factor (0 ==
        // inactive, 1 == highlighted), so the handle fades in/out like the axes.
        let color = match config.visuals.highlight_color {
            Some(hl) => base.lerp_to_gamma(hl, subgizmo.focus_factor),
            None => base,
        };
        let alpha = config.visuals.inactive_alpha
            + (config.visuals.highlight_alpha - config.visuals.inactive_alpha)
                * subgizmo.focus_factor;
        let color = color
            .linear_multiply(alpha)
            .gamma_multiply(config.visuals.scale_alpha);
        if color.a() == 0 {
            return GizmoDrawData::default();
        }

        // Draw in the SHEARED box frame (z=0 plane) so the outline + grips track
        // the sheared box; `box_model == model_matrix` when shear is zero.
        let box_model = config.box_model();
        let shape_builder = ShapeBuilder::new(
            config.view_projection * box_model,
            config.viewport,
            config.pixels_per_point,
        );

        let mut draw_data = GizmoDrawData::default();

        // Box outline drawn by ONE instance only (avoids 8x overdraw of the loop).
        if matches!(handle, BboxHandle::CornerNE) {
            let sw = DVec3::new(-hx, -hy, 0.0);
            let se = DVec3::new(hx, -hy, 0.0);
            let ne = DVec3::new(hx, hy, 0.0);
            let nw = DVec3::new(-hx, hy, 0.0);
            draw_data += shape_builder
                .polyline(&[sw, se, ne, nw, sw], (config.visuals.stroke_width, color))
                .into();
        }

        // Screen-constant grip: a small camera-facing square at the anchor's WORLD
        // position, sized in points (independent of the object's scale) so it always
        // matches the screen-space pick radius `grab_px`. Translucent fill + solid
        // outline reads as a hollow grab handle (Nuke/Maya style). Built with
        // `view_projection` (NOT * model) so the object's scale doesn't stretch it.
        let anchor_world = box_model.transform_point3(handle.anchor(hx, hy));
        let h = (config.scale_factor * config.visuals.bbox_handle_size * 0.5) as f64;
        let right = config.view_right() * h;
        let up = config.view_up() * h;
        let corner = |dx: f64, dy: f64| anchor_world + right * dx + up * dy;
        let screen_builder = ShapeBuilder::new(
            config.view_projection,
            config.viewport,
            config.pixels_per_point,
        );
        draw_data += screen_builder
            .polygon(
                &[
                    corner(-1.0, -1.0),
                    corner(1.0, -1.0),
                    corner(1.0, 1.0),
                    corner(-1.0, 1.0),
                ],
                color.gamma_multiply(0.35),
                (1.0, color),
            )
            .into();

        draw_data
    }
}
