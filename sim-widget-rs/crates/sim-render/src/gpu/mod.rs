//! GPU encoder: device/queue and submission ownership always remain with the host.
use crate::{
    RenderError, Result,
    error::invalid,
    material::{Material, MaterialGpu},
    shader,
};
use wgpu::util::DeviceExt;
/// Mesh vertex ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 3],
}
/// CSS ribbon endpoints and signed corner.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct StrokeVertex {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub corner: [f32; 2],
}
/// Camera/light data is specific to a view, never global mutable state.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FrameGpu {
    pub view_projection: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub orthographic_direction: [f32; 4],
    pub light_direction: [f32; 4],
    pub light_irradiance: [f32; 4],
    pub viewport: [f32; 4],
}
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ObjectGpu {
    pub world: [[f32; 4]; 4],
    pub normal_matrix: [[f32; 4]; 4],
    pub edge_color: [f32; 4],
}
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EnvironmentGpu {
    pub irradiance: [f32; 4],
    pub specular_radiance: [f32; 4],
    pub coat_radiance: [f32; 4],
    pub transmission_radiance: [f32; 4],
}
impl Default for ObjectGpu {
    fn default() -> Self {
        Self {
            world: glam::Mat4::IDENTITY.to_cols_array_2d(),
            normal_matrix: glam::Mat4::IDENTITY.to_cols_array_2d(),
            edge_color: [0.9, 0.94, 1.0, 1.0],
        }
    }
}
impl Default for EnvironmentGpu {
    fn default() -> Self {
        Self {
            irradiance: [0.0; 4],
            specular_radiance: [0.0; 4],
            coat_radiance: [0.0; 4],
            transmission_radiance: [0.0; 4],
        }
    }
}
/// Actual buffers and groups for one draw; removal drops ownership immediately.
pub struct GpuDraw {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    strokes: Option<wgpu::Buffer>,
    index_count: u32,
    stroke_count: u32,
    groups: [wgpu::BindGroup; 4],
    pub allocated_bytes: u64,
    overlay: bool,
}
impl GpuDraw { pub fn as_overlay(mut self)->Self{self.overlay=true;self} }
/// Pipelines shared by all views. Renderer never creates an adapter or submits commands.
pub struct RasterRenderer {
    device: wgpu::Device,
    layouts: [wgpu::BindGroupLayout; 4],
    fill: wgpu::RenderPipeline,
    stroke: wgpu::RenderPipeline,
    overlay_fill: wgpu::RenderPipeline,
    overlay_stroke: wgpu::RenderPipeline,
    pub format: wgpu::TextureFormat,
    pub samples: u32,
}
impl RasterRenderer {
    /// Validate public BSDF composition and create pipelines in a captured error scope.
    pub async fn create(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> Result<Self> {
        Self::create_registered(
            device,
            format,
            samples,
            &shader::material_source(),
            "widget_vs",
            "widget_fs",
        )
        .await
    }
    /// Registered material source is published only after actual GPU module/pipeline validation.
    /// It must implement the explicit four-group ABI and both standard ribbon entrypoints.
    pub async fn create_registered(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        samples: u32,
        source: &str,
        vertex: &str,
        fragment: &str,
    ) -> Result<Self> {
        if device.limits().max_bind_groups < 4 {
            return Err(invalid("device.maxBindGroups", "four groups required"));
        }
        if ![1, 2, 4, 8, 16].contains(&samples) {
            return Err(invalid("samples", "unsupported sample count"));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let contract = shader::validate(source, vertex, fragment)?;
            if contract
                .bindings
                .iter()
                .any(|(group, binding)| *group > 3 || *binding != 0)
            {
                return Err(invalid(
                    "shader.bindings",
                    "registered material uses foreign resources",
                ));
            }
        }
        let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("widget public BSDF"),
            source: wgpu::ShaderSource::Wgsl(source.to_owned().into()),
        });
        let sizes = [
            std::mem::size_of::<FrameGpu>(),
            std::mem::size_of::<ObjectGpu>(),
            std::mem::size_of::<MaterialGpu>(),
            std::mem::size_of::<EnvironmentGpu>(),
        ];
        let layouts = std::array::from_fn(|i| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("widget uniform group"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(sizes[i] as u64),
                    },
                    count: None,
                }],
            })
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("widget four groups"),
            bind_group_layouts: &layouts.iter().map(Some).collect::<Vec<_>>(),
            immediate_size: 0,
        });
        let vertex_attrs = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3];
        let stroke_attrs = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2];
        let build = |stroke: bool, overlay: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if stroke {
                    "widget CSS ribbons"
                } else {
                    "widget BSDF triangles"
                }),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(if stroke { "widget_stroke_vs" } else { vertex }),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: if stroke {
                            std::mem::size_of::<StrokeVertex>()
                        } else {
                            std::mem::size_of::<Vertex>()
                        } as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: if stroke { &stroke_attrs } else { &vertex_attrs },
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if stroke { "widget_stroke_fs" } else { fragment }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(!stroke && !overlay),
                    depth_compare: Some(if overlay{wgpu::CompareFunction::Always}else{wgpu::CompareFunction::GreaterEqual}),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let fill = build(false,false);
        let stroke = build(true,false);
        let overlay_fill=build(false,true);
        let overlay_stroke=build(true,true);
        let validation_error = scope.pop().await;
        let internal_error = internal.pop().await;
        let oom_error = oom.pop().await;
        if let Some(e) = validation_error.or(internal_error).or(oom_error) {
            return Err(RenderError::Gpu(e.to_string()));
        }
        Ok(Self {
            device: device.clone(),
            layouts,
            fill,
            stroke,
            overlay_fill,
            overlay_stroke,
            format,
            samples,
        })
    }
    /// Upload validated indexed triangles and genuine deterministic triangle edges.
    pub fn prepare(
        &self,
        vertices: &[Vertex],
        indices: &[u32],
        frame: &FrameGpu,
        object: &ObjectGpu,
        material: &Material,
        environment: &EnvironmentGpu,
        wireframe: bool,
    ) -> Result<GpuDraw> {
        if vertices.is_empty() || indices.is_empty() || !indices.len().is_multiple_of(3) {
            return Err(invalid("mesh", "nonempty indexed triangle list required"));
        }
        if vertices
            .iter()
            .flat_map(|v| {
                v.position
                    .iter()
                    .chain(v.normal.iter())
                    .chain(v.tangent.iter())
            })
            .any(|n| !n.is_finite())
        {
            return Err(invalid("mesh", "nonfinite vertex"));
        }
        if indices.iter().any(|i| *i as usize >= vertices.len()) {
            return Err(invalid("mesh.indices", "index out of range"));
        }
        validate_draw(vertices, frame, object, environment)?;
        let maximum_edges = indices
            .len()
            .checked_mul(6)
            .ok_or_else(|| RenderError::Budget("edge count overflow".into()))?;
        let maximum_bytes = vertices
            .len()
            .checked_mul(std::mem::size_of::<Vertex>())
            .and_then(|n| n.checked_add(indices.len().checked_mul(4)?))
            .and_then(|n| {
                n.checked_add(if wireframe {
                    maximum_edges.checked_mul(std::mem::size_of::<StrokeVertex>())?
                } else {
                    0
                })
            })
            .ok_or_else(|| RenderError::Budget("geometry byte overflow".into()))?;
        if maximum_bytes > 64 * 1024 * 1024
            || maximum_bytes as u64 > self.device.limits().max_buffer_size
        {
            return Err(RenderError::Budget(
                "geometry exceeds budget before allocation".into(),
            ));
        }
        let mut edges = std::collections::BTreeSet::new();
        if wireframe {
            for t in indices.chunks_exact(3) {
                for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    edges.insert((a.min(b), a.max(b)));
                }
            }
        }
        let mut strokes = Vec::with_capacity(edges.len() * 6);
        for (a, b) in edges {
            for corner in [
                [0.0, -1.0],
                [1.0, -1.0],
                [1.0, 1.0],
                [0.0, -1.0],
                [1.0, 1.0],
                [0.0, 1.0],
            ] {
                strokes.push(StrokeVertex {
                    a: vertices[a as usize].position,
                    b: vertices[b as usize].position,
                    corner,
                });
            }
        }
        self.upload_draw(
            vertices,
            indices,
            &strokes,
            frame,
            object,
            material,
            environment,
        )
    }
    /// Portable standalone segments/arrows share the material/frame ABI and draw only ribbons.
    pub fn prepare_segments(
        &self,
        segments: &[crate::geometry::SegmentGeometry],
        frame: &FrameGpu,
        object: &ObjectGpu,
        material: &Material,
        environment: &EnvironmentGpu,
    ) -> Result<GpuDraw> {
        validate_draw(&[], frame, object, environment)?;
        let count = segments
            .len()
            .checked_mul(6)
            .ok_or_else(|| RenderError::Budget("segment count overflow".into()))?;
        if count > 2_000_000
            || count
                .checked_mul(std::mem::size_of::<StrokeVertex>())
                .is_none_or(|n| n > 64 * 1024 * 1024)
        {
            return Err(RenderError::Budget("segment budget exceeded".into()));
        }
        let mut strokes = Vec::with_capacity(count);
        for segment in segments {
            if !segment.a.is_finite() || !segment.b.is_finite() {
                return Err(invalid("segment", "nonfinite endpoint"));
            }
            let a = segment
                .a
                .to_array()
                .map(|n| crate::material::narrow(n, "segment.a"))
                .into_iter()
                .collect::<Result<Vec<_>>>()?;
            let b = segment
                .b
                .to_array()
                .map(|n| crate::material::narrow(n, "segment.b"))
                .into_iter()
                .collect::<Result<Vec<_>>>()?;
            for corner in [
                [0.0, -1.0],
                [1.0, -1.0],
                [1.0, 1.0],
                [0.0, -1.0],
                [1.0, 1.0],
                [0.0, 1.0],
            ] {
                strokes.push(StrokeVertex {
                    a: [a[0], a[1], a[2]],
                    b: [b[0], b[1], b[2]],
                    corner,
                });
            }
        }
        self.upload_draw(&[], &[], &strokes, frame, object, material, environment)
    }
    fn upload_draw(
        &self,
        vertices: &[Vertex],
        indices: &[u32],
        strokes: &[StrokeVertex],
        frame: &FrameGpu,
        object: &ObjectGpu,
        material: &Material,
        environment: &EnvironmentGpu,
    ) -> Result<GpuDraw> {
        let packed = MaterialGpu::encode(material)?;
        let index_count = u32::try_from(indices.len())
            .map_err(|_| RenderError::Budget("index count overflow".into()))?;
        let vertex_bytes = bytemuck::cast_slice(vertices);
        let index_bytes = bytemuck::cast_slice(indices);
        let stroke_bytes = bytemuck::cast_slice(strokes);
        let allocated_bytes = (vertex_bytes.len()
            + index_bytes.len()
            + stroke_bytes.len()
            + std::mem::size_of::<FrameGpu>()
            + std::mem::size_of::<ObjectGpu>()
            + std::mem::size_of::<MaterialGpu>()
            + std::mem::size_of::<EnvironmentGpu>()) as u64;
        if allocated_bytes > 64 * 1024 * 1024
            || allocated_bytes > self.device.limits().max_buffer_size
        {
            return Err(RenderError::Budget(
                "draw exceeds64MiB/device budget".into(),
            ));
        }
        let buffer = |label, bytes, usage| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage,
                })
        };
        let groups = std::array::from_fn(|i| {
            let data = [
                bytemuck::bytes_of(frame),
                bytemuck::bytes_of(object),
                bytemuck::bytes_of(&packed),
                bytemuck::bytes_of(environment),
            ][i];
            let uniform = buffer(
                "widget uniform",
                data,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("widget draw group"),
                layout: &self.layouts[i],
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }],
            })
        });
        Ok(GpuDraw {
            vertices: buffer(
                "widget vertices",
                vertex_bytes,
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            ),
            indices: buffer(
                "widget indices",
                index_bytes,
                wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            ),
            strokes: (!strokes.is_empty())
                .then(|| buffer("widget edges", stroke_bytes, wgpu::BufferUsages::VERTEX)),
            index_count,
            stroke_count: strokes.len() as u32,
            groups,
            allocated_bytes,
            overlay:false,
        })
    }
    /// Encode a view to host-owned attachments and encoder; caller submits once.
    pub fn encode_view(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        draws: &[&GpuDraw],
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        resolve: Option<&wgpu::TextureView>,
        clear: wgpu::Color,
        viewport: Option<[u32; 4]>,
    ) -> Result<()> {
        if let Some([x, y, w, h]) = viewport {
            if w == 0 || h == 0 || x.checked_add(w).is_none() || y.checked_add(h).is_none() {
                return Err(invalid("viewport", "invalid rectangle"));
            }
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("widget view"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: resolve,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
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
        if let Some([x, y, w, h]) = viewport {
            pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
            pass.set_scissor_rect(x, y, w, h);
        }
        for draw in draws {
            for (i, group) in draw.groups.iter().enumerate() {
                pass.set_bind_group(i as u32, group, &[]);
            }
            if draw.index_count > 0 {
                pass.set_pipeline(if draw.overlay{&self.overlay_fill}else{&self.fill});
                pass.set_vertex_buffer(0, draw.vertices.slice(..));
                pass.set_index_buffer(draw.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..draw.index_count, 0, 0..1);
            }
            if let Some(strokes) = &draw.strokes {
                pass.set_pipeline(if draw.overlay{&self.overlay_stroke}else{&self.stroke});
                pass.set_vertex_buffer(0, strokes.slice(..));
                pass.draw(0..draw.stroke_count, 0..1);
            }
        }
        Ok(())
    }
}

fn validate_draw(
    vertices: &[Vertex],
    frame: &FrameGpu,
    object: &ObjectGpu,
    environment: &EnvironmentGpu,
) -> Result<()> {
    let matrix_values = frame
        .view_projection
        .iter()
        .flatten()
        .chain(object.world.iter().flatten())
        .chain(object.normal_matrix.iter().flatten());
    if matrix_values.into_iter().any(|v| !v.is_finite())
        || frame
            .eye
            .iter()
            .chain(frame.orthographic_direction.iter())
            .chain(frame.light_direction.iter())
            .chain(frame.light_irradiance.iter())
            .chain(frame.viewport.iter())
            .chain(environment.irradiance.iter())
            .chain(environment.specular_radiance.iter())
            .chain(environment.coat_radiance.iter())
            .chain(environment.transmission_radiance.iter())
            .any(|v| !v.is_finite())
    {
        return Err(invalid("draw.uniforms", "nonfinite uniform"));
    }
    if frame.viewport[0] <= 0.0
        || frame.viewport[1] <= 0.0
        || frame.viewport[2] <= 0.0
        || glam::Vec3::from_slice(&frame.light_direction).length_squared() < 1e-12
    {
        return Err(invalid(
            "draw.frame",
            "invalid dimensions, width or light direction",
        ));
    }
    let world = glam::Mat4::from_cols_array_2d(&object.world);
    if world.determinant().abs() < 1e-12 {
        return Err(invalid("draw.object", "singular transform"));
    }
    let expected = world.inverse().transpose();
    let supplied = glam::Mat4::from_cols_array_2d(&object.normal_matrix);
    for column in 0..3 {
        if (expected.col(column).truncate() - supplied.col(column).truncate())
            .abs()
            .max_element()
            > 1e-4
        {
            return Err(invalid(
                "draw.normal_matrix",
                "normal transform must be inverse transpose",
            ));
        }
    }
    if vertices.iter().any(|v| {
        glam::Vec3::from_array(v.normal).length_squared() < 1e-12
            || glam::Vec3::from_array(v.tangent).length_squared() < 1e-12
    }) {
        return Err(invalid("mesh.frame", "zero normal/tangent"));
    }
    if frame.orthographic_direction[3] != 0.0
        && glam::Vec3::from_slice(&frame.orthographic_direction).length_squared() < 1e-12
    {
        return Err(invalid("draw.camera", "zero orthographic direction"));
    }
    Ok(())
}
