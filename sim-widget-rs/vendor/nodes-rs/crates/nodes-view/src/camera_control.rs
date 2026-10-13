//! Host-agnostic camera GESTURE policy — the SINGLE "camera brain" every frontend drives.
//!
//! Both the winit `nodes-editor` and the embeddable egui `nodes-egui` viewport (and any future
//! host) used to carry their OWN copy of the persp↔ortho lens logic and orbit handling, which
//! drifted apart and grew incorrect in one or the other (wild orbit at near-ortho, orbit stuck as
//! free-look after a flight, lens flatten leaking into flight). [`CamControl`] lifts that policy
//! into one place so there is a single implementation to reason about and fix.
//!
//! # What it owns
//! - The **lens intent** ([`LensMode`]): flat near-ortho "2D map" vs opened 3D perspective, plus
//!   the adjustable perspective FOV (`persp_fov`, driven by a dolly-zoom gesture).
//! - The **per-frame FOV ease** ([`CamControl::step`]) — THE single writer of `Camera::fov_y`, with
//!   a pure top-down GATE so a flatten never renders a near-ortho lens on a still-tilted (edge-on)
//!   plane, and self-corrects if the user orbits away mid-flatten.
//! - The **orbit gesture** ([`CamControl::orbit`]): ensure the orbit backend, open perspective, and
//!   scale the angular step by `tan(fov/2)` so a pixel of drag maps to the SAME on-screen sweep at
//!   any FOV (kills the near-ortho "wild spin").
//!
//! # What it does NOT own (stays per-host)
//! Platform I/O (cursor grab, input decode) and release-inertia/coast — those legitimately differ
//! between a winit window and an egui panel, so each host keeps them.

use glam::Vec2;

use crate::Camera;

/// The resting near-orthographic lens (~2°) that reads as a flat 2D node graph.
/// Matches `camera::DEFAULT_FOV_Y`.
pub const FOV_FLAT: f32 = 0.034_906_585; // 2.0°
/// The perspective lens (60°) the view opens to when you start looking around in 3D.
pub const FOV_PERSP: f32 = std::f32::consts::FRAC_PI_3; // 60°
/// Upper clamp for the manually dolly-zoomed perspective FOV (~114°), kept under the camera's hard
/// `FOV_Y_MAX` (120°) so `persp_fov` is always a reachable ease target.
pub const FOV_PERSP_MAX: f32 = 2.0;
/// Below this FOV the view "reads as flat". Retained for API compatibility with older consumers
/// that imported it from `nodes_view`; the lens intent is now explicit via [`LensMode`].
pub const FOV_ORBIT_TRIGGER: f32 = 0.261_799_4; // 15°
/// Default FOV-ease duration (seconds) for hosts without their own animation config.
pub const DEFAULT_FOV_ANIM_SECS: f32 = 0.35;

/// Reaches ~98% of the remaining FOV gap over one `anim_secs` (`rate = K / τ`).
const FOV_EASE_K: f32 = 4.0;
/// `forward · plane_normal` below this counts as settled top-down (the flatten gate).
const TOP_DOWN_DOT: f32 = -0.999;

/// The lens INTENT — the single source of truth for where the FOV wants to be.
///
/// `Flat` = resting near-ortho 2D-map look; `Perspective` = opened 3D tumble view. Board orbit /
/// fly set `Perspective`; align/`H` (settled top-down) sets `Flat`. Because orbit/fly force
/// `Perspective`, a flatten can never run mid-flight — impossible by construction, not by a guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LensMode {
    Flat,
    Perspective,
}

/// The shared camera gesture brain. Hold ONE next to a [`Camera`] in the host's viewport state and
/// route the lens/orbit gestures through it; the FOV itself lives on the camera, so this stays a
/// thin, `Copy` policy object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamControl {
    lens: LensMode,
    persp_fov: f32,
}

impl Default for CamControl {
    fn default() -> Self {
        Self {
            lens: LensMode::Flat,
            persp_fov: FOV_PERSP,
        }
    }
}

impl CamControl {
    /// Fresh brain at the flat resting lens.
    pub fn new() -> Self {
        Self::default()
    }

    /// Current lens intent.
    pub fn lens(&self) -> LensMode {
        self.lens
    }

    /// Current perspective FOV target (radians).
    pub fn persp_fov(&self) -> f32 {
        self.persp_fov
    }

    /// Open the lens to perspective — the shared "start looking around in 3D" intent (orbit press,
    /// fly-engage). The actual FOV eases up in [`Self::step`]; this only sets the intent.
    pub fn open_perspective(&mut self) {
        self.lens = LensMode::Perspective;
    }

    /// Return to the flat 2D lens (align / `H`). Deferred: [`Self::step`]'s top-down gate holds the
    /// perspective FOV until the camera has actually settled top-down, so no distorted intermediate.
    pub fn request_flat(&mut self) {
        self.lens = LensMode::Flat;
    }

    /// Set the perspective FOV target absolutely (clamped to a reachable range).
    pub fn set_persp_fov(&mut self, fov: f32) {
        self.persp_fov = fov.clamp(FOV_FLAT, FOV_PERSP_MAX);
    }

    /// Nudge the perspective FOV target (Ctrl+Alt dolly-zoom drag). Drives the ease TARGET, not
    /// `fov_y` directly, so a manual dolly-zoom sticks instead of being pulled back by [`Self::step`].
    pub fn nudge_persp_fov(&mut self, delta: f32) {
        self.set_persp_fov(self.persp_fov + delta);
    }

    /// Ensure the Houdini orbit backend before a board orbit / framing gesture (a fly backend would
    /// free-look instead of tumble, and RMB-fly release keeps the fly backend to coast). No-op when
    /// already orbiting. The actual switch + pivot-reseat lives in [`Camera::adopt_orbit_backend`].
    pub fn ensure_orbit(&self, cam: &mut Camera) {
        cam.adopt_orbit_backend();
    }

    /// Apply a board ORBIT from a pixel drag `delta_px` at `sensitivity` (rad/px).
    ///
    /// Ensures the orbit backend, opens perspective, and scales the angular step by
    /// `tan(fov/2) / tan(FOV_PERSP/2)` so the on-screen sweep per pixel is identical at any FOV — the
    /// fixed sensitivity otherwise sweeps a huge fraction of the tiny near-ortho cone (the "wild
    /// spin"). Returns the applied `(yaw, pitch)` so a host can seed its own release-inertia coast.
    pub fn orbit(&mut self, cam: &mut Camera, delta_px: Vec2, sensitivity: f32) -> Vec2 {
        cam.adopt_orbit_backend();
        self.open_perspective();
        let fov_scale = (cam.fov_y() * 0.5).tan() / (FOV_PERSP * 0.5).tan();
        let yaw = -delta_px.x * sensitivity * fov_scale;
        let pitch = delta_px.y * sensitivity * fov_scale;
        cam.orbit(yaw, pitch);
        Vec2::new(yaw, pitch)
    }

    /// The FOV (radians) the current lens intent wants RIGHT NOW.
    fn target_fov(&self, cam: &Camera) -> f32 {
        match self.lens {
            LensMode::Perspective => self.persp_fov,
            LensMode::Flat => {
                let top_down = cam.forward().dot(cam.plane.normal) < TOP_DOWN_DOT;
                if top_down { FOV_FLAT } else { cam.fov_y() }
            }
        }
    }

    /// Ease `fov_y` toward the lens target once per frame through `Camera::adjust_fov` (which
    /// dolly-compensates so the framed subject keeps its apparent size). THE single writer of the
    /// lens. Frame-rate-independent exponential approach; snaps instantly when `anim_enabled` is
    /// false; stops cleanly within an epsilon. `anim_secs` sets the ease time-constant.
    pub fn step(&mut self, cam: &mut Camera, dt: f32, anim_enabled: bool, anim_secs: f32) {
        let diff = self.target_fov(cam) - cam.fov_y();
        if diff.abs() < 1.0e-4 {
            return;
        }
        if !anim_enabled {
            cam.adjust_fov(diff);
            return;
        }
        let rate = FOV_EASE_K / anim_secs.max(1.0e-3);
        let t = (1.0 - (-rate * dt).exp()).clamp(0.0, 1.0);
        cam.adjust_fov(diff * t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Orbit from the flat lens opens perspective (mode flips + FOV eases up).
    #[test]
    fn orbit_opens_perspective_and_scales_by_fov() {
        let mut cam = Camera::default();
        let flat_fov = cam.fov_y();
        let mut cc = CamControl::new();
        // A drag at the flat FOV must apply a MUCH smaller angular step than the raw sensitivity
        // (screen-consistent orbit), and must flip the lens intent to perspective.
        let applied = cc.orbit(&mut cam, Vec2::new(100.0, 0.0), 0.004);
        assert_eq!(cc.lens(), LensMode::Perspective);
        let raw = 100.0 * 0.004;
        assert!(
            applied.x.abs() < raw * 0.2,
            "near-ortho orbit must be scaled down, got {}",
            applied.x
        );
        let _ = flat_fov;
    }

    /// The lens eases to 60° in perspective mode and converges.
    #[test]
    fn step_eases_to_perspective() {
        let mut cam = Camera::default();
        let mut cc = CamControl::new();
        cc.open_perspective();
        for _ in 0..240 {
            cc.step(&mut cam, 1.0 / 60.0, true, DEFAULT_FOV_ANIM_SECS);
        }
        assert!(
            (cam.fov_y() - FOV_PERSP).abs() < 1.0e-3,
            "must land on 60°, got {}",
            cam.fov_y()
        );
    }

    /// Flat intent flattens the FOV once the camera is top-down; the default camera IS top-down.
    #[test]
    fn flat_flattens_when_top_down() {
        let mut cam = Camera::default();
        cam.adjust_fov(FOV_PERSP - cam.fov_y()); // open to perspective first
        let mut cc = CamControl::new();
        cc.request_flat();
        for _ in 0..240 {
            cc.step(&mut cam, 1.0 / 60.0, true, DEFAULT_FOV_ANIM_SECS);
        }
        assert!(
            (cam.fov_y() - FOV_FLAT).abs() < 1.0e-3,
            "top-down flat must return to 2°, got {}",
            cam.fov_y()
        );
    }

    /// `set_persp_fov` clamps into the reachable range.
    #[test]
    fn persp_fov_clamped() {
        let mut cc = CamControl::new();
        cc.set_persp_fov(10.0);
        assert!(cc.persp_fov() <= FOV_PERSP_MAX);
        cc.set_persp_fov(-1.0);
        assert!(cc.persp_fov() >= FOV_FLAT);
    }
}
