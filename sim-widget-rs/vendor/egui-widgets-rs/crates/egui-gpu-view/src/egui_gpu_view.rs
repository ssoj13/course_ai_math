//! Display an offscreen `wgpu` texture inside egui without a CPU round-trip.
//!
//! [`GpuTexture`] registers a `wgpu::TextureView` with egui-wgpu and hands back an
//! [`egui::TextureId`] you draw with `painter.image()` / `ui.image()`. Call
//! [`GpuTexture::present`] (or [`GpuTexture::bind`] with a generation counter)
//! each frame after your GPU pass renders into the view.
//!
//! Also provides generic pointer helpers ([`pointer_in_viewport`] etc.) that map
//! an egui [`Response`] into viewport-local pixel coordinates — useful for driving
//! a camera or picking inside a GPU viewport.
//!
//! (Unifies mermaid-rs `GraphGpuBridge` and gitnexus-rs `EguiViewportPresenter`;
//! the pointer helpers were decoupled from their app coordinate types and now
//! speak plain egui `Pos2` / `Vec2`.)

use egui::{Pos2, Response, TextureId, Vec2};
use egui_wgpu::{RenderState, wgpu};

/// Binds an offscreen `wgpu::TextureView` to egui as a native texture, reusing
/// the same [`TextureId`] across frames (only re-uploading the binding when the
/// view actually changes).
pub struct GpuTexture {
    texture_id: Option<TextureId>,
    /// Last generation passed to [`Self::bind`]; lets callers skip redundant
    /// rebinds when the underlying view has not changed.
    last_generation: u64,
}

impl Default for GpuTexture {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuTexture {
    pub fn new() -> Self {
        Self {
            texture_id: None,
            last_generation: 0,
        }
    }

    /// The egui handle, once [`Self::present`]/[`Self::bind`] has run at least once.
    pub fn texture_id(&self) -> Option<TextureId> {
        self.texture_id
    }

    /// Register or update the egui texture from `view`, always re-binding.
    /// Use when you render a fresh frame into `view` every frame.
    pub fn present(
        &mut self,
        render_state: &RenderState,
        view: &wgpu::TextureView,
        filter: wgpu::FilterMode,
    ) -> TextureId {
        let mut renderer = render_state.renderer.write();
        match self.texture_id {
            Some(id) => {
                renderer.update_egui_texture_from_wgpu_texture(
                    &render_state.device,
                    view,
                    filter,
                    id,
                );
                id
            }
            None => {
                let id = renderer.register_native_texture(&render_state.device, view, filter);
                self.texture_id = Some(id);
                id
            }
        }
    }

    /// Like [`Self::present`] but only re-binds when `generation` differs from the
    /// last call — pass a counter you bump whenever the worker produces a new
    /// frame (avoids a redundant `update_*` when nothing changed).
    pub fn bind(
        &mut self,
        render_state: &RenderState,
        view: &wgpu::TextureView,
        generation: u64,
        filter: wgpu::FilterMode,
    ) -> TextureId {
        if generation == self.last_generation
            && let Some(id) = self.texture_id
        {
            return id;
        }
        self.last_generation = generation;
        self.present(render_state, view, filter)
    }

    /// Forget the current `TextureId` (call when the source texture is recreated,
    /// e.g. on resize). The next [`Self::present`] registers a fresh one.
    pub fn invalidate(&mut self) {
        self.texture_id = None;
        self.last_generation = 0;
    }
}

// ─── viewport pointer helpers ────────────────────────────────────────────────
// Map an egui Response into viewport-local pixel coords (origin at the widget's
// top-left), clamped to the widget rect. Generic — plain egui types.

/// Viewport pixel size of the widget that owns the GPU texture.
pub fn viewport_size(response: &Response) -> Vec2 {
    response.rect.size()
}

#[inline]
fn to_local(response: &Response, screen: Pos2) -> Pos2 {
    let r = response.rect;
    Pos2::new(
        (screen.x - r.min.x).clamp(0.0, r.width()),
        (screen.y - r.min.y).clamp(0.0, r.height()),
    )
}

/// Pointer for click / drag — uses `interact_pointer_pos` (never raw hover).
pub fn pointer_in_viewport_active(response: &Response) -> Option<Pos2> {
    response
        .interact_pointer_pos()
        .map(|p| to_local(response, p))
}

/// Pointer for passive hover (falls back to the active position when pressing).
pub fn pointer_in_viewport_hover(response: &Response) -> Option<Pos2> {
    response
        .interact_pointer_pos()
        .or_else(|| response.hover_pos())
        .map(|p| to_local(response, p))
}

/// Default: active interaction wins, else the hover path.
pub fn pointer_in_viewport(response: &Response) -> Option<Pos2> {
    pointer_in_viewport_active(response).or_else(|| pointer_in_viewport_hover(response))
}

/// Pointer for an ACTIVE DRAG — robust to chord-button flicker: falls back to the
/// last known cursor position so the drag never stalls for a frame when a second
/// button is pressed/released. Reports a position even off-widget (correct for a
/// drag, wrong for hover — use [`pointer_in_viewport_hover`] for hover).
pub fn pointer_in_viewport_drag(response: &Response) -> Option<Pos2> {
    response
        .interact_pointer_pos()
        .or_else(|| response.hover_pos())
        .or_else(|| response.ctx.input(|i| i.pointer.latest_pos()))
        .map(|p| to_local(response, p))
}
