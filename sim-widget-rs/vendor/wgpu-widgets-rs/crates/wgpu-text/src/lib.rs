//! Generic wgpu text: glyphon screen labels + plane-locked atlas quads.
//!
//! **WHAT:** [`TextEngine`] renders screen and plane text; [`GlyphAtlas`] exposes
//! the atlas boundary for specialized glyph-quad renderers.
//!
//! **WHY:** generic text remains independent of node/grid HUD buffers and culling.
//!
//! **SOURCE:** HUD-specific code formerly behind a `hud` feature was extracted to
//! the sibling `wgpu-hud` crate; this crate has no `hud` feature.
//!
//! **CONSUMER:** UI and overlay crates use [`TextEngine::new`]; `wgpu-hud` consumes
//! [`GlyphAtlas`] through its public API.

mod atlas;
mod backend;
mod color;
mod engine;
mod labels;
mod mipgen;
mod plane;
mod screen;

pub use backend::{DepthMode, GlyphonBackend, TextBackend, TextRun};
pub use color::{glyphon_to_rgba, rgba_to_glyphon};
pub use engine::{caret_index_for_x, measure_caret_offsets, TextEngine};
pub use labels::{quantize_plane_raster_scale, PlaneLabel, ScreenLabel};

// Public glyph-atlas boundary. `GlyphAtlas` is the shared GPU resource that any
// glyph-quad renderer bakes into and renders from; it is exported so external
// crates (e.g. `wgpu-hud` billboards) can reuse the exact rasterization + bind
// contract instead of duplicating it. `GlyphMetrics` is the value it returns.
pub use atlas::{GlyphAtlas, GlyphMetrics, TextFilterQuality};

// Second text engine (extruded 3D mesh) behind the `engine-mesh3d` feature. Glyphon stays
// the default; this is opt-in so 2D-only consumers avoid the tessellation stack.
#[cfg(feature = "engine-mesh3d")]
mod mesh_backend;
#[cfg(feature = "engine-mesh3d")]
pub use mesh_backend::MeshBackend;
/// Re-export of the underlying 3D mesh crate so consumers can load fonts / tune params
/// without taking a separate dependency.
#[cfg(feature = "engine-mesh3d")]
pub use wgpu_text_3d;
