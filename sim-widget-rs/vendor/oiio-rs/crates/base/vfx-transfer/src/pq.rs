//! SMPTE ST 2084 Perceptual Quantizer (PQ) transfer function.
//!
//! PQ is designed for HDR content, encoding luminance up to 10,000 cd/m2
//! in a perceptually uniform way.
//!
//! # Range
//!
//! - Encoded: [0, 1]
//! - Linear: [0, 10000] cd/m2 (nits)
//!
//! # Reference
//!
//! SMPTE ST 2084:2014
//!
//! # Two copies
//!
//! heif-rs's `heif-transfer` crate has its own copy of this module
//! (`heif-rs/crates/heif-transfer/pq.rs`). heif-rs is a standalone library that must build and
//! work without oiio-rs, and oiio-rs depends on heif-rs, so heif-rs cannot import this crate. The
//! two copies must give identical results: `tests/heif_transfer_parity.rs` compares them bit for
//! bit, domain edges included. Change the curve in both places in the same change. The heif-rs copy
//! is not byte-for-byte: it leaves out the `_fast` variants and the OCIO-only entry points, which
//! heif-rs does not need.
//!
//! # Usage
//!
//! ```rust
//! use vfx_transfer::pq;
//!
//! // Decode PQ signal to absolute luminance
//! let nits = pq::eotf(0.5);
//!
//! // Encode luminance to PQ
//! let signal = pq::oetf(100.0);
//! ```

/// Maximum luminance in cd/m2 (nits).
pub const L_MAX: f32 = 10000.0;

// PQ constants from SMPTE ST 2084. All five are dyadic rationals, exact in f32, and public so
// that shader emitters print these values instead of retyping them.
/// ST 2084 `m1 = 2610/16384`.
pub const M1: f32 = 2610.0 / 16384.0;
/// ST 2084 `m2 = 2523/4096 * 128`.
pub const M2: f32 = 2523.0 / 4096.0 * 128.0;
/// ST 2084 `c1 = 3424/4096` (`= c3 - c2 + 1`).
pub const C1: f32 = 3424.0 / 4096.0;
/// ST 2084 `c2 = 2413/4096 * 32`.
pub const C2: f32 = 2413.0 / 4096.0 * 32.0;
/// ST 2084 `c3 = 2392/4096 * 32`.
pub const C3: f32 = 2392.0 / 4096.0 * 32.0;

/// PQ EOTF: Decodes PQ signal to absolute luminance (cd/m2).
///
/// # Arguments
///
/// * `v` - PQ encoded value [0, 1]
///
/// # Returns
///
/// Absolute luminance in cd/m2 [0, 10000].
///
/// # Example
///
/// ```rust
/// use vfx_transfer::pq::eotf;
///
/// // Reference white (100 nits)
/// let nits = eotf(0.508);
/// assert!((nits - 100.0).abs() < 1.0);
/// ```
#[inline]
pub fn eotf(v: f32) -> f32 {
    eotf_with(v, f32::powf)
}

/// PQ EOTF using [`crate::fast::pow`] — same formula, and it keeps the calling pixel loop
/// vectorised (see that module).
///
/// PQ is the worst case for the approximation in this crate: `1/M1 = 6.28` multiplies the
/// logarithm's error, so the relative error is ~3e-7 rather than ~2e-7. That is still an ulp of
/// f32 and far below the 10/12-bit quantisation of any PQ source.
#[inline]
#[must_use]
pub fn eotf_fast(v: f32) -> f32 {
    eotf_with(v, crate::fast::pow)
}

/// The ST 2084 EOTF — `eotf` and `eotf_fast` differ only in which `pow` they pass in.
///
/// The signal is clamped to ST 2084's `[0, 1]`, the mirror of [`oetf`]'s luminance clamp: `v <= 0`
/// decodes to exactly 0 and `v > 1` to [`L_MAX`]. Unclamped, `den` goes negative once
/// `v > (C2/C3)^M2 ~ 1.98` and the result is NaN.
#[inline(always)]
fn eotf_with(v: f32, powf: impl Fn(f32, f32) -> f32) -> f32 {
    let nits = L_MAX * eotf_ratio(v.min(1.0), powf);
    if v <= 0.0 { 0.0 } else { nits }
}

/// THE PQ decode rational, once: signal `v >= 0` to luminance as a fraction of 10000 cd/m2, with
/// no domain clamp. The inverse of [`oetf_ratio`]. [`eotf`] clamps `v` to `[0, 1]`;
/// [`ocio_pq_to_lin`] does not (OCIO's `Renderer_PQ_TO_LIN` has no clamp either).
///
/// The `max(0)` on the numerator handles the sub-black region `v < c1^m2`; `den` stays positive
/// for `v` in `[0, 1]` (`C2 - C3 * vp` bottoms out at 0.164 when `vp = 1`).
#[inline(always)]
fn eotf_ratio(v: f32, powf: impl Fn(f32, f32) -> f32) -> f32 {
    let vp = powf(v, 1.0 / M2);
    let num = (vp - C1).max(0.0);
    let den = C2 - C3 * vp;
    powf(num / den, 1.0 / M1)
}

/// PQ OETF: Encodes absolute luminance to PQ signal.
///
/// # Arguments
///
/// * `l` - Luminance in cd/m2 [0, 10000]
///
/// # Returns
///
/// PQ encoded value [0, 1].
///
/// # Example
///
/// ```rust
/// use vfx_transfer::pq::oetf;
///
/// // Encode 100 nits (reference white)
/// let signal = oetf(100.0);
/// assert!((signal - 0.508).abs() < 0.01);
/// ```
#[inline]
pub fn oetf(l: f32) -> f32 {
    if l <= 0.0 {
        return 0.0;
    }
    oetf_ratio((l / L_MAX).clamp(0.0, 1.0))
}

/// THE PQ OETF rational, once: `y` is luminance as a fraction of 10000 cd/m2, `y >= 0`, with no
/// domain clamp. [`oetf`] clamps `y` to ST 2084's `[0, 1]` and maps `l <= 0` to exactly 0;
/// [`ocio_lin_to_pq`] does neither.
#[inline(always)]
fn oetf_ratio(y: f32) -> f32 {
    let yp = y.powf(M1);
    let num = C1 + C2 * yp;
    let den = 1.0 + C3 * yp;
    (num / den).powf(M2)
}

/// OpenColorIO's PQ encode: linear where **1.0 = 100 cd/m2**, mirrored about 0, not clamped.
///
/// This is the convention of every OCIO PQ path — the `CURVE - LINEAR_to_ST-2084` builtin and
/// the display builtins built on it (`Displays.cpp` `ST_2084::GenerateLinearToPQOps`) and the
/// `LIN_TO_PQ` FixedFunction (`FixedFunctionOpCPU.cpp:2089-2116`): `N = copysign(PQ(|x|/100), x)`.
/// It differs from [`oetf_normalized`] in three places: no `[0, 10000]` cd/m2 clamp (OCIO lets
/// `N` exceed 1), negatives mirror instead of clamping to 0, and 0 encodes to `c1^m2 ~ 7.4e-7`
/// rather than exactly 0 (OCIO notes that jump is 20x below one 16-bit code value).
#[inline]
pub fn ocio_lin_to_pq(x: f32) -> f32 {
    oetf_ratio((x * 0.01).abs()).copysign(x)
}

/// OpenColorIO's PQ decode, the inverse of [`ocio_lin_to_pq`]: returns linear with 1.0 = 100
/// cd/m2, mirrored about 0 (`Displays.cpp` `ST_2084::GeneratePQToLinearOps`,
/// `FixedFunctionOpCPU.cpp` `Renderer_PQ_TO_LIN`).
#[inline]
pub fn ocio_pq_to_lin(v: f32) -> f32 {
    (100.0 * eotf_ratio(v.abs(), f32::powf)).copysign(v)
}

/// Normalized PQ EOTF: Returns [0, 1] normalized linear.
///
/// Divides absolute luminance by reference white (100 nits).
#[inline]
pub fn eotf_normalized(v: f32) -> f32 {
    eotf(v) / 100.0
}

/// Normalized PQ EOTF built on [`eotf_fast`] — the variant for image-sized loops.
#[inline]
#[must_use]
pub fn eotf_normalized_fast(v: f32) -> f32 {
    eotf_fast(v) / 100.0
}

/// Normalized PQ OETF: Accepts [0, 1] normalized linear.
///
/// Multiplies by reference white (100 nits) before encoding.
#[inline]
pub fn oetf_normalized(l: f32) -> f32 {
    oetf(l * 100.0)
}

/// Applies PQ EOTF to RGB, returning luminance in nits.
#[inline]
pub fn eotf_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [eotf(rgb[0]), eotf(rgb[1]), eotf(rgb[2])]
}

/// Applies PQ OETF to RGB luminance values.
#[inline]
pub fn oetf_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [oetf(rgb[0]), oetf(rgb[1]), oetf(rgb[2])]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let test_values = [0.0, 0.1, 0.5, 1.0, 10.0, 100.0, 1000.0, 10000.0];
        for &l in &test_values {
            let encoded = oetf(l);
            let decoded = eotf(encoded);
            assert!(
                (l - decoded).abs() < l * 0.001 + 0.001,
                "l={}, decoded={}",
                l,
                decoded
            );
        }
    }

    #[test]
    fn test_reference_white() {
        // 100 nits should be around 0.508 in PQ
        let signal = oetf(100.0);
        assert!((signal - 0.508).abs() < 0.01);
    }

    #[test]
    fn test_boundaries() {
        assert_eq!(eotf(0.0), 0.0);
        assert!((eotf(1.0) - L_MAX).abs() < 1.0);
        // The signal clamps to [0, 1]: 3.0 is past the pole at (C2/C3)^M2 ~ 1.98, where an
        // unclamped decode is NaN.
        assert_eq!(eotf(1.5), eotf(1.0));
        assert_eq!(eotf(3.0), eotf(1.0));
        assert_eq!(oetf(0.0), 0.0);
        assert_eq!(oetf(-1.0), 0.0);
    }

    /// The OCIO convention (1.0 = 100 nits, mirrored, unclamped) against PyOpenColorIO 2.5.1's
    /// `CURVE - LINEAR_to_ST-2084` / `CURVE - ST-2084_to_LINEAR` builtins
    /// (`vfx-ocio/tests/data/ocio_builtins_pyocio.json`).
    #[test]
    fn ocio_convention_matches_pyocio() {
        let enc = [
            (0.18, 0.347_966_67),
            (1.5, 0.549_302_04),
            (-0.05, -0.247_847_69),
            (0.02, 0.188_206_99),
        ];
        for (x, want) in enc {
            let got = ocio_lin_to_pq(x);
            let ok = (got - want).abs() <= 1e-4 * want.abs().max(1.0);
            assert!(ok, "lin_to_pq({x}) = {got}, OCIO {want}");
        }
        let dec = [
            (0.18, 0.017_385_814),
            (0.9, 39.056_503),
            (1.5, 31_407.959),
            (-0.05, -0.000_600_016_4),
        ];
        for (v, want) in dec {
            let got = ocio_pq_to_lin(v);
            let ok = (got - want).abs() <= 1e-4 * want.abs().max(1.0);
            assert!(ok, "pq_to_lin({v}) = {got}, OCIO {want}");
        }
        // Past 10000 nits OCIO does not clamp, unlike the ST 2084-domain `oetf`.
        assert!(ocio_lin_to_pq(200.0) > 1.0);
        assert_eq!(oetf(20_000.0), oetf(L_MAX));
    }
}
