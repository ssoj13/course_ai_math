//! Canon Log transfer functions (Canon Log, Canon Log 2, Canon Log 3).
//!
//! Reference: OCIO CanonCameras.cpp, Colour Science Python library
//! Canon Log - original (C300, C500, etc.) - Thorpe 2012
//! Canon Log 2 - used in Cinema EOS cameras (C300 Mark II, C500 Mark II, etc.)
//! Canon Log 3 - wider dynamic range with linear segment near black

/// Canon Log (original) constants - Thorpe 2012
mod clog {
    pub const CUT: f64 = 0.0730597; // Cut point in log domain
    pub const LOG_SLOPE: f64 = 0.529136; // Log segment slope
    pub const LIN_SCALE: f64 = 10.1596; // Linear scaling factor
    pub const NORM: f64 = 0.9; // Normalization (reflection)
}

/// Canon Log 2 constants
mod clog2 {
    pub const CUT: f64 = 0.092864125;
    pub const LOG_SLOPE: f64 = 0.24136077;
    pub const LIN_SCALE: f64 = 87.099375;
    pub const NORM: f64 = 0.9; // normalization factor
}

/// Canon Log 3 constants
mod clog3 {
    pub const CUT_LOW: f64 = 0.097465473;
    pub const CUT_HIGH: f64 = 0.15277891;

    // Log segment (negative side)
    pub const LOG_OFFSET_NEG: f64 = 0.12783901;
    pub const LOG_SLOPE: f64 = 0.36726845;
    pub const LIN_SCALE: f64 = 14.98325;

    // Log segment (positive side)
    pub const LOG_OFFSET_POS: f64 = 0.12240537;

    // Linear segment
    pub const LIN_SLOPE: f64 = 1.9754798;
    pub const LIN_OFFSET: f64 = 0.12512219;

    pub const NORM: f64 = 0.9; // normalization factor
}

// ============================================================================
// Canon Log (original)
// ============================================================================

/// Encode linear to Canon Log (original)
#[inline]
pub fn clog_encode(linear: f32) -> f32 {
    clog_encode_f64(linear as f64) as f32
}

/// Decode Canon Log (original) to linear
#[inline]
pub fn clog_decode(log: f32) -> f32 {
    clog_decode_f64(log as f64) as f32
}

/// Encode linear to Canon Log (f64 precision)
///
/// Formula from Thorpe 2012:
/// - if x < 0: clog = -(0.529136 * log10(-x * 10.1596 + 1) - 0.0730597)
/// - if x >= 0: clog = 0.529136 * log10(x * 10.1596 + 1) + 0.0730597
#[inline]
pub fn clog_encode_f64(linear: f64) -> f64 {
    use clog::*;

    // Denormalize (reflection to scene linear)
    let x = linear / NORM;

    if x < 0.0 {
        -(LOG_SLOPE * (-x * LIN_SCALE + 1.0).log10() - CUT)
    } else {
        LOG_SLOPE * (x * LIN_SCALE + 1.0).log10() + CUT
    }
}

/// Decode Canon Log (original) to linear (f64 precision)
///
/// Formula from Thorpe 2012:
/// - if clog < 0.0730597: out = -(10^((0.0730597 - clog) / 0.529136) - 1) / 10.1596
/// - else: out = (10^((clog - 0.0730597) / 0.529136) - 1) / 10.1596
///
/// Then multiply by 0.9
#[inline]
pub fn clog_decode_f64(log: f64) -> f64 {
    use clog::*;

    let out = if log < CUT {
        -(10.0_f64.powf((CUT - log) / LOG_SLOPE) - 1.0) / LIN_SCALE
    } else {
        (10.0_f64.powf((log - CUT) / LOG_SLOPE) - 1.0) / LIN_SCALE
    };

    out * NORM
}

// ============================================================================
// Canon Log 2
// ============================================================================

/// Encode linear to Canon Log 2
#[inline]
pub fn clog2_encode(linear: f32) -> f32 {
    clog2_encode_f64(linear as f64) as f32
}

/// Decode Canon Log 2 to linear
#[inline]
pub fn clog2_decode(log: f32) -> f32 {
    clog2_decode_f64(log as f64) as f32
}

/// Encode linear to Canon Log 2 (f64 precision)
///
/// Formula (inverse of decode):
/// For normalized linear x (after dividing by 0.9):
/// - if x < 0: log = 0.092864125 - 0.24136077 * log10(-x * 87.099375 + 1)
/// - if x >= 0: log = 0.092864125 + 0.24136077 * log10(x * 87.099375 + 1)
#[inline]
pub fn clog2_encode_f64(linear: f64) -> f64 {
    use clog2::*;

    // Denormalize
    let x = linear / NORM;

    if x < 0.0 {
        CUT - LOG_SLOPE * (-x * LIN_SCALE + 1.0).log10()
    } else {
        CUT + LOG_SLOPE * (x * LIN_SCALE + 1.0).log10()
    }
}

/// Decode Canon Log 2 to linear (f64 precision)
///
/// Formula from OCIO:
/// - if in < 0.092864125: out = -(10^((0.092864125 - in) / 0.24136077) - 1) / 87.099375
/// - else: out = (10^((in - 0.092864125) / 0.24136077) - 1) / 87.099375
///
/// Then multiply by 0.9
#[inline]
pub fn clog2_decode_f64(log: f64) -> f64 {
    use clog2::*;

    let out = if log < CUT {
        -(10.0_f64.powf((CUT - log) / LOG_SLOPE) - 1.0) / LIN_SCALE
    } else {
        (10.0_f64.powf((log - CUT) / LOG_SLOPE) - 1.0) / LIN_SCALE
    };

    out * NORM
}

// ============================================================================
// Canon Log 3
// ============================================================================

/// Encode linear to Canon Log 3
#[inline]
pub fn clog3_encode(linear: f32) -> f32 {
    clog3_encode_f64(linear as f64) as f32
}

/// Decode Canon Log 3 to linear
#[inline]
pub fn clog3_decode(log: f32) -> f32 {
    clog3_decode_f64(log as f64) as f32
}

/// Encode linear to Canon Log 3 (f64 precision)
///
/// Inverse of decode function with three segments
#[inline]
pub fn clog3_encode_f64(linear: f64) -> f64 {
    use clog3::*;

    // Denormalize
    let x = linear / NORM;

    // Break points in linear domain
    let lin_break_low = -0.014; // -0.014 * 0.9 / 0.9
    let lin_break_high = 0.014;

    if x < lin_break_low {
        // Negative log segment
        LOG_OFFSET_NEG - LOG_SLOPE * (-x * LIN_SCALE + 1.0).log10()
    } else if x <= lin_break_high {
        // Linear segment
        x * LIN_SLOPE + LIN_OFFSET
    } else {
        // Positive log segment
        LOG_OFFSET_POS + LOG_SLOPE * (x * LIN_SCALE + 1.0).log10()
    }
}

/// Decode Canon Log 3 to linear (f64 precision)
///
/// Formula from OCIO (three-segment curve):
/// - if in < 0.097465473: out = -(10^((0.12783901 - in) / 0.36726845) - 1) / 14.98325
/// - elif in <= 0.15277891: out = (in - 0.12512219) / 1.9754798  (linear segment)
/// - else: out = (10^((in - 0.12240537) / 0.36726845) - 1) / 14.98325
///
/// Then multiply by 0.9
#[inline]
pub fn clog3_decode_f64(log: f64) -> f64 {
    use clog3::*;

    let out = if log < CUT_LOW {
        // Negative log segment
        -(10.0_f64.powf((LOG_OFFSET_NEG - log) / LOG_SLOPE) - 1.0) / LIN_SCALE
    } else if log <= CUT_HIGH {
        // Linear segment
        (log - LIN_OFFSET) / LIN_SLOPE
    } else {
        // Positive log segment
        (10.0_f64.powf((log - LOG_OFFSET_POS) / LOG_SLOPE) - 1.0) / LIN_SCALE
    };

    out * NORM
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f64 = 1e-9;
    const EPSILON_F32: f32 = 1e-5;

    // ========================================================================
    // Canon Log (original) tests
    // ========================================================================

    #[test]
    fn clog_roundtrip_f64() {
        let test_values = [
            0.0, 0.18, 0.5, 1.0, 0.01, 0.001, 0.1, 0.9, -0.01, -0.001, // negative values
        ];

        for &lin in &test_values {
            let encoded = clog_encode_f64(lin);
            let decoded = clog_decode_f64(encoded);
            assert!(
                (lin - decoded).abs() < EPSILON,
                "CLog roundtrip failed for {lin}: got {decoded}"
            );
        }
    }

    #[test]
    fn clog_roundtrip_f32() {
        let test_values = [0.0f32, 0.18, 0.5, 1.0, 0.01, -0.01];

        for &lin in &test_values {
            let encoded = clog_encode(lin);
            let decoded = clog_decode(encoded);
            assert!(
                (lin - decoded).abs() < EPSILON_F32,
                "CLog f32 roundtrip failed for {lin}: got {decoded}"
            );
        }
    }

    #[test]
    fn clog_zero() {
        // At linear 0, should encode to the cut point
        let encoded = clog_encode_f64(0.0);
        assert!(
            (encoded - clog::CUT).abs() < EPSILON,
            "CLog(0) should be {}, got {encoded}",
            clog::CUT
        );

        // Decode the cut point back to 0
        let decoded = clog_decode_f64(clog::CUT);
        assert!(
            decoded.abs() < EPSILON,
            "CLog decode({}) should be 0, got {decoded}",
            clog::CUT
        );
    }

    #[test]
    fn clog_symmetry() {
        // Canon Log should be anti-symmetric around (0, cut)
        let test_values = [0.01, 0.05, 0.1, 0.5];

        for &x in &test_values {
            let pos = clog_encode_f64(x);
            let neg = clog_encode_f64(-x);

            // Check: pos - cut == cut - neg (anti-symmetry)
            let diff_pos = pos - clog::CUT;
            let diff_neg = clog::CUT - neg;

            assert!(
                (diff_pos - diff_neg).abs() < EPSILON,
                "CLog symmetry failed for x={x}: pos={pos}, neg={neg}"
            );
        }
    }

    #[test]
    fn clog_monotonic() {
        // Encoding should be monotonically increasing
        let mut prev = clog_encode_f64(-0.1);
        for i in 1..100 {
            let lin = -0.1 + 0.012 * i as f64;
            let enc = clog_encode_f64(lin);
            assert!(enc > prev, "CLog not monotonic at lin={lin}");
            prev = enc;
        }
    }

    #[test]
    fn clog_thorpe_table_values() {
        // From Thorpe 2012 Table 2 (without code values normalization)
        // 0% -> 7.3% IRE, 18% -> 32.8% IRE, 90% -> 62.7% IRE
        let lin_18 = 0.18;
        let enc_18 = clog_encode_f64(lin_18);
        // Should be approximately 32.8% = 0.328
        assert!(
            (enc_18 - 0.328).abs() < 0.001,
            "CLog(0.18) should be ~0.328, got {enc_18}"
        );
    }

    // ========================================================================
    // Canon Log 2 tests
    // ========================================================================

    #[test]
    fn clog2_roundtrip_f64() {
        let test_values = [
            0.0, 0.18, 0.5, 1.0, 0.01, 0.001, 0.1, 0.9, -0.01, -0.001, // negative values
        ];

        for &lin in &test_values {
            let encoded = clog2_encode_f64(lin);
            let decoded = clog2_decode_f64(encoded);
            assert!(
                (lin - decoded).abs() < EPSILON,
                "CLog2 roundtrip failed for {lin}: got {decoded}"
            );
        }
    }

    #[test]
    fn clog2_roundtrip_f32() {
        let test_values = [0.0f32, 0.18, 0.5, 1.0, 0.01, -0.01];

        for &lin in &test_values {
            let encoded = clog2_encode(lin);
            let decoded = clog2_decode(encoded);
            assert!(
                (lin - decoded).abs() < EPSILON_F32,
                "CLog2 f32 roundtrip failed for {lin}: got {decoded}"
            );
        }
    }

    #[test]
    fn clog2_zero() {
        // At linear 0, should encode to the cut point
        let encoded = clog2_encode_f64(0.0);
        assert!(
            (encoded - clog2::CUT).abs() < EPSILON,
            "CLog2(0) should be {}, got {encoded}",
            clog2::CUT
        );

        // Decode the cut point back to 0
        let decoded = clog2_decode_f64(clog2::CUT);
        assert!(
            decoded.abs() < EPSILON,
            "CLog2 decode({}) should be 0, got {decoded}",
            clog2::CUT
        );
    }

    #[test]
    fn clog2_symmetry() {
        // Canon Log 2 should be anti-symmetric around (0, cut)
        let test_values = [0.01, 0.05, 0.1, 0.5];

        for &x in &test_values {
            let pos = clog2_encode_f64(x);
            let neg = clog2_encode_f64(-x);

            // Check: pos - cut == cut - neg (anti-symmetry)
            let diff_pos = pos - clog2::CUT;
            let diff_neg = clog2::CUT - neg;

            assert!(
                (diff_pos - diff_neg).abs() < EPSILON,
                "CLog2 symmetry failed for x={x}: pos={pos}, neg={neg}"
            );
        }
    }

    #[test]
    fn clog2_monotonic() {
        // Encoding should be monotonically increasing
        let mut prev = clog2_encode_f64(-0.1);
        for i in 1..100 {
            let lin = -0.1 + 0.012 * i as f64;
            let enc = clog2_encode_f64(lin);
            assert!(enc > prev, "CLog2 not monotonic at lin={lin}");
            prev = enc;
        }
    }

    // ========================================================================
    // Canon Log 3 tests
    // ========================================================================

    #[test]
    fn clog3_roundtrip_f64() {
        let test_values = [
            0.0, 0.18, 0.5, 1.0, 0.01, 0.001, 0.1, 0.9, -0.01, -0.001, // negative values
            0.012,  // in linear segment
            -0.012, // in linear segment (negative)
        ];

        for &lin in &test_values {
            let encoded = clog3_encode_f64(lin);
            let decoded = clog3_decode_f64(encoded);
            assert!(
                (lin - decoded).abs() < EPSILON,
                "CLog3 roundtrip failed for {lin}: encoded={encoded}, decoded={decoded}"
            );
        }
    }

    #[test]
    fn clog3_roundtrip_f32() {
        let test_values = [0.0f32, 0.18, 0.5, 1.0, 0.01, -0.01];

        for &lin in &test_values {
            let encoded = clog3_encode(lin);
            let decoded = clog3_decode(encoded);
            assert!(
                (lin - decoded).abs() < EPSILON_F32,
                "CLog3 f32 roundtrip failed for {lin}: got {decoded}"
            );
        }
    }

    #[test]
    fn clog3_zero() {
        // At linear 0, should be in linear segment
        let encoded = clog3_encode_f64(0.0);
        let expected = clog3::LIN_OFFSET; // 0 * slope + offset = offset

        assert!(
            (encoded - expected).abs() < EPSILON,
            "CLog3(0) should be {expected}, got {encoded}"
        );

        let decoded = clog3_decode_f64(encoded);
        assert!(
            decoded.abs() < EPSILON,
            "CLog3 decode({encoded}) should be 0, got {decoded}"
        );
    }

    #[test]
    fn clog3_linear_segment() {
        // Test values within the linear segment (|x| <= 0.014 before normalization)
        // After normalization by 0.9: |lin| <= 0.0126
        let test_lin = [0.0, 0.005, 0.01, -0.005, -0.01];

        for &lin in &test_lin {
            let x = lin / clog3::NORM; // denormalized
            if x.abs() <= 0.014 {
                // Should be in linear segment
                let encoded = clog3_encode_f64(lin);
                let expected = x * clog3::LIN_SLOPE + clog3::LIN_OFFSET;

                assert!(
                    (encoded - expected).abs() < EPSILON,
                    "CLog3 linear segment: lin={lin}, expected={expected}, got={encoded}"
                );
            }
        }
    }

    #[test]
    fn clog3_monotonic() {
        // Encoding should be monotonically increasing
        let mut prev = clog3_encode_f64(-0.1);
        for i in 1..100 {
            let lin = -0.1 + 0.012 * i as f64;
            let enc = clog3_encode_f64(lin);
            assert!(enc > prev, "CLog3 not monotonic at lin={lin}");
            prev = enc;
        }
    }

    #[test]
    fn clog3_continuity_at_breakpoints() {
        // Check C0 continuity at segment boundaries

        // Break point in log space
        let log_cut_low = clog3::CUT_LOW;
        let log_cut_high = clog3::CUT_HIGH;

        // Decode at exactly the cut points
        let lin_at_low = clog3_decode_f64(log_cut_low);
        let lin_at_high = clog3_decode_f64(log_cut_high);

        // Decode slightly inside/outside the cut points
        let eps = 1e-10;
        let lin_below_low = clog3_decode_f64(log_cut_low - eps);
        let lin_above_high = clog3_decode_f64(log_cut_high + eps);

        // Check continuity
        assert!(
            (lin_at_low - lin_below_low).abs() < 1e-6,
            "Discontinuity at low cut: {} vs {}",
            lin_at_low,
            lin_below_low
        );
        assert!(
            (lin_at_high - lin_above_high).abs() < 1e-6,
            "Discontinuity at high cut: {} vs {}",
            lin_at_high,
            lin_above_high
        );
    }

    // ========================================================================
    // Cross-validation between Log2 and Log3
    // ========================================================================

    #[test]
    fn clog2_vs_clog3_at_unity() {
        // Both should give similar (but not identical) results for 18% grey
        let lin = 0.18;
        let clog2 = clog2_encode_f64(lin);
        let clog3 = clog3_encode_f64(lin);

        // They should both be in a reasonable range (0.3-0.5)
        assert!(
            clog2 > 0.3 && clog2 < 0.6,
            "CLog2(0.18) = {clog2} out of range"
        );
        assert!(
            clog3 > 0.3 && clog3 < 0.6,
            "CLog3(0.18) = {clog3} out of range"
        );
    }
}
