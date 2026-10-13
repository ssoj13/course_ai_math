//! The `_fast` transfer curves must BE their exact definitions, to a stated bound.
//!
//! `vfx_transfer::fast` trades a call to libm for a polynomial so that pixel loops vectorise.
//! That trade is only acceptable while the answer is indistinguishable, so this file measures
//! the difference rather than asserting it — and it also pins the parts of each curve's
//! definition that a branch-free rewrite is most likely to lose: the sign convention below
//! zero and the clamp at zero.

use vfx_transfer::{hlg, pq, rec709, srgb};

/// Worst relative difference between two curves over `[-0.1, 1.1]`.
///
/// The domain runs past both ends on purpose: studio-swing footage legitimately carries
/// sub-black and super-white code values, and a curve that is only right inside `[0, 1]` is a
/// curve that clips real footage. The relative measure is floored by the curve's own output
/// scale so the near-black region, where every curve tends to zero, cannot dominate it.
fn worst_rel_over(fast: fn(f32) -> f32, exact: fn(f32) -> f32, lo: f32, hi: f32) -> (f32, f32) {
    const N: usize = 200_000;
    let (mut worst, mut at) = (0.0f32, 0.0f32);
    for i in 0..=N {
        let x = lo + (hi - lo) * i as f32 / N as f32;
        let (a, b) = (fast(x), exact(x));
        let rel = (a - b).abs() / b.abs().max(1e-3);
        if rel > worst {
            worst = rel;
            at = x;
        }
    }
    (worst, at)
}

fn worst_rel(fast: fn(f32) -> f32, exact: fn(f32) -> f32) -> (f32, f32) {
    worst_rel_over(fast, exact, -0.1, 1.1)
}

/// What "accurate enough" means here, once.
///
/// ~1e-6 relative is about eight f32 ulp, and it is a FLOOR, not sloppiness: a `pow` is
/// `exp2(y * log2(x))`, and f32 cannot hold `log2(x)` to better than one ulp of its integer part
/// (`ulp(4) = 4.8e-7`), which the exponent `y` then multiplies. Buying more would mean carrying
/// the logarithm in two f32 halves the way Cephes' `powf` does — ten more operations per sample
/// to move an error that is already **three orders of magnitude below the quantisation step of a
/// 10-bit source** (1/1024). The exact curves remain one call away for anyone who needs them.
const POW_TOL: f32 = 1e-6;
/// The `exp`-based curves avoid the logarithm entirely, so they hold ~3 ulp.
const EXP_TOL: f32 = 5e-7;

#[test]
fn srgb_fast_matches_exact() {
    let (e, at) = worst_rel(srgb::eotf_fast, srgb::eotf);
    assert!(e < POW_TOL, "sRGB: relative error {e:e} at x={at}");
}

#[test]
fn rec709_fast_matches_exact() {
    let (e, at) = worst_rel(rec709::eotf_fast, rec709::eotf);
    assert!(e < POW_TOL, "Rec.709: relative error {e:e} at x={at}");
}

#[test]
fn hlg_fast_matches_exact() {
    let (e, at) = worst_rel(hlg::eotf_fast, hlg::eotf);
    assert!(e < EXP_TOL, "HLG: relative error {e:e} at x={at}");
}

/// PQ in `f64`, written straight from SMPTE ST 2084 — an oracle deliberately independent of the
/// crate, so it can disagree with it.
fn pq_eotf_f64(v: f64) -> f64 {
    if v <= 0.0 {
        return 0.0;
    }
    const M1: f64 = 2610.0 / 16384.0;
    const M2: f64 = 2523.0 / 4096.0 * 128.0;
    const C1: f64 = 3424.0 / 4096.0;
    const C2: f64 = 2413.0 / 4096.0 * 32.0;
    const C3: f64 = 2392.0 / 4096.0 * 32.0;
    let vp = v.powf(1.0 / M2);
    let num = (vp - C1).max(0.0);
    let den = C2 - C3 * vp;
    10000.0 * (num / den).powf(1.0 / M1)
}

/// PQ is measured against `f64`, not against its own `f32` self, because comparing the two f32
/// curves to each other says nothing here.
///
/// PQ's denominator is `C2 - C3 * v'` = `18.8516 - 18.6875 * v'`, which is 0.164 at `v' = 1` — a
/// subtraction that has already cancelled two decimal digits away. ANY perturbation of `v'`,
/// including the exact f32 version's OWN final rounding, is amplified ~230x there and then
/// raised to `1/M1 = 6.28`. So near white both f32 curves are off by ~1e-5 from the true value,
/// and their difference from each other is not evidence about either one.
///
/// The claim this gate makes is therefore the one that matters: **the approximation is not
/// materially worse than the exact f32 curve it replaces.** Measured, the two are byte-equal
/// (both 5.589e-5 against f64) — the approximation contributes nothing to PQ's error, the curve
/// does. A factor of 4 is the line.
///
/// **What this test CANNOT see, on purpose:** the amplification is ~150x at v=0.1 and ~1000x
/// near white, so it swamps the polynomial across the WHOLE domain — verified by mutation, a
/// `fast::exp2` coefficient perturbed by 2e-5 still passes here. There is no sub-range of PQ
/// where an f32 curve is well conditioned, so no strict bound is available at this level and
/// pretending otherwise would be the vacuous test. Sensitivity to a wrong polynomial lives one
/// level down, in `fast::tests::pow_survives_the_exponents_the_transfer_curves_use`, which gates
/// `pow` at PQ's own exponents (1/78.84 and 1/0.1593) where nothing amplifies it.
#[test]
fn pq_fast_is_no_worse_than_the_exact_f32_curve() {
    const N: usize = 100_000;
    let (mut worst_fast, mut worst_exact, mut at) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..=N {
        let x = -0.1 + 1.1 * i as f64 / N as f64;
        let truth = pq_eotf_f64(x);
        let denom = truth.abs().max(1e-3);
        let ef = (f64::from(pq::eotf_fast(x as f32)) - truth).abs() / denom;
        let ee = (f64::from(pq::eotf(x as f32)) - truth).abs() / denom;
        if ef > worst_fast {
            worst_fast = ef;
            at = x;
        }
        worst_exact = worst_exact.max(ee);
    }
    println!("PQ vs f64: fast {worst_fast:e} (at {at}), exact f32 {worst_exact:e}");
    assert!(
        worst_fast <= 4.0 * worst_exact.max(1e-7),
        "PQ fast {worst_fast:e} is more than 4x the exact f32 curve's own error {worst_exact:e} \
         (worst at x={at})"
    );
}

/// The normalized wrapper is only a divide by 100, so it must not move the error at all.
#[test]
fn pq_normalized_tracks_pq() {
    for i in 0..=1000 {
        let x = i as f32 / 1000.0;
        let a = pq::eotf_normalized_fast(x);
        let b = pq::eotf_fast(x) / 100.0;
        assert_eq!(a, b, "normalized must be eotf/100 at {x}");
    }
}

/// Below zero the curves do two DIFFERENT things, and both are load-bearing.
///
/// sRGB and Rec.709 are odd (mirror: `-f(-x)`) so a negative sample stays negative — that is
/// what keeps out-of-gamut values recoverable through a round trip. HLG and PQ are defined on
/// `[0, 1]` and clamp. A branch-free rewrite that evaluates both segments and selects is
/// exactly where this gets inverted: `ep * ep / 3` is POSITIVE for negative `ep`, so losing
/// HLG's zero guard turns a slightly-negative sample into a positive one.
#[test]
fn the_sign_convention_survives_the_branch_free_rewrite() {
    for x in [-0.5f32, -0.2, -0.05, -0.001] {
        assert!(
            srgb::eotf_fast(x) < 0.0,
            "sRGB must mirror below zero, got {} at {x}",
            srgb::eotf_fast(x)
        );
        assert!(
            (srgb::eotf_fast(x) + srgb::eotf_fast(-x)).abs() < 1e-7,
            "sRGB must be odd at {x}"
        );
        assert!(
            rec709::eotf_fast(x) < 0.0,
            "Rec.709 must mirror below zero, got {} at {x}",
            rec709::eotf_fast(x)
        );
        assert_eq!(hlg::eotf_fast(x), 0.0, "HLG must clamp below zero at {x}");
        assert_eq!(pq::eotf_fast(x), 0.0, "PQ must clamp below zero at {x}");
    }
}

/// The white point must come through unchanged. A transfer that shifts 1.0 by an ulp shows up
/// as a tint on the one value every image contains, and an approximation is the obvious way to
/// introduce it — `pow(1, k)` has to be exactly 1, not 0.9999999.
#[test]
fn the_white_point_is_exact() {
    assert_eq!(srgb::eotf_fast(1.0), 1.0);
    assert_eq!(rec709::eotf_fast(1.0), 1.0);
    assert_eq!(srgb::eotf_fast(0.0), 0.0);
    assert_eq!(rec709::eotf_fast(0.0), 0.0);
    assert_eq!(hlg::eotf_fast(0.0), 0.0);
    assert_eq!(pq::eotf_fast(0.0), 0.0);
    // HLG's own white: BT.2100 puts E' = 1 at linear 1.0.
    assert!((hlg::eotf_fast(1.0) - 1.0).abs() < 1e-6, "HLG white");
}
