//! Numerical helpers for the OCIO processor pipeline.
//!
//! Extracted from `processor.rs` during the STRUCT.2 split (TODO6.md).
//! Pure math (no Processor / ProcessorOp / Config dependencies):
//! - 3x3 matrix inversion (`invert_3x3`)
//! - Transfer-function evaluation (`apply_transfer`)
//! - (LUT inverses live in `inv_lut.rs`: OCIO's exact per-pixel renderers)
//!
//! # No grading-curve kernel lives here — and must not come back
//!
//! This module used to carry its own `compute_bspline_slopes` (Catmull-Rom tangents) and
//! `interpolate_curve_bspline` (a cubic Hermite). Neither was a port of anything: the
//! rustdoc cited `GradingBSplineCurve::computeSlopes`, a function that **does not exist**
//! in the OCIO tree. The reference is a *fitted piecewise-quadratic* spline —
//! `EstimateRGBSlopes` / `FitRGBSpline` / `AdjustRGBSlopes`, evaluated as `(A*t + B)*t + C`
//! over knots that include an inserted ksi per segment
//! (`ops/gradingrgbcurve/GradingBSplineCurve.cpp`).
//!
//! A cubic Hermite through the same knots and slopes is a **different curve family**, so
//! correcting the slope estimation alone could never have reached parity — the evaluation
//! kernel had to go too. `compile.rs` had been building the correct fitted spline for the
//! GPU and then baking the CPU LUT from the Hermite, so CPU and GPU disagreed by
//! construction.
//!
//! The single implementation of the reference machinery is [`crate::grading_bspline`], which
//! the GPU emitters already used and which the CPU apply now evaluates directly (no LUT bake).
//! Its fit runs in `f32`, matching the reference (OCIO's `GradingBSplineCurve` is float), so
//! the CPU evaluation is bit-comparable with the GPU path rather than merely close. The thin wrapper types that
//! briefly bridged this module to it were deleted in `d8ce3c7`, once `compile.rs` stopped
//! re-fitting the spline per sample; anything needing a curve should call `grading_bspline`
//! directly rather than reintroduce a kernel, or a wrapper around one, in this module.
//!
//! All fns are `pub(super)` so the parent `impl Processor` can call them
//! across the file boundary.

use super::TransferStyle;
use vfx_transfer::{
    acescc, acescct, apple_log, bmd_film, bt2020, canon_log, gamma, hlg, log_c, log_c4, pq, rec709,
    red_log, s_log3, srgb, v_log,
};

/// The two half codes bracketing `x` and the fraction between them, for OCIO's half-domain
/// Lut1D. Port of `IndexPair::GetEdgeFloatValues` (`Lut1DOpCPU.cpp:571-620`).
///
/// Returns `(a, b, fraction)` with `x = lerp(half(a), half(b), fraction)`: `a` is the code of
/// smaller magnitude, `b` the next one out (bits grow with magnitude on both sides of zero, so
/// negative inputs index the second half of the table). Infinities and values past `HALF_MAX`
/// clamp to `+/-HALF_MAX`. A NaN input yields fraction 0 on the NaN code, whose table entry
/// OCIO's generators fill with `f(0)`.
pub(crate) fn half_domain_edges(x: f32) -> (usize, usize, f32) {
    use half::f16;
    let mut x = x;
    let mut h = f16::from_f32(x);
    if h.is_infinite() {
        h = if h.is_sign_negative() {
            -f16::MAX
        } else {
            f16::MAX
        };
        x = h.to_f32();
    }
    let (a, mut b);
    // Strict comparison, as the reference: otherwise negative fractions occur.
    if h.to_f32().abs() > x.abs() {
        b = h.to_bits();
        a = b.wrapping_sub(1);
    } else {
        a = h.to_bits();
        b = a.wrapping_add(1);
        let hb = f16::from_bits(b);
        if hb.is_infinite() {
            // e.g. 65519 rounds to HALF_MAX but lies past it: clamp the input too.
            let m = if hb.is_sign_negative() {
                -f16::MAX
            } else {
                f16::MAX
            };
            b = m.to_bits();
            x = m.to_f32();
        }
    }
    let fa = f16::from_bits(a).to_f32();
    let fb = f16::from_bits(b).to_f32();
    let mut frac = (x - fa) / (fb - fa);
    if frac.is_nan() {
        frac = 0.0;
    }
    (a as usize, b as usize, frac)
}

pub(super) fn invert_3x3(m: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

    if det.abs() < 1e-12 {
        return None;
    }

    let inv_det = 1.0 / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ])
}

/// Applies a transfer function to a single value.
/// Delegates to vfx-transfer for the actual math.
pub(super) fn apply_transfer(v: f32, style: TransferStyle, forward: bool) -> f32 {
    match style {
        TransferStyle::Linear => v,

        TransferStyle::Srgb => {
            if forward {
                srgb::oetf(v)
            } else {
                srgb::eotf(v)
            }
        }

        TransferStyle::Rec709 => {
            if forward {
                rec709::oetf(v)
            } else {
                rec709::eotf(v)
            }
        }

        TransferStyle::Rec2020 => {
            // BT.2020 has its own transfer constants (alpha=1.0993, beta=0.0181)
            if forward {
                bt2020::oetf(v)
            } else {
                bt2020::eotf(v)
            }
        }

        TransferStyle::Gamma22 => {
            if forward {
                gamma::gamma_oetf(v, 2.2)
            } else {
                gamma::gamma_eotf(v, 2.2)
            }
        }

        TransferStyle::Gamma24 => {
            if forward {
                gamma::gamma_oetf(v, 2.4)
            } else {
                gamma::gamma_eotf(v, 2.4)
            }
        }

        TransferStyle::Gamma26 => {
            if forward {
                gamma::gamma_oetf(v, 2.6)
            } else {
                gamma::gamma_eotf(v, 2.6)
            }
        }

        TransferStyle::Pq => {
            if forward {
                pq::oetf(v)
            } else {
                pq::eotf(v)
            }
        }

        TransferStyle::Hlg => {
            if forward {
                hlg::oetf(v)
            } else {
                hlg::eotf(v)
            }
        }

        TransferStyle::AcesCct => {
            if forward {
                acescct::encode(v)
            } else {
                acescct::decode(v)
            }
        }

        TransferStyle::AcesCc => {
            if forward {
                acescc::encode(v)
            } else {
                acescc::decode(v)
            }
        }

        TransferStyle::LogC3 => {
            if forward {
                log_c::encode(v)
            } else {
                log_c::decode(v)
            }
        }

        TransferStyle::LogC4 => {
            if forward {
                log_c4::encode(v)
            } else {
                log_c4::decode(v)
            }
        }

        TransferStyle::SLog3 => {
            if forward {
                s_log3::encode(v)
            } else {
                s_log3::decode(v)
            }
        }

        TransferStyle::VLog => {
            if forward {
                v_log::encode(v)
            } else {
                v_log::decode(v)
            }
        }

        TransferStyle::Log3G10 => {
            if forward {
                red_log::log3g10_encode(v)
            } else {
                red_log::log3g10_decode(v)
            }
        }

        TransferStyle::BmdFilmGen5 => {
            if forward {
                bmd_film::bmd_film_gen5_encode(v)
            } else {
                bmd_film::bmd_film_gen5_decode(v)
            }
        }

        TransferStyle::Rec1886 => {
            // Rec.1886: gamma 2.4 for broadcast displays
            if forward {
                gamma::gamma_oetf(v, 2.4)
            } else {
                gamma::gamma_eotf(v, 2.4)
            }
        }

        TransferStyle::AppleLog => {
            if forward {
                apple_log::encode(v)
            } else {
                apple_log::decode(v)
            }
        }

        TransferStyle::CanonCLog2 => {
            if forward {
                canon_log::clog2_encode(v)
            } else {
                canon_log::clog2_decode(v)
            }
        }

        TransferStyle::CanonCLog3 => {
            if forward {
                canon_log::clog3_encode(v)
            } else {
                canon_log::clog3_decode(v)
            }
        }
    }
}

/// A grading RGB curve fitted once, ready to be evaluated many times.
///
#[cfg(test)]
mod curve_tests {
    /// Fit `points` with the reference machinery and return an evaluator.
    ///
    /// These tests were written against a wrapper type in this module, deleted in `d8ce3c7`
    /// once `compile.rs` began evaluating `build_rgb_uniforms` output directly. The wrapper
    /// was only ever a one-curve `build_rgb_uniforms` plus `eval_curve_fwd`, so the tests are
    /// retargeted at that pair rather than dropped — they pin values BETWEEN knots, which is
    /// the discriminating property nothing else covers.
    fn fitted(points: &[[f64; 2]]) -> impl Fn(f64) -> f64 + use<> {
        use crate::grading_bspline::{GradingControlPoint, build_rgb_uniforms};
        let pts: Vec<GradingControlPoint> = points
            .iter()
            .map(|p| GradingControlPoint {
                x: p[0] as f32,
                y: p[1] as f32,
            })
            .collect();
        let empty: [GradingControlPoint; 0] = [];
        let no_slopes: [f32; 0] = [];
        let data = build_rgb_uniforms(
            [&pts, &empty, &empty, &empty],
            [&no_slopes, &no_slopes, &no_slopes, &no_slopes],
        );
        move |x: f64| {
            crate::grading_bspline::eval_curve_fwd(
                0,
                x as f32,
                x as f32,
                &data.knots_offsets,
                &data.coefs_offsets,
                &data.knots,
                &data.coefs,
            ) as f64
        }
    }

    /// Control points of a non-trivial S-curve used across these tests.
    const S_CURVE: [[f64; 2]; 5] = [[0.0, 0.0], [0.25, 0.1], [0.5, 0.5], [0.75, 0.9], [1.0, 1.0]];

    #[test]
    fn curve_passes_through_its_control_points() {
        let c = fitted(&S_CURVE);
        for p in S_CURVE {
            assert!(
                (c(p[0]) - p[1]).abs() < 1e-6,
                "curve must interpolate its control points: f({}) = {}, expected {}",
                p[0],
                c(p[0]),
                p[1]
            );
        }
    }

    /// Pins the fitted-quadratic values between the knots.
    ///
    /// This is the test the suite was missing: swapping the whole evaluation kernel from a
    /// cubic Hermite to the reference's fitted quadratic moved **no** existing test, because
    /// nothing pinned CPU curve output anywhere. Interpolating the control points is not a
    /// discriminating property — every reasonable kernel does that — so these assertions are
    /// deliberately taken between knots, where the families differ.
    #[test]
    fn curve_between_knots_is_the_fitted_quadratic() {
        let c = fitted(&S_CURVE);

        // PyOpenColorIO 2.5.1 (GradingRGBCurveTransform LOG, LOSSLESS CPU). 0.875 lies in the last
        // segment, which the old evaluator got wrong (0.967687, the second-to-last segment).
        for (x, want) in [
            (0.125, 0.018_529),
            (0.375, 0.291_674),
            (0.625, 0.708_326),
            (0.875, 0.981_471),
        ] {
            let got = c(x);
            assert!((got - want).abs() < 1e-5, "f({x}) = {got}, expected {want}");
        }

        // Guard against the two ways this could silently regress to something plausible:
        // a linear interpolant through the knots, or a bypass that returns x unchanged.
        let linear_at_0125 = 0.05; // lerp between (0, 0) and (0.25, 0.1)
        assert!(
            (c(0.125) - linear_at_0125).abs() > 0.02,
            "curve collapsed to linear interpolation between knots"
        );
        assert!(
            (c(0.125) - 0.125).abs() > 0.05,
            "curve collapsed to identity — the fit was bypassed"
        );
    }

    #[test]
    fn identity_curve_stays_identity() {
        let c = fitted(&[[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]]);
        for x in [0.0, 0.3, 0.5, 0.77, 1.0] {
            assert!(
                (c(x) - x).abs() < 1e-6,
                "identity curve moved {x} to {}",
                c(x)
            );
        }
    }
}
