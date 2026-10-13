//! Gamma transfer functions (pure and moncurve).
//!
//! Simple gamma curves and moncurve (gamma with linear segment).
//!
//! ## Pure Gamma
//! - 2.2: Legacy CRT approximation
//! - 2.4: BT.1886 reference EOTF
//! - 2.6: DCI theatrical projection
//!
//! ## Moncurve
//! Gamma with linear segment near black (ExponentWithLinear style).
//! Parameters: gamma (exponent >= 1) and offset (0 to 0.9).
//!
//! The moncurve is the port of OCIO's `GammaOpUtils.cpp` and the one moncurve of the stack
//! (see the section below).
//!
//! # Range
//!
//! - Pure gamma mirrors negatives; the moncurve takes them on its linear segment (OCIO's
//!   default), `moncurve_mirror_*` mirrors them
//!
//! # Two copies
//!
//! heif-rs's `heif-transfer` crate has its own copy of this module
//! (`heif-rs/crates/heif-transfer/gamma.rs`). heif-rs is a standalone library that must build and
//! work without oiio-rs, and oiio-rs depends on heif-rs, so heif-rs cannot import this crate. The
//! two copies must give identical results: `tests/heif_transfer_parity.rs` compares them bit for
//! bit, domain edges included. Change the curve in both places in the same change. The heif-rs copy
//! is not byte-for-byte: it leaves out the `_fast` variants and the OCIO-only entry points, which
//! heif-rs does not need.

/// EOTF for arbitrary gamma: `v^gamma`
///
/// # Example
///
/// ```rust
/// use vfx_transfer::gamma::gamma_eotf;
///
/// let linear = gamma_eotf(0.5, 2.2);
/// ```
#[inline]
pub fn gamma_eotf(v: f32, gamma: f32) -> f32 {
    if v < 0.0 {
        // Mirror mode: -f(-x) for negative input
        -(-v).powf(gamma)
    } else {
        v.powf(gamma)
    }
}

/// OETF for arbitrary gamma: `l^(1/gamma)`
///
/// # Example
///
/// ```rust
/// use vfx_transfer::gamma::gamma_oetf;
///
/// let encoded = gamma_oetf(0.218, 2.2);
/// assert!((encoded - 0.5).abs() < 0.01);
/// ```
#[inline]
pub fn gamma_oetf(l: f32, gamma: f32) -> f32 {
    if l < 0.0 {
        // Mirror mode: -f(-x) for negative input
        -(-l).powf(1.0 / gamma)
    } else {
        l.powf(1.0 / gamma)
    }
}

/// Gamma 2.2 EOTF.
#[inline]
pub fn eotf_22(v: f32) -> f32 {
    gamma_eotf(v, 2.2)
}

/// Gamma 2.2 OETF.
#[inline]
pub fn oetf_22(l: f32) -> f32 {
    gamma_oetf(l, 2.2)
}

/// Gamma 2.4 EOTF (BT.1886 reference).
#[inline]
pub fn eotf_24(v: f32) -> f32 {
    gamma_eotf(v, 2.4)
}

/// Gamma 2.4 OETF.
#[inline]
pub fn oetf_24(l: f32) -> f32 {
    gamma_oetf(l, 2.4)
}

/// Gamma 2.6 EOTF (DCI theatrical).
#[inline]
pub fn eotf_26(v: f32) -> f32 {
    gamma_eotf(v, 2.6)
}

/// Gamma 2.6 OETF.
#[inline]
pub fn oetf_26(l: f32) -> f32 {
    gamma_oetf(l, 2.6)
}

// ============================================================================
// Moncurve (gamma with linear segment): THE moncurve of the whole stack
// ============================================================================
//
// This is the port of OCIO's `GammaOpUtils.cpp` `ComputeParamsFwd`/`ComputeParamsRev` (the
// maths in f64, narrowed once) and of `GammaMoncurveOpCPUFwd/Rev::apply`, and it is the ONLY
// moncurve in the stack: vfx-ocio's `ExponentWithLinearTransform` (CPU apply and the GPU
// emitters) takes its parameters from here, so HEIF/AVIF transfer code and OCIO transforms agree
// numerically with OCIO. heif-rs's `heif-transfer` carries a copy of this file because
// heif-rs is standalone and upstream of oiio-rs (see the module docs, "Two copies");
// `vfx-transfer/tests/heif_transfer_parity.rs` holds the two copies bit-for-bit equal.

/// Renderer parameters of the moncurve for one channel, resolved for one direction.
///
/// Forward (OCIO `MONCURVE_FWD`, encoded -> linear): `x <= break_pnt ? x * slope :
/// pow(x * scale + offset, gamma)`. Reverse (linear -> encoded): `x <= break_pnt ? x * slope :
/// pow(x, gamma) * scale - offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoncurveParams {
    /// Exponent (the reciprocal of the user gamma in the reverse direction).
    pub gamma: f32,
    /// Offset of the power segment.
    pub offset: f32,
    /// Break point between the linear and the power segment.
    pub break_pnt: f32,
    /// Slope of the linear segment.
    pub slope: f32,
    /// Scale of the power segment.
    pub scale: f32,
}

/// OCIO `ComputeParamsFwd` (`forward`) / `ComputeParamsRev` for a user `gamma` (>= 1) and
/// `offset`, with OCIO's guards against the division by zero at gamma 1 / offset 0.
///
/// Why this exact form: it is the port of OCIO's `GammaOpUtils.cpp` `ComputeParamsFwd/Rev`, and
/// the whole stack uses this one formula (vfx-transfer, vfx-ocio's `ExponentWithLinearTransform`
/// on the CPU and GPU, heif-rs), so HEIF/AVIF transfer code matches OCIO numerically. heif-rs's
/// `heif-transfer` holds a copy of this file because heif-rs is standalone and upstream of
/// oiio-rs (see the module docs, "Two copies"); `vfx-transfer/tests/heif_transfer_parity.rs` guards
/// the two copies bit-for-bit.
pub fn moncurve_params(gamma: f64, offset: f64, forward: bool) -> MoncurveParams {
    const EPS: f64 = 1e-6;
    let g = gamma.max(1.0 + EPS);
    let off = offset.max(EPS);
    let a = (g - 1.0) / off;
    if forward {
        let b = off * g / ((g - 1.0) * (1.0 + off));
        MoncurveParams {
            gamma: g as f32,
            offset: (off / (1.0 + off)) as f32,
            break_pnt: (off / (g - 1.0)) as f32,
            slope: (a * b.powf(g)) as f32,
            scale: (1.0 / (1.0 + off)) as f32,
        }
    } else {
        let b = (1.0 + off) / g;
        MoncurveParams {
            gamma: (1.0 / g) as f32,
            offset: off as f32,
            break_pnt: ((off * g) / ((g - 1.0) * (1.0 + off))).powf(g) as f32,
            slope: (a.powf(g - 1.0) * b.powf(g)) as f32,
            scale: (1.0 + off) as f32,
        }
    }
}

/// [`moncurve_params`] forward, for `f32` arguments.
pub fn moncurve_params_fwd(gamma: f32, offset: f32) -> MoncurveParams {
    moncurve_params(gamma as f64, offset as f64, true)
}

/// [`moncurve_params`] reverse, for `f32` arguments.
pub fn moncurve_params_rev(gamma: f32, offset: f32) -> MoncurveParams {
    moncurve_params(gamma as f64, offset as f64, false)
}

/// One channel of the moncurve (`GammaMoncurveOpCPUFwd/Rev::apply`). Values at or below the
/// break, negatives included, take the linear segment (OCIO's default `LINEAR` negative style).
#[inline]
pub fn moncurve_eval(x: f32, p: &MoncurveParams, forward: bool) -> f32 {
    if x <= p.break_pnt {
        x * p.slope
    } else if forward {
        (x * p.scale + p.offset).powf(p.gamma)
    } else {
        x.powf(p.gamma) * p.scale - p.offset
    }
}

/// Moncurve forward (OCIO `MONCURVE_FWD`, encoded -> linear): `pow((x + offset) / (1 + offset),
/// gamma)` above the break, linear below it (negatives included).
///
/// # Example
///
/// ```rust
/// use vfx_transfer::gamma::moncurve_fwd;
///
/// // With gamma=2.2, offset=0.1
/// let linear = moncurve_fwd(0.5, 2.2, 0.1);
/// assert!(linear > 0.0 && linear < 0.5);
/// ```
#[inline]
pub fn moncurve_fwd(x: f32, gamma: f32, offset: f32) -> f32 {
    moncurve_eval(x, &moncurve_params_fwd(gamma, offset), true)
}

/// Moncurve reverse (OCIO `MONCURVE_REV`, linear -> encoded), the inverse of [`moncurve_fwd`].
///
/// # Example
///
/// ```rust
/// use vfx_transfer::gamma::moncurve_rev;
///
/// let encoded = moncurve_rev(0.5, 2.2, 0.1);
/// assert!(encoded > 0.5 && encoded < 1.0);
/// ```
#[inline]
pub fn moncurve_rev(y: f32, gamma: f32, offset: f32) -> f32 {
    moncurve_eval(y, &moncurve_params_rev(gamma, offset), false)
}

/// Moncurve forward with mirrored negatives (OCIO `MONCURVE_MIRROR_FWD`): `sign(x) * f(|x|)`.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::gamma::moncurve_mirror_fwd;
///
/// let pos = moncurve_mirror_fwd(0.5, 2.2, 0.1);
/// let neg = moncurve_mirror_fwd(-0.5, 2.2, 0.1);
/// assert!((pos + neg).abs() < 1e-6);
/// ```
#[inline]
pub fn moncurve_mirror_fwd(x: f32, gamma: f32, offset: f32) -> f32 {
    let r = moncurve_fwd(x.abs(), gamma, offset);
    if x < 0.0 { -r } else { r }
}

/// Moncurve reverse with mirrored negatives (OCIO `MONCURVE_MIRROR_REV`).
///
/// # Example
///
/// ```rust
/// use vfx_transfer::gamma::moncurve_mirror_rev;
///
/// let pos = moncurve_mirror_rev(0.5, 2.2, 0.1);
/// let neg = moncurve_mirror_rev(-0.5, 2.2, 0.1);
/// assert!((pos + neg).abs() < 1e-6);
/// ```
#[inline]
pub fn moncurve_mirror_rev(y: f32, gamma: f32, offset: f32) -> f32 {
    let r = moncurve_rev(y.abs(), gamma, offset);
    if y < 0.0 { -r } else { r }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gamma22_roundtrip() {
        for i in 0..=100 {
            let v = i as f32 / 100.0;
            let linear = eotf_22(v);
            let back = oetf_22(linear);
            assert!((v - back).abs() < 1e-5);
        }
    }

    #[test]
    fn test_gamma_identity() {
        // gamma 1.0 should be identity
        assert_eq!(gamma_eotf(0.5, 1.0), 0.5);
        assert_eq!(gamma_oetf(0.5, 1.0), 0.5);
    }

    #[test]
    fn test_moncurve_roundtrip() {
        // OCIO-compatible parameters
        let gamma = 2.2;
        let offset = 0.1;

        for i in 0..=100 {
            let x = i as f32 / 100.0;
            let encoded = moncurve_fwd(x, gamma, offset);
            let back = moncurve_rev(encoded, gamma, offset);
            assert!(
                (x - back).abs() < 1e-4,
                "x={}, encoded={}, back={}",
                x,
                encoded,
                back
            );
        }
    }

    #[test]
    fn test_moncurve_mirror_roundtrip() {
        let gamma = 2.2;
        let offset = 0.1;

        for i in -100..=100 {
            let x = i as f32 / 100.0;
            let encoded = moncurve_mirror_fwd(x, gamma, offset);
            let back = moncurve_mirror_rev(encoded, gamma, offset);
            assert!((x - back).abs() < 1e-4, "x={}, back={}", x, back);
        }
    }

    #[test]
    fn test_moncurve_negative_mirrored() {
        let gamma = 2.2;
        let offset = 0.1;

        let pos = moncurve_mirror_fwd(0.5, gamma, offset);
        let neg = moncurve_mirror_fwd(-0.5, gamma, offset);
        assert!(
            (pos + neg).abs() < 1e-6,
            "should mirror: {} vs {}",
            pos,
            neg
        );
    }

    #[test]
    fn test_moncurve_continuity() {
        // Check continuity at break point
        let gamma = 2.2;
        let offset = 0.1;
        let p = moncurve_params_fwd(gamma, offset);

        let below = moncurve_fwd(p.break_pnt - 1e-6, gamma, offset);
        let above = moncurve_fwd(p.break_pnt + 1e-6, gamma, offset);
        assert!(
            (below - above).abs() < 1e-4,
            "discontinuity at break: {} vs {}",
            below,
            above
        );
    }

    #[test]
    fn test_moncurve_monotonic() {
        let gamma = 2.2;
        let offset = 0.1;

        let mut prev = 0.0;
        for i in 1..=100 {
            let x = i as f32 / 100.0;
            let y = moncurve_fwd(x, gamma, offset);
            assert!(y > prev, "not monotonic at x={}: {} <= {}", x, y, prev);
            prev = y;
        }
    }

    /// The moncurve is OCIO's: PyOpenColorIO 2.5.1 `ExponentWithLinearTransform` (OPTIMIZATION_NONE)
    /// forward and inverse, LINEAR and MIRROR negatives, three (gamma, offset) pairs. `moncurve_fwd`
    /// used to mirror negatives (-0.0574 at -0.2 for 2.2/0.1; OCIO's LINEAR gives -0.0466) and
    /// derived its parameters in f32.
    #[test]
    fn moncurve_matches_pyocio() {
        let go = [(2.2_f32, 0.1_f32), (2.4, 0.055), (2.6, 0.2)];
        #[rustfmt::skip]
        let fwd = [
            [0.26355353_f32, 0.21404114, 0.2462542],
            [0.004658847, 0.0015476032, 0.0053597614],
            [-0.046588477, -0.015476032, -0.053597618],
        ];
        #[rustfmt::skip]
        let rev = [
            [0.702714_f32, 0.7353569, 0.71917987],
            [0.085837595, 0.15170372, 0.074630186],
            [-0.8585815, -2.5846422, -0.7463018],
        ];
        let mirror_fwd_neg = [-0.05735917_f32, -0.03310476, -0.05747577];
        let mirror_rev_neg = [-0.42927215_f32, -0.4845292, -0.4461698];
        let close = |a: f32, b: f32| (a - b).abs() <= 2e-7 * b.abs().max(1.0);
        for (c, &(g, o)) in go.iter().enumerate() {
            for (row, x) in [0.5_f32, 0.02, -0.2].into_iter().enumerate() {
                assert!(close(moncurve_fwd(x, g, o), fwd[row][c]), "fwd {x} {g}/{o}");
                assert!(close(moncurve_rev(x, g, o), rev[row][c]), "rev {x} {g}/{o}");
            }
            assert!(close(moncurve_mirror_fwd(-0.2, g, o), mirror_fwd_neg[c]));
            assert!(close(moncurve_mirror_rev(-0.2, g, o), mirror_rev_neg[c]));
        }
    }
}
