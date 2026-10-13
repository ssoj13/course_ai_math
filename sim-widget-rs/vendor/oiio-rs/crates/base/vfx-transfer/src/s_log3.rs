//! Sony S-Log3 transfer function.
//!
//! S-Log3 is Sony's logarithmic encoding for their digital cinema cameras.
//! It provides approximately 14 stops of dynamic range.
//!
//! # Range
//!
//! - Encoded: [0, 1] (signal range)
//! - Linear: Scene-referred
//!
//! # Why S-Log3 does NOT use LogCameraTransform
//!
//! S-Log3 has a hardcoded linear segment slope `(171.2102946929 - 95.0) / 0.01125 / 1023.0`
//! that does NOT equal the derivative of the log curve at the break point.
//! This is a Sony-specific design choice — the linear extension is not C1-continuous.
//! Forcing it into `LogCameraTransform` (which derives C1 slope from the log curve)
//! would produce different output and fail the OCIO parity test.
//!
//! # Reference
//!
//! Sony S-Log3 Technical Summary

// S-Log3 constants
const CUT: f32 = 0.01125;

/// S-Log3 encode: Linear to S-Log3.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::s_log3::encode;
///
/// // 18% gray in S-Log3 is approximately 0.41
/// let log = encode(0.18);
/// assert!((log - 0.41).abs() < 0.02);
/// ```
#[inline]
pub fn encode(linear: f32) -> f32 {
    if linear >= CUT {
        // Log segment: (420 + 261.5 * log10((x + 0.01) / 0.19)) / 1023
        (420.0 + 261.5 * ((linear + 0.01) / 0.19).log10()) / 1023.0
    } else {
        // Sony hardcoded linear segment (not C1-continuous with log segment)
        (linear * (171.210_3 - 95.0) / 0.01125 + 95.0) / 1023.0
    }
}

/// S-Log3 decode: S-Log3 to linear.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::s_log3::decode;
///
/// let linear = decode(0.41);
/// assert!((linear - 0.18).abs() < 0.02);
/// ```
#[inline]
pub fn decode(log: f32) -> f32 {
    let x = log * 1023.0;
    if x >= 171.210_3 {
        10.0_f32.powf((x - 420.0) / 261.5) * 0.19 - 0.01
    } else {
        (x - 95.0) * 0.01125 / (171.210_3 - 95.0)
    }
}

/// Applies S-Log3 encoding to RGB.
#[inline]
pub fn encode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [encode(rgb[0]), encode(rgb[1]), encode(rgb[2])]
}

/// Applies S-Log3 decoding to RGB.
#[inline]
pub fn decode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [decode(rgb[0]), decode(rgb[1]), decode(rgb[2])]
}

/// Returns the S-Log3 value for 18% gray.
#[inline]
pub fn middle_gray() -> f32 {
    encode(0.18)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let test_values = [0.001, 0.01, 0.18, 0.5, 1.0, 2.0];
        for &l in &test_values {
            let encoded = encode(l);
            let decoded = decode(encoded);
            assert!(
                (l - decoded).abs() < l * 0.02 + 0.001,
                "l={}, decoded={}",
                l,
                decoded
            );
        }
    }

    #[test]
    fn test_middle_gray() {
        let log = encode(0.18);
        // Sony S-Log3: 18% grey is code value 420 of 1023.
        assert!((log - 420.0 / 1023.0).abs() < 1e-6, "log={}", log);
    }
}
