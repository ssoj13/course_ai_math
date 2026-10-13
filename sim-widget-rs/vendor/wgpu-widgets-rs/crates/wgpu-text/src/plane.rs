//! Plane-locked text: instanced quads transformed by per-label `surface_xfm`.

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use glyphon::{
    Attrs, Buffer as TextBuffer, ContentType, Family, FontSystem, Metrics, Shaping, SwashCache,
};
use wgpu::{
    include_wgsl, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, Buffer as GpuBuffer,
    BufferBindingType, BufferDescriptor, BufferUsages, ColorTargetState, ColorWrites,
    CompareFunction, DepthBiasState, DepthStencilState, Device, FragmentState, MultisampleState,
    PipelineLayoutDescriptor, PrimitiveState, PrimitiveTopology, Queue, RenderPass,
    RenderPipeline, RenderPipelineDescriptor, ShaderStages, StencilState, TextureFormat,
    VertexState,
};

// Note: `uniform_bgl` is built locally for `pipeline_layout` + `uniform_bg`; the bind group
// layout has no later uses, so it is not stored on the struct.

use crate::atlas::GlyphAtlas;
use crate::color::rgba_to_glyphon;
use crate::labels::PlaneLabel;

/// Vertical shift so glyph ink is centered in a layout box of height `box_h`.
fn optical_center_shift_y(ink_top: f32, ink_bottom: f32, box_h: f32) -> f32 {
    if ink_bottom <= ink_top {
        return 0.0;
    }
    box_h * 0.5 - (ink_top + ink_bottom) * 0.5
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct PlaneGlyphInstance {
    pub surface_xfm: [[f32; 4]; 4],
    pub local_rect: [f32; 4],
    pub uv_rect: [f32; 4],
    pub color: [f32; 4],
    pub content_type: u32,
    pub _pad: [u32; 3],
    /// Surface-local clip `[min_x, min_y, max_x, max_y]`. See [`PlaneLabel::NO_CLIP`].
    pub clip_rect: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PlaneUniforms {
    view_proj: [[f32; 4]; 4],
}

pub struct PlaneTextRenderer {
    pipeline: RenderPipeline,
    uniform_buf: GpuBuffer,
    uniform_bg: BindGroup,
    instance_buf: GpuBuffer,
    instance_capacity: u32,
    instances: Vec<PlaneGlyphInstance>,
}

impl PlaneTextRenderer {
    pub fn new(device: &Device, format: TextureFormat, atlas_layout: &BindGroupLayout) -> Self {
        let shader = device.create_shader_module(include_wgsl!("shaders/plane_text.wgsl"));

        let uniform_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("wgpu_text_plane_uniform_bgl"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("wgpu_text_plane_layout"),
            bind_group_layouts: &[Some(&uniform_bgl), Some(atlas_layout)],
            ..Default::default()
        });

        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<PlaneGlyphInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 16,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 32,
                    shader_location: 2,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 48,
                    shader_location: 3,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 64,
                    shader_location: 4,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 80,
                    shader_location: 5,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 96,
                    shader_location: 6,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Uint32,
                    offset: 112,
                    shader_location: 7,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 128,
                    shader_location: 8,
                },
            ],
        };

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("wgpu_text_plane"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(instance_layout)], // wgpu 30: &[Option<VertexBufferLayout>]
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                ..Default::default()
            },
            // Plane text is a UI overlay (node titles, port names, field values): it must
            // composite on top of the graph in submission order, not be depth-tested against
            // coplanar node chrome (which culled all but the first surface). Keep the depth
            // attachment for pass compatibility but never test or write.
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::Always),
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let uniform_buf = device.create_buffer(&BufferDescriptor {
            label: Some("wgpu_text_plane_uniform"),
            size: std::mem::size_of::<PlaneUniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("wgpu_text_plane_uniform_bg"),
            layout: &uniform_bgl,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        let instance_capacity = 256;
        let instance_buf = device.create_buffer(&BufferDescriptor {
            label: Some("wgpu_text_plane_instances"),
            size: (std::mem::size_of::<PlaneGlyphInstance>() as u64) * instance_capacity as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            uniform_buf,
            uniform_bg,
            instance_buf,
            instance_capacity,
            instances: Vec::new(),
        }
    }

    // Eight arguments, and each is a distinct resource this pass genuinely needs: two GPU handles,
    // the font system, the swash cache, the atlas, the labels and the buffer pool. Bundling them
    // into a struct would move the same list one level away without removing anything.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        atlas: &mut GlyphAtlas,
        labels: &[PlaneLabel],
        buffers: &mut Vec<TextBuffer>,
    ) -> Result<(), String> {
        self.instances.clear();
        atlas.trim();

        while buffers.len() < labels.len() {
            buffers.push(TextBuffer::new(font_system, Metrics::new(12.0, 14.0)));
        }

        for (label, buffer) in labels.iter().zip(buffers.iter_mut()) {
            let scale = label.raster_scale.max(1.0);
            let inv = 1.0 / scale;
            let metrics = Metrics::new(label.font_size * scale, label.line_height * scale);
            buffer.set_metrics(metrics);
            // A one-line label is laid out UNBOUNDED and cut by `clip_rect`. Handing cosmic-text
            // a width makes it wrap, and a strip one line tall has nowhere to put the second line -
            // a narrow one even breaks mid-word, since a word that does not fit is split per glyph.
            if label.wrap {
                buffer.set_wrap(glyphon::cosmic_text::Wrap::WordOrGlyph);
                buffer.set_size(
                    Some(label.local_rect[2].max(1.0) * scale),
                    Some(label.local_rect[3].max(1.0) * scale),
                );
            } else {
                buffer.set_wrap(glyphon::cosmic_text::Wrap::None);
                buffer.set_size(None, None);
            }
            let attrs = Attrs::new().family(Family::SansSerif);
            buffer.set_text(&label.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(font_system, false);

            let xfm = mat4_to_cols(label.surface_xfm);
            let color = label.color;
            let glyph_color = rgba_to_glyphon(color);
            let box_h = label.local_rect[3];

            let mut batch = Vec::new();
            let mut ink_top = f32::MAX;
            let mut ink_bottom = f32::MIN;

            for run in buffer.layout_runs() {
                for glyph in run.glyphs.iter() {
                    // Text area origin is in `surface_xfm`; shaped runs start at (0, 0) local.
                    let physical = glyph.physical((0.0, 0.0), 1.0);
                    let Some(m) = atlas.ensure_glyph(
                        device,
                        queue,
                        font_system,
                        swash_cache,
                        physical.cache_key,
                    ) else {
                        continue;
                    };

                    let x = (physical.x + m.left as i32) as f32 * inv;
                    let y = (run.line_y.round() + physical.y as f32 - m.top as f32) * inv;
                    let w = m.width as f32 * inv;
                    let h = m.height as f32 * inv;
                    if w <= 0.0 || h <= 0.0 {
                        continue;
                    }

                    ink_top = ink_top.min(y);
                    ink_bottom = ink_bottom.max(y + h);

                    let ct = content_type_id(m.content_type);
                    let inst_color = match m.content_type {
                        ContentType::Color => [1.0, 1.0, 1.0, 1.0],
                        ContentType::Mask => {
                            let c = glyph_color;
                            [
                                c.r() as f32 / 255.0,
                                c.g() as f32 / 255.0,
                                c.b() as f32 / 255.0,
                                c.a() as f32 / 255.0,
                            ]
                        }
                    };

                    batch.push(PlaneGlyphInstance {
                        surface_xfm: xfm,
                        local_rect: [x, y, w, h],
                        uv_rect: m.uv,
                        color: inst_color,
                        content_type: ct,
                        _pad: [0; 3],
                        // `clip_rect` arrives already expressed in this label's local space
                        // (the caller subtracts the label origin) so it matches the glyph
                        // `local_pos` the fragment shader clip-tests against.
                        clip_rect: label.clip_rect,
                    });
                }
            }

            let shift_y = optical_center_shift_y(ink_top, ink_bottom, box_h);
            if shift_y.abs() > f32::EPSILON {
                for inst in &mut batch {
                    inst.local_rect[1] += shift_y;
                }
            }
            self.instances.extend(batch);
        }
        Ok(())
    }

    pub fn upload(&mut self, device: &Device, queue: &Queue) {
        let count = self.instances.len() as u32;
        if count == 0 {
            return;
        }
        let bytes = bytemuck::cast_slice(&self.instances);
        let needed = bytes.len() as u64;
        if needed > self.instance_buf.size() {
            let cap = count.next_power_of_two().max(256);
            self.instance_capacity = cap;
            self.instance_buf = device.create_buffer(&BufferDescriptor {
                label: Some("wgpu_text_plane_instances"),
                size: (std::mem::size_of::<PlaneGlyphInstance>() as u64) * cap as u64,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.instance_buf, 0, bytes);
    }

    pub fn update_view_proj(&self, queue: &Queue, view_proj: Mat4) {
        let u = PlaneUniforms {
            view_proj: mat4_to_cols(view_proj),
        };
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&u));
    }

    pub fn draw<'pass>(
        &'pass self,
        pass: &mut RenderPass<'pass>,
        atlas_bg: &'pass BindGroup,
        instance_count: u32,
    ) {
        if instance_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.uniform_bg, &[]);
        pass.set_bind_group(1, atlas_bg, &[]);
        pass.set_vertex_buffer(0, self.instance_buf.slice(..));
        pass.draw(0..4, 0..instance_count);
    }

    pub fn instance_count(&self) -> u32 {
        self.instances.len() as u32
    }
}

fn mat4_to_cols(m: Mat4) -> [[f32; 4]; 4] {
    m.to_cols_array_2d()
}

fn content_type_id(ct: ContentType) -> u32 {
    match ct {
        ContentType::Color => 0,
        ContentType::Mask => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::optical_center_shift_y;

    #[test]
    fn centers_ink_in_box() {
        let shift = optical_center_shift_y(4.0, 12.0, 18.0);
        assert!((shift - 1.0).abs() < 1e-5);
    }

    #[test]
    fn no_shift_for_empty_ink() {
        assert_eq!(optical_center_shift_y(0.0, 0.0, 18.0), 0.0);
    }
}
