//! # Viewport ↔ graph plane
//!
//! Answers one question for the whole renderer: **which part of the graph working plane
//! is visible through the current orthographic viewport?**
//!
//! ## What
//!
//! - [`GraphUvRect`] — axis-aligned bounds in graph `(u, v)` (same space as `view_pos`,
//!   layout pins, and node boxes).
//! - [`visible_graph_uv_rect`] — computes that rectangle as **frustum ∩ plane** (not
//!   “nodes on screen” and not an arbitrary world AABB).
//!
//! ## Why
//!
//! Several features need the same robust region:
//!
//! | Problem | Without a shared API |
//! |---------|---------------------|
//! | Background grid | Corner-only unproject + fallback to `target` collapses to a tiny patch at `(0,0)` when orbiting |
//! | Minimap view rect | Would duplicate fragile math |
//! | Label/shape culling | Would drift from grid; shares [`visible_graph_uv_rect`] with grid + [`crate::engine::shape_scene`] / [`crate::engine::text_labels`] |
//!
//! Perimeter sampling + [`Camera::try_screen_to_graph`] (no target fallback) fixes grazing
//! views; an orthographic footprint fallback covers edge-on cameras.
//!
//! ## Where used
//!
//! | Crate / module | API | Purpose |
//! |----------------|-----|---------|
//! | [`crate::engine::grid`] | [`visible_graph_uv_rect`] | Draw grid lines only inside the visible frustum |
//! | [`crate::engine::GraphRenderer::render_scene`] | same | Pass `visible` into shape + label builders |
//! | [`crate::camera::Camera::visible_graph_uv_bounds`] | wraps `visible_graph_uv_rect` | Tuple `(min, max)` for legacy call sites |
//! | `nodes-editor` / [`minimap`](../../nodes-editor/src/panels/minimap.rs) | `visible_graph_uv_bounds` | Yellow “what you see” rectangle on the minimap |
//!
//! **Prefer** [`visible_graph_uv_rect`] when you need [`GraphUvRect::pad`], [`GraphUvRect::union`],
//! or degeneracy checks. Use [`Camera::visible_graph_uv_bounds`] only when a `(Vec2, Vec2)` is enough.
//!
//! ## Related
//!
//! - Input unproject: [`Camera::screen_to_graph`] (fallback) vs [`Camera::try_screen_to_graph`] (strict).
//! - Plane definition: [`crate::GraphPlane`].

mod graph_uv_rect;
mod node_visibility;
mod visible_region;

pub use graph_uv_rect::GraphUvRect;
pub use node_visibility::{
    Frustum, NodeLod, VisibilityConfig, VisibleNodes, compute_visible_nodes,
};
pub use visible_region::visible_graph_uv_rect;
