//! Unified `TextBackend` trait + `TextRun` input type for all text engines in this crate.
//!
//! # Design
//! The goal is a single swap-in point so callers (wgpu-widgets, nodes-view) can switch
//! between the glyphon atlas backend (`GlyphonBackend`) and a future 3-D mesh backend
//! without changing their frame loop.  Every backend exposes the same three-step protocol:
//!
//! 1. [`TextBackend::prepare`] — convert `TextRun` slices into internal GPU state.
//! 2. [`TextBackend::update_view_proj`] — upload the camera matrix.
//! 3. [`TextBackend::render`] — record draw calls into the active render pass.
//!
//! `GlyphonBackend` (defined in this module) wraps the existing [`TextEngine`] and
//! implements this trait, so the engine API is unchanged and existing consumers continue
//! to work without modification.

use glam::Mat4;
use wgpu::{Device, Queue, RenderPass};

use crate::labels::PlaneLabel;
use crate::TextEngine;

// ─────────────────────────────────────────────────────────────────────────────
// TextRun — unified per-label input
// ─────────────────────────────────────────────────────────────────────────────

/// One unit of text to be rendered on a widget/graph plane surface.
///
/// `TextRun` is a **superset** of both `PlaneLabel` (wgpu-text) and
/// `CollectedPlaneText` (wgpu-widgets): the fields are identical so conversion
/// between them is lossless and trivial.  Two backend-specific **hints** extend it:
///
/// * `raster_scale` — glyphon atlas bake resolution multiplier; mesh backends ignore it.
/// * `extrude_depth` — extrusion depth for 3-D mesh backends; glyphon ignores it.
///
/// Callers can use `TextRun { text: ..., ..Default::default() }` for convenience.
#[derive(Debug, Clone)]
pub struct TextRun {
    /// World-from-surface matrix.  Defines where the local pixel coordinate system
    /// sits in world space (origin = top-left of the widget/node surface).
    pub surface_xfm: Mat4,
    /// Layout box in surface-local pixels `[x, y, width, height]` (origin top-left, +y down).
    pub local_rect: [f32; 4],
    /// UTF-8 text to rasterise.
    pub text: String,
    /// Font size in surface-local pixels.
    pub font_size: f32,
    /// Line height in surface-local pixels (usually `font_size + small_gap`).
    pub line_height: f32,
    /// Linear RGBA colour `[r, g, b, a]` in 0-1 range.
    pub color: [f32; 4],
    /// Glyph bake resolution multiplier (1.0 = layout size; >1.0 when the camera zooms in
    /// on the plane so glyphs stay crisp).  Atlas backends use this; mesh backends ignore it.
    pub raster_scale: f32,
    /// May the text wrap inside `local_rect`? See [`PlaneLabel::wrap`] - a one-line strip must
    /// say `false`, or it breaks mid-word when the box is narrow.
    pub wrap: bool,
    /// Surface-local clip rectangle `[min_x, min_y, max_x, max_y]`.  Fragments outside are
    /// discarded by the plane shader.  Use `TextRun::NO_CLIP` for unclipped text.
    pub clip_rect: [f32; 4],
    /// Extrusion depth hint for 3-D mesh backends (metres / graph units).  Glyphon ignores it.
    pub extrude_depth: f32,
}

impl TextRun {
    /// Effectively-infinite clip rect — no visual culling applied.
    pub const NO_CLIP: [f32; 4] = [-1.0e6, -1.0e6, 1.0e6, 1.0e6];
}

impl Default for TextRun {
    fn default() -> Self {
        Self {
            surface_xfm: Mat4::IDENTITY,
            local_rect: [0.0, 0.0, 100.0, 20.0],
            wrap: true,
            text: String::new(),
            font_size: 12.0,
            line_height: 14.0,
            color: [0.92, 0.93, 0.95, 1.0],
            raster_scale: 1.0,
            clip_rect: Self::NO_CLIP,
            extrude_depth: 0.0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DepthMode — how each backend composites against the depth buffer
// ─────────────────────────────────────────────────────────────────────────────

/// Controls depth testing and writing for a text backend.
///
/// Backends declare which mode they operate in via [`TextBackend::depth_mode`].  Callers
/// use this to decide pass ordering or pipeline setup:
///
/// * [`DepthMode::Overlay`] — `depth_compare = Always`, no depth write.  Text paints on
///   top in submission order (painter's algorithm).  Used by the glyphon plane pipeline
///   for HUD overlays and node labels that must not z-fight coplanar chrome.
/// * [`DepthMode::Depth`] — `depth_compare = Less`, depth write enabled.  Text participates
///   in the depth buffer as a true 3-D object.  Intended for the future extruded-mesh
///   backend so node labels sit behind panels that occlude them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthMode {
    /// Painter-order overlay: `depth_compare = Always`, depth write disabled.
    Overlay,
    /// True 3-D ordering: `depth_compare = Less`, depth write enabled.
    Depth,
}

// ─────────────────────────────────────────────────────────────────────────────
// TextBackend trait
// ─────────────────────────────────────────────────────────────────────────────

/// Pluggable text rendering backend for plane-locked widget / graph labels.
///
/// Implementors convert a `&[TextRun]` slice into GPU draw state inside `prepare`,
/// then issue draws in `render`.  Two implementations are provided in this crate:
///
/// - [`GlyphonBackend`] — atlas-based anti-aliased 2-D path (wraps [`TextEngine`]).
/// - *(planned)* `MeshBackend` — extruded 3-D glyph meshes, added in Phase 2.
///
/// # Frame lifecycle
///
/// ```ignore
/// backend.prepare(device, queue, &runs)?;
/// backend.update_view_proj(queue, view_proj);
/// // inside active render pass:
/// backend.render(&mut pass);
/// ```
pub trait TextBackend {
    /// Shape, rasterise, and upload all text for this frame.
    ///
    /// Called once per frame before `update_view_proj` and `render`.  The backend
    /// may allocate GPU buffers, rasterise glyphs into an atlas, or upload mesh
    /// vertex data.  Returns `Err(String)` on GPU or shaping failure.
    fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        runs: &[TextRun],
    ) -> Result<(), String>;

    /// Upload the current view-projection matrix to the GPU uniform buffer.
    ///
    /// Must be called after `prepare` and before `render` each frame, even if
    /// the matrix has not changed (some backends write unconditionally).
    fn update_view_proj(&mut self, queue: &Queue, view_proj: Mat4);

    /// Record draw calls for all prepared text instances into `pass`.
    ///
    /// The `'a` lifetime ties instance-buffer slices (vertex buffers, bind groups)
    /// to the render pass lifetime so they remain valid through the draw call.
    fn render<'a>(&'a self, pass: &mut RenderPass<'a>);

    /// Number of drawable glyph instances this frame.
    ///
    /// Callers may skip render pass setup entirely when this returns `0`,
    /// avoiding a wgpu validation error on backends that reject empty draws.
    fn instance_count(&self) -> u32;

    /// Depth compositing mode this backend operates in.
    ///
    /// Callers use this to decide pass ordering: `Overlay` backends must draw
    /// after all opaque geometry; `Depth` backends can interleave with it.
    fn depth_mode(&self) -> DepthMode;
}

// ─────────────────────────────────────────────────────────────────────────────
// GlyphonBackend — TextBackend impl that borrows a TextEngine
// ─────────────────────────────────────────────────────────────────────────────

/// Atlas-based [`TextBackend`] that borrows an existing [`TextEngine`].
///
/// This is the default backend for wgpu-widgets and nodes-view.  It wraps the
/// glyphon plane pipeline (atlas quads, `depth_compare = Always`, no depth write)
/// and converts each [`TextRun`] into a [`PlaneLabel`] in a reused scratch buffer
/// so no allocation occurs between frames with a stable label count.
///
/// The backend **borrows** rather than owns the engine so the same `TextEngine`
/// can also be used for screen-space labels (HUD text, debug overlays) through
/// the original `&mut` handle after this adapter drops.
///
/// # Example
/// ```ignore
/// let mut backend = GlyphonBackend::new(&mut engine);
/// backend.prepare(device, queue, &runs)?;
/// backend.update_view_proj(queue, view_proj);
/// backend.render(&mut pass);
/// ```
pub struct GlyphonBackend<'a> {
    engine: &'a mut TextEngine,
    /// Scratch buffer reused across frames to avoid per-frame heap allocation.
    scratch: Vec<PlaneLabel>,
}

impl<'a> GlyphonBackend<'a> {
    /// Wrap a [`TextEngine`] for one or more rendering frames.
    pub fn new(engine: &'a mut TextEngine) -> Self {
        Self {
            engine,
            scratch: Vec::new(),
        }
    }
}

impl<'a> TextBackend for GlyphonBackend<'a> {
    /// Convert `runs` → [`PlaneLabel`] slice and call `prepare_plane` on the engine.
    ///
    /// The mapping is field-for-field because `TextRun` was designed as a superset of
    /// `PlaneLabel`; `extrude_depth` has no equivalent and is silently ignored here.
    fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        runs: &[TextRun],
    ) -> Result<(), String> {
        self.scratch.clear();
        self.scratch.reserve(runs.len());
        for r in runs {
            self.scratch.push(PlaneLabel {
                surface_xfm: r.surface_xfm,
                local_rect: r.local_rect,
                text: r.text.clone(),
                font_size: r.font_size,
                line_height: r.line_height,
                wrap: r.wrap,
                color: r.color,
                raster_scale: r.raster_scale,
                clip_rect: r.clip_rect,
            });
        }
        self.engine.prepare_plane(device, queue, &self.scratch)
    }

    fn update_view_proj(&mut self, queue: &Queue, view_proj: Mat4) {
        self.engine.update_plane_view_proj(queue, view_proj);
    }

    fn render<'p>(&'p self, pass: &mut RenderPass<'p>) {
        self.engine.render_plane(pass);
    }

    fn instance_count(&self) -> u32 {
        self.engine.plane_instance_count()
    }

    /// Glyphon plane pipeline uses `depth_compare = Always` with no depth write —
    /// confirmed in `plane.rs` `PlaneTextRenderer::new`: `depth_write_enabled: Some(false)`,
    /// `depth_compare: Some(CompareFunction::Always)`.  Pure painter-order overlay.
    fn depth_mode(&self) -> DepthMode {
        DepthMode::Overlay
    }
}
