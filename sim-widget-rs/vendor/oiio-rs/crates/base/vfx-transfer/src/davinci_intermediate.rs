//! DaVinci Intermediate transfer function.
//!
//! DaVinci Intermediate is Blackmagic Design's log encoding for
//! DaVinci Wide Gamut Intermediate color space in DaVinci Resolve.
//!
//! # Range
//!
//! - Encoded: [0, 1] (signal range)
//! - Linear: Scene-referred (covers >9.1 stops above 18% gray)
//!
//! # Reference
//!
//! DaVinci Resolve 17 Wide Gamut Intermediate documentation (Aug 2021)
//!
//! # Why not LogCameraTransform
//!
//! Uses base-2 log with a pre-shift: `((x + A).log2() + B) * C`,
//! which doesn't match the generic `a * log_base(b*x + c) + d` form.

// DaVinci Intermediate constants
const DI_A: f32 = 0.0075;
const DI_B: f32 = 7.0;
const DI_C: f32 = 0.07329248;
const DI_M: f32 = 10.444_268;
const DI_LIN_CUT: f32 = 0.00262409;
const DI_LOG_CUT: f32 = 0.02740668;

/// DaVinci Intermediate encode: Linear to DaVinci Intermediate.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::davinci_intermediate::encode;
///
/// // 18% gray maps to ~0.336
/// let log = encode(0.18);
/// assert!((log - 0.336).abs() < 0.01);
/// ```
#[inline]
pub fn encode(linear: f32) -> f32 {
    if linear > DI_LIN_CUT {
        ((linear + DI_A).log2() + DI_B) * DI_C
    } else {
        linear * DI_M
    }
}

/// DaVinci Intermediate decode: DaVinci Intermediate to linear.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::davinci_intermediate::decode;
///
/// let linear = decode(0.336);
/// assert!((linear - 0.18).abs() < 0.01);
/// ```
#[inline]
pub fn decode(log: f32) -> f32 {
    if log > DI_LOG_CUT {
        2.0_f32.powf((log / DI_C) - DI_B) - DI_A
    } else {
        log / DI_M
    }
}

/// Applies DaVinci Intermediate encoding to RGB.
#[inline]
pub fn encode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [encode(rgb[0]), encode(rgb[1]), encode(rgb[2])]
}

/// Applies DaVinci Intermediate decoding to RGB.
#[inline]
pub fn decode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [decode(rgb[0]), decode(rgb[1]), decode(rgb[2])]
}

/// Returns the DaVinci Intermediate value for 18% gray.
#[inline]
pub fn middle_gray() -> f32 {
    encode(0.18)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let test_values = [0.001, 0.005, 0.18, 0.5, 1.0, 10.0, 100.0];
        for &l in &test_values {
            let encoded = encode(l);
            let decoded = decode(encoded);
            assert!(
                (l - decoded).abs() < l * 0.01 + 0.0001,
                "l={}, decoded={}",
                l,
                decoded
            );
        }
    }

    #[test]
    fn test_middle_gray() {
        let log = encode(0.18);
        // From docs: 18% gray maps to 0.336043
        assert!((log - 0.336043).abs() < 0.001, "middle gray = {}", log);
    }

    #[test]
    fn test_reference_values() {
        // From documentation table
        assert!((encode(0.0) - 0.0).abs() < 0.001);
        assert!((encode(1.0) - 0.513837).abs() < 0.001);
        assert!((encode(10.0) - 0.756599).abs() < 0.001);
        assert!((encode(100.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_negative() {
        // Handles negative values gracefully (linear region)
        let log = encode(-0.01);
        assert!((log - (-0.104443)).abs() < 0.01, "negative = {}", log);
    }
}
