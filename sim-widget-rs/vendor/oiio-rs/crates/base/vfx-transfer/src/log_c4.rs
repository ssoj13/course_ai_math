//! ARRI LogC4 transfer function.
//!
//! LogC4 is ARRI's latest logarithmic encoding introduced with ALEXA 35.
//! It uses base-2 logarithm unlike LogC3 which uses base-10.
//!
//! # Range
//!
//! - Encoded: approximately [0, 1] for normal scene values
//! - Linear: Scene-referred, supports negative values down to linSideBreak
//!
//! # Key Differences from LogC3
//!
//! - Base 2 instead of base 10
//! - Single curve (no EI variations)
//! - Extended range for HDR
//!
//! # Why not LogCameraTransform
//!
//! LogC4 uses base-2 log and f64 precision throughout. The generic
//! LogCameraTransform is f32/base-10; adapting it would lose precision.
//! - Paired with ARRI Wide Gamut 4 (AWG4)
//!
//! # Reference
//!
//! ARRI LogC4 Specification & OCIO ArriCameras.cpp

/// LogC4 constants from OCIO
/// These define the camera log curve with linear segment for shadow handling
mod constants {
    /// Logarithm base (LogC4 uses base 2, unlike LogC3 which uses base 10)
    pub const BASE: f64 = 2.0;

    /// Linear side slope: scales linear input before log
    pub const LIN_SIDE_SLOPE: f64 = 2231.82630906769;

    /// Linear side offset: added to scaled linear before log
    pub const LIN_SIDE_OFFSET: f64 = 64.0;

    /// Log side slope: scales the logarithm result
    pub const LOG_SIDE_SLOPE: f64 = 0.0647954196341293;

    /// Log side offset: added after log scaling
    pub const LOG_SIDE_OFFSET: f64 = -0.295908392682586;

    /// Linear side break point: below this, use linear segment
    pub const LIN_SIDE_BREAK: f64 = -0.0180569961199113;
}

use constants::*;

/// Compute linear segment parameters at runtime for full precision
fn compute_linear_segment() -> (f64, f64, f64) {
    let lin_at_break = LIN_SIDE_SLOPE * LIN_SIDE_BREAK + LIN_SIDE_OFFSET;

    // log_break = logSideSlope * log2(lin_at_break) / log2(base) + logSideOffset
    // Since base = 2, log2(base) = 1
    let log_break = LOG_SIDE_SLOPE * lin_at_break.log2() + LOG_SIDE_OFFSET;

    // linearSlope for C1 continuity at break point
    // Derivative of log curve at break: logSideSlope * linSideSlope / (lin_at_break * ln(base))
    let ln_base = BASE.ln();
    let linear_slope = LOG_SIDE_SLOPE * LIN_SIDE_SLOPE / (lin_at_break * ln_base);

    // linearOffset to match curve value at break
    let linear_offset = log_break - linear_slope * LIN_SIDE_BREAK;

    (linear_slope, linear_offset, log_break)
}

use std::sync::OnceLock;

// Precomputed at first use
static LINEAR_SEGMENT: OnceLock<(f64, f64, f64)> = OnceLock::new();

/// Get linear segment parameters (slope, offset, log_break)
#[inline]
fn linear_params() -> (f64, f64, f64) {
    *LINEAR_SEGMENT.get_or_init(compute_linear_segment)
}

/// LogC4 encode: Linear to LogC4.
///
/// Converts linear scene light to LogC4 encoded values.
/// Uses base-2 logarithm with a linear segment for shadow handling.
///
/// # Formula
///
/// ```text
/// if linear >= linSideBreak:
///     log = logSideSlope * log2(linSideSlope * linear + linSideOffset) + logSideOffset
/// else:
///     log = linearSlope * linear + linearOffset
/// ```
///
/// # Example
///
/// ```rust
/// use vfx_transfer::log_c4;
///
/// // 18% gray
/// let encoded = log_c4::encode(0.18);
/// assert!((encoded - 0.278).abs() < 0.001);
///
/// // Black
/// let black = log_c4::encode(0.0);
/// assert!((black - 0.092).abs() < 0.001);
/// ```
#[inline]
pub fn encode(linear: f32) -> f32 {
    encode_f64(linear as f64) as f32
}

/// LogC4 encode with f64 precision.
#[inline]
pub fn encode_f64(linear: f64) -> f64 {
    let (linear_slope, linear_offset, _) = linear_params();

    if linear >= LIN_SIDE_BREAK {
        // Log segment
        let x = LIN_SIDE_SLOPE * linear + LIN_SIDE_OFFSET;
        LOG_SIDE_SLOPE * x.log2() + LOG_SIDE_OFFSET
    } else {
        // Linear segment for shadows
        linear_slope * linear + linear_offset
    }
}

/// LogC4 decode: LogC4 to linear.
///
/// Converts LogC4 encoded values to linear scene light.
///
/// # Formula
///
/// ```text
/// if log >= logSideBreak:
///     linear = (2^((log - logSideOffset) / logSideSlope) - linSideOffset) / linSideSlope
/// else:
///     linear = (log - linearOffset) / linearSlope
/// ```
///
/// # Example
///
/// ```rust
/// use vfx_transfer::log_c4;
///
/// // Decode 18% gray
/// let linear = log_c4::decode(0.278);
/// assert!((linear - 0.18).abs() < 0.001);
/// ```
#[inline]
pub fn decode(log: f32) -> f32 {
    decode_f64(log as f64) as f32
}

/// LogC4 decode with f64 precision.
#[inline]
pub fn decode_f64(log: f64) -> f64 {
    let (linear_slope, linear_offset, log_break) = linear_params();

    if log >= log_break {
        // Log segment inverse
        let exp = (log - LOG_SIDE_OFFSET) / LOG_SIDE_SLOPE;
        (2.0_f64.powf(exp) - LIN_SIDE_OFFSET) / LIN_SIDE_SLOPE
    } else {
        // Linear segment inverse
        (log - linear_offset) / linear_slope
    }
}

/// Applies LogC4 encoding to RGB.
#[inline]
pub fn encode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [encode(rgb[0]), encode(rgb[1]), encode(rgb[2])]
}

/// Applies LogC4 decoding to RGB.
#[inline]
pub fn decode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [decode(rgb[0]), decode(rgb[1]), decode(rgb[2])]
}

/// Returns the LogC4 value for 18% gray (middle gray).
///
/// This is approximately 0.278 in LogC4.
#[inline]
pub fn middle_gray() -> f32 {
    encode(0.18)
}

/// Returns the LogC4 value for scene black (0.0 linear).
#[inline]
pub fn scene_black() -> f32 {
    encode(0.0)
}

/// Returns the linear break point value.
#[inline]
pub fn lin_break() -> f64 {
    LIN_SIDE_BREAK
}

/// Returns the log break point value.
#[inline]
pub fn log_break() -> f64 {
    linear_params().2
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test values spanning the full range including linear segment
    const TEST_LINEARS: [f64; 14] = [
        -0.02, -0.01, 0.0, 0.001, 0.01, 0.05, 0.18, 0.38, 1.0, 2.0, 5.0, 10.0, 50.0, 100.0,
    ];

    #[test]
    fn test_known_values() {
        // 18% gray should be around 0.278
        let gray18 = encode_f64(0.18);
        assert!(
            (gray18 - 0.278).abs() < 0.001,
            "18% gray: expected ~0.278, got {}",
            gray18
        );

        // Scene black (0.0) should be around 0.092
        let black = encode_f64(0.0);
        assert!(
            (black - 0.092).abs() < 0.002,
            "Scene black: expected ~0.092, got {}",
            black
        );

        // Linear break point
        let at_break = encode_f64(LIN_SIDE_BREAK);
        let (_, _, log_break) = linear_params();
        assert!(
            (at_break - log_break).abs() < 1e-10,
            "At break: encoded={}, log_break={}",
            at_break,
            log_break
        );
    }

    #[test]
    fn test_encode_decode_inverse() {
        for &linear in &TEST_LINEARS {
            let encoded = encode_f64(linear);
            let decoded = decode_f64(encoded);
            let tolerance = linear.abs() * 1e-12 + 1e-14;
            assert!(
                (linear - decoded).abs() < tolerance,
                "Inverse failed: {} -> {} -> {}, diff = {}",
                linear,
                encoded,
                decoded,
                (linear - decoded).abs()
            );
        }
    }

    #[test]
    fn test_roundtrip() {
        let test_values = [
            -0.02, -0.01, 0.0, 0.001, 0.01, 0.05, 0.18, 0.38, 1.0, 2.0, 5.0, 10.0, 50.0, 100.0,
        ];

        for &linear in &test_values {
            let encoded = encode_f64(linear);
            let decoded = decode_f64(encoded);
            let tolerance = linear.abs() * 1e-12 + 1e-14;
            assert!(
                (linear - decoded).abs() < tolerance,
                "Roundtrip failed: {} -> {} -> {}, diff = {}",
                linear,
                encoded,
                decoded,
                (linear - decoded).abs()
            );
        }
    }

    #[test]
    fn test_middle_gray() {
        let log = encode(0.18);
        // LogC4 18% gray is approximately 0.278
        assert!((log - 0.278).abs() < 0.001);
    }

    #[test]
    fn test_scene_black() {
        let log = encode(0.0);
        // Scene black in LogC4 is approximately 0.092
        assert!((log - 0.092).abs() < 0.001);
    }

    #[test]
    fn test_monotonic() {
        // Encoding should be monotonically increasing
        let mut prev = encode_f64(-0.1);
        for i in 0..1000 {
            let linear = -0.05 + (i as f64) * 0.001;
            let encoded = encode_f64(linear);
            assert!(
                encoded > prev,
                "Not monotonic at linear={}: {} <= {}",
                linear,
                encoded,
                prev
            );
            prev = encoded;
        }
    }

    #[test]
    fn test_continuity_at_break() {
        // Test C0 continuity at break point
        let eps = 1e-10;
        let below = encode_f64(LIN_SIDE_BREAK - eps);
        let at = encode_f64(LIN_SIDE_BREAK);
        let above = encode_f64(LIN_SIDE_BREAK + eps);

        assert!(
            (at - below).abs() < 1e-8,
            "Discontinuity below break: {} vs {}",
            below,
            at
        );
        assert!(
            (above - at).abs() < 1e-8,
            "Discontinuity above break: {} vs {}",
            at,
            above
        );
    }

    #[test]
    fn test_f32_precision() {
        // Ensure f32 version is reasonably accurate
        let test_values = [0.0, 0.18, 1.0, 10.0];
        for &linear in &test_values {
            let f32_result = encode(linear as f32);
            let f64_result = encode_f64(linear) as f32;
            assert!(
                (f32_result - f64_result).abs() < 1e-6,
                "f32 vs f64 mismatch at {}: {} vs {}",
                linear,
                f32_result,
                f64_result
            );
        }
    }

    #[test]
    fn test_rgb_functions() {
        let rgb = [0.1, 0.18, 0.3];
        let encoded = encode_rgb(rgb);
        let decoded = decode_rgb(encoded);

        for i in 0..3 {
            assert!(
                (rgb[i] - decoded[i]).abs() < 1e-5,
                "RGB roundtrip failed at channel {}: {} vs {}",
                i,
                rgb[i],
                decoded[i]
            );
        }
    }
}
