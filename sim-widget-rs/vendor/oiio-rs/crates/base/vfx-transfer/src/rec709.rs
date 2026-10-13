//! Rec.709 (BT.709) transfer function.
//!
//! The Rec.709 OETF is used for HDTV encoding. Note that the commonly
//! used Rec.709 EOTF is actually BT.1886 (gamma 2.4), not the inverse
//! of the OETF.
//!
//! # Range
//!
//! - Input/Output: [0, 1]
//!
//! # Reference
//!
//! ITU-R BT.709-6
//!
//! # Two copies
//!
//! heif-rs's `heif-transfer` crate has its own copy of this module
//! (`heif-rs/crates/heif-transfer/rec709.rs`). heif-rs is a standalone library that must build and
//! work without oiio-rs, and oiio-rs depends on heif-rs, so heif-rs cannot import this crate. The
//! two copies must give identical results: `tests/heif_transfer_parity.rs` compares them bit for
//! bit, domain edges included. Change the curve in both places in the same change. The heif-rs copy
//! is not byte-for-byte: it leaves out the `_fast` variants and the OCIO-only entry points, which
//! heif-rs does not need.

/// Rec.709 OETF: Encodes linear to Rec.709.
///
/// # Formula
///
/// ```text
/// if L < 0.018:
///     V = 4.5 * L
/// else:
///     V = 1.099 * L^0.45 - 0.099
/// ```
#[inline]
pub fn oetf(l: f32) -> f32 {
    if l < 0.0 {
        // Mirror mode: -f(-x) for negative input
        return -oetf(-l);
    }
    if l < 0.018 {
        4.5 * l
    } else {
        1.099 * l.powf(0.45) - 0.099
    }
}

/// Rec.709 inverse OETF: Decodes Rec.709 to linear.
///
/// Note: For display, use BT.1886 (gamma 2.4) instead.
#[inline]
pub fn eotf(v: f32) -> f32 {
    eotf_with(v, f32::powf)
}

/// Rec.709 inverse OETF using [`crate::fast::pow`] — same formula, ~2e-7 relative error, and it
/// keeps the calling pixel loop vectorised (see that module). Use it in image-sized loops.
#[inline]
#[must_use]
pub fn eotf_fast(v: f32) -> f32 {
    eotf_with(v, crate::fast::pow)
}

/// THE Rec.709 inverse OETF, once — `eotf` and `eotf_fast` differ only in which `pow` they pass
/// in. Branch-free and sign-symmetric for the same reasons as [`crate::srgb`].
#[inline(always)]
fn eotf_with(v: f32, powf: impl Fn(f32, f32) -> f32) -> f32 {
    let a = v.abs();
    let linear = a / 4.5;
    let curve = powf((a + 0.099) / 1.099, 1.0 / 0.45);
    let mag = if a < 0.081 { linear } else { curve };
    mag.copysign(v)
}

/// Applies Rec.709 EOTF to an RGB triplet.
#[inline]
pub fn eotf_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [eotf(rgb[0]), eotf(rgb[1]), eotf(rgb[2])]
}

/// Applies Rec.709 OETF to an RGB triplet.
#[inline]
pub fn oetf_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [oetf(rgb[0]), oetf(rgb[1]), oetf(rgb[2])]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        for i in 0..=100 {
            let v = i as f32 / 100.0;
            let linear = eotf(v);
            let back = oetf(linear);
            assert!((v - back).abs() < 1e-4, "v={}, back={}", v, back);
        }
    }

    #[test]
    fn test_boundaries() {
        assert_eq!(oetf(0.0), 0.0);
        assert!((oetf(1.0) - 1.0).abs() < 1e-6);
    }
}
