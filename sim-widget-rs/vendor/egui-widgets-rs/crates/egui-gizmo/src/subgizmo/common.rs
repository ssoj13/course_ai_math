// Vendored and adapted from `transform-gizmo` 0.9.0.
// Copyright (c) Urho Laukkarinen. Licensed under MIT OR Apache-2.0. See NOTICE.
// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.
//
//! Shared subgizmo geometry: arrow/plane/circle picking and drawing helpers.

use crate::config::{CenterShape, GizmoDirection, GizmoMode, PreparedGizmoConfig};
use crate::gizmo::{GizmoDrawData, GizmoLabel, Ray};
use crate::math::{Pos2, ray_to_plane_origin, segment_to_segment};
use crate::shape::ShapeBuilder;
use egui::{Align2, Color32};
use enumset::EnumSet;
use glam::{DMat3, DMat4, DQuat, DVec3};
use std::ops::{Add, RangeInclusive};

use super::Picked;

const ARROW_FADE: RangeInclusive<f64> = 0.95..=0.99;
const PLANE_FADE: RangeInclusive<f64> = 0.70..=0.86;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) enum TransformKind {
    Axis,
    Plane,
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct PickResult {
    pub subgizmo_point: DVec3,
    pub picked: bool,
    pub t: f64,
}

impl Picked for PickResult {
    fn picked(&self) -> bool {
        self.picked
    }
}

struct ArrowParams {
    start: DVec3,
    end: DVec3,
    direction: DVec3,
    length: f64,
}

fn arrow_modes_overlapping(mode: GizmoMode, other_modes: EnumSet<GizmoMode>) -> bool {
    (mode == GizmoMode::TranslateX && other_modes.contains(GizmoMode::ScaleX))
        || (mode == GizmoMode::TranslateY && other_modes.contains(GizmoMode::ScaleY))
        || (mode == GizmoMode::TranslateZ && other_modes.contains(GizmoMode::ScaleZ))
        || (mode == GizmoMode::ScaleX && other_modes.contains(GizmoMode::TranslateX))
        || (mode == GizmoMode::ScaleY && other_modes.contains(GizmoMode::TranslateY))
        || (mode == GizmoMode::ScaleZ && other_modes.contains(GizmoMode::TranslateZ))
}

fn arrow_params(config: &PreparedGizmoConfig, direction: DVec3, mode: GizmoMode) -> ArrowParams {
    let width = (config.scale_factor * config.visuals.stroke_width) as f64;

    // Per-element length multiplier: translate axes use `arrow_scale`, scale
    // axes use `scale_box_scale`. Default 1.0 leaves the geometry unchanged.
    let m = (if mode.is_translate() {
        config.visuals.arrow_scale
    } else {
        config.visuals.scale_box_scale
    }) as f64;

    let (start, length) = if mode.is_translate() && arrow_modes_overlapping(mode, config.modes) {
        // Modes contain both translate and scale. Use a bit different translate arrow, so the modes do not overlap.
        let length = (config.scale_factor * config.visuals.gizmo_size) as f64 * m;
        let start = direction * (length + (width * 3.0));

        let length = length * 0.2 + width;

        (start, length)
    } else {
        let start = direction * (width * 0.5 + inner_circle_radius(config));
        let mut length =
            (config.scale_factor * config.visuals.gizmo_size) as f64 * m - start.length();

        if config.modes.len() > 1 {
            length -= width * 2.0;
        }

        (start, length)
    };

    ArrowParams {
        start,
        end: start + direction * length,
        direction,
        length,
    }
}

fn arrow_visibility(config: &PreparedGizmoConfig, direction: DVec3) -> f64 {
    let dot = config.eye_to_model_dir.dot(direction).abs();
    (1.0 - (dot - *ARROW_FADE.start()) / (*ARROW_FADE.end() - *ARROW_FADE.start())).min(1.0)
}

pub(crate) fn pick_arrow(
    config: &PreparedGizmoConfig,
    ray: Ray,
    direction: GizmoDirection,
    mode: GizmoMode,
) -> PickResult {
    let ray_length = 1e+14;

    let direction = gizmo_normal(config, direction);

    let mut arrow_params = arrow_params(config, direction, mode);
    arrow_params.start += config.translation;
    arrow_params.end += config.translation;

    let (ray_t, subgizmo_t) = segment_to_segment(
        ray.origin,
        ray.origin + ray.direction * ray_length,
        arrow_params.start,
        arrow_params.end,
    );

    let ray_point = ray.origin + ray.direction * ray_length * ray_t;
    let subgizmo_point =
        arrow_params.start + arrow_params.direction * arrow_params.length * subgizmo_t;
    let dist = (ray_point - subgizmo_point).length();

    let visibility = arrow_visibility(config, direction);

    let picked = visibility > 0.0 && dist <= config.focus_distance as f64;

    PickResult {
        subgizmo_point,
        picked,
        t: ray_t,
    }
}

pub(crate) fn pick_plane(
    config: &PreparedGizmoConfig,
    ray: Ray,
    direction: GizmoDirection,
) -> PickResult {
    let origin = plane_global_origin(config, direction);

    let normal = gizmo_normal(config, direction);

    let (t, dist_from_origin) = ray_to_plane_origin(normal, origin, ray.origin, ray.direction);

    let ray_point = ray.origin + ray.direction * t;

    let visibility = plane_visibility(config, direction);

    let picked = visibility > 0.0 && dist_from_origin <= plane_size(config);

    PickResult {
        subgizmo_point: ray_point,
        picked,
        t,
    }
}

pub(crate) fn pick_circle(
    config: &PreparedGizmoConfig,
    ray: Ray,
    radius: f64,
    filled: bool,
) -> PickResult {
    let origin = config.translation;
    let normal = -config.view_forward();

    let (t, dist_from_gizmo_origin) =
        ray_to_plane_origin(normal, origin, ray.origin, ray.direction);

    let hit_pos = ray.origin + ray.direction * t;

    let picked = if filled {
        dist_from_gizmo_origin <= radius + config.focus_distance as f64
    } else {
        (dist_from_gizmo_origin - radius).abs() <= config.focus_distance as f64
    };

    PickResult {
        subgizmo_point: hit_pos,
        picked,
        t,
    }
}

pub(crate) fn draw_arrow(
    config: &PreparedGizmoConfig,
    focus: f32,
    direction: GizmoDirection,
    mode: GizmoMode,
    group_alpha: f32, // per-group transparency (translate_alpha / scale_alpha)
) -> GizmoDrawData {
    let opacity = arrow_visibility(config, gizmo_normal(config, direction));

    if opacity <= 1e-4 {
        return GizmoDrawData::default();
    }

    // Apply view-angle fade then the caller's per-group alpha.
    let color = gizmo_color(config, focus, direction)
        .gamma_multiply(opacity as f32)
        .gamma_multiply(group_alpha);

    let transform = if config.local_space() {
        DMat4::from_rotation_translation(config.rotation, config.translation)
    } else {
        DMat4::from_translation(config.translation)
    };

    let shape_builder = ShapeBuilder::new(
        config.view_projection * transform,
        config.viewport,
        config.pixels_per_point,
    );

    let direction = gizmo_local_normal(config, direction);

    let arrow_params = arrow_params(config, direction, mode);

    let tip_stroke_width = 2.4 * config.visuals.stroke_width;
    let tip_length = (tip_stroke_width * config.scale_factor) as f64;

    let tip_start = arrow_params.end - arrow_params.direction * tip_length;

    let mut draw_data = GizmoDrawData::default();
    draw_data = draw_data.add(
        shape_builder
            .line_segment(
                arrow_params.start,
                tip_start,
                (config.visuals.stroke_width, color),
            )
            .into(),
    );

    if mode.is_scale() {
        draw_data = draw_data.add(
            shape_builder
                .line_segment(tip_start, arrow_params.end, (tip_stroke_width, color))
                .into(),
        );
    } else if mode.is_translate() {
        draw_data = draw_data.add(
            shape_builder
                .arrow(tip_start, arrow_params.end, (tip_stroke_width, color))
                .into(),
        );
    }

    draw_data
}

pub(crate) fn draw_plane(
    config: &PreparedGizmoConfig,
    focus: f32,
    direction: GizmoDirection,
    group_alpha: f32, // per-group transparency (translate_alpha / scale_alpha)
) -> GizmoDrawData {
    let opacity = plane_visibility(config, direction);

    if opacity <= 1e-4 {
        return GizmoDrawData::default();
    }

    // Apply camera-facing fade then the caller's per-group alpha.
    let color = gizmo_color(config, focus, direction)
        .gamma_multiply(opacity as f32)
        .gamma_multiply(group_alpha);

    let transform = if config.local_space() {
        DMat4::from_rotation_translation(config.rotation, config.translation)
    } else {
        DMat4::from_translation(config.translation)
    };

    let shape_builder = ShapeBuilder::new(
        config.view_projection * transform,
        config.viewport,
        config.pixels_per_point,
    );

    let scale = plane_size(config) * 0.5;
    let a = plane_bitangent(direction) * scale;
    let b = plane_tangent(direction) * scale;
    let origin = plane_local_origin(config, direction);

    let mut draw_data = GizmoDrawData::default();
    draw_data = draw_data.add(
        shape_builder
            .polygon(
                &[
                    origin - b - a,
                    origin + b - a,
                    origin + b + a,
                    origin - b + a,
                ],
                color,
                (0.0, Color32::TRANSPARENT),
            )
            .into(),
    );
    draw_data
}

pub(crate) fn draw_circle(
    config: &PreparedGizmoConfig,
    color: Color32,
    radius: f64,
    filled: bool,
) -> GizmoDrawData {
    if color.a() == 0 {
        return GizmoDrawData::default();
    }

    let rotation = {
        let forward = config.view_forward();
        let right = config.view_right();
        let up = config.view_up();

        DQuat::from_mat3(&DMat3::from_cols(up, -forward, -right))
    };

    let transform = DMat4::from_rotation_translation(rotation, config.translation);

    let shape_builder = ShapeBuilder::new(
        config.view_projection * transform,
        config.viewport,
        config.pixels_per_point,
    );

    let mut draw_data = GizmoDrawData::default();
    if filled {
        draw_data = draw_data.add(
            shape_builder
                .filled_circle(radius, color, (0.0, Color32::TRANSPARENT))
                .into(),
        );
    } else {
        draw_data = draw_data.add(
            shape_builder
                .circle(radius, (config.visuals.stroke_width, color))
                .into(),
        );
    }
    draw_data
}

/// Draw the configurable screen-center handle (Ring or Box) for translate/scale view handles.
///
/// Uses `center_color`, `center_stroke_width`, and `center_shape` from visuals so
/// the center can be styled independently of the per-axis handles. `center_alpha`
/// (part of the per-group transparency system) is applied here.
///
/// LEAVE arcball's `draw_circle` alone — arcball is a trackball, not a center handle.
pub(crate) fn draw_center(config: &PreparedGizmoConfig, focus: f32) -> GizmoDrawData {
    // `center_scale` lets the center handle be sized independently of the axes.
    let radius = inner_circle_radius(config) * config.visuals.center_scale as f64;

    // Center handle has its own colour, bypassing the per-axis palette. Lerp toward
    // the highlight colour (when set) + the highlight alpha by the eased focus
    // factor so the handle fades in/out (focus 0 == inactive, 1 == highlighted).
    let base_color = config.visuals.center_color;
    let color = match config.visuals.highlight_color {
        Some(hl) => base_color.lerp_to_gamma(hl, focus),
        None => base_color,
    };
    let alpha = config.visuals.inactive_alpha
        + (config.visuals.highlight_alpha - config.visuals.inactive_alpha) * focus;
    // focused/inactive alpha first, then center_alpha for the per-group fade.
    let color = color
        .linear_multiply(alpha)
        .gamma_multiply(config.visuals.center_alpha);

    if color.a() == 0 {
        return GizmoDrawData::default();
    }

    // Screen-facing rotation: same as draw_circle (local Y = -view_forward,
    // local X = view_up, local Z = -view_right) so the XZ plane = view plane.
    let rotation = {
        let forward = config.view_forward();
        let right = config.view_right();
        let up = config.view_up();
        DQuat::from_mat3(&DMat3::from_cols(up, -forward, -right))
    };
    let transform = DMat4::from_rotation_translation(rotation, config.translation);
    let shape_builder = ShapeBuilder::new(
        config.view_projection * transform,
        config.viewport,
        config.pixels_per_point,
    );

    let stroke_px = config.visuals.center_stroke_width;
    let mut draw_data = GizmoDrawData::default();

    match config.visuals.center_shape {
        CenterShape::Ring => {
            draw_data = draw_data.add(shape_builder.circle(radius, (stroke_px, color)).into());
        }
        CenterShape::Box => {
            // Square outline in the view plane. Corners at (±r, 0, ±r) in local
            // space map to a screen-aligned square (local Y = -view_forward).
            let r = radius;
            draw_data = draw_data.add(
                shape_builder
                    .polygon(
                        &[
                            DVec3::new(-r, 0.0, -r),
                            DVec3::new(r, 0.0, -r),
                            DVec3::new(r, 0.0, r),
                            DVec3::new(-r, 0.0, r),
                        ],
                        Color32::TRANSPARENT,
                        (stroke_px, color),
                    )
                    .into(),
            );
        }
    }

    draw_data
}

pub(crate) const fn plane_bitangent(direction: GizmoDirection) -> DVec3 {
    match direction {
        GizmoDirection::X => DVec3::Y,
        GizmoDirection::Y => DVec3::Z,
        GizmoDirection::Z => DVec3::X,
        GizmoDirection::View => DVec3::ZERO, // Unused
    }
}

pub(crate) const fn plane_tangent(direction: GizmoDirection) -> DVec3 {
    match direction {
        GizmoDirection::X => DVec3::Z,
        GizmoDirection::Y => DVec3::X,
        GizmoDirection::Z => DVec3::Y,
        GizmoDirection::View => DVec3::ZERO, // Unused
    }
}

pub(crate) fn plane_size(config: &PreparedGizmoConfig) -> f64 {
    // `plane_scale` tunes the planar-handle size; only the `gizmo_size`-derived
    // term is scaled so the stroke-width padding stays constant.
    (config.scale_factor
        * (config.visuals.gizmo_size * 0.1 * config.visuals.plane_scale
            + config.visuals.stroke_width * 2.0)) as f64
}

pub(crate) fn plane_local_origin(config: &PreparedGizmoConfig, direction: GizmoDirection) -> DVec3 {
    // Scale the planar-handle offset together with its size (`plane_scale`) so
    // the quad stays clear of the axes when enlarged.
    let offset = config.scale_factor * config.visuals.gizmo_size * 0.5 * config.visuals.plane_scale;

    let a = plane_bitangent(direction);
    let b = plane_tangent(direction);
    (a + b) * offset as f64
}

pub(crate) fn plane_global_origin(
    config: &PreparedGizmoConfig,
    direction: GizmoDirection,
) -> DVec3 {
    let mut origin = plane_local_origin(config, direction);
    if config.local_space() {
        origin = config.rotation * origin;
    }
    origin + config.translation
}

pub(crate) fn plane_visibility(config: &PreparedGizmoConfig, direction: GizmoDirection) -> f64 {
    let dot = config
        .eye_to_model_dir
        .dot(gizmo_normal(config, direction))
        .abs();
    (1.0 - ((1.0 - dot) - *PLANE_FADE.start()) / (*PLANE_FADE.end() - *PLANE_FADE.start())).min(1.0)
}

/// Radius to use for inner circle subgizmos
pub(crate) fn inner_circle_radius(config: &PreparedGizmoConfig) -> f64 {
    // `ring_scale` tunes the rotation-ring radius (inner ring).
    (config.scale_factor * config.visuals.gizmo_size * config.visuals.ring_scale) as f64 * 0.2
}

/// Radius to use for outer circle subgizmos
pub(crate) fn outer_circle_radius(config: &PreparedGizmoConfig) -> f64 {
    // `ring_scale` tunes the rotation-ring radius; only the `gizmo_size` term is
    // scaled so the stroke-width padding stays constant.
    (config.scale_factor
        * (config.visuals.gizmo_size * config.visuals.ring_scale
            + config.visuals.stroke_width
            + 5.0)) as f64
}

/// Build a single drag-HUD text label (the gizmo text channel). Centralises the
/// `Color32` -> linear premultiplied RGBA conversion so every subgizmo emits HUD
/// text the same way the painter expects (see `egui_gizmo::paint`).
pub(crate) fn hud_label(
    screen: Pos2,
    text: String,
    color: Color32,
    size: f32,
    anchor: Align2,
) -> GizmoLabel {
    GizmoLabel {
        pos: [screen.x, screen.y],
        text,
        color: egui::Rgba::from(color).to_array(),
        size,
        anchor,
    }
}

pub(crate) fn gizmo_local_normal(config: &PreparedGizmoConfig, direction: GizmoDirection) -> DVec3 {
    match direction {
        GizmoDirection::X => DVec3::X,
        GizmoDirection::Y => DVec3::Y,
        GizmoDirection::Z => DVec3::Z,
        GizmoDirection::View => -config.view_forward(),
    }
}

pub(crate) fn gizmo_normal(config: &PreparedGizmoConfig, direction: GizmoDirection) -> DVec3 {
    let mut normal = gizmo_local_normal(config, direction);

    if config.local_space() && direction != GizmoDirection::View {
        normal = config.rotation * normal;
    }

    normal
}

pub(crate) fn gizmo_color(
    config: &PreparedGizmoConfig,
    focus: f32,
    direction: GizmoDirection,
) -> Color32 {
    let color = match direction {
        GizmoDirection::X => config.visuals.x_color,
        GizmoDirection::Y => config.visuals.y_color,
        GizmoDirection::Z => config.visuals.z_color,
        GizmoDirection::View => config.visuals.s_color,
    };

    // Lerp toward the highlight colour (when set) + the highlight alpha by the
    // eased focus factor so the handle fades in/out (focus 0 == inactive palette
    // colour + inactive_alpha, 1 == highlight colour + highlight_alpha).
    let color = match config.visuals.highlight_color {
        Some(hl) => color.lerp_to_gamma(hl, focus),
        None => color,
    };

    let alpha = config.visuals.inactive_alpha
        + (config.visuals.highlight_alpha - config.visuals.inactive_alpha) * focus;

    color.linear_multiply(alpha)
}
