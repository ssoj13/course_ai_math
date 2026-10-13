//! Graph-plane labels via `wgpu-text`.

use wgpu::{Device, Queue, RenderPass, TextureFormat};

use wgpu_text::TextEngine;
pub use wgpu_text::TextFilterQuality;

/// GPU text for graph titles and port names (plane-locked).
pub struct GraphTextRenderer(TextEngine);

impl GraphTextRenderer {
    /// Default quality: [`TextFilterQuality::Linear`] — see [`Self::with_quality`] to
    /// pick `Anisotropic` (mip chain + anisotropic filtering) for a camera that
    /// regularly orbits/tilts the graph plane, where a single flat bake doesn't
    /// uniformly match the on-screen footprint.
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        Self(TextEngine::new(device, queue, format))
    }

    pub fn with_quality(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        quality: TextFilterQuality,
    ) -> Self {
        Self(TextEngine::with_quality(device, queue, format, quality))
    }

    pub fn prepare_plane(
        &mut self,
        device: &Device,
        queue: &Queue,
        labels: &[wgpu_text::PlaneLabel],
    ) -> Result<(), String> {
        self.0.prepare_plane(device, queue, labels)
    }

    pub fn update_plane_view_proj(&self, queue: &Queue, view_proj: glam::Mat4) {
        self.0.update_plane_view_proj(queue, view_proj);
    }

    pub fn render_plane<'pass>(&'pass self, pass: &mut RenderPass<'pass>) {
        self.0.render_plane(pass);
    }

    pub fn plane_instance_count(&self) -> u32 {
        self.0.plane_instance_count()
    }

    pub fn engine_mut(&mut self) -> &mut TextEngine {
        &mut self.0
    }
}
