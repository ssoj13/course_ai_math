//! Graph working plane in world space (SSOT for 2D node coordinates).

use glam::{Mat4, Vec2, Vec3};

/// Infinite plane: node graph lives in `(u, v)` on this surface.
///
/// Right-handed frame: **`normal = axis_v × axis_u`**.  
/// World position: `origin − u·axis_u − v·axis_v`.
///
/// Both tangents are negated relative to their raw axes so that, under the
/// canonical (non-mirrored) RH ortho in [`crate::Camera::view_proj`], graph **+u**
/// goes **right** and graph **+v** goes **down** on screen. This is the single
/// place the screen-orientation sign lives (mirrored in `shape.wgsl` / `line.wgsl`
/// and `wgpu_widgets_graph::PlaneFrame`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphPlane {
    pub origin: Vec3,
    /// Unit normal (toward the camera in top-down setup).
    pub normal: Vec3,
    /// Unit tangent: graph **+u** (node editor default: world **+X**).
    pub axis_u: Vec3,
    /// Unit tangent for graph **+v** (screen-down via minus in [`Self::graph_to_world`]).
    pub axis_v: Vec3,
}

impl GraphPlane {
    /// Build a plane from explicit graph axes (orthonormal, coplanar).
    ///
    /// # Panics (debug)
    /// `axis_u` and `axis_v` must be unit-length, perpendicular, and `axis_v × axis_u` must be non-zero.
    pub fn from_uv_axes(origin: Vec3, axis_u: Vec3, axis_v: Vec3) -> Self {
        let axis_u = axis_u.normalize();
        let axis_v = axis_v.normalize();
        debug_assert!(
            axis_u.dot(axis_v).abs() < 1e-4,
            "axis_u and axis_v must be perpendicular"
        );
        let normal = axis_v.cross(axis_u).normalize();
        debug_assert!(normal.length_squared() > 0.5, "degenerate graph axes");
        Self {
            origin,
            normal,
            axis_u,
            axis_v,
        }
    }

    /// Node editor default: vertical **blackboard** in the **XY** plane at `z = 0`.
    ///
    /// - Graph `u` → world **+X** (right on the board)
    /// - Graph **+v** → screen-down → world **−Y**
    /// - Plane normal **−Z**; camera in front at **−Z**, looks toward **+Z**, `up` = **+Y**
    #[inline]
    pub fn default_graph() -> Self {
        Self::blackboard_xy()
    }

    /// Vertical board at `z = 0` (school-blackboard layout, **Y-up** world).
    #[inline]
    pub fn blackboard_xy() -> Self {
        Self::from_uv_axes(Vec3::ZERO, Vec3::X, Vec3::Y)
    }

    /// Horizontal floor at `y = 0` (legacy top-down orbit on **XZ**).
    #[inline]
    pub fn xz_floor_y_up() -> Self {
        Self::from_uv_axes(Vec3::ZERO, Vec3::X, Vec3::Z)
    }

    /// `world_from_local` for a surface anchored at graph `(u, v)` top-left.
    ///
    /// Local units are graph UV (1 unit = 1 layout pixel), matching widget surfaces.
    /// Local +x → world −axis_u and local +y → world −axis_v, matching
    /// [`Self::graph_to_world`] so surfaces share the graph's screen orientation.
    #[inline]
    pub fn surface_xfm_from_graph_uv(&self, anchor_uv: Vec2) -> Mat4 {
        self.surface_xfm_from_graph_uv_offset(anchor_uv, Vec3::ZERO)
    }

    /// As [`Self::surface_xfm_from_graph_uv`] with a world-space `origin_offset`.
    ///
    /// The offset nudges the surface off the working plane (e.g. toward the camera by
    /// `camera.forward() * eps`) so coplanar plane-text wins the depth test against the
    /// node bodies the shape pass writes at the same plane `z`.
    #[inline]
    pub fn surface_xfm_from_graph_uv_offset(&self, anchor_uv: Vec2, origin_offset: Vec3) -> Mat4 {
        let origin = self.graph_to_world(anchor_uv.x, anchor_uv.y) + origin_offset;
        Mat4::from_cols(
            (-self.axis_u).extend(0.0),
            (-self.axis_v).extend(0.0),
            self.normal.extend(0.0),
            origin.extend(1.0),
        )
    }

    /// Map graph coordinates to world position.
    #[inline]
    pub fn graph_to_world(&self, u: f32, v: f32) -> Vec3 {
        // Both tangents negated so +u→screen-right, +v→screen-down under the
        // non-mirrored RH ortho (see type docs / FIX 2).
        self.origin - self.axis_u * u - self.axis_v * v
    }

    /// Project a point on the plane into graph `(u, v)`.
    #[inline]
    pub fn world_to_graph(&self, world: Vec3) -> Vec2 {
        let rel = world - self.origin;
        Vec2::new(-rel.dot(self.axis_u), -rel.dot(self.axis_v))
    }

    /// Ray ∩ plane → graph `(u, v)`, or `None` if parallel or behind the ray origin.
    pub fn intersect_ray(&self, origin: Vec3, dir: Vec3) -> Option<Vec2> {
        let denom = dir.dot(self.normal);
        if denom.abs() < 1e-6 {
            return None;
        }
        let t = (self.origin - origin).dot(self.normal) / denom;
        if t < 0.0 {
            return None;
        }
        let hit = origin + dir * t;
        Some(self.world_to_graph(hit))
    }
}

/// Post-rotate a plane-surface transform about a `pivot` within the plane — the SINGLE rotation
/// compose shared by every node-attached surface that isn't the SDF (which rotates in-shader):
/// inline widgets (`node_widgets`) and text labels (`text_labels`). `base` maps surface-local →
/// world; `pivot` is the node center in world; `normal` is the plane normal (rotation axis);
/// `angle` is radians (matching [`crate::NodeXform`] / `shape.wgsl`). Identity for `angle == 0`.
pub fn rotate_surface_about(base: Mat4, pivot: Vec3, normal: Vec3, angle: f32) -> Mat4 {
    if angle == 0.0 {
        return base;
    }
    Mat4::from_translation(pivot)
        * Mat4::from_axis_angle(normal, angle)
        * Mat4::from_translation(-pivot)
        * base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xz_floor_roundtrip() {
        let p = GraphPlane::xz_floor_y_up();
        assert!((p.normal - Vec3::Y).length() < 1e-4);
        assert!((p.axis_u - Vec3::X).length() < 1e-4);
        assert!((p.axis_v - Vec3::Z).length() < 1e-4);
        for uv in [(0.0, 0.0), (100.0, -50.0), (-12.5, 3.25)] {
            let w = p.graph_to_world(uv.0, uv.1);
            let back = p.world_to_graph(w);
            assert!((back.x - uv.0).abs() < 1e-4);
            assert!((back.y - uv.1).abs() < 1e-4);
            assert!(w.y.abs() < 1e-4, "floor at y=0, got {w:?}");
            assert!(
                (w.x + uv.0).abs() < 1e-4,
                "w.x = -graph_u, got w={w:?} uv={uv:?}"
            );
            assert!(
                (w.z + uv.1).abs() < 1e-4,
                "w.z = -graph_v, got w={w:?} uv={uv:?}"
            );
        }
    }

    #[test]
    fn xz_ray_from_above() {
        let p = GraphPlane::xz_floor_y_up();
        // graph u=-10 → world x=10, graph v=20 → world z=-20 (see graph_to_world)
        let origin = Vec3::new(10.0, 500.0, -20.0);
        let dir = Vec3::new(0.0, -1.0, 0.0);
        let uv = p.intersect_ray(origin, dir).expect("hit");
        assert!((uv.x + 10.0).abs() < 1e-4);
        assert!((uv.y - 20.0).abs() < 1e-4);
    }

    #[test]
    fn blackboard_xy_on_z0() {
        let p = GraphPlane::blackboard_xy();
        assert!((p.normal + Vec3::Z).length() < 1e-4);
        assert!((p.axis_v - Vec3::Y).length() < 1e-4);
        let w = p.graph_to_world(10.0, 20.0);
        assert!(w.z.abs() < 1e-4, "board at z=0, got {w:?}");
        assert!((w.x + 10.0).abs() < 1e-4, "u right → −X world, got {w:?}");
        assert!((w.y + 20.0).abs() < 1e-4, "v down → −Y");
    }

    #[test]
    fn blackboard_ray_from_front() {
        let p = GraphPlane::blackboard_xy();
        let origin = Vec3::new(10.0, -20.0, -500.0);
        let dir = Vec3::new(0.0, 0.0, 1.0);
        let uv = p.intersect_ray(origin, dir).expect("hit");
        assert!((uv.x + 10.0).abs() < 1e-4);
        assert!((uv.y - 20.0).abs() < 1e-4);
    }

    #[test]
    fn parallel_ray_misses() {
        let p = GraphPlane::xz_floor_y_up();
        assert!(p.intersect_ray(Vec3::ZERO, Vec3::X).is_none());
    }
}
