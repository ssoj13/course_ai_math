//! Implementation of [`crate::visible_graph_uv_rect`].
//!
//! Module-level docs: [`crate::viewport`].

use glam::{Vec2, Vec3};

use crate::camera::Camera;

use super::graph_uv_rect::GraphUvRect;

/// Subdivisions per screen edge when sampling the viewport perimeter.
const SAMPLES_PER_EDGE: usize = 8;
/// Minimum span (graph units) before treating a rect as degenerate.
const MIN_SPAN: f32 = 1e-3;

/// Returns the axis-aligned graph UV region visible through `viewport`.
///
/// # What
///
/// The 2D slice of the graph plane that lies inside the view frustum, expressed as a
/// [`GraphUvRect`] (AABB in layout coordinates).
///
/// The frustum is a genuine PERSPECTIVE one (the camera stopped being orthographic in 2026-07;
/// its default FOV is merely tiny, which only makes it LOOK ortho).
///
/// # Why
///
/// - **Grid**: only emit line segments over this range (see `engine::grid::build_grid_vertices`).
/// - **Minimap** (egui editor): draw the yellow “camera window” via `Camera::visible_graph_uv_bounds`.
///
/// This is NOT the node-visibility policy: culling + LOD is [`crate::compute_visible_nodes`], which
/// tests a clip-space frustum against each node's AABB and stays correct at grazing angles.
/// - **Callers** should not reimplement corner unproject; grazing angles used to collapse
///   to a single quad at the target when misses fell back to `(0,0)`.
///
/// # Algorithm
///
/// 1. Sample the viewport perimeter (corners + subdivided edges).
/// 2. [`Camera::try_screen_to_graph`] each sample — `None` if the ray misses the plane.
/// 3. If fewer than two hits or a degenerate AABB → `frustum_rect_on_plane` fallback.
///
/// # Arguments
///
/// * `viewport` — pixel size `(width, height)` matching the render target / input layer.
pub fn visible_graph_uv_rect(camera: &Camera, viewport: Vec2) -> GraphUvRect {
    let w = viewport.x.max(1.0);
    let h = viewport.y.max(1.0);

    let mut rect = GraphUvRect::empty();
    let mut hits = 0u32;

    for screen in sample_viewport_perimeter(w, h, SAMPLES_PER_EDGE) {
        if let Some(uv) = camera.try_screen_to_graph(screen, w, h) {
            rect.expand(uv);
            hits += 1;
        }
    }

    if hits >= 2 && !rect.is_degenerate(MIN_SPAN) {
        return rect;
    }

    frustum_rect_on_plane(camera, viewport)
}

/// Pixel positions along the viewport border (for frustum footprint sampling).
fn sample_viewport_perimeter(w: f32, h: f32, per_edge: usize) -> Vec<Vec2> {
    let n = per_edge.max(1);
    let mut out = Vec::with_capacity(4 + 4 * n);

    let mut add_segment = |a: Vec2, b: Vec2| {
        for i in 0..=n {
            let t = i as f32 / n as f32;
            out.push(a + (b - a) * t);
        }
    };

    let tl = Vec2::new(0.0, 0.0);
    let tr = Vec2::new(w, 0.0);
    let br = Vec2::new(w, h);
    let bl = Vec2::new(0.0, h);

    add_segment(tl, tr);
    add_segment(tr, br);
    add_segment(br, bl);
    add_segment(bl, tl);

    out
}

/// Fallback when perimeter rays miss the plane: perspective frustum half-extents at the
/// target depth, projected to UV.
///
/// The visible half-height on the plane is `dist * tan(fov/2)` (and `* aspect` for width) —
/// the perspective replacement for the old `ortho_width * 0.5`. Used only for grazing views
/// where most corner rays miss the plane; the primary path is perimeter sampling above.
fn frustum_rect_on_plane(camera: &Camera, viewport: Vec2) -> GraphUvRect {
    let w = viewport.x.max(1.0);
    let h = viewport.y.max(1.0);
    let aspect = w / h;
    let hh = camera.distance() * (camera.fov_y() * 0.5).tan();
    let hw = hh * aspect;

    let pose = camera.pose();
    // glam 0.33: `Mat4::look_at_rh` → `camera::rh::view::look_at_mat4` (identical math).
    let view = glam::camera::rh::view::look_at_mat4(pose.eye, pose.eye + pose.forward(), pose.up());
    let inv_view = view.inverse();
    let right = inv_view.x_axis.truncate();
    let up = inv_view.y_axis.truncate();

    let right_on_plane = project_tangent_on_plane(right, camera.plane.normal);
    let up_on_plane = project_tangent_on_plane(up, camera.plane.normal);

    let mut rect = GraphUvRect::empty();
    let center = camera.target_world();

    for sx in [-1.0f32, 1.0] {
        for sy in [-1.0f32, 1.0] {
            let world = center + right_on_plane * (sx * hw) + up_on_plane * (sy * hh);
            rect.expand(camera.plane.world_to_graph(world));
        }
    }

    if rect.is_degenerate(MIN_SPAN) {
        let c = camera.target_on_graph();
        return GraphUvRect::from_center_size(c, Vec2::new(hw * 2.0, hh * 2.0));
    }

    rect
}

#[inline]
fn project_tangent_on_plane(v: Vec3, plane_normal: Vec3) -> Vec3 {
    let n = plane_normal.normalize();
    v - n * v.dot(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;

    #[test]
    fn default_view_includes_negative_quadrant() {
        let cam = Camera::default();
        let r = visible_graph_uv_rect(&cam, Vec2::new(800.0, 600.0));
        assert!(!r.is_empty());
        assert!(r.min.x < 0.0, "min u: {:?}", r.min);
        assert!(r.min.y < 0.0, "min v: {:?}", r.min);
        assert!(r.max.x > 0.0 && r.max.y > 0.0, "max: {:?}", r.max);
    }

    #[test]
    fn tilted_view_still_has_reasonable_extent() {
        // Use the editor's default zoom (1 px per world) so the framing is realistic — the
        // raw `Camera::default()` is near-ortho and zoomed right in (±~17 units), which is a
        // valid state but not what a graph is ever viewed at.
        let vp = Vec2::new(800.0, 600.0);
        let mut cam = Camera::default();
        cam.reset_default_zoom(vp);
        cam.orbit(0.8, 0.6);
        let r = visible_graph_uv_rect(&cam, vp);
        assert!(
            !r.is_degenerate(10.0),
            "rect: min={:?} max={:?}",
            r.min,
            r.max
        );
        let span = r.size();
        assert!(span.x > 50.0 && span.y > 50.0, "span={span:?}");
    }

    #[test]
    fn grazing_view_uses_frustum_fallback() {
        let vp = Vec2::new(800.0, 600.0);
        let mut cam = Camera::default();
        cam.reset_default_zoom(vp);
        cam.orbit(1.2, 1.4);
        let r = visible_graph_uv_rect(&cam, vp);
        assert!(!r.is_degenerate(20.0), "min={:?} max={:?}", r.min, r.max);
    }
}
