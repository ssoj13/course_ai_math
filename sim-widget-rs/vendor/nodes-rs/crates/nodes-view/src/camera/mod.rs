//! Camera and graph working plane.
//!
//! Screen ↔ graph mapping lives on [`Camera`]. The **visible frustum region** on the plane
//! is computed in [`crate::viewport`] ([`crate::visible_graph_uv_rect`]), not here.

pub mod flat;
pub mod graph_plane;

pub use cam_controls::{CameraIntent, CameraKind};
pub use cam_viewport::ViewportSize;
pub use flat::{Camera, OrbitMode};
pub use graph_plane::{GraphPlane, rotate_surface_about};
