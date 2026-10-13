//! World-space gizmos (axes at origin, future helpers).
//!
//! # What
//!
//! Debug/reference geometry in **world** coordinates, separate from graph-UV line passes
//! (grid, wires on the plane).
//!
//! # Why
//!
//! The plane line shader maps graph `(u,v)` → world on `z = 0`; a **Z** axis must stick out
//! of the board, so axes use [`WorldLinePipeline`] in this module.
//!
//! # Where used
//!
//! - [`crate::engine::GraphRenderer::render_scene`] — after the background grid pass.

mod world_axes;
mod world_line_pipeline;

pub use world_axes::{WorldAxesConfig, build_world_axes_vertices};
pub use world_line_pipeline::{
    WorldLinePipeline, WorldLineVertex, draw_world_lines, world_line_buffer,
};
