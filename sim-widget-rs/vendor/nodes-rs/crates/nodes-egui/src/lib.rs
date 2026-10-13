//! egui host bridge for [`nodes_view::GraphRenderer`].
//!
//! ## Typical frame (node editor)
//!
//! 1. `GraphEditorApp` / diagram host calls [`GraphViewportState::configure_wgpu_render_state`]
//!    once with `eframe::CreationContext::wgpu_render_state`.
//! 2. Each frame in the Graph dock tab: [`GraphViewportState::ui`] →
//!    [`nodes_view::GraphRenderer::resize`] + [`nodes_view::GraphRenderer::render`].
//! 3. [`GraphGpuBridge::present`] uploads the offscreen texture → [`egui::Image`].
//!
//! Interaction (pan, pick, wires) lives in `graph_viewport`; GPU drawing stays in `nodes-view`.

pub mod gpu_bridge;
pub mod graph_viewport;

pub use gpu_bridge::GraphGpuBridge;
pub use graph_viewport::{
    GraphViewportConfig, GraphViewportExtras, GraphViewportHost, GraphViewportState, SubnetHost,
    ViewportEditKind, ViewportEditPhase, WireDropActions,
};
