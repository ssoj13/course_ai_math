//! Checked independent cameras: RH, y-up, reversed-Z[0,1].
use crate::{Result, bounds::Bounds, error::invalid};
use glam::{DMat4, DVec2, DVec3, DVec4};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Projection {
    Orthographic { vertical_span: f64 },
    Perspective { vertical_fov: f64 },
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    pub near: f64,
    pub far: f64,
    pub projection: Projection,
}
#[derive(Clone, Copy, Debug, PartialEq)]
/// Coherent immutable matrix bundle constructed only by Camera::matrices.
/// ```compile_fail
/// let mut camera=sim_render::camera::Camera::default().matrices([800.0,480.0]).unwrap();
/// camera.inverse=glam::DMat4::IDENTITY;
/// ```
pub struct CameraMatrices {
    view: DMat4,
    projection: DMat4,
    view_projection: DMat4,
    inverse: DMat4,
    eye: DVec3,
    toward_eye: DVec3,
    css: [f64; 2],
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: DVec3,
    pub direction: DVec3,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: [0.0, 0.0, 10.0],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            near: 0.01,
            far: 10000.0,
            projection: Projection::Orthographic { vertical_span: 8.0 },
        }
    }
}
impl Camera {
    pub fn matrices(&self, css: [f64; 2]) -> Result<CameraMatrices> {
        let eye = DVec3::from_array(self.eye);
        let target = DVec3::from_array(self.target);
        let up = DVec3::from_array(self.up);
        if !eye.is_finite()
            || !target.is_finite()
            || !up.is_finite()
            || css.iter().any(|v| !v.is_finite() || *v <= 0.0)
            || !self.near.is_finite()
            || !self.far.is_finite()
            || self.near <= 0.0
            || self.far <= self.near
        {
            return Err(invalid("camera", "nonfinite or invalid clip/viewport"));
        }
        let forward = target - eye;
        if forward.length_squared() < 1e-20
            || up.length_squared() < 1e-20
            || forward.cross(up).length_squared() < 1e-20
        {
            return Err(invalid("camera", "singular view orientation"));
        }
        let aspect = css[0] / css[1];
        let projection = match self.projection {
            Projection::Orthographic { vertical_span } => {
                if !vertical_span.is_finite() || vertical_span <= 0.0 {
                    return Err(invalid("camera.span", "positive finite span required"));
                }
                let y = vertical_span / 2.0;
                DMat4::orthographic_rh(-y * aspect, y * aspect, -y, y, self.far, self.near)
            }
            Projection::Perspective { vertical_fov } => {
                if !vertical_fov.is_finite() || !(0.001..3.13).contains(&vertical_fov) {
                    return Err(invalid("camera.fov", "FOV must lie in(0.001,3.13) radians"));
                }
                DMat4::perspective_rh(vertical_fov, aspect, self.far, self.near)
            }
        };
        let view = DMat4::look_at_rh(eye, target, up);
        let vp = projection * view;
        let inverse = vp.inverse();
        if !inverse.is_finite() {
            return Err(invalid("camera", "singular inverse"));
        }
        Ok(CameraMatrices {
            view,
            projection,
            view_projection: vp,
            inverse,
            eye,
            toward_eye: -forward.normalize(),
            css,
        })
    }
    pub fn fit(&self, bounds: Bounds, css: [f64; 2], padding_css: f64) -> Result<Self> {
        self.matrices(css)?;
        if !padding_css.is_finite() || padding_css < 0.0 || padding_css * 2.0 >= css[0].min(css[1])
        {
            return Err(invalid("camera.fit", "invalid padding"));
        }
        let center = bounds.center();
        let direction = (DVec3::from_array(self.eye) - DVec3::from_array(self.target)).normalize();
        let view = DMat4::look_at_rh(center + direction, center, DVec3::from_array(self.up));
        let points = bounds.corners().map(|p| view.transform_vector3(p - center));
        let rx = points.iter().map(|p| p.x.abs()).fold(0.0, f64::max);
        let ry = points.iter().map(|p| p.y.abs()).fold(0.0, f64::max);
        let rz = points.iter().map(|p| p.z.abs()).fold(0.0, f64::max);
        let frac = [
            1.0 - 2.0 * padding_css / css[0],
            1.0 - 2.0 * padding_css / css[1],
        ];
        let (projection, distance) = match self.projection {
            Projection::Orthographic { .. } => {
                let span = (2.0 * ry / frac[1])
                    .max(2.0 * rx / (css[0] / css[1] * frac[0]))
                    .max(0.01);
                (
                    Projection::Orthographic {
                        vertical_span: span,
                    },
                    (DVec3::from_array(self.eye) - DVec3::from_array(self.target))
                        .length()
                        .max(rz + self.near * 2.0),
                )
            }
            Projection::Perspective { vertical_fov } => {
                let tan = (vertical_fov / 2.0).tan();
                let distance = (ry / (tan * frac[1])).max(rx / (tan * css[0] / css[1] * frac[0]))
                    + rz
                    + self.near;
                (self.projection, distance)
            }
        };
        let mut result = *self;
        result.eye = (center + direction * distance).to_array();
        result.target = center.to_array();
        result.projection = projection;
        result.far = result.far.max(distance + rz + self.near);
        Ok(result)
    }
}
impl CameraMatrices {
    pub fn view(&self) -> DMat4 { self.view }
    pub fn projection(&self) -> DMat4 { self.projection }
    pub fn view_projection(&self) -> DMat4 { self.view_projection }
    pub fn inverse(&self) -> DMat4 { self.inverse }
    pub fn eye(&self) -> DVec3 { self.eye }
    pub fn toward_eye(&self) -> DVec3 { self.toward_eye }
    pub fn css(&self) -> [f64; 2] { self.css }

    pub fn project(&self, world: DVec3) -> Option<DVec3> {
        let c = self.view_projection * world.extend(1.0);
        // Homogeneous roundoff at a mathematically exact clip plane is scale relative.
        let tolerance = 64.0 * f64::EPSILON * c.abs().max_element().max(1.0);
        if !c.is_finite() || c.w <= 1e-12 || c.z < -tolerance || c.z > c.w + tolerance {
            return None;
        }
        let n = c.truncate() / c.w;
        Some(DVec3::new(
            (n.x + 1.0) * 0.5 * self.css[0],
            (1.0 - n.y) * 0.5 * self.css[1],
            n.z.clamp(0.0, 1.0),
        ))
    }
    pub fn unproject(&self, css: DVec2, depth: f64) -> Result<DVec3> {
        if !css.is_finite() || !depth.is_finite() || !(0.0..=1.0).contains(&depth) {
            return Err(invalid("camera.unproject", "invalid coordinates/depth"));
        }
        let world = self.inverse
            * DVec4::new(
                2.0 * css.x / self.css[0] - 1.0,
                1.0 - 2.0 * css.y / self.css[1],
                depth,
                1.0,
            );
        if !world.is_finite() || world.w.abs() < 1e-15 {
            return Err(invalid("camera.unproject", "point at infinity"));
        }
        Ok(world.truncate() / world.w)
    }
    pub fn ray(&self, css: DVec2) -> Result<Ray> {
        let near = self.unproject(css, 1.0)?;
        let far = self.unproject(css, 0.0)?;
        Ok(Ray {
            origin: near,
            direction: (far - near).normalize(),
        })
    }
}
