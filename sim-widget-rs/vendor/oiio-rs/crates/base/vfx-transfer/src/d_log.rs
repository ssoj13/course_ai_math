//! DJI D-Log transfer function.
//!
//! D-Log is DJI's logarithmic encoding for their Zenmuse cameras
//! (X5S, X7, X9). Based on film scan characteristics.
//!
//! # Range
//!
//! - Encoded: [0, 1] (signal range)
//! - Linear: Scene-referred
//!
//! # Reference
//!
//! DJI Cinema Color System whitepaper (D-Log/D-Gamut)

use crate::log_camera::LogCameraTransform;
use std::sync::OnceLock;

// D-Log primary parameters, all from the DJI "Cinema Color System" white paper:
//
//   x >  0.0078 :  y = 0.256663 * log10(x * 0.9892 + 0.0108) + 0.584555
//   x <= 0.0078 :  y = 6.025 * x + 0.0929
//
// Rewritten as LogCamera: log_slope=E, log_offset=F, lin_slope=C, lin_offset=D, base=10.
//
// COLOR-13, resolved: the linear tail is DJI's published pair, NOT a C1 re-derivation.
// DJI's two branches are C0-consistent (they meet at the break to 2e-6) but not C1: the log
// branch's tangent there is ~6.224, while DJI publishes slope 6.025. Deriving the tail from
// continuity therefore produced 5.955x+0.09346, ~1.2% off the vendor curve, and lifted black
// from the published 95/1023 to ~0.0935. The vendor's own numbers win — a decoder reading DJI
// footage must invert what DJI's encoder emitted, whatever its internal consistency.
//
// Contrast with Panasonic V-Log in this crate, whose vendor pair IS C1-consistent and is
// reproduced exactly by the derivation; that is why `v_log.rs` can keep using `build`.
const BASE: f64 = 10.0;
const LOG_SLOPE: f64 = 0.256663; // E
const LOG_OFFSET: f64 = 0.584555; // F
const LIN_SLOPE: f64 = 0.9892; // C
const LIN_OFFSET: f64 = 0.0108; // D
const LIN_BREAK: f64 = 0.0078; // LIN_CUT
const LIN_EXT_SLOPE: f64 = 6.025; // A — published, not derived
const LIN_EXT_OFFSET: f64 = 0.0929; // B — published black level, 95/1023

static TRANSFORM: OnceLock<LogCameraTransform> = OnceLock::new();

#[inline]
fn transform() -> &'static LogCameraTransform {
    TRANSFORM.get_or_init(|| {
        LogCameraTransform::build_with_lin_ext(
            BASE,
            LOG_SLOPE,
            LOG_OFFSET,
            LIN_SLOPE,
            LIN_OFFSET,
            LIN_BREAK,
            LIN_EXT_SLOPE,
            LIN_EXT_OFFSET,
        )
    })
}

/// D-Log encode: Linear to D-Log.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::d_log::encode;
///
/// // 18% gray in D-Log is approximately 0.399
/// let log = encode(0.18);
/// assert!((log - 0.399).abs() < 0.01);
/// ```
#[inline]
pub fn encode(linear: f32) -> f32 {
    transform().encode_f32(linear)
}

/// D-Log decode: D-Log to linear.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::d_log::decode;
///
/// let linear = decode(0.399);
/// assert!((linear - 0.18).abs() < 0.01);
/// ```
#[inline]
pub fn decode(log: f32) -> f32 {
    transform().decode_f32(log)
}

/// Applies D-Log encoding to RGB.
#[inline]
pub fn encode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [encode(rgb[0]), encode(rgb[1]), encode(rgb[2])]
}

/// Applies D-Log decoding to RGB.
#[inline]
pub fn decode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [decode(rgb[0]), decode(rgb[1]), decode(rgb[2])]
}

/// Returns the D-Log value for 18% gray.
#[inline]
pub fn middle_gray() -> f32 {
    encode(0.18)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let test_values = [0.001, 0.005, 0.18, 0.5, 1.0, 2.0];
        for &l in &test_values {
            let encoded = encode(l);
            let decoded = decode(encoded);
            assert!(
                (l - decoded).abs() < l * 0.01 + 0.001,
                "l={}, decoded={}",
                l,
                decoded
            );
        }
    }

    #[test]
    fn test_middle_gray() {
        let log = encode(0.18);
        // 18% gray maps to ~408/1023 ≈ 0.399
        assert!((log - 0.399).abs() < 0.01, "middle gray = {}", log);
    }

    #[test]
    fn test_black() {
        let log = encode(0.0);
        // Published black level 95/1023 ≈ 0.0929, now hit exactly rather than approached.
        assert!((log - 0.0929).abs() < 1e-6, "black = {}", log);
    }

    /// Provenance for the two published branches: they are C0-consistent with each other, which
    /// is the independent evidence that the linear pair belongs to this log pair (a transcription
    /// error would not meet at the break). They are deliberately NOT C1 — see the module note.
    #[test]
    fn test_vendor_branches_agree_at_break() {
        let t = transform();
        let from_log =
            t.log_slope * (t.lin_slope * LIN_BREAK + t.lin_offset).log10() + t.log_offset;
        let from_lin = LIN_EXT_SLOPE * LIN_BREAK + LIN_EXT_OFFSET;
        assert!(
            (from_log - from_lin).abs() < 1e-5,
            "C0 gap at break: log={from_log}, lin={from_lin}"
        );

        // The tangent of the log branch at the break is 5.9551, not the published 6.025 — a 1.2%
        // gap. This pins the non-C1 character so a future "cleanup" back to `build` (which would
        // silently reinstate 5.955) fails loudly here.
        let tangent = t.log_slope * t.lin_slope
            / (std::f64::consts::LN_10 * (t.lin_slope * LIN_BREAK + t.lin_offset));
        assert!(
            (tangent - 5.955115).abs() < 1e-5 && (tangent - LIN_EXT_SLOPE).abs() > 0.05,
            "expected vendor pair to be non-C1, tangent={tangent}"
        );
    }

    #[test]
    fn test_white() {
        let log = encode(0.9);
        // 90% reflectance maps to ~586/1023 ≈ 0.573
        assert!((log - 0.573).abs() < 0.02, "white = {}", log);
    }
}
