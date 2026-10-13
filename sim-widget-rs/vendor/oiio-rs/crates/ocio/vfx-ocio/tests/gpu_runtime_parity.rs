//! GPU==CPU parity for the `gpu`-feature colour-processing runtime (`vfx_ocio::apply_gpu`).
//!
//! Proves the runtime actually EXECUTES a transform correctly (not just that it compiles a shader):
//! for a representative `Processor` it runs the SAME transform on the CPU (`Processor::apply_rgba`,
//! the trusted oracle) and on the GPU (`apply_gpu`, which emits the WGSL and runs it on the shared
//! device) over a small synthetic image, and asserts the two agree within tolerance.
//!
//! Two transforms:
//! - a non-symmetric colour **Matrix** (WGSL matrix emission is the documented-correct side vs CPU),
//!   compared to ~f32 precision;
//! - the **ACES 2.0 `RGB -> JMh`** FixedFunction (a real ACES transform already CPU-verified by
//!   `aces2_gpu_emission.rs`), exercising the Mat3/float scalar-uniform upload path end to end, on
//!   HDR non-grey inputs (hue is well defined) with the same relative bound the live-wgpu emission
//!   test uses.
//!
//! Gated on `feature = "gpu"` (the only config where `apply_gpu` exists); compiles to nothing
//! otherwise. Requires a GPU — a real RTX 3080 Ti is present, so this asserts the shared device is
//! available rather than skipping.
#![cfg(feature = "gpu")]

use vfx_ocio::{
    BuiltinTransferTransform, FixedFunctionStyle, FixedFunctionTransform, GpuBufferExecutor,
    GpuTextureExecutor, GradingPrimary, GradingPrimaryStyle, GradingPrimaryTransform, GradingRgbm,
    Interpolation, Lut1DTransform, Lut3DTransform, Processor, Transform, TransformDirection,
    apply_gpu, apply_gpu_texture,
};

/// Whether the shared GPU device exists. Without one, `vfx_core::test_support::gpu_unavailable`
/// prints a SKIPPED line (or fails the test under `VFX_REQUIRE_GPU=1`) and the caller returns;
/// the tests below then `expect` success, because with a device a GPU error is a real failure.
fn have_gpu(test: &str) -> bool {
    let present = gpu_mem::shared_device().is_some();
    if !present {
        vfx_core::test_support::gpu_unavailable(test, "gpu_mem::shared_device() is None");
    }
    present
}

/// OCIO relative/absolute closeness (port of EqualWithSafeRelError, min_expected = 1.0), matching
/// `aces2_gpu_emission.rs`.
fn close(actual: f32, expected: f32, eps: f32) -> bool {
    if actual == expected {
        return true;
    }
    let div = expected.abs().max(1.0);
    ((actual - expected).abs() / div) <= eps
}

/// Max relative error across all channels of all pixels, for reporting.
fn max_rel_err(got: &[[f32; 4]], want: &[[f32; 4]], channels: usize) -> f32 {
    let mut m = 0.0f32;
    for (g, w) in got.iter().zip(want) {
        for c in 0..channels {
            let div = w[c].abs().max(1.0);
            m = m.max((g[c] - w[c]).abs() / div);
        }
    }
    m
}

/// With a GPU present `apply_gpu` MUST succeed (no GPU: SKIPPED, or a failure under
/// `VFX_REQUIRE_GPU=1`, via `have_gpu`). Also pins
/// that repeated `shared_device()` calls return the SAME `&'static` context — i.e. the runtime
/// reuses one device, never a second negotiation.
#[test]
fn shared_device_present_and_singleton() {
    if !have_gpu("shared_device_present_and_singleton") {
        return;
    }
    let a =
        gpu_mem::shared_device().expect("shared GPU device must be present (real GPU expected)");
    let b = gpu_mem::shared_device().expect("shared GPU device must be present on second call");
    assert!(
        std::ptr::eq(a, b),
        "shared_device() returned two different contexts — the device is not being reused"
    );
}

/// Non-symmetric colour matrix (a transpose bug would change the result) with an identity alpha row.
const TEST_MATRIX: [f64; 16] = [
    0.6, 0.3, 0.1, 0.0, //
    0.2, 0.7, 0.1, 0.0, //
    0.1, 0.2, 0.7, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// A 3x2 synthetic image of varied, non-neutral RGBA pixels.
///
/// PLAN9.md O2: domain widened to roughly `[-1, 8]` per channel (was `[0.02, 1.5]`, entirely
/// positive) - a CPU oracle that only agrees with the shader inside `[0.02, 1.5]` proves nothing
/// for ACEScg, which legitimately carries negative components and values well above 1.
fn synthetic_image() -> Vec<[f32; 4]> {
    vec![
        [0.10, 0.20, 0.30, 1.0],
        [0.90, -0.10, 0.40, 0.5],
        [0.25, 0.75, -0.55, 1.0],
        [1.50, -1.00, 0.60, 0.8],
        [8.00, 0.66, 0.99, 1.0],
        [-0.02, 0.48, 4.00, 0.25],
    ]
}

#[test]
fn matrix_gpu_equals_cpu() {
    if !have_gpu("matrix_gpu_equals_cpu") {
        return;
    }
    assert!(
        gpu_mem::shared_device().is_some(),
        "shared GPU device must be present"
    );

    let (w, h) = (3u32, 2u32);
    let input = synthetic_image();

    let transform = Transform::matrix(TEST_MATRIX);
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from matrix transform");

    // CPU oracle: apply in place.
    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);

    // GPU runtime.
    let gpu = apply_gpu(&processor, w, h, &input).expect("apply_gpu (matrix) must succeed");
    assert_eq!(gpu.len(), cpu.len(), "pixel count mismatch");

    // Matrix maths is identical on both sides bar f32 rounding; a tight bound catches a transpose or
    // an upload/packing error.
    let eps = 1e-4;
    for (i, (g, c)) in gpu.iter().zip(&cpu).enumerate() {
        for ch in 0..4 {
            assert!(
                close(g[ch], c[ch], eps),
                "matrix pixel {i} chan {ch}: gpu={:.6} cpu={:.6} (eps={eps})",
                g[ch],
                c[ch]
            );
        }
    }
    eprintln!(
        "matrix_gpu_equals_cpu: max relative error = {:.3e}",
        max_rel_err(&gpu, &cpu, 4)
    );
}

/// Flatten `[[f32; 4]]` pixels to the interleaved RGBA-f32 slice `GpuImage::upload_rgba_f32` wants.
fn flatten(pixels: &[[f32; 4]]) -> Vec<f32> {
    pixels.iter().flat_map(|p| p.iter().copied()).collect()
}

/// Three-way parity for the ZERO-COPY texture-native path (`apply_gpu_texture`):
///   (1) CPU oracle  `Processor::apply_rgba`,
///   (2) buffer GPU  `apply_gpu` (D1, host RGBA-f32 round-trip, rgba32float storage),
///   (3) texture GPU `apply_gpu_texture` (this slice: GpuImage in/out, rgba16float, no host round-trip).
///
/// Asserts (2)≈(1) and (3)≈(1), and (3)≈(2). Because (3) stores through the canonical `Rgba16Float`
/// `GpuImage` at BOTH the upload (input) and the store (output) boundary, its only extra error over
/// the f32 buffer path is f16 quantization (~2.4e-4 for values in [0,1], a little more for the 1.5
/// HDR sample here) plus the matrix's own channel mixing — so (3) uses a looser bound than the
/// ~f32-tight (2). The transform is the same non-symmetric colour Matrix `matrix_gpu_equals_cpu`
/// uses (a transpose or packing bug would move the result).
#[test]
fn texture_gpu_three_way_parity() {
    if !have_gpu("texture_gpu_three_way_parity") {
        return;
    }
    assert!(
        gpu_mem::shared_device().is_some(),
        "shared GPU device must be present"
    );

    let (w, h) = (3u32, 2u32);
    let input = synthetic_image();

    let transform = Transform::matrix(TEST_MATRIX);
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from matrix transform");

    // (1) CPU oracle.
    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);

    // (2) buffer GPU (D1).
    let buf = apply_gpu(&processor, w, h, &input).expect("apply_gpu (matrix) must succeed");
    assert_eq!(buf.len(), cpu.len(), "buffer pixel count mismatch");

    // (3) texture GPU (this slice): host upload -> GpuImage -> apply_gpu_texture -> readback. The
    // only host transfers are these boundary calls; apply_gpu_texture itself does NO host round-trip.
    let src_img = gpu_mem::GpuImage::upload_rgba_f32(w, h, &flatten(&input)).expect("upload input");
    let out_img = apply_gpu_texture(&processor, &src_img).expect("apply_gpu_texture must succeed");
    assert_eq!((out_img.width(), out_img.height()), (w, h), "output dims");
    let tex_flat = out_img.read_rgba_f32().expect("readback output");
    assert_eq!(
        tex_flat.len(),
        cpu.len() * 4,
        "texture pixel count mismatch"
    );
    let tex: Vec<[f32; 4]> = tex_flat
        .as_chunks::<4>().0.iter()
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();

    // (2)≈(1): matrix maths is f32 on both sides; a tight bound catches transpose/packing errors.
    let eps_buf = 1e-4;
    // (3)≈(1) and (3)≈(2): f16 storage at the GpuImage upload+store boundaries dominates the delta.
    let eps_tex = 2e-3;
    for (i, ((b, c), t)) in buf.iter().zip(&cpu).zip(&tex).enumerate() {
        for ch in 0..4 {
            assert!(
                close(b[ch], c[ch], eps_buf),
                "buffer pixel {i} chan {ch}: gpu={:.6} cpu={:.6} (eps={eps_buf})",
                b[ch],
                c[ch]
            );
            assert!(
                close(t[ch], c[ch], eps_tex),
                "texture pixel {i} chan {ch}: gpu={:.6} cpu={:.6} (eps={eps_tex})",
                t[ch],
                c[ch]
            );
            assert!(
                close(t[ch], b[ch], eps_tex),
                "texture-vs-buffer pixel {i} chan {ch}: tex={:.6} buf={:.6} (eps={eps_tex})",
                t[ch],
                b[ch]
            );
        }
    }
    eprintln!(
        "texture_gpu_three_way_parity: max rel err (3)vs(1) = {:.3e}, (3)vs(2) = {:.3e}, (2)vs(1) = {:.3e}",
        max_rel_err(&tex, &cpu, 4),
        max_rel_err(&tex, &buf, 4),
        max_rel_err(&buf, &cpu, 4)
    );
}

/// AP0 primaries as the FixedFunction `params` vector (r,g,b,w chromaticities) — matches
/// `aces2_gpu_emission.rs::ap0_params`.
fn ap0_params() -> Vec<f64> {
    vec![0.7347, 0.2653, 0.0, 1.0, 0.0001, -0.077, 0.32168, 0.33767]
}

#[test]
fn aces2_rgb_to_jmh_gpu_equals_cpu() {
    if !have_gpu("aces2_rgb_to_jmh_gpu_equals_cpu") {
        return;
    }
    assert!(
        gpu_mem::shared_device().is_some(),
        "shared GPU device must be present"
    );

    // HDR, non-neutral AP0 rows — hue is well defined (no zero-chroma singularity), and J/M leave
    // the unit cube, the domain the old LUT bake could not represent.
    #[expect(
        clippy::excessive_precision,
        reason = "probe rows keep the nine digits they were chosen with; f32 rounds them"
    )]
    let input: Vec<[f32; 4]> = vec![
        [2.781808965, 0.179178253, -0.022103530, 1.0],
        [3.344523751, 3.617862727, -0.006002689, 1.0],
        [0.562714786, 3.438684474, 0.016100841, 1.0],
        [0.710000000, 0.510000000, 0.810000000, 1.0],
    ];
    let (w, h) = (input.len() as u32, 1u32);

    let transform = Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style: FixedFunctionStyle::AcesRgbToJmh20,
        params: ap0_params(),
        direction: TransformDirection::Forward,
    });
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from ACES2 RGB->JMh transform");

    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);

    let gpu = apply_gpu(&processor, w, h, &input).expect("apply_gpu (ACES2 RGB->JMh) must succeed");
    assert_eq!(gpu.len(), cpu.len(), "pixel count mismatch");

    // GPU transcendentals differ slightly from the CPU's; 5e-3 relative is the same bound the
    // live-wgpu emission test uses, comfortably tighter than any LUT-corner artifact this replaces.
    let eps = 5e-3;
    for (i, (g, c)) in gpu.iter().zip(&cpu).enumerate() {
        for ch in 0..3 {
            assert!(
                close(g[ch], c[ch], eps),
                "aces2 rgb->jmh pixel {i} chan {ch}: gpu={:.6} cpu={:.6} (eps={eps})",
                g[ch],
                c[ch]
            );
        }
    }
    eprintln!(
        "aces2_rgb_to_jmh_gpu_equals_cpu: max relative error (JMh) = {:.3e}",
        max_rel_err(&gpu, &cpu, 3)
    );
}

/// PLAN9.md O2: the sRGB curve mirrors negatives (`sign(x) * f(|x|)`) on the CPU oracle
/// (`vfx_transfer::srgb`); the GPU emitter used to select its branch on the raw signed value, which
/// is wrong past the toe break (`-0.5` gave `-6.46` on GPU against the oracle's `-0.735` - an 8x
/// divergence, see `gpu/transfer.rs`'s `toe_power` history). This runs the REAL runtime
/// (`apply_gpu`), not just the text emitter, so it would fail again if either side started clamping
/// or the mirror regressed.
#[test]
fn transfer_srgb_gpu_equals_cpu_negatives_and_hdr() {
    if !have_gpu("transfer_srgb_gpu_equals_cpu_negatives_and_hdr") {
        return;
    }
    assert!(
        gpu_mem::shared_device().is_some(),
        "shared GPU device must be present"
    );

    let (w, h) = (3u32, 2u32);
    let input = synthetic_image();

    let transform = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "srgb".to_string(),
        direction: TransformDirection::Forward,
    });
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from sRGB builtin transfer");

    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);

    let gpu = apply_gpu(&processor, w, h, &input).expect("apply_gpu (sRGB transfer) must succeed");
    assert_eq!(gpu.len(), cpu.len(), "pixel count mismatch");

    let eps = 1e-4;
    for (i, (g, c)) in gpu.iter().zip(&cpu).enumerate() {
        for ch in 0..3 {
            assert!(
                close(g[ch], c[ch], eps),
                "srgb transfer pixel {i} chan {ch}: gpu={:.6} cpu={:.6} (eps={eps})",
                g[ch],
                c[ch]
            );
        }
    }
    eprintln!(
        "transfer_srgb_gpu_equals_cpu_negatives_and_hdr: max relative error = {:.3e}",
        max_rel_err(&gpu, &cpu, 3)
    );
}

/// ST 2084 PQ (`BuiltinTransfer` "pq", absolute cd/m2) on the real runtime, both directions, at
/// the domain edges (ROADMAP G8). The CPU owner `vfx_transfer::pq` maps luminance `<= 0` to exactly
/// 0 (the bare rational gives `c1^m2 ~ 7.4e-7`), clamps luminance above 10000, and clamps the
/// signal to `[0, 1]`: past `(c2/c3)^m2 ~ 1.98` an unclamped decode is NaN. The GPU used to encode
/// 0 as 7.4e-7 and floor the decode's denominator instead of clamping the signal.
#[test]
fn transfer_pq_gpu_equals_cpu_at_the_domain_edges() {
    if !have_gpu("transfer_pq_gpu_equals_cpu_at_the_domain_edges") {
        return;
    }
    let run = |direction: TransformDirection, values: &[f32], eps: f32| {
        let input: Vec<[f32; 4]> = values.chunks(3).map(|c| [c[0], c[1], c[2], 1.0]).collect();
        let transform = Transform::BuiltinTransfer(BuiltinTransferTransform {
            style: "pq".to_string(),
            direction,
        });
        let processor =
            Processor::from_transform(&transform, TransformDirection::Forward).expect("processor");
        let mut cpu = input.clone();
        processor.apply_rgba(&mut cpu);
        let gpu = apply_gpu(&processor, input.len() as u32, 1, &input).expect("apply_gpu (PQ)");
        for (i, (g, c)) in gpu.iter().zip(&cpu).enumerate() {
            for ch in 0..3 {
                assert!(
                    c[ch].is_finite() && close(g[ch], c[ch], eps),
                    "pq {direction:?} pixel {i} ({:?}) chan {ch}: gpu={} cpu={}",
                    input[i],
                    g[ch],
                    c[ch]
                );
            }
        }
    };
    // Encode: nits -> signal. Exactly 0 at and below 0 on both sides, 1 above 10000.
    run(
        TransformDirection::Forward,
        &[
            -50.0, -0.0, 0.0, 0.001, 0.1, 100.0, 1000.0, 10000.0, 25000.0,
        ],
        2e-5,
    );
    // Decode: signal -> nits. 1.5 and 3.0 are out of range (3.0 is past the pole): 10000.
    run(
        TransformDirection::Inverse,
        &[-0.5, 0.0, 1e-4, 0.1, 0.508, 0.75, 1.0, 1.5, 3.0],
        1e-4,
    );
    let mut cpu = [[0.0, 0.0, 3.0, 1.0f32]];
    let decode = Processor::from_transform(
        &Transform::BuiltinTransfer(BuiltinTransferTransform {
            style: "pq".to_string(),
            direction: TransformDirection::Inverse,
        }),
        TransformDirection::Forward,
    )
    .expect("processor");
    decode.apply_rgba(&mut cpu);
    assert_eq!(
        cpu[0][..3],
        [0.0, 0.0, 10000.0],
        "CPU decode clamps the signal to [0, 1]"
    );
}

/// PLAN9.md O0 "done when": a compiled executor produces the SAME result as the one-shot
/// `apply_gpu`/`apply_gpu_texture` convenience wrappers, across MULTIPLE `execute` calls with
/// DIFFERENT inputs each time - not just "compiles once", but genuinely reusable without the
/// pipeline/bind-group state going stale or a buffer being freed out from under `bg1` between
/// calls (the risk `CompiledOcio::compile`'s doc comment calls out explicitly).
#[test]
fn executor_reuse_matches_one_shot_and_stays_correct_across_calls() {
    if !have_gpu("executor_reuse_matches_one_shot_and_stays_correct_across_calls") {
        return;
    }
    assert!(
        gpu_mem::shared_device().is_some(),
        "shared GPU device must be present"
    );

    let transform = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "srgb".to_string(),
        direction: TransformDirection::Forward,
    });
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from sRGB builtin transfer");

    let (w, h) = (3u32, 2u32);
    let inputs: [Vec<[f32; 4]>; 3] = [
        synthetic_image(),
        synthetic_image()
            .iter()
            .map(|p| [p[0] * 0.5, p[1] * 0.5, p[2] * 0.5, p[3]])
            .collect(),
        synthetic_image()
            .iter()
            .map(|p| [p[0] + 2.0, p[1] + 2.0, p[2] + 2.0, p[3]])
            .collect(),
    ];

    // Buffer path: one compile, three execute calls with three different inputs.
    let buf_exec = GpuBufferExecutor::compile(&processor).expect("compile buffer executor");
    for (i, input) in inputs.iter().enumerate() {
        let mut cpu = input.clone();
        processor.apply_rgba(&mut cpu);
        let via_executor = buf_exec.execute(w, h, input).expect("executor execute");
        let via_one_shot = apply_gpu(&processor, w, h, input).expect("apply_gpu");
        for (ch, ((e, o), c)) in via_executor.iter().zip(&via_one_shot).zip(&cpu).enumerate() {
            for k in 0..3 {
                assert!(
                    close(e[k], c[k], 1e-4),
                    "call {i} pixel {ch} chan {k}: executor={:.6} cpu={:.6}",
                    e[k],
                    c[k]
                );
                assert!(
                    close(e[k], o[k], 1e-6),
                    "call {i} pixel {ch} chan {k}: executor={:.6} one-shot={:.6}",
                    e[k],
                    o[k]
                );
            }
        }
    }

    // Texture path: one compile, three execute_new calls, same three inputs.
    let tex_exec = GpuTextureExecutor::compile(&processor).expect("compile texture executor");
    for (i, input) in inputs.iter().enumerate() {
        let mut cpu = input.clone();
        processor.apply_rgba(&mut cpu);
        let src = gpu_mem::GpuImage::upload_rgba_f32(w, h, &flatten(input)).expect("upload");
        let out = tex_exec.execute_new(&src).expect("executor execute_new");
        let got_flat = out.read_rgba_f32().expect("readback");
        for (ch, (g, c)) in got_flat.as_chunks::<4>().0.iter().zip(&cpu).enumerate() {
            for k in 0..3 {
                assert!(
                    close(g[k], c[k], 2e-3),
                    "texture call {i} pixel {ch} chan {k}: got={:.6} cpu={:.6}",
                    g[k],
                    c[k]
                );
            }
        }
    }

    // Caller-owned-output variant (`execute`, not `execute_new`): reuse ONE output image across
    // calls too, proving it isn't secretly required to be fresh each time.
    let owned_output = gpu_mem::GpuImage::new(w, h).expect("alloc output");
    for input in &inputs {
        let src = gpu_mem::GpuImage::upload_rgba_f32(w, h, &flatten(input)).expect("upload");
        tex_exec
            .execute(&src, &owned_output)
            .expect("executor execute (caller-owned output)");
        let mut cpu = input.clone();
        processor.apply_rgba(&mut cpu);
        let got_flat = owned_output.read_rgba_f32().expect("readback");
        for (ch, (g, c)) in got_flat.as_chunks::<4>().0.iter().zip(&cpu).enumerate() {
            for k in 0..3 {
                assert!(
                    close(g[k], c[k], 2e-3),
                    "owned-output pixel {ch} chan {k}: got={:.6} cpu={:.6}",
                    g[k],
                    c[k]
                );
            }
        }
    }
}

/// PLAN9.md O1 "done when": a LUT-bearing processor runs on GPU and matches `Processor::
/// apply_rgba` - `apply_gpu`/`apply_gpu_texture` used to refuse any Lut1D/Lut3D processor outright,
/// so this exercises a path that plainly could not run before this unit (the WGSL itself was also
/// unvalidatable - see `gpu::tests::test_lut1d_lut3d_wgsl_validates`'s discovery of the
/// `texture_1d`/`vec2` coordinate mismatch this unit also fixed).
#[test]
fn lut1d_and_lut3d_gpu_equals_cpu() {
    if !have_gpu("lut1d_and_lut3d_gpu_equals_cpu") {
        return;
    }
    assert!(
        gpu_mem::shared_device().is_some(),
        "shared GPU device must be present"
    );

    let (w, h) = (3u32, 2u32);
    let input = synthetic_image();

    // Non-identity 1D LUT: bakes a gamma-2.2-ish curve, so a wrong sample (wrong axis, wrong
    // texture dimension, wrong domain mapping) shows up as a real numeric divergence, not a
    // silently-passing no-op.
    let lut1d_data: Vec<f32> = (0..256).map(|i| (i as f32 / 255.0).powf(2.2)).collect();
    let mut lut1d = Lut1DTransform::from_data(lut1d_data);
    lut1d.direction = TransformDirection::Forward;
    let transform = Transform::Lut1D(lut1d);
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from Lut1D transform");

    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);
    let gpu = apply_gpu(&processor, w, h, &input).expect("apply_gpu (Lut1D) must succeed");
    // GPU texture-unit bilinear filtering interpolates at LOWER precision than the CPU's exact f32
    // lerp (common GPU hardware behaviour, not a logic bug) - measured ~3e-3 on real hardware here.
    let eps = 5e-3;
    for (i, (g, c)) in gpu.iter().zip(&cpu).enumerate() {
        for ch in 0..3 {
            assert!(
                close(g[ch], c[ch], eps),
                "Lut1D pixel {i} chan {ch}: gpu={:.6} cpu={:.6} (eps={eps})",
                g[ch],
                c[ch]
            );
        }
    }

    // Non-identity 3D LUT: a simple per-axis scale baked into a small (size=9) cube, run through
    // the zero-copy texture path this time (apply_gpu_texture), the one O1 names explicitly.
    // `Lut3DTransform::data`'s convention is R-FASTEST / B-SLOWEST (matching `Lut3DTransform::
    // identity`'s own `for b { for g { for r { data.push([r,g,b]) } } }` nesting) - NOT the
    // R-slowest/B-fastest layout `gpu.rs`'s `bake_lut3d` reshuffles it INTO for the actual GPU
    // texture upload (what the WGSL sampling comment describes). Those are two different layers;
    // this constructs the Transform-level input, so it follows `identity`'s convention.
    let size = 9usize;
    let n = |x: usize| x as f32 / (size - 1) as f32;
    let lut3d_data: Vec<[f32; 3]> = (0..size)
        .flat_map(|b| {
            (0..size)
                .flat_map(move |g| (0..size).map(move |r| [n(r) * 0.8, n(g) * 0.8, n(b) * 0.8]))
        })
        .collect();
    // `from_data` defaults to `Interpolation::Tetrahedral`; this test covers the trilinear path
    // (hardware filtering). The tetrahedral GPU path is `tetrahedral_lut3d_gpu_equals_cpu` below.
    let mut lut3d = Lut3DTransform::from_data(lut3d_data, size).expect("build Lut3D from data");
    lut3d.interpolation = Interpolation::Linear;
    lut3d.direction = TransformDirection::Forward;
    let transform = Transform::Lut3D(lut3d);
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from Lut3D transform");

    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);
    let src = gpu_mem::GpuImage::upload_rgba_f32(w, h, &flatten(&input)).expect("upload input");
    let out = apply_gpu_texture(&processor, &src).expect("apply_gpu_texture (Lut3D) must succeed");
    let got_flat = out.read_rgba_f32().expect("readback");
    let eps_tex = 5e-3; // + f16 GpuImage storage quantization on top of the LUT's own interpolation
    for (i, (g, c)) in got_flat.as_chunks::<4>().0.iter().zip(&cpu).enumerate() {
        for ch in 0..3 {
            assert!(
                close(g[ch], c[ch], eps_tex),
                "Lut3D pixel {i} chan {ch}: gpu={:.6} cpu={:.6} (eps={eps_tex})",
                g[ch],
                c[ch]
            );
        }
    }
}

/// Rec.709 limiting primaries for the Output Transform's display end, matching
/// `aces2_gpu_emission.rs::ot_primaries`/`ot_params`.
fn ot_params() -> Vec<f64> {
    vec![1000.0, 0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290]
}

/// PLAN9.md O2 "done when": the FULL `AcesOutputTransform20` (tonescale + chroma compress + gamut
/// compress, not just the `RGB->JMh` stage `aces2_rgb_to_jmh_gpu_equals_cpu` already covers) through
/// the REAL runtime (`apply_gpu` and the zero-copy `apply_gpu_texture`), on scene-linear AP0 HDR
/// input including negative components - the domain the old `[0,1]^3` LUT bake could not represent
/// and the previous parity tests never exercised for this transform.
#[test]
fn aces2_output_transform_gpu_equals_cpu() {
    if !have_gpu("aces2_output_transform_gpu_equals_cpu") {
        return;
    }
    assert!(
        gpu_mem::shared_device().is_some(),
        "shared GPU device must be present"
    );

    let input: Vec<[f32; 4]> = vec![
        [4.0, 0.25, 0.06, 1.0],
        [8.0, 6.0, 0.10, 1.0],
        [0.55, 7.0, 3.0, 1.0],
        [1.8, 2.2, 8.0, 1.0],
        [-0.5, 0.5, 6.0, 1.0],
        [3.0, -1.2, 3.0, 1.0],
        [0.9, 0.75, 0.55, 1.0],
        [0.18, 0.18, 0.18, 1.0],
    ];
    let (w, h) = (input.len() as u32, 1u32);

    let transform = Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style: FixedFunctionStyle::AcesOutputTransform20,
        params: ot_params(),
        direction: TransformDirection::Forward,
    });
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)
        .expect("processor from ACES2 output transform");

    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);

    let gpu = apply_gpu(&processor, w, h, &input).expect("apply_gpu (ACES2 output) must succeed");
    assert_eq!(gpu.len(), cpu.len(), "buffer pixel count mismatch");

    let src_img = gpu_mem::GpuImage::upload_rgba_f32(w, h, &flatten(&input)).expect("upload input");
    let out_img = apply_gpu_texture(&processor, &src_img).expect("apply_gpu_texture must succeed");
    let tex_flat = out_img.read_rgba_f32().expect("readback output");
    let tex: Vec<[f32; 4]> = tex_flat
        .as_chunks::<4>().0.iter()
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();

    // Same bound `aces2_rgb_to_jmh_gpu_equals_cpu` uses for the GPU-vs-CPU transcendentals; the
    // texture path adds f16 quantization on top (see `texture_gpu_three_way_parity`'s note).
    let eps_buf = 5e-3;
    let eps_tex = 1e-2;
    for (i, ((g, c), t)) in gpu.iter().zip(&cpu).zip(&tex).enumerate() {
        for ch in 0..3 {
            assert!(
                close(g[ch], c[ch], eps_buf),
                "output-transform buffer pixel {i} chan {ch}: gpu={:.6} cpu={:.6} (eps={eps_buf})",
                g[ch],
                c[ch]
            );
            assert!(
                close(t[ch], c[ch], eps_tex),
                "output-transform texture pixel {i} chan {ch}: tex={:.6} cpu={:.6} (eps={eps_tex})",
                t[ch],
                c[ch]
            );
        }
    }
    eprintln!(
        "aces2_output_transform_gpu_equals_cpu: max rel err buf={:.3e} tex={:.3e}",
        max_rel_err(&gpu, &cpu, 3),
        max_rel_err(&tex, &cpu, 3)
    );
}

/// GradingPrimary (LOG style, forward AND inverse) — the checkpoint this test
/// belongs to per `docs/ROADMAP.md`'s GradingPrimary item: unified type +
/// LOG style, CPU + all four GPU emitters, verified with a real wgpu run,
/// not just shader-text assertions. Uses a non-trivial `master` alongside
/// per-channel values on brightness/contrast/gamma specifically to exercise
/// the RGBM combination (`op::grading_primary_params`, ported from OCIO's
/// `GradingPrimaryPreRender::update`) end to end on the GPU — a test built
/// from all-identity values could pass by accident even with the master
/// combination silently missing.
#[test]
fn grading_primary_log_gpu_equals_cpu() {
    if !have_gpu("grading_primary_log_gpu_equals_cpu") {
        return;
    }
    assert!(
        gpu_mem::shared_device().is_some(),
        "shared GPU device must be present"
    );

    let (w, h) = (3u32, 2u32);
    let input = synthetic_image();

    let value = GradingPrimary {
        brightness: GradingRgbm::new(0.5, -0.3, 0.2, 1.0),
        contrast: GradingRgbm::new(1.1, 0.9, 1.05, 1.2),
        gamma: GradingRgbm::new(0.95, 1.1, 1.0, 1.05),
        saturation: 1.15,
        ..GradingPrimary::log_default()
    };

    for direction in [TransformDirection::Forward, TransformDirection::Inverse] {
        let transform = Transform::GradingPrimary(GradingPrimaryTransform {
            name: String::new(),
            style: GradingPrimaryStyle::Log,
            value: value.clone(),
            direction,
        });
        let processor = Processor::from_transform(&transform, TransformDirection::Forward)
            .expect("processor from GradingPrimary transform");

        let mut cpu = input.clone();
        processor.apply_rgba(&mut cpu);

        let gpu =
            apply_gpu(&processor, w, h, &input).expect("apply_gpu (GradingPrimary) must succeed");
        assert_eq!(gpu.len(), cpu.len(), "pixel count mismatch ({direction:?})");

        let eps = 5e-3;
        for (i, (g, c)) in gpu.iter().zip(&cpu).enumerate() {
            for ch in 0..3 {
                assert!(
                    close(g[ch], c[ch], eps),
                    "GradingPrimary LOG {direction:?} pixel {i} chan {ch}: \
                     gpu={:.6} cpu={:.6} (eps={eps})",
                    g[ch],
                    c[ch]
                );
            }
        }
        eprintln!(
            "grading_primary_log_gpu_equals_cpu ({direction:?}): max relative error = {:.3e}",
            max_rel_err(&gpu, &cpu, 3)
        );
    }
}

/// B28: alpha on the GPU is the CPU's (OCIO's): a 4x4 matrix with an alpha column, row and offset,
/// and exponent / moncurve ops with their own alpha parameters in every negative style, on
/// pixels with negative and fractional alpha. All four channels are compared.
#[test]
fn rgba_ops_gpu_equals_cpu() {
    use vfx_ocio::{
        ExponentTransform, ExponentWithLinearTransform, MatrixTransform, NegativeStyle,
    };
    #[rustfmt::skip]
    let m = [0.9, 0.05, 0.02, 0.1,  0.03, 1.1, 0.0, -0.05,  0.01, 0.02, 0.8, 0.2,  0.25, -0.1, 0.05, 0.7];
    let mut ops = vec![Transform::Matrix(MatrixTransform {
        name: String::new(),
        matrix: m,
        offset: [0.01, -0.02, 0.03, 0.05],
        direction: TransformDirection::Forward,
    })];
    for style in [
        NegativeStyle::Mirror,
        NegativeStyle::PassThru,
        NegativeStyle::Clamp,
    ] {
        ops.push(Transform::Exponent(ExponentTransform {
            name: String::new(),
            value: [1.2, 1.1, 0.9, 1.5],
            negative_style: style,
            direction: TransformDirection::Forward,
        }));
    }
    for (mirror, direction) in [
        (false, TransformDirection::Forward),
        (true, TransformDirection::Inverse),
    ] {
        let mut t =
            ExponentWithLinearTransform::new([2.4, 2.2, 2.0, 1.8], [0.055, 0.099, 0.1, 0.2]);
        if mirror {
            t.set_negative_style(NegativeStyle::Mirror).unwrap();
        }
        t.direction = direction;
        ops.push(Transform::ExponentWithLinear(t));
    }
    let processor = Processor::from_transform(&Transform::group(ops), TransformDirection::Forward)
        .expect("processor");

    let (w, h) = (3u32, 2u32);
    let mut input = synthetic_image();
    input[1][3] = -0.3;
    input[3][3] = 0.05;
    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);
    let gpu = apply_gpu(&processor, w, h, &input).expect("apply_gpu");
    let eps = 1e-4;
    for (i, (g, c)) in gpu.iter().zip(&cpu).enumerate() {
        for ch in 0..4 {
            assert!(
                close(g[ch], c[ch], eps),
                "pixel {i} chan {ch}: gpu={:.6} cpu={:.6} (in {:?})",
                g[ch],
                c[ch],
                input[i]
            );
        }
    }
    eprintln!(
        "rgba_ops_gpu_equals_cpu: max relative error = {:.3e}",
        max_rel_err(&gpu, &cpu, 4)
    );
}

/// EMIT-12: a GradingPrimary that only clamps is not a bypass. OCIO's local bypass
/// (`GradingPrimaryPreRender::update`, `GradingPrimary.cpp:108-110`) requires saturation 1 AND
/// `clampBlack == NoClampBlack()` AND `clampWhite == NoClampWhite()`; the GPU bypass test here
/// ignored the clamps, so the op did nothing on the GPU. PyOCIO 2.5.1, every style,
/// clampBlack 0.1 / clampWhite 0.6: [0.0, 0.3, 0.9] -> [0.1, 0.3, 0.6].
#[test]
fn grading_primary_clamp_only_is_applied_on_gpu() {
    let input = vec![[0.0_f32, 0.3, 0.9, 1.0], [-2.0, 0.55, 4.0, 0.5]];
    let want = [[0.1_f32, 0.3, 0.6], [0.1, 0.55, 0.6]];
    for (style, base) in [
        (GradingPrimaryStyle::Log, GradingPrimary::log_default()),
        (
            GradingPrimaryStyle::Linear,
            GradingPrimary::linear_default(),
        ),
        (GradingPrimaryStyle::Video, GradingPrimary::video_default()),
    ] {
        for direction in [TransformDirection::Forward, TransformDirection::Inverse] {
            let transform = Transform::GradingPrimary(GradingPrimaryTransform {
                name: String::new(),
                style,
                value: GradingPrimary {
                    clamp_black: 0.1,
                    clamp_white: 0.6,
                    ..base.clone()
                },
                direction,
            });
            let processor = Processor::from_transform(&transform, TransformDirection::Forward)
                .expect("processor");
            let mut cpu = input.clone();
            processor.apply_rgba(&mut cpu);
            let gpu = apply_gpu(&processor, 2, 1, &input).expect("apply_gpu");
            for i in 0..2 {
                for ch in 0..3 {
                    assert!(
                        close(cpu[i][ch], want[i][ch], 1e-6)
                            && close(gpu[i][ch], want[i][ch], 1e-6),
                        "{style:?} {direction:?} pixel {i} chan {ch}: cpu {} gpu {} want {}",
                        cpu[i][ch],
                        gpu[i][ch],
                        want[i][ch]
                    );
                }
            }
        }
    }
}

/// OCIO-M9 remainder: a 1D LUT's input domain is per channel (`.cube` `DOMAIN_MIN r g b`; OCIO
/// turns it into a per-channel min/max matrix before the LUT). The GPU used channel 0's domain for
/// all three. PyOCIO 2.5.1 on this file (linear interpolation).
#[test]
fn lut1d_per_channel_domain_on_gpu() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dom.cube");
    std::fs::write(
        &path,
        "LUT_1D_SIZE 5\nDOMAIN_MIN 0.0 0.1 -0.2\nDOMAIN_MAX 1.0 2.0 0.8\n\
         0.000000 0.000000 0.200000\n0.062500 0.500000 0.350000\n0.250000 0.707107 0.500000\n\
         0.562500 0.866025 0.650000\n1.000000 1.000000 0.800000\n",
    )
    .unwrap();
    let input = vec![
        [0.3_f32, 0.7, 0.1, 1.0],
        [0.9, 1.5, -0.1, 1.0],
        [0.55, 0.2, 0.6, 1.0],
    ];
    let want = [
        [0.1_f32, 0.5545018, 0.38],
        [0.8249999, 0.8576609, 0.26],
        [0.3125, 0.1052632, 0.68],
    ];
    let processor =
        Processor::from_transform(&Transform::file(&path), TransformDirection::Forward).unwrap();
    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);
    let gpu = apply_gpu(&processor, 3, 1, &input).expect("apply_gpu");
    for i in 0..3 {
        for ch in 0..3 {
            assert!(
                close(cpu[i][ch], want[i][ch], 1e-5) && close(gpu[i][ch], want[i][ch], 1e-3),
                "pixel {i} chan {ch}: cpu {} gpu {} PyOCIO {}",
                cpu[i][ch],
                gpu[i][ch],
                want[i][ch]
            );
        }
    }
}

/// OCIO-M4 GPU half: every ExposureContrast style divides by the pivot, which OCIO floors at
/// `EC::MIN_PIVOT` on the CPU and on the GPU (`ExposureContrastOpGPU.cpp:100,134,169`). PyOCIO
/// 2.5.1, exposure 0.5, contrast 1.3, gamma 1.1, pivot 0, on [0.002, 0.18, 0.9].
#[test]
fn exposure_contrast_pivot_zero_is_floored_on_gpu() {
    use vfx_ocio::{ExposureContrastStyle, ExposureContrastTransform};
    let input = vec![[0.002_f32, 0.18, 0.9, 1.0]];
    for (style, want) in [
        (
            ExposureContrastStyle::Linear,
            [0.004423_f32, 2.755996, 27.530134],
        ),
        (ExposureContrastStyle::Video, [0.000918, 0.572238, 5.716042]),
        (
            ExposureContrastStyle::Logarithmic,
            [0.06578, 0.32032, 1.34992],
        ),
    ] {
        let t = Transform::ExposureContrast(ExposureContrastTransform {
            name: String::new(),
            exposure: 0.5,
            contrast: 1.3,
            gamma: 1.1,
            pivot: 0.0,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            style,
            direction: TransformDirection::Forward,
            dynamic: vfx_ocio::EcDynamic::NONE,
        });
        let processor = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut cpu = input.clone();
        processor.apply_rgba(&mut cpu);
        let gpu = apply_gpu(&processor, 1, 1, &input).expect("apply_gpu");
        for ch in 0..3 {
            assert!(
                // 5e-5 against PyOCIO: its CPU pow is the SSE approximation.
                close(cpu[0][ch], want[ch], 5e-5) && close(gpu[0][ch], cpu[0][ch], 1e-4),
                "{style:?} chan {ch}: cpu {} gpu {} PyOCIO {}",
                cpu[0][ch],
                gpu[0][ch],
                want[ch]
            );
        }
    }
}

/// OCIO-M12: OCIO's log ops floor their argument at `std::numeric_limits<float>::min()` on the
/// CPU and in their shaders (`LogOpCPU.cpp:393`, `LogOpGPU.cpp:20,130,272`); the allocation's
/// LG2 is a log op too. The GPU used 1e-10 and the CPU allocation 1e-10, so tiny and zero inputs
/// came out at log2(1e-10) = -33.2 instead of down to -126. PyOCIO 2.5.1 on [1e-20, 1e-12, 0].
#[test]
fn log_ops_floor_at_flt_min() {
    use vfx_ocio::{AllocationTransform, AllocationType, LogAffineTransform, LogTransform};
    let input = vec![[1e-20_f32, 1e-12, 0.0, 1.0]];
    let fwd = TransformDirection::Forward;
    let cases = [
        (
            "log2",
            Transform::Log(LogTransform {
                name: String::new(),
                base: 2.0,
                direction: fwd,
            }),
            [-66.438576_f32, -39.863144, -125.99998],
        ),
        (
            "allocation lg2",
            Transform::Allocation(AllocationTransform {
                allocation: AllocationType::Log2,
                vars: vec![-70.0, 4.0],
                direction: fwd,
            }),
            [0.048127294, 0.40725476, -0.7567566],
        ),
        (
            "log affine base 10",
            Transform::LogAffine(LogAffineTransform {
                base: 10.0,
                ..Default::default()
            }),
            [-20.000006, -12.000003, -37.929775],
        ),
    ];
    for (name, t, want) in cases {
        let processor = Processor::from_transform(&t, fwd).unwrap();
        let mut cpu = input.clone();
        processor.apply_rgba(&mut cpu);
        let gpu = apply_gpu(&processor, 1, 1, &input).expect("apply_gpu");
        for ch in 0..3 {
            assert!(
                close(cpu[0][ch], want[ch], 1e-5) && close(gpu[0][ch], want[ch], 1e-5),
                "{name} chan {ch}: cpu {} gpu {} PyOCIO {}",
                cpu[0][ch],
                gpu[0][ch],
                want[ch]
            );
        }
    }
}

/// An inverse 3D LUT on the GPU is the exact inverse baked on a 48^3 grid over [0,1]^3, as OCIO's
/// GPU path does (`MakeFastLut3DFromInverse`); PyOpenColorIO 2.5.1 at its default level applies
/// the same bake on the CPU. `.vf` = 4x4 then LUT; inverse = inverse LUT then inverse 4x4.
#[test]
fn inverse_lut3d_on_gpu_is_ocios_48_grid_bake() {
    let mut rows = Vec::new();
    for r in 0..5 {
        for g in 0..5 {
            for b in 0..5 {
                let (r, g, b) = (r as f64 / 4.0, g as f64 / 4.0, b as f64 / 4.0);
                rows.push(format!(
                    "{:.6} {:.6} {:.6}",
                    r.powf(1.5) * 0.9 + 0.05 * g,
                    g.powf(0.8),
                    0.1 + 0.8 * b.powf(1.2)
                ));
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nl.vf");
    std::fs::write(
        &path,
        format!(
            "#Inventor V2.1 ascii\ngrid_size 5 5 5\nglobal_transform 0.225 0.0125 0.005 0.1  \
             0.0075 0.275 0.0 -0.05  0.0025 0.005 0.2 0.2  0.0625 -0.025 0.0125 0.7\ndata\n{}\n",
            rows.join("\n")
        ),
    )
    .unwrap();
    let processor =
        Processor::from_transform(&Transform::file(&path), TransformDirection::Inverse).unwrap();
    let input = vec![[0.3_f32, 0.2, 0.1, 0.8], [0.5, 0.6, 0.4, 1.0]];
    // PyOCIO OPTIMIZATION_DEFAULT (LUT_INV_FAST).
    let want = [
        [0.3249304_f32, 0.1423357, -0.208619, 1.0418427],
        [0.4318461, 0.4201863, 0.1635522, 1.2962134],
    ];
    let gpu = apply_gpu(&processor, 2, 1, &input).expect("apply_gpu");
    for i in 0..2 {
        for ch in 0..4 {
            // Hardware trilinear filtering has limited fractional precision.
            assert!(
                close(gpu[i][ch], want[i][ch], 5e-3),
                "pixel {i} chan {ch}: gpu {} PyOCIO {}",
                gpu[i][ch],
                want[i][ch]
            );
        }
    }
}

/// B5b: a `Tetrahedral` (and `Best`) 3D LUT is tetrahedral on the GPU too: OCIO
/// `Lut3DOpGPU.cpp`'s six-case body over NEAREST fetches (`gpu/lut3d.rs`), against the CPU
/// (`vfx_lut::interpolate_lattice`, pinned to PyOpenColorIO by
/// `processor::tests::lut3d_interpolation_matches_pyocio`). A strongly non-linear, asymmetric
/// 5^3 LUT with a non-unit domain; probes between nodes in all six tetrahedra, on nodes, and out
/// of the domain (clamped, as the CPU). No hardware weight quantisation is involved, so the bound
/// is f32-tight. Red before: every emitter sampled with hardware trilinear, which differs from
/// the CPU tetrahedral result by more than 1e-3 on these probes (the last assertion).
#[test]
fn tetrahedral_lut3d_gpu_equals_cpu() {
    if !have_gpu("tetrahedral_lut3d_gpu_equals_cpu") {
        return;
    }
    let size = 5usize;
    let t = |i: usize| i as f32 / (size - 1) as f32;
    let mut data = Vec::new();
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let (r, g, b) = (t(r), t(g), t(b));
                data.push([
                    r * r * 0.9 + 0.3 * g * b,
                    g.sqrt() * 0.7 + 0.2 * r,
                    (b * b * b) + 0.1 * r * g,
                ]);
            }
        }
    }
    let lut = Lut3DTransform::from_data(data, size)
        .expect("5^3")
        .with_domain([-0.1, 0.0, 0.05], [1.1, 1.2, 0.95]);
    let input: Vec<[f32; 4]> = vec![
        [0.62, 0.31, 0.12, 1.0], // R > G > B
        [0.62, 0.12, 0.31, 1.0], // R > B > G
        [0.33, 0.12, 0.66, 1.0], // B > R > G
        [0.12, 0.33, 0.66, 1.0], // B > G > R
        [0.33, 0.66, 0.12, 1.0], // G > R > B
        [0.12, 0.66, 0.33, 1.0], // G > B > R
        [0.2, 0.3, 0.5, 0.5],    // a node
        [-0.4, 1.7, 1.2, 1.0],   // out of the domain
        [1.1, 1.2, 0.95, 1.0],   // the domain's top corner
    ];
    for interp in [Interpolation::Tetrahedral, Interpolation::Best] {
        let processor = Processor::from_transform(
            &Transform::Lut3D(Lut3DTransform {
                interpolation: interp,
                ..lut.clone()
            }),
            TransformDirection::Forward,
        )
        .expect("processor");
        let mut cpu = input.clone();
        processor.apply_rgba(&mut cpu);
        let gpu = apply_gpu(&processor, input.len() as u32, 1, &input).expect("apply_gpu");
        let err = max_rel_err(&gpu, &cpu, 3);
        assert!(
            err <= 2e-6,
            "{interp:?}: max rel err {err}\ngpu {gpu:?}\ncpu {cpu:?}"
        );
    }
    // The probes discriminate: trilinear lands elsewhere.
    let trilinear = Processor::from_transform(
        &Transform::Lut3D(Lut3DTransform {
            interpolation: Interpolation::Linear,
            ..lut.clone()
        }),
        TransformDirection::Forward,
    )
    .unwrap();
    let tetra =
        Processor::from_transform(&Transform::Lut3D(lut), TransformDirection::Forward).unwrap();
    let (mut a, mut b) = (input.clone(), input.clone());
    trilinear.apply_rgba(&mut a);
    tetra.apply_rgba(&mut b);
    assert!(
        max_rel_err(&a, &b, 3) > 1e-3,
        "probes do not separate tetrahedral from trilinear"
    );
}
