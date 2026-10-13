//! Apple Log transfer function.
//!
//! Reference: OCIO AppleCameras.cpp
//! Apple Log is used in Apple ProRes RAW recordings from iPhone and other devices.
//!
//! The curve has three segments:
//! - Below 0: constant (clamped to R_0)
//! - Gamma segment: parabolic curve for low light
//! - Log segment: logarithmic curve for mid/high light

use std::sync::OnceLock;

/// Apple Log constants from OCIO
mod constants {
    pub const R_0: f64 = -0.05641088; // mirror point
    pub const R_T: f64 = 0.01; // linear break point
    pub const C: f64 = 47.28711236; // post-power scale
    pub const BETA: f64 = 0.00964052; // lin-side offset
    pub const GAMMA: f64 = 0.08550479; // log-side slope
    pub const DELTA: f64 = 0.69336945; // log-side offset
    pub const BASE: f64 = 2.0; // log base
}

/// Cached P_t value (break point in log domain)
static P_T: OnceLock<f64> = OnceLock::new();

/// Get P_t = c * (R_t - R_0)^2
#[inline]
fn p_t() -> f64 {
    *P_T.get_or_init(|| {
        use constants::*;
        C * (R_T - R_0).powi(2)
    })
}

/// Encode linear to Apple Log
#[inline]
pub fn encode(linear: f32) -> f32 {
    encode_f64(linear as f64) as f32
}

/// Decode Apple Log to linear
#[inline]
pub fn decode(log: f32) -> f32 {
    decode_f64(log as f64) as f32
}

/// Encode linear to Apple Log (f64 precision)
///
/// Inverse of decode function:
/// - if linear <= R_0: out = 0 (or could be negative, but we clamp)
/// - elif linear < R_t: out = c * (linear - R_0)^2  (gamma segment)
/// - else: out = gamma * log2(linear + beta) + delta  (log segment)
#[inline]
pub fn encode_f64(linear: f64) -> f64 {
    use constants::*;

    if linear <= R_0 {
        // Below mirror point - clamp to 0
        0.0
    } else if linear < R_T {
        // Gamma segment (parabolic)
        C * (linear - R_0).powi(2)
    } else {
        // Log segment
        GAMMA * (linear + BETA).log2() + DELTA
    }
}

/// Decode Apple Log to linear (f64 precision)
///
/// Formula from OCIO:
/// - if in >= P_t: out = 2^((in - delta) / gamma) - beta
/// - elif in >= 0: out = sqrt(in / c) + R_0
/// - else: out = R_0
#[inline]
pub fn decode_f64(log: f64) -> f64 {
    use constants::*;

    let pt = p_t();

    if log >= pt {
        // Log segment
        BASE.powf((log - DELTA) / GAMMA) - BETA
    } else if log >= 0.0 {
        // Gamma segment
        (log / C).sqrt() + R_0
    } else {
        // Below zero - clamp to R_0
        R_0
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use constants::*;

    const EPSILON: f64 = 1e-9;
    const EPSILON_F32: f32 = 1e-5;

    #[test]
    fn roundtrip_f64() {
        // Test values in valid range (above R_0)
        let test_values = [0.0, 0.001, 0.01, 0.02, 0.05, 0.1, 0.18, 0.5, 1.0, 2.0, 5.0];

        for &lin in &test_values {
            let encoded = encode_f64(lin);
            let decoded = decode_f64(encoded);
            assert!(
                (lin - decoded).abs() < EPSILON,
                "Apple Log roundtrip failed for {lin}: encoded={encoded}, decoded={decoded}"
            );
        }
    }

    #[test]
    fn roundtrip_f32() {
        let test_values = [0.0f32, 0.01, 0.18, 0.5, 1.0];

        for &lin in &test_values {
            let encoded = encode(lin);
            let decoded = decode(encoded);
            assert!(
                (lin - decoded).abs() < EPSILON_F32,
                "Apple Log f32 roundtrip failed for {lin}: got {decoded}"
            );
        }
    }

    #[test]
    fn zero_encode() {
        // Linear 0 should be in gamma segment
        let encoded = encode_f64(0.0);
        let expected = C * (0.0 - R_0).powi(2);

        assert!(
            (encoded - expected).abs() < EPSILON,
            "Apple Log(0) should be {expected}, got {encoded}"
        );

        // Decode back
        let decoded = decode_f64(encoded);
        assert!(
            decoded.abs() < EPSILON,
            "Apple Log decode({encoded}) should be 0, got {decoded}"
        );
    }

    #[test]
    fn break_point_continuity() {
        // Check C0 continuity at R_t (gamma/log boundary)
        let eps = 1e-10;

        // Encode just below and at R_t
        let below = encode_f64(R_T - eps);
        let at = encode_f64(R_T);

        assert!(
            (below - at).abs() < 1e-6,
            "Discontinuity at R_t: {} vs {}",
            below,
            at
        );

        // Also check that at R_t, both formulas give same result
        let gamma_result = C * (R_T - R_0).powi(2);
        let log_result = GAMMA * (R_T + BETA).log2() + DELTA;

        assert!(
            (gamma_result - log_result).abs() < 1e-6,
            "Segment mismatch at R_t: gamma={gamma_result}, log={log_result}"
        );
    }

    #[test]
    fn p_t_value() {
        // P_t should equal the encoded value of R_t
        let pt = p_t();
        let encoded_rt = encode_f64(R_T);

        // Note: tiny floating-point difference is expected because
        // P_t uses gamma formula, encode(R_t) uses log formula
        assert!(
            (pt - encoded_rt).abs() < 1e-6,
            "P_t ({pt}) should be close to encode(R_t) ({encoded_rt})"
        );
    }

    #[test]
    fn clamp_below_r0() {
        // Values below R_0 should clamp
        let below_r0 = R_0 - 0.1;
        let encoded = encode_f64(below_r0);

        assert!(
            encoded == 0.0,
            "Encode below R_0 should be 0, got {encoded}"
        );
    }

    #[test]
    fn decode_negative() {
        // Decoding negative values should return R_0
        let decoded = decode_f64(-0.1);

        assert!(
            (decoded - R_0).abs() < EPSILON,
            "Decode(-0.1) should be R_0 ({R_0}), got {decoded}"
        );
    }

    #[test]
    fn monotonic() {
        // Encoding should be monotonically increasing (above R_0)
        let mut prev = encode_f64(R_0 + 1e-10);
        for i in 1..100 {
            let lin = R_0 + 0.1 * i as f64;
            let enc = encode_f64(lin);
            assert!(enc > prev, "Apple Log not monotonic at lin={lin}");
            prev = enc;
        }
    }

    #[test]
    fn known_values() {
        // Test against OCIO LUT-generated values
        // 18% grey should be around middle of curve
        let grey18 = encode_f64(0.18);
        assert!(
            grey18 > 0.4 && grey18 < 0.8,
            "Apple Log(0.18) = {grey18} out of expected range"
        );

        // 1.0 should be higher
        let white = encode_f64(1.0);
        assert!(
            white > grey18,
            "Apple Log(1.0) = {white} should be > Apple Log(0.18) = {grey18}"
        );
    }

    #[test]
    fn gamma_segment_parabolic() {
        // In gamma segment, the curve should be parabolic
        // Check that derivative is proportional to (x - R_0)
        let x2 = 0.005; // in gamma segment

        // y = c * (x - R_0)^2
        // dy/dx = 2 * c * (x - R_0)
        let expected_slope_at_x2 = 2.0 * C * (x2 - R_0);

        // Numerical derivative
        let eps = 1e-8;
        let numerical_slope = (encode_f64(x2 + eps) - encode_f64(x2 - eps)) / (2.0 * eps);

        assert!(
            (expected_slope_at_x2 - numerical_slope).abs() < 1e-4,
            "Gamma segment slope mismatch: expected {expected_slope_at_x2}, got {numerical_slope}"
        );
    }
}
