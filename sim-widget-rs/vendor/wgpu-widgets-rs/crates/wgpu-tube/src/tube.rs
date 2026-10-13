//! Procedural cubic-bezier **tube renderer** for wgpu using GPU vertex pulling.
//!
//! # What
//!
//! `wgpu-tube` draws many animated 3D tubes (pipes/wires) where each tube is defined by one
//! compact per-instance record ([`TubeInstance`]). The GPU expands this data into shaded
//! triangles in the vertex shader, using a per-instance rotation-minimizing frame computed by a
//! compute pre-pass.
//!
//! # Why this design
//!
//! CPU-side per-tube meshing does not scale when thousands of curves are rebuilt every frame.
//! This crate uses one reusable parametric template mesh (topology only) and one draw call for
//! all tubes:
//!
//! - **Template vertex buffer (step = Vertex)** stores only `(i, j, part)` parametric addresses.
//! - **Template index buffer** stores static triangle topology for body + two cap surfaces.
//! - **Instance buffer (step = Instance)** stores curve control points, endpoint radii/colors,
//!   and cap modes. It is also bound as a read-only storage buffer for the compute pre-pass.
//!
//! # Frame construction: GPU rotation-minimizing frame (RMF)
//!
//! A naive per-vertex frame that picks the "least-aligned world axis" as reference **twists**:
//! as the curve tangent rotates through that axis, the chosen reference flips and the whole ring
//! rolls. To avoid this we compute a proper **rotation-minimizing frame** with a compute
//! pre-pass using the *double-reflection* method (Wang, Jüttler, Zheng, Liu 2008,
//! "Computation of Rotation Minimizing Frames").
//!
//! Pipeline per frame:
//!
//! 1. [`TubeRenderer::encode_prepass`] dispatches one compute invocation per tube. Each invocation
//!    walks `M + 1` centerline samples, seeds an arbitrary reference normal at `u = 0`, and
//!    propagates it with the double-reflection step. Results are written to a `frames` storage
//!    buffer as [`TubeFrame`] records at index `instance * (M + 1) + i`.
//! 2. The vertex shader reads `frames[instance * (M + 1) + i]` to get position `P`, tangent `T`
//!    and rotation-minimized normal `N`, re-orthonormalizes `(T, N, B)`, and places the ring
//!    vertex. No axis-picking, so no twist.
//!
//! Because the reference normal is only *seeded* arbitrarily at the start and then transported
//! continuously, the exact seed does not matter — the frame stays coherent along the curve and
//! across curves that bend out of plane (helices, out-of-plane S-curves).
//!
//! # wgpu feature / downlevel caveat
//!
//! The vertex shader reads a **read-only storage buffer** (`frames`). This is core WebGPU and
//! works on native Metal/Vulkan/DX12 backends without extra features. Some downlevel GLES targets
//! restrict storage buffers in the vertex stage; on such adapters this renderer needs a backend
//! that reports vertex-stage storage support. No `unsafe` and no optional wgpu features are used.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

const PART_BODY: u32 = 0;
const PART_START_CAP: u32 = 1;
const PART_END_CAP: u32 = 2;

const DEFAULT_LIGHT_DIR: [f32; 4] = [0.35, 0.65, 0.45, 0.0];

/// Compute workgroup size (invocations per workgroup) for the RMF pre-pass.
const RMF_WORKGROUP_SIZE: u32 = 64;

/// Tube cap mode encoded into [`TubeInstance::caps`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CapKind {
    /// Disk in the end plane.
    Flat = 0,
    /// Hemisphere oriented along tangent.
    Round = 1,
    /// Short box-like extrusion from the end plane.
    Cube = 2,
    /// Cone-like arrowhead (flare then tip).
    Arrow = 3,
}

impl CapKind {
    #[inline]
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Round,
            2 => Self::Cube,
            3 => Self::Arrow,
            _ => Self::Flat,
        }
    }
}

/// Pack start/end cap kinds into [`TubeInstance::caps`].
#[inline]
pub fn pack_caps(start: CapKind, end: CapKind) -> u32 {
    (start as u32) | ((end as u32) << 8)
}

/// GPU instance payload for one tube.
///
/// Layout is `#[repr(C)]` + [`Pod`] so it can be uploaded directly with
/// `bytemuck::cast_slice`.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct TubeInstance {
    /// `xyz`: bezier start position, `w`: radius at `u=0`.
    pub p0: [f32; 4],
    /// `xyz`: bezier control point 1, `w`: reserved.
    pub p1: [f32; 4],
    /// `xyz`: bezier control point 2, `w`: reserved.
    pub p2: [f32; 4],
    /// `xyz`: bezier end position, `w`: radius at `u=1`.
    pub p3: [f32; 4],
    /// RGBA at `u=0`.
    pub color0: [f32; 4],
    /// RGBA at `u=1`.
    pub color1: [f32; 4],
    /// Packed caps: `start_cap | (end_cap << 8)` where each cap is [`CapKind`] as `u8`.
    pub caps: u32,
    /// Padding keeps 16-byte alignment for the trailing `u32x4` vertex attribute.
    pub _pad: [u32; 3],
}

/// One rotation-minimized centerline sample produced by the compute pre-pass.
///
/// # Layout
///
/// `#[repr(C)]` + [`Pod`], padded to 16-byte alignment as three `vec4<f32>` so it maps directly
/// onto a WGSL `array<TubeFrame>` storage element (48 bytes):
///
/// - `pos_u`: centerline position in `xyz`, curve parameter `u` in `w`.
/// - `tangent`: normalized curve tangent in `xyz`, `w` reserved (0).
/// - `normal`: rotation-minimized unit normal in `xyz`, `w` reserved (0).
///
/// The binormal is not stored: the vertex shader derives it as `B = normalize(cross(T, N))` and
/// re-orthonormalizes `N` so the ring basis is exactly orthonormal even after float drift.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct TubeFrame {
    /// Centerline position `xyz`, curve parameter `u` in `w`.
    pub pos_u: [f32; 4],
    /// Normalized tangent `xyz`, `w` reserved.
    pub tangent: [f32; 4],
    /// Rotation-minimized unit normal `xyz`, `w` reserved.
    pub normal: [f32; 4],
}

/// Renderer configuration.
#[derive(Clone, Copy, Debug)]
pub struct TubeRendererDesc {
    /// Color render target format.
    pub color_format: wgpu::TextureFormat,
    /// Optional depth attachment format.
    pub depth_format: Option<wgpu::TextureFormat>,
    /// MSAA sample count.
    pub sample_count: u32,
    /// Ring tessellation count (`N`) around circumference.
    pub ring_segments: u32,
    /// Longitudinal tessellation count (`M`) along curve length.
    pub length_segments: u32,
    /// Use reversed-Z depth compare (`GreaterEqual`) when `true`.
    pub reversed_z: bool,
}

impl Default for TubeRendererDesc {
    fn default() -> Self {
        Self {
            color_format: wgpu::TextureFormat::Bgra8Unorm,
            depth_format: Some(wgpu::TextureFormat::Depth32Float),
            sample_count: 1,
            ring_segments: 8,
            length_segments: 36,
            reversed_z: false,
        }
    }
}

/// How a tube's `radius` (per-instance `p0.w`/`p3.w`) is interpreted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TubeWidthMode {
    /// Radius is in WORLD units — the tube is a real 3D pipe (thin far, thick near). Stable and
    /// self-consistent; the honest 3D look.
    #[default]
    World,
    /// Radius is in SCREEN PIXELS — the vertex shader converts it to world PER VERTEX using that
    /// vertex's own depth (`clip.w`), so the tube keeps a constant on-screen thickness at any
    /// depth or camera angle. Robust: `clip.w` is bounded for visible geometry, so the width can
    /// never "explode" the way a single host-side `radius / px_per_world` division did.
    ScreenPx,
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
    /// `x`: pixel scale `0.5 * viewport.y * (1/tan(fov_y/2))` (world→px factor at `clip.w = 1`);
    /// `y`: width mode (`0` = World, `1` = ScreenPx); `z`,`w`: reserved.
    params: [f32; 4],
}

/// Compute-pass parameters: `length_segments` (M), current `instance_count`, and padding to
/// 16 bytes for uniform alignment.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct RmfParams {
    length_segments: u32,
    instance_count: u32,
    _pad: [u32; 2],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct TemplateVertex {
    ijp: [u32; 3],
    _pad: u32,
}

#[derive(Debug)]
struct TemplateMesh {
    vertices: Vec<TemplateVertex>,
    indices: Vec<u32>,
}

/// Draws all submitted tubes in one indexed-instanced call, backed by a compute RMF pre-pass.
pub struct TubeRenderer {
    pipeline: wgpu::RenderPipeline,
    compute_pipeline: wgpu::ComputePipeline,
    render_bgl: wgpu::BindGroupLayout,
    compute_bgl: wgpu::BindGroupLayout,
    render_bg: wgpu::BindGroup,
    compute_bg: wgpu::BindGroup,
    camera_buffer: wgpu::Buffer,
    params_buffer: wgpu::Buffer,
    template_vertex_buffer: wgpu::Buffer,
    template_index_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    frames_buffer: wgpu::Buffer,
    instance_capacity: usize,
    instance_count: u32,
    index_count: u32,
    ring_segments: u32,
    length_segments: u32,
    /// Radius interpretation (see [`TubeWidthMode`]); folded into the camera uniform each upload.
    width_mode: TubeWidthMode,
    /// World→pixel scale at `clip.w = 1` (`0.5 * viewport.y / tan(fov_y/2)`), for `ScreenPx`.
    px_scale: f32,
}

impl TubeRenderer {
    /// Create a renderer, its render + compute pipelines, and static template geometry.
    pub fn new(device: &wgpu::Device, desc: TubeRendererDesc) -> Self {
        let ring_segments = desc.ring_segments.max(3);
        let length_segments = desc.length_segments.max(1);

        let template = build_template_mesh(length_segments, ring_segments);
        let index_count = u32::try_from(template.indices.len()).unwrap_or(u32::MAX);

        let template_vertex_buffer = create_initialized_buffer(
            device,
            "wgpu_tube.template_vertices",
            wgpu::BufferUsages::VERTEX,
            bytemuck::cast_slice(&template.vertices),
        );
        let template_index_buffer = create_initialized_buffer(
            device,
            "wgpu_tube.template_indices",
            wgpu::BufferUsages::INDEX,
            bytemuck::cast_slice(&template.indices),
        );

        let identity_camera = CameraUniform {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            light_dir: DEFAULT_LIGHT_DIR,
            params: [1.0, 0.0, 0.0, 0.0],
        };
        let camera_buffer = create_initialized_buffer(
            device,
            "wgpu_tube.camera_uniform",
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            bytemuck::bytes_of(&identity_camera),
        );

        let init_params = RmfParams {
            length_segments,
            instance_count: 0,
            _pad: [0; 2],
        };
        let params_buffer = create_initialized_buffer(
            device,
            "wgpu_tube.rmf_params",
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            bytemuck::bytes_of(&init_params),
        );

        // Group 0 (render): camera uniform (VS + FS) + read-only frames storage (VS).
        let render_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wgpu_tube.render_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        // Compute group: params uniform, read-only instances storage, read-write frames storage.
        let compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wgpu_tube.compute_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("wgpu_tube.render_pipeline_layout"),
                bind_group_layouts: &[Some(&render_bgl)],
                ..Default::default()
            });

        let render_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wgpu_tube.render_shader"),
            source: wgpu::ShaderSource::Wgsl(
                build_render_shader_source(length_segments, ring_segments).into(),
            ),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wgpu_tube.pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: Some("vs_main"),
                // wgpu 30: VertexState.buffers is &[Option<VertexBufferLayout>].
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<TemplateVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Uint32x3,
                        }],
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<TubeInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 0,
                                shader_location: 1,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 16,
                                shader_location: 2,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 32,
                                shader_location: 3,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 48,
                                shader_location: 4,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 64,
                                shader_location: 5,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 80,
                                shader_location: 6,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Uint32x4,
                                offset: 96,
                                shader_location: 7,
                            },
                        ],
                    }),
                ],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: desc.color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: desc.depth_format.map(|format| wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(true),
                depth_compare: Some(if desc.reversed_z {
                    wgpu::CompareFunction::GreaterEqual
                } else {
                    wgpu::CompareFunction::LessEqual
                }),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: desc.sample_count.max(1),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("wgpu_tube.compute_pipeline_layout"),
                bind_group_layouts: &[Some(&compute_bgl)],
                ..Default::default()
            });

        let compute_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wgpu_tube.compute_shader"),
            source: wgpu::ShaderSource::Wgsl(build_compute_shader_source(length_segments).into()),
        });

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("wgpu_tube.compute_pipeline"),
            layout: Some(&compute_pipeline_layout),
            module: &compute_shader,
            entry_point: Some("cs_main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let instance_capacity = 1usize;
        let instance_buffer = create_instance_buffer(device, instance_capacity);
        let frames_buffer = create_frames_buffer(device, instance_capacity, length_segments);

        let render_bg = build_render_bind_group(device, &render_bgl, &camera_buffer, &frames_buffer);
        let compute_bg = build_compute_bind_group(
            device,
            &compute_bgl,
            &params_buffer,
            &instance_buffer,
            &frames_buffer,
        );

        Self {
            pipeline,
            compute_pipeline,
            render_bgl,
            compute_bgl,
            render_bg,
            compute_bg,
            camera_buffer,
            params_buffer,
            template_vertex_buffer,
            template_index_buffer,
            instance_buffer,
            frames_buffer,
            instance_capacity,
            instance_count: 0,
            index_count,
            ring_segments,
            length_segments,
            width_mode: TubeWidthMode::World,
            px_scale: 1.0,
        }
    }

    /// Select how per-instance `radius` is interpreted (World units vs constant screen pixels).
    /// For [`TubeWidthMode::ScreenPx`] the caller must also keep [`Self::set_screen_scale`] current
    /// (viewport height + FOV can change per frame). No-op-cheap: takes effect on the next camera
    /// upload ([`Self::set_camera`] / [`Self::set_camera_with_light`]).
    pub fn set_width_mode(&mut self, mode: TubeWidthMode) {
        self.width_mode = mode;
    }

    /// World→pixel scale for [`TubeWidthMode::ScreenPx`]: `0.5 * viewport.y / tan(fov_y / 2)`
    /// (i.e. how many pixels one world unit spans at `clip.w = 1`). The shader divides by each
    /// vertex's `clip.w` to hold the tube at a constant pixel width regardless of depth.
    pub fn set_screen_scale(&mut self, px_scale: f32) {
        self.px_scale = px_scale.max(1.0e-4);
    }

    /// Upload camera matrix, keeping the default light direction.
    pub fn set_camera(&self, queue: &wgpu::Queue, view_proj: Mat4) {
        self.set_camera_with_light(
            queue,
            view_proj,
            Vec3::new(DEFAULT_LIGHT_DIR[0], DEFAULT_LIGHT_DIR[1], DEFAULT_LIGHT_DIR[2]),
        );
    }

    /// Upload camera matrix and explicit light direction.
    pub fn set_camera_with_light(&self, queue: &wgpu::Queue, view_proj: Mat4, light_dir: Vec3) {
        let mut light = light_dir;
        if light.length_squared() <= 1.0e-8 {
            light = Vec3::new(DEFAULT_LIGHT_DIR[0], DEFAULT_LIGHT_DIR[1], DEFAULT_LIGHT_DIR[2]);
        }
        let light = light.normalize();
        let mode = match self.width_mode {
            TubeWidthMode::World => 0.0,
            TubeWidthMode::ScreenPx => 1.0,
        };
        let camera = CameraUniform {
            view_proj: view_proj.to_cols_array_2d(),
            light_dir: [light.x, light.y, light.z, 0.0],
            params: [self.px_scale, mode, 0.0, 0.0],
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera));
    }

    /// Replace instance data and grow the instance + frames buffers when needed.
    ///
    /// The `frames` storage buffer is sized `capacity * (M + 1) * sizeof(TubeFrame)` and grows in
    /// lockstep with the instance buffer, so the compute pre-pass always has one frame slot per
    /// centerline sample of every live tube.
    pub fn update_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[TubeInstance],
    ) {
        self.instance_count = u32::try_from(instances.len()).unwrap_or(u32::MAX);

        // Keep the compute pass' instance_count in sync even when the set is empty.
        let params = RmfParams {
            length_segments: self.length_segments,
            instance_count: self.instance_count,
            _pad: [0; 2],
        };
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));

        if instances.is_empty() {
            return;
        }

        if instances.len() > self.instance_capacity {
            self.instance_capacity = instances.len().next_power_of_two();
            self.instance_buffer = create_instance_buffer(device, self.instance_capacity);
            self.frames_buffer =
                create_frames_buffer(device, self.instance_capacity, self.length_segments);
            self.render_bg = build_render_bind_group(
                device,
                &self.render_bgl,
                &self.camera_buffer,
                &self.frames_buffer,
            );
            self.compute_bg = build_compute_bind_group(
                device,
                &self.compute_bgl,
                &self.params_buffer,
                &self.instance_buffer,
                &self.frames_buffer,
            );
        }

        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(instances));
    }

    /// Encode the RMF compute pre-pass into `encoder`.
    ///
    /// Consumers **must** call this once per frame *before* their render pass (and before
    /// [`TubeRenderer::draw`]), so the `frames` buffer is populated for the vertex shader. It is a
    /// no-op when there are no instances. Dispatches `ceil(instance_count / 64)` workgroups.
    pub fn encode_prepass(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.instance_count == 0 {
            return;
        }
        let workgroups = self.instance_count.div_ceil(RMF_WORKGROUP_SIZE);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("wgpu_tube.rmf_prepass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.compute_pipeline);
        pass.set_bind_group(0, &self.compute_bg, &[]);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }

    /// Draw all uploaded tubes. Reads the `frames` buffer filled by [`Self::encode_prepass`].
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.instance_count == 0 || self.index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.render_bg, &[]);
        pass.set_vertex_buffer(0, self.template_vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        pass.set_index_buffer(self.template_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..self.instance_count);
    }

    /// Current static ring-segment count (`N`).
    #[inline]
    pub fn ring_segments(&self) -> u32 {
        self.ring_segments
    }

    /// Current static length-segment count (`M`).
    #[inline]
    pub fn length_segments(&self) -> u32 {
        self.length_segments
    }
}

/// Cubic bezier point for parameter `u` in `[0, 1]`.
#[inline]
pub fn bezier_point(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, u: f32) -> Vec3 {
    let t = u.clamp(0.0, 1.0);
    let omt = 1.0 - t;
    (omt * omt * omt) * p0 + (3.0 * omt * omt * t) * p1 + (3.0 * omt * t * t) * p2 + (t * t * t) * p3
}

/// Cubic bezier derivative at parameter `u` in `[0, 1]`.
#[inline]
pub fn bezier_tangent(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, u: f32) -> Vec3 {
    let t = u.clamp(0.0, 1.0);
    let omt = 1.0 - t;
    (3.0 * omt * omt) * (p1 - p0) + (6.0 * omt * t) * (p2 - p1) + (3.0 * t * t) * (p3 - p2)
}

/// One double-reflection RMF step (Wang et al. 2008), the CPU mirror of the compute-shader inner
/// loop.
///
/// Given the previous sample `(x_i, t_i, r_i)` (position, unit tangent, unit reference normal) and
/// the next sample position/tangent `(x_next, t_next)`, returns the transported, re-orthonormalized
/// reference normal `r_{i+1}`. Degenerate reflection planes (`c1`/`c2` below epsilon) fall back to
/// the previous vector so the frame never explodes.
pub fn rmf_step(x_i: Vec3, t_i: Vec3, r_i: Vec3, x_next: Vec3, t_next: Vec3) -> Vec3 {
    let eps = 1.0e-12_f32;

    let v1 = x_next - x_i;
    let c1 = v1.dot(v1);
    let (r_l, t_l) = if c1 < eps {
        (r_i, t_i)
    } else {
        let k1 = 2.0 / c1;
        (r_i - k1 * v1.dot(r_i) * v1, t_i - k1 * v1.dot(t_i) * v1)
    };

    let v2 = t_next - t_l;
    let c2 = v2.dot(v2);
    let mut r_next = if c2 < eps {
        r_l
    } else {
        let k2 = 2.0 / c2;
        r_l - k2 * v2.dot(r_l) * v2
    };

    // Re-orthonormalize against the new tangent to kill accumulated drift.
    r_next -= r_next.dot(t_next) * t_next;
    let len = r_next.length();
    if len > 1.0e-6 {
        r_next / len
    } else {
        r_i
    }
}

/// Seed reference normal at `u = 0`: pick the least-aligned world axis and project it perpendicular
/// to the tangent. The choice is arbitrary — the double-reflection propagation keeps the frame
/// coherent regardless of the seed. Mirrors the compute shader's `seed_normal`; used by tests.
#[cfg(test)]
fn seed_normal(t0: Vec3) -> Vec3 {
    let a = t0.abs();
    let axis = if a.x <= a.y && a.x <= a.z {
        Vec3::X
    } else if a.y <= a.x && a.y <= a.z {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let mut n = axis - axis.dot(t0) * t0;
    if n.length() <= 1.0e-6 {
        n = Vec3::Y - Vec3::Y.dot(t0) * t0;
    }
    n.normalize_or_zero()
}

fn build_template_mesh(length_segments: u32, ring_segments: u32) -> TemplateMesh {
    let m = length_segments.max(1);
    let n = ring_segments.max(3);
    let mut vertices = Vec::new();

    for part in [PART_BODY, PART_START_CAP, PART_END_CAP] {
        for i in 0..=m {
            for j in 0..=n {
                vertices.push(TemplateVertex {
                    ijp: [i, j, part],
                    _pad: 0,
                });
            }
        }
    }

    let per_part_vertices = (m + 1) * (n + 1);
    let mut indices = Vec::new();
    for part_index in 0..3u32 {
        let base = part_index * per_part_vertices;
        append_grid_indices(base, m, n, &mut indices);
    }

    TemplateMesh { vertices, indices }
}

fn append_grid_indices(base: u32, rows: u32, cols: u32, out: &mut Vec<u32>) {
    let stride = cols + 1;
    for i in 0..rows {
        for j in 0..cols {
            let a = base + i * stride + j;
            let b = a + 1;
            let c = a + stride;
            let d = c + 1;
            out.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
}

fn frame_count_per_instance(length_segments: u32) -> usize {
    (length_segments as usize) + 1
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("wgpu_tube.instances"),
        size: buffer_size_bytes::<TubeInstance>(capacity.max(1)),
        usage: wgpu::BufferUsages::VERTEX
            | wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_frames_buffer(
    device: &wgpu::Device,
    capacity: usize,
    length_segments: u32,
) -> wgpu::Buffer {
    let frames = capacity.max(1).saturating_mul(frame_count_per_instance(length_segments));
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("wgpu_tube.frames"),
        size: buffer_size_bytes::<TubeFrame>(frames.max(1)),
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    })
}

fn build_render_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    camera_buffer: &wgpu::Buffer,
    frames_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("wgpu_tube.render_bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: frames_buffer.as_entire_binding(),
            },
        ],
    })
}

fn build_compute_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    instance_buffer: &wgpu::Buffer,
    frames_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("wgpu_tube.compute_bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: instance_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: frames_buffer.as_entire_binding(),
            },
        ],
    })
}

fn buffer_size_bytes<T>(count: usize) -> u64 {
    let elem = std::mem::size_of::<T>();
    let bytes = count.saturating_mul(elem);
    bytes as u64
}

fn create_initialized_buffer(
    device: &wgpu::Device,
    label: &str,
    usage: wgpu::BufferUsages,
    data: &[u8],
) -> wgpu::Buffer {
    let byte_len = data.len().max(1) as u64;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: byte_len,
        usage,
        mapped_at_creation: true,
    });

    if !data.is_empty() {
        {
            let mut mapped = buffer.slice(..).get_mapped_range_mut().expect("mapped_at_creation buffer is always mappable"); // wgpu 30: fallible
            mapped.copy_from_slice(data);
        }
    }

    buffer.unmap();
    buffer
}

/// WGSL shared by both shaders: `TubeFrame`, bezier evaluation, tangent guards.
fn wgsl_common(length_segments: u32) -> String {
    format!(
        r#"
const LENGTH_SEGMENTS_U: u32 = {length_segments}u;
const LENGTH_SEGMENTS: f32 = {length_segments}.0;
const FRAMES_PER_TUBE: u32 = {frames_per_tube}u;

struct TubeFrame {{
    pos_u: vec4<f32>,
    tangent: vec4<f32>,
    normal: vec4<f32>,
}};

fn bezier3(p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>, p3: vec3<f32>, u: f32) -> vec3<f32> {{
    let t = clamp(u, 0.0, 1.0);
    let omt = 1.0 - t;
    return omt * omt * omt * p0
         + 3.0 * omt * omt * t * p1
         + 3.0 * omt * t * t * p2
         + t * t * t * p3;
}}

fn bezier3_tangent(p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>, p3: vec3<f32>, u: f32) -> vec3<f32> {{
    let t = clamp(u, 0.0, 1.0);
    let omt = 1.0 - t;
    return 3.0 * omt * omt * (p1 - p0)
         + 6.0 * omt * t * (p2 - p1)
         + 3.0 * t * t * (p3 - p2);
}}

fn safe_tangent(p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>, p3: vec3<f32>, u: f32) -> vec3<f32> {{
    let raw = bezier3_tangent(p0, p1, p2, p3, u);
    if length(raw) <= 1.0e-6 {{
        return normalize(p3 - p0 + vec3<f32>(1.0e-6, 0.0, 0.0));
    }}
    return normalize(raw);
}}
"#,
        frames_per_tube = frame_count_per_instance(length_segments),
    )
}

/// Compute shader source: one invocation per tube runs the double-reflection RMF and writes
/// `frames`.
fn build_compute_shader_source(length_segments: u32) -> String {
    format!(
        "{common}{body}",
        common = wgsl_common(length_segments),
        body = r#"
struct Instance {
    p0: vec4<f32>,
    p1: vec4<f32>,
    p2: vec4<f32>,
    p3: vec4<f32>,
    color0: vec4<f32>,
    color1: vec4<f32>,
    caps: vec4<u32>,
};

struct Params {
    length_segments: u32,
    instance_count: u32,
    pad0: u32,
    pad1: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> instances: array<Instance>;
@group(0) @binding(2) var<storage, read_write> frames: array<TubeFrame>;

fn seed_normal(t0: vec3<f32>) -> vec3<f32> {
    let a = abs(t0);
    var axis = vec3<f32>(0.0, 0.0, 1.0);
    if a.x <= a.y && a.x <= a.z {
        axis = vec3<f32>(1.0, 0.0, 0.0);
    } else if a.y <= a.x && a.y <= a.z {
        axis = vec3<f32>(0.0, 1.0, 0.0);
    }
    var n = axis - dot(axis, t0) * t0;
    if length(n) <= 1.0e-6 {
        n = vec3<f32>(0.0, 1.0, 0.0) - dot(vec3<f32>(0.0, 1.0, 0.0), t0) * t0;
    }
    return normalize(n);
}

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let inst = gid.x;
    if inst >= params.instance_count {
        return;
    }

    let it = instances[inst];
    let p0 = it.p0.xyz;
    let p1 = it.p1.xyz;
    let p2 = it.p2.xyz;
    let p3 = it.p3.xyz;

    let m = LENGTH_SEGMENTS_U;
    let base = inst * FRAMES_PER_TUBE;

    var x_i = bezier3(p0, p1, p2, p3, 0.0);
    var t_i = safe_tangent(p0, p1, p2, p3, 0.0);
    var r_i = seed_normal(t_i);

    frames[base] = TubeFrame(
        vec4<f32>(x_i, 0.0),
        vec4<f32>(t_i, 0.0),
        vec4<f32>(r_i, 0.0),
    );

    for (var i: u32 = 1u; i <= m; i = i + 1u) {
        let u = f32(i) / LENGTH_SEGMENTS;
        let x_next = bezier3(p0, p1, p2, p3, u);
        let t_next = safe_tangent(p0, p1, p2, p3, u);

        // Double-reflection RMF (Wang et al. 2008).
        let v1 = x_next - x_i;
        let c1 = dot(v1, v1);
        var r_l = r_i;
        var t_l = t_i;
        if c1 >= 1.0e-12 {
            let k1 = 2.0 / c1;
            r_l = r_i - k1 * dot(v1, r_i) * v1;
            t_l = t_i - k1 * dot(v1, t_i) * v1;
        }

        let v2 = t_next - t_l;
        let c2 = dot(v2, v2);
        var r_next = r_l;
        if c2 >= 1.0e-12 {
            let k2 = 2.0 / c2;
            r_next = r_l - k2 * dot(v2, r_l) * v2;
        }

        // Re-orthonormalize against the new tangent.
        r_next = r_next - dot(r_next, t_next) * t_next;
        if length(r_next) > 1.0e-6 {
            r_next = normalize(r_next);
        } else {
            r_next = r_i;
        }

        frames[base + i] = TubeFrame(
            vec4<f32>(x_next, u),
            vec4<f32>(t_next, 0.0),
            vec4<f32>(r_next, 0.0),
        );

        x_i = x_next;
        t_i = t_next;
        r_i = r_next;
    }
}
"#,
    )
}

/// Render shader source: vertex-pulls the template against per-instance data and the RMF `frames`.
fn build_render_shader_source(length_segments: u32, ring_segments: u32) -> String {
    format!(
        "{common}{body}",
        common = wgsl_common(length_segments),
        body = format!(
            r#"
const PART_BODY: u32 = 0u;
const PART_START_CAP: u32 = 1u;
const PART_END_CAP: u32 = 2u;

const CAP_FLAT: u32 = 0u;
const CAP_ROUND: u32 = 1u;
const CAP_CUBE: u32 = 2u;
const CAP_ARROW: u32 = 3u;

const RING_SEGMENTS: f32 = {ring_segments}.0;
const TAU: f32 = 6.28318530718;

struct CameraUniform {{
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    params: vec4<f32>,
}};

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(0) @binding(1) var<storage, read> frames: array<TubeFrame>;

// Constant-screen-px width: the WORLD radius that projects to a fixed pixel size at `center`'s
// depth. `params.y`: 0 = World (radius is already world units -> factor 1); 1 = ScreenPx.
// `params.x`: world->pixel scale at `clip.w = 1`. Per vertex: world_radius = px * clip.w / scale,
// so a nearer segment (small clip.w) shrinks and a far one grows to hold the same on-screen width.
// `clip.w` is bounded for visible geometry, so this can never balloon the way a single host-side
// `radius / px_per_world` division did.
fn tube_radius_scale(center: vec3<f32>) -> f32 {{
    if (camera.params.y < 0.5) {{
        return 1.0;
    }}
    let cw = (camera.view_proj * vec4<f32>(center, 1.0)).w;
    return max(cw, 1.0e-4) / max(camera.params.x, 1.0e-4);
}}

struct VsIn {{
    @location(0) ijp: vec3<u32>,
    @location(1) p0: vec4<f32>,
    @location(2) p1: vec4<f32>,
    @location(3) p2: vec4<f32>,
    @location(4) p3: vec4<f32>,
    @location(5) color0: vec4<f32>,
    @location(6) color1: vec4<f32>,
    @location(7) caps_pack: vec4<u32>,
}};

struct VsOut {{
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) normal_ws: vec3<f32>,
    @location(1) u: f32,
    @location(2) color0: vec4<f32>,
    @location(3) color1: vec4<f32>,
}};

struct Basis {{
    center: vec3<f32>,
    tangent: vec3<f32>,
    normal: vec3<f32>,
    binormal: vec3<f32>,
    u: f32,
}};

fn decode_cap(caps: u32, part: u32) -> u32 {{
    if part == PART_START_CAP {{
        return caps & 255u;
    }}
    return (caps >> 8u) & 255u;
}}

// Read a stored RMF sample and rebuild an orthonormal (T, N, B) basis.
fn basis_at(instance: u32, i: u32) -> Basis {{
    let idx = instance * FRAMES_PER_TUBE + min(i, LENGTH_SEGMENTS_U);
    let f = frames[idx];
    let t = normalize(f.tangent.xyz);
    var n = f.normal.xyz - dot(f.normal.xyz, t) * t;
    if length(n) <= 1.0e-6 {{
        n = f.normal.xyz;
    }}
    n = normalize(n);
    let b = normalize(cross(t, n));
    let n2 = normalize(cross(b, t));
    var out: Basis;
    out.center = f.pos_u.xyz;
    out.tangent = t;
    out.normal = n2;
    out.binormal = b;
    out.u = f.pos_u.w;
    return out;
}}

@vertex
fn vs_main(input: VsIn, @builtin(instance_index) instance: u32) -> VsOut {{
    let i = input.ijp.x;
    let j = f32(input.ijp.y);
    let part = input.ijp.z;

    let theta = (j / RING_SEGMENTS) * TAU;
    let ct = cos(theta);
    let st = sin(theta);

    let r0 = input.p0.w;
    let r1 = input.p3.w;

    var world_pos: vec3<f32>;
    var world_normal: vec3<f32>;
    var out_u: f32;

    if part == PART_BODY {{
        let frame = basis_at(instance, i);
        let radius = max(mix(r0, r1, frame.u), 1.0e-4) * tube_radius_scale(frame.center);
        let radial = normalize(ct * frame.normal + st * frame.binormal);
        world_pos = frame.center + radius * radial;
        world_normal = radial;
        out_u = frame.u;
    }} else {{
        let is_end = part == PART_END_CAP;
        let side = select(-1.0, 1.0, is_end);
        let ring_index = select(0u, LENGTH_SEGMENTS_U, is_end);
        let frame = basis_at(instance, ring_index);
        let end_u = select(0.0, 1.0, is_end);
        out_u = end_u;
        let radius = max(mix(r0, r1, end_u), 1.0e-4) * tube_radius_scale(frame.center);
        let radial = normalize(ct * frame.normal + st * frame.binormal);
        let axis = frame.tangent;
        let cap_mode = decode_cap(input.caps_pack.x, part);
        let cap_u = f32(i) / LENGTH_SEGMENTS;

        if cap_mode == CAP_ROUND {{
            let phi = cap_u * 1.57079632679;
            let axial = side * radius * sin(phi);
            let radial_len = radius * cos(phi);
            world_pos = frame.center + axial * axis + radial_len * radial;
            world_normal = normalize(side * sin(phi) * axis + cos(phi) * radial);
        }} else if cap_mode == CAP_CUBE {{
            let square = 1.0 / max(abs(ct), abs(st));
            let radial_len = radius * square;
            let axial = side * radius * cap_u;
            world_pos = frame.center + radial_len * radial + axial * axis;
            world_normal = normalize(radial_len * radial + 0.35 * axial * axis);
        }} else if cap_mode == CAP_ARROW {{
            let t = clamp(cap_u, 0.0, 1.0);
            var radial_scale = 0.0;
            if t < 0.4 {{
                radial_scale = mix(1.0, 2.0, t / 0.4);
            }} else {{
                radial_scale = mix(2.0, 0.0, (t - 0.4) / 0.6);
            }}
            let radial_len = radius * radial_scale;
            let axial = side * radius * 2.0 * t;
            world_pos = frame.center + radial_len * radial + axial * axis;
            world_normal = normalize(1.2 * radial + (-side) * 0.8 * axis);
        }} else {{
            let radial_len = radius * (1.0 - cap_u);
            world_pos = frame.center + radial_len * radial;
            world_normal = side * -axis;
        }}
    }}

    var out: VsOut;
    out.clip_pos = camera.view_proj * vec4<f32>(world_pos, 1.0);
    out.normal_ws = normalize(world_normal);
    out.u = clamp(out_u, 0.0, 1.0);
    out.color0 = input.color0;
    out.color1 = input.color1;
    return out;
}}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {{
    let color = mix(input.color0, input.color1, input.u);
    let light = normalize(select(vec3<f32>(0.35, 0.65, 0.45), camera.light_dir.xyz, length(camera.light_dir.xyz) > 1.0e-6));
    let lambert = max(dot(normalize(input.normal_ws), light), 0.0);
    let shade = 0.35 + 0.65 * lambert;
    return vec4<f32>(color.rgb * shade, color.a);
}}
"#,
            ring_segments = ring_segments,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_endpoints_match() {
        let p0 = Vec3::new(-1.0, 0.5, 2.0);
        let p1 = Vec3::new(-0.2, 1.2, 0.0);
        let p2 = Vec3::new(0.3, -1.1, 0.3);
        let p3 = Vec3::new(2.0, 0.2, -0.4);

        assert_eq!(bezier_point(p0, p1, p2, p3, 0.0), p0);
        assert_eq!(bezier_point(p0, p1, p2, p3, 1.0), p3);
    }

    #[test]
    fn bezier_tangent_nonzero_for_regular_curve() {
        let p0 = Vec3::new(0.0, 0.0, 0.0);
        let p1 = Vec3::new(0.8, 1.5, 0.0);
        let p2 = Vec3::new(1.6, -1.0, 0.4);
        let p3 = Vec3::new(2.4, 0.2, 0.6);

        let tan = bezier_tangent(p0, p1, p2, p3, 0.5);
        assert!(tan.length_squared() > 1.0e-6);
    }

    #[test]
    fn template_counts_match_grid_and_parts() {
        let m = 9;
        let n = 7;
        let mesh = build_template_mesh(m, n);
        let expected_vertices = (3 * (m + 1) * (n + 1)) as usize;
        let expected_indices = (3 * m * n * 6) as usize;
        assert_eq!(mesh.vertices.len(), expected_vertices);
        assert_eq!(mesh.indices.len(), expected_indices);
    }

    #[test]
    fn tube_instance_layout_is_stable_pod_size() {
        assert_eq!(std::mem::size_of::<TubeInstance>(), 112);
    }

    #[test]
    fn tube_frame_layout_is_16b_aligned() {
        assert_eq!(std::mem::size_of::<TubeFrame>(), 48);
        assert_eq!(std::mem::align_of::<TubeFrame>(), 4);
        // Three vec4 lanes -> byte size is a multiple of 16 for WGSL storage layout.
        assert_eq!(std::mem::size_of::<TubeFrame>() % 16, 0);
    }

    /// Walk a smoothly turning, out-of-plane curve with the CPU mirror of the RMF step and check
    /// the frame stays perpendicular to the tangent and never flips sign (no twist).
    #[test]
    fn rmf_frame_stays_perpendicular_and_continuous() {
        let p0 = Vec3::new(-1.0, 0.0, 0.0);
        let p1 = Vec3::new(-0.3, 1.2, 0.8);
        let p2 = Vec3::new(0.6, -0.9, 1.4);
        let p3 = Vec3::new(1.4, 0.3, -0.6);

        let m = 64u32;
        let mut x_i = bezier_point(p0, p1, p2, p3, 0.0);
        let mut t_i = bezier_tangent(p0, p1, p2, p3, 0.0).normalize();
        let mut r_i = seed_normal(t_i);

        assert!(r_i.dot(t_i).abs() < 1.0e-5, "seed not perpendicular");

        for i in 1..=m {
            let u = i as f32 / m as f32;
            let x_next = bezier_point(p0, p1, p2, p3, u);
            let t_next = bezier_tangent(p0, p1, p2, p3, u).normalize();
            let r_next = rmf_step(x_i, t_i, r_i, x_next, t_next);

            assert!(
                r_next.dot(t_next).abs() < 1.0e-4,
                "frame drifted off-perpendicular at u={u}: dot={}",
                r_next.dot(t_next)
            );
            assert!((r_next.length() - 1.0).abs() < 1.0e-4, "frame not unit length at u={u}");
            // Continuity: adjacent reference normals must not flip for a smoothly turning curve.
            assert!(
                r_i.dot(r_next) > 0.0,
                "frame flipped sign at u={u}: dot={}",
                r_i.dot(r_next)
            );

            x_i = x_next;
            t_i = t_next;
            r_i = r_next;
        }
    }
}
