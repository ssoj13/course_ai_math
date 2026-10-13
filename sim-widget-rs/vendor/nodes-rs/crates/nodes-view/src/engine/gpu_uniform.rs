//! Shared view + graph-plane uniform for line and SDF shape pipelines.

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

use crate::camera::{Camera, GraphPlane};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ViewPlaneUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub plane_origin: [f32; 4],
    pub plane_axis_u: [f32; 4],
    pub plane_axis_v: [f32; 4],
}

impl ViewPlaneUniforms {
    pub fn from_view_proj_and_plane(view_proj: Mat4, plane: &GraphPlane) -> Self {
        let o = plane.origin;
        let u = plane.axis_u;
        let v = plane.axis_v;
        Self {
            view_proj: view_proj.to_cols_array_2d(),
            plane_origin: [o.x, o.y, o.z, 0.0],
            plane_axis_u: [u.x, u.y, u.z, 0.0],
            plane_axis_v: [v.x, v.y, v.z, 0.0],
        }
    }

    pub fn from_camera(camera: &Camera, view_proj: Mat4) -> Self {
        Self::from_view_proj_and_plane(view_proj, &camera.plane)
    }
}
