//! Show an HDR/LDR RGBA image inside egui via wgpu.
//!
//! [`HdrView`] owns a small present pipeline (a textured quad) and uploads interleaved
//! RGBA pixel buffers (`u8` / `f16` / `f32`). Each frame the caller
//! [`stage_frame`](HdrView::stage_frame)s the bytes + a model/view/proj transform, then
//! the [`HdrPaintCallback`] draws it in egui's wgpu render pass.
//!
//! **Colour management is upstream.** Exposure and the ACES 2.0 display transform are
//! applied by the host (playa PLAN9.md W3) before staging. This presenter is a dumb
//! sample — no `is_hdr` gate, no Narkowicz/Reinhard tonemap, no gamma. The public
//! `exposure` / `gamma` / `tonemap` fields remain as API stubs until callers drop them.
//!
//! ## Staging costs nothing
//! [`stage_frame`](HdrView::stage_frame) takes a [`PixelBytes`] — a shared
//! handle — and NOT a `Vec<u8>`, because the upload only needs to READ the
//! bytes. Owning them forced every caller to hand over a buffer it could give
//! away, i.e. to copy: playa was allocating and memcpy-ing a fresh 33 MB `Vec`
//! for every 4K frame it displayed, which measured at 18 ms — about 80 % of its
//! whole UI frame — to move bytes that were already sitting in memory in exactly
//! the right layout. A shared handle keeps the producer's buffer alive until the
//! callback runs, which is all this ever needed.
//!
//! Any `Arc<T> where T: AsRef<[u8]> + Send + Sync` coerces, so `Arc<Vec<u8>>`
//! works out of the box; a producer with its own pixel type implements
//! `AsRef<[u8]>` on it once and passes its own `Arc` untouched, whatever the
//! format.
//!
//! Unlike `egui-gpu-view` (which displays an already-rendered offscreen texture),
//! this presents CPU pixel buffers.
//!
//! ## Frame
//! 1. Once: [`HdrView::set_output_format`] with the egui target format (do it
//!    every frame from `RenderState::target_format` if it can change).
//! 2. Each frame: [`HdrView::stage_frame`] with the RGBA bytes (u8: 4·w·h,
//!    f16: 8·w·h little-endian, f32: 16·w·h) + the transform.
//! 3. Add a paint callback wrapping [`HdrPaintCallback`] over the shared
//!    `Arc<Mutex<HdrView>>`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::PaintCallbackInfo;
use egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};
use wgpu::util::DeviceExt;

const WGSL: &str = include_str!("hdr_image.wgsl");

/// On-disk / on-wire pixel format of the staged RGBA buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdrFormat {
    /// 8-bit per channel. 4 bytes/pixel.
    Rgba8,
    /// 16-bit half float per channel. 8 bytes/pixel.
    Rgba16F,
    /// 32-bit float per channel. 16 bytes/pixel.
    Rgba32F,
}

impl HdrFormat {
    /// The texture format the staged bytes upload as, for a render target of `target`.
    ///
    /// 8-bit codes are display-encoded. An `*Srgb` target encodes what the shader writes,
    /// so the sample must come back decoded (`Rgba8UnormSrgb`); any other target (a
    /// `*Unorm` surface, egui-display's extended-sRGB float canvas) stores what the shader
    /// writes, so the sample must stay the code (`Rgba8Unorm`). Either way the code lands
    /// as itself. Uploading `Rgba8UnormSrgb` into the float canvas had shown 8-bit mid
    /// grey (code 128, 0.502) as its decode 0.216: two stops dark.
    fn wgpu(self, target: wgpu::TextureFormat) -> wgpu::TextureFormat {
        match self {
            HdrFormat::Rgba8 if target.is_srgb() => wgpu::TextureFormat::Rgba8UnormSrgb,
            HdrFormat::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
            HdrFormat::Rgba16F => wgpu::TextureFormat::Rgba16Float,
            HdrFormat::Rgba32F => wgpu::TextureFormat::Rgba32Float,
        }
    }
    /// Bytes per pixel of the staged buffer.
    fn bpp(self) -> u32 {
        match self {
            HdrFormat::Rgba8 => 4,
            HdrFormat::Rgba16F => 8,
            HdrFormat::Rgba32F => 16,
        }
    }
}

/// Stub display-transfer selector. The fragment shader is a passthrough; playa
/// applies exposure + ACES 2.0 OT upstream (PLAN9.md W3). Kept until W4 deletes
/// the viewport shader combo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tonemap {
    #[default]
    None,
    Reinhard,
    Aces,
}

impl Tonemap {
    fn code(self) -> u32 {
        match self {
            Tonemap::None => 0,
            Tonemap::Reinhard => 1,
            Tonemap::Aces => 2,
        }
    }
}

/// Model / view / projection transform for the image quad (row-major 4×4).
#[derive(Clone, Copy, Debug)]
pub struct Mvp {
    pub model: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
    pub proj: [[f32; 4]; 4],
}

impl Default for Mvp {
    fn default() -> Self {
        let id = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        Self {
            model: id,
            view: id,
            proj: id,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct VsUniforms {
    model: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    proj: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FsUniforms {
    exposure: f32,
    gamma: f32,
    is_hdr: u32,
    tonemap_mode: u32,
}

/// Shared handle to the interleaved RGBA bytes of one frame.
///
/// Shared rather than owned because the upload happens LATER, inside the egui
/// wgpu callback, so a plain borrow cannot live long enough — but nothing here
/// ever needs to mutate or keep the bytes. `Arc<Vec<u8>>` satisfies it directly;
/// so does an `Arc` of any producer type that implements `AsRef<[u8]>`, which is
/// how a caller avoids re-packing its pixels into a `Vec` it does not need.
pub type PixelBytes = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// CPU → GPU upload queued before the egui wgpu callback runs.
struct StagedUpload {
    format: HdrFormat,
    bytes: PixelBytes,
    w: u32,
    h: u32,
}

/// wgpu resources + state for presenting one HDR/LDR image in egui.
pub struct HdrView {
    /// API stub — not applied in the shader (OT is upstream). Prefer 1.0.
    pub exposure: f32,
    /// API stub — not applied in the shader.
    pub gamma: f32,
    /// API stub — shader is always passthrough.
    pub tonemap: Tonemap,
    output_format: Option<wgpu::TextureFormat>,
    sampler: Option<wgpu::Sampler>,
    quad_vbo: Option<wgpu::Buffer>,
    vs_buf: Option<wgpu::Buffer>,
    fs_buf: Option<wgpu::Buffer>,
    bind_group: Option<wgpu::BindGroup>,
    bgl: Option<wgpu::BindGroupLayout>,
    pip_layout: Option<wgpu::PipelineLayout>,
    shader: Option<wgpu::ShaderModule>,
    pipelines: HashMap<(wgpu::TextureFormat, wgpu::TextureFormat), wgpu::RenderPipeline>,
    image_tex: Option<wgpu::Texture>,
    image_view: Option<wgpu::TextureView>,
    tex_w: u32,
    tex_h: u32,
    tex_format: HdrFormat,
    tex_native_format: Option<wgpu::TextureFormat>,
    last_error: Option<String>,
    staged: Option<StagedUpload>,
    queued_mvp: Option<Mvp>,
}

impl Default for HdrView {
    fn default() -> Self {
        Self::new()
    }
}

impl HdrView {
    pub fn new() -> Self {
        Self {
            exposure: 1.0,
            gamma: 2.2,
            tonemap: Tonemap::None,
            output_format: None,
            sampler: None,
            quad_vbo: None,
            vs_buf: None,
            fs_buf: None,
            bind_group: None,
            bgl: None,
            pip_layout: None,
            shader: None,
            pipelines: HashMap::new(),
            image_tex: None,
            image_view: None,
            tex_w: 0,
            tex_h: 0,
            tex_format: HdrFormat::Rgba8,
            tex_native_format: None,
            last_error: None,
            staged: None,
            queued_mvp: None,
        }
    }

    /// Set the egui/swap-chain target format. Call once (or each frame from
    /// `RenderState::target_format`) before the paint callback runs.
    pub fn set_output_format(&mut self, format: wgpu::TextureFormat) {
        self.output_format = Some(format);
    }

    /// Last shader/pipeline error, if any.
    pub fn last_error(&self) -> Option<String> {
        self.last_error.clone()
    }

    /// `true` if the staged image's dimensions differ from the live texture.
    pub fn needs_texture_update(&self, width: usize, height: usize) -> bool {
        width as u32 != self.tex_w || height as u32 != self.tex_h || self.image_tex.is_none()
    }

    /// Stage `rgba_bytes` (interleaved RGBA in `format`; see crate docs for the
    /// expected byte count) for upload, with the quad transform for this frame.
    ///
    /// Cheap by construction: the handle is cloned, the pixels are not.
    pub fn stage_frame(
        &mut self,
        format: HdrFormat,
        rgba_bytes: PixelBytes,
        width: usize,
        height: usize,
        mvp: Mvp,
    ) {
        self.queued_mvp = Some(mvp);
        self.tex_format = format;
        self.staged = Some(StagedUpload {
            format,
            bytes: rgba_bytes,
            w: width as u32,
            h: height as u32,
        });
    }

    /// Update the transform without re-uploading pixels (e.g. pan/zoom only).
    pub fn skip_upload_this_frame(&mut self, mvp: Mvp) {
        self.queued_mvp = Some(mvp);
        self.staged = None;
    }

    fn ensure_basics(&mut self, device: &wgpu::Device) -> Result<(), String> {
        if self.quad_vbo.is_none() {
            let quad: [[f32; 4]; 6] = [
                [-0.5, -0.5, 0.0, 1.0],
                [0.5, -0.5, 1.0, 1.0],
                [0.5, 0.5, 1.0, 0.0],
                [-0.5, -0.5, 0.0, 1.0],
                [0.5, 0.5, 1.0, 0.0],
                [-0.5, 0.5, 0.0, 0.0],
            ];
            self.quad_vbo = Some(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("hdr_view_quad_vbo"),
                    contents: bytemuck::cast_slice(&quad),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            );
        }
        if self.vs_buf.is_none() {
            self.vs_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("hdr_view_vs_uni"),
                size: std::mem::size_of::<VsUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if self.fs_buf.is_none() {
            self.fs_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("hdr_view_fs_uni"),
                size: std::mem::size_of::<FsUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if self.sampler.is_none() {
            self.sampler = Some(device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("hdr_view_sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }));
        }
        if self.shader.is_none() {
            self.shader = Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("hdr_image"),
                source: wgpu::ShaderSource::Wgsl(WGSL.into()),
            }));
            self.last_error = None;
        }
        if self.bgl.is_none() {
            let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("hdr_view_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
            self.pip_layout = Some(device.create_pipeline_layout(
                &wgpu::PipelineLayoutDescriptor {
                    label: Some("hdr_view_pip_layout"),
                    bind_group_layouts: &[Some(&bgl)],
                    immediate_size: 0,
                },
            ));
            self.bgl = Some(bgl);
        }
        Ok(())
    }

    fn ensure_pipeline(
        &mut self,
        device: &wgpu::Device,
        target_fmt: wgpu::TextureFormat,
        src: HdrFormat,
    ) -> Result<(), String> {
        let img_native = src.wgpu(target_fmt);
        let key = (target_fmt, img_native);
        if self.pipelines.contains_key(&key) {
            return Ok(());
        }
        let shader = self
            .shader
            .as_ref()
            .ok_or_else(|| "no shader".to_string())?;
        let pip_layout = self
            .pip_layout
            .as_ref()
            .ok_or_else(|| "no layout".to_string())?;
        let pl = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hdr_view_present"),
            layout: Some(pip_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                // wgpu 30: VertexState.buffers is now &[Option<VertexBufferLayout>].
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_fmt,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.insert(key, pl);
        Ok(())
    }

    fn recreate_texture(
        &mut self,
        device: &wgpu::Device,
        w: u32,
        h: u32,
        native: wgpu::TextureFormat,
    ) {
        self.bind_group = None;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hdr_view_tex"),
            size: wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: native,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.image_tex = Some(tex);
        self.image_view = Some(view);
        self.tex_w = w;
        self.tex_h = h;
        self.tex_native_format = Some(native);
    }

    fn rebuild_bind_group(&mut self, device: &wgpu::Device) {
        let (Some(bgl), Some(vs), Some(fs), Some(view), Some(samp)) = (
            self.bgl.as_ref(),
            self.vs_buf.as_ref(),
            self.fs_buf.as_ref(),
            self.image_view.as_ref(),
            self.sampler.as_ref(),
        ) else {
            return;
        };
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hdr_view_bind_group"),
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: vs.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: fs.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(samp),
                },
            ],
        }));
    }

    fn apply_staged(&mut self, queue: &wgpu::Queue, device: &wgpu::Device) {
        let Some(st) = self.staged.take() else {
            return;
        };
        let Some(target) = self.output_format else {
            self.staged = Some(st);
            return;
        };
        let native = st.format.wgpu(target);
        if st.w != self.tex_w
            || st.h != self.tex_h
            || self.image_tex.is_none()
            || self.tex_native_format != Some(native)
        {
            self.recreate_texture(device, st.w, st.h, native);
            self.rebuild_bind_group(device);
        }
        if let Some(ref tex) = self.image_tex {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                // Deref past the `Arc`'s own `AsRef` to reach the payload's.
                (*st.bytes).as_ref(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(st.format.bpp() * st.w),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: st.w,
                    height: st.h,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    fn write_uniforms(&self, queue: &wgpu::Queue) {
        let Some(mvp) = self.queued_mvp else {
            return;
        };
        let vs = VsUniforms {
            model: mvp.model,
            view: mvp.view,
            proj: mvp.proj,
        };
        let fs = FsUniforms {
            exposure: self.exposure,
            gamma: self.gamma,
            // Unused by the passthrough FS; kept for uniform layout stability.
            is_hdr: 0,
            tonemap_mode: self.tonemap.code(),
        };
        if let Some(ref b) = self.vs_buf {
            queue.write_buffer(b, 0, bytemuck::bytes_of(&vs));
        }
        if let Some(ref b) = self.fs_buf {
            queue.write_buffer(b, 0, bytemuck::bytes_of(&fs));
        }
    }

    /// Drop all GPU handles (call on shutdown / device loss).
    pub fn destroy(&mut self) {
        self.bind_group = None;
        self.pipelines.clear();
        self.image_view = None;
        self.image_tex = None;
        self.shader = None;
        self.pip_layout = None;
        self.bgl = None;
        self.sampler = None;
        self.vs_buf = None;
        self.fs_buf = None;
        self.quad_vbo = None;
        self.tex_w = 0;
        self.tex_h = 0;
        self.tex_native_format = None;
    }
}

/// egui ↔ wgpu bridge: uploads in `prepare`, draws the quad in `paint`. Wrap a
/// shared `Arc<Mutex<HdrView>>` and add it as an egui paint callback.
#[derive(Clone)]
pub struct HdrPaintCallback {
    pub inner: Arc<Mutex<HdrView>>,
}

impl CallbackTrait for HdrPaintCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        _res: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Ok(mut guard) = self.inner.lock() else {
            return Vec::new();
        };
        let Some(output_fmt) = guard.output_format else {
            guard.last_error =
                Some("HdrView: missing output_format (call set_output_format)".to_string());
            return Vec::new();
        };
        if let Err(e) = guard.ensure_basics(device) {
            guard.last_error = Some(e);
            return Vec::new();
        }
        let tex_fmt = guard.tex_format;
        if let Err(e) = guard.ensure_pipeline(device, output_fmt, tex_fmt) {
            guard.last_error = Some(e);
            return Vec::new();
        }
        guard.apply_staged(queue, device);
        if guard.image_tex.is_some() && guard.bind_group.is_none() && guard.image_view.is_some() {
            guard.rebuild_bind_group(device);
        }
        guard.write_uniforms(queue);
        Vec::new()
    }

    fn paint(
        &self,
        info: PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _res: &CallbackResources,
    ) {
        let Ok(guard) = self.inner.lock() else {
            return;
        };
        let Some(output_fmt) = guard.output_format else {
            return;
        };
        let native = guard.tex_format.wgpu(output_fmt);
        let (Some(pipeline), Some(bg), Some(vbo)) = (
            guard.pipelines.get(&(output_fmt, native)),
            guard.bind_group.as_ref(),
            guard.quad_vbo.as_ref(),
        ) else {
            return;
        };

        let clip = info.clip_rect_in_pixels();
        render_pass.set_scissor_rect(
            clip.left_px.max(0) as u32,
            clip.top_px.max(0) as u32,
            clip.width_px.max(1) as u32,
            clip.height_px.max(1) as u32,
        );
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, bg, &[]);
        render_pass.set_vertex_buffer(0, vbo.slice(..));
        render_pass.draw(0..6, 0..1);
    }
}
