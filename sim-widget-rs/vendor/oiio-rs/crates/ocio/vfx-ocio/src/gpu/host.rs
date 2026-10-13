//! Host-owned GPU resources realizing OCIO's `@group(1)`/`@group(2)` bind groups — the SSOT
//! resource builder any host (this crate's own [`super::runtime::CompiledOcio`], or an external
//! consumer running the emitted WGSL on its OWN `wgpu::Device`) uses to turn a [`GpuShaderCode`]
//! into real `wgpu::Buffer`/`wgpu::Texture`/`wgpu::BindGroup` resources.
//!
//! # Why this exists (task: item 3 — one resource builder, no second copy)
//! Before this module, `gpu/runtime.rs`'s `CompiledOcio::compile` was the ONLY code that packed
//! scalar/array uniforms into `ocio_scalars`/`ocio_arrays_i`/`ocio_arrays_f`, uploaded LUT textures,
//! and built the two bind groups — inline, private, and unreachable by anything outside this crate.
//! An external host (vfx-view's `OcioGpuPass`, usd-gl-engine's `from_processor_wgsl` consumer) had
//! no choice but to re-derive that packing/upload logic itself, and both got it wrong in different
//! ways (see the crate's `AGENTS.md` §GPU host API and this file's `tests.rs` sibling). This module
//! is now the ONE implementation; `CompiledOcio::compile` calls [`GpuOcioResources::for_pipeline`]
//! instead of carrying its own copy.
//!
//! # Two entry points, one packing/upload core
//! A host may build its compute pipeline either way, and both are supported:
//! - [`GpuOcioResources::for_pipeline`] — the host (or [`super::runtime::CompiledOcio`]) creates its
//!   `wgpu::ComputePipeline` with `layout: None` (wgpu's automatic/reflection layout, what this
//!   crate's own runtime does) and hands the pipeline here; bind groups are built against
//!   `pipeline.get_bind_group_layout(OCIO_UNIFORM_GROUP)`/`(OCIO_TEXTURE_GROUP)`, so no
//!   `wgpu::BindGroupLayout` is owned by the returned [`GpuOcioResources`].
//! - [`GpuOcioResources::build`] — the host wants an EXPLICIT `wgpu::PipelineLayout` (its own
//!   `@group(3)`+ bindings alongside OCIO's, all declared up front, no reflection). This creates and
//!   OWNS the two `wgpu::BindGroupLayout`s (exposed via [`GpuOcioResources::bind_group_layout`]) so
//!   the host can assemble `wgpu::PipelineLayoutDescriptor { bind_group_layouts: &[shell_layout,
//!   resources.bind_group_layout(OCIO_UNIFORM_GROUP).unwrap(), ...] }` BEFORE creating the pipeline.
//! - [`GpuOcioResources::build_for`] — the same explicit layouts for another shader stage: a host
//!   that runs `ocio_transform` in a FRAGMENT shader (a viewer drawing tiles in its render pass)
//!   builds them visible to `wgpu::ShaderStages::FRAGMENT`. `build` is `build_for(.., COMPUTE)`;
//!   the stage is the only difference, the packing and the LUT upload are the same code.
//!
//! Both paths share the same private packing (`pack_scalars`/`pack_arrays_i`/`pack_arrays_f`,
//! ported unchanged from the old `runtime.rs` inline code) and LUT texture-creation core — see
//! [`GpuOcioResources::make_buffers`]/[`GpuOcioResources::make_textures`].
//!
//! # Uniform updates (task: item 4)
//! [`GpuOcioResources::update_scalars`] re-packs and re-uploads `ocio_scalars` from a
//! [`GpuShaderCode`] whose uniform DEFAULT VALUES a host has changed via
//! [`GpuShaderCode::set_uniform_value`] — the dynamic-property path (exposure/contrast/gamma,
//! GradingPrimary/RgbCurve/HueCurve) a host updates per frame. It reuses the exact same
//! [`pack_scalars`] a fresh [`GpuOcioResources::build`]/`for_pipeline` call would, so "build once,
//! then update every frame" and "rebuild every frame" produce byte-identical buffer contents.

use gpu_mem::wgpu;

use super::contract::{OCIO_TEXTURE_GROUP, OCIO_UNIFORM_GROUP};
use super::{GpuInterpolation, GpuShaderCode, GpuTextureType, GpuUniformValue};

/// Packs every scalar uniform into the tight `array<u32>` `ocio_scalars` buffer, in registration
/// order — the order `emit_uniform_decls_wgsl` assigns each `<NAME>_OFFSET`. Arrays are skipped
/// (they ride the separate `ocio_arrays_{i,f}` buffers). `Bool`→u32 0/1, `Int`→i32 bits, everything
/// else→f32 bits, exactly as the `scalar_{f32,i32,u32,vec3,vec4,mat3,mat4}` accessors read them.
///
/// Ported unchanged from the old `runtime.rs::CompiledOcio::compile` inline body (PLAN9.md O0/O1's
/// packing rule) — this is now its ONE home.
fn pack_scalars(code: &GpuShaderCode) -> Vec<u8> {
    let mut bytes = Vec::new();
    let push_f = |b: &mut Vec<u8>, f: f32| b.extend_from_slice(&f.to_le_bytes());
    for u in code.uniforms() {
        match &u.default_value {
            GpuUniformValue::Float(v) => push_f(&mut bytes, *v),
            GpuUniformValue::Bool(b) => bytes.extend_from_slice(&(*b as u32).to_le_bytes()),
            GpuUniformValue::Int(i) => bytes.extend_from_slice(&i.to_le_bytes()),
            GpuUniformValue::Vec3(a) => a.iter().for_each(|&x| push_f(&mut bytes, x)),
            GpuUniformValue::Vec4(a) => a.iter().for_each(|&x| push_f(&mut bytes, x)),
            GpuUniformValue::Mat3(m) => m.iter().for_each(|&x| push_f(&mut bytes, x)),
            GpuUniformValue::Mat4(m) => m.iter().for_each(|&x| push_f(&mut bytes, x)),
            GpuUniformValue::ArrayFloat(_) | GpuUniformValue::ArrayInt(_) => {}
        }
    }
    bytes
}

/// Concatenates each `ArrayFloat` payload — PADDED to its `max_len` — into `ocio_arrays_f`, in
/// registration order. See `wgsl.rs::emit_uniform_decls_wgsl` for why the padding is load-bearing.
fn pack_arrays_f(code: &GpuShaderCode) -> Vec<u8> {
    let mut bytes = Vec::new();
    for u in code.uniforms() {
        if let GpuUniformValue::ArrayFloat(v) = &u.default_value {
            let len = u.max_len.unwrap_or(v.len());
            for i in 0..len {
                bytes.extend_from_slice(&v.get(i).copied().unwrap_or(0.0).to_le_bytes());
            }
        }
    }
    bytes
}

/// `ArrayInt` sibling of [`pack_arrays_f`] for the `ocio_arrays_i` buffer.
fn pack_arrays_i(code: &GpuShaderCode) -> Vec<u8> {
    let mut bytes = Vec::new();
    for u in code.uniforms() {
        if let GpuUniformValue::ArrayInt(v) = &u.default_value {
            let len = u.max_len.unwrap_or(v.len());
            for i in 0..len {
                bytes.extend_from_slice(&v.get(i).copied().unwrap_or(0).to_le_bytes());
            }
        }
    }
    bytes
}

/// Host-owned GPU resources for OCIO's `@group(1)` (scalar/array uniform storage buffers) and
/// `@group(2)` (LUT textures + samplers) — see the module docs for the two ways to build one.
///
/// Mirrors OCIO's `GpuShaderDesc` resource surface (`getUniform`/`getTexture`/`getTexture3D`
/// describe what a host must bind; this struct IS the bound resources) translated to wgpu.
pub struct GpuOcioResources {
    // `scalar_buf` is read back by `update_scalars`; `arrays_i_buf`/`arrays_f_buf` are write-once
    // (no dynamic array uniform exists in this crate today, see `update_scalars`'s docs) and, like
    // `lut_textures`/`lut_views`/`lut_samplers`, are kept alive here purely because the bind groups
    // above hold wgpu's own (refcounted) reference to the SAME resources, not these `Option`/`Vec`
    // fields — dropping the field early would still be safe (wgpu resources are independently
    // refcounted), but keeping them here documents ownership and matches the buffer/texture
    // lifetime pattern the rest of this crate's GPU code uses.
    scalar_buf: Option<wgpu::Buffer>,
    #[allow(dead_code)]
    arrays_i_buf: Option<wgpu::Buffer>,
    #[allow(dead_code)]
    arrays_f_buf: Option<wgpu::Buffer>,
    #[allow(dead_code)]
    lut_textures: Vec<wgpu::Texture>,
    #[allow(dead_code)]
    lut_views: Vec<wgpu::TextureView>,
    #[allow(dead_code)]
    lut_samplers: Vec<wgpu::Sampler>,
    bind_group_layout_1: Option<wgpu::BindGroupLayout>,
    bind_group_1: Option<wgpu::BindGroup>,
    bind_group_layout_2: Option<wgpu::BindGroupLayout>,
    bind_group_2: Option<wgpu::BindGroup>,
}

impl GpuOcioResources {
    /// Creates the `ocio_scalars`/`ocio_arrays_i`/`ocio_arrays_f` storage buffers this
    /// [`GpuShaderCode`] needs — only the ones it actually declares (matching
    /// `wgsl.rs::emit_uniform_decls_wgsl`, which omits empty ones).
    fn make_buffers(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        code: &GpuShaderCode,
    ) -> (
        Option<wgpu::Buffer>,
        Option<wgpu::Buffer>,
        Option<wgpu::Buffer>,
    ) {
        let make_storage = |name: &str, data: &[u8]| -> Option<wgpu::Buffer> {
            if data.is_empty() {
                return None;
            }
            let b = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(name),
                size: data.len() as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&b, 0, data);
            Some(b)
        };
        let scalar_buf = make_storage("ocio_scalars", &pack_scalars(code));
        let arrays_i_buf = make_storage("ocio_arrays_i", &pack_arrays_i(code));
        let arrays_f_buf = make_storage("ocio_arrays_f", &pack_arrays_f(code));
        (scalar_buf, arrays_i_buf, arrays_f_buf)
    }

    /// Uploads every LUT texture this [`GpuShaderCode`] declares, with a matching sampler each —
    /// `(idx+1)*2` / `+1` binding, exactly `wgsl.rs::emit_texture_bindings_wgsl`'s `binding_base`.
    fn make_textures(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        code: &GpuShaderCode,
    ) -> (
        Vec<wgpu::Texture>,
        Vec<wgpu::TextureView>,
        Vec<wgpu::Sampler>,
    ) {
        let mut textures = Vec::with_capacity(code.textures().len());
        let mut views = Vec::with_capacity(code.textures().len());
        let mut samplers = Vec::with_capacity(code.textures().len());
        for tex in code.textures() {
            let (dimension, size) = match tex.texture_type {
                // Genuine 1D texture (PLAN9.md O1): a D2-with-height-1 "packed" texture would NOT
                // satisfy `texture_1d<f32>` — wgpu validates dimension against the shader's declared
                // texture type exactly.
                GpuTextureType::Texture1D => (
                    wgpu::TextureDimension::D1,
                    wgpu::Extent3d {
                        width: tex.width,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                ),
                GpuTextureType::Texture2D => (
                    wgpu::TextureDimension::D2,
                    wgpu::Extent3d {
                        width: tex.width,
                        height: tex.height,
                        depth_or_array_layers: 1,
                    },
                ),
                GpuTextureType::Texture3D => (
                    wgpu::TextureDimension::D3,
                    wgpu::Extent3d {
                        width: tex.width,
                        height: tex.height,
                        depth_or_array_layers: tex.depth,
                    },
                ),
            };
            let wgpu_tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(&tex.name),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            // `tex.data` is already RGBA-f32 (`GpuTexture`'s own contract) — reinterpret, not
            // reassemble.
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &wgpu_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&tex.data),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(tex.width * 16),
                    rows_per_image: Some(size.height),
                },
                size,
            );
            let view = wgpu_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let filter = match tex.interpolation {
                GpuInterpolation::Nearest => wgpu::FilterMode::Nearest,
                GpuInterpolation::Linear => wgpu::FilterMode::Linear,
            };
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(&format!("{} sampler", tex.name)),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: filter,
                min_filter: filter,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            });
            textures.push(wgpu_tex);
            views.push(view);
            samplers.push(sampler);
        }
        (textures, views, samplers)
    }

    /// `@group(1)` bind-group entries for whichever of the three storage buffers exist.
    fn g1_entries<'a>(
        scalar_buf: &'a Option<wgpu::Buffer>,
        arrays_i_buf: &'a Option<wgpu::Buffer>,
        arrays_f_buf: &'a Option<wgpu::Buffer>,
    ) -> Vec<wgpu::BindGroupEntry<'a>> {
        let mut entries = Vec::new();
        if let Some(b) = scalar_buf {
            entries.push(wgpu::BindGroupEntry {
                binding: 0,
                resource: b.as_entire_binding(),
            });
        }
        if let Some(b) = arrays_i_buf {
            entries.push(wgpu::BindGroupEntry {
                binding: 1,
                resource: b.as_entire_binding(),
            });
        }
        if let Some(b) = arrays_f_buf {
            entries.push(wgpu::BindGroupEntry {
                binding: 2,
                resource: b.as_entire_binding(),
            });
        }
        entries
    }

    /// `@group(2)` bind-group entries: one texture + sampler pair per LUT at `(idx+1)*2` / `+1`.
    fn g2_entries<'a>(
        views: &'a [wgpu::TextureView],
        samplers: &'a [wgpu::Sampler],
    ) -> Vec<wgpu::BindGroupEntry<'a>> {
        let mut entries = Vec::new();
        for (idx, (view, sampler)) in views.iter().zip(samplers).enumerate() {
            let binding_base = ((idx + 1) * 2) as u32;
            entries.push(wgpu::BindGroupEntry {
                binding: binding_base,
                resource: wgpu::BindingResource::TextureView(view),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: binding_base + 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            });
        }
        entries
    }

    /// Builds resources whose bind groups are created against a PIPELINE's own automatic
    /// (`layout: None`) bind-group layouts — `pipeline` must already be compiled from WGSL that
    /// includes `code`'s prelude (e.g. [`super::GpuProcessor::wgsl_prelude`] plus the host's shell).
    ///
    /// This is what [`super::runtime::CompiledOcio::compile`] uses (it builds its pipeline with
    /// `layout: None`, same as before this refactor) and what a host with no need for an explicit
    /// `wgpu::PipelineLayout` should use too. No [`wgpu::BindGroupLayout`] is owned by the result —
    /// [`Self::bind_group_layout`] returns `None` for both groups.
    pub fn for_pipeline(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        code: &GpuShaderCode,
        pipeline: &wgpu::ComputePipeline,
    ) -> Self {
        let (scalar_buf, arrays_i_buf, arrays_f_buf) = Self::make_buffers(device, queue, code);
        let (lut_textures, lut_views, lut_samplers) = Self::make_textures(device, queue, code);

        let g1 = Self::g1_entries(&scalar_buf, &arrays_i_buf, &arrays_f_buf);
        let bind_group_1 = (!g1.is_empty()).then(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vfx-ocio group(1) uniforms"),
                layout: &pipeline.get_bind_group_layout(OCIO_UNIFORM_GROUP),
                entries: &g1,
            })
        });

        let g2 = Self::g2_entries(&lut_views, &lut_samplers);
        let bind_group_2 = (!g2.is_empty()).then(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vfx-ocio group(2) LUTs"),
                layout: &pipeline.get_bind_group_layout(OCIO_TEXTURE_GROUP),
                entries: &g2,
            })
        });

        Self {
            scalar_buf,
            arrays_i_buf,
            arrays_f_buf,
            lut_textures,
            lut_views,
            lut_samplers,
            bind_group_layout_1: None,
            bind_group_1,
            bind_group_layout_2: None,
            bind_group_2,
        }
    }

    /// Builds resources with their OWN explicit `wgpu::BindGroupLayout`s (read-only storage buffers
    /// at group 1, texture+sampler pairs at group 2) — for a host that wants to assemble an explicit
    /// `wgpu::PipelineLayout` (its own bindings at [`super::OCIO_HOST_GROUP_START`]+ declared
    /// alongside OCIO's) BEFORE creating its compute pipeline, rather than relying on wgpu's
    /// reflection-derived automatic layout.
    ///
    /// [`Self::bind_group_layout`] returns the owned layouts; a host passes them straight into
    /// `wgpu::PipelineLayoutDescriptor::bind_group_layouts` in group order.
    pub fn build(device: &wgpu::Device, queue: &wgpu::Queue, code: &GpuShaderCode) -> Self {
        Self::build_for(device, queue, code, wgpu::ShaderStages::COMPUTE)
    }

    /// [`Self::build`] with the owned layouts visible to `visibility` — e.g.
    /// `wgpu::ShaderStages::FRAGMENT` for a host that calls `ocio_transform` from a fragment shader.
    /// A LUT texture is `Rgba32Float` sampled with a filtering sampler, which needs the device
    /// feature `wgpu::Features::FLOAT32_FILTERABLE` wherever `code.textures()` is not empty.
    pub fn build_for(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        code: &GpuShaderCode,
        visibility: wgpu::ShaderStages,
    ) -> Self {
        let (scalar_buf, arrays_i_buf, arrays_f_buf) = Self::make_buffers(device, queue, code);
        let (lut_textures, lut_views, lut_samplers) = Self::make_textures(device, queue, code);

        let mut layout1_entries = Vec::new();
        if scalar_buf.is_some() {
            layout1_entries.push(storage_layout_entry(0, visibility));
        }
        if arrays_i_buf.is_some() {
            layout1_entries.push(storage_layout_entry(1, visibility));
        }
        if arrays_f_buf.is_some() {
            layout1_entries.push(storage_layout_entry(2, visibility));
        }
        let bind_group_layout_1 = (!layout1_entries.is_empty()).then(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("vfx-ocio group(1) layout"),
                entries: &layout1_entries,
            })
        });
        let g1 = Self::g1_entries(&scalar_buf, &arrays_i_buf, &arrays_f_buf);
        let bind_group_1 = bind_group_layout_1.as_ref().map(|layout| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vfx-ocio group(1) uniforms"),
                layout,
                entries: &g1,
            })
        });

        let mut layout2_entries = Vec::new();
        for (idx, tex) in code.textures().iter().enumerate() {
            let binding_base = ((idx + 1) * 2) as u32;
            layout2_entries.push(texture_layout_entry(
                binding_base,
                tex.texture_type,
                visibility,
            ));
            layout2_entries.push(sampler_layout_entry(binding_base + 1, visibility));
        }
        let bind_group_layout_2 = (!layout2_entries.is_empty()).then(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("vfx-ocio group(2) layout"),
                entries: &layout2_entries,
            })
        });
        let g2 = Self::g2_entries(&lut_views, &lut_samplers);
        let bind_group_2 = bind_group_layout_2.as_ref().map(|layout| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vfx-ocio group(2) LUTs"),
                layout,
                entries: &g2,
            })
        });

        Self {
            scalar_buf,
            arrays_i_buf,
            arrays_f_buf,
            lut_textures,
            lut_views,
            lut_samplers,
            bind_group_layout_1,
            bind_group_1,
            bind_group_layout_2,
            bind_group_2,
        }
    }

    /// The bind group for [`OCIO_UNIFORM_GROUP`] (1) or [`OCIO_TEXTURE_GROUP`] (2), or `None` if
    /// that group has nothing to bind (no dynamic uniforms / no LUTs) or `group` is neither.
    pub fn bind_group(&self, group: u32) -> Option<&wgpu::BindGroup> {
        match group {
            OCIO_UNIFORM_GROUP => self.bind_group_1.as_ref(),
            OCIO_TEXTURE_GROUP => self.bind_group_2.as_ref(),
            _ => None,
        }
    }

    /// The OWNED explicit bind-group layout for `group`, if this [`GpuOcioResources`] was built via
    /// [`Self::build`] (not [`Self::for_pipeline`], which borrows the pipeline's own reflection
    /// layout instead and returns `None` here for both groups).
    pub fn bind_group_layout(&self, group: u32) -> Option<&wgpu::BindGroupLayout> {
        match group {
            OCIO_UNIFORM_GROUP => self.bind_group_layout_1.as_ref(),
            OCIO_TEXTURE_GROUP => self.bind_group_layout_2.as_ref(),
            _ => None,
        }
    }

    /// Re-packs and re-uploads `ocio_scalars` from `code`'s CURRENT uniform default values (task:
    /// item 4 — dynamic properties). A host that changed a value via
    /// [`GpuShaderCode::set_uniform_value`] (exposure/contrast/gamma, a GradingPrimary/RgbCurve/
    /// HueCurve field, …) calls this once per change/frame instead of rebuilding the whole
    /// [`GpuOcioResources`]. No-op (returns `false`) if this shader has no scalar buffer at all —
    /// mirrors [`Self::bind_group`]'s "nothing to bind" case.
    ///
    /// Only `ocio_scalars` is covered: every dynamic property this crate exposes (the
    /// `EcDynamic` exposure-contrast scalars, `Processor::set_dynamic`; find their uniforms with
    /// `GpuProcessor::dynamic_uniforms`) is scalar-valued. A future dynamic ARRAY property would need the analogous
    /// `update_arrays_f`/`update_arrays_i`, not yet needed by anything in this crate.
    pub fn update_scalars(&self, queue: &wgpu::Queue, code: &GpuShaderCode) -> bool {
        match &self.scalar_buf {
            Some(buf) => {
                queue.write_buffer(buf, 0, &pack_scalars(code));
                true
            }
            None => false,
        }
    }
}

fn storage_layout_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_layout_entry(
    binding: u32,
    texture_type: GpuTextureType,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    let view_dimension = match texture_type {
        GpuTextureType::Texture1D => wgpu::TextureViewDimension::D1,
        GpuTextureType::Texture2D => wgpu::TextureViewDimension::D2,
        GpuTextureType::Texture3D => wgpu::TextureViewDimension::D3,
    };
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_layout_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
