//! Frontend-agnostic camera navigation: map a decoded per-frame [`NavInput`] onto
//! the shared [`Camera`], so EVERY frontend (the winit `nodes-editor`, the embeddable
//! egui `nodes-egui` viewport, and any future host) drives the camera through ONE
//! implementation instead of each duplicating the input->camera mapping.
//!
//! Split of responsibilities (the systematic seam):
//! - **Frontend** (winit / egui): read its own raw input, decide bindings (which
//!   button orbits vs pans vs zooms, whether flight is engaged), and its own
//!   px->value conversions (pan px->plane-UV, orbit px->radians, zoom factor). It
//!   also owns platform-only concerns like cursor grab. It fills a [`NavInput`].
//! - **This layer** (shared): owns HOW each decoded nav maps to the camera — the
//!   flight intent sequence (Look/Thrust/Roll/Boost), orbit/pan/zoom dispatch, and
//!   the once-per-frame `update_dynamics`. Single source of truth for camera
//!   manipulation, reused by both frontends.

use crate::Camera;
use cam_controls::CameraIntent;
use cam_viewport::ViewportSize;
use glam::Vec2;

/// Body-relative flight axes for one frame. `forward`/`right`/`up` are signed
/// thrust magnitudes (e.g. `W-S`, `D-A`, `Space-C`); `roll` is `E-Q`; `boost` is a
/// speed modifier; `look_*` are the accumulated mouse-look deltas in radians.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlyAxes {
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub roll: f32,
    pub boost: bool,
    pub look_dyaw: f32,
    pub look_dpitch: f32,
}

/// One frame of decoded navigation. Each field is `Some` only for the nav the
/// frontend's bindings triggered this frame. The frontend keeps its raw-input
/// reading + px->value conversions; this struct is the frontend-neutral hand-off.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NavInput {
    /// Active flight (a fly backend is engaged): thrust/roll/boost + mouse-look.
    pub fly: Option<FlyAxes>,
    /// Orbit tumble in radians `(yaw, pitch)`.
    pub orbit: Option<(f32, f32)>,
    /// Pan on the graph plane, in plane-UV units.
    pub pan_uv: Option<Vec2>,
    /// Zoom toward `cursor` (viewport px) by `factor` (`>1` zooms in).
    pub zoom: Option<(Vec2, f32)>,
}

impl NavInput {
    /// True when nothing this frame moves the camera (still call [`apply_nav`] so
    /// inertia/dynamics keep coasting).
    pub fn is_idle(&self) -> bool {
        self.fly.is_none() && self.orbit.is_none() && self.pan_uv.is_none() && self.zoom.is_none()
    }
}

/// Apply ONE frame of flight input to `cam` (mouse-look + thrust/roll/boost), the
/// intent sequence shared by every frontend's fly path. Does NOT advance dynamics
/// — the caller owns `update_dynamics` (so a frontend that already ticks it once
/// per frame, like the winit editor, can reuse this without double-integrating).
pub fn apply_fly(cam: &mut Camera, f: &FlyAxes, dt: f32, vp: ViewportSize) {
    if f.look_dyaw != 0.0 || f.look_dpitch != 0.0 {
        cam.apply_intent(
            CameraIntent::Look {
                dyaw: f.look_dyaw,
                dpitch: f.look_dpitch,
            },
            dt,
            vp,
        );
    }
    cam.apply_intent(
        CameraIntent::Thrust {
            forward: f.forward,
            right: f.right,
            up: f.up,
        },
        dt,
        vp,
    );
    cam.apply_intent(CameraIntent::Roll { d: f.roll }, dt, vp);
    cam.apply_intent(CameraIntent::Boost(f.boost), dt, vp);
}

/// Apply one frame of navigation to `cam`, then advance its dynamics/inertia once.
/// `vp_px` is the viewport size in pixels (width, height). Call every frame even
/// with an idle [`NavInput`] so fly momentum / pan coast decay smoothly.
pub fn apply_nav(cam: &mut Camera, input: &NavInput, dt: f32, vp_px: Vec2) {
    let vs = ViewportSize::new(vp_px.x.max(1.0) as u32, vp_px.y.max(1.0) as u32);
    if let Some(f) = &input.fly {
        apply_fly(cam, f, dt, vs);
    }
    if let Some((yaw, pitch)) = input.orbit {
        cam.orbit(yaw, pitch);
    }
    if let Some(uv) = input.pan_uv {
        cam.pan_on_plane(uv);
    }
    if let Some((cursor, factor)) = input.zoom {
        cam.zoom_toward(cursor, factor, vp_px);
    }

    cam.update_dynamics(dt);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp() -> Vec2 {
        Vec2::new(800.0, 600.0)
    }

    /// A pan NavInput moves the camera; an idle one leaves it put (modulo inertia).
    #[test]
    fn pan_moves_camera_idle_does_not() {
        let mut cam = Camera::default();
        let before = cam.view_proj(800.0, 600.0);

        apply_nav(
            &mut cam,
            &NavInput {
                pan_uv: Some(Vec2::new(10.0, 0.0)),
                ..Default::default()
            },
            1.0 / 60.0,
            vp(),
        );
        assert_ne!(
            before,
            cam.view_proj(800.0, 600.0),
            "pan must move the camera view"
        );

        // Idle: an at-rest orbit camera should not drift.
        let mut cam2 = Camera::default();
        let a = cam2.view_proj(800.0, 600.0);
        apply_nav(&mut cam2, &NavInput::default(), 1.0 / 60.0, vp());
        assert_eq!(
            a,
            cam2.view_proj(800.0, 600.0),
            "idle nav must not move an at-rest camera"
        );
    }

    /// Zoom toward the center changes the camera distance.
    #[test]
    fn zoom_changes_distance() {
        let mut cam = Camera::default();
        let d0 = cam.distance();
        apply_nav(
            &mut cam,
            &NavInput {
                zoom: Some((Vec2::new(400.0, 300.0), 1.2)),
                ..Default::default()
            },
            1.0 / 60.0,
            vp(),
        );
        assert_ne!(d0, cam.distance(), "zoom must change the dolly distance");
    }

    /// Flight thrust engages the Fps backend's motion (distance/pose changes).
    #[test]
    fn fly_thrust_moves_the_eye() {
        let mut cam = Camera::default();
        cam.set_kind(crate::CameraKind::Fps);
        let before = cam.distance();
        // Several frames of forward thrust so inertia integrates a visible move.
        for _ in 0..30 {
            apply_nav(
                &mut cam,
                &NavInput {
                    fly: Some(FlyAxes {
                        forward: 1.0,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                1.0 / 60.0,
                vp(),
            );
        }
        assert_ne!(before, cam.distance(), "forward thrust must fly the camera");
    }

    #[test]
    fn is_idle_reports_correctly() {
        assert!(NavInput::default().is_idle());
        assert!(
            !NavInput {
                orbit: Some((0.1, 0.0)),
                ..Default::default()
            }
            .is_idle()
        );
    }
}
