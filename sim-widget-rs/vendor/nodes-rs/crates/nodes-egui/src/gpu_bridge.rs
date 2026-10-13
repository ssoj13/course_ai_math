//! Register [`nodes_view::GraphRenderer`]'s offscreen color buffer as an egui native texture.

use egui::TextureId;
use egui_wgpu::RenderState;
use nodes_view::GraphRenderer;
use wgpu::FilterMode;

/// Holds the [`TextureId`] returned by egui-wgpu for the graph viewport image.
///
/// Call [`Self::present`] every frame **after** [`GraphRenderer::render`] so egui displays
/// the graph drawn this frame. Registers the NON-sRGB reinterpret view
/// ([`GraphRenderer::color_texture_view_unorm`]): egui-wgpu samples texels as raw gamma bytes, so
/// registering the sRGB view would hardware-decode and darken the image ~2.2×.
pub struct GraphGpuBridge {
    texture_id: Option<TextureId>,
}

impl Default for GraphGpuBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphGpuBridge {
    pub fn new() -> Self {
        Self { texture_id: None }
    }

    pub fn texture_id(&self) -> Option<TextureId> {
        self.texture_id
    }

    /// Update or create the egui texture from `renderer`'s offscreen view.
    ///
    /// Returns `false` if the renderer has no color view or zero viewport size.
    pub fn present(&mut self, render_state: &RenderState, renderer: &GraphRenderer) -> bool {
        // Non-sRGB view so egui samples the raw gamma bytes (no hardware sRGB decode → no darkening);
        // fall back to the default view if the renderer predates the unorm view.
        let Some(view) = renderer
            .color_texture_view_unorm()
            .or_else(|| renderer.color_texture_view())
        else {
            return false;
        };
        let (w, h) = renderer.viewport_size();
        if w == 0 || h == 0 {
            return false;
        }
        let mut egui_renderer = render_state.renderer.write();
        if let Some(id) = self.texture_id {
            egui_renderer.update_egui_texture_from_wgpu_texture(
                &render_state.device,
                view,
                FilterMode::Linear,
                id,
            );
        } else {
            self.texture_id = Some(egui_renderer.register_native_texture(
                &render_state.device,
                view,
                FilterMode::Linear,
            ));
        }
        true
    }
}
