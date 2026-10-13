//! Every OCIO builtin transform on the GPU (real wgpu) against the CPU processor.
//!
//! The CPU side is pinned to PyOpenColorIO by `builtin_parity_table.rs` and
//! `half_domain_builtins.rs`; this file pins the GPU to the CPU for the same 98 styles, so a
//! convention the CPU gets right (PQ's 1.0 = 100 nits, HLG's E_MAX = 3, the LINEAR moncurve
//! negatives, the unbounded ACES 1.x tone-curve splines) cannot silently differ in the shader.
//! The four styles built on a half-domain Lut1D (ADX density, the DCI roll-white) run in both
//! directions: forward through the half-code texture (`gpu/lut1d_half.rs`, OCIO
//! `Lut1DOpGPU.cpp`), inverse through OCIO's half-domain bake of the exact inverse
//! (`MakeFastLut1DFromInverse`).
#![cfg(feature = "gpu")]

use vfx_ocio::{BuiltinTransform, Processor, Transform, TransformDirection, apply_gpu};

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

/// The probes of `builtin_parity_table.rs` plus an HDR value and a negative-heavy one.
const PROBES: [[f32; 4]; 7] = [
    [0.18, 0.18, 0.18, 1.0],
    [0.5, 0.4, 0.3, 1.0],
    [0.02, 0.3, 0.9, 1.0],
    [1.5, 0.8, 0.05, 1.0],
    [-0.05, 0.1, 0.25, 1.0],
    [4.0, 2.5, 0.7, 1.0],
    [-0.2, -0.01, 0.6, 1.0],
];

/// Styles built on a half-domain Lut1D (ADX density, the DCI roll-white).
fn uses_half_domain_lut(style: &str) -> bool {
    style.starts_with("ADX") || style.contains("sim-DCI")
}

/// Relative (above 1) / absolute (below 1) tolerance. The CPU runs f32 too, so most styles agree
/// to ~1e-6; the slack is for the sampled paths: a 4096-entry [0, 1] Lut1D (ACEScc, Canon Log)
/// is filtered by the texture unit, and the ACES 2.0 output transform's tables are sampled on
/// the GPU (`aces2_gpu_emission.rs` uses the same order of bound). A half-domain LUT is filtered
/// between adjacent half codes; its inverse is a bake (the CPU inverts exactly).
fn tolerance(style: &str) -> f32 {
    if style.contains("_2.0") || style.contains("CLOG") || style.starts_with("ACEScc_") {
        3e-3
    } else if style == "CURVE - ST-2084_to_LINEAR" {
        // PQ decode amplifies the shader's `pow` error by 1/m1 ~ 6.3 near the top of the range.
        1e-3
    } else if uses_half_domain_lut(style) {
        1e-3
    } else {
        2e-4
    }
}

/// PQ decode has a pole at code ~1.99 (`c2 - c3 * x = 0`); both sides run off to ~1e9 there and
/// agree on nothing, so probes past the signal range are not compared for it.
fn comparable(style: &str, input: f32) -> bool {
    style != "CURVE - ST-2084_to_LINEAR" || input.abs() <= 1.5
}

#[test]
fn every_builtin_gpu_matches_cpu() {
    if !have_gpu("every_builtin_gpu_matches_cpu") {
        return;
    }
    let mut failures = Vec::new();
    let mut checked = 0;
    for style in vfx_ocio::builtin::transforms::styles() {
        let directions: &[TransformDirection] = if uses_half_domain_lut(style) {
            &[TransformDirection::Forward, TransformDirection::Inverse]
        } else {
            &[TransformDirection::Forward]
        };
        for &direction in directions {
            let t = Transform::Builtin(BuiltinTransform {
                style: style.to_string(),
                direction,
            });
            let proc = Processor::from_transform(&t, TransformDirection::Forward).expect(style);
            let mut cpu = PROBES.to_vec();
            proc.apply_rgba(&mut cpu);
            let gpu = match apply_gpu(&proc, PROBES.len() as u32, 1, &PROBES) {
                Ok(g) => g,
                Err(e) => {
                    failures.push(format!("{style} {direction:?}: GPU refused: {e}"));
                    continue;
                }
            };
            checked += 1;
            let tol = tolerance(style);
            for (k, (c, g)) in cpu.iter().zip(&gpu).enumerate() {
                for ch in 0..3 {
                    if !comparable(style, PROBES[k][ch]) {
                        continue;
                    }
                    let diff = (c[ch] - g[ch]).abs();
                    if diff.is_nan() || diff > tol * c[ch].abs().max(1.0) {
                        failures.push(format!(
                            "{style} {direction:?}: probe {:?} ch{ch}: cpu {} gpu {}",
                            PROBES[k], c[ch], g[ch]
                        ));
                    }
                }
            }
        }
    }
    assert!(
        checked >= 102,
        "only {checked} style/direction pairs ran on the GPU"
    );
    assert!(
        failures.is_empty(),
        "{} GPU/CPU mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
