//! GPU graph canvas: grid, SDF nodes, tube wires, and glyphon labels.
//!
//! ## Where this is used
//!
//! | Consumer | Entry point |
//! |----------|-------------|
//! | `nodes-egui` | [`GraphRenderer::render`] + [`GraphRenderer::color_texture_view`] |
//! | `nodes-editor` / diagram | Same via `nodes-egui::GraphViewportState` |
//! | `nodes-view/examples/winit_interactive` | [`GraphRenderer::render_to`] (swapchain view) |
//!
//! ## Render targets (do not mix)
//!
//! - **Internal offscreen** — [`GraphRenderer::render`]
//!   calls [`Self::ensure_offscreen_target`] once per frame, then draws. The texture is reused until
//!   `width`, `height`, `format`, or `msaa_samples` changes. `nodes-egui::GraphGpuBridge` reads it via
//!   [`GraphRenderer::color_texture_view`].
//! - **External view** — [`GraphRenderer::render_to`] draws into a caller-owned
//!   [`wgpu::TextureView`] and never touches the internal texture.
//!
//! [`render_scene`] only encodes GPU work; it must **not** allocate targets.
//!
//! ## Per-frame pipeline (`render_scene`)
//!
//! 1. [`shape_scene::build_shape_instances`] + `wire_scene` → [`ShapeInstance`] list
//! 2. [`pick_index::PickIndex::build`] (CPU hit test, same geometry as GPU)
//! 3. Render pass: grid + world XYZ axes (`line_pipeline`, `WorldLinePipeline`) to N-sample color
//! 4. Render pass: instanced SDF quads (`shape_pipeline`, `shape.wgsl`) + tube wires (`wgpu_tube`) to N-sample color + resolve
//! 5. Render pass: instanced SDF ids (`ShapeIdPipeline`, `shape.wgsl::fs_id`) to 1x `R32Uint`
//! 6. Render pass: port/title labels (`text_labels`, `text_renderer`) on resolved 1x color
//!
//! Call [`GraphRenderer::sync_interaction_pick`] before pointer input when layout changed
//! without calling `render` in the same frame.

mod gpu_uniform;
mod grid;
mod image_pipeline;
mod line_pipeline;
mod node_scene;
// `pick_index` and `shape2d` are crate internals. They are exposed (doc-hidden) so the
// criterion bench in `benches/pick_index.rs` can construct realistic scenes without going
// through the full `GraphRenderer` plumbing. Downstream consumers should treat the path
// as unstable.
#[doc(hidden)]
pub mod pick_index;
#[doc(hidden)]
pub mod shape2d;
mod shape_pipeline;
mod shape_scene;
mod shape_sdf;
mod text_labels;
mod text_renderer;
mod wire_scene;

use std::collections::HashMap;
use std::sync::Arc;

use nodes_core::{LayoutCache, NodeId, NodeTypeRegistry, PinSide, Subnet, ValueTypes};
use thiserror::Error;
use wgpu::util::DeviceExt;

use glam::Vec2;

use crate::camera::flat::Camera;
use crate::gizmo::{
    WorldAxesConfig, WorldLinePipeline, build_world_axes_vertices, draw_world_lines,
    world_line_buffer,
};
use crate::layout_cache::sync_layout_cache;
use crate::{VisibilityConfig, VisibleNodes, compute_visible_nodes};
use gpu_uniform::ViewPlaneUniforms;
use image_pipeline::{ImageInstanceGpu, ImagePipeline, ImagePreview};
use line_pipeline::{LinePipeline, LineVertex};
use pick_index::PickIndex;
use shape_pipeline::{ShapeIdPipeline, ShapePipeline, instances_to_gpu};
use shape_scene::{ShapeSceneParams, build_shape_instances};
use text_labels::build_graph_plane_labels;
use wire_scene::{WIRE_HALF_WIDTH_UV, append_marquee_shape, build_wire_tubes, wire_preview_tube};

pub use node_scene::GraphBackend;
pub use shape2d::{PickLayer, PickTarget};

pub use text_renderer::{GraphTextRenderer, TextFilterQuality};

/// Clear color for the internal offscreen pass and legacy flat renderer.
#[derive(Debug, Clone)]
pub struct GraphRendererDesc {
    pub clear_color: wgpu::Color,
    /// MSAA sample count for color passes (1, 2, 4, 8).
    ///
    /// Values are clamped to at least 1 by [`GraphRenderer`]. The host is responsible for
    /// choosing a device-supported value.
    pub msaa_samples: u32,
    /// Glyph-atlas sampling quality for graph-plane text (port names, titles, ...).
    /// `Linear` (the default) fixes wgpu's blocky raw-sampler default; pick
    /// `Anisotropic` if the host camera regularly orbits/tilts the graph plane.
    pub text_quality: TextFilterQuality,
}

impl Default for GraphRendererDesc {
    fn default() -> Self {
        Self {
            clear_color: wgpu::Color {
                r: 0.12,
                g: 0.13,
                b: 0.15,
                a: 1.0,
            },
            msaa_samples: 4,
            text_quality: TextFilterQuality::default(),
        }
    }
}

/// In-progress wire drag preview (passed to [`GraphRenderer::render`] from the egui viewport).
#[derive(Debug, Clone, Copy)]
pub struct WirePreview {
    pub start_world: [f32; 2],
    pub end_world: [f32; 2],
    pub color: [f32; 4],
}

/// Inputs for one graph draw (subnet + camera + optional precomputed layout).
#[derive(Clone)]
pub struct RenderParams<'a> {
    pub subnet: &'a Subnet,
    pub camera: &'a Camera,
    pub layout: Option<&'a LayoutCache>,
    pub selected: &'a [NodeId],
    pub wire_preview: Option<WirePreview>,
    /// Active selection marquee corners in graph UV `(start, end)`.
    pub marquee_uv: Option<([f32; 2], [f32; 2])>,
    pub value_types: &'a ValueTypes,
    pub registry: &'a NodeTypeRegistry,
    pub hover: Option<PickTarget>,
    /// OPTIONAL per-node body-fill override forwarded to [`shape_scene::ShapeSceneParams::node_tint`].
    /// `None` (the default in every existing caller) leaves node bodies theme-colored; a diagram host
    /// supplies a map to tint nodes (e.g. by live pipeline status). See that field for the contract.
    pub node_tint: Option<&'a HashMap<NodeId, [f32; 4]>>,
    /// Draw node titles as graph-plane labels.
    ///
    /// `false` for the winit editor, whose `wgpu-widgets` title bar owns the title strip (drawing
    /// it here too would double-print it). `true` for hosts with no widget layer — the egui
    /// DIAGRAM embed — which otherwise shows titleless cards. Titles belong in the renderer, NOT
    /// in a host-side 2D overlay: the viewport is a 3D scene, and screen-space text cannot follow
    /// a perspective/orbited/rotated node. See `text_labels`' module doc.
    pub draw_node_titles: bool,
}

/// Failure from [`GraphRenderer::render`] / [`GraphRenderer::render_to`].
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("wgpu not attached; call attach_wgpu first")]
    NotAttached,
    #[error("zero viewport size")]
    ZeroSize,
}

/// Wgpu renderer for the node graph canvas.
///
/// Lifecycle: [`GraphRenderer::new`] → [`GraphRenderer::attach_wgpu`] → per frame
/// [`GraphRenderer::resize`] + [`GraphRenderer::render`].
pub struct GraphRenderer {
    desc: GraphRendererDesc,
    device: Option<Arc<wgpu::Device>>,
    queue: Option<Arc<wgpu::Queue>>,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    msaa_samples: u32,
    color_texture: Option<wgpu::Texture>,
    color_view: Option<wgpu::TextureView>,
    /// Non-sRGB reinterpret view of `color_texture` for egui native-texture registration: egui-wgpu
    /// samples texels as raw gamma bytes, so an sRGB view (which the sampler hardware-decodes) would
    /// darken the image ~2.2×. `None` until `ensure_offscreen_target`; egui hosts read it via
    /// `color_texture_view_unorm`, while the winit blit keeps the sRGB `color_view`.
    color_view_unorm: Option<wgpu::TextureView>,
    msaa_color_texture: Option<wgpu::Texture>,
    msaa_color_view: Option<wgpu::TextureView>,
    depth_texture: Option<wgpu::Texture>,
    depth_view: Option<wgpu::TextureView>,
    id_depth_texture: Option<wgpu::Texture>,
    id_depth_view: Option<wgpu::TextureView>,
    line_pipeline: Option<LinePipeline>,
    world_line_pipeline: Option<WorldLinePipeline>,
    shape_pipeline: Option<ShapePipeline>,
    shape_id_pipeline: Option<ShapeIdPipeline>,
    /// Per-node image-preview pipeline (textured quad on a Live2D node's body); built on attach,
    /// rebuilt on MSAA change. `None` until attached.
    image_pipeline: Option<ImagePipeline>,
    /// Linear sampler shared by every image-preview texture.
    image_sampler: Option<wgpu::Sampler>,
    /// Per-node uploaded preview texture + bind group, keyed by node id; re-uploaded only when the
    /// node's `cooked_gen` changes (see `render_scene`). Evicted when the node stops previewing.
    image_textures: HashMap<NodeId, image_pipeline::CachedImageTexture>,
    tube_renderer: Option<wgpu_tube::TubeRenderer>,
    /// Offscreen R32Uint id target used by the dedicated id pass.
    id_target: Option<wgpu_pick::IdTarget>,
    /// Asynchronous GPU readback helper for one-pixel id picks.
    picker: wgpu_pick::IdPicker,
    /// Per-frame id allocation table (GPU id -> logical pick target).
    pick_registry: wgpu_pick::IdRegistry<PickTarget>,
    /// Most recently resolved GPU pick target (one-frame delayed).
    last_gpu_pick: Option<PickTarget>,
    last_layout: LayoutCache,
    last_pick_index: PickIndex,
    text_renderer: Option<GraphTextRenderer>,
    /// `true`: wires keep a constant on-screen thickness ([`wire_width_px`]); `false`: they
    /// scale with zoom in graph-UV units ([`WIRE_HALF_WIDTH_UV`]).
    wire_constant_px: bool,
    /// Constant wire thickness in screen pixels (used when `wire_constant_px`).
    wire_width_px: f32,
    /// Number of ring subdivisions around each connection tube (tessellation quality).
    /// Higher = rounder tube at a proportional vertex cost. Applied when the
    /// [`wgpu_tube::TubeRenderer`] is (re)created; changing it rebuilds that renderer.
    tube_ring_segments: u32,
    /// Whether the background grid is drawn (world XYZ axes are unaffected). Toggled from the host
    /// (editor: Shift+L). Default `true`.
    grid_visible: bool,
    /// Active color palette (SSOT). Every graph element (node bodies, pins, grid, wire preview,
    /// marquee) reads its colors from here; set via [`Self::set_theme`] / [`ui_theme::Themed`].
    theme: ui_theme::Theme,
    /// Which node-body renderer is active (`Sdf` = 2D-on-plane, `Mesh3D` = real extruded cards).
    backend: GraphBackend,
    /// Real-3D node renderer (Mesh3D backend). Caches one procedural mesh per distinct node size
    /// and instances the rest (`wgpu-node`); created lazily on attach.
    node_renderer: Option<wgpu_node::NodeRenderer>,
    /// Shared node-card style (shape/radius/depth) for the Mesh3D backend; per-node size is filled
    /// from the layout in `node_scene`.
    node_style: wgpu_node::NodeStyle,
    /// Frustum culling + screen-size LOD on/off (default `true`). When on, `render_scene` builds a
    /// [`VisibleNodes`] via [`compute_visible_nodes`] and hands it to every graph layer; when off,
    /// all nodes draw at full detail. Toggled by the host (editor: menu "Frustum cull"); a debug
    /// escape hatch if a projection edge case ever hides an on-screen node.
    cull_enabled: bool,
}

impl ui_theme::Themed for GraphRenderer {
    fn set_theme(&mut self, theme: &ui_theme::Theme) {
        self.theme = theme.clone();
    }
}

/// Borrowed compositing targets for drawing an EXTERNAL overlay widget layer (e.g. a
/// `wgpu-widgets` `WidgetRenderer`) over the graph's OWN resolved color target — the embed
/// equivalent of the standalone editor drawing its HUD into the winit surface. Kept
/// widget-agnostic: `nodes-view` never names the widget types; the caller owns the widget
/// renderer and issues the pass with these handles.
///
/// Obtain via [`GraphRenderer::overlay_targets`] ONLY AFTER [`GraphRenderer::render`] — `color`
/// then holds the fully composited (post-MSAA-resolve, post-graph-text) graph, and the graph's
/// own encoder is already submitted, so the overlay is a fresh `LoadOp::Load` pass. `color` and
/// `depth` are the SAME single-sample views `color_texture_view`/`depth_view` expose, size-matched
/// per the MRT rule; `device`/`queue` are the renderer's own (guarantees the overlay is encoded on
/// the same device that owns `color`).
pub struct OverlayTargets<'a> {
    /// Resolved 1× color target (== [`GraphRenderer::color_texture_view`]).
    pub color: &'a wgpu::TextureView,
    /// Shared 1× depth (`id_depth_view`, `Depth32Float`); widget passes use `compare=Always`.
    pub depth: Option<&'a wgpu::TextureView>,
    /// Shared glyph engine (same one graph labels use).
    pub text: &'a mut wgpu_text::TextEngine,
    /// The renderer's own device — the one `color` belongs to.
    pub device: &'a wgpu::Device,
    /// The renderer's own queue.
    pub queue: &'a wgpu::Queue,
}

impl GraphRenderer {
    /// Headless-safe constructor; GPU resources appear after [`Self::attach_wgpu`].
    pub fn new(desc: GraphRendererDesc) -> Self {
        let msaa_samples = desc.msaa_samples.max(1);
        let mut desc = desc;
        desc.msaa_samples = msaa_samples;
        Self {
            desc,
            device: None,
            queue: None,
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 1,
            height: 1,
            msaa_samples,
            color_texture: None,
            color_view: None,
            color_view_unorm: None,
            msaa_color_texture: None,
            msaa_color_view: None,
            depth_texture: None,
            depth_view: None,
            id_depth_texture: None,
            id_depth_view: None,
            line_pipeline: None,
            world_line_pipeline: None,
            shape_pipeline: None,
            shape_id_pipeline: None,
            image_pipeline: None,
            image_sampler: None,
            image_textures: HashMap::new(),
            tube_renderer: None,
            id_target: None,
            picker: wgpu_pick::IdPicker::new(),
            pick_registry: wgpu_pick::IdRegistry::new(),
            last_gpu_pick: None,
            last_layout: LayoutCache::default(),
            last_pick_index: PickIndex::default(),
            text_renderer: None,
            wire_constant_px: true,
            wire_width_px: 4.0,
            tube_ring_segments: 8,
            grid_visible: true,
            theme: ui_theme::Theme::default(),
            backend: GraphBackend::default(),
            node_renderer: None,
            node_style: wgpu_node::NodeStyle::default(),
            cull_enabled: true,
        }
    }

    /// Whether frustum culling + screen-size LOD is active (see [`Self::set_cull_enabled`]).
    pub fn cull_enabled(&self) -> bool {
        self.cull_enabled
    }

    /// Enable/disable frustum culling + LOD. When off, every node draws at full detail. Kept as a
    /// host-facing toggle (and debug escape hatch) so the same policy the widget layer uses can be
    /// flipped in lockstep from the editor.
    pub fn set_cull_enabled(&mut self, on: bool) {
        self.cull_enabled = on;
    }

    /// Active node-body backend (`Sdf` default, `Mesh3D` = real 3D cards via `wgpu-node`).
    pub fn backend(&self) -> GraphBackend {
        self.backend
    }

    /// Switch the node-body backend. `Mesh3D` draws real extruded cards from the single
    /// procedural `wgpu-node` mesh (themed by `ui-theme`), composited under the SDF chrome/text.
    pub fn set_backend(&mut self, backend: GraphBackend) {
        self.backend = backend;
    }

    /// The shared Mesh3D node-card style (shape/radius/depth).
    pub fn node_style(&self) -> wgpu_node::NodeStyle {
        self.node_style
    }

    /// Replace the shared Mesh3D node-card style — affects ALL 3D nodes at once (SSOT). Cheap:
    /// the renderer rebuilds cached meshes lazily on the next frame.
    pub fn set_node_style(&mut self, style: wgpu_node::NodeStyle) {
        self.node_style = style;
    }

    /// Build the Mesh3D node renderer from the current color format / MSAA count. Meshes are built
    /// lazily per node size. Single source of truth for the descriptor (attach + MSAA rebuild).
    fn make_node_renderer(&self, device: &wgpu::Device) -> wgpu_node::NodeRenderer {
        wgpu_node::NodeRenderer::new(
            device,
            wgpu_node::NodeRendererDesc {
                color_format: self.format,
                depth_format: Some(wgpu::TextureFormat::Depth32Float),
                sample_count: self.msaa_samples,
                reversed_z: true,
            },
        )
    }

    /// Replace the active color palette (SSOT). Takes effect on the next `render`.
    pub fn set_theme(&mut self, theme: ui_theme::Theme) {
        self.theme = theme;
    }

    /// The active color palette.
    pub fn theme(&self) -> &ui_theme::Theme {
        &self.theme
    }

    /// Toggle constant-screen-pixel wire thickness vs zoom-scaled UV. Returns the new state
    /// (`true` = constant px).
    pub fn toggle_wire_constant(&mut self) -> bool {
        self.wire_constant_px = !self.wire_constant_px;
        self.wire_constant_px
    }

    /// Is the constant-screen-pixel wire mode (`TubeWidthMode::ScreenPx`) active? `false` = the
    /// zoom-scaled world-radius 3D pipe.
    pub fn wire_constant(&self) -> bool {
        self.wire_constant_px
    }

    /// Set the wire width mode explicitly (`true` = constant screen px, `false` = world 3D pipe).
    /// Menu-driven counterpart to [`Self::toggle_wire_constant`].
    pub fn set_wire_constant(&mut self, on: bool) {
        self.wire_constant_px = on;
    }

    /// Whether the background grid is drawn (world XYZ axes are always drawn).
    pub fn grid_visible(&self) -> bool {
        self.grid_visible
    }

    /// Show/hide the background grid (host toggle; editor binds Shift+L). Returns the new state.
    pub fn set_grid_visible(&mut self, on: bool) {
        self.grid_visible = on;
    }

    /// Flip grid visibility and return the new state.
    pub fn toggle_grid_visible(&mut self) -> bool {
        self.grid_visible = !self.grid_visible;
        self.grid_visible
    }

    /// Set the constant wire thickness in screen pixels (used when constant-px mode is on).
    /// This also drives the connection tube radius (the tube radius is the wire half-width).
    pub fn set_wire_width_px(&mut self, px: f32) {
        self.wire_width_px = px.max(0.5);
    }

    /// Current connection-tube ring subdivision count.
    pub fn tube_ring_segments(&self) -> u32 {
        self.tube_ring_segments
    }

    /// Set the connection-tube ring subdivision count (tessellation quality) and rebuild the
    /// tube renderer if wgpu is attached. Clamped to `3..=64` (below 3 is degenerate, above 64
    /// is wasteful for thin wires). No-op if the value is unchanged.
    pub fn set_tube_ring_segments(&mut self, n: u32) {
        let clamped = n.clamp(3, 64);
        if clamped == self.tube_ring_segments {
            return;
        }
        self.tube_ring_segments = clamped;
        if let Some(device) = self.device.clone() {
            self.tube_renderer = Some(self.make_tube_renderer(&device));
        }
    }

    /// Build a [`wgpu_tube::TubeRenderer`] from the renderer's current color format, MSAA sample
    /// count and ring subdivision count. Single source of truth for the tube descriptor so the
    /// attach / MSAA-change / ring-change paths never drift apart.
    fn make_tube_renderer(&self, device: &wgpu::Device) -> wgpu_tube::TubeRenderer {
        wgpu_tube::TubeRenderer::new(
            device,
            wgpu_tube::TubeRendererDesc {
                color_format: self.format,
                depth_format: Some(wgpu::TextureFormat::Depth32Float),
                sample_count: self.msaa_samples,
                ring_segments: self.tube_ring_segments,
                length_segments: 36,
                reversed_z: true,
            },
        )
    }

    /// Current color-pass MSAA sample count.
    pub fn msaa_samples(&self) -> u32 {
        self.msaa_samples
    }

    /// Change color-pass MSAA sample count and rebuild GPU resources if needed.
    ///
    /// Values are clamped to at least 1. The caller is responsible for providing a
    /// device-supported sample count.
    pub fn set_msaa_samples(&mut self, n: u32) {
        let clamped = n.max(1);
        if clamped == self.msaa_samples {
            return;
        }
        self.msaa_samples = clamped;
        self.desc.msaa_samples = clamped;
        if let Some(device) = self.device.as_ref() {
            self.line_pipeline = Some(LinePipeline::new(device, self.format, self.msaa_samples));
            self.world_line_pipeline = Some(WorldLinePipeline::new(
                device,
                self.format,
                self.msaa_samples,
            ));
            self.shape_pipeline = Some(ShapePipeline::new(
                device,
                self.format,
                wgpu::TextureFormat::Depth32Float,
                self.msaa_samples,
            ));
            self.shape_id_pipeline = Some(ShapeIdPipeline::new(
                device,
                wgpu::TextureFormat::Depth32Float,
            ));
            // Rebuild the image pipeline for the new sample count; the sampler + per-node textures
            // (group 1) are sample-count-independent and kept.
            self.image_pipeline = Some(ImagePipeline::new(
                device,
                self.format,
                wgpu::TextureFormat::Depth32Float,
                self.msaa_samples,
            ));
            self.tube_renderer = Some(self.make_tube_renderer(device));
            self.node_renderer = Some(self.make_node_renderer(device));
        }
        self.invalidate_offscreen_target();
        self.ensure_offscreen_target();
    }

    /// Current glyph-atlas filtering quality for graph-plane text.
    pub fn text_quality(&self) -> TextFilterQuality {
        self.desc.text_quality
    }

    /// Change the glyph-atlas filtering quality at runtime (recreates the text renderer).
    ///
    /// `Anisotropic` builds a mip chain for oblique/orbit views; `Linear` (default) is fine for
    /// near-top-down. No-op if the quality is unchanged or wgpu is not attached yet.
    pub fn set_text_quality(&mut self, quality: TextFilterQuality) {
        if quality == self.desc.text_quality {
            return;
        }
        self.desc.text_quality = quality;
        if let (Some(device), Some(queue)) = (self.device.clone(), self.queue.clone()) {
            self.text_renderer = Some(GraphTextRenderer::with_quality(
                &device,
                &queue,
                self.format,
                quality,
            ));
        }
    }

    /// Bind shared `wgpu` device/queue (same as eframe/egui). Recreates pipelines and offscreen target.
    pub fn attach_wgpu(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) {
        self.device = Some(Arc::new(device.clone()));
        self.queue = Some(Arc::new(queue.clone()));
        self.format = format;
        self.line_pipeline = Some(LinePipeline::new(device, format, self.msaa_samples));
        self.world_line_pipeline = Some(WorldLinePipeline::new(device, format, self.msaa_samples));
        self.shape_pipeline = Some(ShapePipeline::new(
            device,
            format,
            wgpu::TextureFormat::Depth32Float,
            self.msaa_samples,
        ));
        self.shape_id_pipeline = Some(ShapeIdPipeline::new(
            device,
            wgpu::TextureFormat::Depth32Float,
        ));
        self.image_pipeline = Some(ImagePipeline::new(
            device,
            format,
            wgpu::TextureFormat::Depth32Float,
            self.msaa_samples,
        ));
        self.image_sampler = Some(device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nodes_view_image_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        }));
        self.tube_renderer = Some(self.make_tube_renderer(device));
        self.node_renderer = Some(self.make_node_renderer(device));
        self.id_target = Some(wgpu_pick::IdTarget::new(device, self.width, self.height));
        self.text_renderer = Some(GraphTextRenderer::with_quality(
            device,
            queue,
            format,
            self.desc.text_quality,
        ));
        self.invalidate_offscreen_target();
        self.ensure_offscreen_target();
    }

    /// Viewport pixel size; reallocates offscreen target when dimensions change.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.ensure_offscreen_target();
    }

    pub fn viewport_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Offscreen color target after [`Self::render`]. Used by `nodes-egui::GraphGpuBridge::present`
    /// (via the unorm view) and the winit blit (this sRGB view).
    pub fn color_texture_view(&self) -> Option<&wgpu::TextureView> {
        self.color_view.as_ref()
    }

    /// Non-sRGB reinterpret view of the offscreen color, for egui native-texture registration.
    /// egui-wgpu samples texels as raw gamma; an sRGB view would hardware-decode → darken ~2.2×. The
    /// winit blit keeps [`Self::color_texture_view`] (sRGB) so its sample-decode/encode round-trips
    /// to identity against the sRGB swapchain.
    pub fn color_texture_view_unorm(&self) -> Option<&wgpu::TextureView> {
        self.color_view_unorm.as_ref()
    }

    /// Render into the internal offscreen color target (registered by [`GraphGpuBridge`] / egui).
    pub fn render(&mut self, params: &RenderParams<'_>) -> Result<(), RenderError> {
        if self.device.is_none() {
            return Err(RenderError::NotAttached);
        }
        if self.width == 0 || self.height == 0 {
            return Err(RenderError::ZeroSize);
        }
        self.ensure_offscreen_target();
        let layout = params
            .layout
            .cloned()
            .unwrap_or_else(|| sync_layout_cache(params.subnet));
        let color_view = self
            .color_view
            .as_ref()
            .ok_or(RenderError::NotAttached)?
            .clone();
        self.render_scene(&layout, params, &color_view)
    }

    /// Render directly into a swapchain or external color target (e.g. winit surface).
    /// Does not touch the internal offscreen texture used by [`Self::render`].
    ///
    /// # MSAA contract
    /// When `msaa_samples > 1`, `color_view` is used as the **resolve target** for the internal
    /// multisampled color texture, so it MUST have the same dimensions as the renderer
    /// ([`Self::resize`]) and be single-sampled. Call `resize(w, h)` to match the target before
    /// each frame (the reference editor does this on `WindowEvent::Resized`); a size mismatch is
    /// a wgpu validation error at pass begin.
    pub fn render_to(
        &mut self,
        params: &RenderParams<'_>,
        color_view: &wgpu::TextureView,
    ) -> Result<(), RenderError> {
        let layout = params
            .layout
            .cloned()
            .unwrap_or_else(|| sync_layout_cache(params.subnet));
        self.render_scene(&layout, params, color_view)
    }

    /// Draw grid, SDF shapes, tube wires, and labels into `color_view` (caller owns target lifetime).
    fn render_scene(
        &mut self,
        layout: &LayoutCache,
        params: &RenderParams<'_>,
        color_view: &wgpu::TextureView,
    ) -> Result<(), RenderError> {
        // The draw inputs already arrive as ONE [`RenderParams`]; both callers used to explode it
        // into a dozen positional arguments here. It is passed through whole and unpacked into the
        // same locals instead. `layout` stays separate: `params.layout` is optional and each caller
        // resolves it (clone-or-recompute) before calling.
        let subnet = params.subnet;
        let camera = params.camera;
        let selected = params.selected;
        let wire_preview = params.wire_preview;
        let marquee_uv = params.marquee_uv;
        let value_types = params.value_types;
        let registry = params.registry;
        let hover = params.hover.as_ref();
        let node_tint = params.node_tint;
        let draw_node_titles = params.draw_node_titles;

        self.ensure_offscreen_target();
        self.last_layout = layout.clone();

        let w = self.width as f32;
        let h = self.height as f32;
        let viewport = Vec2::new(w, h);
        // UNIFIED node visibility: ONE visible-set computed once here and handed to EVERY graph
        // layer (shapes, tube wires, plane text, Mesh3D bodies) so they cull + LOD identically and a
        // node is never drawn half. The editor's node-widget layer calls the SAME
        // `compute_visible_nodes` for its surfaces. This replaces the retired `visible_graph_uv_rect`
        // cull, which collapsed at grazing angles (perimeter rays miss the plane) and skipped the
        // widget layer; here we PROJECT node AABBs into clip space instead (robust, see
        // `node_visibility`). Gated by `cull_enabled`; when off, reports all nodes visible at Full.
        let vis_cfg =
            VisibilityConfig::for_backend(self.cull_enabled, self.backend == GraphBackend::Mesh3D);
        let visible = compute_visible_nodes(layout, camera, viewport, vis_cfg);
        let cull = Some(&visible);

        // Wire half-width in graph-UV units. Constant-px mode converts pixels → UV using the
        // on-plane pixels-per-world scale (perspective: evaluated at the graph plane, since 1
        // graph UV unit == 1 world unit), so the on-screen thickness stays fixed regardless of
        // dolly; otherwise the wire scales with zoom like node bodies.
        //
        // The px→world conversion is done PER VERTEX in the tube shader (`TubeWidthMode::ScreenPx`)
        // from each vertex's own `clip.w`, NOT by one host-side division of a target-point scale
        // (which returned ~0 when the plane target neared/passed the camera and "exploded" the tube
        // into a blob for a frame). So here we only pick the radius UNITS + mode: constant-px passes
        // the half-width in PIXELS, world mode passes it in world units.
        let (wire_half, tube_width_mode) = if self.wire_constant_px {
            (self.wire_width_px * 0.5, wgpu_tube::TubeWidthMode::ScreenPx)
        } else {
            (WIRE_HALF_WIDTH_UV, wgpu_tube::TubeWidthMode::World)
        };

        let mut image_previews: Vec<ImagePreview> = Vec::new();
        let mut shape_instances = build_shape_instances(
            &ShapeSceneParams {
                layout,
                subnet,
                registry,
                value_types,
                selected,
                hover,
                visible: cull,
                theme: &self.theme,
                node_tint,
            },
            &mut self.pick_registry,
            &mut image_previews,
        );
        if let Some((a, b)) = marquee_uv {
            append_marquee_shape(&mut shape_instances, a, b, &self.theme);
        }

        let mut tube_instances = Vec::new();
        build_wire_tubes(
            wire_scene::WireGraph {
                graph: nodes_core::GraphRead {
                    subnet,
                    registry,
                    value_types,
                },
                layout,
            },
            cull,
            &camera.plane,
            wire_half,
            &mut tube_instances,
        );
        if let Some(wp) = wire_preview {
            tube_instances.push(wire_preview_tube(
                &camera.plane,
                wp.start_world,
                wp.end_world,
                wp.color,
                wire_half,
            ));
        }

        self.last_pick_index = PickIndex::build(&shape_instances, PickIndex::default_cell());

        let device = self.device.clone().ok_or(RenderError::NotAttached)?;
        let queue = self.queue.clone().ok_or(RenderError::NotAttached)?;
        if self.width == 0 || self.height == 0 {
            return Err(RenderError::ZeroSize);
        }

        // Grid covers 4× the scene bounding box (from this frame's layout), finite and stable at
        // every angle — see `grid.rs`. `last_layout` was set above from `params.layout`.
        let scene_bounds = nodes_core::layout::layout_uv_bounds(&self.last_layout)
            .map(|(mn, mx)| (glam::Vec2::new(mn[0], mn[1]), glam::Vec2::new(mx[0], mx[1])));
        // Grid can be toggled off (Shift+L in the editor); when hidden we skip vertex generation
        // entirely so pass 1 only clears + draws the world axes.
        let grid_verts = if self.grid_visible {
            grid::build_grid_vertices(camera, w, h, scene_bounds, &self.theme)
        } else {
            Vec::new()
        };

        let view_proj = camera.view_proj(w, h);
        if let Some(tube_renderer) = self.tube_renderer.as_mut() {
            tube_renderer.set_width_mode(tube_width_mode);
            if tube_width_mode == wgpu_tube::TubeWidthMode::ScreenPx {
                // World→pixel scale at clip.w = 1: 0.5 * viewport.y / tan(fov_y/2). The tube shader
                // divides each vertex's clip.w by this to hold a constant on-screen wire thickness.
                let proj_yy = 1.0 / (camera.fov_y() * 0.5).tan();
                tube_renderer.set_screen_scale(0.5 * h * proj_yy);
            }
            tube_renderer.set_camera(&queue, view_proj);
            tube_renderer.update_instances(&device, &queue, &tube_instances);
        }
        // Mesh3D backend: each node picks a cached procedural mesh (by real size) and draws
        // instanced. Built from the SAME layout + theme the SDF path uses.
        if self.backend == GraphBackend::Mesh3D {
            let batches = node_scene::build_node_batches(
                layout,
                subnet,
                registry,
                &camera.plane,
                &self.theme,
                &self.node_style,
                cull,
            );
            if let Some(nr) = self.node_renderer.as_mut() {
                nr.set_camera(&queue, view_proj);
                nr.begin_frame();
                for (params, instance) in &batches {
                    nr.push(params, *instance);
                }
                nr.end_frame(&queue);
            }
        }

        // --- Image previews: upload each Live2D node's cooked image to a per-node cached texture
        // (re-`write_texture` ONLY when its `cooked_gen`/dims changed) and build the instance buffer
        // for the image pass. Must run BEFORE the immutable pipeline borrows below (it mutates
        // `self.image_textures`). `image_pipeline`/`sampler` are disjoint fields from it. ---
        let mut image_instances: Vec<ImageInstanceGpu> = Vec::new();
        let mut image_draws: Vec<(NodeId, u32)> = Vec::new();
        // Previews draw only in the Sdf backend (the Mesh3D card path has no image support yet), so
        // upload/keep textures only there; free them otherwise (no wasted VRAM in Mesh3D).
        if self.backend != GraphBackend::Sdf {
            self.image_textures.clear();
        } else if let (Some(image_pipeline), Some(sampler)) =
            (self.image_pipeline.as_ref(), self.image_sampler.as_ref())
        {
            let max_dim = device.limits().max_texture_dimension_2d;
            let live: std::collections::HashSet<NodeId> =
                image_previews.iter().map(|p| p.node_id).collect();
            self.image_textures.retain(|id, _| live.contains(id));
            for preview in &image_previews {
                // GPU safety: skip zero / over-max-dimension images — `create_texture` /
                // `write_texture` would raise a wgpu validation error and crash the app. Drop any
                // stale cache entry for such a node (falls back to a blank body, not a hard crash).
                if preview.w == 0 || preview.h == 0 || preview.w > max_dim || preview.h > max_dim {
                    self.image_textures.remove(&preview.node_id);
                    continue;
                }
                let stale = self.image_textures.get(&preview.node_id).is_none_or(|c| {
                    c.cooked_gen != preview.cooked_gen || c.w != preview.w || c.h != preview.h
                });
                if stale {
                    // Fetch the RGBA8 bytes lazily (only on a cache miss) from the cooked output.
                    if let Some(node) = subnet.nodes.get(&preview.node_id)
                        && let Some(name) = node.output_names().into_iter().next()
                        && let Some(v) = node.output_value(&name)
                        && let Some((_dt, shape, data)) = v.as_buffer()
                        && shape.len() == 3
                        && data.len() == preview.w as usize * preview.h as usize * 4
                    {
                        let cache = image_pipeline.upload(
                            &device,
                            &queue,
                            sampler,
                            image_pipeline::RgbaImage {
                                w: preview.w,
                                h: preview.h,
                                pixels: data,
                            },
                            preview.cooked_gen,
                        );
                        self.image_textures.insert(preview.node_id, cache);
                    }
                }
                if self.image_textures.contains_key(&preview.node_id) {
                    let index = image_instances.len() as u32;
                    image_instances.push(ImageInstanceGpu {
                        aabb_min: preview.aabb_min,
                        aabb_max: preview.aabb_max,
                        pivot: preview.pivot,
                        rotation: preview.rotation,
                        depth_bias: preview.depth_bias,
                    });
                    image_draws.push((preview.node_id, index));
                }
            }
        }
        let image_inst_buf = if image_instances.is_empty() {
            None
        } else {
            Some(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("nodes_view_image_instances"),
                    contents: bytemuck::cast_slice(&image_instances),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            )
        };

        let line_pipeline = self
            .line_pipeline
            .as_ref()
            .ok_or(RenderError::NotAttached)?;
        let shape_pipeline = self
            .shape_pipeline
            .as_ref()
            .ok_or(RenderError::NotAttached)?;
        let shape_id_pipeline = self
            .shape_id_pipeline
            .as_ref()
            .ok_or(RenderError::NotAttached)?;
        let uniform = ViewPlaneUniforms::from_camera(camera, view_proj);
        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("nodes_view_uniform"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let line_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nodes_view_line_bg"),
            layout: &line_pipeline.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        let grid_buf = line_buffer(&device, &grid_verts);

        let axes_verts = build_world_axes_vertices(WorldAxesConfig::default());
        let axes_buf = world_line_buffer(&device, &axes_verts);
        let axes_world_bind = self
            .world_line_pipeline
            .as_ref()
            .map(|world_line| world_line.create_bind_group(&device, view_proj));

        let gpu_shapes = instances_to_gpu(&shape_instances);
        let shape_buf = if gpu_shapes.is_empty() {
            None
        } else {
            Some(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("nodes_view_shapes"),
                    contents: bytemuck::cast_slice(&gpu_shapes),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            )
        };
        let shape_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nodes_view_shape_bg"),
            layout: &shape_pipeline.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });
        let shape_id_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nodes_view_shape_id_bg"),
            layout: &shape_id_pipeline.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });
        // Image-preview group-0 (camera) bind group reuses the shared uniform buffer; the per-node
        // texture bind-group refs are collected now (self.image_textures is done mutating) so the
        // render pass borrows them immutably alongside the shape pipeline.
        let image_uniform_bind = self
            .image_pipeline
            .as_ref()
            .map(|p| p.uniform_bind_group(&device, &uniform_buf));
        let image_pipeline_ref = self.image_pipeline.as_ref();
        let image_draw_refs: Vec<(&wgpu::BindGroup, u32)> = image_draws
            .iter()
            .filter_map(|(id, idx)| self.image_textures.get(id).map(|c| (&c.bind_group, *idx)))
            .collect();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nodes_view_encoder"),
        });
        if let Some(tube_renderer) = self.tube_renderer.as_ref() {
            tube_renderer.encode_prepass(&mut encoder);
        }
        let tube_renderer = self.tube_renderer.as_ref();
        let node_renderer = if self.backend == GraphBackend::Mesh3D {
            self.node_renderer.as_ref()
        } else {
            None
        };
        // NO MIXING: SDF and Mesh are distinct backends. In Mesh3D the node bodies come ONLY from
        // the 3D mesh; the SDF shape COLOR pass is skipped entirely (the flat gray quads were the
        // "gray box behind the node"). The SDF id pass still runs so nodes stay pickable until the
        // mesh backend grows its own pins/pick. (This is the interim seam before the crate split.)
        let draw_sdf_color = self.backend == GraphBackend::Sdf;

        let color_render_view = if self.msaa_samples > 1 {
            self.msaa_color_view
                .as_ref()
                .ok_or(RenderError::NotAttached)?
        } else {
            color_view
        };
        let resolve_target = if self.msaa_samples > 1 {
            Some(color_view)
        } else {
            None
        };

        // Pass 1: clear + grid + world axes.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nodes_view_grid"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_render_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.desc.clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            draw_lines(
                &mut pass,
                line_pipeline,
                &line_bind,
                grid_buf.as_ref(),
                grid_verts.len(),
            );
            if let (Some(world_line), Some(buf), Some((_axes_ub, axes_bind))) = (
                self.world_line_pipeline.as_ref(),
                axes_buf.as_ref(),
                axes_world_bind.as_ref(),
            ) {
                draw_world_lines(
                    &mut pass,
                    world_line,
                    axes_bind,
                    Some(buf),
                    axes_verts.len(),
                );
            }
        }

        let Some(depth_view) = self.depth_view.as_ref() else {
            return Err(RenderError::NotAttached);
        };
        // Pass 2: shapes + tubes color (MSAA) + optional resolve into the 1x color target.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nodes_view_shapes"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_render_view,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        // Reversed-Z: clear to FAR (0.0), keep GREATER depth.
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Mesh3D: node bodies are the 3D cards ONLY (no SDF quads). SDF backend: flat quads.
            if let Some(node_renderer) = node_renderer {
                node_renderer.draw(&mut pass);
            }
            if draw_sdf_color {
                if let Some(buf) = shape_buf.as_ref() {
                    shape_pipeline.encode(&mut pass, &shape_bind, buf, gpu_shapes.len() as u32);
                }
                // Image previews on top of the SDF body fill (Title-layer depth bias), one draw per
                // node binding its own texture; still under the Pass-4 plane text.
                if let (Some(ipipe), Some(ubind), Some(inst_buf)) = (
                    image_pipeline_ref,
                    image_uniform_bind.as_ref(),
                    image_inst_buf.as_ref(),
                ) {
                    for &(tex_bg, index) in &image_draw_refs {
                        ipipe.draw(&mut pass, ubind, tex_bg, inst_buf, index);
                    }
                }
            }
            if let Some(tube_renderer) = tube_renderer {
                tube_renderer.draw(&mut pass);
            }
        }

        let Some(id_target) = self.id_target.as_ref() else {
            return Err(RenderError::NotAttached);
        };
        let Some(id_depth_view) = self.id_depth_view.as_ref() else {
            return Err(RenderError::NotAttached);
        };
        // Pass 3: single-sampled pick ids with matching shape coverage/discards.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nodes_view_shapes_id"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: id_target.view(),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu_pick::IdTarget::clear_value()),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: id_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(buf) = shape_buf.as_ref() {
                shape_id_pipeline.encode(&mut pass, &shape_id_bind, buf, gpu_shapes.len() as u32);
            }
        }

        // Pass 4: graph-plane labels (port names, summaries, and — for widget-less hosts — node
        // titles) on the resolved 1x color target.
        let plane_labels = build_graph_plane_labels(
            layout,
            subnet,
            registry,
            camera,
            viewport,
            cull,
            &self.theme,
            draw_node_titles,
        );
        if let Some(tr) = self.text_renderer.as_mut()
            && !plane_labels.is_empty()
        {
            if let Err(e) = tr.prepare_plane(&device, &queue, &plane_labels) {
                log::warn!("nodes_view graph text: {e}");
            } else {
                log::trace!(
                    target: "nodes_view::text",
                    "plane labels: {} labels, {} glyph instances",
                    plane_labels.len(),
                    tr.plane_instance_count()
                );
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("nodes_view_text"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: color_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: id_depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                tr.update_plane_view_proj(&queue, view_proj);
                tr.render_plane(&mut pass);
            }
        }

        self.picker.submit(
            &device,
            &mut encoder,
            id_target.texture(),
            (self.width, self.height),
        );

        queue.submit(Some(encoder.finish()));
        if let Some(id) = self.picker.poll(&device) {
            self.last_gpu_pick = if id == wgpu_pick::ID_NONE {
                None
            } else {
                self.pick_registry.get(id).cloned()
            };
        }
        Ok(())
    }

    /// Single-sampled depth buffer aligned with text/id passes.
    ///
    /// Hosts use this for widget overlay depth testing; it stays 1x even when color uses MSAA.
    pub fn depth_view(&self) -> Option<&wgpu::TextureView> {
        self.id_depth_view.as_ref()
    }

    /// Shared [`wgpu_text::TextEngine`] for graph labels and widget overlay text.
    pub fn widget_text_engine_mut(&mut self) -> Option<&mut wgpu_text::TextEngine> {
        self.text_renderer.as_mut().map(|t| t.engine_mut())
    }

    /// Borrow depth + plane text together (separate fields — avoids split-borrow issues in the host).
    pub fn with_widget_text<R>(
        &mut self,
        f: impl FnOnce(Option<&wgpu::TextureView>, &mut wgpu_text::TextEngine) -> R,
    ) -> Option<R> {
        let depth = self.id_depth_view.as_ref();
        let text = self.text_renderer.as_mut()?.engine_mut();
        Some(f(depth, text))
    }

    /// Borrow the [`OverlayTargets`] for compositing an external widget layer over the graph's
    /// own color target (see that type's docs). `None` until attached AND the offscreen target is
    /// allocated (i.e. after the first [`Self::render`]). Widget-agnostic: the caller drives the
    /// widget renderer with these handles. The five borrows are DISJOINT struct fields, so this
    /// hands back one `&mut` (text) alongside four `&` safely — all tied to the single `&mut self`.
    pub fn overlay_targets(&mut self) -> Option<OverlayTargets<'_>> {
        Some(OverlayTargets {
            color: self.color_view.as_ref()?,
            depth: self.id_depth_view.as_ref(),
            text: self.text_renderer.as_mut()?.engine_mut(),
            device: self.device.as_deref()?,
            queue: self.queue.as_deref()?,
        })
    }

    /// Plane-locked text on an existing color target (e.g. widget values on the node board).
    pub fn render_plane_overlay(
        &mut self,
        labels: &[wgpu_text::PlaneLabel],
        color_view: &wgpu::TextureView,
        view_proj: glam::Mat4,
    ) -> Result<(), RenderError> {
        if labels.is_empty() {
            return Ok(());
        }
        let device = self.device.clone().ok_or(RenderError::NotAttached)?;
        let queue = self.queue.clone().ok_or(RenderError::NotAttached)?;
        let Some(tr) = self.text_renderer.as_mut() else {
            return Ok(());
        };
        if let Err(e) = tr.prepare_plane(&device, &queue, labels) {
            log::warn!("nodes_view plane text prepare: {e}");
            return Ok(());
        }
        if tr.plane_instance_count() == 0 {
            return Ok(());
        }
        tr.update_plane_view_proj(&queue, view_proj);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nodes_view_plane_overlay"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nodes_view_plane_overlay_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            tr.render_plane(&mut pass);
        }
        queue.submit(Some(encoder.finish()));
        Ok(())
    }

    /// Rebuild CPU pick grid from layout (call before hit tests if [`Self::render`] was skipped).
    ///
    /// Uses the same [`compute_visible_nodes`] cull + LOD as [`Self::render_scene`] when the
    /// renderer has a non-zero size, so the CPU pick index matches the drawn scene.
    pub fn sync_interaction_pick(
        &mut self,
        layout: &LayoutCache,
        subnet: &Subnet,
        registry: &NodeTypeRegistry,
        value_types: &ValueTypes,
        selected: &[NodeId],
        camera: &Camera,
    ) {
        self.last_layout = layout.clone();
        let w = self.width as f32;
        let h = self.height as f32;
        // Match the drawn scene's visible-set so the CPU pick index (fallback / egui path) contains
        // exactly the nodes on screen at the same LOD — SAME `compute_visible_nodes` as render.
        let vis_cfg =
            VisibilityConfig::for_backend(self.cull_enabled, self.backend == GraphBackend::Mesh3D);
        let visible = if w > 0.0 && h > 0.0 {
            compute_visible_nodes(layout, camera, Vec2::new(w, h), vis_cfg)
        } else {
            VisibleNodes::default()
        };
        let shape_instances = build_shape_instances(
            &ShapeSceneParams {
                layout,
                subnet,
                registry,
                value_types,
                selected,
                hover: None,
                visible: Some(&visible),
                theme: &self.theme,
                // Pick-only rebuild: body fill is discarded in the id pass, so no tint needed.
                node_tint: None,
            },
            &mut self.pick_registry,
            // Pick-only path: previews aren't needed here (the invisible NodeBody pick quad already
            // makes the image region selectable), so collect into a throwaway.
            &mut Vec::new(),
        );
        self.last_pick_index = PickIndex::build(&shape_instances, PickIndex::default_cell());
    }

    /// Queue a GPU object-id pick at viewport pixel coordinates (top-left origin, physical px).
    ///
    /// Resolved **within the same** [`Self::render`] / [`Self::render_to`] call that follows:
    /// that call renders the id target, copies the cursor's pixel, and blocks on
    /// `device.poll` to read it back (see [`Self::render_scene`] → `picker.submit`/`poll`).
    /// So the result in [`Self::last_gpu_pick`] reflects the SAME frame's scene and the SAME
    /// frame's id→target map — no cross-frame staleness or id desync. The cost is a small GPU
    /// stall per pick (negligible for the graph scene; could be deferred to next-frame readback
    /// later to remove the stall, at the price of 1-frame staleness).
    pub fn request_pick(&mut self, px: u32, py: u32) {
        self.picker.request(px, py);
    }

    /// Resolved target of the most recent [`Self::request_pick`] (synchronous — see there).
    ///
    /// `None` means the cursor was over background (id 0) or no pick was requested. The value
    /// is refreshed by the picker `poll` at the end of `render_scene`, so read it after a
    /// `render`/`render_to` in the same frame the request was made.
    pub fn last_gpu_pick(&self) -> Option<PickTarget> {
        self.last_gpu_pick.clone()
    }

    /// Topmost pickable shape at graph UV `world` (uses last `sync_interaction_pick` / `render`).
    pub fn pick_target_at(&self, world: [f32; 2], padding_world: f32) -> Option<PickTarget> {
        self.last_pick_index.pick_topmost(world, padding_world)
    }

    /// Layout from the most recent `render` / `sync_interaction_pick`.
    pub fn last_layout(&self) -> &LayoutCache {
        &self.last_layout
    }

    /// PLACED (rotation-applied) pin center in graph UV from [`Self::last_layout`].
    ///
    /// Goes through [`LayoutCache::pin_world`] (the SSOT), so the wire-drag preview start and the
    /// connection drop origin sit on the DRAWN pin of a rotated node — not its unrotated local slot.
    pub fn last_layout_pin(&self, node_id: NodeId, side: PinSide, idx: usize) -> Option<[f32; 2]> {
        self.last_layout.pin_world(node_id, side, idx)
    }

    fn invalidate_offscreen_target(&mut self) {
        self.color_texture = None;
        self.color_view = None;
        self.color_view_unorm = None;
        self.msaa_color_texture = None;
        self.msaa_color_view = None;
        self.depth_texture = None;
        self.depth_view = None;
        self.id_depth_texture = None;
        self.id_depth_view = None;
    }

    /// (Re)allocate internal offscreen targets when size, format, or MSAA changes.
    /// Call once per frame before drawing; [`render_scene`] must not allocate targets.
    fn ensure_offscreen_target(&mut self) {
        let Some(device) = self.device.clone() else {
            return;
        };
        if self.width == 0 || self.height == 0 {
            return;
        }
        if let (Some(color), Some(depth), Some(id_depth), Some(id_target)) = (
            &self.color_texture,
            &self.depth_texture,
            &self.id_depth_texture,
            self.id_target.as_ref(),
        ) {
            let size = color.size();
            let (id_w, id_h) = id_target.size();
            let msaa_ok = if self.msaa_samples > 1 {
                self.msaa_color_texture.as_ref().is_some_and(|msaa| {
                    msaa.size() == size
                        && msaa.format() == self.format
                        && msaa.sample_count() == self.msaa_samples
                })
            } else {
                self.msaa_color_texture.is_none()
            };
            if size.width == self.width
                && size.height == self.height
                && color.format() == self.format
                && color.sample_count() == 1
                && depth.size() == size
                && depth.sample_count() == self.msaa_samples
                && id_depth.size() == size
                && id_depth.sample_count() == 1
                && id_w == self.width
                && id_h == self.height
                && msaa_ok
            {
                return;
            }
        }

        let size = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };

        // 1x resolve target that hosts can sample from (`color_texture_view`). `view_formats` lists
        // the non-sRGB counterpart so egui hosts can register a raw-gamma reinterpret view (see
        // `color_view_unorm`); sRGB↔unorm of the same base is the one always-allowed reinterpret (no
        // feature flag). `remove_srgb_suffix` is a no-op when the format is already non-sRGB, so the
        // unorm view then just equals the default view.
        let unorm = self.format.remove_srgb_suffix();
        let color_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nodes_view_color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[unorm],
        });
        let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor::default());
        // Raw-gamma view egui samples through (no hardware sRGB decode) — see `color_view_unorm`.
        let color_view_unorm = color_tex.create_view(&wgpu::TextureViewDescriptor {
            format: Some(unorm),
            ..Default::default()
        });
        self.color_texture = Some(color_tex);
        self.color_view = Some(color_view);
        self.color_view_unorm = Some(color_view_unorm);

        if self.msaa_samples > 1 {
            let msaa_color = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("nodes_view_msaa_color"),
                size,
                mip_level_count: 1,
                sample_count: self.msaa_samples,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let msaa_color_view = msaa_color.create_view(&wgpu::TextureViewDescriptor::default());
            self.msaa_color_texture = Some(msaa_color);
            self.msaa_color_view = Some(msaa_color_view);
        } else {
            self.msaa_color_texture = None;
            self.msaa_color_view = None;
        }

        let depth_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nodes_view_depth"),
            size,
            mip_level_count: 1,
            sample_count: self.msaa_samples,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth_texture = Some(depth_tex);
        self.depth_view = Some(depth_view);

        let id_depth_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nodes_view_id_depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let id_depth_view = id_depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.id_depth_texture = Some(id_depth_tex);
        self.id_depth_view = Some(id_depth_view);

        match self.id_target.as_mut() {
            Some(id_target) => id_target.resize(&device, self.width, self.height),
            None => {
                self.id_target = Some(wgpu_pick::IdTarget::new(&device, self.width, self.height));
            }
        }
    }
}

fn line_buffer(device: &wgpu::Device, verts: &[LineVertex]) -> Option<wgpu::Buffer> {
    if verts.is_empty() {
        return None;
    }
    Some(
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("nodes_view_lines"),
            contents: bytemuck::cast_slice(verts),
            usage: wgpu::BufferUsages::VERTEX,
        }),
    )
}

fn draw_lines<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    pipeline: &'a LinePipeline,
    bind_group: &'a wgpu::BindGroup,
    vertex_buf: Option<&'a wgpu::Buffer>,
    vert_count: usize,
) {
    if vert_count == 0 {
        return;
    }
    pass.set_pipeline(&pipeline.line_pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    if let Some(vb) = vertex_buf {
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.draw(0..vert_count as u32, 0..1);
    }
}
