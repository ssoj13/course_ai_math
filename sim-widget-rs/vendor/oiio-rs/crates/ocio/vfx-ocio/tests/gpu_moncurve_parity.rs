//! GPU == CPU for `ExponentWithLinear` (the OCIO MonCurve), both directions.
//!
//! # Why this file exists
//!
//! The GPU emitters implemented the *unnormalised* moncurve — `(x+off)^g` forward and
//! `x^(1/g) − off` inverse — while OCIO (and this crate's CPU port in `processor/apply.rs`)
//! implement the curve normalised by `1/(1+offset)` so that `f(1) == 1`. The emitted curve was
//! internally self-consistent (its own tangent met its own power branch at the break), so nothing
//! looked broken: it was simply a *different curve*.
//!
//! Measured downstream in playa, on the studio config's `sRGB - Display`: scene-linear 0.18 through
//! `ACES 2.0 - SDR 100 nits (Rec.709)` encoded to **0.32809 on the GPU against 0.34916 on the
//! CPU** — 12 % dark in display light (8.79 nits where ACES specifies 10.0), on every pixel of
//! every frame.
//!
//! # Why the existing tests did not catch it
//!
//! There are **two spellings of sRGB** in the GPU emitters. `gpu_runtime_parity.rs` exercises
//! `GpuOp::Transfer { style: "srgb" }` from `gpu/transfer.rs`, whose constants are correct. The op
//! a real OCIO config actually produces for a display encode is `GpuOp::ExponentWithLinear`, and
//! that one had no parity test at all. A green suite over the wrong spelling is worth nothing.
//!
//! So this file pins the op **by name**, not by intent.
#![cfg(feature = "gpu")]

use vfx_ocio::{
    ExponentWithLinearTransform, NegativeStyle, Processor, Transform, TransformDirection, apply_gpu,
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

/// The moncurve is smooth but its two branches meet at a break, so a single tolerance has to hold
/// on both sides of it. 1e-5 is ~3 orders tighter than the 12 % defect and ~2 tighter than the
/// f32 shader noise floor measured on the matrix path in `gpu_runtime_parity.rs`.
const EPS: f32 = 1e-5;

fn srgb_moncurve(direction: TransformDirection, negative_style: NegativeStyle) -> Transform {
    Transform::ExponentWithLinear(ExponentWithLinearTransform {
        name: String::new(),
        gamma: [2.4, 2.4, 2.4, 1.0],
        offset: [0.055, 0.055, 0.055, 0.0],
        // Mirror is what the studio config's `DISPLAY - CIE-XYZ-D65_to_sRGB - MIRROR NEGS` uses;
        // Linear is OCIO's default (`DISPLAY - CIE-XYZ-D65_to_sRGB`), which sends negatives down
        // the linear leg instead of reflecting the power curve. The probes include a negative so
        // both are exercised.
        negative_style,
        direction,
    })
}

/// Values chosen to land on BOTH sides of the break in both directions, plus a negative (Mirror)
/// and an over-range one. The forward break is at 0.0392857 and the inverse break at 0.0030399,
/// so 0.002 / 0.02 sit in the linear toe and the rest on the power branch.
const PROBES: [f32; 7] = [-0.25, 0.002, 0.02, 0.1, 0.18, 0.5, 1.0];

fn parity(direction: TransformDirection, negative_style: NegativeStyle, label: &str) {
    let proc = Processor::from_transform(
        &srgb_moncurve(direction, negative_style),
        TransformDirection::Forward,
    )
    .expect("build processor");

    let pixels: Vec<[f32; 4]> = PROBES.iter().map(|&v| [v, v, v, 1.0]).collect();
    let mut cpu = pixels.clone();
    proc.apply_rgba(&mut cpu);
    let gpu = apply_gpu(&proc, PROBES.len() as u32, 1, &pixels).expect("gpu apply");

    let mut worst = 0.0f32;
    for (i, (c, g)) in cpu.iter().zip(gpu.iter()).enumerate() {
        for ch in 0..3 {
            let d = (c[ch] - g[ch]).abs();
            worst = worst.max(d);
            // Relative above 1: the Linear style's inverse sends -0.25 to about -3.2.
            assert!(
                d <= EPS * c[ch].abs().max(1.0),
                "{label}: probe {i} (in={}) channel {ch}: cpu={} gpu={} delta={d}",
                PROBES[i],
                c[ch],
                g[ch]
            );
        }
    }
    println!("{label}: worst |cpu-gpu| = {worst:.3e}");
}

#[test]
fn moncurve_forward_gpu_matches_cpu() {
    if !have_gpu("moncurve_forward_gpu_matches_cpu") {
        return;
    }
    parity(
        TransformDirection::Forward,
        NegativeStyle::Mirror,
        "forward mirror",
    );
    parity(
        TransformDirection::Forward,
        NegativeStyle::Linear,
        "forward linear",
    );
}

#[test]
fn moncurve_inverse_gpu_matches_cpu() {
    if !have_gpu("moncurve_inverse_gpu_matches_cpu") {
        return;
    }
    parity(
        TransformDirection::Inverse,
        NegativeStyle::Mirror,
        "inverse mirror",
    );
    parity(
        TransformDirection::Inverse,
        NegativeStyle::Linear,
        "inverse linear",
    );
}

/// The absolute anchor, so this cannot drift with both implementations at once.
///
/// The sRGB inverse-EOTF is published: `1.055·L^(1/2.4) − 0.055`. This is the number ACES leans on
/// when it places SDR mid-grey at 10 % of peak — display-linear 0.10 encodes to 0.3491902, which is
/// what a correct display encode must produce and what the defective emitter got wrong by 12 %.
#[test]
fn moncurve_inverse_hits_the_published_srgb_value() {
    if !have_gpu("moncurve_inverse_hits_the_published_srgb_value") {
        return;
    }
    let proc = Processor::from_transform(
        &srgb_moncurve(TransformDirection::Inverse, NegativeStyle::Mirror),
        TransformDirection::Forward,
    )
    .expect("build processor");

    let l = 0.10f32;
    let expected = 1.055 * l.powf(1.0 / 2.4) - 0.055; // 0.3491902

    let input = vec![[l, l, l, 1.0]];
    let mut cpu = input.clone();
    proc.apply_rgba(&mut cpu);
    let gpu = apply_gpu(&proc, 1, 1, &input).expect("gpu apply");

    for (label, got) in [("cpu", cpu[0][0]), ("gpu", gpu[0][0])] {
        assert!(
            (got - expected).abs() <= 2e-4,
            "{label}: display-linear {l} encoded to {got}, expected {expected} \
             (the published sRGB inverse-EOTF, and ACES' 10 %-of-peak mid-grey anchor)"
        );
    }
}
