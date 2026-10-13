//! ACES transfer function validation tests.
//!
//! Validates ACEScc and ACEScct implementations against official ACES specifications.
//!
//! # Reference Documents
//!
//! - AMPAS S-2014-003: ACEScc specification
//! - AMPAS S-2016-001: ACEScct specification
//! - OpenColorIO/src/OpenColorIO/transforms/builtins/ACES.cpp

use vfx_transfer::{acescc, acescct};

// ============================================================================
// ACEScct Reference Values
// ============================================================================
// Source: AMPAS S-2016-001 and OpenColorIO ACES.cpp
//
// ACEScct formula:
//   if linear <= 0.0078125 (2^-7):
//     ACEScct = A * linear + B
//   else:
//     ACEScct = (log2(linear) + 9.72) / 17.52
//
// where:
//   A = 10.5402377416545
//   B = 0.0729055341958355
//   X_BRK = 0.0078125 (break point)
//   Y_BRK = 0.155251141552511 (value at break)

const ACESCCT_X_BRK: f32 = 0.0078125; // 2^-7
const ACESCCT_Y_BRK: f32 = 0.155_251_15;
const ACESCCT_B: f32 = 0.072_905_53;

/// ACEScct reference values computed from official formula.
/// Formula (log region): (log2(linear) + 9.72) / 17.52
/// Formula (toe): A * linear + B where A=10.5402377416545, B=0.0729055341958355
const ACESCCT_REFERENCE: &[(f32, f32)] = &[
    // (linear, ACEScct)
    (0.0, 0.072_905_53),       // Zero -> B constant
    (0.0001, 0.073959),        // Toe region: A*0.0001 + B = 0.0729 + 0.00105 = 0.07396
    (0.001, 0.08345),          // Toe region: A*0.001 + B = 0.0729 + 0.01054 = 0.08344
    (0.0078125, 0.155_251_15), // Break point (exact)
    // Values above break use log formula: (log2(x) + 9.72) / 17.52
    // 0.01: log2(0.01) = -6.6439; (-6.6439 + 9.72) / 17.52 = 0.1756
    (0.01, 0.1756),
    // 0.02: log2(0.02) = -5.6439; (-5.6439 + 9.72) / 17.52 = 0.2327
    (0.02, 0.2327),
    // 0.05: log2(0.05) = -4.3219; (-4.3219 + 9.72) / 17.52 = 0.3080
    (0.05, 0.3080),
    // 0.10: log2(0.10) = -3.3219; (-3.3219 + 9.72) / 17.52 = 0.3651
    (0.10, 0.3651),
    // 0.18: log2(0.18) = -2.4739; (-2.4739 + 9.72) / 17.52 = 0.4135
    (0.18, 0.4135),
    // 0.5: log2(0.5) = -1.0; (-1.0 + 9.72) / 17.52 = 0.4977
    (0.5, 0.4977),
    // 1.0: log2(1.0) = 0; (0 + 9.72) / 17.52 = 0.5548
    (1.0, 0.5548),
    // 2.0: log2(2.0) = 1; (1 + 9.72) / 17.52 = 0.6119
    (2.0, 0.6119),
    // 4.0: log2(4.0) = 2; (2 + 9.72) / 17.52 = 0.6690
    (4.0, 0.6690),
    // 8.0: log2(8.0) = 3; (3 + 9.72) / 17.52 = 0.7260
    (8.0, 0.7260),
    // 16.0: log2(16.0) = 4; (4 + 9.72) / 17.52 = 0.7831
    (16.0, 0.7831),
];

/// ACEScc reference values computed from official formula.
/// Pure log formula: (log2(linear) + 9.72) / 17.52
/// Same as ACEScct in log region
const ACESCC_REFERENCE: &[(f32, f32)] = &[
    // (linear, ACEScc) - same as ACEScct in log region
    (0.01, 0.1756),
    (0.02, 0.2327),
    (0.05, 0.3080),
    (0.10, 0.3651),
    (0.18, 0.4135),
    (0.5, 0.4977),
    (1.0, 0.5548),
    (2.0, 0.6119),
    (4.0, 0.6690),
    (8.0, 0.7260),
    (16.0, 0.7831),
];

// ============================================================================
// ACEScct Validation Tests
// ============================================================================

#[test]
fn test_acescct_zero() {
    // Zero should encode to B constant
    let encoded = acescct::encode(0.0);
    assert!(
        (encoded - ACESCCT_B).abs() < 1e-6,
        "ACEScct(0.0) = {} (expected {})",
        encoded,
        ACESCCT_B
    );
}

#[test]
fn test_acescct_break_point() {
    // Break point should be continuous
    let encoded = acescct::encode(ACESCCT_X_BRK);
    assert!(
        (encoded - ACESCCT_Y_BRK).abs() < 1e-6,
        "ACEScct(X_BRK) = {} (expected {})",
        encoded,
        ACESCCT_Y_BRK
    );
}

#[test]
fn test_acescct_reference_values() {
    // Test against official ACES reference values
    for &(linear, expected) in ACESCCT_REFERENCE {
        let encoded = acescct::encode(linear);
        let tolerance = if linear < 0.01 { 1e-4 } else { 1e-3 };
        assert!(
            (encoded - expected).abs() < tolerance,
            "ACEScct({}) = {} (expected {}, diff={})",
            linear,
            encoded,
            expected,
            (encoded - expected).abs()
        );
    }
}

#[test]
fn test_acescct_midgray() {
    // 18% gray is the most critical value
    // log2(0.18) = -2.4739; (-2.4739 + 9.72) / 17.52 = 0.4135
    let encoded = acescct::encode(0.18);
    assert!(
        (encoded - 0.4135).abs() < 0.001,
        "ACEScct(0.18) = {} (expected ~0.4135)",
        encoded
    );
}

#[test]
fn test_acescct_roundtrip_precision() {
    // Verify encode->decode roundtrip with high precision
    let test_values: &[f32] = &[
        0.0, 0.0001, 0.001, 0.005, 0.0078125, 0.01, 0.05, 0.10, 0.18, 0.5, 1.0, 2.0, 4.0, 10.0,
    ];
    for &linear in test_values {
        let encoded = acescct::encode(linear);
        let decoded = acescct::decode(encoded);
        let tolerance = linear.max(1e-6) * 1e-4;
        assert!(
            (linear - decoded).abs() < tolerance,
            "ACEScct roundtrip: {} -> {} -> {} (diff={})",
            linear,
            encoded,
            decoded,
            (linear - decoded).abs()
        );
    }
}

#[test]
fn test_acescct_monotonic() {
    // ACEScct must be strictly monotonic
    let mut prev = acescct::encode(0.0);
    for i in 1..1000 {
        let linear = i as f32 / 100.0;
        let encoded = acescct::encode(linear);
        assert!(
            encoded > prev,
            "ACEScct not monotonic at {}: {} <= {}",
            linear,
            encoded,
            prev
        );
        prev = encoded;
    }
}

// ============================================================================
// ACEScc Validation Tests
// ============================================================================

#[test]
fn test_acescc_reference_values() {
    // Test against official ACES reference values
    for &(linear, expected) in ACESCC_REFERENCE {
        let encoded = acescc::encode(linear);
        let tolerance = 0.002; // ACEScc has some variation near zero
        assert!(
            (encoded - expected).abs() < tolerance,
            "ACEScc({}) = {} (expected {}, diff={})",
            linear,
            encoded,
            expected,
            (encoded - expected).abs()
        );
    }
}

#[test]
fn test_acescc_midgray() {
    // 18% gray is the most critical value
    let encoded = acescc::encode(0.18);
    assert!(
        (encoded - 0.4135).abs() < 0.001,
        "ACEScc(0.18) = {} (expected ~0.4135)",
        encoded
    );
}

#[test]
fn test_acescc_matches_acescct_above_break() {
    // Above the break point, ACEScc and ACEScct should be identical
    let test_values: &[f32] = &[0.01, 0.05, 0.10, 0.18, 0.5, 1.0, 2.0, 4.0, 10.0];
    for &linear in test_values {
        let cc = acescc::encode(linear);
        let cct = acescct::encode(linear);
        assert!(
            (cc - cct).abs() < 1e-4,
            "ACEScc and ACEScct differ at {}: cc={}, cct={}",
            linear,
            cc,
            cct
        );
    }
}

#[test]
fn test_acescc_roundtrip_precision() {
    // Verify encode->decode roundtrip (skip very small values due to log behavior)
    let test_values: &[f32] = &[0.001, 0.01, 0.05, 0.10, 0.18, 0.5, 1.0, 2.0, 4.0, 10.0];
    for &linear in test_values {
        let encoded = acescc::encode(linear);
        let decoded = acescc::decode(encoded);
        let tolerance = linear * 1e-4;
        assert!(
            (linear - decoded).abs() < tolerance,
            "ACEScc roundtrip: {} -> {} -> {} (diff={})",
            linear,
            encoded,
            decoded,
            (linear - decoded).abs()
        );
    }
}

// ============================================================================
// Cross-validation Tests
// ============================================================================

#[test]
fn test_aces_transfers_same_log_region() {
    // Both ACEScc and ACEScct use the same formula in the log region:
    // (log2(linear) + 9.72) / 17.52
    let test_values: &[f32] = &[0.18, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0];

    for &linear in test_values {
        let cc = acescc::encode(linear);
        let cct = acescct::encode(linear);

        // Both should produce exactly the same result in log region
        assert!(
            (cc - cct).abs() < 1e-6,
            "Log region mismatch at {}: cc={}, cct={}",
            linear,
            cc,
            cct
        );

        // Verify formula: (log2(linear) + 9.72) / 17.52
        let expected = (linear.log2() + 9.72) / 17.52;
        assert!(
            (cc - expected).abs() < 1e-5,
            "Formula mismatch at {}: got {}, expected {}",
            linear,
            cc,
            expected
        );
    }
}

#[test]
fn test_aces_output_range() {
    // Verify output range matches ACES spec
    // ACEScct: min ~0.0729 (at 0), max ~1.468 (at 65504)
    // ACEScc: min ~-0.358 (at 0), max ~1.468 (at 65504)

    // ACEScct at zero
    let cct_zero = acescct::encode(0.0);
    assert!(
        cct_zero > 0.07 && cct_zero < 0.08,
        "ACEScct(0) out of range: {}",
        cct_zero
    );

    // ACEScct at max half-float
    let cct_max = acescct::encode(65504.0);
    assert!(
        cct_max > 1.4 && cct_max < 1.5,
        "ACEScct(65504) out of range: {}",
        cct_max
    );

    // ACEScc at small value (it clamps to avoid log(0))
    let cc_small = acescc::encode(0.0);
    assert!(
        cc_small > -0.4 && cc_small < -0.3,
        "ACEScc(0) out of range: {}",
        cc_small
    );

    // ACEScc at max half-float
    let cc_max = acescc::encode(65504.0);
    assert!(
        cc_max > 1.4 && cc_max < 1.5,
        "ACEScc(65504) out of range: {}",
        cc_max
    );
}
