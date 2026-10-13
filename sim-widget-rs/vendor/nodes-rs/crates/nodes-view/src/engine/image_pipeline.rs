//! Per-node image-preview pipeline (`image.wgsl`): draws a node's cooked RGBA8 image as a textured
//! quad on its body rect. Group 0 reuses the shared camera/graph-plane uniform (same
//! `ViewPlaneUniforms` buffer the SDF shape pass binds); group 1 is the node's texture + sampler.
//!
//! One texture per node is cached ([`CachedImageTexture`]) and re-uploaded only when the node's
//! `cooked_gen` changes or its dimensions differ (see the renderer's upload trigger) — the pixels
//! are the expensive part, so we never re-`write_texture` an unchanged image.

use bytemuck::{Pod, Zeroable};
use nodes_core::NodeId;
use wgpu::include_wgsl;
use wgpu::util::DeviceExt;

/// One node's image-preview placement (metadata only — the RGBA8 pixels are fetched lazily on a
/// cache miss, keyed by `gen`, so an unchanged image is never re-read). Collected in `shape_scene`
/// (where each node's draw-rank/Title-layer depth is in scope) and consumed by the renderer.
#[derive(Clone, Copy)]
pub struct ImagePreview {
    pub node_id: NodeId,
    /// Body content rect in graph-UV (the same padded `regions.body` the Live2D swatch uses).
    pub aabb_min: [f32; 2],
    pub aabb_max: [f32; 2],
    /// Node rotation pivot + angle (radians) — matches the SDF body's placement.
    pub pivot: [f32; 2],
    pub rotation: f32,
    /// Node's Title-layer reversed-Z depth bias (so the image draws above its body fill).
    pub depth_bias: f32,
    pub w: u32,
    pub h: u32,
    /// The node's `cooked_gen` when this preview was collected — the texture-cache freshness key.
    pub cooked_gen: u64,
}

/// Unit-quad vertex (shared with the shape pipeline's layout — one `[f32;2]` corner in `[0,1]`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    corner: [f32; 2],
}

const QUAD: [QuadVertex; 6] = [
    QuadVertex { corner: [0.0, 0.0] },
    QuadVertex { corner: [1.0, 0.0] },
    QuadVertex { corner: [1.0, 1.0] },
    QuadVertex { corner: [0.0, 0.0] },
    QuadVertex { corner: [1.0, 1.0] },
    QuadVertex { corner: [0.0, 1.0] },
];

/// Per-preview instance: the body AABB (graph-UV), the node pivot + rotation, and the reversed-Z
/// depth bias (the node's Title-layer depth, so the image sits above its body fill).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ImageInstanceGpu {
    pub aabb_min: [f32; 2],
    pub aabb_max: [f32; 2],
    pub pivot: [f32; 2],
    pub rotation: f32,
    pub depth_bias: f32,
}

/// A CPU-side RGBA8 image awaiting upload: `pixels` is `[h, w, 4]`, top row first.
///
/// The dimensions and the buffer are one value — `pixels.len()` is only meaningful as
/// `w * h * 4` — so they travel together rather than as three positional arguments that a
/// caller could transpose.
#[derive(Clone, Copy)]
pub struct RgbaImage<'a> {
    pub w: u32,
    pub h: u32,
    pub pixels: &'a [u8],
}

/// A node's uploaded preview texture + its group-1 bind group, tagged with the `cooked_gen` and
/// dimensions it was built from (so the renderer knows when to re-upload).
pub struct CachedImageTexture {
    pub bind_group: wgpu::BindGroup,
    pub cooked_gen: u64,
    pub w: u32,
    pub h: u32,
    // Kept alive for `bind_group` (which borrows the view, which borrows the texture).
    _view: wgpu::TextureView,
    _texture: wgpu::Texture,
}

pub struct ImagePipeline {
    pub pipeline: wgpu::RenderPipeline,
    /// Group 0 layout (camera/graph-plane uniform) — structurally identical to the shape pipeline's.
    pub uniform_bgl: wgpu::BindGroupLayout,
    /// Group 1 layout (texture + sampler).
    pub texture_bgl: wgpu::BindGroupLayout,
    quad_buffer: wgpu::Buffer,
}

impl ImagePipeline {
    /// Build the image-preview pipeline for the given color format / depth format / MSAA count
    /// (must match the shape color pass so the two share the Pass-2 targets).
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let uniform_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nodes_view_image_uniform_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nodes_view_image_texture_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nodes_view_image_pl"),
            bind_group_layouts: &[Some(&uniform_bgl), Some(&texture_bgl)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(include_wgsl!("image.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nodes_view_image"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &image_vertex_buffers().map(Some), // wgpu 30: &[Option<VBL>]
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            // Same reversed-Z depth as the shape color pass (GreaterEqual, clear 0.0).
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..wgpu::MultisampleState::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let quad_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("nodes_view_image_quad"),
            contents: bytemuck::cast_slice(&QUAD),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self {
            pipeline,
            uniform_bgl,
            texture_bgl,
            quad_buffer,
        }
    }

    /// Build the group-0 (camera uniform) bind group from the shared `ViewPlaneUniforms` buffer.
    pub fn uniform_bind_group(
        &self,
        device: &wgpu::Device,
        uniform_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nodes_view_image_uniform_bg"),
            layout: &self.uniform_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        })
    }

    /// Create a texture from an [`RgbaImage`], upload it, and build the group-1 bind group.
    /// `cooked_gen` tags the cache entry with the node's `cooked_gen`.
    pub fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sampler: &wgpu::Sampler,
        image: RgbaImage<'_>,
        cooked_gen: u64,
    ) -> CachedImageTexture {
        let RgbaImage { w, h, pixels: rgba } = image;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nodes_view_image_preview"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // sRGB so the shader's sampled color displays with correct gamma (photographic preview).
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nodes_view_image_texture_bg"),
            layout: &self.texture_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        CachedImageTexture {
            bind_group,
            cooked_gen,
            w,
            h,
            _view: view,
            _texture: texture,
        }
    }

    /// Draw one preview instance (index `index` in `instance_buffer`) with its `tex_bg` texture.
    pub fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        uniform_bg: &'a wgpu::BindGroup,
        tex_bg: &'a wgpu::BindGroup,
        instance_buffer: &'a wgpu::Buffer,
        index: u32,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, uniform_bg, &[]);
        pass.set_bind_group(1, tex_bg, &[]);
        pass.set_vertex_buffer(0, self.quad_buffer.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        pass.draw(0..6, index..index + 1);
    }
}

fn image_vertex_buffers() -> [wgpu::VertexBufferLayout<'static>; 2] {
    [
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        },
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageInstanceGpu>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 28,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        },
    ]
}
