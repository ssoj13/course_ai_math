//! Verification for the ACES 2.0 `RGB <-> JMh` GPU emitter (`gpu/aces2.rs`), the first ACES2
//! stage taken off the `[0,1]^3` LUT-bake path (ACES2-03).
//!
//! Two checks, keyed on the *unbounded* domain the old bake could not represent (J up to ~173,
//! M up to ~206, hue across the full circle — none of it inside the unit cube):
//!
//! 1. **naga parses** the full generated WGSL for both directions — WGSL is the dialect whose
//!    front end catches what the C-family three do not (`let`/`var`, swizzle assignment, operand
//!    typing).
//! 2. **Live wgpu** compiles the emitted WGSL as its real compute shader, with the uniforms the
//!    emitter uploads, runs it on a device over a buffer of points, and compares the read-back to
//!    the CPU oracle. Without an adapter or `FLOAT32_FILTERABLE` the test reports a skip
//!    (`vfx_core::test_support::gpu_unavailable`), a failure under `VFX_REQUIRE_GPU=1`. It adopts
//!    the cluster's single shared device (`gpu_mem::shared_device()`, crate gpu-info-rs).
//!
//! A third check used to compare a hand transcription of each shader body, kept in this file,
//! with the oracle. An emitter change never reached it, so it is gone: the live tests run the
//! emitted code itself.
//!
//! The oracle itself is anchored to OpenColorIO goldens by `red_gate_aces2_goldens.rs`; this file
//! trusts that and checks only that the GPU path reproduces the oracle.

use vfx_ocio::aces2::{
    ACES_AP0, ACES_AP1, Aces2State, ChromaCompressParams, Chromaticity, GamutCompressParams,
    JMhParams, Primaries, SharedCompressionParams, ToneScaleParams, gamut_compress_fwd_deg,
    gamut_compress_inv_deg, init_chroma_compress_params, init_gamut_compress_params,
    init_jmh_params, init_output_transform, init_shared_compression_params, init_tonescale_params,
    jmh_degrees_to_rgb, output_transform_fwd, output_transform_inv, rgb_to_jmh_degrees,
    tonescale_compress_fwd, tonescale_compress_inv,
};
use vfx_ocio::{
    FixedFunctionStyle, FixedFunctionTransform, GpuLanguage, GpuProcessor, GpuShaderCode,
    GpuUniformValue, Processor, Transform, TransformDirection,
};

// --- Universal CAM16 constants, mirrored from `aces2.rs` (they are `pub(crate)` there, so a test
// in an external crate restates them with a citation; the emitter itself sources them directly). ---
const RAD_PER_DEG: f32 = std::f32::consts::PI / 180.0;

// --- Test points (from OpenColorIO FixedFunctionOpCPU_tests.cpp:696, AP0). These deliberately
// leave the unit cube: RGB rows include HDR values up to ~3.8, and the JMh rows carry J up to 173
// and M up to 207 across hues spanning 4°..342° — the domain the LUT bake clamped into a corner. ---

/// Scene-linear AP0 RGB inputs for `RGB -> JMh`.
const RGB_INPUT: &[[f32; 3]] = &[
    [2.781_808_9, 0.179_178_25, -0.022_103_53], // ACEScg red x4 (HDR, negative B)
    [3.344_523_7, 3.617_862_7, -0.006002689],   // yellow x4
    [0.562_714_8, 3.438_684_5, 0.016_100_84],   // green x4
    [1.218_191, 3.820_821_8, 4.022_103_3],      // cyan x4
    [0.655_476_3, 0.382_137_27, 4.006_003],     // blue x4
    [3.437_285_2, 0.561_315_54, 3.983_899],     // magenta x4
    [0.11, 0.02, 0.04],
    [0.71, 0.51, 0.81],
    [0.43, 0.82, 0.71],
    [0.18, 0.18, 0.18], // grey
];

/// JMh (J, M, hue-degrees) inputs for `JMh -> RGB`, the OCIO-expected output of the rows above.
const JMH_INPUT: &[[f32; 3]] = &[
    [107.480_64, 206.827_3, 25.025_11],
    [173.194_08, 133.330_89, 106.183_45],
    [139.210_22, 191.922_36, 147.056_49],
    [157.905_17, 111.975_31, 192.204_73],
    [79.229_28, 100.424_66, 268.442_1],
    [132.888_14, 173.358_78, 341.715_24],
    [26.112_514, 42.523_605, 4.173_158],
    [79.190_46, 25.002_3, 332.159_76],
    [81.912_56, 39.754_81, 182.925_75],
    [33.924_664, 12.254_567, 38.146_66],
];

// --- OCIO relative/absolute comparison (port of EqualWithSafeRelError, min_expected = 1.0). ---
fn close(actual: f32, expected: f32, eps: f32) -> bool {
    if actual == expected {
        return true;
    }
    let div = expected.abs().max(1.0);
    ((actual - expected).abs() / div) <= eps
}

fn assert_close(label: &str, a: [f32; 3], e: [f32; 3], eps: f32) {
    for c in 0..3 {
        assert!(
            close(a[c], e[c], eps),
            "{label} chan {c}: gpu/emit={:.6} oracle={:.6} (eps={eps})",
            a[c],
            e[c]
        );
    }
}

/// AP0 primaries as the `params` vector the FixedFunction transform expects (r,g,b,w chromaticities).
fn ap0_params() -> Vec<f64> {
    vec![0.7347, 0.2653, 0.0, 1.0, 0.0001, -0.077, 0.32168, 0.33767]
}

/// The CPU oracle's `JMhParams` for AP0 — identical to what the processor builds from `ap0_params`.
fn oracle() -> JMhParams {
    init_jmh_params(&ACES_AP0)
}

/// Builds the generated shader for one direction. `forward` = `RGB -> JMh`.
fn shader(forward: bool, language: GpuLanguage) -> GpuShaderCode {
    let style = if forward {
        FixedFunctionStyle::AcesRgbToJmh20
    } else {
        FixedFunctionStyle::AcesJmhToRgb20
    };
    let transform = Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style,
        params: ap0_params(),
        direction: TransformDirection::Forward,
    });
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from ACES2 RGB<->JMh transform");
    GpuProcessor::from_processor(&processor)
        .expect("gpu processor")
        .generate_shader(language)
        .unwrap()
}

// ===========================================================================
// 1. naga parses the generated WGSL
// ===========================================================================

#[test]
fn naga_parses_generated_wgsl() {
    for forward in [true, false] {
        let code = shader(forward, GpuLanguage::Wgsl);
        let src = code.fragment_code();
        if let Err(e) = naga::front::wgsl::parse_str(src) {
            panic!(
                "naga rejected generated WGSL (forward={forward}):\n{}\n\n{src}",
                e.emit_to_string(src)
            );
        }
    }
}

// ===========================================================================
// 2. Live wgpu run of the emitted shader vs the CPU oracle
// ===========================================================================

/// Packs the shader's scalar uniforms into the tight `array<u32>` `ocio_scalars` storage buffer,
/// in registration order (the order `generate_shader` assigns `*_OFFSET`), each f32 bit-cast.
/// Array uniforms are skipped — they live in the separate `ocio_arrays_f` buffer ([`pack_arrays_f`])
/// and do not occupy scalar offsets.
fn pack_scalars(code: &GpuShaderCode) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut push = |f: f32| bytes.extend_from_slice(&f.to_le_bytes());
    for u in code.uniforms() {
        match &u.default_value {
            GpuUniformValue::Float(v) => push(*v),
            GpuUniformValue::Mat3(m) => m.iter().for_each(|&x| push(x)),
            GpuUniformValue::ArrayFloat(_) | GpuUniformValue::ArrayInt(_) => {}
            other => panic!("unexpected uniform kind: {other:?}"),
        }
    }
    bytes
}

/// Concatenates every `ArrayFloat` payload into the `ocio_arrays_f` storage buffer, in registration
/// order — the order that fixes each array's `<NAME>_BASE`. Empty when the shader has no float
/// arrays (the `RGB<->JMh` case), in which case no `ocio_arrays_f` binding is created.
fn pack_arrays_f(code: &GpuShaderCode) -> Vec<u8> {
    let mut bytes = Vec::new();
    for u in code.uniforms() {
        if let GpuUniformValue::ArrayFloat(v) = &u.default_value {
            v.iter()
                .for_each(|&x| bytes.extend_from_slice(&x.to_le_bytes()));
        }
    }
    bytes
}

fn align_up(v: u32, a: u32) -> u32 {
    v.div_ceil(a) * a
}

/// Runs the emitted compute shader `code` over `inputs`, returning one output triple per input, or
/// `None` if no suitable device is available (test skips rather than fails). Binds `ocio_scalars`
/// (group1/binding0) always and `ocio_arrays_f` (group1/binding2) when the shader declares float
/// arrays — the reach-M table the tonescale op samples.
fn run_on_gpu(code: &GpuShaderCode, inputs: &[[f32; 3]]) -> Option<Vec<[f32; 3]>> {
    pollster::block_on(async move {
        // ADOPT the cluster's single shared device (`gpu_mem::shared_device()`, crate gpu-info-rs)
        // instead of negotiating our own `Instance`/`request_adapter`/`request_device`. That rogue
        // negotiation was exactly the kind the rest of the cluster eliminated; the one process-wide
        // device is built once at the root with every stable feature (incl. FLOAT32_FILTERABLE) and
        // the adapter's max limits, so nothing needs requesting here. Skips cleanly (returns None,
        // not fails) when the machine has no GPU, mirroring the crate's other GPU gates.
        let gpu = gpu_mem::shared_device()?;
        let device = &gpu.device;
        let queue = &gpu.queue;

        // The generated shader samples `texture_2d<f32>` with a filtering sampler; the inverse
        // direction feeds J/M up to ~207, which needs f32 (not f16) input precision — so the input
        // texture is Rgba32Float and must be sampled as filterable.
        if !gpu
            .adapter
            .features()
            .contains(wgpu::Features::FLOAT32_FILTERABLE)
        {
            eprintln!("skipping live wgpu run: shared adapter lacks FLOAT32_FILTERABLE");
            return None;
        }

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aces2_jmh"),
            source: wgpu::ShaderSource::Wgsl(code.fragment_code().into()),
        });

        let n = inputs.len() as u32;

        // Input/output Rgba32Float textures, n x 1.
        let tex_size = wgpu::Extent3d {
            width: n,
            height: 1,
            depth_or_array_layers: 1,
        };
        let input_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("input"),
            size: tex_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let output_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("output"),
            size: tex_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        // Upload inputs as RGBA32F rows (alpha = 1).
        let mut input_bytes = Vec::with_capacity(inputs.len() * 16);
        for p in inputs {
            for &c in p {
                input_bytes.extend_from_slice(&c.to_le_bytes());
            }
            input_bytes.extend_from_slice(&1.0f32.to_le_bytes());
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &input_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &input_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(n * 16),
                rows_per_image: Some(1),
            },
            tex_size,
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());

        let scalar_bytes = pack_scalars(code);
        let scalar_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ocio_scalars"),
            size: scalar_bytes.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&scalar_buf, 0, &scalar_bytes);

        // Float arrays (the tonescale reach-M table) ride `ocio_arrays_f` at group1/binding2. Absent
        // for the RGB<->JMh shaders, so the buffer and its binding are created only when non-empty.
        let arrays_f_bytes = pack_arrays_f(code);
        let arrays_f_buf = if arrays_f_bytes.is_empty() {
            None
        } else {
            let b = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ocio_arrays_f"),
                size: arrays_f_bytes.len() as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&b, 0, &arrays_f_bytes);
            Some(b)
        };

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("aces2_jmh"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        // Bind groups from the pipeline's auto layout: group0 = {input, output, sampler},
        // group1 = {ocio_scalars}. Match generate_wgsl's binding numbers exactly.
        let input_view = input_tex.create_view(&Default::default());
        let output_view = output_tex.create_view(&Default::default());
        let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("g0"),
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
        let mut g1_entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: scalar_buf.as_entire_binding(),
        }];
        if let Some(b) = &arrays_f_buf {
            g1_entries.push(wgpu::BindGroupEntry {
                binding: 2,
                resource: b.as_entire_binding(),
            });
        }
        let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("g1"),
            layout: &pipeline.get_bind_group_layout(1),
            entries: &g1_entries,
        });

        // Read-back buffer for the output texture (bytes_per_row aligned to 256).
        let row_bytes = align_up(n * 16, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: row_bytes as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bg0, &[]);
            pass.set_bind_group(1, &bg1, &[]);
            // workgroup_size(8, 8); one workgroup covers n<=8 wide, 1 tall.
            pass.dispatch_workgroups(n.div_ceil(8), 1, 1);
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
                    rows_per_image: Some(1),
                },
            },
            tex_size,
        );
        queue.submit(std::iter::once(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().ok()?.ok()?;

        let data = slice.get_mapped_range().expect("get_mapped_range"); // wgpu 30
        let mut out = Vec::with_capacity(inputs.len());
        for i in 0..inputs.len() {
            let base = i * 16;
            let f = |o: usize| f32::from_le_bytes(data[base + o..base + o + 4].try_into().unwrap());
            out.push([f(0), f(4), f(8)]);
        }
        drop(data);
        readback.unmap();
        Some(out)
    })
}

/// Maps `(J, M, hue-degrees)` to `(J, M·cos h, M·sin h)`.
///
/// Hue is a polar angle and is undefined at zero chroma: for a neutral grey `M ≈ 0`, and the GPU's
/// `atan2` of two near-zero components lands on a different angle than the CPU's, a difference with
/// no perceptual meaning. Comparing the Cartesian chroma components instead makes the singularity
/// harmless (both `a` and `b` are ~0 there) while still pinning hue wherever chroma is non-trivial.
/// The exact hue expression is separately checked bit-for-bit by `cpu_equals_emit_rgb_to_jmh`.
fn jmh_cart(v: [f32; 3]) -> [f32; 3] {
    let hr = v[2] * RAD_PER_DEG;
    [v[0], v[1] * hr.cos(), v[1] * hr.sin()]
}

#[test]
fn live_wgpu_matches_oracle_rgb_to_jmh() {
    let Some(got) = run_on_gpu(&shader(true, GpuLanguage::Wgsl), RGB_INPUT) else {
        vfx_core::test_support::gpu_unavailable(
            "live_wgpu_matches_oracle_rgb_to_jmh",
            "no shared adapter with FLOAT32_FILTERABLE",
        );
        return;
    };
    let p = oracle();
    for (rgb, g) in RGB_INPUT.iter().zip(got) {
        let want = rgb_to_jmh_degrees(rgb, &p);
        // GPU transcendentals (pow/atan2) differ slightly from the CPU's; a relative bound of
        // 5e-3 is comfortably tighter than any LUT-corner artifact this replaces yet tolerant of
        // hardware math precision. Compared in Cartesian chroma so the zero-chroma hue singularity
        // (the grey row) does not register a spurious mismatch.
        assert_close(
            &format!("live rgb->jmh {rgb:?}"),
            jmh_cart(g),
            jmh_cart(want),
            5e-3,
        );
    }
}

#[test]
fn live_wgpu_matches_oracle_jmh_to_rgb() {
    let Some(got) = run_on_gpu(&shader(false, GpuLanguage::Wgsl), JMH_INPUT) else {
        vfx_core::test_support::gpu_unavailable(
            "live_wgpu_matches_oracle_jmh_to_rgb",
            "no shared adapter with FLOAT32_FILTERABLE",
        );
        return;
    };
    let p = oracle();
    for (jmh, g) in JMH_INPUT.iter().zip(got) {
        let want = jmh_degrees_to_rgb(jmh, &p);
        assert_close(&format!("live jmh->rgb {jmh:?}"), g, want, 5e-3);
    }
}

// ===========================================================================
// 3. ACES2 Tonescale + Chroma compress (stage 2)
// ===========================================================================
//
// Same checks (naga, live wgpu), keyed on the high-J domain the `[0,1]^3` bake
// could not represent. Inputs are the JMh the previous stage actually produces — `rgb_to_jmh` of the
// HDR RGB rows (J up to ~173) for the forward direction, and the forward tonescale of those (display
// JMh) for the inverse — so they are in-domain and NaN-free while far outside the unit cube. The
// forward outputs are pinned directly against the oracle, so a shared solver error cannot hide by
// cancelling in a fwd∘inv round-trip.

// Universal constants restated (they are `pub(crate)` in aces2.rs; an external test cites them).
/// Peak luminance the tonescale FixedFunction transform is built at (`params[0]`).
const TS_PEAK: f32 = 1000.0;

/// The five CPU param structs for the tonescale op at [`TS_PEAK`], built exactly as the processor
/// does from `AcesTonescaleCompress20` (AP0 input primaries, AP1 reach gamut).
fn ts_oracle() -> (
    JMhParams,
    ToneScaleParams,
    SharedCompressionParams,
    ChromaCompressParams,
) {
    let jmh = init_jmh_params(&ACES_AP0);
    let ts = init_tonescale_params(TS_PEAK);
    let reach = init_jmh_params(&ACES_AP1);
    let shared = init_shared_compression_params(TS_PEAK, &jmh, &reach);
    let chroma = init_chroma_compress_params(TS_PEAK, &ts);
    (jmh, ts, shared, chroma)
}

/// Builds the generated tonescale shader for one direction, guarding the fwd/inv wiring: the emitter
/// tags its block, so a direction mix-up fails loudly here instead of silently checking the wrong body.
fn ts_shader(forward: bool, language: GpuLanguage) -> GpuShaderCode {
    let transform = Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style: FixedFunctionStyle::AcesTonescaleCompress20,
        params: vec![TS_PEAK as f64],
        direction: if forward {
            TransformDirection::Forward
        } else {
            TransformDirection::Inverse
        },
    });
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from ACES2 tonescale transform");
    let code = GpuProcessor::from_processor(&processor)
        .expect("gpu processor")
        .generate_shader(language)
        .unwrap();
    let tag = if forward { "(fwd)" } else { "(inv)" };
    assert!(
        code.fragment_code().contains(tag),
        "tonescale shader missing {tag} body"
    );
    code
}

/// Forward JMh inputs: the JMh the RGB->JMh stage produces for the HDR RGB rows (J up to ~173).
fn ts_fwd_inputs(p: &JMhParams) -> Vec<[f32; 3]> {
    RGB_INPUT
        .iter()
        .map(|rgb| rgb_to_jmh_degrees(rgb, p))
        .collect()
}

/// Inverse JMh inputs: the forward tonescale of the forward inputs (display-side JMh, in the inverse
/// domain), so the inverse is exercised on values it will actually see.
fn ts_inv_inputs(
    p: &JMhParams,
    ts: &ToneScaleParams,
    shared: &SharedCompressionParams,
    chroma: &ChromaCompressParams,
) -> Vec<[f32; 3]> {
    ts_fwd_inputs(p)
        .iter()
        .map(|jmh| tonescale_compress_fwd(jmh, p, ts, shared, chroma))
        .collect()
}

#[test]
fn naga_parses_generated_tonescale_wgsl() {
    for forward in [true, false] {
        let code = ts_shader(forward, GpuLanguage::Wgsl);
        let src = code.fragment_code();
        if let Err(e) = naga::front::wgsl::parse_str(src) {
            panic!(
                "naga rejected generated tonescale WGSL (forward={forward}):\n{}\n\n{src}",
                e.emit_to_string(src)
            );
        }
    }
}

#[test]
fn live_wgpu_matches_oracle_tonescale_fwd() {
    let (p, ts, shared, chroma) = ts_oracle();
    let inputs = ts_fwd_inputs(&p);
    let Some(got) = run_on_gpu(&ts_shader(true, GpuLanguage::Wgsl), &inputs) else {
        vfx_core::test_support::gpu_unavailable(
            "live_wgpu_matches_oracle_tonescale_fwd",
            "no shared adapter with FLOAT32_FILTERABLE",
        );
        return;
    };
    for (jmh, g) in inputs.iter().zip(got) {
        let want = tonescale_compress_fwd(jmh, &p, &ts, &shared, &chroma);
        assert_close(&format!("live tonescale fwd {jmh:?}"), g, want, 5e-3);
    }
}

#[test]
fn live_wgpu_matches_oracle_tonescale_inv() {
    let (p, ts, shared, chroma) = ts_oracle();
    let inputs = ts_inv_inputs(&p, &ts, &shared, &chroma);
    let Some(got) = run_on_gpu(&ts_shader(false, GpuLanguage::Wgsl), &inputs) else {
        vfx_core::test_support::gpu_unavailable(
            "live_wgpu_matches_oracle_tonescale_inv",
            "no shared adapter with FLOAT32_FILTERABLE",
        );
        return;
    };
    for (jmh, g) in inputs.iter().zip(got) {
        let want = tonescale_compress_inv(jmh, &p, &ts, &shared, &chroma);
        assert_close(&format!("live tonescale inv {jmh:?}"), g, want, 5e-3);
    }
}

// ===========================================================================
// 4. ACES2 Gamut compress (stage 3)
// ===========================================================================
//
// Same checks (naga, live wgpu), keyed on the high-J / cusp-adjacent domain the
// `[0,1]^3` bake could not represent. Inputs are the JMh the pipeline actually feeds the op —
// tonescale-compressed display JMh (forward) and the gamut-forward of those (inverse) — so they are
// in-domain and NaN-free.

// Universal gamut constants restated (they are `pub(crate)` in aces2.rs; an external test cites them).
/// Peak luminance the gamut FixedFunction transform is built at (`params[0]`).
const GC_PEAK: f32 = 1000.0;

/// Rec.709 limiting primaries used for the gamut op (`params[1..9]`), matching [`gc_oracle`].
fn gc_primaries() -> Primaries {
    Primaries {
        red: Chromaticity { x: 0.64, y: 0.33 },
        green: Chromaticity { x: 0.30, y: 0.60 },
        blue: Chromaticity { x: 0.15, y: 0.06 },
        white: Chromaticity {
            x: 0.3127,
            y: 0.3290,
        },
    }
}

/// Transform params for the gamut op: peak + Rec.709 primary chromaticities.
fn gc_params() -> Vec<f64> {
    vec![
        GC_PEAK as f64,
        0.64,
        0.33,
        0.30,
        0.60,
        0.15,
        0.06,
        0.3127,
        0.3290,
    ]
}

/// The CPU `SharedCompressionParams` + `GamutCompressParams` the processor builds for the gamut op at
/// [`GC_PEAK`] with Rec.709 limiting primaries — identical to `fixed_function_to_aces2_op`.
fn gc_oracle() -> (SharedCompressionParams, GamutCompressParams) {
    let p_in = init_jmh_params(&ACES_AP0);
    let p_lim = init_jmh_params(&gc_primaries());
    let ts = init_tonescale_params(GC_PEAK);
    let reach = init_jmh_params(&ACES_AP1);
    let shared = init_shared_compression_params(GC_PEAK, &p_in, &reach);
    let gamut = init_gamut_compress_params(GC_PEAK, &p_in, &p_lim, &ts, &shared, &reach);
    (shared, gamut)
}

/// Builds the generated gamut shader for one direction, guarding the fwd/inv wiring via the block tag.
fn gc_shader(forward: bool, language: GpuLanguage) -> GpuShaderCode {
    let transform = Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style: FixedFunctionStyle::AcesGamutCompress20,
        params: gc_params(),
        direction: if forward {
            TransformDirection::Forward
        } else {
            TransformDirection::Inverse
        },
    });
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from ACES2 gamut transform");
    let code = GpuProcessor::from_processor(&processor)
        .expect("gpu processor")
        .generate_shader(language)
        .unwrap();
    let tag = if forward { "(fwd)" } else { "(inv)" };
    assert!(
        code.fragment_code().contains(tag),
        "gamut shader missing {tag} body"
    );
    code
}

/// Forward gamut inputs: the display JMh the gamut op sees in the pipeline — the tonescale-compressed
/// JMh of the HDR RGB rows (J up to the display peak, hues spanning the circle).
fn gc_fwd_inputs() -> Vec<[f32; 3]> {
    let jmh = init_jmh_params(&ACES_AP0);
    let ts = init_tonescale_params(GC_PEAK);
    let reach = init_jmh_params(&ACES_AP1);
    let shared = init_shared_compression_params(GC_PEAK, &jmh, &reach);
    let chroma = init_chroma_compress_params(GC_PEAK, &ts);
    RGB_INPUT
        .iter()
        .map(|rgb| {
            let j = rgb_to_jmh_degrees(rgb, &jmh);
            tonescale_compress_fwd(&j, &jmh, &ts, &shared, &chroma)
        })
        .collect()
}

/// Inverse gamut inputs: the gamut-forward of the forward inputs, i.e. the values the inverse sees.
fn gc_inv_inputs(shared: &SharedCompressionParams, gamut: &GamutCompressParams) -> Vec<[f32; 3]> {
    gc_fwd_inputs()
        .iter()
        .map(|jmh| gamut_compress_fwd_deg(jmh, shared, gamut))
        .collect()
}

#[test]
fn naga_parses_generated_gamut_wgsl() {
    for forward in [true, false] {
        let code = gc_shader(forward, GpuLanguage::Wgsl);
        let src = code.fragment_code();
        if let Err(e) = naga::front::wgsl::parse_str(src) {
            panic!(
                "naga rejected generated gamut WGSL (forward={forward}):\n{}\n\n{src}",
                e.emit_to_string(src)
            );
        }
    }
}

#[test]
fn live_wgpu_matches_oracle_gamut_fwd() {
    let (shared, gamut) = gc_oracle();
    let inputs = gc_fwd_inputs();
    let Some(got) = run_on_gpu(&gc_shader(true, GpuLanguage::Wgsl), &inputs) else {
        vfx_core::test_support::gpu_unavailable(
            "live_wgpu_matches_oracle_gamut_fwd",
            "no shared adapter with FLOAT32_FILTERABLE",
        );
        return;
    };
    for (jmh, g) in inputs.iter().zip(got) {
        let want = gamut_compress_fwd_deg(jmh, &shared, &gamut);
        assert_close(&format!("live gamut fwd {jmh:?}"), g, want, 5e-3);
    }
}

#[test]
fn live_wgpu_matches_oracle_gamut_inv() {
    let (shared, gamut) = gc_oracle();
    let inputs = gc_inv_inputs(&shared, &gamut);
    let Some(got) = run_on_gpu(&gc_shader(false, GpuLanguage::Wgsl), &inputs) else {
        vfx_core::test_support::gpu_unavailable(
            "live_wgpu_matches_oracle_gamut_inv",
            "no shared adapter with FLOAT32_FILTERABLE",
        );
        return;
    };
    for (jmh, g) in inputs.iter().zip(got) {
        let want = gamut_compress_inv_deg(jmh, &shared, &gamut);
        assert_close(&format!("live gamut inv {jmh:?}"), g, want, 5e-3);
    }
}

// ===========================================================================
// 5. ACES2 Output Transform (stage 4) — the full scene<->display composition
// ===========================================================================
//
// This is the direction that CRUSHED HIGHLIGHTS under the old `[0,1]^3` bake: the forward output
// transform was sampled over the unit cube with no shaper, so every scene-linear value > 1.0 clamped
// at the cube's far face and HDR highlights flattened. The op is now emitted in-shader as the
// composition of the three verified stages (RGB<->JMh, tonescale+chroma, gamut), so it carries the
// unbounded scene-linear domain end to end.
//
// The emitted body is literally stage 1 -> 2 -> 3 -> 1 (with the two RGB<->JMh ends using different
// primaries), as the shader reuses the stage emitters. Inputs are HDR
// scene-linear RGB with channels up to 8.0 (well above the old clamp), and the forward output is
// pinned ABSOLUTELY against `output_transform_fwd`, which is where the bake was wrong.

/// Peak luminance the Output Transform is built at (`params[0]`).
const OT_PEAK: f32 = 1000.0;

/// Rec.709 limiting primaries for the Output Transform display end (`params[1..9]`).
fn ot_primaries() -> Primaries {
    Primaries {
        red: Chromaticity { x: 0.64, y: 0.33 },
        green: Chromaticity { x: 0.30, y: 0.60 },
        blue: Chromaticity { x: 0.15, y: 0.06 },
        white: Chromaticity {
            x: 0.3127,
            y: 0.3290,
        },
    }
}

/// Transform params for the Output Transform: peak + Rec.709 display primary chromaticities.
fn ot_params() -> Vec<f64> {
    vec![
        OT_PEAK as f64,
        0.64,
        0.33,
        0.30,
        0.60,
        0.15,
        0.06,
        0.3127,
        0.3290,
    ]
}

/// The CPU `Aces2State` the processor builds for the Output Transform at [`OT_PEAK`] with Rec.709
/// limiting primaries — identical to what `fixed_function_to_aces2_op` constructs.
fn ot_oracle() -> Aces2State {
    init_output_transform(OT_PEAK, &ot_primaries())
}

/// HDR scene-linear AP0 RGB inputs for the forward Output Transform: positive highlights with
/// channels up to 8.0 — the domain the `[0,1]^3` bake clamped and flattened. A neutral grey and a
/// mid-tone anchor the low end.
const OT_HDR_RGB: &[[f32; 3]] = &[
    [4.0, 0.25, 0.06],
    [8.0, 6.0, 0.10],
    [0.55, 7.0, 3.0],
    [1.8, 2.2, 8.0],
    [6.5, 0.5, 6.0],
    [3.0, 3.0, 3.0],
    [0.9, 0.75, 0.55],
    [0.18, 0.18, 0.18],
];

/// Builds the generated Output Transform shader for one direction, guarding the fwd/inv wiring: the
/// composed body carries its stages' `(fwd)`/`(inv)` tags, so a direction mix-up fails loudly here.
fn ot_shader(forward: bool, language: GpuLanguage) -> GpuShaderCode {
    let transform = Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style: FixedFunctionStyle::AcesOutputTransform20,
        params: ot_params(),
        direction: if forward {
            TransformDirection::Forward
        } else {
            TransformDirection::Inverse
        },
    });
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from ACES2 output-transform");
    let code = GpuProcessor::from_processor(&processor)
        .expect("gpu processor")
        .generate_shader(language)
        .unwrap();
    // Forward composes tonescale(fwd)+gamut(fwd); inverse composes gamut(inv)+tonescale(inv).
    let (ts_tag, gc_tag) = if forward {
        ("tonescale + chroma compress (fwd)", "gamut compress (fwd)")
    } else {
        ("tonescale + chroma compress (inv)", "gamut compress (inv)")
    };
    let src = code.fragment_code();
    assert!(src.contains(ts_tag), "output shader missing `{ts_tag}`");
    assert!(src.contains(gc_tag), "output shader missing `{gc_tag}`");
    code
}

#[test]
fn naga_parses_generated_output_transform_wgsl() {
    for forward in [true, false] {
        let code = ot_shader(forward, GpuLanguage::Wgsl);
        let src = code.fragment_code();
        if let Err(e) = naga::front::wgsl::parse_str(src) {
            panic!(
                "naga rejected generated output-transform WGSL (forward={forward}):\n{}\n\n{src}",
                e.emit_to_string(src)
            );
        }
    }
}

#[test]
fn live_wgpu_matches_oracle_output_transform_fwd() {
    let st = ot_oracle();
    let inputs = OT_HDR_RGB.to_vec();
    // The rows must exercise what the old `[0,1]^3` bake got wrong: the transform of an HDR row
    // differs from the transform of the row clamped to the unit cube.
    let discriminating = inputs
        .iter()
        .filter(|rgb| rgb.iter().any(|&c| c > 1.0))
        .filter(|rgb| {
            let clamped = rgb.map(|c| c.clamp(0.0, 1.0));
            let (a, b) = (
                output_transform_fwd(rgb, &st),
                output_transform_fwd(&clamped, &st),
            );
            (0..3).any(|c| (a[c] - b[c]).abs() > 0.02)
        })
        .count();
    assert!(
        discriminating >= 4,
        "too few HDR rows exercise the highlight fix"
    );
    let Some(got) = run_on_gpu(&ot_shader(true, GpuLanguage::Wgsl), &inputs) else {
        vfx_core::test_support::gpu_unavailable(
            "live_wgpu_matches_oracle_output_transform_fwd",
            "no shared adapter with FLOAT32_FILTERABLE",
        );
        return;
    };
    for (rgb, g) in inputs.iter().zip(got) {
        let want = output_transform_fwd(rgb, &st);
        assert_close(&format!("live output fwd {rgb:?}"), g, want, 5e-3);
    }
}

#[test]
fn live_wgpu_matches_oracle_output_transform_inv() {
    let st = ot_oracle();
    let inputs: Vec<[f32; 3]> = OT_HDR_RGB
        .iter()
        .map(|rgb| output_transform_fwd(rgb, &st))
        .collect();
    let Some(got) = run_on_gpu(&ot_shader(false, GpuLanguage::Wgsl), &inputs) else {
        vfx_core::test_support::gpu_unavailable(
            "live_wgpu_matches_oracle_output_transform_inv",
            "no shared adapter with FLOAT32_FILTERABLE",
        );
        return;
    };
    for (rgb, g) in inputs.iter().zip(got) {
        let want = output_transform_inv(rgb, &st);
        assert_close(&format!("live output inv {rgb:?}"), g, want, 5e-3);
    }
}

// ===========================================================================
// Consistency: every ACES2 op is WGSL-native (ACES2-03 stage 5)
// ===========================================================================
//
// All four ACES2 ops emit real in-shader WGSL, so the STRICT `from_processor_wgsl`
// path — which errors on any non-native op instead of falling back to a bake — must
// accept a *standalone* one of each, not only the composed OutputTransform. A rejection
// here means an op emits native WGSL yet is missing from `is_wgsl_native_op`, the exact
// inconsistency this stage closes. The emitted body is additionally naga-parsed so
// "native" means "parses as WGSL", not merely "the list contains the variant".

/// Builds a single-op processor for `style`/`params`, asserts it survives the strict
/// (native-only) `from_processor_wgsl` path, and that the emitted WGSL body parses under naga.
fn assert_wgsl_native(label: &str, style: FixedFunctionStyle, params: Vec<f64>) {
    let transform = Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style,
        params,
        direction: TransformDirection::Forward,
    });
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .unwrap_or_else(|e| panic!("{label}: processor build failed: {e}"));
    let gpu = GpuProcessor::from_processor_wgsl(&processor)
        .unwrap_or_else(|e| panic!("{label}: from_processor_wgsl rejected a native ACES2 op: {e}"));
    let code = gpu.generate_shader(GpuLanguage::Wgsl).unwrap();
    let src = code.fragment_code();
    assert!(
        !src.trim().is_empty(),
        "{label}: WGSL-native processor emitted an empty body"
    );
    if let Err(e) = naga::front::wgsl::parse_str(src) {
        panic!(
            "{label}: naga rejected WGSL from the strict path:\n{}\n\n{src}",
            e.emit_to_string(src)
        );
    }
}

/// The four ACES2 GpuOp variants (five styles — `RGB<->JMh` has one per direction) all round-trip
/// through the strict WGSL path as native ops, matching their in-shader emission.
#[test]
fn all_aces2_ops_are_wgsl_native() {
    assert_wgsl_native(
        "Aces2RgbJmh (RgbToJmh)",
        FixedFunctionStyle::AcesRgbToJmh20,
        ap0_params(),
    );
    assert_wgsl_native(
        "Aces2RgbJmh (JmhToRgb)",
        FixedFunctionStyle::AcesJmhToRgb20,
        ap0_params(),
    );
    assert_wgsl_native(
        "Aces2TonescaleCompress",
        FixedFunctionStyle::AcesTonescaleCompress20,
        vec![TS_PEAK as f64],
    );
    assert_wgsl_native(
        "Aces2GamutCompress",
        FixedFunctionStyle::AcesGamutCompress20,
        gc_params(),
    );
    assert_wgsl_native(
        "Aces2OutputTransform",
        FixedFunctionStyle::AcesOutputTransform20,
        ot_params(),
    );
}
