//! Red-gate goldens for camera log transfer curves (findings COLOR-12, COLOR-13).
//!
//! # What this pins
//!
//! Vendor-published curve shape for two camera encodings whose in-crate unit tests are
//! *fitted to the implementation* rather than to the vendor specification:
//!
//! - **COLOR-12** — `vfx-transfer/src/s_log2.rs:26-45` implements **S-Log1**, not S-Log2. It
//!   omits the `155/219` input scale that distinguishes the two curves, and instead carries an
//!   unsourced `/1.03` output divisor that appears in no Sony document.
//! - **COLOR-13** — `vfx-transfer/src/d_log.rs:26-31` derives its linear-tail from a
//!   C1-continuity condition rather than using the values DJI publishes.
//!
//! # Why the existing tests miss it
//!
//! Both modules test *round-trip* (`encode` then `decode`) and *self-consistency* (monotonicity,
//! break-point agreement). A round-trip is blind to any defect the forward and inverse share —
//! and here they share the whole curve. `s_log2.rs`'s `test_middle_gray` asserts
//! `0.34 < encode(0.18) < 0.38`, a window derived by *reading the implementation's output*
//! (0.3495). The correct S-Log2 value, 0.30798, falls outside that window. The test is green
//! and wrong; its bounds must not be reused.
//!
//! # Expected to fail
//!
//! These tests encode correct behaviour and are **expected to fail** until COLOR-12 and
//! COLOR-13 are repaired. Do not relax a tolerance to make one pass.

use vfx_transfer::{d_log, s_log2};

/// Absolute tolerance for a code value in [0,1]. Tight enough that a curve-shape error cannot
/// hide, loose enough to absorb f32 rounding through a `log10`/`powf` pair.
const TOL: f32 = 1e-4;

fn assert_close(actual: f32, expected: f32, tol: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{what}: actual={actual:.6}, expected={expected:.6}, delta={:.6} (tol={tol})",
        (actual - expected).abs()
    );
}

// ---------------------------------------------------------------------------------------------
// COLOR-12 — Sony S-Log2
// ---------------------------------------------------------------------------------------------
//
// SOURCE — Sony "S-Log2 Technical Summary" (S-Log2 gamma curve, reflected in the ACES IDT
// `ACEScsc.Sony_SLog2_SGamut_to_ACES.ctl`). Forward, for scene-linear reflectance `x` where
// x = 0.18 is an 18% grey card:
//
//     x >= 0 :  y = 0.432699 * log10(155.0 * x / 219.0 + 0.037584) + 0.616596 + 0.03
//     x <  0 :  y = 3.53881278538813 * x + 0.030001222851889303
//
// The `155/219` factor is the S-Log2 headroom rescale and is exactly what separates S-Log2 from
// S-Log1; there is no `/1.03` term anywhere in the specification.
//
// PROVENANCE CHECK performed here rather than asserted on faith — two independent confirmations
// that the constants above belong together:
//
//   1. Evaluating the log branch at x = 0 gives 0.432699*log10(0.037584) + 0.646596
//      = 0.030001222851889303, reproducing the published linear-branch intercept to all
//      17 digits. A transcription error could not do that.
//   2. d/dx of the log branch at x = 0 is
//      0.432699 * log10(e) * (155/219) / 0.037584 = 3.538790, matching the published linear
//      slope 3.538813 to 7e-6 — i.e. the vendor pair is C1-continuous, so no separate source is
//      needed for the negative extension.
//
// Golden values below are that formula evaluated at f64 and rounded to 6 places.

/// 18% grey. The canonical published S-Log2 anchor: **0.3080**.
///
/// Current code returns ≈0.3495 because it lacks the `155/219` input scale and divides by 1.03.
#[test]
fn red_gate_slog2_is_slog1_color_12_middle_gray() {
    assert_close(s_log2::encode(0.18), 0.307981, TOL, "S-Log2 encode(0.18)");
}

/// Curve shape across the working range. A single anchor could in principle be matched by a
/// wrong curve; five cannot.
#[test]
fn red_gate_slog2_is_slog1_color_12_curve_shape() {
    // (linear, expected code value) — 0.432699*log10(155*x/219 + 0.037584) + 0.646596
    let goldens = [
        (0.01_f32, 0.062424_f32),
        (0.18, 0.307981),
        (0.90, 0.572616),
        (1.00, 0.591366),
        (2.00, 0.716822),
    ];
    for (linear, expected) in goldens {
        assert_close(
            s_log2::encode(linear),
            expected,
            TOL,
            &format!("S-Log2 encode({linear})"),
        );
    }
}

/// At linear 0 the curve must reach the published intercept 0.030001222851889303.
///
/// The implementation instead returns `CODE_BREAK / 1.03` ≈ 0.029128 — its own `test_zero`
/// asserts exactly that, which is why the defect survived: the test restates the code.
#[test]
fn red_gate_slog2_is_slog1_color_12_black_intercept() {
    assert_close(
        s_log2::encode(0.0),
        0.030001223,
        TOL,
        "S-Log2 encode(0.0) (published intercept)",
    );
}

/// `decode` must invert the *correct* curve, not merely invert `encode`. Anchored on the
/// published code value, so it fails even though the implementation's own round-trip passes.
#[test]
fn red_gate_slog2_is_slog1_color_12_decode_anchor() {
    assert_close(
        s_log2::decode(0.307981),
        0.18,
        1e-3,
        "S-Log2 decode(0.307981) (18% grey code value)",
    );
}

/// Negative-slope C1 continuity, checked as a *relation* rather than against a literal, so this
/// assertion carries no unsourced constant of its own.
#[test]
fn red_gate_slog2_is_slog1_color_12_negative_slope() {
    // Published linear slope, equal to the analytic derivative of the log branch at 0 (see the
    // provenance note above). Measured as a finite difference over the negative extension.
    let measured = (s_log2::encode(-0.001) - s_log2::encode(-0.003)) / 0.002;
    assert_close(measured, 3.5388128, 1e-3, "S-Log2 negative-extension slope");
}

// ---------------------------------------------------------------------------------------------
// COLOR-13 — DJI D-Log
// ---------------------------------------------------------------------------------------------
//
// SOURCE — DJI "Cinema Color System" white paper (D-Log / D-Gamut), forward transform:
//
//     x >  0.0078 :  y = 0.256663 * log10(x * 0.9892 + 0.0108) + 0.584555
//     x <= 0.0078 :  y = 6.025 * x + 0.0929
//
// `d_log.rs` keeps the log branch verbatim but discards the published linear branch, rebuilding
// it from a C1-continuity condition at the break; the plan records the result as
// `5.955*x + 0.09346`.
//
// PROVENANCE CHECK: the two published branches meet at the break to within 2e-6 —
//   log branch  @ 0.0078 = 0.256663*log10(0.0078*0.9892 + 0.0108) + 0.584555 = 0.139897
//   linear      @ 0.0078 = 6.025*0.0078 + 0.0929                             = 0.139895
// so the vendor pair is C0-consistent and the published constants are mutually corroborating.
// (It is *not* C1-consistent — the log branch's tangent at the break is ≈6.224, not 6.025 —
// which is precisely why deriving the tail instead of reading it off the spec drifts.)

/// Black level. DJI publishes 95/1023 ≈ 0.0929; the C1-derived tail lifts it to ≈0.0935.
#[test]
fn red_gate_dlog_linear_tail_color_13_black() {
    assert_close(d_log::encode(0.0), 0.0929, TOL, "D-Log encode(0.0)");
}

/// Two points inside the linear tail. Both lie strictly below the 0.0078 break, so they probe
/// the published branch and nothing else.
#[test]
fn red_gate_dlog_linear_tail_color_13_slope() {
    let goldens = [
        (0.0_f32, 0.0929_f32),
        (0.002, 0.104950),
        (0.004, 0.117000),
        (0.0078, 0.139895),
    ];
    for (linear, expected) in goldens {
        assert_close(
            d_log::encode(linear),
            expected,
            TOL,
            &format!("D-Log encode({linear})"),
        );
    }
}

/// CONTROL — the log branch is untouched by COLOR-13 and is **expected to pass**. It is here to
/// prove the failures above are localised to the linear tail rather than a global miscalibration.
#[test]
fn control_dlog_log_branch_matches_vendor() {
    let goldens = [(0.18_f32, 0.398765_f32), (0.50, 0.508489), (0.90, 0.572944)];
    for (linear, expected) in goldens {
        assert_close(
            d_log::encode(linear),
            expected,
            1e-3,
            &format!("D-Log encode({linear}) [control]"),
        );
    }
}
