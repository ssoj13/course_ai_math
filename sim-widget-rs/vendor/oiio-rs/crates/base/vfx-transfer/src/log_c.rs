//! ARRI LogC transfer function (LogC3, EI 800).
//!
//! LogC is ARRI's logarithmic encoding for their digital cinema cameras.
//! This implementation uses LogC3 (current standard) at EI 800.
//!
//! # Range
//!
//! - Encoded: [0, 1] (signal range)
//! - Linear: Scene-referred (can be negative for out-of-gamut)
//!
//! # Reference
//!
//! ARRI LogC3 Specification

use crate::log_camera::LogCameraTransform;
use std::sync::OnceLock;

// LogC3 EI 800 primary parameters.
// Formula: C * log10(A*x + B) + D  (for x > CUT)
//   => log_slope=C, log_offset=D, lin_slope=A, lin_offset=B, base=10
const BASE: f64 = 10.0;
const LOG_SLOPE: f64 = 0.247190; // C
const LOG_OFFSET: f64 = 0.385537; // D
const LIN_SLOPE: f64 = 5.555556; // A
const LIN_OFFSET: f64 = 0.052272; // B
const LIN_BREAK: f64 = 0.010591; // CUT

static TRANSFORM: OnceLock<LogCameraTransform> = OnceLock::new();

#[inline]
fn transform() -> &'static LogCameraTransform {
    TRANSFORM.get_or_init(|| {
        LogCameraTransform::build(
            BASE, LOG_SLOPE, LOG_OFFSET, LIN_SLOPE, LIN_OFFSET, LIN_BREAK,
        )
    })
}

/// LogC encode: Linear to LogC3.
///
/// Converts linear scene light to LogC encoded values.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::log_c::encode;
///
/// // 18% gray in LogC is approximately 0.391
/// let log = encode(0.18);
/// assert!((log - 0.391).abs() < 0.01);
/// ```
#[inline]
pub fn encode(linear: f32) -> f32 {
    transform().encode_f32(linear)
}

/// LogC decode: LogC3 to linear.
///
/// Converts LogC encoded values to linear scene light.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::log_c::decode;
///
/// let linear = decode(0.391);
/// assert!((linear - 0.18).abs() < 0.01);
/// ```
#[inline]
pub fn decode(log: f32) -> f32 {
    transform().decode_f32(log)
}

/// Applies LogC encoding to RGB.
#[inline]
pub fn encode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [encode(rgb[0]), encode(rgb[1]), encode(rgb[2])]
}

/// Applies LogC decoding to RGB.
#[inline]
pub fn decode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [decode(rgb[0]), decode(rgb[1]), decode(rgb[2])]
}

/// Returns the LogC value for 18% gray (middle gray).
#[inline]
pub fn middle_gray() -> f32 {
    encode(0.18)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let test_values = [0.0, 0.01, 0.18, 0.5, 1.0, 2.0, 10.0];
        for &l in &test_values {
            let encoded = encode(l);
            let decoded = decode(encoded);
            assert!(
                (l - decoded).abs() < l * 0.001 + 0.001,
                "l={}, decoded={}",
                l,
                decoded
            );
        }
    }

    #[test]
    fn test_middle_gray() {
        // 18% gray should be around 0.391 in LogC
        let log = encode(0.18);
        assert!((log - 0.391).abs() < 0.01);
    }

    #[test]
    fn test_black() {
        // EI800 linear segment offset f = 0.092809 (ARRI LogC3 parameter table).
        assert!((encode(0.0) - 0.092809).abs() < 1e-6, "{}", encode(0.0));
    }

    #[test]
    fn test_clipping() {
        // Above 1.0 linear should still encode smoothly
        let log = encode(2.0);
        assert!(log > encode(1.0));
    }
}
