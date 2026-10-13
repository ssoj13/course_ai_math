// Vendored and adapted from `transform-gizmo` 0.9.0.
// Copyright (c) Urho Laukkarinen. Licensed under MIT OR Apache-2.0. See NOTICE.
// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use egui::{Align2, Color32};

use crate::config::GizmoDirection;
use crate::gizmo::{GizmoDrawData, GizmoResult, Ray};
use crate::math::{
    DMat3, DMat4, DQuat, DVec2, DVec3, Pos2, ray_to_plane_origin, rotation_align,
    round_to_interval, world_to_screen,
};
use crate::shape::ShapeBuilder;
use crate::subgizmo::common::{
    gizmo_color, gizmo_local_normal, gizmo_normal, hud_label, outer_circle_radius,
};
use crate::subgizmo::{SubGizmoConfig, SubGizmoKind};

use super::Picked;

pub(crate) type RotationSubGizmo = SubGizmoConfig<Rotation>;

#[derive(Debug, Copy, Clone, Hash)]
pub(crate) struct RotationParams {
    pub direction: GizmoDirection,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct RotationState {
    start_axis_angle: f64,
    start_rotation_angle: f64,
    last_rotation_angle: f64,
    current_delta: f64,
    /// Cursor screen position stashed each `update()`, so the cursor-less
    /// `draw()` can anchor the drag HUD readout next to the pointer.
    last_cursor: Pos2,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct Rotation;

pub(crate) struct RotationPickResult {
    angle: f64,
    rotation_angle: f64,
    picked: bool,
    t: f64,
}
impl Picked for RotationPickResult {
    fn picked(&self) -> bool {
        self.picked
    }
}

impl SubGizmoKind for Rotation {
    type Params = RotationParams;
    type State = RotationState;
    type PickPreview = RotationPickResult;

    fn pick_preview(subgizmo: &SubGizmoConfig<Self>, ray: Ray) -> Self::PickPreview
    where
        Self: Sized,
    {
        let radius = arc_radius(subgizmo);
        let config = subgizmo.config;
        let origin = config.translation;
        let normal = gizmo_normal(&subgizmo.config, subgizmo.direction);
        let tangent = tangent(subgizmo);

        let (t, dist_from_gizmo_origin) =
            ray_to_plane_origin(normal, origin, ray.origin, ray.direction);
        let dist_from_gizmo_edge = (dist_from_gizmo_origin - radius).abs();

        let hit_pos = ray.origin + ray.direction * t;
        let dir_to_origin = (origin - hit_pos).normalize();
        let nearest_circle_pos = hit_pos + dir_to_origin * (dist_from_gizmo_origin - radius);

        let offset = (nearest_circle_pos - origin).normalize();

        let angle = if subgizmo.direction == GizmoDirection::View {
            f64::atan2(tangent.cross(normal).dot(offset), tangent.dot(offset))
        } else {
            let mut forward = config.view_forward();
            if config.left_handed {
                forward *= -1.0;
            }
            f64::atan2(offset.cross(forward).dot(normal), offset.dot(forward))
        };

        let picked = dist_from_gizmo_edge <= config.focus_distance as f64
            && angle.abs() < arc_angle(subgizmo);

        let rotation_angle = rotation_angle(subgizmo, ray.screen_pos).unwrap_or(0.0);

        Self::PickPreview {
            angle,
            rotation_angle,
            picked,
            t,
        }
    }

    fn pick(subgizmo: &mut RotationSubGizmo, ray: Ray) -> Option<f64> {
        let pick_result = Self::pick_preview(subgizmo, ray);

        subgizmo.state.start_axis_angle = pick_result.angle;
        subgizmo.state.start_rotation_angle = pick_result.rotation_angle;
        subgizmo.state.last_rotation_angle = pick_result.rotation_angle;
        subgizmo.state.current_delta = 0.0;

        if pick_result.picked {
            Some(pick_result.t)
        } else {
            None
        }
    }

    fn update(subgizmo: &mut RotationSubGizmo, ray: Ray) -> Option<GizmoResult> {
        let config = subgizmo.config;
        // Stash for the HUD (see `RotationState::last_cursor`).
        subgizmo.state.last_cursor = ray.screen_pos;

        let mut rotation_angle = rotation_angle(subgizmo, ray.screen_pos)?;
        if config.snapping {
            rotation_angle = round_to_interval(
                rotation_angle - subgizmo.state.start_rotation_angle,
                config.snap_angle as f64,
            ) + subgizmo.state.start_rotation_angle;
        }

        let mut angle_delta = rotation_angle - subgizmo.state.last_rotation_angle;

        // Always take the smallest angle, e.g. -10° instead of 350°
        if angle_delta > PI {
            angle_delta -= TAU;
        } else if angle_delta < -PI {
            angle_delta += TAU;
        }

        subgizmo.state.last_rotation_angle = rotation_angle;
        subgizmo.state.current_delta += angle_delta;

        let normal = gizmo_local_normal(&subgizmo.config, subgizmo.direction);

        Some(GizmoResult::Rotation {
            axis: normal,
            delta: -angle_delta,
            total: subgizmo.state.current_delta,
            is_view_axis: subgizmo.direction == GizmoDirection::View,
        })
    }

    fn draw(subgizmo: &RotationSubGizmo) -> GizmoDrawData {
        let config = subgizmo.config;

        let transform = rotation_matrix(subgizmo);
        let shape_builder = ShapeBuilder::new(
            config.view_projection * transform,
            config.viewport,
            config.pixels_per_point,
        );

        // Per-group transparency for all rotation ring draw calls in this frame.
        let color = gizmo_color(&subgizmo.config, subgizmo.focus_factor, subgizmo.direction)
            .gamma_multiply(config.visuals.rotate_alpha);
        let stroke = (config.visuals.stroke_width, color);

        let radius = arc_radius(subgizmo);

        // Drag HUD master gate: the whole readout/dial/sweep restyle only kicks
        // in while THIS handle is actively dragged and the caller left the HUD
        // on. When off we fall back to the original constant sweep alpha.
        let hud_on = subgizmo.active && config.visuals.show_drag_hud;
        let sweep_base = if hud_on {
            config.visuals.hud_sweep_alpha
        } else {
            0.25
        };

        let mut draw_data = GizmoDrawData::default();

        if !subgizmo.active {
            let angle = arc_angle(subgizmo);
            draw_data += shape_builder
                .arc(radius, FRAC_PI_2 - angle, FRAC_PI_2 + angle, stroke)
                .into();
        } else {
            let mut start_angle = subgizmo.state.start_axis_angle + FRAC_PI_2;
            let mut end_angle = start_angle + subgizmo.state.current_delta;

            if start_angle > end_angle {
                // First make it so that end angle is always greater than start angle
                std::mem::swap(&mut start_angle, &mut end_angle);
            }

            // The polyline does not get rendered correctly if
            // the start and end lines are exactly the same
            end_angle += 1e-5;

            let total_angle = end_angle - start_angle;

            let full_circles = (total_angle / std::f64::consts::TAU).abs() as u32;

            end_angle -= TAU * full_circles as f64;

            let mut start_angle_2 = end_angle;
            let mut end_angle_2 = start_angle + TAU;

            if config
                .view_forward()
                .dot(gizmo_normal(&config, subgizmo.direction))
                < 0.0
            {
                // Swap start and end angles based on the view direction relative to gizmo normal.
                // Otherwise the filled sector gets drawn incorrectly.
                std::mem::swap(&mut start_angle, &mut end_angle);
                std::mem::swap(&mut start_angle_2, &mut end_angle_2);
            }

            draw_data += shape_builder
                .polyline(
                    &[
                        DVec3::new(start_angle.cos() * radius, 0.0, start_angle.sin() * radius),
                        DVec3::new(0.0, 0.0, 0.0),
                        DVec3::new(end_angle.cos() * radius, 0.0, end_angle.sin() * radius),
                    ],
                    stroke,
                )
                .into();

            if full_circles > 0 {
                draw_data += shape_builder
                    .sector(
                        radius,
                        start_angle_2,
                        end_angle_2,
                        color.linear_multiply((sweep_base * full_circles as f32).min(1.0)),
                        (0.0, Color32::TRANSPARENT),
                    )
                    .into();
            }

            draw_data += shape_builder
                .sector(
                    radius,
                    start_angle,
                    end_angle,
                    color.linear_multiply((sweep_base * (full_circles + 1) as f32).min(1.0)),
                    (0.0, Color32::TRANSPARENT),
                )
                .into();

            draw_data += shape_builder.circle(radius, stroke).into();

            // --- Drag HUD: dial tick ring + degree labels --------------------
            // Majors land on `dial_tick_step_deg`, minors at the half-step.
            // Ticks that coincide with a snap multiple are emphasised. The
            // degree numbers are emitted through the text channel (`hud_label`).
            if hud_on && config.visuals.dial_ticks {
                let tick_color = config.visuals.hud_tick_color;
                let step_deg = config.visuals.dial_tick_step_deg.max(1.0) as f64;
                let step = step_deg.to_radians();
                let half_step = step / 2.0;
                let major_w = stroke.0;
                let minor_w = stroke.0 / 2.0;
                // Tick angles run in the ring's local frame (same as the snap
                // ticks above): 0..TAU, no view-direction fix-up needed.
                let count = (TAU / half_step).round() as usize;
                for i in 0..count {
                    let angle = i as f64 * half_step;
                    let is_major = (i % 2) == 0;
                    let pos = DVec3::new(angle.cos(), 0.0, angle.sin());
                    // Emphasise ticks that fall on a snap multiple when snapping.
                    let on_snap = config.snapping && {
                        let snap = config.snap_angle as f64;
                        snap > 1e-6 && (angle % snap).min(snap - (angle % snap)) < 1e-3
                    };
                    let (inner, outer, width) = if is_major {
                        (1.05, 1.12, major_w)
                    } else {
                        (1.05, 1.09, minor_w)
                    };
                    let col = if on_snap {
                        gizmo_color(&subgizmo.config, subgizmo.focus_factor, subgizmo.direction)
                    } else if is_major {
                        tick_color
                    } else {
                        tick_color.gamma_multiply(0.6)
                    };
                    draw_data += shape_builder
                        .line_segment(pos * radius * inner, pos * radius * outer, (width, col))
                        .into();

                    // Degree number at each major tick, just outside the ring.
                    if is_major {
                        let label_world = DVec3::new(
                            angle.cos() * radius * 1.25,
                            0.0,
                            angle.sin() * radius * 1.25,
                        );
                        if let Some(screen) = world_to_screen(
                            config.viewport,
                            config.view_projection * transform,
                            label_world,
                        ) {
                            let deg = (angle.to_degrees().round() as i64).rem_euclid(360);
                            draw_data.labels.push(hud_label(
                                screen,
                                format!("{deg}"),
                                config.visuals.hud_text_color,
                                config.visuals.hud_font_size,
                                Align2::CENTER_CENTER,
                            ));
                        }
                    }
                }
            }

            // Draw snapping ticks
            if config.snapping {
                let stroke_width = stroke.0 / 2.0;
                for i in 0..((TAU / config.snap_angle as f64) as usize + 1) {
                    let angle = i as f64 * config.snap_angle as f64 + end_angle;
                    let pos = DVec3::new(angle.cos(), 0.0, angle.sin());
                    draw_data += shape_builder
                        .line_segment(
                            pos * radius * 1.1,
                            pos * radius * 1.2,
                            (stroke_width, stroke.1),
                        )
                        .into();
                }
            }

            // --- Drag HUD: numeric readout near the cursor -------------------
            // `current_delta` already reflects snapping (the `update` snaps the
            // raw angle before accumulating), so this prints the SNAPPED value
            // when snapping is on. Anchored a few px from the stashed cursor so
            // the pointer doesn't cover it.
            if hud_on && config.visuals.hud_readout {
                let cursor = subgizmo.state.last_cursor;
                let pos = Pos2::new(cursor.x + 14.0, cursor.y - 14.0);
                draw_data.labels.push(hud_label(
                    pos,
                    format!("{:.1}\u{00b0}", subgizmo.state.current_delta.to_degrees()),
                    config.visuals.hud_text_color,
                    config.visuals.hud_font_size,
                    Align2::LEFT_BOTTOM,
                ));
            }
        }

        draw_data
    }
}

/// Calculates angle of the rotation axis arc.
/// The arc is a semicircle, which turns into a full circle when viewed
/// directly from the front.
fn arc_angle(subgizmo: &SubGizmoConfig<Rotation>) -> f64 {
    let dot = gizmo_normal(&subgizmo.config, subgizmo.direction)
        .dot(subgizmo.config.view_forward())
        .abs();
    let min_dot = 0.990;
    let max_dot = 0.995;

    let mut angle =
        f64::min(1.0, f64::max(0.0, dot - min_dot) / (max_dot - min_dot)) * FRAC_PI_2 + FRAC_PI_2;
    if (angle - PI).abs() < 1e-2 {
        angle = PI;
    }
    angle
}

/// Calculates a matrix used when rendering the rotation axis.
fn rotation_matrix(subgizmo: &SubGizmoConfig<Rotation>) -> DMat4 {
    if subgizmo.direction == GizmoDirection::View {
        let forward = subgizmo.config.view_forward();
        let right = subgizmo.config.view_right();
        let up = subgizmo.config.view_up();

        let rotation = DQuat::from_mat3(&DMat3::from_cols(up, -forward, -right));

        return DMat4::from_rotation_translation(rotation, subgizmo.config.translation);
    }

    // First rotate towards the gizmo normal
    let local_normal = gizmo_local_normal(&subgizmo.config, subgizmo.direction);
    let rotation = rotation_align(DVec3::Y, local_normal);
    let mut rotation = DQuat::from_mat3(&rotation);
    let config = subgizmo.config;

    if config.local_space() {
        rotation = config.rotation * rotation;
    }

    let tangent = tangent(subgizmo);
    let normal = gizmo_normal(&subgizmo.config, subgizmo.direction);
    let mut forward = config.view_forward();
    if config.left_handed {
        forward *= -1.0;
    }
    let angle = f64::atan2(tangent.cross(forward).dot(normal), tangent.dot(forward));

    // Rotate towards the camera, along the rotation axis.
    rotation = DQuat::from_axis_angle(normal, angle) * rotation;

    DMat4::from_rotation_translation(rotation, config.translation)
}

fn rotation_angle(subgizmo: &SubGizmoConfig<Rotation>, cursor_pos: Pos2) -> Option<f64> {
    let viewport = subgizmo.config.viewport;
    let gizmo_pos = world_to_screen(viewport, subgizmo.config.mvp, DVec3::new(0.0, 0.0, 0.0))?;
    let delta = DVec2::new(
        cursor_pos.x as f64 - gizmo_pos.x as f64,
        cursor_pos.y as f64 - gizmo_pos.y as f64,
    )
    .normalize();

    if delta.is_nan() {
        return None;
    }

    let mut angle = f64::atan2(delta.y, delta.x);
    if subgizmo
        .config
        .view_forward()
        .dot(gizmo_normal(&subgizmo.config, subgizmo.direction))
        < 0.0
    {
        angle *= -1.0;
    }

    Some(angle)
}

fn tangent(subgizmo: &SubGizmoConfig<Rotation>) -> DVec3 {
    let mut tangent = match subgizmo.direction {
        GizmoDirection::X | GizmoDirection::Y => DVec3::Z,
        GizmoDirection::Z => -DVec3::Y,
        GizmoDirection::View => -subgizmo.config.view_right(),
    };

    if subgizmo.config.local_space() && subgizmo.direction != GizmoDirection::View {
        tangent = subgizmo.config.rotation * tangent;
    }

    tangent
}

fn arc_radius(subgizmo: &SubGizmoConfig<Rotation>) -> f64 {
    if subgizmo.direction == GizmoDirection::View {
        outer_circle_radius(&subgizmo.config)
    } else {
        (subgizmo.config.scale_factor * subgizmo.config.visuals.gizmo_size) as f64
    }
}
