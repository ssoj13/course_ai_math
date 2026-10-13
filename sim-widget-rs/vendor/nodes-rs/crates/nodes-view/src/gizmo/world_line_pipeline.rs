//! Colored line segments in world space (gizmo axes).

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::include_wgsl;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct WorldLineVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniforms {
    view_proj: [[f32; 4]; 4],
}

pub struct WorldLinePipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
}

impl WorldLinePipeline {
    /// Build the world-line pipeline for a specific color sample count.
    ///
    /// The caller picks `samples` to match the render-pass color attachment count.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, samples: u32) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nodes_view_world_line_bgl"),
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nodes_view_world_line_pl"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(include_wgsl!("world_line.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nodes_view_world_line"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    // wgpu 30: &[Option<VertexBufferLayout>]
                    array_stride: std::mem::size_of::<WorldLineVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x3,
                        },
                        wgpu::VertexAttribute {
                            offset: 12,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                })],
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
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: samples,
                ..wgpu::MultisampleState::default()
            },
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            bind_group_layout,
        }
    }

    pub fn create_bind_group(
        &self,
        device: &wgpu::Device,
        view_proj: Mat4,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let u = ViewUniforms {
            view_proj: view_proj.to_cols_array_2d(),
        };
        let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("nodes_view_world_line_uniform"),
            contents: bytemuck::bytes_of(&u),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nodes_view_world_line_bg"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buf.as_entire_binding(),
            }],
        });
        (buf, bg)
    }
}

pub fn draw_world_lines<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    pipeline: &'a WorldLinePipeline,
    bind_group: &'a wgpu::BindGroup,
    vertex_buf: Option<&'a wgpu::Buffer>,
    vert_count: usize,
) {
    if vert_count == 0 {
        return;
    }
    pass.set_pipeline(&pipeline.pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    if let Some(vb) = vertex_buf {
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.draw(0..vert_count as u32, 0..1);
    }
}

pub fn world_line_buffer(device: &wgpu::Device, verts: &[WorldLineVertex]) -> Option<wgpu::Buffer> {
    if verts.is_empty() {
        return None;
    }
    Some(
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("nodes_view_world_lines"),
            contents: bytemuck::cast_slice(verts),
            usage: wgpu::BufferUsages::VERTEX,
        }),
    )
}
