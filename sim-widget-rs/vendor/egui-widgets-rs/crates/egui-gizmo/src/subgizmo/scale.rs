// Vendored and adapted from `transform-gizmo` 0.9.0.
// Copyright (c) Urho Laukkarinen. Licensed under MIT OR Apache-2.0. See NOTICE.
// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.

use glam::DVec3;

use egui::Align2;

use crate::math::{Pos2, ray_to_ray, round_to_interval, world_to_screen};
use crate::shape::ShapeBuilder;

use crate::config::{GizmoDirection, GizmoMode};
use crate::gizmo::{GizmoDrawData, GizmoResult, Ray};
use crate::subgizmo::common::{
    draw_arrow, draw_center, draw_plane, gizmo_local_normal, gizmo_normal, hud_label,
    outer_circle_radius, pick_arrow, pick_circle, pick_plane, plane_bitangent, plane_tangent,
};
use crate::subgizmo::{SubGizmoConfig, SubGizmoKind, common::TransformKind};

use super::common::PickResult;

pub(crate) type ScaleSubGizmo = SubGizmoConfig<Scale>;

#[derive(Debug, Copy, Clone, Hash)]
pub(crate) struct ScaleParams {
    pub mode: GizmoMode,
    pub direction: GizmoDirection,
    pub transform_kind: TransformKind,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct ScaleState {
    /// Screen-space pixel distance from the projected gizmo origin at pick time.
    /// Divisor for the RADIAL factor, which is what the plane / view handles use.
    start_delta: f64,
    /// Signed distance ALONG THE HANDLE'S OWN AXIS, in world units, at pick time.
    /// Divisor for the axis factor. `None` when the axis solve is degenerate (see
    /// [`axis_scale_factor`]), which falls the handle back to the radial measure.
    start_axis_t: Option<f64>,
    /// The factor `update()` actually applied, stashed so the cursor-less (and
    /// ray-less) `draw()` HUD reports the SAME number rather than recomputing a
    /// radial one that no longer matches an axis drag.
    last_factor: f64,
    /// Cursor screen position stashed each `update()`, so the cursor-less
    /// `draw()` can anchor the drag HUD readout next to the pointer.
    last_cursor: Pos2,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct Scale;

impl SubGizmoKind for Scale {
    type Params = ScaleParams;
    type State = ScaleState;
    type PickPreview = PickResult;

    fn pick_preview(subgizmo: &SubGizmoConfig<Self>, ray: Ray) -> super::common::PickResult
    where
        Self: Sized,
    {
        match (subgizmo.transform_kind, subgizmo.direction) {
            (TransformKind::Plane, GizmoDirection::View) => pick_circle(
                &subgizmo.config,
                ray,
                outer_circle_radius(&subgizmo.config),
                false,
            ),
            (TransformKind::Plane, _) => pick_plane(&subgizmo.config, ray, subgizmo.direction),
            (TransformKind::Axis, _) => {
                pick_arrow(&subgizmo.config, ray, subgizmo.direction, subgizmo.mode)
            }
        }
    }

    fn pick(subgizmo: &mut ScaleSubGizmo, ray: Ray) -> Option<f64> {
        let pick_result = Self::pick_preview(subgizmo, ray);

        let start_delta = distance_from_origin_2d(subgizmo, ray.screen_pos)?;

        // Floor the start distance at 1px: it is the divisor for the scale factor
        // (update + the HUD readout). Picking with the cursor exactly on the
        // projected origin would otherwise give start_delta == 0 -> inf/NaN factor.
        subgizmo.state.start_delta = start_delta.max(1.0);

        // Where along the handle's OWN axis the grab landed. This is what makes an axis drag
        // measure travel ALONG the arrow instead of distance from the centre.
        subgizmo.state.start_axis_t = match subgizmo.transform_kind {
            TransformKind::Axis => axis_t(subgizmo, ray).filter(|t| t.abs() > 1e-6),
            TransformKind::Plane => None,
        };
        subgizmo.state.last_factor = 1.0;

        if pick_result.picked {
            Some(pick_result.t)
        } else {
            None
        }
    }

    fn update(subgizmo: &mut ScaleSubGizmo, ray: Ray) -> Option<GizmoResult> {
        // Stash for the HUD (see `ScaleState::last_cursor`).
        subgizmo.state.last_cursor = ray.screen_pos;

        // An AXIS handle measures travel along ITS OWN axis; a plane / view handle measures the
        // radial distance from the projected origin.
        //
        // These are not interchangeable, and conflating them is what made the arrows feel wrong:
        // with a radial measure, dragging an X arrow straight UP - perpendicular to X, where the
        // handle cannot possibly point - still grows the distance from the centre and so still
        // scaled X. Every direction scaled every axis it was pointing at, so the arrows behaved
        // like a single radial slider wearing three hats. Projecting onto the axis makes
        // perpendicular motion do nothing, which is what Maya / Blender / AE all do.
        let mut delta = match subgizmo.transform_kind {
            TransformKind::Axis => axis_scale_factor(subgizmo, ray),
            TransformKind::Plane => None,
        }
        // Radial fallback: plane / view handles always, and an axis whose solve is degenerate
        // (pointing at the viewer) - better a usable radial drag than a factor that jumps.
        .or_else(|| {
            Some(distance_from_origin_2d(subgizmo, ray.screen_pos)? / subgizmo.state.start_delta)
        })?;

        if subgizmo.config.snapping {
            delta = round_to_interval(delta, subgizmo.config.snap_scale as f64);
        }
        delta = delta.max(1e-4);
        subgizmo.state.last_factor = delta;
        delta -= 1.0;

        let direction = match (subgizmo.transform_kind, subgizmo.direction) {
            (TransformKind::Axis, _) => gizmo_local_normal(&subgizmo.config, subgizmo.direction),
            (TransformKind::Plane, GizmoDirection::View) => DVec3::ONE,
            (TransformKind::Plane, _) => (plane_bitangent(subgizmo.direction)
                + plane_tangent(subgizmo.direction))
            .normalize(),
        };

        let scale = DVec3::ONE + (direction * delta);

        Some(GizmoResult::Scale { total: scale })
    }

    fn draw(subgizmo: &ScaleSubGizmo) -> GizmoDrawData {
        let config = subgizmo.config;
        let mut draw_data = match (subgizmo.transform_kind, subgizmo.direction) {
            (TransformKind::Axis, _) => draw_arrow(
                &config,
                subgizmo.focus_factor,
                subgizmo.direction,
                subgizmo.mode,
                config.visuals.scale_alpha,
            ),
            // The screen-center uniform-scale handle uses the configurable
            // center shape/colour rather than the per-axis palette.
            (TransformKind::Plane, GizmoDirection::View) => {
                draw_center(&config, subgizmo.focus_factor)
            }
            (TransformKind::Plane, _) => draw_plane(
                &config,
                subgizmo.focus_factor,
                subgizmo.direction,
                config.visuals.scale_alpha,
            ),
        };

        // --- Drag HUD: screen-space scale ruler + factor readout -------------
        // `ScaleState` stores only `start_delta` (a SCREEN-space pixel distance
        // from the gizmo origin at pick time), so the ruler is reconstructed in
        // SCREEN space — we never add a world start point or touch the scale
        // pick/update math. The ruler runs origin -> current cursor; a start
        // tick marks `start_delta` along the same direction.
        let hud_on = subgizmo.active && config.visuals.show_drag_hud;
        if hud_on {
            // ShapeBuilder mvp is irrelevant for the 2D ruler calls, but we need
            // an instance for `line_segment_2d`; pass `mvp` (any valid matrix).
            let sb = ShapeBuilder::new(config.mvp, config.viewport, config.pixels_per_point);
            let guide_color = config.visuals.hud_guide_color;
            let tick_color = config.visuals.hud_tick_color;
            let thin = (config.visuals.stroke_width * 0.6).max(0.5);

            // Radial ruler, drawn only for the handles whose factor IS radial. On an axis handle
            // it would draw a line to the cursor while the scale follows the axis projection -
            // i.e. it would show a measurement the drag does not use. The readout below covers
            // both cases.
            let radial_ruler = subgizmo.transform_kind == TransformKind::Plane
                || subgizmo.state.start_axis_t.is_none();
            if radial_ruler
                && let Some(origin) = world_to_screen(config.viewport, config.mvp, DVec3::ZERO)
            {
                let cursor = subgizmo.state.last_cursor;
                let dir = (cursor - origin).normalized();
                // Guard against a zero-length direction (cursor on origin).
                if dir.length() > 0.5 {
                    let start_tick = origin + dir * subgizmo.state.start_delta as f32;
                    // Ruler line: origin -> current handle (cursor) screen pos.
                    draw_data += sb
                        .line_segment_2d(origin, cursor, (thin, guide_color))
                        .into();
                    // Start tick: a short perpendicular mark at `start_delta`.
                    let perp = egui::Vec2::new(-dir.y, dir.x) * 5.0;
                    draw_data += sb
                        .line_segment_2d(start_tick - perp, start_tick + perp, (thin, tick_color))
                        .into();
                }
            }

            // Factor readout near the cursor. `draw` has no ray, so it cannot redo the axis
            // projection - it reports the factor `update` actually applied.
            if config.visuals.hud_readout {
                let factor = subgizmo.state.last_factor;
                let cursor = subgizmo.state.last_cursor;
                let pos = Pos2::new(cursor.x + 14.0, cursor.y - 14.0);
                draw_data.labels.push(hud_label(
                    pos,
                    format!("{factor:.3}\u{00d7}"),
                    config.visuals.hud_text_color,
                    config.visuals.hud_font_size,
                    Align2::LEFT_BOTTOM,
                ));
            }
        }

        draw_data
    }
}

/// Signed distance from the gizmo origin to the cursor ray's closest approach ALONG this
/// handle's own axis, in world units.
///
/// Returns `None` when the axis points (nearly) straight at the viewer: the arrow projects to a
/// dot, the ray/axis solve is ill-conditioned, and the ratio would jump wildly. Callers fall back
/// to the radial measure there.
fn axis_t(subgizmo: &ScaleSubGizmo, ray: Ray) -> Option<f64> {
    let origin = subgizmo.config.translation;
    // World-space (orientation-aware) axis for the GEOMETRY. Note `update` separately uses
    // `gizmo_local_normal` for the RESULT, because a scale is applied in the object's own axes.
    let direction = gizmo_normal(&subgizmo.config, subgizmo.direction);

    if direction.dot(subgizmo.config.view_forward()).abs() > 0.999 {
        return None;
    }

    let (_ray_t, t) = ray_to_ray(ray.origin, ray.direction, origin, direction);
    t.is_finite().then_some(t)
}

/// Scale factor for an AXIS handle: how far along the axis the cursor is now, over how far it was
/// when the handle was grabbed. Perpendicular cursor motion leaves this unchanged.
///
/// `None` if the pick never established a usable axis position (degenerate axis, or a grab landing
/// on the origin itself, where the ratio has no meaning) - the caller then uses the radial measure.
fn axis_scale_factor(subgizmo: &ScaleSubGizmo, ray: Ray) -> Option<f64> {
    let start = subgizmo.state.start_axis_t?;
    Some(axis_t(subgizmo, ray)? / start)
}

fn distance_from_origin_2d<T: SubGizmoKind>(
    subgizmo: &SubGizmoConfig<T>,
    cursor_pos: Pos2,
) -> Option<f64> {
    let viewport = subgizmo.config.viewport;
    let gizmo_pos = world_to_screen(viewport, subgizmo.config.mvp, DVec3::new(0.0, 0.0, 0.0))?;

    Some(cursor_pos.distance(gizmo_pos) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The invariant the axis scale factor rests on, and the one the old radial measure broke:
    /// moving the cursor PERPENDICULAR to the handle's axis must not change the position along
    /// that axis - so it must not scale anything.
    ///
    /// Modelled on the orthographic 2D case playa uses: the view ray runs along -Z, the handle is
    /// the X axis through the origin.
    #[test]
    fn perpendicular_cursor_motion_does_not_move_along_the_axis() {
        let axis_origin = DVec3::ZERO;
        let axis_dir = DVec3::X;
        let ray_dir = DVec3::NEG_Z;

        let t_at = |x: f64, y: f64| {
            let (_, t) = ray_to_ray(DVec3::new(x, y, 1000.0), ray_dir, axis_origin, axis_dir);
            t
        };

        // Grab at x = 100, then drag straight UP by 400px. Position along X is unchanged...
        let start = t_at(100.0, 0.0);
        let after_vertical_drag = t_at(100.0, 400.0);
        assert!((start - 100.0).abs() < 1e-9);
        assert!(
            (after_vertical_drag - start).abs() < 1e-9,
            "vertical drag moved along X: {start} -> {after_vertical_drag}"
        );
        // ...so the factor is exactly 1: the layer does not resize.
        assert!((after_vertical_drag / start - 1.0).abs() < 1e-9);

        // The radial measure this replaced grew by |(100,400)| / 100 = 4.12x for the SAME drag.
        let radial_before = (100.0f64 * 100.0).sqrt();
        let radial_after = (100.0f64 * 100.0 + 400.0 * 400.0).sqrt();
        assert!(radial_after / radial_before > 4.0);

        // Dragging ALONG the axis still scales, proportionally.
        assert!((t_at(250.0, 0.0) / start - 2.5).abs() < 1e-9);
    }
}
