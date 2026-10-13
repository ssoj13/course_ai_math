// Vendored and adapted from `transform-gizmo` 0.9.0.
// Copyright (c) Urho Laukkarinen. Licensed under MIT OR Apache-2.0. See NOTICE.
// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.

use crate::math::{DVec3, Pos2, intersect_plane, ray_to_ray, round_to_interval};

use egui::Align2;

use crate::config::{GizmoDirection, GizmoMode, GizmoOrientation};
use crate::gizmo::{GizmoDrawData, GizmoResult, Ray};
use crate::shape::ShapeBuilder;
use crate::subgizmo::common::{
    draw_arrow, draw_center, draw_plane, gizmo_normal, hud_label, inner_circle_radius, pick_arrow,
    pick_circle, pick_plane, plane_bitangent, plane_global_origin, plane_tangent,
};
use crate::subgizmo::{SubGizmoConfig, SubGizmoKind, common::TransformKind};

use super::common::PickResult;

pub(crate) type TranslationSubGizmo = SubGizmoConfig<Translation>;

#[derive(Debug, Copy, Clone, Hash)]
pub(crate) struct TranslationParams {
    pub mode: GizmoMode,
    pub direction: GizmoDirection,
    pub transform_kind: TransformKind,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct TranslationState {
    start_view_dir: DVec3,
    start_point: DVec3,
    last_point: DVec3,
    current_delta: DVec3,
    /// Cursor screen position stashed each `update()`, so the cursor-less
    /// `draw()` can anchor the drag HUD readout next to the pointer.
    last_cursor: Pos2,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct Translation;

impl SubGizmoKind for Translation {
    type Params = TranslationParams;
    type State = TranslationState;
    type PickPreview = PickResult;

    fn pick_preview(subgizmo: &TranslationSubGizmo, ray: Ray) -> PickResult {
        match (subgizmo.transform_kind, subgizmo.direction) {
            (TransformKind::Plane, GizmoDirection::View) => pick_circle(
                &subgizmo.config,
                ray,
                inner_circle_radius(&subgizmo.config),
                true,
            ),
            (TransformKind::Plane, _) => pick_plane(&subgizmo.config, ray, subgizmo.direction),
            (TransformKind::Axis, _) => {
                pick_arrow(&subgizmo.config, ray, subgizmo.direction, subgizmo.mode)
            }
        }
    }

    fn pick(subgizmo: &mut TranslationSubGizmo, ray: Ray) -> Option<f64> {
        let pick_result = Self::pick_preview(subgizmo, ray);

        subgizmo.state.start_view_dir = subgizmo.config.view_forward();
        subgizmo.state.start_point = pick_result.subgizmo_point;
        subgizmo.state.last_point = pick_result.subgizmo_point;
        subgizmo.state.current_delta = DVec3::ZERO;

        if pick_result.picked {
            Some(pick_result.t)
        } else {
            None
        }
    }

    fn update(subgizmo: &mut TranslationSubGizmo, ray: Ray) -> Option<GizmoResult> {
        // Stash for the HUD (see `TranslationState::last_cursor`).
        subgizmo.state.last_cursor = ray.screen_pos;
        if subgizmo.config.view_forward() != subgizmo.state.start_view_dir {
            // If the view_forward direction has changed, i.e. camera has rotated,
            // refresh the subgizmo state by calling pick. Feels a bit hacky, but
            // fixes the issue where the target starts flying away if camera is rotated
            // while view plane translation is active.
            Self::pick(subgizmo, ray);
        }

        let mut new_point = if subgizmo.transform_kind == TransformKind::Axis {
            point_on_axis(subgizmo, ray)
        } else {
            point_on_plane(
                gizmo_normal(&subgizmo.config, subgizmo.direction),
                plane_global_origin(&subgizmo.config, subgizmo.direction),
                ray,
            )?
        };

        let mut new_delta = new_point - subgizmo.state.start_point;

        if subgizmo.config.snapping {
            new_delta = if subgizmo.transform_kind == TransformKind::Axis {
                snap_translation_vector(subgizmo, new_delta)
            } else {
                snap_translation_plane(subgizmo, new_delta)
            };
            new_point = subgizmo.state.start_point + new_delta;
        }

        let mut translation_delta = new_point - subgizmo.state.last_point;
        let mut total_translation = new_point - subgizmo.state.start_point;

        if subgizmo.config.orientation() == GizmoOrientation::Local {
            let inverse_rotation = subgizmo.config.rotation.inverse();
            translation_delta = inverse_rotation * translation_delta;
            total_translation = inverse_rotation * total_translation;
        }

        subgizmo.state.last_point = new_point;
        subgizmo.state.current_delta = new_delta;

        Some(GizmoResult::Translation {
            delta: translation_delta,
            total: total_translation,
        })
    }

    /// Maya MMB reuses two translation handle families: the screen-space `View`
    /// plane handle for the free move, and the per-axis arrows for the `Shift`
    /// axis lock. Plane handles (`TranslateXY` etc.) are NOT exposed to MMB.
    fn mmb_translate_dir(subgizmo: &TranslationSubGizmo) -> Option<GizmoDirection> {
        match (subgizmo.transform_kind, subgizmo.direction) {
            (TransformKind::Plane, GizmoDirection::View) => Some(GizmoDirection::View),
            (
                TransformKind::Axis,
                dir @ (GizmoDirection::X | GizmoDirection::Y | GizmoDirection::Z),
            ) => Some(dir),
            _ => None,
        }
    }

    /// Seed the drag-start state for a force-activation (Maya MMB). Unlike the
    /// normal `pick` path — which seeds `start_point` from the picked handle
    /// geometry and only stays jump-free because the cursor is ON the handle — MMB
    /// can grab a handle with the cursor anywhere. So we seed `start_point` /
    /// `last_point` with the SAME projection `update` uses (the axis line for an
    /// arrow, the handle plane for `View`), guaranteeing a zero first-frame delta.
    fn seed_drag_start(subgizmo: &mut TranslationSubGizmo, ray: Ray) {
        subgizmo.state.start_view_dir = subgizmo.config.view_forward();
        let start_point = if subgizmo.transform_kind == TransformKind::Axis {
            point_on_axis(subgizmo, ray)
        } else {
            point_on_plane(
                gizmo_normal(&subgizmo.config, subgizmo.direction),
                plane_global_origin(&subgizmo.config, subgizmo.direction),
                ray,
            )
            .unwrap_or(subgizmo.config.translation)
        };
        subgizmo.state.start_point = start_point;
        subgizmo.state.last_point = start_point;
        subgizmo.state.current_delta = DVec3::ZERO;
    }

    fn draw(subgizmo: &TranslationSubGizmo) -> GizmoDrawData {
        let config = subgizmo.config;
        let mut draw_data = match (subgizmo.transform_kind, subgizmo.direction) {
            (TransformKind::Axis, _) => draw_arrow(
                &config,
                subgizmo.focus_factor,
                subgizmo.direction,
                subgizmo.mode,
                config.visuals.translate_alpha,
            ),
            // The screen-center free-translate handle uses the configurable
            // center shape/colour rather than the per-axis palette.
            (TransformKind::Plane, GizmoDirection::View) => {
                draw_center(&config, subgizmo.focus_factor)
            }
            (TransformKind::Plane, _) => draw_plane(
                &config,
                subgizmo.focus_factor,
                subgizmo.direction,
                config.visuals.translate_alpha,
            ),
        };

        // --- Drag HUD: axis guide line + start ghost + delta trail -----------
        // All HUD geometry here is in WORLD space, so the ShapeBuilder uses
        // `view_projection` directly (IDENTITY model) — NOT `model_matrix`/`mvp`.
        // `start_point`/`last_point` are world positions even in Local
        // orientation (the inverse-rotation in `update` only re-expresses the
        // REPORTED deltas; the stored points stay world).
        let hud_on = subgizmo.active && config.visuals.show_drag_hud;
        if hud_on && config.visuals.axis_guides {
            let sb = ShapeBuilder::new(
                config.view_projection,
                config.viewport,
                config.pixels_per_point,
            );
            let guide_color = config.visuals.hud_guide_color;
            let tick_color = config.visuals.hud_tick_color;
            let thin = (config.visuals.stroke_width * 0.6).max(0.5);

            let start = subgizmo.state.start_point;
            let current = subgizmo.state.last_point;

            // Axis line across the viewport — only for single-axis handles
            // (plane / View handles span two axes, so a single line is wrong).
            if subgizmo.transform_kind == TransformKind::Axis {
                let axis = gizmo_normal(&config, subgizmo.direction);
                const BIG: f64 = 1.0e4;
                draw_data += sb
                    .line_segment(
                        config.translation - axis * BIG,
                        config.translation + axis * BIG,
                        (thin, guide_color),
                    )
                    .into();
            }

            // Start ghost: a small screen-facing cross at the drag origin. Built
            // from the camera's screen-right / screen-up vectors scaled to a
            // world length, so it stays a readable cross from any view (and in
            // 2D, where view_right/up are the screen axes).
            let ghost = (config.scale_factor as f64) * 6.0;
            let right = config.view_right() * ghost;
            let up = config.view_up() * ghost;
            draw_data += sb
                .line_segment(start - right, start + right, (thin, guide_color))
                .into();
            draw_data += sb
                .line_segment(start - up, start + up, (thin, guide_color))
                .into();

            // Delta trail from start to the current point.
            draw_data += sb.line_segment(start, current, (thin, tick_color)).into();

            // Numeric readout: world drag distance. `current_delta` already
            // reflects snapping (set from the snapped `new_delta` in `update`),
            // so this prints the snapped distance when snapping is on.
            if config.visuals.hud_readout {
                let cursor = subgizmo.state.last_cursor;
                let pos = crate::math::Pos2::new(cursor.x + 14.0, cursor.y - 14.0);
                draw_data.labels.push(hud_label(
                    pos,
                    format!("{:.3}", subgizmo.state.current_delta.length()),
                    config.visuals.hud_text_color,
                    config.visuals.hud_font_size,
                    Align2::LEFT_BOTTOM,
                ));
            }
        }

        draw_data
    }
}

/// Finds the nearest point on line that points in translation subgizmo direction
fn point_on_axis(subgizmo: &SubGizmoConfig<Translation>, ray: Ray) -> DVec3 {
    let origin = subgizmo.config.translation;
    let direction = gizmo_normal(&subgizmo.config, subgizmo.direction);

    let (_ray_t, subgizmo_t) = ray_to_ray(ray.origin, ray.direction, origin, direction);

    origin + direction * subgizmo_t
}

fn point_on_plane(plane_normal: DVec3, plane_origin: DVec3, ray: Ray) -> Option<DVec3> {
    let mut t = 0.0;
    if !intersect_plane(
        plane_normal,
        plane_origin,
        ray.origin,
        ray.direction,
        &mut t,
    ) {
        None
    } else {
        Some(ray.origin + ray.direction * t)
    }
}

fn snap_translation_vector(subgizmo: &SubGizmoConfig<Translation>, new_delta: DVec3) -> DVec3 {
    let delta_length = new_delta.length();
    if delta_length > 1e-5 {
        new_delta / delta_length
            * round_to_interval(delta_length, subgizmo.config.snap_distance as f64)
    } else {
        new_delta
    }
}

fn snap_translation_plane(subgizmo: &SubGizmoConfig<Translation>, new_delta: DVec3) -> DVec3 {
    let mut bitangent = plane_bitangent(subgizmo.direction);
    let mut tangent = plane_tangent(subgizmo.direction);
    if subgizmo.config.local_space() {
        bitangent = subgizmo.config.rotation * bitangent;
        tangent = subgizmo.config.rotation * tangent;
    }
    let cb = new_delta.cross(-bitangent);
    let ct = new_delta.cross(tangent);
    let lb = cb.length();
    let lt = ct.length();
    let n = gizmo_normal(&subgizmo.config, subgizmo.direction);

    if lb > 1e-5 && lt > 1e-5 {
        bitangent * round_to_interval(lt, subgizmo.config.snap_distance as f64) * (ct / lt).dot(n)
            + tangent
                * round_to_interval(lb, subgizmo.config.snap_distance as f64)
                * (cb / lb).dot(n)
    } else {
        new_delta
    }
}
