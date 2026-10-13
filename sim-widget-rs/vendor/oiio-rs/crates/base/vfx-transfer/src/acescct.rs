//! ACEScct transfer function.
//!
//! ACEScct is a logarithmic color space designed for color grading in ACES.
//! It has a "toe" (linear segment near black) for better behavior
//! in shadows compared to ACEScc.
//!
//! # Range
//!
//! - Linear input: scene-referred, typically [0, 65504]
//! - Encoded output: approximately [-0.36, 1.47]
//!
//! # Reference
//!
//! AMPAS S-2016-001 - ACEScct specification

// ACEScct constants
const X_BRK: f32 = 0.0078125; // 2^-7
const Y_BRK: f32 = 0.155_251_15; // evaluated at X_BRK
const A: f32 = 10.540_237;
const B: f32 = 0.072_905_53;

/// ACEScct encode: Converts ACES linear to ACEScct.
///
/// # Formula
///
/// ```text
/// if linear <= 0.0078125:
///     ACEScct = A * linear + B
/// else:
///     ACEScct = (log2(linear) + 9.72) / 17.52
/// ```
///
/// # Example
///
/// ```rust
/// use vfx_transfer::acescct::encode;
///
/// let cct = encode(0.18);
/// assert!((cct - 0.4135).abs() < 0.001);
/// ```
#[inline]
pub fn encode(linear: f32) -> f32 {
    if linear <= X_BRK {
        A * linear + B
    } else {
        (linear.log2() + 9.72) / 17.52
    }
}

/// ACEScct decode: Converts ACEScct to ACES linear.
///
/// # Formula
///
/// ```text
/// if ACEScct <= Y_BRK:
///     linear = (ACEScct - B) / A
/// else:
///     linear = 2^(ACEScct * 17.52 - 9.72)
/// ```
///
/// # Example
///
/// ```rust
/// use vfx_transfer::acescct::decode;
///
/// let linear = decode(0.4135);
/// assert!((linear - 0.18).abs() < 0.001);
/// ```
#[inline]
pub fn decode(cct: f32) -> f32 {
    if cct <= Y_BRK {
        (cct - B) / A
    } else {
        2.0_f32.powf(cct * 17.52 - 9.72)
    }
}

/// Applies ACEScct encode to an RGB triplet.
#[inline]
pub fn encode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [encode(rgb[0]), encode(rgb[1]), encode(rgb[2])]
}

/// Applies ACEScct decode to an RGB triplet.
#[inline]
pub fn decode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [decode(rgb[0]), decode(rgb[1]), decode(rgb[2])]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let values = [0.0, 0.001, 0.01, 0.18, 1.0, 10.0, 100.0];
        for &v in &values {
            let encoded = encode(v);
            let back = decode(encoded);
            assert!(
                (v - back).abs() < 1e-5 * v.max(1.0),
                "v={}, back={}",
                v,
                back
            );
        }
    }

    #[test]
    fn test_midgray() {
        // 18% gray should encode to approximately 0.4135
        let encoded = encode(0.18);
        assert!((encoded - 0.4135).abs() < 0.001);
    }

    #[test]
    fn test_toe() {
        // Values at the toe/linear segment
        let small = 0.005;
        let encoded = encode(small);
        let back = decode(encoded);
        assert!((small - back).abs() < 1e-6);
    }
}
