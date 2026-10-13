//! Host-style GPU integration test for the public host API (task: "one implementation shared with
//! `CompiledOcio`" — vfx-ocio's GPU host API, `docs/ROADMAP.md`'s GPU-host-API item).
//!
//! Simulates exactly the situation `vfx-view`'s `OcioGpuPass` and `usd-gl-engine`'s
//! `from_processor_wgsl` consumer are in: a host that (1) has its OWN compute pipeline with its OWN
//! entry point name (not `"main"`, the name `gpu/runtime.rs`'s executors use), (2) declares its OWN
//! uniform at `@group(3)` (host-territory per [`vfx_ocio::OCIO_HOST_GROUP_START`]), and (3) runs an
//! OCIO transform that reads `@group(1)` dynamic uniforms (`ExposureContrast`) — the exact class of
//! op that silently read UNBOUND memory under vfx-view's old `wrap_ocio_wgsl` (which declared its
//! own uniform AT `@group(1) @binding(0)`, colliding with `ocio_scalars`, and never bound
//! `@group(1)`/`@group(2)` at all).
//!
//! Builds NO shader text by hand beyond the host's own shell: the colour maths comes entirely from
//! [`GpuProcessor::wgsl_prelude`], and OCIO's own resources come entirely from
//! [`GpuOcioResources::for_pipeline`] — the same two calls `gpu/runtime.rs`'s `CompiledOcio::compile`
//! makes internally. Only `@group(0)` (I/O) and `@group(3)` (the host's own uniform) are built by
//! this test, exactly as an external host would.
//!
//! Runs on [`gpu_mem::shared_device`] like every other `--features gpu` test in this crate (a real
//! device is required; `VFX_REQUIRE_GPU=1` makes a missing one a hard failure rather than a skip) —
//! nothing here depends on `gpu_mem`'s device being "the" shared one; a host with its own
//! `wgpu::Instance`/`Device` would use the identical `GpuOcioResources`/`wgsl_prelude` calls.
#![cfg(feature = "gpu")]

use gpu_mem::wgpu;
use vfx_ocio::{
    ExposureContrastStyle, ExposureContrastTransform, GpuLanguage, GpuOcioResources, GpuProcessor,
    OCIO_HOST_GROUP_START, Processor, Transform, TransformDirection, WGSL_TRANSFORM_FUNCTION,
};

fn have_gpu(test: &str) -> bool {
    let present = gpu_mem::shared_device().is_some();
    if !present {
        vfx_core::test_support::gpu_unavailable(test, "gpu_mem::shared_device() is None");
    }
    present
}

fn close(actual: f32, expected: f32, eps: f32) -> bool {
    if actual == expected {
        return true;
    }
    let div = expected.abs().max(1.0);
    ((actual - expected).abs() / div) <= eps
}

fn align_up(v: u32, a: u32) -> u32 {
    v.div_ceil(a) * a
}

/// The host's own compute shell: its OWN entry point name (`host_apply`, proving nothing here
/// depends on `gpu/runtime.rs`'s private `"main"` convention), its OWN `@group(0)` I/O (D1
/// host-buffer round trip, matching this crate's own shell shape so the numeric comparison isn't
/// confounded by a different I/O path), and a host uniform at `@group(3) @binding(0)` — exactly
/// [`OCIO_HOST_GROUP_START`] — applied AFTER [`WGSL_TRANSFORM_FUNCTION`] runs.
fn host_shell_wgsl() -> String {
    format!(
        "\
struct HostUniforms {{
    scale: f32,
}};

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var tex_sampler: sampler;
@group({host}) @binding(0) var<uniform> host_uniforms: HostUniforms;

@compute @workgroup_size(8, 8)
fn host_apply(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let dims = textureDimensions(input_texture);
    if (gid.x >= dims.x || gid.y >= dims.y) {{ return; }}
    let uv = (vec2<f32>(gid.xy) + vec2<f32>(0.5, 0.5)) / vec2<f32>(dims);
    let src = textureSampleLevel(input_texture, tex_sampler, uv, 0.0);
    let transformed = {func}(src);
    let scaled = vec4<f32>(transformed.rgb * host_uniforms.scale, transformed.a);
    textureStore(output_texture, vec2<i32>(gid.xy), scaled);
}}
",
        host = OCIO_HOST_GROUP_START,
        func = WGSL_TRANSFORM_FUNCTION,
    )
}

/// A GradingPrimary-adjacent, uniform-bearing op that reads `@group(1)` (ExposureContrast): this is
/// the case that silently misrendered under vfx-view's old group-1-colliding host uniform.
fn exposure_contrast_processor() -> Processor {
    let t = Transform::ExposureContrast(ExposureContrastTransform {
        name: String::new(),
        exposure: 0.6,
        contrast: 1.15,
        gamma: 1.05,
        pivot: 0.18,
        log_exposure_step: 0.088,
        log_mid_gray: 0.435,
        style: ExposureContrastStyle::Linear,
        direction: TransformDirection::Forward,
        dynamic: vfx_ocio::EcDynamic::NONE,
    });
    Processor::from_transform(&t, TransformDirection::Forward)
        .expect("processor from ExposureContrast transform")
}

fn synthetic_image() -> Vec<[f32; 4]> {
    vec![
        [0.10, 0.20, 0.30, 1.0],
        [0.90, 0.15, 0.40, 0.5],
        [0.25, 0.75, 0.55, 1.0],
        [1.20, 0.35, 0.60, 0.8],
        [0.05, 0.66, 0.99, 1.0],
        [0.40, 0.48, 0.02, 0.25],
    ]
}

/// Builds the host's pipeline (its own entry point, `layout: None` auto layout — the "host does NOT
/// build an explicit pipeline layout" case), OCIO's `@group(1)`/`@group(2)` resources via
/// [`GpuOcioResources::for_pipeline`], and the host's own `@group(3)` uniform buffer/bind group, then
/// runs one dispatch and reads back the result. Mirrors the D1 round trip `gpu/runtime.rs`'s
/// `GpuBufferExecutor::execute` uses internally, built here entirely from the PUBLIC host API.
fn run_host_pipeline(
    processor: &Processor,
    host_scale: f32,
    width: u32,
    height: u32,
    pixels: &[[f32; 4]],
) -> Vec<[f32; 4]> {
    let gpu = gpu_mem::shared_device().expect("shared GPU device must be present");
    let device = &gpu.device;
    let queue = &gpu.queue;

    let gpu_proc = GpuProcessor::from_processor_wgsl(processor)
        .expect("processor must be WGSL-native for this test");
    let code = gpu_proc
        .generate_shader(GpuLanguage::Wgsl)
        .expect("generate_shader must succeed for the code metadata (textures/uniforms)");
    let prelude = gpu_proc.wgsl_prelude();
    let shader_src = format!("{prelude}{}", host_shell_wgsl());

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("gpu_host_api test shader"),
        source: wgpu::ShaderSource::Wgsl(shader_src.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("gpu_host_api test pipeline"),
        layout: None,
        module: &module,
        entry_point: Some("host_apply"),
        compilation_options: Default::default(),
        cache: None,
    });

    // OCIO's own resources — the SAME builder `CompiledOcio::compile` uses, on THIS pipeline.
    let ocio_resources = GpuOcioResources::for_pipeline(device, queue, &code, &pipeline);

    // The host's own @group(3) uniform.
    let host_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("host uniforms"),
        size: 16, // one f32 scalar, padded to the 16-byte uniform-buffer minimum binding alignment
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut host_bytes = [0u8; 16];
    host_bytes[0..4].copy_from_slice(&host_scale.to_le_bytes());
    queue.write_buffer(&host_buf, 0, &host_bytes);
    let host_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("host @group(3)"),
        layout: &pipeline.get_bind_group_layout(OCIO_HOST_GROUP_START),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: host_buf.as_entire_binding(),
        }],
    });

    // @group(0): the host's own D1 I/O, same shape as `gpu/runtime.rs`'s D1 shell.
    let tex_size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let input_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gpu_host_api input"),
        size: tex_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let output_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gpu_host_api output"),
        size: tex_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let input_bytes: &[u8] = bytemuck::cast_slice(pixels);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &input_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        input_bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 16),
            rows_per_image: Some(height),
        },
        tex_size,
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
    let input_view = input_tex.create_view(&Default::default());
    let output_view = output_tex.create_view(&Default::default());
    let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gpu_host_api g0"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&input_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&output_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    let row_bytes = align_up(width * 16, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu_host_api readback"),
        size: (row_bytes as u64) * (height as u64),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bg0, &[]);
        if let Some(bg1) = ocio_resources.bind_group(vfx_ocio::OCIO_UNIFORM_GROUP) {
            pass.set_bind_group(vfx_ocio::OCIO_UNIFORM_GROUP, bg1, &[]);
        }
        if let Some(bg2) = ocio_resources.bind_group(vfx_ocio::OCIO_TEXTURE_GROUP) {
            pass.set_bind_group(vfx_ocio::OCIO_TEXTURE_GROUP, bg2, &[]);
        }
        pass.set_bind_group(OCIO_HOST_GROUP_START, &host_bind_group, &[]);
        pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &output_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(height),
            },
        },
        tex_size,
    );
    queue.submit(std::iter::once(encoder.finish()));
    read_rgba32f(device, &readback, row_bytes, width, height)
}

/// The full host scenario: own entry point, own `@group(3)` uniform, OCIO op with `@group(1)`
/// uniforms, `layout: None` auto pipeline layout (mirrors what a host that does NOT hand-build an
/// explicit `wgpu::PipelineLayout` would do). Compares against the CPU oracle scaled by the SAME
/// host multiplier, proving the host's own post-processing and OCIO's transform compose correctly on
/// one pipeline with no binding collisions.
#[test]
fn host_pipeline_with_own_entry_point_and_group3_uniform_matches_cpu() {
    if !have_gpu("host_pipeline_with_own_entry_point_and_group3_uniform_matches_cpu") {
        return;
    }

    let processor = exposure_contrast_processor();
    let (w, h) = (3u32, 2u32);
    let input = synthetic_image();
    let host_scale = 1.25f32;

    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);
    for px in &mut cpu {
        px[0] *= host_scale;
        px[1] *= host_scale;
        px[2] *= host_scale;
    }

    let gpu = run_host_pipeline(&processor, host_scale, w, h, &input);
    assert_eq!(gpu.len(), cpu.len(), "pixel count mismatch");

    let eps = 1e-4;
    for (i, (g, c)) in gpu.iter().zip(&cpu).enumerate() {
        for ch in 0..4 {
            assert!(
                close(g[ch], c[ch], eps),
                "pixel {i} chan {ch}: gpu={:.6} cpu(+host scale)={:.6} (eps={eps})",
                g[ch],
                c[ch]
            );
        }
    }
}

/// Same scenario, but with an EXPLICIT `wgpu::PipelineLayout` built from
/// [`GpuOcioResources::build`]'s owned bind-group layouts plus the host's own — the "host DOES
/// hand-build an explicit pipeline layout" case task item 3 requires working too.
#[test]
fn host_pipeline_with_explicit_layout_matches_cpu() {
    if !have_gpu("host_pipeline_with_explicit_layout_matches_cpu") {
        return;
    }

    let gpu = gpu_mem::shared_device().expect("shared GPU device must be present");
    let device = &gpu.device;
    let queue = &gpu.queue;

    let processor = exposure_contrast_processor();
    let (w, h) = (3u32, 2u32);
    let input = synthetic_image();
    let host_scale = 0.8f32;

    let gpu_proc = GpuProcessor::from_processor_wgsl(&processor).unwrap();
    let code = gpu_proc.generate_shader(GpuLanguage::Wgsl).unwrap();
    let prelude = gpu_proc.wgsl_prelude();
    let shader_src = format!("{prelude}{}", host_shell_wgsl());
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("gpu_host_api explicit-layout shader"),
        source: wgpu::ShaderSource::Wgsl(shader_src.into()),
    });

    // OCIO's resources built with THEIR OWN owned bind-group layouts (no pipeline exists yet).
    let ocio_resources = GpuOcioResources::build(device, queue, &code);

    // @group(0) layout (the host's own D1 I/O).
    let group0_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("g0 layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba32Float,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    // @group(3) layout (the host's own uniform).
    let group3_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("g3 layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });

    // Assemble the explicit pipeline layout in group order: 0, [1], [2], 3. This processor
    // (ExposureContrast, no LUTs) has @group(1) but not @group(2); an explicit `PipelineLayout`
    // must still list every group index the shader references, in order, with no gaps.
    let group1_layout = ocio_resources
        .bind_group_layout(vfx_ocio::OCIO_UNIFORM_GROUP)
        .expect("ExposureContrast must register @group(1)");
    assert!(
        ocio_resources
            .bind_group_layout(vfx_ocio::OCIO_TEXTURE_GROUP)
            .is_none(),
        "ExposureContrast has no LUTs; @group(2) must not exist"
    );
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("gpu_host_api explicit pipeline layout"),
        // `bind_group_layouts[i]` IS `@group(i)` — this processor has no LUTs (no `@group(2)`), but
        // the host shell still declares its uniform at literal `@group(3)` (`OCIO_HOST_GROUP_START`),
        // so index 2 must be `None` to keep the array's positions aligned with the group numbers.
        bind_group_layouts: &[
            Some(&group0_layout),
            Some(group1_layout),
            None,
            Some(&group3_layout),
        ],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("gpu_host_api explicit-layout pipeline"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("host_apply"),
        compilation_options: Default::default(),
        cache: None,
    });

    let host_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("host uniforms"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut host_bytes = [0u8; 16];
    host_bytes[0..4].copy_from_slice(&host_scale.to_le_bytes());
    queue.write_buffer(&host_buf, 0, &host_bytes);
    let host_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("host @group(3) explicit"),
        layout: &group3_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: host_buf.as_entire_binding(),
        }],
    });

    let tex_size = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    let input_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("explicit input"),
        size: tex_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let output_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("explicit output"),
        size: tex_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let input_bytes: &[u8] = bytemuck::cast_slice(&input);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &input_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        input_bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 16),
            rows_per_image: Some(h),
        },
        tex_size,
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
    let input_view = input_tex.create_view(&Default::default());
    let output_view = output_tex.create_view(&Default::default());
    let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("explicit g0"),
        layout: &group0_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&input_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&output_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    let row_bytes = align_up(w * 16, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("explicit readback"),
        size: (row_bytes as u64) * (h as u64),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bg0, &[]);
        pass.set_bind_group(
            vfx_ocio::OCIO_UNIFORM_GROUP,
            ocio_resources
                .bind_group(vfx_ocio::OCIO_UNIFORM_GROUP)
                .unwrap(),
            &[],
        );
        pass.set_bind_group(OCIO_HOST_GROUP_START, &host_bind_group, &[]);
        pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &output_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(h),
            },
        },
        tex_size,
    );
    queue.submit(std::iter::once(encoder.finish()));
    let got = read_rgba32f(device, &readback, row_bytes, w, h);

    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);
    for px in &mut cpu {
        px[0] *= host_scale;
        px[1] *= host_scale;
        px[2] *= host_scale;
    }

    let eps = 1e-4;
    for (i, (g, c)) in got.iter().zip(&cpu).enumerate() {
        for ch in 0..4 {
            assert!(
                close(g[ch], c[ch], eps),
                "explicit-layout pixel {i} chan {ch}: gpu={:.6} cpu(+host scale)={:.6} (eps={eps})",
                g[ch],
                c[ch]
            );
        }
    }
}

/// A RENDER host: `ocio_transform` called from a FRAGMENT shader, OCIO's resources built with
/// [`GpuOcioResources::build_for`]`(.., FRAGMENT)` and assembled into an explicit render pipeline
/// layout next to the host's own `@group(0)` - the shape of a viewer that colour-manages the tiles
/// it draws inside its own render pass. The `@group(1)` dynamic uniforms (ExposureContrast) must
/// reach the fragment stage; the result equals the CPU processor.
#[test]
fn host_fragment_pipeline_matches_cpu() {
    if !have_gpu("host_fragment_pipeline_matches_cpu") {
        return;
    }
    let gpu = gpu_mem::shared_device().expect("shared GPU device must be present");
    let (device, queue) = (&gpu.device, &gpu.queue);

    let processor = exposure_contrast_processor();
    let (w, h) = (3u32, 2u32);
    let input = synthetic_image();
    let gpu_proc = GpuProcessor::from_processor_wgsl(&processor).unwrap();
    let code = gpu_proc.generate_shader(GpuLanguage::Wgsl).unwrap();
    let shell = format!(
        "\
@group(0) @binding(0) var input_texture: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {{
    return {func}(textureLoad(input_texture, vec2<i32>(pos.xy), 0));
}}
",
        func = WGSL_TRANSFORM_FUNCTION,
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("gpu_host_api fragment shader"),
        source: wgpu::ShaderSource::Wgsl(format!("{}{shell}", gpu_proc.wgsl_prelude()).into()),
    });
    let ocio = GpuOcioResources::build_for(device, queue, &code, wgpu::ShaderStages::FRAGMENT);
    let group0_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("g0 layout (fragment)"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });
    let group1_layout = ocio
        .bind_group_layout(vfx_ocio::OCIO_UNIFORM_GROUP)
        .expect("@group(1)");
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("gpu_host_api fragment pipeline layout"),
        bind_group_layouts: &[Some(&group0_layout), Some(group1_layout)],
        immediate_size: 0,
    });
    let format = wgpu::TextureFormat::Rgba32Float;
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("gpu_host_api fragment pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(format.into())],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    let extent = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    let make = |usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let src = make(wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    queue.write_texture(
        src.as_image_copy(),
        bytemuck::cast_slice(&input),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 16),
            rows_per_image: Some(h),
        },
        extent,
    );
    let dst = make(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    let g0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &group0_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&src.create_view(&Default::default())),
        }],
    });
    let row = align_up(w * 16, 256);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let view = dst.create_view(&Default::default());
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &g0, &[]);
        pass.set_bind_group(
            vfx_ocio::OCIO_UNIFORM_GROUP,
            ocio.bind_group(vfx_ocio::OCIO_UNIFORM_GROUP),
            &[],
        );
        pass.draw(0..3, 0..1);
    }
    enc.copy_texture_to_buffer(
        dst.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        extent,
    );
    queue.submit([enc.finish()]);
    let got = read_rgba32f(device, &readback, row, w, h);

    let mut want = input.clone();
    processor.apply_rgba(&mut want);
    for (i, (g, e)) in got.iter().zip(&want).enumerate() {
        for c in 0..4 {
            assert!(
                close(g[c], e[c], 1e-4),
                "pixel {i} channel {c}: gpu {} cpu {}",
                g[c],
                e[c]
            );
        }
    }
}

/// Map `readback` (after its copy was submitted) and return its `w x h` `Rgba32Float` texels,
/// dropping the `row_bytes` padding of each row.
fn read_rgba32f(
    device: &wgpu::Device,
    readback: &wgpu::Buffer,
    row_bytes: u32,
    w: u32,
    h: u32,
) -> Vec<[f32; 4]> {
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
    rx.recv()
        .expect("map callback channel")
        .expect("buffer map");
    let data = slice.get_mapped_range().expect("get_mapped_range");
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h as usize {
        let row_start = y * row_bytes as usize;
        let row_end = row_start + (w as usize) * 16;
        let row_pixels: &[[f32; 4]] = bytemuck::cast_slice(&data[row_start..row_end]);
        out.extend_from_slice(row_pixels);
    }
    drop(data);
    readback.unmap();
    out
}
