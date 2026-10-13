//! Sony S-Log2 transfer function.
//!
//! S-Log2 is Sony's earlier logarithmic encoding, used in cameras like
//! F65, F55, F5, FS700, and FS7 (before S-Log3 became standard).
//!
//! # Range
//!
//! - Encoded: [0, 1] (signal range)
//! - Linear: Scene-referred
//!
//! # Reference
//!
//! - Sony "S-Log2 Technical Summary"
//! - ACES IDT `ACEScsc.Sony_SLog2_SGamut_to_ACES.ctl`
//!
//! There is no S-Log2 builtin in the OpenColorIO checkout we audit against
//! (`transforms/builtins/Cameras.cpp` carries S-Log3 only), so this curve is derived from Sony's
//! published formulation rather than transcribed from OCIO.
//!
//! # Why not LogCameraTransform
//!
//! The linear break sits at `x = 0` rather than at a positive cut, so the generic
//! `LogCameraTransform` break machinery buys nothing here.
//!
//! # COLOR-12, fixed
//!
//! This module previously implemented Sony **S-Log1**: it omitted the `155/219` input scale that
//! is precisely what distinguishes S-Log2 from S-Log1, and carried an unsourced `/1.03` output
//! divisor that appears in no Sony document. 18% grey encoded to 0.3495 instead of the published
//! 0.30798. Both the missing scale and the spurious divisor are corrected below.

// S-Log2 forward, for scene-linear reflectance x (x = 0.18 is an 18% grey card):
//
//   x >= 0 :  y = 0.432699 * log10(155*x/219 + 0.037584) + 0.616596 + 0.03
//   x <  0 :  y = 3.53881278538813 * x + 0.030001222851889303

/// Input headroom rescale, `155/219`. This factor is the whole difference between S-Log2 and
/// S-Log1; dropping it is what made this module compute the wrong curve (COLOR-12).
const IN_SCALE: f64 = 155.0 / 219.0;

/// Log side slope
const LOG_SLOPE: f64 = 0.432699;

/// Log side offset
const LOG_OFFSET: f64 = 0.616596;

/// Linear side offset (added to scaled linear before the log)
const LIN_OFFSET: f64 = 0.037584;

/// Output offset, added alongside `LOG_OFFSET`
const OUT_OFFSET: f64 = 0.03;

/// Slope of the negative extension.
///
/// Derived rather than transcribed: it is the analytic derivative of the log branch at `x = 0`,
/// `LOG_SLOPE * log10(e) * IN_SCALE / LIN_OFFSET` = 3.538790, which reproduces Sony's published
/// slope 3.53881278538813 to 7e-6. The vendor pair is C1-continuous here, so no separate source
/// is needed for the extension.
const LIN_SLOPE: f64 = LOG_SLOPE * std::f64::consts::LOG10_E * IN_SCALE / LIN_OFFSET;

/// Code value at linear = 0, where the two branches meet.
///
/// Sony publishes 0.030001222851889303 as the linear branch's intercept; evaluating the log
/// branch at `x = 0` gives `0.432699*log10(0.037584) + 0.646596`, reproducing that to all 17
/// digits (asserted in `test_code_break_provenance`). `log10` is not `const fn`, hence the
/// literal.
const CODE_BREAK: f64 = 0.030001222851889303;

/// S-Log2 encode: Linear to S-Log2.
///
/// # Formula
///
/// For linear >= 0:
///   y = 0.432699 * log10(155*linear/219 + 0.037584) + 0.616596 + 0.03
///
/// For linear < 0:
///   y = linear * 3.538813 + 0.030001223
///
/// # Example
///
/// ```rust
/// use vfx_transfer::s_log2::encode;
///
/// // 18% gray in S-Log2 is 0.3080 (S-Log3, for contrast, puts it at ~0.41)
/// let log = encode(0.18);
/// assert!((log - 0.3080).abs() < 0.001);
/// ```
#[inline]
pub fn encode(linear: f32) -> f32 {
    let x = linear as f64;
    let y = if x >= 0.0 {
        LOG_SLOPE * (IN_SCALE * x + LIN_OFFSET).log10() + LOG_OFFSET + OUT_OFFSET
    } else {
        // Linear extension for negative values (tangent of the log branch at 0)
        x * LIN_SLOPE + CODE_BREAK
    };
    y as f32
}

/// S-Log2 decode: S-Log2 to linear.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::s_log2::decode;
///
/// let linear = decode(0.307981);
/// assert!((linear - 0.18).abs() < 0.001);
/// ```
#[inline]
pub fn decode(log: f32) -> f32 {
    let y = log as f64;
    let x = if y >= CODE_BREAK {
        (10.0_f64.powf((y - OUT_OFFSET - LOG_OFFSET) / LOG_SLOPE) - LIN_OFFSET) / IN_SCALE
    } else {
        (y - CODE_BREAK) / LIN_SLOPE
    };
    x as f32
}

/// Applies S-Log2 encoding to RGB.
#[inline]
pub fn encode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [encode(rgb[0]), encode(rgb[1]), encode(rgb[2])]
}

/// Applies S-Log2 decoding to RGB.
#[inline]
pub fn decode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [decode(rgb[0]), decode(rgb[1]), decode(rgb[2])]
}

/// Returns the S-Log2 value for 18% gray.
#[inline]
pub fn middle_gray() -> f32 {
    encode(0.18)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let test_values = [0.0, 0.001, 0.01, 0.18, 0.5, 1.0, 2.0, 5.0];
        for &l in &test_values {
            let encoded = encode(l);
            let decoded = decode(encoded);
            let tol = l.abs() * 0.001 + 0.0001;
            assert!(
                (l - decoded).abs() < tol,
                "l={}, encoded={}, decoded={}, diff={}",
                l,
                encoded,
                decoded,
                (l - decoded).abs()
            );
        }
    }

    #[test]
    fn test_roundtrip_negative() {
        let test_values = [-0.01, -0.001];
        for &l in &test_values {
            let encoded = encode(l);
            let decoded = decode(encoded);
            let tol = l.abs() * 0.01 + 0.0001;
            assert!(
                (l - decoded).abs() < tol,
                "l={}, encoded={}, decoded={}, diff={}",
                l,
                encoded,
                decoded,
                (l - decoded).abs()
            );
        }
    }

    /// Replaces a test that asserted `0.34 < encode(0.18) < 0.38` — a window read off the old
    /// (S-Log1) implementation's output of 0.3495, which is why it stayed green through COLOR-12.
    /// The published S-Log2 anchor is 0.30798.
    #[test]
    fn test_middle_gray() {
        let log = encode(0.18);
        assert!((log - 0.307981).abs() < 1e-4, "log={}", log);
    }

    /// Replaces a test that asserted `encode(0.0) == CODE_BREAK / SCALE` — i.e. it restated the
    /// implementation, including the spurious `/1.03`, so it could not detect the defect.
    #[test]
    fn test_zero() {
        let encoded = encode(0.0);
        assert!(
            (encoded - 0.030001223).abs() < 1e-6,
            "encoded={}, expected published intercept 0.030001223",
            encoded
        );
    }

    /// Provenance for `CODE_BREAK`: Sony's published linear intercept is exactly the log branch
    /// evaluated at x = 0, so the literal is not an independent unsourced constant.
    #[test]
    fn test_code_break_provenance() {
        let from_log = LOG_SLOPE * LIN_OFFSET.log10() + LOG_OFFSET + OUT_OFFSET;
        assert!(
            (from_log - CODE_BREAK).abs() < 1e-15,
            "from_log={from_log}, CODE_BREAK={CODE_BREAK}"
        );
    }

    /// Provenance for `LIN_SLOPE`: the derived tangent matches Sony's published 3.53881278538813.
    #[test]
    fn test_lin_slope_matches_published() {
        assert!(
            (LIN_SLOPE - 3.53881278538813).abs() < 1e-4,
            "LIN_SLOPE={LIN_SLOPE}"
        );
    }

    #[test]
    fn test_monotonic() {
        let mut prev = encode(-0.01);
        for i in 0..100 {
            let linear = i as f32 / 50.0; // 0 to 2
            let encoded = encode(linear);
            assert!(
                encoded > prev,
                "Not monotonic at {}: {} <= {}",
                linear,
                encoded,
                prev
            );
            prev = encoded;
        }
    }
}
