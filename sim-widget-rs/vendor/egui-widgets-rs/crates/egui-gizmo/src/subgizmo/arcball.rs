// Vendored and adapted from `transform-gizmo` 0.9.0.
// Copyright (c) Urho Laukkarinen. Licensed under MIT OR Apache-2.0. See NOTICE.
// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.

use crate::config::PreparedGizmoConfig;
use crate::gizmo::{GizmoDrawData, GizmoResult, Ray};
use crate::math::{DMat3, DQuat, DVec3, Pos2, world_to_screen};
use crate::subgizmo::common::{draw_circle, pick_circle};
use crate::subgizmo::{SubGizmoConfig, SubGizmoKind};
use egui::Color32;

use super::common::PickResult;

pub(crate) type ArcballSubGizmo = SubGizmoConfig<Arcball>;

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct ArcballState {
    last_pos: Pos2,
    total_rotation: DQuat,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct Arcball;

impl SubGizmoKind for Arcball {
    type Params = ();
    type State = ArcballState;
    type PickPreview = PickResult;

    fn pick_preview(subgizmo: &SubGizmoConfig<Self>, ray: Ray) -> super::common::PickResult
    where
        Self: Sized,
    {
        pick_circle(
            &subgizmo.config,
            ray,
            arcball_radius(&subgizmo.config),
            true,
        )
    }

    fn pick(subgizmo: &mut ArcballSubGizmo, ray: Ray) -> Option<f64> {
        let pick_result = Self::pick_preview(subgizmo, ray);

        subgizmo.state.last_pos = ray.screen_pos;

        if !pick_result.picked {
            return None;
        }

        Some(f64::MAX)
    }

    fn update(subgizmo: &mut ArcballSubGizmo, ray: Ray) -> Option<GizmoResult> {
        let last = subgizmo.state.last_pos;
        let cur = ray.screen_pos;
        subgizmo.state.last_pos = cur;

        // Virtual-trackball (Shoemake) rotation in SCREEN space: map the current and
        // previous cursor positions onto a sphere centred on the gizmo with a radius
        // tied to its on-screen size, then build the rotation taking one sphere point
        // onto the other. This makes sensitivity independent of camera distance/FOV
        // (a drag across the gizmo radius ≈ 90°). The upstream near-plane-unproject
        // approach (origin = view_forward direction, angle × 10) was ~20× too slow on
        // a mid-distance perspective camera and camera-dependent.
        let center = world_to_screen(subgizmo.config.viewport, subgizmo.config.mvp, DVec3::ZERO);
        let rotation_delta = match center {
            Some(center) if (cur - last).length_sq() > f32::EPSILON => {
                let radius = (subgizmo.config.visuals.gizmo_size as f64).max(1.0);
                let va = trackball_vec(cur, center, radius);
                let vb = trackball_vec(last, center, radius);

                // Rotation taking vb -> va, built in eye space then mapped to world.
                let axis_eye = vb.cross(va);
                let angle = vb.dot(va).clamp(-1.0, 1.0).acos();
                if axis_eye.length_squared() > 1e-12 && angle > 1e-9 {
                    // Eye -> world basis: columns are the camera axes (screen +x =
                    // right, +y = up, toward-viewer = -forward).
                    let cam = DMat3::from_cols(
                        subgizmo.config.view_right(),
                        subgizmo.config.view_up(),
                        -subgizmo.config.view_forward(),
                    );
                    DQuat::from_axis_angle((cam * axis_eye).normalize(), angle)
                } else {
                    DQuat::IDENTITY
                }
            }
            _ => DQuat::IDENTITY,
        };

        subgizmo.state.total_rotation = rotation_delta.mul_quat(subgizmo.state.total_rotation);

        Some(GizmoResult::Arcball {
            delta: rotation_delta,
            total: subgizmo.state.total_rotation,
        })
    }

    fn draw(subgizmo: &ArcballSubGizmo) -> GizmoDrawData {
        draw_circle(
            &subgizmo.config,
            // Fade the trackball tint in/out with the eased focus factor (0 -> 0.10).
            Color32::WHITE.gamma_multiply(0.10 * subgizmo.focus_factor),
            arcball_radius(&subgizmo.config),
            true,
        )
    }
}

/// Map a screen point onto the virtual-trackball sphere of the given screen
/// `radius`, centred at `center`. Inside the disc the sphere bulges toward the
/// viewer; outside it falls to the silhouette (z = 0). Screen Y is down, so it is
/// flipped to the +Y-up math convention.
fn trackball_vec(p: Pos2, center: Pos2, radius: f64) -> DVec3 {
    let x = (p.x - center.x) as f64 / radius;
    let y = -((p.y - center.y) as f64) / radius;
    let d2 = x * x + y * y;
    let z = if d2 <= 1.0 { (1.0 - d2).sqrt() } else { 0.0 };
    DVec3::new(x, y, z).normalize()
}

/// Radius to use for outer circle subgizmos
pub(crate) fn arcball_radius(config: &PreparedGizmoConfig) -> f64 {
    (config.scale_factor * (config.visuals.gizmo_size + config.visuals.stroke_width - 5.0)) as f64
}
