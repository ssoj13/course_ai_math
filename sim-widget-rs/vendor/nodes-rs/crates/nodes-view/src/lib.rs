//! Wgpu-first graph viewport renderer (public API — no egui types).
//!
//! Host applications (`nodes-editor`, `mermaid-rs` diagram mode, examples) integrate via
//! [`GraphRenderer`] + [`Camera`]. egui wiring lives in the `nodes-egui` crate (`GraphGpuBridge`).
//!
//! ## Stack map
//!
//! ```text
//! nodes-core     Subnet, LayoutCache, pins, auto_layout
//! nodes-view     GraphRenderer::render / pick  (this crate)
//! nodes-egui     GraphViewportState::ui, present → egui::Image
//! nodes-editor   GraphEditorApp, dock, demo graph
//! ```
//!
//! See [`engine`] for offscreen vs external render targets and the per-frame GPU passes.
//!
//! ## Coordinates (node editor)
//!
//! - **Graph UV** (`view_pos`): top-left origin, **+u** right, **+v** down — layout, pick, wires.
//! - **World**: default vertical **XY board** at `z = 0`, **Y-up**; camera in front (`−Z`) via [`GraphPlane::default_graph`].
//! - **Screen**: [`Camera::screen_to_graph`] / [`Camera::graph_to_screen`] for input and labels.
//! - **Visible region** ([`viewport`] module): [`visible_graph_uv_rect`], [`GraphUvRect`] —
//!   frustum ∩ graph plane; used by grid, minimap, and any on-screen culling.
//!   See [`viewport`] module docs for what / why / call-site table.
//! - Node title/body/toggle regions: [`nodes_core::node_regions`].
//!
//! ## Layout cache
//!
//! [`sync_layout_cache`] wraps [`nodes_core::compute_static_layout`]. Pass the result in
//! [`RenderParams::layout`] when the host already computed layout this frame (editor viewport).

pub mod camera;
pub mod camera_control;
pub mod engine;
pub mod gizmo;
pub mod layout_cache;
pub mod nav;
pub mod viewport;
pub use camera::{Camera, GraphPlane, OrbitMode, rotate_surface_about};
pub use camera_control::{CamControl, FOV_FLAT, FOV_ORBIT_TRIGGER, FOV_PERSP, LensMode};
pub use nav::{FlyAxes, NavInput, apply_fly, apply_nav};
// `CameraPose` is re-exported so a host can SNAPSHOT the camera (see `nodes_editor::persist`)
// without taking a direct `cam-controls` dependency — nodes-view is the camera facade.
pub use cam_controls::{
    CameraIntent, CameraKind, CameraNavigationSettings, CameraPose, InertiaSettings,
};
pub use cam_viewport::ViewportSize;
pub use engine::{
    GraphBackend, GraphRenderer, GraphRendererDesc, GraphTextRenderer, OverlayTargets, PickTarget,
    RenderError, RenderParams, WirePreview,
};
pub use layout_cache::sync_layout_cache;
pub use nodes_core::LayoutCache;
pub use viewport::{
    Frustum, GraphUvRect, NodeLod, VisibilityConfig, VisibleNodes, compute_visible_nodes,
    visible_graph_uv_rect,
};
pub use wgpu_node::{NodeMeshParams, NodeShape, NodeStyle};
pub use wgpu_text::{PlaneLabel, ScreenLabel, TextEngine, TextFilterQuality};
