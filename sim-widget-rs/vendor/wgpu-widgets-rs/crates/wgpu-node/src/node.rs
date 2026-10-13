//! `wgpu-node` — THE procedural 3D node card generator + batched instanced renderer.
//!
//! The caller passes the REAL node parameters ([`NodeMeshParams`]: width/height/corner radius/
//! depth/band heights in node units) and gets a mesh built at that exact size, centered at the
//! origin in its own local frame. Because the radius is in the same units as width/height, corners
//! are TRUE quarter circles at any aspect ratio (no stretch). The caller then places each node with
//! a RIGID transform (rotation onto the graph plane + translation, NO scale) — see [`NodeInstance`].
//!
//! ## Speed (hundreds of nodes)
//! [`NodeRenderer`] caches one mesh per distinct parameter set and instances all nodes that share
//! it: node COLORS ride the instance (body/title/footer + `pick_id`), so nodes of the same size but
//! different category share ONE mesh and draw in a single instanced call per size. Typical graphs
//! have few distinct sizes → a couple of draw calls for the whole graph.
//!
//! Materials come from the shared [`ui_theme::Theme`] (SSOT) via [`node_materials`], so the 3D
//! nodes match the SDF backend and a theme switch restyles everything.

#![warn(missing_docs)]

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3};
use wgpu::util::DeviceExt;

/// RGBA color, 0..1 per channel (matches [`ui_theme::Rgba`]).
pub type Rgba = [f32; 4];

/// Part tag per vertex — selects which instance color (`body`/`title`/`footer`) shades it.
pub mod part {
    /// Node body / card.
    pub const BODY: u32 = 0;
    /// Title bar (top).
    pub const TITLE: u32 = 1;
    /// Footer bar (bottom).
    pub const FOOTER: u32 = 2;
}

/// Whether a node card is a single flat layer of polygons or a real extruded 3D box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NodeShape {
    /// One layer of polygons on the front face (no back / no side walls). Flat card; cheapest.
    Flat,
    /// Real extruded box: front + back + smooth side walls (all normals outward).
    #[default]
    Extruded,
}

/// Full procedural node-card parameters, in REAL node units (same units as the graph layout).
///
/// The mesh is built at this exact size, centered at the origin: `x ∈ [-width/2, width/2]`,
/// `y ∈ [-height/2, height/2]`, and (Extruded) `z ∈ [-depth/2, depth/2]` with `+y` up (title on
/// top) and `+z` toward the camera. Corner radius is a true circle because it shares units with
/// width/height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeMeshParams {
    /// Flat single-layer card, or a real extruded 3D box.
    pub shape: NodeShape,
    /// Node width in node units.
    pub width: f32,
    /// Node height in node units.
    pub height: f32,
    /// Corner radius in node units (true quarter-circle; clamped to `min(width,height)/2`).
    pub corner_radius: f32,
    /// Silhouette subdivisions per rounded corner (smoothness).
    pub corner_segments: u32,
    /// Card thickness in node units for [`NodeShape::Extruded`] (e.g. `1.0`; `0` ≈ flat).
    pub depth: f32,
    /// Title band height in node units (0 = no distinct title band).
    pub title_h: f32,
    /// Footer band height in node units (0 = no distinct footer band).
    pub footer_h: f32,
}

impl Default for NodeMeshParams {
    fn default() -> Self {
        Self {
            shape: NodeShape::Extruded,
            width: 320.0,
            height: 160.0,
            corner_radius: 12.0,
            corner_segments: 12,
            depth: 1.0,
            title_h: 28.0,
            footer_h: 0.0,
        }
    }
}

/// Global node-card STYLE (shared by every node): shape, radius, smoothness, thickness. The
/// per-node width/height/band-heights are supplied at build time; combine via [`Self::params`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeStyle {
    /// Flat or extruded.
    pub shape: NodeShape,
    /// Corner radius in node units.
    pub corner_radius: f32,
    /// Corner smoothness (arc subdivisions).
    pub corner_segments: u32,
    /// Extruded thickness in node units.
    pub depth: f32,
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self {
            shape: NodeShape::Extruded,
            corner_radius: 12.0,
            corner_segments: 12,
            depth: 1.0,
        }
    }
}

impl NodeStyle {
    /// Combine this global style with per-node dimensions into full [`NodeMeshParams`].
    pub fn params(&self, width: f32, height: f32, title_h: f32, footer_h: f32) -> NodeMeshParams {
        NodeMeshParams {
            shape: self.shape,
            width,
            height,
            corner_radius: self.corner_radius,
            corner_segments: self.corner_segments,
            depth: self.depth,
            title_h,
            footer_h,
        }
    }
}

/// One mesh vertex: position + normal + [`part`] tag.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct Vertex {
    /// Position in the node's local frame (node units, centered at origin).
    pub position: [f32; 3],
    /// Outward normal (for lighting).
    pub normal: [f32; 3],
    /// Part tag ([`part::BODY`] / `TITLE` / `FOOTER`).
    pub part: u32,
}

/// CPU-side generated node mesh (cached + instanced by [`NodeRenderer`]).
#[derive(Clone, Debug, Default)]
pub struct NodeMesh {
    /// Vertex list.
    pub vertices: Vec<Vertex>,
    /// Triangle index list.
    pub indices: Vec<u32>,
}

/// Per-node draw instance: RIGID transform (rotation + translation, NO scale — the mesh is already
/// at real size) + per-part colors + object-id for GPU picking.
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
pub struct NodeInstance {
    /// Local → world transform. Columns are the node's world axes (unit right/up/front) + origin.
    pub model: [[f32; 4]; 4],
    /// Body material color.
    pub body: Rgba,
    /// Title-band material color.
    pub title: Rgba,
    /// Footer-band material color.
    pub footer: Rgba,
    /// Object id for the pick buffer (`0` = non-pickable).
    pub pick_id: u32,
    /// Pad to 16-byte alignment.
    pub _pad: [u32; 3],
}

/// Node material colors resolved from the shared theme (title = category accent, so category
/// identity reads on the title band; body/footer are theme surfaces). ONE place for node color
/// policy — matches the SDF backend.
pub fn node_materials(theme: &ui_theme::Theme, category_accent: Rgba) -> (Rgba, Rgba, Rgba) {
    (theme.node_body, category_accent, theme.bg_sunken)
}

// ---------------------------------------------------------------------------
// Mesh generation (real units, centered at origin)
// ---------------------------------------------------------------------------

/// Rounded-rect half-width at height `y` for half-extents `hw,hh` and corner radius `r`.
fn half_width_at(y: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let ay = y.abs();
    let flat_top = hh - r;
    if ay <= flat_top {
        hw
    } else {
        let dy = ay - flat_top;
        if dy >= r {
            (hw - r).max(0.0)
        } else {
            (hw - r) + (r * r - dy * dy).sqrt()
        }
    }
}

/// Which colored band a front-face point at height `y` belongs to.
fn band_at(y: f32, title_y: Option<f32>, footer_y: Option<f32>) -> u32 {
    if let Some(t) = title_y
        && y >= t {
            return part::TITLE;
        }
    if let Some(f) = footer_y
        && y <= f {
            return part::FOOTER;
        }
    part::BODY
}

/// Sorted, de-duplicated scanline y-levels: box edges, band seams, and corner-arc samples (so the
/// silhouette curves). Seams are exact levels → no triangle straddles a band boundary.
fn scanlines(hh: f32, r: f32, seg: u32, title_y: Option<f32>, footer_y: Option<f32>) -> Vec<f32> {
    let mut levels = vec![-hh, hh];
    if let Some(t) = title_y {
        levels.push(t);
    }
    if let Some(f) = footer_y {
        levels.push(f);
    }
    for i in 1..seg.max(1) {
        let phi = (i as f32 / seg.max(1) as f32) * std::f32::consts::FRAC_PI_2;
        let yy = (hh - r) + r * phi.sin();
        levels.push(yy);
        levels.push(-yy);
    }
    levels.sort_by(|a, b| a.total_cmp(b));
    levels.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    levels
}

/// Append the banded front/back face at `z` with flat normal `(0,0,nz)`.
fn push_face(
    mesh: &mut NodeMesh,
    levels: &[f32],
    hw: f32,
    hh: f32,
    r: f32,
    z: f32,
    nz: f32,
    title_y: Option<f32>,
    footer_y: Option<f32>,
) {
    let normal = [0.0, 0.0, nz];
    for w in levels.windows(2) {
        let (y0, y1) = (w[0], w[1]);
        if (y1 - y0).abs() < 1e-5 {
            continue;
        }
        let hx0 = half_width_at(y0, hw, hh, r);
        let hx1 = half_width_at(y1, hw, hh, r);
        if hx0 <= 1e-5 && hx1 <= 1e-5 {
            continue;
        }
        let p = if nz > 0.0 {
            band_at(0.5 * (y0 + y1), title_y, footer_y)
        } else {
            part::BODY
        };
        let base = mesh.vertices.len() as u32;
        for pos in [[-hx0, y0, z], [hx0, y0, z], [hx1, y1, z], [-hx1, y1, z]] {
            mesh.vertices.push(Vertex { position: pos, normal, part: p });
        }
        if nz > 0.0 {
            mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        } else {
            mesh.indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
}

/// Closed rounded-rect outline (CCW) sampled at the same `levels` as the faces.
fn outline(levels: &[f32], hw: f32, hh: f32, r: f32) -> Vec<Vec2> {
    let mut pts = Vec::with_capacity(levels.len() * 2);
    for &y in levels {
        pts.push(Vec2::new(half_width_at(y, hw, hh, r), y));
    }
    for &y in levels.iter().rev() {
        pts.push(Vec2::new(-half_width_at(y, hw, hh, r), y));
    }
    pts.dedup_by(|a, b| (a.x - b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5);
    pts
}

/// Append extruded side walls: a CONTINUOUS ring with per-vertex smooth normals (central-difference
/// tangent), so rounded corners shade smoothly (no facets). All triangles wind outward.
fn push_sides(mesh: &mut NodeMesh, outline: &[Vec2], z_front: f32, z_back: f32) {
    let n = outline.len();
    if n < 3 {
        return;
    }
    let mut normals = Vec::with_capacity(n);
    for i in 0..n {
        let prev = outline[(i + n - 1) % n];
        let next = outline[(i + 1) % n];
        let tangent = (next - prev).normalize_or_zero();
        normals.push(Vec2::new(tangent.y, -tangent.x).normalize_or_zero());
    }
    let base = mesh.vertices.len() as u32;
    for (p, nr) in outline.iter().zip(normals.iter()) {
        mesh.vertices.push(Vertex { position: [p.x, p.y, z_front], normal: [nr.x, nr.y, 0.0], part: part::BODY });
    }
    for (p, nr) in outline.iter().zip(normals.iter()) {
        mesh.vertices.push(Vertex { position: [p.x, p.y, z_back], normal: [nr.x, nr.y, 0.0], part: part::BODY });
    }
    let n = n as u32;
    for i in 0..n {
        let i1 = (i + 1) % n;
        let (f0, f1, b0, b1) = (base + i, base + i1, base + n + i, base + n + i1);
        mesh.indices.extend_from_slice(&[f0, b1, f1, f0, b0, b1]);
    }
}

/// Build the procedural node mesh at REAL size, centered at the origin (see [`NodeMeshParams`]).
pub fn build_node_mesh(p: &NodeMeshParams) -> NodeMesh {
    let mut mesh = NodeMesh::default();
    let hw = (p.width * 0.5).max(1e-3);
    let hh = (p.height * 0.5).max(1e-3);
    let r = p.corner_radius.clamp(0.0, hw.min(hh));
    let seg = p.corner_segments.max(1);
    let title_y = (p.title_h > 1e-3).then(|| (hh - p.title_h).clamp(-hh, hh));
    let footer_y = (p.footer_h > 1e-3).then(|| (-hh + p.footer_h).clamp(-hh, hh));
    let levels = scanlines(hh, r, seg, title_y, footer_y);

    match p.shape {
        NodeShape::Flat => {
            push_face(&mut mesh, &levels, hw, hh, r, 0.0, 1.0, title_y, footer_y);
        }
        NodeShape::Extruded => {
            let zf = (p.depth * 0.5).max(1e-3);
            let zb = -zf;
            push_face(&mut mesh, &levels, hw, hh, r, zf, 1.0, title_y, footer_y);
            push_face(&mut mesh, &levels, hw, hh, r, zb, -1.0, title_y, footer_y);
            let ol = outline(&levels, hw, hh, r);
            push_sides(&mut mesh, &ol, zf, zb);
        }
    }
    mesh
}

// ---------------------------------------------------------------------------
// Batched instanced renderer (mesh cache keyed by parameters)
// ---------------------------------------------------------------------------

/// Default light direction (world), upper-front.
const DEFAULT_LIGHT_DIR: [f32; 4] = [0.35, 0.6, 0.7, 0.0];

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
}

/// Quantized parameter key so nodes of the same size/shape share one cached mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct MeshKey {
    shape: u8,
    w: i32,
    h: i32,
    r: i32,
    seg: u32,
    depth: i32,
    title: i32,
    footer: i32,
}

impl MeshKey {
    fn of(p: &NodeMeshParams) -> Self {
        let q = |x: f32| (x * 4.0).round() as i32; // 0.25-unit resolution
        Self {
            shape: match p.shape {
                NodeShape::Flat => 0,
                NodeShape::Extruded => 1,
            },
            w: q(p.width),
            h: q(p.height),
            r: q(p.corner_radius),
            seg: p.corner_segments.max(1),
            depth: q(p.depth),
            title: q(p.title_h),
            footer: q(p.footer_h),
        }
    }
}

/// One cached mesh + its instances for the current frame.
struct Batch {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    instances: Vec<NodeInstance>,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    instance_count: u32,
    used_this_frame: bool,
}

/// Renderer configuration.
#[derive(Clone, Copy, Debug)]
pub struct NodeRendererDesc {
    /// Color target format.
    pub color_format: wgpu::TextureFormat,
    /// Optional depth attachment format.
    pub depth_format: Option<wgpu::TextureFormat>,
    /// MSAA sample count.
    pub sample_count: u32,
    /// Reversed-Z depth compare (`GreaterEqual`) when `true` (matches nodes-view).
    pub reversed_z: bool,
}

impl Default for NodeRendererDesc {
    fn default() -> Self {
        Self {
            color_format: wgpu::TextureFormat::Bgra8Unorm,
            depth_format: Some(wgpu::TextureFormat::Depth32Float),
            sample_count: 1,
            reversed_z: true,
        }
    }
}

/// Batched instanced node renderer with a per-parameter mesh cache.
///
/// Per frame: [`Self::begin_frame`], then [`Self::push`] each node (mesh built + cached lazily),
/// then [`Self::end_frame`] to upload, then [`Self::set_camera_with_light`] + [`Self::draw`].
pub struct NodeRenderer {
    device: wgpu::Device,
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    camera_buffer: wgpu::Buffer,
    batches: HashMap<MeshKey, Batch>,
}

impl NodeRenderer {
    /// Create the renderer + pipeline.
    pub fn new(device: &wgpu::Device, desc: NodeRendererDesc) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wgpu_node.shader"),
            source: wgpu::ShaderSource::Wgsl(NODE_WGSL.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("wgpu_node.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera = CameraUniform {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            light_dir: DEFAULT_LIGHT_DIR,
        };
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("wgpu_node.camera"),
            contents: bytemuck::bytes_of(&camera),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("wgpu_node.bg"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: camera_buffer.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wgpu_node.pl"),
            bind_group_layouts: &[Some(&bgl)],
            ..Default::default()
        });
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 24, shader_location: 2, format: wgpu::VertexFormat::Uint32 },
            ],
        };
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<NodeInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 16, shader_location: 4, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 32, shader_location: 5, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 48, shader_location: 6, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 64, shader_location: 7, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 80, shader_location: 8, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 96, shader_location: 9, format: wgpu::VertexFormat::Float32x4 },
            ],
        };
        let depth_stencil = desc.depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: Some(true),
            depth_compare: Some(if desc.reversed_z {
                wgpu::CompareFunction::GreaterEqual
            } else {
                wgpu::CompareFunction::LessEqual
            }),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wgpu_node.pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[Some(vertex_layout), Some(instance_layout)],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: desc.color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil,
            multisample: wgpu::MultisampleState { count: desc.sample_count.max(1), ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        Self {
            device: device.clone(),
            pipeline,
            bgl,
            bind_group,
            camera_buffer,
            batches: HashMap::new(),
        }
    }

    /// Start a frame: clear per-batch instance lists (cached meshes are retained).
    pub fn begin_frame(&mut self) {
        for b in self.batches.values_mut() {
            b.instances.clear();
            b.used_this_frame = false;
        }
    }

    /// Queue one node instance; builds + caches the mesh for `params` on first use.
    pub fn push(&mut self, params: &NodeMeshParams, instance: NodeInstance) {
        let key = MeshKey::of(params);
        let device = &self.device;
        let batch = self.batches.entry(key).or_insert_with(|| {
            let mesh = build_node_mesh(params);
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("wgpu_node.vertices"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("wgpu_node.indices"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            let instance_capacity = 64usize;
            let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("wgpu_node.instances"),
                size: (instance_capacity * std::mem::size_of::<NodeInstance>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            Batch {
                vertex_buffer,
                index_buffer,
                index_count: mesh.indices.len() as u32,
                instances: Vec::new(),
                instance_buffer,
                instance_capacity,
                instance_count: 0,
                used_this_frame: false,
            }
        });
        batch.instances.push(instance);
        batch.used_this_frame = true;
    }

    /// Upload all queued instances; grows per-batch buffers as needed. Unused cached meshes are
    /// kept (bounded by the number of distinct sizes) for reuse next frame.
    pub fn end_frame(&mut self, queue: &wgpu::Queue) {
        for b in self.batches.values_mut() {
            b.instance_count = b.instances.len() as u32;
            if b.instances.is_empty() {
                continue;
            }
            if b.instances.len() > b.instance_capacity {
                b.instance_capacity = b.instances.len().next_power_of_two();
                b.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("wgpu_node.instances"),
                    size: (b.instance_capacity * std::mem::size_of::<NodeInstance>()) as wgpu::BufferAddress,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            queue.write_buffer(&b.instance_buffer, 0, bytemuck::cast_slice(&b.instances));
        }
    }

    /// Update the camera view-projection (default light).
    pub fn set_camera(&self, queue: &wgpu::Queue, view_proj: Mat4) {
        self.set_camera_with_light(
            queue,
            view_proj,
            Vec3::new(DEFAULT_LIGHT_DIR[0], DEFAULT_LIGHT_DIR[1], DEFAULT_LIGHT_DIR[2]),
        );
    }

    /// Update the camera view-projection and a world-space directional light.
    pub fn set_camera_with_light(&self, queue: &wgpu::Queue, view_proj: Mat4, light_dir: Vec3) {
        let l = light_dir.normalize_or_zero();
        let camera = CameraUniform {
            view_proj: view_proj.to_cols_array_2d(),
            light_dir: [l.x, l.y, l.z, 0.0],
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera));
    }

    /// Draw all batches (one instanced call per distinct mesh).
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        for b in self.batches.values() {
            if b.instance_count == 0 || b.index_count == 0 {
                continue;
            }
            pass.set_vertex_buffer(0, b.vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, b.instance_buffer.slice(..));
            pass.set_index_buffer(b.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..b.index_count, 0, 0..b.instance_count);
        }
    }

    /// Access the bind-group layout.
    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bgl
    }
}

const NODE_WGSL: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
};
@group(0) @binding(0) var<uniform> cam: Camera;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) part: u32,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) body: vec4<f32>,
    @location(8) title: vec4<f32>,
    @location(9) footer: vec4<f32>,
) -> VsOut {
    let model = mat4x4<f32>(m0, m1, m2, m3);
    let world = model * vec4<f32>(position, 1.0);
    var o: VsOut;
    o.clip = cam.view_proj * world;
    // Rigid transform (unit columns) → normals transform directly.
    o.normal = normalize((model * vec4<f32>(normal, 0.0)).xyz);
    var col = body.rgb;
    if (part == 1u) {
        col = title.rgb;
    } else if (part == 2u) {
        col = footer.rgb;
    }
    o.color = col;
    return o;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let l = normalize(cam.light_dir.xyz);
    let lambert = max(dot(n, l), 0.0);
    // Nearly flat so the front color matches the SDF backend; sides get a subtle depth cue.
    let ambient = 0.9;
    let shade = ambient + (1.0 - ambient) * lambert;
    return vec4<f32>(in.color * shade, 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn extruded(width: f32, height: f32) -> NodeMesh {
        build_node_mesh(&NodeMeshParams { width, height, ..NodeMeshParams::default() })
    }

    #[test]
    fn mesh_has_geometry_and_parts() {
        let mesh = extruded(320.0, 160.0);
        assert!(!mesh.vertices.is_empty() && !mesh.indices.is_empty());
        assert!(mesh.indices.len().is_multiple_of(3));
        let n = mesh.vertices.len() as u32;
        assert!(mesh.indices.iter().all(|&i| i < n));
        assert!(mesh.vertices.iter().any(|v| v.part == part::TITLE));
        assert!(mesh.vertices.iter().any(|v| v.part == part::BODY));
    }

    #[test]
    fn flat_is_single_layer_forward() {
        let mesh = build_node_mesh(&NodeMeshParams { shape: NodeShape::Flat, ..NodeMeshParams::default() });
        assert!(mesh.vertices.iter().all(|v| v.normal[2] > 0.9));
    }

    /// True circular corners: sampling the top-right corner arc, every silhouette point must be
    /// exactly `r` from the corner center in BOTH x and y — a circle, not an aspect-stretched ellipse.
    #[test]
    fn corners_are_true_circles_regardless_of_aspect() {
        let r = 20.0;
        for (w, h) in [(400.0, 120.0), (120.0, 400.0), (200.0, 200.0)] {
            let hw = w * 0.5;
            let hh = h * 0.5;
            let cx = hw - r;
            let cy = hh - r;
            let mut steps = 0;
            for k in 0..=24 {
                let y = (hh - r) + r * (k as f32 / 24.0);
                let x = half_width_at(y, hw, hh, r);
                if x < hw - 1e-3 {
                    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                    assert!((d - r).abs() < 1e-2, "corner not circular: d={d} r={r} (w={w},h={h})");
                    steps += 1;
                }
            }
            assert!(steps > 0, "no corner samples for w={w},h={h}");
        }
    }

    /// Every triangle winds outward (agrees with stored normals; extruded faces point away from
    /// center) — no flipped / inside-out faces.
    #[test]
    fn windings_agree_with_normals() {
        for shape in [NodeShape::Flat, NodeShape::Extruded] {
            let mesh = build_node_mesh(&NodeMeshParams { shape, ..NodeMeshParams::default() });
            for tri in mesh.indices.as_chunks::<3>().0 {
                let p = |i: u32| Vec3::from_array(mesh.vertices[i as usize].position);
                let n = |i: u32| Vec3::from_array(mesh.vertices[i as usize].normal);
                let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
                let face = (b - a).cross(c - a);
                if face.length_squared() < 1e-9 {
                    continue;
                }
                let face_n = face.normalize();
                let stored = (n(tri[0]) + n(tri[1]) + n(tri[2])).normalize_or_zero();
                assert!(face_n.dot(stored) > 0.5, "flipped face for {shape:?}");
                if shape == NodeShape::Extruded {
                    let centroid = (a + b + c) / 3.0;
                    assert!(face_n.dot(centroid) > 0.0, "inside-out face for {shape:?}");
                }
            }
        }
    }
}
