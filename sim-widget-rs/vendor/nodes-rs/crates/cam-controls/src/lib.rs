//! Universal camera navigation (host-agnostic, RH Y-up, [`glam`]).
//!
//! # Crates
//! - **`cam-viewport`** — pixel / NDC / ray math
//! - **`cam-controls`** (this crate) — rigs, fit, inertia, transitions
//!
//! Platform adapters (`cam-controls-winit`, `cam-controls-egui`) and the full
//! `Controls` input layer are part of the same **0.0.1** release (see `CAM_TODO.md`).

pub mod controller;
pub mod controls;
pub mod pose;
pub mod settings;

mod convention;
mod dynamics;
mod fit;
mod orientation;
mod projection;
pub mod rig;

pub use controller::{CameraController, CameraKind, FpsFly, HoudiniOrbit, QuatOrbit, SpaceFlight};
pub use controls::{CameraIntent, ControlLock, Controls};
pub use convention::{DEFAULT_OFFSET_AXIS, WORLD_UP};
pub use dynamics::{
    FlyInertia, InertiaState, TransitionState, damp_factor, damp_scalar, damp_vec3,
};
pub use fit::{aabb_center_radius, distance_to_frame_sphere};
pub use orientation::{
    DEFAULT_PITCH_LIMIT, OrientationMode, apply_orbit_delta, orientation_from_yaw_pitch,
    yaw_pitch_from_orientation,
};
pub use pose::CameraPose;
pub use projection::PerspectiveSettings;
pub use rig::{OrbitPose, OrbitRig};
pub use settings::{CameraNavigationSettings, InertiaSettings, MouseSteer, ZoomMode};
