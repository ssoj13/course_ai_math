// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.
//
//! 2D shear handles (P2b, step 3).
//!
//! Two screen-grip bars on the gizmo's flat z=0 box (template: `bbox.rs`):
//! - the **X** grip sits at the TOP edge mid `(0, hy, 0)`; dragging it horizontally
//!   shears the box in X (`x' = x + amount * y`),
//! - the **Y** grip sits at the RIGHT edge mid `(hx, 0, 0)`; dragging it vertically
//!   shears the box in Y (`y' = y + amount * x`),
//!
//! both about the gizmo PIVOT (local origin = box centre). This is the Nuke/Maya 2D
//! shear gesture. Shear is an AFFINE that is NOT TRS-representable, so — unlike the
//! bbox handle — it does NOT modify the target transform: it emits
//! [`GizmoResult::Shear`] carrying the absolute shear factor (measured from the
//! frozen drag-start frame), and the facade forwards it on `GizmoResult::kind`. The
//! caller applies the shear to its own geometry.
//!
//! Like bbox, the drag math runs in a FROZEN start-of-drag model frame (see
//! [`ShearState`]) so the live gizmo motion can never feed back into the input.

use glam::{DMat4, DVec3};

use crate::config::PreparedGizmoConfig;
use crate::gizmo::{GizmoDrawData, GizmoResult, Ray};
use crate::math::{intersect_plane, world_to_screen};
use crate::shape::ShapeBuilder;
use crate::subgizmo::{SubGizmoConfig, SubGizmoKind};

use super::common::PickResult;

pub(crate) type ShearSubGizmo = SubGizmoConfig<Shear>;

/// Which local axis a shear grip drives. Crate-internal twin of the public
/// [`crate::ShearAxis`]; the facade narrows one to the other.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) enum ShearAxis2 {
    /// Horizontal shear from the top-edge grip (`x' = x + amount * y`).
    X,
    /// Vertical shear from the right-edge grip (`y' = y + amount * x`).
    Y,
}

impl ShearAxis2 {
    /// All shear grips, in a stable order (used by the builder).
    pub(crate) const ALL: [ShearAxis2; 2] = [ShearAxis2::X, ShearAxis2::Y];

    /// This grip's anchor in the gizmo LOCAL z=0 plane (half-extent `hx`/`hy`).
    /// X → top edge mid; Y → right edge mid.
    fn anchor(self, hx: f64, hy: f64) -> DVec3 {
        match self {
            ShearAxis2::X => DVec3::new(0.0, hy, 0.0),
            ShearAxis2::Y => DVec3::new(hx, 0.0, 0.0),
        }
    }
}

#[derive(Debug, Copy, Clone, Hash)]
pub(crate) struct ShearParams {
    pub axis: ShearAxis2,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct ShearState {
    /// Gizmo model matrix captured at pick. The drag math runs entirely in THIS
    /// fixed frame (see the module docs / `bbox::BboxState`): deriving `cur_local`
    /// from the LIVE model could feed the result back into the input.
    start_model: DMat4,
    /// Cursor's ray∩local-plane hit at pick, in `start_model` LOCAL coords.
    start_local: DVec3,
    /// The gizmo's owned shear on this axis CAPTURED AT PICK. The drag reports
    /// `start_base + drift` (drift measured from `start_local`). Capturing the base
    /// ONCE — instead of re-reading the live, every-frame-growing `config.shear` —
    /// is what makes a held-still cursor report a CONSTANT shear; reading it live
    /// double-counts the drift each frame and the shear runs away.
    start_base: f64,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct Shear;

/// Local half-extent of the box, read from the (caller-configurable) config.
/// Same source as the bbox handles so the two handle sets share one box.
fn half_extent(config: &PreparedGizmoConfig) -> (f64, f64) {
    (
        config.bbox_half_extent.x as f64,
        config.bbox_half_extent.y as f64,
    )
}

/// Screen-pixel pick tolerance — the world-unit `focus_distance` recovered to
/// screen px by dividing out `scale_factor` (duplicated from `bbox.rs`, kept local
/// so the two handle modules stay independent).
fn focus_px(config: &PreparedGizmoConfig) -> f32 {
    if config.scale_factor.abs() > 1e-6 {
        config.focus_distance / config.scale_factor
    } else {
        config.focus_distance
    }
}

/// Effective grab radius (screen px) for a grip: same sizing as the bbox grips so
/// the pick zone matches the drawn square, never below the default handle radius.
fn grab_px(config: &PreparedGizmoConfig) -> f32 {
    (config.visuals.bbox_handle_size * 0.7).max(focus_px(config))
}

impl SubGizmoKind for Shear {
    type Params = ShearParams;
    type State = ShearState;
    type PickPreview = PickResult;

    fn pick_preview(subgizmo: &ShearSubGizmo, ray: Ray) -> PickResult {
        let config = subgizmo.config;
        let axis = subgizmo.axis;
        let (hx, hy) = half_extent(&config);

        // Anchor + plane come from the SHEARED box frame so the grip sits on the
        // sheared edge (zero shear => `box_model == model_matrix`, identical).
        let box_model = config.box_model();
        let Some(anchor_screen) = world_to_screen(
            config.viewport,
            config.view_projection * box_model,
            axis.anchor(hx, hy),
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

    fn pick(subgizmo: &mut ShearSubGizmo, ray: Ray) -> Option<f64> {
        let config = subgizmo.config;
        let axis = subgizmo.axis;
        let (hx, hy) = half_extent(&config);

        // The pick TEST must match the DRAWN grip, which rides the sheared box
        // frame (`box_model`), so the anchor screen position is computed there too
        // (zero shear => `box_model == model_matrix`, identical to before).
        let anchor_screen = world_to_screen(
            config.viewport,
            config.view_projection * config.box_model(),
            axis.anchor(hx, hy),
        )?;

        // Local plane = gizmo z=0: normal is the local +Z in world, origin the
        // gizmo translation (same intersect as the bbox handle).
        let normal = (config.rotation * DVec3::Z).normalize();
        let origin = config.translation;
        let mut t = 0.0;
        if !intersect_plane(normal, origin, ray.origin, ray.direction, &mut t) {
            return None;
        }
        let hit_world = ray.origin + ray.direction * t;

        // Freeze the COMPUTE reference frame (the UN-sheared `model_matrix`) for
        // the whole drag (see `ShearState`); the drift is measured against this,
        // and the gizmo adds the owned `base` shear on top (see `update`).
        let model = config.model_matrix;
        subgizmo.state.start_model = model;
        subgizmo.state.start_local = model.inverse().transform_point3(hit_world);
        // Capture the owned base shear ONCE here (not live in `update`).
        subgizmo.state.start_base = match axis {
            ShearAxis2::X => config.shear.x as f64,
            ShearAxis2::Y => config.shear.y as f64,
        };

        if ray.screen_pos.distance(anchor_screen) <= grab_px(&config) {
            Some(t)
        } else {
            None
        }
    }

    fn update(subgizmo: &mut ShearSubGizmo, ray: Ray) -> Option<GizmoResult> {
        let state = subgizmo.state;
        let axis = subgizmo.axis;
        let (hx, hy) = half_extent(&subgizmo.config);

        // Intersect against the FIXED start-of-drag plane (see `ShearState`).
        let normal = state.start_model.transform_vector3(DVec3::Z).normalize();
        let origin = state.start_model.transform_point3(DVec3::ZERO);
        let mut t = 0.0;
        if !intersect_plane(normal, origin, ray.origin, ray.direction, &mut t) {
            return None;
        }
        let hit_world = ray.origin + ray.direction * t;
        let cur_local = state.start_model.inverse().transform_point3(hit_world);

        // Shear factor about the gizmo PIVOT (box centre = local origin). The grip
        // drifts over half the box extent, so normalise the drift by that extent to
        // get a unit-less shear factor:
        //   X grip (top edge): the top drifts in x over the half-HEIGHT hy,
        //   Y grip (right edge): the right drifts in y over the half-WIDTH  hx.
        // Guard against a degenerate (zero) extent to avoid a div-by-zero NaN.
        //
        // The reported amount is ABSOLUTE: `start_base + drift`. `start_base` is the
        // gizmo's owned shear CAPTURED ONCE AT PICK (NOT the live, every-frame-growing
        // `config.shear`). The COMPUTE plane is the frozen UN-sheared `start_model`,
        // so the drift is measured in the un-sheared local frame and added to the
        // captured base: a held-still cursor stays constant, and a re-grab continues
        // from the current shear instead of snapping to drift-from-zero. (Reading the
        // base live here would double-count the drift each frame → runaway shear.)
        let base = state.start_base;
        let drift = match axis {
            ShearAxis2::X => {
                if hy.abs() > 1e-9 {
                    (cur_local.x - state.start_local.x) / hy
                } else {
                    0.0
                }
            }
            ShearAxis2::Y => {
                if hx.abs() > 1e-9 {
                    (cur_local.y - state.start_local.y) / hx
                } else {
                    0.0
                }
            }
        };
        let amount = base + drift;

        Some(GizmoResult::Shear { axis, amount })
    }

    fn draw(subgizmo: &ShearSubGizmo) -> GizmoDrawData {
        let config = subgizmo.config;
        let axis = subgizmo.axis;
        let (hx, hy) = half_extent(&config);

        // Neutral (screen) colour with the standard focus highlight, then the
        // shared `scale_alpha` group fade (shear is a scale-family handle).
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

        // Screen-constant grip: a small camera-facing square at the anchor's WORLD
        // position, sized in points (independent of the object's scale) so it always
        // matches the screen-space pick radius `grab_px`. Same construction as the
        // bbox grip (translucent fill + solid outline = a hollow grab handle), built
        // with `view_projection` (NOT * model) so the object's scale doesn't stretch
        // it. The anchor rides the SHEARED box frame (`box_model`) so the grip
        // tracks the sheared edge (zero shear => `box_model == model_matrix`).
        let anchor_world = config.box_model().transform_point3(axis.anchor(hx, hy));
        let h = (config.scale_factor * config.visuals.bbox_handle_size * 0.5) as f64;
        let right = config.view_right() * h;
        let up = config.view_up() * h;
        let corner = |dx: f64, dy: f64| anchor_world + right * dx + up * dy;
        let screen_builder = ShapeBuilder::new(
            config.view_projection,
            config.viewport,
            config.pixels_per_point,
        );

        // A thin bar along the grip's slide direction so the handle reads as the
        // axis it shears: X grips slide horizontally (wide bar), Y grips slide
        // vertically (tall bar).
        let (wx, wy) = match axis {
            ShearAxis2::X => (1.0, 0.35),
            ShearAxis2::Y => (0.35, 1.0),
        };
        let mut draw_data = GizmoDrawData::default();
        draw_data += screen_builder
            .polygon(
                &[
                    corner(-wx, -wy),
                    corner(wx, -wy),
                    corner(wx, wy),
                    corner(-wx, wy),
                ],
                color.gamma_multiply(0.35),
                (1.0, color),
            )
            .into();

        draw_data
    }
}
