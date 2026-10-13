//! Box-filter mip-chain generator for the glyph atlas textures.
//!
//! wgpu never auto-generates mips, and the atlas is a dynamically-written sparse
//! texture (glyphs land at arbitrary packed rects as they're first needed), so
//! there's no single upload point to hook a generic mip helper into. Instead:
//! whenever [`crate::atlas::GlyphAtlas`] writes a new glyph (or grows), it marks
//! its mip chain dirty; [`MipGenerator::generate`] then re-derives every mip
//! level from level 0 in one pass, run once per frame if dirty (see
//! `GlyphAtlas::flush_mips`) rather than per glyph.

use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, ColorTargetState, ColorWrites,
    CommandEncoder, Device, FilterMode, FragmentState, LoadOp, MultisampleState, Operations,
    PipelineLayoutDescriptor, PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor,
    ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, Texture, TextureFormat,
    TextureSampleType, TextureView, TextureViewDescriptor, TextureViewDimension, VertexState,
};

const SHADER: &str = include_str!("shaders/mipgen.wgsl");

/// Regenerates the mip chain of a single glyph-atlas texture (color or mask —
/// each format needs its own pipeline since render targets are format-specific).
pub struct MipGenerator {
    pipeline: RenderPipeline,
    bind_group_layout: BindGroupLayout,
    sampler: Sampler,
}

impl MipGenerator {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("wgpu_text_mipgen_shader"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("wgpu_text_mipgen_bgl"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        view_dimension: TextureViewDimension::D2,
                        sample_type: TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("wgpu_text_mipgen_sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("wgpu_text_mipgen_pl"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("wgpu_text_mipgen_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            bind_group_layout,
            sampler,
        }
    }

    /// Re-derive every mip level `1..mip_count` of `texture` from level 0, one
    /// render pass per level (each level's source is the level just generated
    /// before it, so 4->16 etc. all narrow correctly instead of every level
    /// re-sampling level 0 directly).
    pub fn generate(
        &self,
        device: &Device,
        encoder: &mut CommandEncoder,
        texture: &Texture,
        mip_count: u32,
    ) {
        for level in 1..mip_count {
            let src_view = texture.create_view(&TextureViewDescriptor {
                label: Some("wgpu_text_mipgen_src"),
                base_mip_level: level - 1,
                mip_level_count: Some(1),
                ..Default::default()
            });
            let dst_view = texture.create_view(&TextureViewDescriptor {
                label: Some("wgpu_text_mipgen_dst"),
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            });
            let bind_group = device.create_bind_group(&BindGroupDescriptor {
                label: Some("wgpu_text_mipgen_bg"),
                layout: &self.bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(&src_view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.render_level(encoder, &dst_view, &bind_group);
        }
    }

    fn render_level(&self, encoder: &mut CommandEncoder, dst_view: &TextureView, bind_group: &BindGroup) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("wgpu_text_mipgen_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: dst_view,
                resolve_target: None,
                depth_slice: None,
                ops: Operations {
                    load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Mip levels for a `size x size` texture (full chain down to 1x1): `size.ilog2() + 1`.
pub fn mip_levels_for_size(size: u32) -> u32 {
    size.max(1).ilog2() + 1
}
