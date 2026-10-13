//! GPU == CPU for the curve-shaped FixedFunctions (PQ, gamma-log, double-log), both directions,
//! on real wgpu.
//!
//! These styles used to be emitted four times over. GLSL emitted nothing for gamma-log and
//! double-log; the others used `sign()` for OCIO's `copysign(1, x)`, which differs exactly at the
//! mirror point — where Apple Log's decode, fed a clamped negative (0), must return `R_0`. They
//! now share `gpu/ff_curves.rs` with the parameters the CPU apply uses; this file runs the WGSL
//! backend against the CPU oracle, which is itself pinned to PyOpenColorIO by the builtin table.
#![cfg(feature = "gpu")]

use vfx_ocio::{
    FixedFunctionStyle, FixedFunctionTransform, Processor, Transform, TransformDirection, apply_gpu,
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

/// HLG OETF with E_MAX = 3, OCIO's non-LUT parameters (`Displays.cpp` `HLG::params`).
fn hlg_params() -> Vec<f64> {
    let a = 0.17883277_f64;
    let b = (1.0 - 4.0 * a) * 3.0 / 12.0;
    let c0 = 0.5 - a * (4.0 * a).ln();
    let c = (12.0_f64 / 3.0).ln() * a + c0;
    vec![
        0.0,
        3.0 / 12.0,
        0.5,
        1.0,
        0.0,
        std::f64::consts::E,
        a,
        c,
        1.0,
        -b,
    ]
}

/// Apple Log, OCIO's non-LUT parameters (`AppleCameras.cpp`).
fn apple_log_params() -> Vec<f64> {
    let (r0, rt, c, beta, gamma, delta) = (
        -0.05641088,
        0.01,
        47.28711236,
        0.00964052,
        0.08550479,
        0.69336945,
    );
    vec![r0, rt, 2.0, c, -r0, 2.0, gamma, delta, 1.0, beta]
}

/// Canon Log 3, OCIO's non-LUT parameters (`CanonCameras.cpp`), with a real linear segment.
fn clog3_params() -> Vec<f64> {
    vec![
        10.0,
        -0.014 * 0.9,
        0.014 * 0.9,
        -0.36726845,
        0.12783901,
        -14.98325 / 0.9,
        1.0,
        0.36726845,
        0.12240537,
        14.98325 / 0.9,
        1.0,
        1.9754798,
        0.12512219,
    ]
}

/// `tol` is relative, scaled by max(1, |v|): 2e-5 is a few f32 ulps through the curves'
/// transcendental functions; the PQ decode needs more, since `1/m1 ~ 6.3` amplifies the GPU's
/// `pow` error near the top of the code range.
fn parity(
    style: FixedFunctionStyle,
    params: Vec<f64>,
    direction: TransformDirection,
    probes: &[f32],
    tol: f32,
) {
    let t = Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style,
        params,
        direction,
    });
    let proc = Processor::from_transform(&t, TransformDirection::Forward).expect("processor");
    // `+ 0.0` turns the -0.0 that `-0.0 * 0.5` makes into +0.0: the CPU (like OCIO's CPU)
    // mirrors with `copysign(1, x)`, which is -1 for -0.0, and a shader cannot see that sign
    // bit without bit casts; every other value, including +0.0 at the mirror point, must agree.
    let pixels: Vec<[f32; 4]> = probes
        .iter()
        .map(|&v| [v, -v * 0.5 + 0.0, v * 2.0, 1.0])
        .collect();
    let mut cpu = pixels.clone();
    proc.apply_rgba(&mut cpu);
    let gpu = apply_gpu(&proc, pixels.len() as u32, 1, &pixels).expect("gpu apply");
    for (i, (c, g)) in cpu.iter().zip(&gpu).enumerate() {
        for ch in 0..3 {
            assert!(
                (c[ch] - g[ch]).abs() <= tol * c[ch].abs().max(1.0),
                "{style:?} {direction:?} pixel {i} ({:?}) ch{ch}: cpu {} gpu {}",
                pixels[i],
                c[ch],
                g[ch]
            );
        }
    }
}

#[test]
fn pq_gpu_matches_cpu() {
    if !have_gpu("pq_gpu_matches_cpu") {
        return;
    }
    let probes = [-0.3, 0.0, 0.0005, 0.18, 1.0, 5.0, 40.0];
    parity(
        FixedFunctionStyle::LinToPq,
        vec![],
        TransformDirection::Forward,
        &probes,
        2e-5,
    );
    // The third channel doubles each probe, so these stay below ~1.2: PQ decode has a pole at
    // x = c2/c3 (code ~1.99) where CPU and GPU both run off to ~1e9 and agree on nothing.
    let codes = [-0.4, 0.0, 0.05, 0.3, 0.45, 0.55, 0.6];
    parity(
        FixedFunctionStyle::LinToPq,
        vec![],
        TransformDirection::Inverse,
        &codes,
        1e-4,
    );
    parity(
        FixedFunctionStyle::PqToLin,
        vec![],
        TransformDirection::Forward,
        &codes,
        1e-4,
    );
}

#[test]
fn gamma_log_gpu_matches_cpu() {
    if !have_gpu("gamma_log_gpu_matches_cpu") {
        return;
    }
    let lin = [-0.5, -0.01, 0.0, 0.05, 0.25, 0.26, 1.0, 12.0];
    parity(
        FixedFunctionStyle::LinToGammaLog,
        hlg_params(),
        TransformDirection::Forward,
        &lin,
        2e-5,
    );
    let sig = [-0.4, 0.0, 0.1, 0.5, 0.51, 0.9, 1.1];
    parity(
        FixedFunctionStyle::GammaLogToLin,
        hlg_params(),
        TransformDirection::Forward,
        &sig,
        2e-5,
    );
    // Apple Log decode at and below its mirror point (0 in the encoded domain).
    let apple = [0.0, 0.001, 0.004, 0.2, 0.6, 0.9];
    parity(
        FixedFunctionStyle::GammaLogToLin,
        apple_log_params(),
        TransformDirection::Forward,
        &apple,
        2e-5,
    );
}

#[test]
fn double_log_gpu_matches_cpu() {
    if !have_gpu("double_log_gpu_matches_cpu") {
        return;
    }
    let lin = [-0.3, -0.0126, 0.0, 0.0126, 0.2, 1.0, 8.0];
    parity(
        FixedFunctionStyle::LinToDoubleLog,
        clog3_params(),
        TransformDirection::Forward,
        &lin,
        2e-5,
    );
    let code = [0.0, 0.09, 0.1, 0.125, 0.16, 0.5, 1.0];
    parity(
        FixedFunctionStyle::DoubleLogToLin,
        clog3_params(),
        TransformDirection::Forward,
        &code,
        2e-5,
    );
}

/// The CPU side of the mirror-point case, anchored: Apple Log decodes 0 to `R_0`, the value OCIO's
/// half LUT stores for every input `<= 0` (`AppleCameras.cpp` `GenerateLutValues`).
#[test]
fn apple_log_decode_at_the_mirror_point_is_r0() {
    if !have_gpu("apple_log_decode_at_the_mirror_point_is_r0") {
        return;
    }
    let t = Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style: FixedFunctionStyle::GammaLogToLin,
        params: apple_log_params(),
        direction: TransformDirection::Forward,
    });
    let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
    let px = [[0.0_f32, 0.0, 0.0, 1.0]];
    let gpu = apply_gpu(&proc, 1, 1, &px).unwrap();
    let mut cpu = px;
    proc.apply_rgba(&mut cpu);
    for (label, v) in [("cpu", cpu[0][0]), ("gpu", gpu[0][0])] {
        assert!((v - -0.05641088).abs() < 1e-6, "{label}: {v}");
    }
}
