//! [`MeshBackend`] — a [`TextBackend`] that renders text as real extruded 3-D glyph
//! meshes via `wgpu-text-3d`.
//!
//! This is the second engine behind the `engine-mesh3d` feature. Unlike [`GlyphonBackend`]
//! (atlas quads, painter-order overlay), this backend produces depth-tested triangle
//! geometry: it declares [`DepthMode::Depth`] so a host that gives its render pass a depth
//! attachment gets correct occlusion between text and other 3-D content.
//!
//! [`GlyphonBackend`]: crate::GlyphonBackend
//!
//! # Coordinate mapping
//! Glyph outlines come out of the font in **font units** with **+y up** and the baseline at
//! `y = 0`. Widget/graph surfaces use **surface-local pixels** with **+y down**. Each
//! [`TextRun`] is therefore placed with the model matrix:
//!
//! ```text
//!   model = surface_xfm
//!         * translate(local_rect.x, local_rect.y + font_size, 0)   // baseline ≈ top + em
//!         * scale(s, -s, s)                                         // font units -> px, flip y
//!   where s = font_size / units_per_em
//! ```
//!
//! # Caching
//! Per-glyph tessellation is cached inside [`TextEngine3D`] (its internal `MeshCache`).
//! GPU buffers are rebuilt each frame via `clear()` + `upload()` — simple and correct;
//! Phase 5 may add per-run GPU-mesh reuse keyed by `(text, font_size)`.

use glam::{Mat4, Vec3};
use wgpu::{Device, Queue, RenderPass};

use wgpu_text_3d::tessellate::LyonTessellator;
use wgpu_text_3d::{
    ComposedMesher, ExtrudeParams, FontFace, LayoutOptions, Renderer3D, TextEngine3D,
};

use crate::backend::{DepthMode, TextBackend, TextRun};

/// Glyph-mesh cache capacity (distinct glyph meshes kept tessellated in memory).
const MESH_CACHE_CAP: usize = 1024;
/// Direction from the surface toward the light (matches the `hello_3d` demo).
const LIGHT_DIR: Vec3 = Vec3::new(0.4, 0.8, 0.6);
/// Ambient term — kept high so overlay text reads as nearly flat/bright, not dramatically lit.
const AMBIENT: f32 = 0.6;

/// A [`TextBackend`] backed by `wgpu-text-3d`'s extruded-mesh renderer.
///
/// Owns its font, mesher and [`Renderer3D`]. Construct once with the target color/depth
/// formats and font bytes, then drive it with the standard `prepare → update_view_proj →
/// render` lifecycle each frame.
pub struct MeshBackend {
    /// Parsed font face the runs are shaped against.
    face: FontFace,
    /// Units-per-em of `face`, cached for the font-units → pixels scale factor.
    upem: f32,
    /// Glyph tessellator + per-glyph mesh cache.
    engine: TextEngine3D<ComposedMesher<LyonTessellator>>,
    /// GPU pipeline + uploaded per-run meshes.
    renderer: Renderer3D,
}

impl MeshBackend {
    /// Build a mesh backend for a single font.
    ///
    /// * `color_format` / `depth_format` must match the render pass the host will draw into.
    ///   `depth_format` should be `Some(_)` for correct occlusion (this backend is
    ///   [`DepthMode::Depth`]); `None` disables depth state in the pipeline.
    /// * `font_bytes` is a TrueType/OpenType file (face index 0 is used).
    /// * `extrude_em` is the extrusion depth as a fraction of the em (e.g. `0.1`); it is
    ///   applied to every run.
    pub fn new(
        device: &Device,
        color_format: wgpu::TextureFormat,
        depth_format: Option<wgpu::TextureFormat>,
        font_bytes: &[u8],
        extrude_em: f32,
    ) -> Result<Self, String> {
        let face = FontFace::from_bytes(font_bytes.to_vec(), 0)
            .map_err(|e| format!("MeshBackend: parse font: {e}"))?;
        let upem = face.units_per_em();
        let mut engine = TextEngine3D::new(ComposedMesher::new(LyonTessellator::default()), MESH_CACHE_CAP);
        // Extrusion depth in font units; flat sides keep the silhouette crisp for UI text.
        engine.extrude_params = ExtrudeParams {
            depth: upem * extrude_em.max(0.0),
            smooth_angle: std::f32::consts::FRAC_PI_6,
            flat_sides: false,
        };
        let renderer = Renderer3D::new(device, color_format, depth_format, 1);
        Ok(Self {
            face,
            upem,
            engine,
            renderer,
        })
    }

    /// Model matrix placing a run's glyph mesh (font units, +y up) into its surface-local
    /// pixel box (+y down) and then into world space via `surface_xfm`.
    fn model_for(&self, run: &TextRun) -> Mat4 {
        let s = if self.upem > 0.0 {
            run.font_size / self.upem
        } else {
            0.0
        };
        let baseline = Vec3::new(run.local_rect[0], run.local_rect[1] + run.font_size, 0.0);
        // scale.y is negated to flip font's +y-up into the surface's +y-down convention.
        run.surface_xfm * Mat4::from_translation(baseline) * Mat4::from_scale(Vec3::new(s, -s, s))
    }
}

impl TextBackend for MeshBackend {
    fn prepare(&mut self, device: &Device, _queue: &Queue, runs: &[TextRun]) -> Result<(), String> {
        // Simple correct strategy: rebuild the GPU mesh set each frame. Glyph tessellation
        // is still cached inside `engine`, so only the (cheap) buffer uploads repeat.
        self.renderer.clear();
        for run in runs {
            if run.text.trim().is_empty() {
                continue;
            }
            let (mesh, _) = self
                .engine
                .build_text_mesh(&self.face, &run.text, &LayoutOptions::default())
                .map_err(|e| format!("MeshBackend: build_text_mesh({:?}): {e}", run.text))?;
            let model = self.model_for(run);
            self.renderer.upload(device, &mesh, model, run.color);
        }
        Ok(())
    }

    fn update_view_proj(&mut self, queue: &Queue, view_proj: Mat4) {
        self.renderer.update_scene(queue, view_proj, LIGHT_DIR, AMBIENT);
    }

    fn render<'a>(&'a self, pass: &mut RenderPass<'a>) {
        self.renderer.draw_all(pass);
    }

    fn instance_count(&self) -> u32 {
        self.renderer.mesh_count() as u32
    }

    /// Extruded meshes are real geometry: depth-tested and depth-writing.
    fn depth_mode(&self) -> DepthMode {
        DepthMode::Depth
    }
}
