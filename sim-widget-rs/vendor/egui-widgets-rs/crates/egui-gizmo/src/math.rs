// Vendored and adapted from `transform-gizmo` 0.9.0.
// Copyright (c) Urho Laukkarinen. Licensed under MIT OR Apache-2.0. See NOTICE.
// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.
//
//! Pure math helpers for the gizmo core. NO egui paint here — only geometry.
//!
//! All math runs in **glam f64** internally (`DMat4`/`DVec3`/`DQuat`). The
//! public facade in [`crate`] widens f32→f64 once at the boundary and narrows
//! back on the way out. `mint` is intentionally absent.

pub use egui::{Pos2, Rect};
pub use glam::{DMat3, DMat4, DQuat, DVec2, DVec3, DVec4, Vec4Swizzles};

/// A scale / rotation / translation transform, expressed in **glam f64**.
///
/// This is the gizmo's INTERNAL transform type. The public-facing equivalent is
/// [`crate::Transform`] (glam f32); conversion happens at the facade boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub scale: DVec3,
    pub rotation: DQuat,
    pub translation: DVec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            scale: DVec3::ONE,
            rotation: DQuat::IDENTITY,
            translation: DVec3::ZERO,
        }
    }
}

impl Transform {
    pub fn from_scale_rotation_translation(
        scale: DVec3,
        rotation: DQuat,
        translation: DVec3,
    ) -> Self {
        Self {
            scale,
            rotation,
            translation,
        }
    }
}

/// Creates a matrix that represents rotation between two 3d vectors
///
/// Credit: <https://www.iquilezles.org/www/articles/noacos/noacos.htm>
pub(crate) fn rotation_align(from: DVec3, to: DVec3) -> DMat3 {
    let v = from.cross(to);
    let c = from.dot(to);
    let k = 1.0 / (1.0 + c);

    DMat3::from_cols_array(&[
        v.x * v.x * k + c,
        v.x * v.y * k + v.z,
        v.x * v.z * k - v.y,
        v.y * v.x * k - v.z,
        v.y * v.y * k + c,
        v.y * v.z * k + v.x,
        v.z * v.x * k + v.y,
        v.z * v.y * k - v.x,
        v.z * v.z * k + c,
    ])
}

/// Finds points on two rays that are closest to each other.
/// This can be used to determine the shortest distance between those two rays.
///
/// Credit: Practical Geometry Algorithms by Daniel Sunday: <http://geomalgorithms.com/code.html>
pub(crate) fn ray_to_ray(a1: DVec3, adir: DVec3, b1: DVec3, bdir: DVec3) -> (f64, f64) {
    let b = adir.dot(bdir);
    let w = a1 - b1;
    let d = adir.dot(w);
    let e = bdir.dot(w);
    let dot = 1.0 - b * b;
    if dot < 1e-8 {
        (0.0, e)
    } else {
        ((b * e - d) / dot, (e - b * d) / dot)
    }
}

/// Finds points on two segments that are closest to each other.
/// This can be used to determine the shortest distance between those two segments.
///
/// Credit: Practical Geometry Algorithms by Daniel Sunday: <http://geomalgorithms.com/code.html>
pub(crate) fn segment_to_segment(a1: DVec3, a2: DVec3, b1: DVec3, b2: DVec3) -> (f64, f64) {
    let da = a2 - a1;
    let db = b2 - b1;
    let la = da.length_squared();
    let lb = db.length_squared();
    let dd = da.dot(db);
    let d1 = a1 - b1;
    let d = da.dot(d1);
    let e = db.dot(d1);
    let n = la * lb - dd * dd;

    let mut sn;
    let mut tn;
    let mut sd = n;
    let mut td = n;

    if n < 1e-8 {
        sn = 0.0;
        sd = 1.0;
        tn = e;
        td = lb;
    } else {
        sn = dd * e - lb * d;
        tn = la * e - dd * d;
        if sn < 0.0 {
            sn = 0.0;
            tn = e;
            td = lb;
        } else if sn > sd {
            sn = sd;
            tn = e + dd;
            td = lb;
        }
    }

    if tn < 0.0 {
        tn = 0.0;
        if -d < 0.0 {
            sn = 0.0;
        } else if -d > la {
            sn = sd;
        } else {
            sn = -d;
            sd = la;
        }
    } else if tn > td {
        tn = td;
        if (-d + dd) < 0.0 {
            sn = 0.0;
        } else if (-d + dd) > la {
            sn = sd;
        } else {
            sn = -d + dd;
            sd = la;
        }
    }

    let ta = if sn.abs() < 1e-8 { 0.0 } else { sn / sd };
    let tb = if tn.abs() < 1e-8 { 0.0 } else { tn / td };

    (ta, tb)
}

/// Finds the intersection point of a ray and a plane
pub(crate) fn intersect_plane(
    plane_normal: DVec3,
    plane_origin: DVec3,
    ray_origin: DVec3,
    ray_dir: DVec3,
    t: &mut f64,
) -> bool {
    let denom = plane_normal.dot(ray_dir);

    if denom.abs() < 10e-8 {
        false
    } else {
        *t = (plane_origin - ray_origin).dot(plane_normal) / denom;
        *t >= 0.0
    }
}

/// Finds the intersection point of a ray and a plane
/// and distance from the intersection to the plane origin
pub(crate) fn ray_to_plane_origin(
    disc_normal: DVec3,
    disc_origin: DVec3,
    ray_origin: DVec3,
    ray_dir: DVec3,
) -> (f64, f64) {
    let mut t = 0.0;
    if intersect_plane(disc_normal, disc_origin, ray_origin, ray_dir, &mut t) {
        let p = ray_origin + ray_dir * t;
        let v = p - disc_origin;
        let d2 = v.dot(v);
        (t, f64::sqrt(d2))
    } else {
        (t, f64::MAX)
    }
}

/// Rounds given value to the nearest interval
pub(crate) fn round_to_interval(val: f64, interval: f64) -> f64 {
    (val / interval).round() * interval
}

/// The map from NDC (`[-1, 1]`, y up) to screen points of `viewport` (y down), column major: the one
/// viewport transform of the gizmo, and of any host drawing over the same view (an OpenFX overlay).
pub fn viewport_matrix(viewport: Rect) -> glam::DMat3 {
    let center = viewport.center();
    glam::DMat3::from_cols(
        glam::DVec3::new(viewport.width() as f64 / 2.0, 0.0, 0.0),
        glam::DVec3::new(0.0, -(viewport.height() as f64) / 2.0, 0.0),
        glam::DVec3::new(center.x as f64, center.y as f64, 1.0),
    )
}

/// Calculates 2d screen coordinates from 3d world coordinates
pub(crate) fn world_to_screen(viewport: Rect, mvp: DMat4, pos: DVec3) -> Option<Pos2> {
    let pos = mvp * DVec4::from((pos, 1.0));

    if pos.w < 1e-10 {
        return None;
    }

    let screen = viewport_matrix(viewport) * glam::DVec3::new(pos.x / pos.w, pos.y / pos.w, 1.0);
    Some(Pos2::new(screen.x as f32, screen.y as f32))
}

/// Calculates 3d world coordinates from 2d screen coordinates
pub(crate) fn screen_to_world(viewport: Rect, mat: DMat4, pos: Pos2, z: f64) -> DVec3 {
    let x = (((pos.x - viewport.min.x) / viewport.width()) * 2.0 - 1.0) as f64;
    let y = (((pos.y - viewport.min.y) / viewport.height()) * 2.0 - 1.0) as f64;

    let mut world_pos = mat * DVec4::new(x, -y, z, 1.0);

    // w is zero when far plane is set to infinity
    if world_pos.w.abs() < 1e-7 {
        world_pos.w = 1e-7;
    }

    world_pos /= world_pos.w;

    world_pos.xyz()
}
