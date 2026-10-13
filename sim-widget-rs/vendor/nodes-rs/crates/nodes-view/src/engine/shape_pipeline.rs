//! Instanced SDF shape quads (`shape.wgsl`).
//!
//! Color and object-id are rendered via separate pipelines so the color pass can use MSAA while
//! the id target stays single-sampled for readback picking.

use bytemuck::{Pod, Zeroable};
use wgpu::include_wgsl;
use wgpu::util::DeviceExt;

use super::shape2d::{Shape2d, ShapeInstance};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    corner: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ShapeInstanceGpu {
    pub aabb_min: [f32; 2],
    pub aabb_max: [f32; 2],
    pub kind: u32,
    pub sides: u32,
    pub p0: [f32; 4],
    pub p1: [f32; 4],
    pub fill: [f32; 4],
    pub stroke: [f32; 4],
    /// Object-id written in the dedicated id pass (`0` means background / non-pickable).
    pub pick_id: u32,
    /// In-plane rotation (radians) about [`Self::pivot`], applied to the DRAWN quad in `shape.wgsl`
    /// while the SDF is evaluated in the shape's local frame. `0` = axis-aligned.
    pub rotation: f32,
    /// Rotation pivot in graph-UV (the owning node's center).
    pub pivot: [f32; 2],
}

const QUAD: [QuadVertex; 6] = [
    QuadVertex { corner: [0.0, 0.0] },
    QuadVertex { corner: [1.0, 0.0] },
    QuadVertex { corner: [1.0, 1.0] },
    QuadVertex { corner: [0.0, 0.0] },
    QuadVertex { corner: [1.0, 1.0] },
    QuadVertex { corner: [0.0, 1.0] },
];

pub struct ShapePipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    quad_buffer: wgpu::Buffer,
}

pub struct ShapeIdPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    quad_buffer: wgpu::Buffer,
}

impl ShapePipeline {
    /// Build the color shape pipeline for an MSAA sample count chosen by the caller.
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let bind_group_layout = create_bind_group_layout(device, "nodes_view_shape_bgl");
        let layout = create_pipeline_layout(device, &bind_group_layout, "nodes_view_shape_pl");
        let shader = device.create_shader_module(include_wgsl!("shape.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nodes_view_shape"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &shape_vertex_buffers().map(Some), // wgpu 30: &[Option<VBL>]
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
            // Reversed-Z: near maps to NDC 1, far to 0 (see `Camera::view_proj`). The pass clears
            // depth to 0.0 and keeps the fragment with the GREATER depth, so a shape with a larger
            // `depth_bias` (nearer the camera) wins — the mirror of the old `Less` + clear-1.0 setup.
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
        let quad_buffer = create_quad_buffer(device, "nodes_view_shape_quad");
        Self {
            pipeline,
            bind_group_layout,
            quad_buffer,
        }
    }

    pub fn encode<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        bind_group: &'a wgpu::BindGroup,
        instance_buffer: &'a wgpu::Buffer,
        instance_count: u32,
    ) {
        if instance_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, self.quad_buffer.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        pass.draw(0..6, 0..instance_count);
    }
}

impl ShapeIdPipeline {
    /// Build the single-sampled id pipeline used for GPU picking readback.
    pub fn new(device: &wgpu::Device, depth_format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = create_bind_group_layout(device, "nodes_view_shape_id_bgl");
        let layout = create_pipeline_layout(device, &bind_group_layout, "nodes_view_shape_id_pl");
        let shader = device.create_shader_module(include_wgsl!("shape.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nodes_view_shape_id"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &shape_vertex_buffers().map(Some), // wgpu 30: &[Option<VBL>]
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_id"),
                targets: &[Some(wgpu_pick::IdTarget::color_target_state())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                ..wgpu::MultisampleState::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let quad_buffer = create_quad_buffer(device, "nodes_view_shape_id_quad");
        Self {
            pipeline,
            bind_group_layout,
            quad_buffer,
        }
    }

    pub fn encode<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        bind_group: &'a wgpu::BindGroup,
        instance_buffer: &'a wgpu::Buffer,
        instance_count: u32,
    ) {
        if instance_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, self.quad_buffer.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        pass.draw(0..6, 0..instance_count);
    }
}

fn create_bind_group_layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
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
    })
}

fn create_pipeline_layout(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    label: &str,
) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    })
}

fn create_quad_buffer(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(&QUAD),
        usage: wgpu::BufferUsages::VERTEX,
    })
}

fn shape_vertex_buffers() -> [wgpu::VertexBufferLayout<'static>; 2] {
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
            array_stride: std::mem::size_of::<ShapeInstanceGpu>() as wgpu::BufferAddress,
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
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: 20,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 40,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 56,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 72,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::offset_of!(ShapeInstanceGpu, pick_id) as wgpu::BufferAddress,
                    shader_location: 9,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::offset_of!(ShapeInstanceGpu, pivot) as wgpu::BufferAddress,
                    shader_location: 10,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::offset_of!(ShapeInstanceGpu, rotation) as wgpu::BufferAddress,
                    shader_location: 11,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        },
    ]
}

pub fn instances_to_gpu(instances: &[ShapeInstance]) -> Vec<ShapeInstanceGpu> {
    instances.iter().map(instance_to_gpu).collect()
}

fn instance_to_gpu(inst: &ShapeInstance) -> ShapeInstanceGpu {
    let aabb = inst.shape.aabb_with_stroke(inst.style.stroke_width);
    let (kind, sides, p0, p1) = match &inst.shape {
        Shape2d::RoundedRect { min, max, radius } => (
            0u32,
            0u32,
            [min[0], min[1], max[0], max[1]],
            [
                radius.max(0.0),
                inst.style.stroke_width,
                inst.depth_bias,
                0.0,
            ],
        ),
        Shape2d::Circle { center, radius } => (
            1u32,
            0u32,
            [center[0], center[1], radius.max(0.0), 0.0],
            [0.0, inst.style.stroke_width, inst.depth_bias, 0.0],
        ),
        Shape2d::RegularPolygon {
            center,
            radius,
            sides,
            rotation,
        } => (
            2u32,
            *sides as u32,
            [center[0], center[1], radius.max(0.0), *rotation],
            [0.0, inst.style.stroke_width, inst.depth_bias, 0.0],
        ),
        Shape2d::Segment { a, b, half_width } => (
            3u32,
            0u32,
            [a[0], a[1], b[0], b[1]],
            [half_width.max(0.5), 0.0, inst.depth_bias, 0.0],
        ),
    };
    // Pick-only marker in the free `p1.w` slot: these hit rects are stamped into the id buffer but
    // never drawn in color (see `shape.wgsl` `p1.w` branches). Their VISUALS (field value box, title
    // chrome flags) are drawn by the editor's widget layer, so GPU pick can classify them without a
    // second, competing draw. Covers inline fields and the title chrome flags.
    use super::shape2d::PickTarget;
    let mut p1 = p1;
    if matches!(
        inst.pick,
        PickTarget::NodeField { .. }
            | PickTarget::ViewModeToggle(_)
            | PickTarget::BypassFlag(_)
            | PickTarget::DisplayFlag(_)
    ) {
        p1[3] = 1.0;
    }
    ShapeInstanceGpu {
        aabb_min: [aabb[0], aabb[1]],
        aabb_max: [aabb[2], aabb[3]],
        kind,
        sides,
        p0,
        p1,
        fill: inst.style.fill,
        stroke: inst.style.stroke,
        pick_id: inst.pick_id,
        rotation: inst.rotation,
        pivot: inst.pivot,
    }
}
