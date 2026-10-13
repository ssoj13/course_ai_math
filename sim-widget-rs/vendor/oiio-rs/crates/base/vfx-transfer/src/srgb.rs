//! sRGB transfer function.
//!
//! The sRGB standard uses a piecewise function combining a linear segment
//! near black with a power curve (approximately gamma 2.2) for the rest.
//!
//! # Range
//!
//! - Input/Output: [0, 1]
//!
//! # Reference
//!
//! IEC 61966-2-1:1999
//!
//! # Two copies
//!
//! heif-rs's `heif-transfer` crate has its own copy of this module
//! (`heif-rs/crates/heif-transfer/srgb.rs`). heif-rs is a standalone library that must build and
//! work without oiio-rs, and oiio-rs depends on heif-rs, so heif-rs cannot import this crate. The
//! two copies must give identical results: `tests/heif_transfer_parity.rs` compares them bit for
//! bit, domain edges included. Change the curve in both places in the same change. The heif-rs copy
//! is not byte-for-byte: it leaves out the `_fast` variants and the OCIO-only entry points, which
//! heif-rs does not need.

/// sRGB EOTF: Decodes sRGB encoded values to linear light.
///
/// Converts gamma-encoded sRGB [0, 1] to linear [0, 1].
///
/// # Formula
///
/// ```text
/// if V <= 0.04045:
///     L = V / 12.92
/// else:
///     L = ((V + 0.055) / 1.055)^2.4
/// ```
///
/// # Example
///
/// ```rust
/// use vfx_transfer::srgb::eotf;
///
/// let linear = eotf(0.5);
/// assert!((linear - 0.214).abs() < 0.01);
/// ```
#[inline]
pub fn eotf(v: f32) -> f32 {
    eotf_with(v, f32::powf)
}

/// sRGB EOTF using [`crate::fast::pow`] instead of `powf` — same formula, ~2e-7 relative error,
/// and it keeps the calling pixel loop vectorised. See that module for why that is the whole
/// point. Use this in image-sized loops and [`eotf`] when you want the reference answer.
#[inline]
#[must_use]
pub fn eotf_fast(v: f32) -> f32 {
    eotf_with(v, crate::fast::pow)
}

/// THE sRGB EOTF, once. `eotf` and `eotf_fast` differ only in which `pow` they hand in — two
/// copies of the formula would be two places for the constants to drift.
///
/// Written branch-free (both segments evaluated, then selected) because that is what lets the
/// fast variant vectorise; the curve is odd, so the sign is stripped and reapplied rather than
/// recursing into `-eotf(-v)`.
#[inline(always)]
fn eotf_with(v: f32, powf: impl Fn(f32, f32) -> f32) -> f32 {
    let a = v.abs();
    let linear = a / 12.92;
    let curve = powf((a + 0.055) / 1.055, 2.4);
    let mag = if a <= 0.04045 { linear } else { curve };
    mag.copysign(v)
}

/// sRGB OETF: Encodes linear light to sRGB.
///
/// Converts linear [0, 1] to gamma-encoded sRGB [0, 1].
///
/// # Formula
///
/// ```text
/// if L <= 0.0031308:
///     V = L * 12.92
/// else:
///     V = 1.055 * L^(1/2.4) - 0.055
/// ```
///
/// # Example
///
/// ```rust
/// use vfx_transfer::srgb::oetf;
///
/// let encoded = oetf(0.214);
/// assert!((encoded - 0.5).abs() < 0.01);
/// ```
#[inline]
pub fn oetf(l: f32) -> f32 {
    if l < 0.0 {
        // Mirror mode: -f(-x) for negative input
        return -oetf(-l);
    }
    if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// Applies sRGB EOTF to an RGB triplet.
#[inline]
pub fn eotf_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [eotf(rgb[0]), eotf(rgb[1]), eotf(rgb[2])]
}

/// Applies sRGB OETF to an RGB triplet.
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
            assert!((v - back).abs() < 1e-5, "v={}, back={}", v, back);
        }
    }

    #[test]
    fn test_boundaries() {
        assert_eq!(eotf(0.0), 0.0);
        assert!((eotf(1.0) - 1.0).abs() < 1e-6);
        assert_eq!(oetf(0.0), 0.0);
        assert!((oetf(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_midpoint() {
        // sRGB 0.5 should be approximately 0.214 linear
        let linear = eotf(0.5);
        assert!((linear - 0.214).abs() < 0.01);
    }
}
