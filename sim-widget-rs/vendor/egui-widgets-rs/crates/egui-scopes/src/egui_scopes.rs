//! GPU video scopes (histogram / waveform / vectorscope / RGB parade / false
//! colour) for egui.
//!
//! Each scope runs entirely on the GPU via wgpu 29 / egui 0.34: the pipelines
//! and WGSL shaders scatter directly from a caller-supplied linear
//! `Rgba32Float` source `wgpu::TextureView` (zero CPU re-upload — single GPU
//! source). The source is any texture you already have on the GPU; no app types.
//!
//! Architecture (per scope):
//!   tick()       — scatter + reduce + draw compute passes; reads the source
//!                  linear `wgpu::TextureView` (zero CPU upload) and writes an
//!                  output texture sized to the panel.
//!   texture_id() — egui handle to display via `painter.image()`.
//!   show_panel() — paints the output texture into the egui panel rect.
//!
//! This module contains the GPU compute path only; there is no CPU fallback —
//! the viewer is wgpu-only and the GPU panels are the sole scope implementation.

mod bucket;
mod falsecolor;
mod histogram;
mod parade;
mod pipe;
mod scope;
mod vectorscope;
mod waveform;

pub(crate) use bucket::BucketSizer;

pub use falsecolor::GpuFalseColorPanel;
pub use histogram::GpuHistogramPanel;
pub use parade::GpuParadePanel;
pub use vectorscope::GpuVectorscopePanel;
pub use waveform::GpuWaveformPanel;

/// Context passed to GPU panel operations: the device/queue plus the egui-wgpu
/// renderer (used to register/update the panel's output texture as an egui
/// `TextureId`).
pub struct PanelCtx<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub renderer: &'a mut egui_wgpu::Renderer,
}

/// A scope panel that runs entirely on the GPU.
pub trait GpuPanel: Send + Sync {
    /// Scatter from source texture and draw the result at `(out_w, out_h)`.
    fn tick(
        &mut self,
        ctx: &mut PanelCtx,
        src: &wgpu::TextureView,
        src_w: u32,
        src_h: u32,
        out_w: u32,
        out_h: u32,
    );
    /// Release GPU resources (the output texture + the egui `TextureId`) when the
    /// panel's tab is closed. egui's `Renderer` retains a native texture's bind
    /// group (and its VRAM) until `free_texture`, so a closed scope would otherwise
    /// hold its output texture resident. Idempotent; the panel rebuilds lazily on
    /// the next [`GpuPanel::tick`].
    fn release(&mut self, renderer: &mut egui_wgpu::Renderer);
    /// egui handle to the output texture (None until first successful tick).
    fn texture_id(&self) -> Option<egui::TextureId>;
    /// Physical bucket size of the output texture (for UV crop in [`show_panel`]).
    fn physical_bucket_size(&self) -> (u32, u32) {
        (0, 0)
    }
}

// ─── shared output format ─────────────────────────────────────────────────────

pub(crate) const PANEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Create a storage-write + texture-bindable output texture.
pub(crate) fn make_output_texture(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: PANEL_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

/// Storage buffer (r/w) for atomic u32 bins.
pub(crate) fn make_bins_buffer(
    device: &wgpu::Device,
    label: &str,
    size_bytes: u64,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Uniform buffer: `{ src_w: u32, src_h: u32, out_w: u32, out_h: u32 }` = 16 bytes.
pub(crate) fn make_params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("analysis_params"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Register or update the egui TextureId for an output view.
pub(crate) fn sync_egui_texture(
    ctx: &mut PanelCtx,
    view: &wgpu::TextureView,
    id: &mut Option<egui::TextureId>,
) {
    if let Some(existing) = *id {
        ctx.renderer.update_egui_texture_from_wgpu_texture(
            ctx.device,
            view,
            wgpu::FilterMode::Linear,
            existing,
        );
    } else {
        *id = Some(ctx.renderer.register_native_texture(
            ctx.device,
            view,
            wgpu::FilterMode::Linear,
        ));
    }
}

// ─── bind group layout entry helpers ─────────────────────────────────────────

pub(crate) fn storage_rw_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

pub(crate) fn storage_ro_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

pub(crate) fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

pub(crate) fn storage_tex_write_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: PANEL_FORMAT,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

/// Bind group layout entry for a 2D float texture (non-filterable, for textureLoad).
pub(crate) fn texture_2d_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// Paint the GPU panel texture filling the available space.
///
/// Returns the size used. Pass that back into [`GpuPanel::tick`] next frame.
/// If no texture exists yet, allocates space without painting (the scope shows
/// blank until its first compute tick completes).
pub fn show_panel(panel: &dyn GpuPanel, ui: &mut egui::Ui, square: bool) -> egui::Vec2 {
    let avail = ui.available_size();
    let size = if square {
        egui::Vec2::splat(avail.x.min(avail.y).max(1.0))
    } else {
        egui::vec2(avail.x.max(1.0), avail.y.max(1.0))
    };
    let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());
    if let Some(tid) = panel.texture_id() {
        let (bucket_w, bucket_h) = panel.physical_bucket_size();
        let uv_max = if bucket_w > 0 && bucket_h > 0 {
            egui::pos2(size.x / bucket_w as f32, size.y / bucket_h as f32)
        } else {
            egui::pos2(1.0, 1.0)
        };
        let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, uv_max);
        ui.painter().image(tid, rect, uv, egui::Color32::WHITE);
    }
    size
}

#[cfg(test)]
mod wgsl_validation {
    //! Headless naga validation of every scope's WGSL. wgpu only validates a shader
    //! at pipeline creation (on a real device), so a typo in a compute shader passes
    //! `cargo build` and fails at runtime — this parses AND fully validates each
    //! source so `cargo test` catches it without a GPU.

    fn validate(label: &str, src: &str) {
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("[{label}] WGSL parse failed:\n{}", e.emit_to_string(src)));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|e| panic!("[{label}] WGSL validation failed: {e:?}"));
    }

    #[test]
    fn all_scope_shaders_validate() {
        let sources = super::histogram::WGSL_SOURCES
            .iter()
            .chain(super::waveform::WGSL_SOURCES)
            .chain(super::vectorscope::WGSL_SOURCES)
            .chain(super::parade::WGSL_SOURCES)
            .chain(super::falsecolor::WGSL_SOURCES);
        for (label, src) in sources {
            validate(label, src);
        }
    }
}
