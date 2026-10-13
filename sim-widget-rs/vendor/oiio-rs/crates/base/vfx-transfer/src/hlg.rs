//! Hybrid Log-Gamma (HLG) transfer function.
//!
//! HLG is designed for HDR broadcast, compatible with SDR displays.
//! Uses a logarithmic curve for highlights and a gamma curve for shadows.
//!
//! # Range
//!
//! - Encoded: [0, 1]
//! - Linear: [0, 1] (scene-referred, relative)
//!
//! # Reference
//!
//! ITU-R BT.2100-2
//!
//! # Two copies
//!
//! heif-rs's `heif-transfer` crate has its own copy of this module
//! (`heif-rs/crates/heif-transfer/hlg.rs`). heif-rs is a standalone library that must build and
//! work without oiio-rs, and oiio-rs depends on heif-rs, so heif-rs cannot import this crate. The
//! two copies must give identical results: `tests/heif_transfer_parity.rs` compares them bit for
//! bit, domain edges included. Change the curve in both places in the same change. The heif-rs copy
//! is not byte-for-byte: it leaves out the `_fast` variants and the OCIO-only entry points, which
//! heif-rs does not need.

// HLG constants
const A: f32 = 0.17883277;
const B: f32 = 0.28466892; // 1 - 4*A
const C: f32 = 0.559_910_7; // 0.5 - A*ln(4*A)

/// HLG OETF: Encodes linear scene light to HLG signal.
///
/// # Formula
///
/// ```text
/// if E <= 1/12:
///     E' = sqrt(3 * E)
/// else:
///     E' = A * ln(12*E - B) + C
/// ```
///
/// # Example
///
/// ```rust
/// use vfx_transfer::hlg::oetf;
///
/// let signal = oetf(0.5);
/// ```
#[inline]
pub fn oetf(e: f32) -> f32 {
    if e <= 0.0 {
        0.0
    } else if e <= 1.0 / 12.0 {
        (3.0 * e).sqrt()
    } else {
        A * (12.0 * e - B).ln() + C
    }
}

/// HLG inverse OETF: Decodes HLG signal to linear scene light.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::hlg::eotf;
///
/// let linear = eotf(0.5);
/// ```
#[inline]
pub fn eotf(ep: f32) -> f32 {
    eotf_with(ep, f32::exp)
}

/// HLG inverse OETF using [`crate::fast::exp`] — same formula, ~2e-7 relative error, and it keeps
/// the calling pixel loop vectorised.
///
/// This is the curve that motivated [`crate::fast`]: on a 4K 10-bit HLG frame the libm `exp`
/// alone was 69 % of the entire YUV->ACEScg conversion.
#[inline]
#[must_use]
pub fn eotf_fast(ep: f32) -> f32 {
    eotf_with(ep, crate::fast::exp)
}

/// THE HLG inverse OETF, once — `eotf` and `eotf_fast` differ only in which `exp` they pass in.
///
/// Branch-free: both segments are evaluated and selected. Unlike sRGB/Rec.709 this curve is NOT
/// odd — BT.2100 defines it on `[0, 1]` — so negatives clamp to zero rather than mirroring, and
/// that guard has to survive the rewrite (`ep * ep / 3` is positive for negative `ep`, so
/// dropping it would turn a slightly-negative sample into a positive one).
#[inline(always)]
fn eotf_with(ep: f32, exp: impl Fn(f32) -> f32) -> f32 {
    let lower = ep * ep / 3.0;
    let upper = (exp((ep - C) / A) + B) / 12.0;
    let curve = if ep <= 0.5 { lower } else { upper };
    if ep <= 0.0 { 0.0 } else { curve }
}

// HLG OOTF (Opto-Optical Transfer Function) used to live here as `pub fn ootf(y: f32, gamma:
// f32) -> f32 { y.powf(gamma - 1.0) * y }` — DELETED (5c7b28f), not fixed, for two reasons:
//
// 1. It was wrong twice over. BT.2100's OOTF is `F_D = alpha * Y_S^(gamma-1) * E_S`, where `Y_S`
//    is the TRIPLET luminance `0.2627*R + 0.6780*G + 0.0593*B` applied to each channel — a
//    per-channel scalar changes saturation, not brightness, which is a different operation. And
//    `gamma` is meant to be derived from the display's peak luminance
//    (`gamma = 1.2 + 0.42*log10(L_W/1000)`); the one call site anywhere in this tree hardcoded
//    `1.2`, i.e. it was only ever correct at exactly 1000 nits.
// 2. It was the wrong LAYER even fixed. OOTF is a scene-to-DISPLAY operation whose parameters
//    depend on viewing conditions (peak luminance) known only at present/view time — it does not
//    belong in a decode-side transfer-curve crate next to EOTF/OETF, which have no such
//    dependency. It belongs beside the display transform, and that is where it
//    landed: vfx-ocio's display chain applies the real HLG OOTF (3143932). Do not resurrect it here.
//
// Confirmed dead before deletion: zero call sites in this crate or in oiio-rs/playa outside its
// own cross-crate parity test, and zero in heif-rs's byte-for-byte copy
// (`heif-transfer/hlg.rs`, deleted identically, same commit) outside that same test.

/// Applies HLG OETF to RGB.
#[inline]
pub fn oetf_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [oetf(rgb[0]), oetf(rgb[1]), oetf(rgb[2])]
}

/// Applies HLG EOTF to RGB.
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
            let e = i as f32 / 100.0;
            let encoded = oetf(e);
            let decoded = eotf(encoded);
            assert!((e - decoded).abs() < 1e-4, "e={}, decoded={}", e, decoded);
        }
    }

    #[test]
    fn test_boundaries() {
        assert_eq!(oetf(0.0), 0.0);
        assert!((oetf(1.0) - 1.0).abs() < 1e-6);
        assert_eq!(eotf(0.0), 0.0);
        assert!((eotf(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_transition_point() {
        // At the segment break E=1/12 the OETF is sqrt(3*E)=sqrt(3/12)=sqrt(1/4)=0.5
        // exactly (ITU-R BT.2100-2): the lower branch sqrt(3E) and upper branch
        // A*ln(12E-B)+C meet at this point.
        let e = 1.0 / 12.0;
        assert!((oetf(e) - 0.5).abs() < 1e-6, "oetf(1/12)={}", oetf(e));
    }
}
