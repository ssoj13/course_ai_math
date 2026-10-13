//! BT.2020 (Rec.2020) transfer function.
//!
//! BT.2020 defines its own OETF distinct from BT.709, using different
//! constants (alpha, beta) that provide better precision for 10/12-bit
//! encoding.
//!
//! # Range
//!
//! - Input/Output: [0, 1]
//!
//! # Reference
//!
//! ITU-R BT.2020-2, Table 4
//!
//! # Two copies
//!
//! heif-rs's `heif-transfer` crate has its own copy of this module
//! (`heif-rs/crates/heif-transfer/bt2020.rs`). heif-rs is a standalone library that must build and
//! work without oiio-rs, and oiio-rs depends on heif-rs, so heif-rs cannot import this crate. The
//! two copies must give identical results: `tests/heif_transfer_parity.rs` compares them bit for
//! bit, domain edges included. Change the curve in both places in the same change. The heif-rs copy
//! is not byte-for-byte: it leaves out the `_fast` variants and the OCIO-only entry points, which
//! heif-rs does not need.

/// Alpha constant for 10-bit BT.2020.
const ALPHA: f32 = 1.099_296_8;

/// Beta threshold: linear values below this use linear segment.
const BETA: f32 = 0.018_053_97;

/// Linear segment slope: 4.5.
const LIN_SLOPE: f32 = 4.5;

/// Power exponent for the nonlinear segment.
const POWER: f32 = 0.45;

/// BT.2020 OETF: Encodes linear to BT.2020.
///
/// # Formula
///
/// ```text
/// if L >= beta:
///     V = alpha * L^0.45 - (alpha - 1)
/// else:
///     V = 4.5 * L
/// ```
#[inline]
pub fn oetf(l: f32) -> f32 {
    if l < 0.0 {
        return -oetf(-l);
    }
    if l >= BETA {
        ALPHA * l.powf(POWER) - (ALPHA - 1.0)
    } else {
        LIN_SLOPE * l
    }
}

/// BT.2020 inverse OETF (EOTF): Decodes BT.2020 to linear.
///
/// Note: For display, BT.2020 recommends BT.1886 (gamma 2.4).
/// This is the mathematical inverse of the OETF.
#[inline]
pub fn eotf(v: f32) -> f32 {
    if v < 0.0 {
        return -eotf(-v);
    }
    // Threshold in code domain: 4.5 * beta
    let code_break = LIN_SLOPE * BETA;
    if v >= code_break {
        ((v + (ALPHA - 1.0)) / ALPHA).powf(1.0 / POWER)
    } else {
        v / LIN_SLOPE
    }
}

/// Applies BT.2020 OETF to an RGB triplet.
#[inline]
pub fn oetf_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [oetf(rgb[0]), oetf(rgb[1]), oetf(rgb[2])]
}

/// Applies BT.2020 EOTF to an RGB triplet.
#[inline]
pub fn eotf_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [eotf(rgb[0]), eotf(rgb[1]), eotf(rgb[2])]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        for i in 0..=100 {
            let v = i as f32 / 100.0;
            let encoded = oetf(v);
            let decoded = eotf(encoded);
            assert!(
                (v - decoded).abs() < 1e-4,
                "v={}, encoded={}, decoded={}, diff={}",
                v,
                encoded,
                decoded,
                (v - decoded).abs()
            );
        }
    }

    #[test]
    fn test_boundaries() {
        assert_eq!(oetf(0.0), 0.0);
        assert!((oetf(1.0) - 1.0).abs() < 1e-6);
        assert_eq!(eotf(0.0), 0.0);
        assert!((eotf(1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_differs_from_rec709() {
        // BT.2020 uses different constants than BT.709
        // At the threshold, the behavior should differ
        let bt2020_val = oetf(BETA);
        let rec709_equiv = 1.099_f32 * BETA.powf(0.45) - 0.099;
        // They should NOT be equal (different alpha constants)
        assert!(
            (bt2020_val - rec709_equiv).abs() > 1e-6,
            "BT.2020 and BT.709 should differ: {} vs {}",
            bt2020_val,
            rec709_equiv
        );
    }

    #[test]
    fn test_negative_mirror() {
        // Mirror mode: -f(-x) for negative input
        let val = oetf(-0.5);
        assert!(val < 0.0, "oetf(-0.5) should be negative, got {val}");
        assert!(
            (val + oetf(0.5)).abs() < 1e-6,
            "oetf(-x) should equal -oetf(x)"
        );
        let val2 = eotf(-0.3);
        assert!(val2 < 0.0, "eotf(-0.3) should be negative, got {val2}");
        assert!(
            (val2 + eotf(0.3)).abs() < 1e-6,
            "eotf(-x) should equal -eotf(x)"
        );
    }

    #[test]
    fn test_continuity_at_threshold() {
        // At beta, both branches should give the same result
        let linear_val = LIN_SLOPE * BETA;
        let power_val = ALPHA * BETA.powf(POWER) - (ALPHA - 1.0);
        assert!(
            (linear_val - power_val).abs() < 1e-5,
            "Discontinuity at threshold: linear={}, power={}",
            linear_val,
            power_val
        );
    }
}
