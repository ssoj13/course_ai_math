// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.
//
//! 2D corner-pin handles (P2b, step 4).
//!
//! Nuke-style corner-pin: FOUR independently-draggable corners on the gizmo's flat
//! z=0 world plane. Dragging a corner moves only that corner; the outcome is the set
//! of four corner positions `[DVec2; 4]` that the CALLER feeds to a homography (the
//! gizmo does NOT compute or apply the homography — it only reports the corners).
//!
//! Unlike every other handle, the corner-pin's outcome is NOT a TRS `Transform`, so
//! the gizmo must OWN the corner positions across frames: they live in
//! [`crate::config::PreparedGizmoConfig::corner_pin`] (seeded once from the target
//! box, then re-fed from the gizmo's persistent store every frame — see
//! [`crate::gizmo::Gizmo`]). Each corner handle reads ALL FOUR from the config so it
//! can report the full updated set; only `self.corner`'s entry is moved.
//!
//! The four corners are WORLD-space positions, so they project with
//! `config.view_projection` (NOT `config.mvp`, which folds in the model matrix —
//! the corners are already in world space). Like the bbox / shear handles, the drag
//! intersects the FROZEN start-of-drag z=0 plane (see [`CornerPinState`]).

use glam::{DMat4, DVec2, DVec3};

use crate::config::PreparedGizmoConfig;
use crate::gizmo::{GizmoDrawData, GizmoResult, Ray};
use crate::math::{intersect_plane, world_to_screen};
use crate::shape::ShapeBuilder;
use crate::subgizmo::{SubGizmoConfig, SubGizmoKind};

use super::common::PickResult;

pub(crate) type CornerPinSubGizmo = SubGizmoConfig<CornerPin>;

/// Per-handle parameter: which of the four corners (0..4, fixed order SW, SE, NE,
/// NW — matching [`crate::gizmo::seed_corner_pin`]) this instance drives.
#[derive(Debug, Copy, Clone, Hash)]
pub(crate) struct CornerPinParams {
    pub corner: usize,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct CornerPinState {
    /// Gizmo model matrix captured at pick. Only used to freeze the z=0 drag plane
    /// for the whole gesture (origin + normal), so live gizmo motion can never feed
    /// back into the cursor→plane intersection (cf. `bbox::BboxState`).
    start_model: DMat4,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct CornerPin;

/// Screen-pixel pick tolerance — the world-unit `focus_distance` recovered to
/// screen px by dividing out `scale_factor` (duplicated from `bbox.rs`, kept local
/// so the handle modules stay independent).
fn focus_px(config: &PreparedGizmoConfig) -> f32 {
    if config.scale_factor.abs() > 1e-6 {
        config.focus_distance / config.scale_factor
    } else {
        config.focus_distance
    }
}

/// Effective grab radius (screen px) for a corner grip: same sizing as the bbox
/// grips so the pick zone matches the drawn square, never below the default handle
/// radius (reuse of the bbox `grab_px` logic).
fn grab_px(config: &PreparedGizmoConfig) -> f32 {
    (config.visuals.bbox_handle_size * 0.7).max(focus_px(config))
}

impl SubGizmoKind for CornerPin {
    type Params = CornerPinParams;
    type State = CornerPinState;
    type PickPreview = PickResult;

    fn pick_preview(subgizmo: &CornerPinSubGizmo, ray: Ray) -> PickResult {
        let config = subgizmo.config;
        let corner = config.corner_pin[subgizmo.corner];

        // Corners are WORLD positions → project with `view_projection`, NOT `mvp`.
        let Some(corner_screen) = world_to_screen(config.viewport, config.view_projection, corner)
        else {
            return PickResult {
                subgizmo_point: DVec3::ZERO,
                picked: false,
                t: 0.0,
            };
        };

        let normal = (config.rotation * DVec3::Z).normalize();
        let origin = config.translation;
        let mut t = 0.0;
        let hit = if intersect_plane(normal, origin, ray.origin, ray.direction, &mut t) {
            ray.origin + ray.direction * t
        } else {
            DVec3::ZERO
        };

        let picked = ray.screen_pos.distance(corner_screen) <= grab_px(&config);
        PickResult {
            subgizmo_point: hit,
            picked,
            t,
        }
    }

    fn pick(subgizmo: &mut CornerPinSubGizmo, ray: Ray) -> Option<f64> {
        let config = subgizmo.config;
        let corner = config.corner_pin[subgizmo.corner];

        let corner_screen = world_to_screen(config.viewport, config.view_projection, corner)?;

        // z=0 world plane: normal = local +Z in world, origin = gizmo translation
        // (same intersect as the bbox / shear handles).
        let normal = (config.rotation * DVec3::Z).normalize();
        let origin = config.translation;
        let mut t = 0.0;
        if !intersect_plane(normal, origin, ray.origin, ray.direction, &mut t) {
            return None;
        }

        // Freeze the reference frame for the whole drag (see `CornerPinState`).
        subgizmo.state.start_model = config.model_matrix;

        if ray.screen_pos.distance(corner_screen) <= grab_px(&config) {
            Some(t)
        } else {
            None
        }
    }

    fn update(subgizmo: &mut CornerPinSubGizmo, ray: Ray) -> Option<GizmoResult> {
        let state = subgizmo.state;
        let config = subgizmo.config;

        // Intersect against the FIXED start-of-drag z=0 plane (see `CornerPinState`).
        let normal = state.start_model.transform_vector3(DVec3::Z).normalize();
        let origin = state.start_model.transform_point3(DVec3::ZERO);
        let mut t = 0.0;
        if !intersect_plane(normal, origin, ray.origin, ray.direction, &mut t) {
            return None;
        }
        let hit_world = ray.origin + ray.direction * t;

        // Build the new corner set: only the grabbed corner moves to the cursor's
        // plane hit; the other three are carried unchanged from the config.
        let mut cs = config.corner_pin;
        cs[subgizmo.corner] = hit_world;

        // Report the corners as their world XY in the z=0 plane (the frame plane the
        // demo also draws in: ortho down -Z, identity-ish model). World Z is 0 here.
        let corners = [
            DVec2::new(cs[0].x, cs[0].y),
            DVec2::new(cs[1].x, cs[1].y),
            DVec2::new(cs[2].x, cs[2].y),
            DVec2::new(cs[3].x, cs[3].y),
        ];

        Some(GizmoResult::CornerPin { corners })
    }

    fn draw(subgizmo: &CornerPinSubGizmo) -> GizmoDrawData {
        let config = subgizmo.config;

        // Neutral (screen) colour with the standard focus highlight, then the shared
        // `scale_alpha` group fade (corner-pin shares the 2D-handle UX group).
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

        let mut draw_data = GizmoDrawData::default();

        // Outline polyline through the four corners (world → screen via
        // `view_projection`). Drawn by ONE instance only (the SW corner = index 0)
        // to avoid 4x overdraw of the shared quad outline.
        let cs = config.corner_pin;
        if subgizmo.corner == 0 {
            let outline_builder = ShapeBuilder::new(
                config.view_projection,
                config.viewport,
                config.pixels_per_point,
            );
            draw_data += outline_builder
                .polyline(
                    &[cs[0], cs[1], cs[2], cs[3], cs[0]],
                    (config.visuals.stroke_width, color),
                )
                .into();
        }

        // Screen-constant translucent grip square at THIS corner's WORLD position,
        // sized in points (independent of object scale) so it always matches the
        // pick radius `grab_px`. Built with `view_projection` (corners are world).
        let corner_world = cs[subgizmo.corner];
        let h = (config.scale_factor * config.visuals.bbox_handle_size * 0.5) as f64;
        let right = config.view_right() * h;
        let up = config.view_up() * h;
        let grip = |dx: f64, dy: f64| corner_world + right * dx + up * dy;
        let screen_builder = ShapeBuilder::new(
            config.view_projection,
            config.viewport,
            config.pixels_per_point,
        );
        draw_data += screen_builder
            .polygon(
                &[
                    grip(-1.0, -1.0),
                    grip(1.0, -1.0),
                    grip(1.0, 1.0),
                    grip(-1.0, 1.0),
                ],
                color.gamma_multiply(0.35),
                (1.0, color),
            )
            .into();

        draw_data
    }
}
