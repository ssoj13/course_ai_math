// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.
//
//! 3D bounding-box scale handles — the volumetric twin of `bbox.rs`.
//!
//! 26 grips on the gizmo's LOCAL box, one per sign vector in {-1,0,1}³ \ {0}:
//! 6 face centres (one non-zero sign), 12 edge mids (two), 8 corners (three).
//! Dragging a grip scales the target about the OPPOSITE anchor along the grip's
//! non-zero axes (face → 1 axis, edge → 2, corner → 3) — the 3D analogue of the
//! planar bbox gesture. Emits a TRS [`GizmoResult::BoxScale`] (scale about a
//! world pivot), so it rides the existing box-scale result path unchanged.
//!
//! Drag math runs in a frozen reference frame against a single camera-facing
//! plane through the box centre (normal = `eye_to_model_dir`), reading the hit's
//! local coords and forming a per-axis ratio — the same scheme as the planar box
//! generalised from one plane-axis-pair to three axes.

use glam::{DMat4, DVec3};

use crate::config::PreparedGizmoConfig;
use crate::gizmo::{GizmoDrawData, GizmoResult, Ray};
use crate::math::{intersect_plane, world_to_screen};
use crate::shape::ShapeBuilder;
use crate::subgizmo::{SubGizmoConfig, SubGizmoKind};

use super::common::PickResult;

pub(crate) type Bbox3DSubGizmo = SubGizmoConfig<Bbox3D>;

/// One of the 26 box grips as a sign per axis (each ∈ {-1, 0, 1}, not all zero).
/// The count of non-zero signs is the grip kind: 1 = face, 2 = edge, 3 = corner.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub(crate) struct Bbox3DHandle {
    pub sx: i8,
    pub sy: i8,
    pub sz: i8,
}

impl Bbox3DHandle {
    /// All 26 grips in a stable order (used by the builder).
    pub(crate) fn all() -> Vec<Bbox3DHandle> {
        let mut v = Vec::with_capacity(26);
        for sx in -1i8..=1 {
            for sy in -1i8..=1 {
                for sz in -1i8..=1 {
                    if sx == 0 && sy == 0 && sz == 0 {
                        continue;
                    }
                    v.push(Bbox3DHandle { sx, sy, sz });
                }
            }
        }
        v
    }

    /// This grip's anchor in the gizmo LOCAL frame (half-extent `h`).
    fn anchor(self, h: DVec3) -> DVec3 {
        DVec3::new(
            self.sx as f64 * h.x,
            self.sy as f64 * h.y,
            self.sz as f64 * h.z,
        )
    }

    /// The pivot to scale about = the OPPOSITE anchor.
    fn pivot(self, h: DVec3) -> DVec3 {
        -self.anchor(h)
    }

    /// Which axes this grip scales (its non-zero sign components).
    fn axes(self) -> (bool, bool, bool) {
        (self.sx != 0, self.sy != 0, self.sz != 0)
    }

    /// The single grip that also draws the shared box outline (avoids 26× overdraw).
    fn draws_outline(self) -> bool {
        self.sx == 1 && self.sy == 1 && self.sz == 1
    }
}

#[derive(Debug, Copy, Clone, Hash)]
pub(crate) struct Bbox3DParams {
    pub handle: Bbox3DHandle,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct Bbox3DState {
    /// Gizmo model matrix frozen at pick (see `bbox::BboxState` for the rationale:
    /// scaling about a pivot moves the centre, so the reference frame must freeze).
    start_model: DMat4,
    /// Camera-facing drag plane normal, frozen at pick.
    plane_normal: DVec3,
    /// Cursor's plane hit at pick, in `start_model` LOCAL coords.
    start_local: DVec3,
    /// Opposite anchor in WORLD at pick — the pivot the scale is about.
    pivot_world: DVec3,
}

#[derive(Default, Debug, Copy, Clone)]
pub(crate) struct Bbox3D;

/// Local half-extent of the box. `bbox_half_extent` is a `Vec2` (the planar box's
/// reference); the 3D box mirrors X onto Z so the default reference is a cube
/// ±0.5 — the actual box size still comes from the target's `scale`.
fn half_extent(config: &PreparedGizmoConfig) -> DVec3 {
    DVec3::new(
        config.bbox_half_extent.x as f64,
        config.bbox_half_extent.y as f64,
        config.bbox_half_extent.x as f64,
    )
}

/// Per-axis scale factor about `pivot` from `start`→`cur`. `1.0` when degenerate.
fn factor(cur: f64, start: f64, pivot: f64) -> f64 {
    let denom = start - pivot;
    if denom.abs() > 1e-6 {
        (cur - pivot) / denom
    } else {
        1.0
    }
}

/// Screen-pixel pick tolerance (matches the arrow/plane handles' grab radius).
fn focus_px(config: &PreparedGizmoConfig) -> f32 {
    if config.scale_factor.abs() > 1e-6 {
        config.focus_distance / config.scale_factor
    } else {
        config.focus_distance
    }
}

/// Effective grab radius (screen px) — sized to the drawn grip square.
fn grab_px(config: &PreparedGizmoConfig) -> f32 {
    (config.visuals.bbox_handle_size * 0.7).max(focus_px(config))
}

/// Camera-facing drag plane through the box centre: normal = eye→model dir,
/// origin = the box model translation.
fn drag_plane(config: &PreparedGizmoConfig) -> (DVec3, DVec3) {
    let normal = if config.eye_to_model_dir.length_squared() > 1e-12 {
        config.eye_to_model_dir.normalize()
    } else {
        DVec3::Z
    };
    let origin = config.box_model().transform_point3(DVec3::ZERO);
    (normal, origin)
}

/// Closest point on the line `o + s·d` to the ray `ro + t·rd` (skew-line closest
/// approach). Returns the LINE point; falls back to `o` when near-parallel.
fn closest_on_line(o: DVec3, d: DVec3, ro: DVec3, rd: DVec3) -> DVec3 {
    let w0 = o - ro;
    let a = d.dot(d);
    let b = d.dot(rd);
    let c = rd.dot(rd);
    let dd = d.dot(w0);
    let e = rd.dot(w0);
    let denom = a * c - b * b;
    let s = if denom.abs() > 1e-9 {
        (b * e - c * dd) / denom
    } else {
        0.0
    };
    o + d * s
}

/// Cursor hit in the gizmo LOCAL frame, constrained to the grip's drag subspace:
/// a **face** (1 axis) drags along that world axis line; an **edge** (2 axes)
/// drags in the plane those two axes span (normal = the unscaled axis); a
/// **corner** (3 axes) drags on the frozen camera-facing plane. `model` is the
/// frozen box frame; `cam_normal` the frozen eye→model direction.
fn local_hit(handle: Bbox3DHandle, model: DMat4, cam_normal: DVec3, ray: Ray) -> Option<DVec3> {
    let (do_x, do_y, do_z) = handle.axes();
    let n_axes = do_x as u32 + do_y as u32 + do_z as u32;
    let center = model.transform_point3(DVec3::ZERO);

    let world_hit = match n_axes {
        1 => {
            // Line along the single scaled local axis through the box centre.
            let local_axis = if do_x {
                DVec3::X
            } else if do_y {
                DVec3::Y
            } else {
                DVec3::Z
            };
            let dir = model.transform_vector3(local_axis).normalize_or_zero();
            if dir.length_squared() < 1e-12 {
                return None;
            }
            closest_on_line(center, dir, ray.origin, ray.direction)
        }
        2 => {
            // Plane spanned by the two scaled axes; normal = the UNSCALED axis.
            let unscaled = if !do_x {
                DVec3::X
            } else if !do_y {
                DVec3::Y
            } else {
                DVec3::Z
            };
            let normal = model.transform_vector3(unscaled).normalize_or_zero();
            let mut t = 0.0;
            if normal.length_squared() < 1e-12
                || !intersect_plane(normal, center, ray.origin, ray.direction, &mut t)
            {
                return None;
            }
            ray.origin + ray.direction * t
        }
        _ => {
            // Corner: the frozen camera-facing plane (no single axis/plane fits).
            let mut t = 0.0;
            if !intersect_plane(cam_normal, center, ray.origin, ray.direction, &mut t) {
                return None;
            }
            ray.origin + ray.direction * t
        }
    };
    Some(model.inverse().transform_point3(world_hit))
}

impl SubGizmoKind for Bbox3D {
    type Params = Bbox3DParams;
    type State = Bbox3DState;
    type PickPreview = PickResult;

    fn pick_preview(subgizmo: &Bbox3DSubGizmo, ray: Ray) -> PickResult {
        let config = subgizmo.config;
        let handle = subgizmo.handle;
        let h = half_extent(&config);
        let box_model = config.box_model();
        let Some(anchor_screen) = world_to_screen(
            config.viewport,
            config.view_projection * box_model,
            handle.anchor(h),
        ) else {
            return PickResult {
                subgizmo_point: DVec3::ZERO,
                picked: false,
                t: 0.0,
            };
        };
        let (normal, origin) = drag_plane(&config);
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

    fn pick(subgizmo: &mut Bbox3DSubGizmo, ray: Ray) -> Option<f64> {
        let config = subgizmo.config;
        let handle = subgizmo.handle;
        let h = half_extent(&config);
        let box_model = config.box_model();
        let anchor_screen = world_to_screen(
            config.viewport,
            config.view_projection * box_model,
            handle.anchor(h),
        )?;
        let (cam_normal, _) = drag_plane(&config);
        let start_local = local_hit(handle, box_model, cam_normal, ray)?;
        subgizmo.state.start_model = box_model;
        subgizmo.state.plane_normal = cam_normal;
        subgizmo.state.start_local = start_local;
        subgizmo.state.pivot_world = box_model.transform_point3(handle.pivot(h));
        // Pick depth for closest-subgizmo sorting: ray parameter to the anchor.
        let anchor_world = box_model.transform_point3(handle.anchor(h));
        let depth = (anchor_world - ray.origin).dot(ray.direction);
        if ray.screen_pos.distance(anchor_screen) <= grab_px(&config) {
            Some(depth)
        } else {
            None
        }
    }

    fn update(subgizmo: &mut Bbox3DSubGizmo, ray: Ray) -> Option<GizmoResult> {
        let state = subgizmo.state;
        let handle = subgizmo.handle;
        let h = half_extent(&subgizmo.config);
        let pivot_local = handle.pivot(h);

        let cur_local = local_hit(handle, state.start_model, state.plane_normal, ray)?;

        let (do_x, do_y, do_z) = handle.axes();
        let sx = if do_x {
            factor(cur_local.x, state.start_local.x, pivot_local.x)
        } else {
            1.0
        };
        let sy = if do_y {
            factor(cur_local.y, state.start_local.y, pivot_local.y)
        } else {
            1.0
        };
        let sz = if do_z {
            factor(cur_local.z, state.start_local.z, pivot_local.z)
        } else {
            1.0
        };

        Some(GizmoResult::BoxScale {
            pivot: state.pivot_world,
            scale: DVec3::new(sx, sy, sz),
        })
    }

    fn draw(subgizmo: &Bbox3DSubGizmo) -> GizmoDrawData {
        let config = subgizmo.config;
        let handle = subgizmo.handle;
        let h = half_extent(&config);

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

        let box_model = config.box_model();
        let mut draw_data = GizmoDrawData::default();

        // Shared 3D box outline — drawn once (by the +++ corner) as 12 edges.
        if handle.draws_outline() {
            let shape_builder = ShapeBuilder::new(
                config.view_projection * box_model,
                config.viewport,
                config.pixels_per_point,
            );
            // 8 corners indexed by bit: x = bit0, y = bit1, z = bit2.
            let corner = |i: usize| {
                DVec3::new(
                    if i & 1 == 0 { -h.x } else { h.x },
                    if i & 2 == 0 { -h.y } else { h.y },
                    if i & 4 == 0 { -h.z } else { h.z },
                )
            };
            const EDGES: [(usize, usize); 12] = [
                (0, 1),
                (0, 2),
                (0, 4),
                (1, 3),
                (1, 5),
                (2, 3),
                (2, 6),
                (3, 7),
                (4, 5),
                (4, 6),
                (5, 7),
                (6, 7),
            ];
            for (a, b) in EDGES {
                draw_data += shape_builder
                    .polyline(
                        &[corner(a), corner(b)],
                        (config.visuals.stroke_width, color),
                    )
                    .into();
            }
        }

        // Screen-constant grip square at this handle's anchor (camera-facing,
        // sized in points so it matches the screen-space pick radius).
        let anchor_world = box_model.transform_point3(handle.anchor(h));
        let hsz = (config.scale_factor * config.visuals.bbox_handle_size * 0.5) as f64;
        let right = config.view_right() * hsz;
        let up = config.view_up() * hsz;
        let c = |dx: f64, dy: f64| anchor_world + right * dx + up * dy;
        let screen_builder = ShapeBuilder::new(
            config.view_projection,
            config.viewport,
            config.pixels_per_point,
        );
        draw_data += screen_builder
            .polygon(
                &[c(-1.0, -1.0), c(1.0, -1.0), c(1.0, 1.0), c(-1.0, 1.0)],
                color.gamma_multiply(0.35),
                (1.0, color),
            )
            .into();

        draw_data
    }
}
