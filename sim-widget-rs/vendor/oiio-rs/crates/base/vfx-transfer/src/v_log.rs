//! Panasonic V-Log transfer function.
//!
//! V-Log is Panasonic's logarithmic encoding for their VariCam
//! and Lumix cameras.
//!
//! # Range
//!
//! - Encoded: [0, 1] (signal range)
//! - Linear: Scene-referred
//!
//! # Reference
//!
//! Panasonic V-Log/V-Gamut Technical Documentation

use crate::log_camera::LogCameraTransform;
use std::sync::OnceLock;

// V-Log primary parameters.
// Encode formula: C * log10(x + B) + D  (for x >= CUT1)
// => lin_slope=1, lin_offset=B, log_slope=C, log_offset=D, base=10
// Linear extension: 5.6 * x + 0.125
//
// Note: decode uses hardcoded log_cut=0.181 which is the log value at CUT1.
// We rely on LogCameraTransform to derive this consistently.
const BASE: f64 = 10.0;
const LOG_SLOPE: f64 = 0.241514; // C
const LOG_OFFSET: f64 = 0.598206; // D
const LIN_SLOPE: f64 = 1.0;
const LIN_OFFSET: f64 = 0.00873; // B
const LIN_BREAK: f64 = 0.01; // CUT1

static TRANSFORM: OnceLock<LogCameraTransform> = OnceLock::new();

#[inline]
fn transform() -> &'static LogCameraTransform {
    TRANSFORM.get_or_init(|| {
        LogCameraTransform::build(
            BASE, LOG_SLOPE, LOG_OFFSET, LIN_SLOPE, LIN_OFFSET, LIN_BREAK,
        )
    })
}

/// V-Log encode: Linear to V-Log.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::v_log::encode;
///
/// // 18% gray in V-Log is approximately 0.423
/// let log = encode(0.18);
/// assert!((log - 0.423).abs() < 0.01);
/// ```
#[inline]
pub fn encode(linear: f32) -> f32 {
    transform().encode_f32(linear)
}

/// V-Log decode: V-Log to linear.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::v_log::decode;
///
/// let linear = decode(0.423);
/// assert!((linear - 0.18).abs() < 0.01);
/// ```
#[inline]
pub fn decode(log: f32) -> f32 {
    transform().decode_f32(log)
}

/// Applies V-Log encoding to RGB.
#[inline]
pub fn encode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [encode(rgb[0]), encode(rgb[1]), encode(rgb[2])]
}

/// Applies V-Log decoding to RGB.
#[inline]
pub fn decode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [decode(rgb[0]), decode(rgb[1]), decode(rgb[2])]
}

/// Returns the V-Log value for 18% gray.
#[inline]
pub fn middle_gray() -> f32 {
    encode(0.18)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let test_values = [0.005, 0.18, 0.5, 1.0, 2.0];
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
        assert!((log - 0.423).abs() < 0.01);
    }

    #[test]
    fn test_black() {
        let log = encode(0.0);
        assert!(log > 0.0); // V-Log has lifted black
    }
}
