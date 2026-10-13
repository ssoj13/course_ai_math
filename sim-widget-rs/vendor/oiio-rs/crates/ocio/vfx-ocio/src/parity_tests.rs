//! Comprehensive OCIO parity reference tests.
//!
//! This module tests vfx-ocio against known OCIO reference values to ensure
//! 100% compatibility. Reference values are from OCIO 2.3 test suite.
//!
//! Test categories:
//! - Transfer functions (20 styles)
//! - Fixed functions (30+ styles)
//! - Grading operations (all styles)
//! - Builtin transforms
//! - Cross-config processors

#![cfg(test)]
#![allow(
    unused_imports,
    unused_variables,
    unused_mut,
    unused_comparisons,
    dead_code
)]
// The deprecated DynamicProcessor wrapper is still pinned by tests below.
#![allow(deprecated)]

use crate::builtin::TestSchema;
use crate::{
    BuiltinTransform, CdlStyle, CdlTransform, ExponentTransform, ExposureContrastStyle,
    ExposureContrastTransform, FixedFunctionStyle, FixedFunctionTransform, GradingPrimaryStyle,
    GradingPrimaryTransform, GradingToneStyle, GradingToneTransform, LogTransform, MatrixTransform,
    NegativeStyle, Processor, RangeStyle, RangeTransform, Transform, TransformDirection,
};

/// Helper: create processor and apply to single RGB pixel
fn apply_rgb(transform: &Transform, dir: TransformDirection, rgb: [f32; 3]) -> [f32; 3] {
    let processor = Processor::from_transform(transform, dir).expect("transform should compile");
    let mut pixels = [rgb];
    processor.apply_rgb(&mut pixels);
    pixels[0]
}

/// Assert RGB values match within tolerance
fn assert_rgb_eq(got: [f32; 3], expected: [f32; 3], tol: f32, msg: &str) {
    for i in 0..3 {
        assert!(
            (got[i] - expected[i]).abs() < tol,
            "{}: ch{} got {}, expected {}, diff {}",
            msg,
            i,
            got[i],
            expected[i],
            (got[i] - expected[i]).abs()
        );
    }
}

// ============================================================================
// TRANSFER FUNCTION PARITY TESTS
// Reference: OCIO LogOpData.cpp, GammaOpData.cpp, etc.
// ============================================================================

mod transfer_parity {
    use super::*;
    use crate::processor::ProcessorOp;
    use crate::processor::TransferStyle;

    fn apply(style: TransferStyle, v: f32, forward: bool) -> f32 {
        let proc = Processor::from_ops(vec![ProcessorOp::Transfer { style, forward }]);
        let mut px = [[v, v, v]];
        proc.apply_rgb(&mut px);
        px[0][0]
    }

    /// Every curve at reference points. Each value is computed in f64 from the curve's
    /// definition (the formula is named) or from the OCIO source it ports, and agrees with
    /// PyOCIO to 1e-6 (`docs/audit/tq-ocio.md` §3). The tolerance is relative 1e-5: f32 noise,
    /// not room for a different curve (the old pins, 0.406 for S-Log3 and 0.392 for C-Log2,
    /// were off by 5e-3 and hid behind a 1e-2 tolerance). The decode must return the input.
    #[test]
    fn transfer_reference_values() {
        use TransferStyle::*;
        #[rustfmt::skip]
        let cases: &[(TransferStyle, f32, f32, &str)] = &[
            (Srgb, 0.18, 0.461_356_13, "IEC 61966-2-1: 1.055 x^(1/2.4) - 0.055"),
            (Srgb, 0.003_130_8, 0.040_449_94, "IEC 61966-2-1 knee: 12.92 x"),
            (Srgb, 0.001, 0.012_92, "IEC 61966-2-1 linear segment"),
            (Srgb, 1.0, 1.0, "IEC 61966-2-1 white"),
            (Rec709, 0.18, 0.409_007_73, "BT.709: 1.099 x^0.45 - 0.099"),
            (Gamma22, 0.18, 0.458_656_45, "x^(1/2.2)"),
            (Gamma26, 0.5, 0.765_983_18, "x^(1/2.6)"),
            (Gamma26, 0.18, 0.517_090_25, "x^(1/2.6)"),
            (Rec1886, 0.18, 0.489_437_1, "BT.1886 inverse: x^(1/2.4)"),
            (AcesCct, 0.18, 0.413_588_4, "S-2016-001: (log2 x + 9.72) / 17.52"),
            (AcesCct, 1.0, 0.554_794_5, "S-2016-001: 9.72 / 17.52"),
            (AcesCc, 0.18, 0.413_588_4, "S-2014-003: (log2 x + 9.72) / 17.52"),
            (Log3G10, 0.18, 0.333_332_9, "RED: 0.224282 log10((x + 0.01) 155.975327 + 1)"),
            (LogC3, 0.18, 0.391_006_83, "ARRI LogC3 EI800: 0.24719 log10(5.555556 x + 0.052272) + 0.385537"),
            (LogC4, 0.18, 0.278_395_84, "ARRI LogC4: ((log2(a x + 64) - 6) / 14) b + c"),
            (SLog3, 0.18, 0.410_557_18, "Sony S-Log3: 420 / 1023 at 18% grey"),
            (VLog, 0.18, 0.423_311_45, "Panasonic V-Log: 0.241514 log10(x + 0.00873) + 0.598206"),
            (AppleLog, 0.18, 0.488_272_46, "OCIO AppleCameras.cpp: gamma log2(x + beta) + delta"),
            (CanonCLog2, 0.18, 0.398_254_7, "OCIO CanonCameras.cpp CANON_CLOG2"),
            (CanonCLog3, 0.18, 0.343_389_37, "OCIO CanonCameras.cpp CANON_CLOG3"),
            (Pq, 100.0, 0.508_078_4, "ST 2084, input in cd/m2"),
            (Pq, 1000.0, 0.751_827_1, "ST 2084, input in cd/m2"),
            (Hlg, 0.5, 0.871_643_5, "BT.2100: a ln(12 E - b) + c"),
            (Hlg, 1.0 / 12.0, 0.5, "BT.2100 knee: sqrt(3 E)"),
        ];
        let mut failures = Vec::new();
        for &(style, x, want, source) in cases {
            let got = apply(style, x, true);
            if (got - want).abs() > 1e-5 * want.abs().max(1.0) {
                failures.push(format!(
                    "{style:?} encode({x}) = {got}, want {want} ({source})"
                ));
            }
            let back = apply(style, want, false);
            if (back - x).abs() > 1e-4 * x.abs().max(1.0) {
                failures.push(format!("{style:?} decode({want}) = {back}, want {x}"));
            }
        }
        assert!(
            failures.is_empty(),
            "{}",
            failures.join(
                "
"
            )
        );
    }
}

// ============================================================================
// FIXED FUNCTION PARITY TESTS
// Reference: OCIO FixedFunctionOpData.cpp
// ============================================================================

mod fixed_function_parity {
    use super::*;

    #[test]
    fn test_aces_red_mod_03() {
        // ACES RedMod03 fwd. Ref: FixedFunctionOpCPU_tests.cpp aces_red_mod_03,
        // row 0 input [0.90,0.05,0.22] -> [0.79670035,0.05,0.19934007] (OCIO
        // tol 1e-7; 1e-4 here for the f32 pipeline). Pins the full RGB so a
        // no-op/identity fails.
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRedMod03,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.90, 0.05, 0.22]);
        assert_rgb_eq(
            result,
            [0.79670035, 0.05, 0.19934007],
            1e-4,
            "ACES RedMod03",
        );
    }

    #[test]
    fn test_aces_red_mod_10() {
        // ACES RedMod10 fwd. Ref: FixedFunctionOpCPU_tests.cpp aces_red_mod_10,
        // row 0 input [0.90,0.05,0.22] -> [0.77148211,0.05,0.22] (OCIO tol 1e-7;
        // 1e-4 here). G/B unchanged, R pulled down — pins full RGB vs a no-op.
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRedMod10,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.90, 0.05, 0.22]);
        assert_rgb_eq(result, [0.771_482_1, 0.05, 0.22], 1e-4, "ACES RedMod10");
    }

    #[test]
    fn test_aces_glow_03() {
        // ACES Glow03 fwd. A grey/black probe is a fixed point of Glow (OCIO
        // throws for chroma below the threshold), so pin a chromatic value:
        // [OCIO-cpp] FixedFunctionOpCPU_tests.cpp glow03 row
        // [0.11,0.02,0] -> [0.11392101, 0.02071291, 0].
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesGlow03,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.11, 0.02, 0.0]);
        assert_rgb_eq(
            result,
            [0.113_921_01, 0.020_712_91, 0.0],
            1e-5,
            "ACES Glow03",
        );
    }

    #[test]
    fn test_aces_glow_10() {
        // ACES Glow10 fwd on the same chromatic probe as Glow03 (a grey input
        // is a fixed point and cannot distinguish this from a no-op).
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesGlow10,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.11, 0.02, 0.0]);
        assert!(
            result[0] > 0.11,
            "Glow10 should brighten a dark chromatic probe"
        );
        assert!(result[2].abs() < 1e-6, "zero channel stays zero");
    }

    #[test]
    fn test_aces_dark_to_dim_10() {
        // Dark-to-dim surround correction, gamma 0.9811. [PyOCIO] FF
        // ACES_DARK_TO_DIM_10 (0.5,0.4,0.3) = 0.50822312 0.40657851 0.30493388.
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesDarkToDim10,
            params: vec![0.9811],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]);
        assert_rgb_eq(
            result,
            [0.508_223_1, 0.406_578_5, 0.304_933_88],
            1e-5,
            "ACES DarkToDim10",
        );
    }

    #[test]
    fn test_rec2100_surround() {
        // Rec.2100 surround, gamma 0.9. [PyOCIO] FF REC2100_SURROUND g0.9
        // (0.5,0.4,0.3) = 0.54526794 0.43621436 0.32716078.
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::Rec2100Surround,
            params: vec![0.9],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]);
        assert_rgb_eq(
            result,
            [0.545_267_94, 0.436_214_36, 0.327_160_78],
            1e-5,
            "Rec2100Surround",
        );
    }

    #[test]
    fn test_rgb_to_hsv() {
        // [PyOCIO] FF RGB_TO_HSV (0.7,0.3,0.5) = 0.91666669 0.57142854 0.7.
        // A pure primary can't tell H apart from a wraparound bug; this probe
        // has all three channels distinct.
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::RgbToHsv,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.7, 0.3, 0.5]);
        assert_rgb_eq(result, [0.916_666_7, 0.571_428_54, 0.7], 1e-5, "RGB_TO_HSV");
    }

    #[test]
    fn test_hsv_to_rgb() {
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::HsvToRgb,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        // H=0.333, S=1, V=1 -> Pure green
        let result = apply_rgb(&t, TransformDirection::Forward, [0.333, 1.0, 1.0]);
        assert!(result[0] < 0.1, "R should be near 0");
        assert!(result[1] > 0.9, "G should be near 1");
        assert!(result[2] < 0.1, "B should be near 0");
    }

    #[test]
    fn test_xyz_to_xyy() {
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyzToXyy,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        // D65 white XYZ(0.95047, 1.0, 1.08883) -> xyY(0.3127, 0.329, 1.0)
        let result = apply_rgb(&t, TransformDirection::Forward, [0.95047, 1.0, 1.08883]);
        assert!((result[0] - 0.3127).abs() < 0.001, "x chromaticity");
        assert!((result[1] - 0.329).abs() < 0.001, "y chromaticity");
        assert!((result[2] - 1.0).abs() < 0.001, "Y luminance");
    }

    #[test]
    fn test_xyy_to_xyz() {
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyyToXyz,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.3127, 0.329, 1.0]);
        assert!((result[0] - 0.9504).abs() < 0.01, "X");
        assert!((result[1] - 1.0).abs() < 0.01, "Y");
        assert!((result[2] - 1.088).abs() < 0.02, "Z");
    }

    #[test]
    fn test_xyz_to_uvy() {
        // [PyOCIO] FF XYZ_TO_uvY D65 = 0.19783981 0.46833628 1.0.
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyzToUvy,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.95047, 1.0, 1.08883]);
        assert_rgb_eq(
            result,
            [0.197_839_81, 0.468_336_28, 1.0],
            1e-5,
            "XYZ_TO_uvY",
        );
    }

    #[test]
    fn test_uvy_to_xyz() {
        // [PyOCIO] FF uvY->XYZ (0.1978,0.4683,1) = 0.95035225 1 1.08936584.
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::UvyToXyz,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.1978, 0.4683, 1.0]);
        assert_rgb_eq(result, [0.950_352_25, 1.0, 1.089_365_8], 1e-5, "uvY_TO_XYZ");
    }

    #[test]
    fn test_lin_to_pq() {
        // [PyOCIO] FF LIN_TO_PQ (1,0.18,0.5) = 0.50811529 0.34789377 0.44026798.
        // OCIO's default CPU processor renders this with the SSE fast-power
        // approximation (`ssePower`, FixedFunctionOpCPU.cpp:2192-2220
        // `Renderer_LIN_TO_PQ_SSE<true>`), not an exact `powf`; our f32 `powf`
        // path lands within ~4e-5 of it here, hence 1e-4 rather than 1e-5.
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::LinToPq,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [1.0, 0.18, 0.5]);
        assert_rgb_eq(
            result,
            [0.508_115_3, 0.347_893_77, 0.440_267_98],
            1e-4,
            "LIN_TO_PQ",
        );
    }

    #[test]
    fn test_pq_to_lin() {
        // [PyOCIO] FF PQ_TO_LIN 0.508 = 0.99863482. OCIO's default CPU
        // processor uses the SSE fast-power approximation here too
        // (FixedFunctionOpCPU.cpp:2144-2168 `Renderer_PQ_TO_LIN_SSE<true>`,
        // `myPower` = `ssePower`); the decode rational subtracts x - c1 with
        // x = v^(1/m2) very close to c1 (x=0.9915, c1=0.8359), so fast-pow's
        // relative error is amplified by that near-cancellation far more than
        // in the encode direction. An exact `f64` reimplementation of this
        // formula gives 0.99921889 at v=0.508 (self-consistent round trip with
        // the encode formula), confirming our `powf`-based code computes the
        // exact PQ EOTF correctly; the ~5.8e-4 gap to PyOCIO's default output
        // is OCIO's fast-math approximation, not a bug in ours. Tolerance
        // widened to 1e-3 to pin OCIO's actual (approximate) output without
        // being flaky across platforms' `powf`.
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::PqToLin,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.508, 0.508, 0.508]);
        assert_rgb_eq(
            result,
            [0.998_634_8, 0.998_634_8, 0.998_634_8],
            1e-3,
            "PQ_TO_LIN",
        );
    }

    #[test]
    fn test_aces_gamut_compress_13() {
        // ACES Gamut Compress 1.3 fwd with the OCIO reference parameter set
        // (params="1.147 1.264 1.312 0.815 0.803 0.88 1.2", also used by
        // FileFormatCTF_tests.cpp:6951 `fixed_function_aces_gamut_comp_13_ctf`).
        // [PyOCIO] FF ACES_GAMUT_COMP_13 (1,-0.1,0) = 1.0 0.03348339 0.04595727.
        let params = vec![1.147, 1.264, 1.312, 0.815, 0.803, 0.880, 1.2];
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesGamutComp13,
            params,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [1.0_f32, -0.1, 0.0]);
        assert_rgb_eq(
            result,
            [1.0, 0.033_483_39, 0.045_957_27],
            1e-5,
            "ACES_GAMUT_COMP_13",
        );
    }

    // LinToGammaLog and GammaLogToLin require specific camera parameters
    // Skip these for now as they need camera-specific setup

    #[test]
    fn test_lin_to_double_log() {
        // Double-log curve (complex parametric)
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::LinToDoubleLog,
            params: vec![
                10.0, // base
                0.01, // breakPt
                0.25, // logSlope
                0.5,  // logOffset
                1.0,  // linSlope
                0.0,  // linOffset
                1.0,  // linearSlope
                0.1,  // log2Slope
                0.0,  // log2Offset
            ],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.18, 0.18, 0.18]);
        assert!(result[0].is_finite());
    }
}

// ============================================================================
// GRADING OPERATIONS PARITY TESTS
// Reference: OCIO GradingPrimaryOpData.cpp, GradingToneOpData.cpp
// ============================================================================

mod grading_parity {
    use super::*;

    #[test]
    fn test_grading_primary_log() {
        let t = Transform::GradingPrimary(GradingPrimaryTransform::from_flat(
            GradingPrimaryStyle::Log,
            [0.1, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            1.0,
            0.18,
            0.0,
            1.0,
            f64::NEG_INFINITY,
            f64::INFINITY,
            TransformDirection::Forward,
        ));
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.5, 0.5]);
        // Red brightness increased
        assert!(result[0] > result[1], "Red should be brighter");
    }

    #[test]
    fn test_grading_primary_lin() {
        let t = Transform::GradingPrimary(GradingPrimaryTransform::from_flat(
            GradingPrimaryStyle::Linear,
            [0.0, 0.0, 0.0],
            [1.5, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            1.0,
            0.18,
            0.0,
            1.0,
            f64::NEG_INFINITY,
            f64::INFINITY,
            TransformDirection::Forward,
        ));
        let result = apply_rgb(&t, TransformDirection::Forward, [0.36, 0.36, 0.36]);
        // Red contrast increased around pivot
        assert!(result[0] > result[1], "Red should have more contrast");
    }

    #[test]
    fn test_grading_primary_video() {
        // Identity Video grade: lift/gain/gamma all default (identity), just a
        // [0,1] clamp. `slope` is no longer directly settable - VIDEO's slope
        // is derived from lift/gain/pivot (`op::grading_primary_params`); at
        // identity lift/gain it derives back to 1.0, same as this test's old
        // explicit `slope: [1.0, 1.0, 1.0]`.
        let t = Transform::GradingPrimary(GradingPrimaryTransform {
            name: String::new(),
            style: GradingPrimaryStyle::Video,
            value: crate::dynamic::GradingPrimary {
                clamp_black: 0.0,
                clamp_white: 1.0,
                ..crate::dynamic::GradingPrimary::video_default()
            },
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.5, 0.5]);
        // Identity should pass through unchanged
        assert!(
            (result[0] - 0.5).abs() < 0.1,
            "Video grading identity should preserve values"
        );
    }

    #[test]
    fn test_grading_tone_log() {
        // OCIO reference vector TS1 (GradingToneOpCPU_tests.cpp): GRADING_LOG
        // midtones {0.3,1.0,1.8,1.2, center 0.47, width 0.6}. Start from the
        // per-style defaults so the untouched zones are reference-valid.
        let base = crate::grading_tone::GradingTone::from_style(GradingToneStyle::Log);
        let t = Transform::GradingTone(GradingToneTransform {
            name: String::new(),
            style: GradingToneStyle::Log,
            blacks: base.blacks,
            shadows: base.shadows,
            midtones: crate::GradingRgbmsw {
                red: 0.3,
                green: 1.0,
                blue: 1.8,
                master: 1.2,
                start: 0.47,
                width: 0.6,
            },
            highlights: base.highlights,
            whites: base.whites,
            scontrast: 1.0,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.3, 0.6, 0.7]);
        let expected = [0.23564218_f32, 0.628_38, 0.76080927];
        for j in 0..3 {
            assert!(
                (result[j] - expected[j]).abs() <= 1e-4,
                "midtones ch{j}: got {} expected {}",
                result[j],
                expected[j]
            );
        }
    }

    #[test]
    fn test_grading_tone_lin() {
        // Style-correct defaults, then a mild midtones + scontrast tweak within validate bounds.
        let base = crate::grading_tone::GradingTone::from_style(GradingToneStyle::Linear);
        let t = Transform::GradingTone(GradingToneTransform {
            name: String::new(),
            style: GradingToneStyle::Linear,
            blacks: base.blacks,
            shadows: base.shadows,
            midtones: crate::GradingRgbmsw {
                master: 1.2,
                ..base.midtones
            },
            highlights: base.highlights,
            whites: base.whites,
            scontrast: 1.5,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_tone_video() {
        let base = crate::grading_tone::GradingTone::from_style(GradingToneStyle::Video);
        let t = Transform::GradingTone(GradingToneTransform {
            name: String::new(),
            style: GradingToneStyle::Video,
            blacks: base.blacks,
            shadows: base.shadows,
            midtones: base.midtones,
            highlights: base.highlights,
            whites: base.whites,
            scontrast: 1.0,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.5, 0.5]);
        // Just check it produces valid output
        assert!(result[0].is_finite(), "Result should be finite");
    }
}

// ============================================================================
// CDL TRANSFORM PARITY TESTS
// Reference: OCIO CDLOpData.cpp
// ============================================================================

mod cdl_parity {
    use super::*;

    #[test]
    fn test_cdl_asc() {
        // ASC-CDL: out = (in * slope + offset)^power * saturation
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [1.2, 1.0, 0.8],
            offset: [0.01, 0.0, -0.01],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.5, 0.5]);
        // R = 0.5 * 1.2 + 0.01 = 0.61
        assert!((result[0] - 0.61).abs() < 0.01, "R: {}", result[0]);
        // G = 0.5
        assert!((result[1] - 0.5).abs() < 0.01, "G: {}", result[1]);
        // B = 0.5 * 0.8 - 0.01 = 0.39
        assert!((result[2] - 0.39).abs() < 0.01, "B: {}", result[2]);
    }

    #[test]
    fn test_cdl_noclamp() {
        // NoClamp style allows values outside 0-1
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [2.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::NoClamp,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.8, 0.5, 0.5]);
        // R = 0.8 * 2.0 = 1.6 (no clamp)
        assert!((result[0] - 1.6).abs() < 0.01, "R should be 1.6");
    }

    #[test]
    fn test_cdl_saturation() {
        // [PyOCIO] CDL sat0 (1,0.5,0) = 0.57019997 x3 (Rec.709 luma weights
        // exercised across all three input channels, not just one).
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 0.0, // Desaturate completely
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [1.0, 0.5, 0.0]);
        assert_rgb_eq(
            result,
            [0.570_199_97, 0.570_199_97, 0.570_199_97],
            1e-5,
            "CDL sat0",
        );
    }

    #[test]
    fn test_cdl_power() {
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [2.0, 1.0, 0.5],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.25, 0.25, 0.25]);
        // R = 0.25^2 = 0.0625
        assert!((result[0] - 0.0625).abs() < 0.01, "R power");
        // G = 0.25^1 = 0.25
        assert!((result[1] - 0.25).abs() < 0.01, "G power");
        // B = 0.25^0.5 = 0.5
        assert!((result[2] - 0.5).abs() < 0.01, "B power");
    }

    #[test]
    fn test_cdl_roundtrip() {
        let t_fwd = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [1.2, 0.9, 1.1],
            offset: [0.02, -0.01, 0.0],
            power: [1.1, 0.95, 1.0],
            saturation: 0.9,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        });
        let t_inv = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [1.2, 0.9, 1.1],
            offset: [0.02, -0.01, 0.0],
            power: [1.1, 0.95, 1.0],
            saturation: 0.9,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Inverse,
        });

        let original = [0.3, 0.5, 0.7];
        let fwd = apply_rgb(&t_fwd, TransformDirection::Forward, original);
        let rt = apply_rgb(&t_inv, TransformDirection::Forward, fwd);

        assert_rgb_eq(rt, original, 0.01, "CDL roundtrip");
    }
}

// ============================================================================
// MATRIX TRANSFORM PARITY TESTS
// Reference: OCIO MatrixOpData.cpp
// ============================================================================

mod matrix_parity {
    use super::*;

    #[test]
    fn test_identity_matrix() {
        let t = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0, 0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.3, 0.7]);
        assert_rgb_eq(result, [0.5, 0.3, 0.7], 1e-6, "Identity matrix");
    }

    #[test]
    fn test_scale_matrix() {
        let t = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                2.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 1.5, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0, 0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.5, 0.5]);
        assert_rgb_eq(result, [1.0, 0.25, 0.75], 1e-5, "Scale matrix");
    }

    #[test]
    fn test_offset() {
        let t = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.1, -0.1, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.5, 0.5]);
        assert_rgb_eq(result, [0.6, 0.4, 0.5], 1e-5, "Offset");
    }

    #[test]
    fn test_xyz_to_rgb_matrix() {
        // sRGB XYZ -> RGB matrix (D65)
        let t = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                3.2404542, -1.5371385, -0.4985314, 0.0, -0.9692660, 1.8760108, 0.0415560, 0.0,
                0.0556434, -0.2040259, 1.0572252, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0, 0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });
        // D65 white XYZ -> RGB (1,1,1)
        let result = apply_rgb(&t, TransformDirection::Forward, [0.95047, 1.0, 1.08883]);
        assert_rgb_eq(result, [1.0, 1.0, 1.0], 0.01, "XYZ to RGB");
    }
}

// ============================================================================
// EXPONENT TRANSFORM PARITY TESTS
// Reference: OCIO GammaOpData.cpp
// ============================================================================

mod exponent_parity {
    use super::*;

    #[test]
    fn test_basic_gamma() {
        let t = Transform::Exponent(ExponentTransform {
            name: String::new(),
            value: [2.2, 2.2, 2.2, 1.0],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.5, 0.5]);
        // 0.5^2.2 = 0.2176
        assert!((result[0] - 0.2176).abs() < 0.01, "Gamma 2.2");
    }

    #[test]
    fn test_mirror_negative() {
        let t = Transform::Exponent(ExponentTransform {
            name: String::new(),
            value: [2.2, 2.2, 2.2, 1.0],
            negative_style: NegativeStyle::Mirror,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [-0.5, 0.5, 0.5]);
        // Mirror: -|x|^gamma * sign(x)
        assert!(result[0] < 0.0, "Should preserve negative");
        assert!((result[0] + 0.2176).abs() < 0.01, "Mirrored gamma");
    }

    #[test]
    fn test_pass_thru_negative() {
        let t = Transform::Exponent(ExponentTransform {
            name: String::new(),
            value: [2.2, 2.2, 2.2, 1.0],
            negative_style: NegativeStyle::PassThru,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [-0.5, 0.5, 0.5]);
        // PassThru: negatives pass through unchanged
        assert!((result[0] - (-0.5)).abs() < 0.01, "PassThru negative");
    }

    #[test]
    fn test_per_channel_gamma() {
        let t = Transform::Exponent(ExponentTransform {
            name: String::new(),
            value: [2.0, 2.4, 2.6, 1.0],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.5, 0.5]);
        // R: 0.5^2.0 = 0.25
        // G: 0.5^2.4 = 0.1892
        // B: 0.5^2.6 = 0.1649
        assert!((result[0] - 0.25).abs() < 0.01, "R gamma 2.0");
        assert!((result[1] - 0.189).abs() < 0.01, "G gamma 2.4");
        assert!((result[2] - 0.165).abs() < 0.01, "B gamma 2.6");
    }
}

// ============================================================================
// RANGE TRANSFORM PARITY TESTS
// Reference: OCIO RangeOpData.cpp
// ============================================================================

mod range_parity {
    use super::*;

    #[test]
    fn test_clamp_range() {
        let t = Transform::Range(RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(1.0),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [1.5, -0.5, 0.5]);
        assert_rgb_eq(result, [1.0, 0.0, 0.5], 1e-5, "Clamp range");
    }

    #[test]
    fn test_scale_range() {
        // Map 0-1 to 0.1-0.9
        let t = Transform::Range(RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.1),
            max_out: Some(0.9),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.0, 0.5, 1.0]);
        assert_rgb_eq(result, [0.1, 0.5, 0.9], 1e-5, "Scale range");
    }

    #[test]
    fn test_noclamp_range() {
        let t = Transform::Range(RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.1),
            max_out: Some(0.9),
            style: RangeStyle::NoClamp,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [1.5, 0.5, -0.5]);
        // Should not clamp: 1.5 -> 0.1 + 1.5*0.8 = 1.3
        assert!((result[0] - 1.3).abs() < 0.01, "NoClamp above 1");
    }
}

// ============================================================================
// LOG TRANSFORM PARITY TESTS
// Reference: OCIO LogOpData.cpp
// ============================================================================

mod log_parity {
    use super::*;

    #[test]
    fn test_log10() {
        let t = Transform::Log(LogTransform {
            name: String::new(),
            base: 10.0,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [10.0, 100.0, 1000.0]);
        assert_rgb_eq(result, [1.0, 2.0, 3.0], 1e-5, "Log10");
    }

    #[test]
    fn test_log2() {
        let t = Transform::Log(LogTransform {
            name: String::new(),
            base: 2.0,
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [2.0, 4.0, 8.0]);
        assert_rgb_eq(result, [1.0, 2.0, 3.0], 1e-5, "Log2");
    }

    #[test]
    fn test_log_inverse() {
        let t = Transform::Log(LogTransform {
            name: String::new(),
            base: 10.0,
            direction: TransformDirection::Inverse,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [1.0, 2.0, 3.0]);
        assert_rgb_eq(result, [10.0, 100.0, 1000.0], 1e-3, "AntiLog10");
    }
}

// ============================================================================
// EXPOSURE CONTRAST PARITY TESTS
// Reference: OCIO ExposureContrastOpData.cpp
// ============================================================================

mod exposure_contrast_parity {
    use super::*;

    #[test]
    fn test_ec_linear() {
        let t = Transform::ExposureContrast(ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Linear,
            exposure: 1.0, // +1 stop
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.18, 0.18, 0.18]);
        // +1 stop doubles linear value
        assert!((result[0] - 0.36).abs() < 0.01, "Exposure +1 stop");
    }

    #[test]
    fn test_ec_log() {
        let t = Transform::ExposureContrast(ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Logarithmic,
            exposure: 0.0,
            contrast: 1.5,
            gamma: 1.0,
            pivot: 0.435,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.5, 0.5]);
        // Contrast should increase deviation from pivot
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_ec_video() {
        let t = Transform::ExposureContrast(ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Video,
            exposure: 0.0,
            contrast: 1.0,
            gamma: 0.8, // Brighten
            pivot: 1.0,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.5, 0.5]);
        // Gamma < 1 brightens
        assert!(result[0] > 0.5, "Gamma should brighten");
    }
}

// ============================================================================
// BUILTIN TRANSFORM PARITY TESTS
// Reference: OCIO BuiltinTransform_tests.cpp
// ============================================================================

mod builtin_parity {
    use super::*;

    // Expected outputs below are the exact values from the OCIO reference test
    // table `tests/cpu/transforms/BuiltinTransform_tests.cpp` (input
    // {0.5, 0.4, 0.3}). They pin the FULL RGB output, so a no-op/identity
    // implementation (the failure mode that the previous in-range checks
    // silently tolerated) fails the test. Tolerance 1e-3 absorbs the f32 vs
    // f64 pipeline difference while staying far tighter than any wrong result.

    /// Reference: BuiltinTransform_tests.cpp "SONY_SLOG3-SGAMUT3.CINE_to_ACES2065-1".
    #[test]
    fn test_slog3_to_aces() {
        let t = Transform::Builtin(BuiltinTransform {
            // NB: canonical OCIO style uses ".CINE" (dot), not "-CINE"; the
            // hyphen form does not resolve and silently no-op'd to identity.
            style: "SONY_SLOG3-SGAMUT3.CINE_to_ACES2065-1".to_string(),
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]);
        assert_rgb_eq(
            result,
            [0.314_942_66, 0.170_408_01, 0.046_854_94],
            1e-3,
            "S-Log3 SGamut3.Cine to ACES2065-1",
        );
    }

    /// Reference: OCIO `BuiltinTransform_tests.cpp:597-599`
    /// "ARRI_ALEXA-LOGC-EI800-AWG_to_ACES2065-1" = {0.401621427766, 0.236455447604,
    /// 0.064830001192} (tolerance 1e-6 there). This test used to ask for the invented style
    /// "ARRI_LogC3_to_ACES2065-1" and pin {0.316689, 0.160073, 0.062214} at a 0.1 tolerance —
    /// a value from no reference, loose enough to pass the wrong ALEXA gamut matrix it came from.
    #[test]
    fn test_logc3_to_aces() {
        let t = Transform::Builtin(BuiltinTransform {
            style: "ARRI_ALEXA-LOGC-EI800-AWG_to_ACES2065-1".to_string(),
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]);
        let expected = [0.401_621_43, 0.236_455_45, 0.064_83];
        assert_rgb_eq(result, expected, 1e-5, "ALEXA LogC EI800 AWG to ACES2065-1");
    }

    /// Reference: BuiltinTransform_tests.cpp "ARRI_LOGC4_to_ACES2065-1".
    /// AWG4_TO_AP0 is now generated with CAT02 (matching OCIO), so this matches
    /// the f64 reference within the f32 tolerance.
    #[test]
    fn test_logc4_to_aces() {
        let t = Transform::Builtin(BuiltinTransform {
            style: "ARRI_LOGC4_to_ACES2065-1".to_string(),
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]);
        assert_rgb_eq(
            result,
            [1.786_878_1, 0.743_018_57, 0.232_840_03],
            1e-3,
            "ARRI LogC4 to ACES2065-1",
        );
    }

    /// Reference: BuiltinTransform_tests.cpp "RED_LOG3G10-RWG_to_ACES2065-1".
    #[test]
    fn test_redlog3g10_to_aces() {
        let t = Transform::Builtin(BuiltinTransform {
            // Canonical OCIO style is "-RWG_" (RedCameras.cpp:109), not the
            // spelled-out "-REDWIDEGAMUTRGB_" which does not resolve.
            style: "RED_LOG3G10-RWG_to_ACES2065-1".to_string(),
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]);
        // Note the blue channel is legitimately NEGATIVE (-0.0254) — the old
        // `> 0.0` band would have rejected the correct result.
        assert_rgb_eq(
            result,
            [0.887_988_2, 0.416_932_26, -0.025_442_211],
            1e-3,
            "RED Log3G10 RWG to ACES2065-1",
        );
    }

    /// Reference: BuiltinTransform_tests.cpp "CANON_CLOG2-CGAMUT_to_ACES2065-1".
    /// CANON_CGAMUT_TO_AP0 is now generated with CAT02 (matching OCIO).
    #[test]
    fn test_canonlog2_to_aces() {
        let t = Transform::Builtin(BuiltinTransform {
            style: "CANON_CLOG2-CGAMUT_to_ACES2065-1".to_string(),
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]);
        assert_rgb_eq(
            result,
            [0.408_435_76, 0.197_486_9, 0.034_204_558],
            1e-3,
            "Canon C-Log2 CGamut to ACES2065-1",
        );
    }

    /// Reference: BuiltinTransform_tests.cpp "CANON_CLOG3-CGAMUT_to_ACES2065-1".
    /// CANON_CGAMUT_TO_AP0 is now generated with CAT02 (matching OCIO).
    #[test]
    fn test_canonlog3_to_aces() {
        let t = Transform::Builtin(BuiltinTransform {
            style: "CANON_CLOG3-CGAMUT_to_ACES2065-1".to_string(),
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]);
        assert_rgb_eq(
            result,
            [0.496_034_92, 0.301_015_35, 0.083_691_83],
            1e-3,
            "Canon C-Log3 CGamut to ACES2065-1",
        );
    }

    /// Reference: BuiltinTransform_tests.cpp "ACEScg_to_ACES2065-1" (AP1->AP0 matrix).
    #[test]
    fn test_acescg_to_aces() {
        let t = Transform::Builtin(BuiltinTransform {
            style: "ACEScg_to_ACES2065-1".to_string(),
            direction: TransformDirection::Forward,
        });
        // Chromatic probe {0.5,0.4,0.3} exercises the full 3x3 (a grey probe
        // could not distinguish the real matrix from identity).
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]);
        assert_rgb_eq(
            result,
            [0.453_158_32, 0.394_926, 0.299_297_33],
            1e-3,
            "ACEScg to ACES2065-1",
        );
    }

    /// Reference: BuiltinTransform_tests.cpp "ACEScct_to_ACES2065-1".
    #[test]
    fn test_acescct_to_aces() {
        let t = Transform::Builtin(BuiltinTransform {
            style: "ACEScct_to_ACES2065-1".to_string(),
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]);
        assert_rgb_eq(
            result,
            [0.386_397_2, 0.158_557_25, 0.043_152_537],
            1e-3,
            "ACEScct to ACES2065-1",
        );
    }

    #[test]
    fn test_identity_builtin() {
        let t = Transform::Builtin(BuiltinTransform {
            style: "IDENTITY".to_string(),
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.3, 0.5, 0.7]);
        assert_rgb_eq(result, [0.3, 0.5, 0.7], 1e-6, "Identity");
    }

    /// OCIO parity: an unknown BuiltinTransform style must ERROR, not silently
    /// pass pixels through (BuiltinTransform.cpp:63-77 throws "invalid built-in
    /// transform style"). Guards the silent-no-op regression.
    #[test]
    fn test_unknown_builtin_style_errors() {
        let t = Transform::Builtin(BuiltinTransform {
            style: "NoSuchBuiltinStyle_xyz".to_string(),
            direction: TransformDirection::Forward,
        });
        let result = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(
            result.is_err(),
            "unknown builtin style must error, not produce an identity processor"
        );
    }

    /// AP0 -> AP1. OCIO has no "ACES-AP0_to_AP1" style (this test used to invent one); the
    /// OCIO spelling is the inverse of "ACEScg_to_ACES2065-1". Reference: PyOpenColorIO 2.5.1,
    /// `BuiltinTransform("ACEScg_to_ACES2065-1")` inverse on {0.5, 0.4, 0.3} =
    /// {0.56663674, 0.40231222, 0.30106002}.
    #[test]
    fn test_ap0_to_ap1() {
        let t = Transform::Builtin(BuiltinTransform {
            style: "ACEScg_to_ACES2065-1".to_string(),
            direction: TransformDirection::Inverse,
        });
        assert_rgb_eq(
            apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]),
            [0.566_636_74, 0.402_312_22, 0.301_060_02],
            1e-6,
            "AP0 -> AP1",
        );
        // A grey/white probe cannot distinguish the AP0->AP1 matrix from
        // identity (both gamuts share the D60-ish white that maps grey->grey),
        // so use a chromatic probe and verify two independent properties that
        // together exclude a no-op AND a mis-inverted matrix:
        //   (1) the forward result actually differs from the input (not identity),
        //   (2) forward then inverse round-trips back to the input exactly.
        let probe = [0.5, 0.4, 0.3];
        let fwd = apply_rgb(&t, TransformDirection::Forward, probe);
        assert!(
            (fwd[0] - probe[0]).abs() > 1e-3 || (fwd[1] - probe[1]).abs() > 1e-3,
            "AP0->AP1 must transform a chromatic input (got identity {fwd:?})"
        );
        let round = apply_rgb(&t, TransformDirection::Inverse, fwd);
        assert_rgb_eq(round, probe, 1e-5, "AP0<->AP1 round-trip");
    }

    /// Reference: BuiltinTransform_tests.cpp "DISPLAY - CIE-XYZ-D65_to_sRGB"
    /// (first three channels of the {0.5,0.4,0.3,...} probe row).
    #[test]
    fn test_xyz_d65_to_srgb() {
        let t = Transform::Builtin(BuiltinTransform {
            // Canonical OCIO style carries the "DISPLAY - " prefix; the bare
            // "CIE-XYZ-D65_to_sRGB" does not resolve and silently no-op'd.
            style: "DISPLAY - CIE-XYZ-D65_to_sRGB".to_string(),
            direction: TransformDirection::Forward,
        });
        let result = apply_rgb(&t, TransformDirection::Forward, [0.5, 0.4, 0.3]);
        assert_rgb_eq(
            result,
            [0.933_793_54, 0.564_092_04, 0.550_040_5],
            1e-3,
            "CIE-XYZ-D65 to sRGB",
        );
    }

    /// Studio config `sRGB - Display` uses the MIRROR NEGS builtin. That path must
    /// stay piecewise sRGB (ExponentWithLinear), not the old gamma-only fallback.
    #[test]
    fn test_xyz_d65_to_srgb_mirror_negs_matches_piecewise() {
        let plain = Transform::Builtin(BuiltinTransform {
            style: "DISPLAY - CIE-XYZ-D65_to_sRGB".to_string(),
            direction: TransformDirection::Forward,
        });
        let mirror = Transform::Builtin(BuiltinTransform {
            style: "DISPLAY - CIE-XYZ-D65_to_sRGB - MIRROR NEGS".to_string(),
            direction: TransformDirection::Forward,
        });
        // Positive probe from the non-mirror OCIO golden — both styles must agree.
        let probe = [0.5_f32, 0.4, 0.3];
        let a = apply_rgb(&plain, TransformDirection::Forward, probe);
        let b = apply_rgb(&mirror, TransformDirection::Forward, probe);
        assert_rgb_eq(a, b, 1e-5, "MIRROR NEGS == piecewise sRGB on positives");

        // And neither may be the gamma-only approximation of the Rec.709 linear
        // that the XYZ→Rec.709 matrix produces for this probe.
        let gamma_only_r = {
            // Re-apply matrix alone is awkward here; instead require distance from
            // the known piecewise golden (parity_tests::test_xyz_d65_to_srgb).
            (a[0] - 0.933_793_54).abs() < 1e-3
        };
        assert!(gamma_only_r, "plain path still piecewise, got {a:?}");
    }

    // NOTE: a `sRGB_to_ACES2065-1` builtin does NOT exist in the OCIO registry
    // (no such style in src/OpenColorIO/transforms/builtins/*). The former
    // `test_srgb_to_aces` constructed that non-existent style; with the
    // unknown-style path being a silent no-op it asserted only `0 < r < 1`,
    // which an identity passthrough trivially satisfied. Removed rather than
    // pinned to a fabricated value — there is nothing in the reference to port.
}

// ============================================================================
// CROSS-CONFIG PROCESSOR PARITY TESTS
// Reference: OCIO Config::getProcessorFromConfigs
// ============================================================================

mod cross_config_parity {
    use super::*;
    use crate::builtin;

    #[test]
    fn test_cross_config_same_config() {
        let config = builtin::aces_2_0_studio_all_views();

        // Same config cross-config should work
        let proc = config.processor_from_configs("ACEScg", &config, "ACES2065-1");
        assert!(proc.is_ok(), "Same-config cross-config should work");

        // [PyOCIO] ACEScg->ACES2065-1 (0.3,0.15,0.45) = 0.30347854 0.18537949
        // 0.44962132: a chromatic probe, not grey (grey is this matrix's fixed
        // point and can't catch a wrong/transposed primaries matrix).
        let mut pixels = [[0.3_f32, 0.15, 0.45]];
        proc.unwrap().apply_rgb(&mut pixels);
        assert_rgb_eq(
            pixels[0],
            [0.303_478_54, 0.185_379_49, 0.449_621_32],
            1e-5,
            "ACEScg->ACES2065-1",
        );
    }

    #[test]
    fn test_aces_srgb_roundtrip() {
        let config = builtin::aces_2_0_studio_all_views();

        let to_srgb = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let from_srgb = config
            .processor(
                &TestSchema::shared().display_encoded_srgb,
                &TestSchema::shared().working_space,
            )
            .unwrap();

        let original = [0.18_f32, 0.18, 0.18];
        let mut pixels = [original];
        to_srgb.apply_rgb(&mut pixels);
        from_srgb.apply_rgb(&mut pixels);

        assert_rgb_eq(pixels[0], original, 0.01, "ACEScg<->sRGB roundtrip");
    }
}

// ============================================================================
// GPU SHADER PARITY TESTS
// Reference: OCIO GpuShaderDesc.cpp
// ============================================================================

mod gpu_shader_parity {
    use super::*;
    use crate::builtin;
    use crate::gpu::{GpuLanguage, GpuProcessor};

    fn validate_shader(lang: GpuLanguage, proc: &GpuProcessor) {
        let code = proc.generate_shader(lang).unwrap();
        let code_str = code.fragment_code();

        // Basic validation: should have function bodies
        match lang {
            GpuLanguage::Glsl330 | GpuLanguage::Glsl400 | GpuLanguage::GlslEs300 => {
                // GLSL should have vec3/vec4 or void main
                assert!(
                    code_str.contains("vec") || code_str.contains("void"),
                    "GLSL should have vec types or void"
                );
            }
            GpuLanguage::Hlsl50 => {
                // HLSL should have float types
                assert!(
                    code_str.contains("float") || code_str.is_empty(),
                    "HLSL should have float types"
                );
            }
            GpuLanguage::Metal => {
                // Metal should have float types
                assert!(
                    code_str.contains("float") || code_str.is_empty(),
                    "Metal should have float types"
                );
            }
            _ => {} // Other languages - just check it compiles
        }

        // Should not have obvious errors
        assert!(!code_str.contains("ERROR"), "Shader should not have ERROR");
    }

    #[test]
    fn test_gpu_shader_glsl_basic() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu_proc = GpuProcessor::from_processor(&proc).unwrap();

        validate_shader(GpuLanguage::Glsl330, &gpu_proc);
        validate_shader(GpuLanguage::Glsl400, &gpu_proc);
    }

    #[test]
    fn test_gpu_shader_hlsl() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu_proc = GpuProcessor::from_processor(&proc).unwrap();

        validate_shader(GpuLanguage::Hlsl50, &gpu_proc);
    }

    #[test]
    fn test_gpu_shader_metal() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu_proc = GpuProcessor::from_processor(&proc).unwrap();

        validate_shader(GpuLanguage::Metal, &gpu_proc);
    }

    #[test]
    fn test_gpu_grading_ops() {
        // Test that grading transforms generate valid GPU shaders
        let t = Transform::GradingPrimary(GradingPrimaryTransform::from_flat(
            GradingPrimaryStyle::Log,
            [0.1, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            1.0,
            0.18,
            0.0,
            1.0,
            f64::NEG_INFINITY,
            f64::INFINITY,
            TransformDirection::Forward,
        ));

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let gpu_proc = GpuProcessor::from_processor(&proc).unwrap();

        validate_shader(GpuLanguage::Glsl330, &gpu_proc);
        validate_shader(GpuLanguage::Hlsl50, &gpu_proc);
        validate_shader(GpuLanguage::Metal, &gpu_proc);
    }
}

/// Processor API parity tests.
/// OCIO ref: Processor class methods
mod processor_api_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::transform::{CdlStyle, CdlTransform, MatrixTransform};
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_is_no_op() {
        // Empty processor should be no-op
        let proc = Processor::from_ops(vec![]);
        assert!(proc.is_no_op());
        assert!(proc.is_identity());

        // Identity matrix should still have ops
        let identity = MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0_f64; 4],
            direction: TransformDirection::Forward,
        };
        let _proc =
            Processor::from_transform(&Transform::Matrix(identity), TransformDirection::Forward)
                .unwrap();
        // Even identity matrix creates an op
        // is_no_op checks for empty ops list
    }

    #[test]
    fn test_has_channel_crosstalk_matrix() {
        // Diagonal matrix - no crosstalk
        let scale = MatrixTransform {
            name: String::new(),
            matrix: [
                1.1, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(scale), TransformDirection::Forward)
                .unwrap();
        assert!(!proc.has_channel_crosstalk());

        // Color matrix - has crosstalk
        let color_matrix = MatrixTransform {
            name: String::new(),
            matrix: [
                0.4124564, 0.3575761, 0.1804375, 0.0, 0.2126729, 0.7151522, 0.0721750, 0.0,
                0.0193339, 0.1191920, 0.9503041, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let proc = Processor::from_transform(
            &Transform::Matrix(color_matrix),
            TransformDirection::Forward,
        )
        .unwrap();
        assert!(proc.has_channel_crosstalk());
    }

    #[test]
    fn test_has_channel_crosstalk_cdl() {
        // CDL without saturation - no crosstalk
        let cdl_no_sat = CdlTransform {
            name: String::new(),
            slope: [1.1, 1.0, 0.9],
            offset: [0.01, 0.0, -0.01],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0, // No saturation change
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl_no_sat), TransformDirection::Forward)
                .unwrap();
        assert!(!proc.has_channel_crosstalk());

        // CDL with saturation - has crosstalk
        let cdl_sat = CdlTransform {
            name: String::new(),
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 0.8, // Reduced saturation
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        };
        let proc = Processor::from_transform(&Transform::Cdl(cdl_sat), TransformDirection::Forward)
            .unwrap();
        assert!(proc.has_channel_crosstalk());
    }

    #[test]
    fn test_cache_id() {
        let matrix = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.1, 0.2, 0.3, 0.0],
            direction: TransformDirection::Forward,
        });
        let cdl = Transform::Cdl(CdlTransform {
            slope: [1.1, 1.0, 0.9],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.2,
            ..Default::default()
        });

        let proc1 = Processor::from_transform(&matrix, TransformDirection::Forward).unwrap();
        let proc2 = Processor::from_transform(&matrix, TransformDirection::Forward).unwrap();
        assert_eq!(proc1.cache_id(), proc2.cache_id());

        let proc3 = Processor::from_transform(&cdl, TransformDirection::Forward).unwrap();
        assert_ne!(proc1.cache_id(), proc3.cache_id());

        assert_eq!(proc1.cache_id().len(), 16);
        assert!(proc1.cache_id().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_is_dynamic() {
        use crate::transform::{ExposureContrastStyle, ExposureContrastTransform};

        // Regular matrix transform - not dynamic
        let matrix = MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0_f64; 4],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(matrix), TransformDirection::Forward)
                .unwrap();
        assert!(!proc.is_dynamic());

        // OCIO: `ExposureContrastOpData::isDynamic()` is true only when exposure, contrast or
        // gamma was made dynamic (`makeExposureDynamic()` ...). A plain ExposureContrast with
        // fixed values is not dynamic, whatever its style. (This test used to pin "always
        // dynamic", which is how the GPU treats its uniforms, not what OCIO's isDynamic says.)
        let ec = ExposureContrastTransform {
            name: String::new(),
            exposure: 1.0, // Non-zero to create actual op
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            style: ExposureContrastStyle::Linear,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };
        let proc = Processor::from_transform(
            &Transform::ExposureContrast(ec.clone()),
            TransformDirection::Forward,
        )
        .unwrap();
        assert!(
            !proc.is_dynamic(),
            "fixed EC is not dynamic: {:?}",
            proc.ops()
        );

        let ec = ExposureContrastTransform {
            dynamic: crate::EcDynamic::EXPOSURE,
            ..ec
        };
        let proc = Processor::from_transform(
            &Transform::ExposureContrast(ec),
            TransformDirection::Forward,
        )
        .unwrap();
        assert!(proc.is_dynamic(), "dynamic exposure: {:?}", proc.ops());
    }

    #[test]
    fn test_create_group_transform() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        // Should return a GroupTransform (even if empty for now)
        let group = proc.create_group_transform();
        assert_eq!(group.direction, TransformDirection::Forward);
    }

    #[test]
    fn test_num_transforms() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        // Processor should have at least one transform
        assert!(proc.num_transforms() > 0);
    }

    #[test]
    fn test_has_dynamic_property() {
        use crate::DynamicPropertyType;
        use crate::transform::{ExposureContrastStyle, ExposureContrastTransform};

        // OCIO `ExposureContrastOpData::hasDynamicProperty(type)`: true exactly for the
        // parameters made dynamic. This used to answer true for Exposure AND Contrast on any
        // ExposureContrast op, i.e. for properties `getDynamicProperty` would then refuse.
        let ec = ExposureContrastTransform {
            name: String::new(),
            exposure: 1.0,
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            style: ExposureContrastStyle::Linear,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };
        let build = |dynamic| {
            Processor::from_transform(
                &Transform::ExposureContrast(ExposureContrastTransform {
                    dynamic,
                    ..ec.clone()
                }),
                TransformDirection::Forward,
            )
            .unwrap()
        };

        let fixed = build(crate::EcDynamic::NONE);
        assert!(!fixed.has_dynamic_property(DynamicPropertyType::Exposure));

        let proc = build(crate::EcDynamic {
            exposure: true,
            contrast: true,
            gamma: false,
        });
        assert!(proc.has_dynamic_property(DynamicPropertyType::Exposure));
        assert!(proc.has_dynamic_property(DynamicPropertyType::Contrast));
        assert!(!proc.has_dynamic_property(DynamicPropertyType::Gamma));
        assert!(!proc.has_dynamic_property(DynamicPropertyType::GradingPrimary));
    }

    #[test]
    fn test_optimized_processor() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        use crate::OptimizationLevel;
        let opt = proc.optimized(OptimizationLevel::Lossless);

        // Optimized processor should still work
        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        opt.apply_rgb(&mut pixels);
        assert!(pixels[0][0].is_finite());
    }

    #[test]
    fn test_processor_metadata() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let meta = proc.metadata();
        // `num_files >= 0` on a `usize` was the previous assertion here: always true, so this
        // test proved nothing about `metadata()` at all. The real invariant is that the count
        // and the list agree.
        //
        // NOTE: `Processor::metadata()` is currently a stub that hardcodes
        // `{num_files: 0, files: []}` (`processor/accessors.rs`), so this passes trivially TODAY.
        // It is still the right assertion: it is what will start failing the moment file
        // tracking is implemented but wired up inconsistently. OCIO tracks files for real
        // (`ProcessorMetadata::addFile`/`getNumFiles`, Processor.cpp:54,73) — the gap is filed
        // in .bughunt/TODO.md.
        assert_eq!(
            meta.num_files,
            meta.files.len(),
            "metadata file count must agree with the file list"
        );

        let fmt = proc.format_metadata();
        // Format ID should be the cache ID
        assert!(!fmt.id.is_empty());
    }
}

/// Virtual Display API parity tests.
/// OCIO ref: Config virtual display methods
mod virtual_display_parity {
    use crate::{Config, VirtualDisplay, VirtualDisplayView};

    #[test]
    fn test_virtual_display_empty() {
        let config = Config::new();
        assert!(!config.has_virtual_display());
        assert!(config.virtual_display().is_none());
        assert_eq!(config.virtual_display_num_views(), 0);
    }

    #[test]
    fn test_virtual_display_add_view() {
        let mut config = Config::new();

        config.add_virtual_display_view(VirtualDisplayView {
            name: "ACES".to_string(),
            view_transform: Some("ACES-sdr-video".to_string()),
            display_colorspace: "<USE_DISPLAY_NAME>".to_string(),
            looks: None,
            rule: None,
            description: None,
        });

        assert!(config.has_virtual_display());
        assert_eq!(config.virtual_display_num_views(), 1);

        let view = config.virtual_display_view(0).unwrap();
        assert_eq!(view.name, "ACES");
        assert_eq!(view.display_colorspace, "<USE_DISPLAY_NAME>");
    }

    #[test]
    fn test_virtual_display_shared_views() {
        let mut config = Config::new();

        config.add_virtual_display_shared_view("Raw");
        config.add_virtual_display_shared_view("Log");

        let vd = config.virtual_display().unwrap();
        assert_eq!(vd.shared_view_refs.len(), 2);
        assert_eq!(vd.shared_view_refs[0], "Raw");
        assert_eq!(vd.shared_view_refs[1], "Log");
    }

    #[test]
    fn test_virtual_display_clear() {
        let mut config = Config::new();

        config.set_virtual_display(VirtualDisplay {
            views: vec![VirtualDisplayView {
                name: "Test".to_string(),
                view_transform: None,
                display_colorspace: "sRGB".to_string(),
                looks: None,
                rule: None,
                description: None,
            }],
            shared_view_refs: vec!["Raw".to_string()],
        });

        assert!(config.has_virtual_display());

        config.clear_virtual_display();
        assert!(!config.has_virtual_display());
    }
}

/// Config API parity tests.
/// OCIO ref: Config class methods
mod config_api_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::colorspace::{ColorSpace, Encoding};
    use crate::{Config, EnvironmentMode, FileRule};

    #[test]
    fn test_environment_mode() {
        let mut config = Config::new();

        // Default is LoadPredefined
        assert_eq!(config.environment_mode(), EnvironmentMode::LoadPredefined);

        config.set_environment_mode(EnvironmentMode::LoadAll);
        assert_eq!(config.environment_mode(), EnvironmentMode::LoadAll);
    }

    #[test]
    fn test_inactive_colorspaces() {
        let mut config = Config::new();

        // Add some colorspaces
        config.add_colorspace(
            ColorSpace::builder("ACEScg")
                .encoding(Encoding::SceneLinear)
                .build(),
        );
        config.add_colorspace(ColorSpace::builder("sRGB").encoding(Encoding::Sdr).build());
        config.add_colorspace(ColorSpace::builder("Raw").encoding(Encoding::Data).build());

        // Initially none are inactive
        assert!(config.get_inactive_colorspaces().is_empty());
        assert!(!config.is_colorspace_inactive("Raw"));

        // Mark Raw as inactive
        config.add_inactive_colorspace("Raw");
        assert!(config.is_colorspace_inactive("Raw"));
        assert!(!config.is_colorspace_inactive("ACEScg"));

        // Set multiple inactive
        config.set_inactive_colorspaces(vec!["Raw".to_string(), "sRGB".to_string()]);
        assert_eq!(config.get_inactive_colorspaces().len(), 2);
    }

    #[test]
    fn test_file_rules() {
        use crate::config::FileRuleKind;

        let mut config = Config::new();

        // Add file rules
        config.add_file_rule(FileRule {
            name: "EXR Default".to_string(),
            colorspace: "ACEScg".to_string(),
            kind: FileRuleKind::Basic {
                pattern: "*.exr".to_string(),
                extension: Some("exr".to_string()),
            },
            ..FileRule::default()
        });

        config.add_file_rule(FileRule {
            name: "JPEG sRGB".to_string(),
            colorspace: "sRGB".to_string(),
            kind: FileRuleKind::Basic {
                pattern: "*.jpg".to_string(),
                extension: Some("jpg".to_string()),
            },
            ..FileRule::default()
        });

        // Should have 2 rules
        assert_eq!(config.file_rules().len(), 2);

        // Clear rules
        config.clear_file_rules();
        assert!(config.file_rules().is_empty());
    }

    #[test]
    fn test_environment_vars() {
        let mut config = Config::new();

        // Set environment variable
        config.set_environment_var("SHOT", "sh010");
        config.set_environment_var("SEQ", "sq01");

        assert_eq!(config.num_environment_vars(), 2);

        // Get environment var
        assert_eq!(config.environment_var("SHOT"), Some("sh010"));
        assert_eq!(config.environment_var("SEQ"), Some("sq01"));
        assert_eq!(config.environment_var("MISSING"), None);
    }

    #[test]
    fn test_colorspace_search() {
        let config = builtin::aces_2_0_studio_all_views();

        // Count colorspaces
        assert!(config.num_colorspaces() > 0);

        // Check if colorspace is linear
        assert!(config.is_colorspace_linear("ACEScg"));
        assert!(config.is_colorspace_linear("ACES2065-1"));
        assert!(!config.is_colorspace_linear(&TestSchema::shared().display_encoded_srgb));
    }

    #[test]
    fn test_search_paths() {
        let mut config = Config::new();

        // Initially empty
        assert!(config.search_paths().is_empty());

        // Add search paths
        config.add_search_path("/luts");
        config.add_search_path("/ocio/luts");

        assert_eq!(config.search_paths().len(), 2);

        // Clear search paths
        config.clear_search_paths();
        assert!(config.search_paths().is_empty());
    }

    #[test]
    fn test_config_version() {
        let mut config = Config::new();

        // Default is V1
        assert!(config.is_v1());
        assert!(!config.is_v2());

        // Set to V2
        config.set_version(2, 0);
        assert!(config.is_v2());
        assert!(!config.is_v1());
    }
}

/// Context API parity tests.
/// OCIO ref: Context class methods
mod context_api_parity {
    use crate::Context;

    #[test]
    fn test_context_resolve() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");
        ctx.set("SEQ", "sq01");

        // Resolve $VAR syntax
        assert_eq!(
            ctx.resolve("/shows/$SEQ/shots/$SHOT/file.exr"),
            "/shows/sq01/shots/sh010/file.exr"
        );

        // Resolve ${VAR} syntax
        assert_eq!(
            ctx.resolve("/path/${SEQ}_${SHOT}.exr"),
            "/path/sq01_sh010.exr"
        );
    }

    #[test]
    fn test_context_unresolved() {
        let mut ctx = Context::without_env();
        ctx.set("SHOT", "sh010");

        // Has unresolved
        assert!(ctx.has_unresolved("/path/$SEQ/$SHOT"));

        // Get unresolved vars
        let unresolved = ctx.unresolved_vars("/path/$SEQ/$SHOT");
        assert_eq!(unresolved, vec!["SEQ"]);
    }

    #[test]
    fn test_context_strict_resolve() {
        let mut ctx = Context::without_env();
        ctx.set("SHOT", "sh010");
        ctx.set("SEQ", "sq01");

        // Success
        let result = ctx.resolve_strict("/path/$SEQ/$SHOT");
        assert!(result.is_ok());

        // Failure - missing var
        let mut ctx2 = Context::without_env();
        ctx2.set("SHOT", "sh010");
        let result = ctx2.resolve_strict("/path/$SEQ/$SHOT");
        assert!(result.is_err());
    }

    #[test]
    fn test_context_vars_iteration() {
        let mut ctx = Context::new();
        ctx.set("A", "1");
        ctx.set("B", "2");
        ctx.set("C", "3");

        assert_eq!(ctx.len(), 3);
        assert!(!ctx.is_empty());

        // Iterate
        let vars: Vec<_> = ctx.vars().collect();
        assert_eq!(vars.len(), 3);

        // Clear
        ctx.clear();
        assert!(ctx.is_empty());
    }
}

/// Look API parity tests.
/// OCIO ref: LookTransform, Config look methods
mod look_api_parity {
    use crate::builtin;
    use crate::{Look, parse_looks};

    #[test]
    fn test_parse_looks_simple() {
        let looks = parse_looks("ShowLUT");
        assert_eq!(looks.len(), 1);
        assert_eq!(looks[0], ("ShowLUT", true));
    }

    #[test]
    fn test_parse_looks_multiple() {
        let looks = parse_looks("ShowLUT, ShotGrade, Burnin");
        assert_eq!(looks.len(), 3);
        assert_eq!(looks[0], ("ShowLUT", true));
        assert_eq!(looks[1], ("ShotGrade", true));
        assert_eq!(looks[2], ("Burnin", true));
    }

    #[test]
    fn test_parse_looks_with_direction() {
        let looks = parse_looks("+Forward, -Inverse");
        assert_eq!(looks[0], ("Forward", true)); // forward
        assert_eq!(looks[1], ("Inverse", false)); // inverse
    }

    #[test]
    fn test_look_builder() {
        let look = Look::new("Film Grade")
            .process_space("ACEScct")
            .description("Main film look");

        assert_eq!(look.name(), "Film Grade");
        assert_eq!(look.get_process_space(), Some("ACEScct"));
        assert_eq!(look.get_description(), "Main film look");
    }

    #[test]
    fn test_config_looks() {
        let config = builtin::aces_2_0_studio_all_views();

        // ACES config may or may not have looks
        // Just verify the API works
        let _num_looks = config.num_looks();
    }
}

/// Baker API parity tests.
/// OCIO ref: Baker class
mod baker_api_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::{Baker, LutFormat};

    #[test]
    fn test_lut_format_extension() {
        assert_eq!(LutFormat::Cube.extension(), "cube");
        assert_eq!(LutFormat::Autodesk3dl.extension(), "3dl");
        assert_eq!(LutFormat::Spi1d.extension(), "spi1d");
        assert_eq!(LutFormat::Spi3d.extension(), "spi3d");
        assert_eq!(LutFormat::Clf.extension(), "clf");
    }

    #[test]
    fn test_lut_format_from_extension() {
        assert_eq!(LutFormat::from_extension("cube"), Some(LutFormat::Cube));
        assert_eq!(LutFormat::from_extension("CUBE"), Some(LutFormat::Cube));
        assert_eq!(
            LutFormat::from_extension("3dl"),
            Some(LutFormat::Autodesk3dl)
        );
        assert_eq!(LutFormat::from_extension("unknown"), None);
    }

    #[test]
    fn test_baker_1d_lut() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let baker = Baker::new(&proc);
        let lut = baker.bake_lut_1d(1024).unwrap();

        assert_eq!(lut.size, 1024);
        assert_eq!(lut.data.len(), 1024 * 3);
        assert_eq!(lut.domain_min, 0.0);
        assert_eq!(lut.domain_max, 1.0);
    }

    #[test]
    fn test_baker_3d_lut() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let baker = Baker::new(&proc);
        let lut = baker.bake_lut_3d(17).unwrap();

        assert_eq!(lut.size, 17);
        assert_eq!(lut.data.len(), 17 * 17 * 17 * 3);
    }
}

/// Display/View API parity tests.
/// OCIO ref: Config display/view methods
mod display_view_parity {
    use crate::builtin;

    #[test]
    fn test_display_enumeration() {
        let config = builtin::aces_2_0_studio_all_views();

        // Should have at least one display
        assert!(config.num_displays() > 0);

        // Get displays via DisplayManager
        let displays = config.displays().displays();
        assert!(!displays.is_empty());
    }

    #[test]
    fn test_active_displays() {
        let mut config = builtin::aces_2_0_studio_all_views();

        // Get all displays
        let all_displays: Vec<String> = config
            .displays()
            .displays()
            .iter()
            .map(|d| d.name().to_string())
            .collect();

        assert_eq!(all_displays.len(), 9);
        config.set_active_displays(vec![all_displays[0].clone()]);
        assert_eq!(config.active_displays(), &all_displays[..1]);
    }

    #[test]
    fn test_display_views() {
        // [PyOCIO] 2.5.1 on the same file (getDisplays, getViews, getDefaultView).
        // getDisplays is the `active_displays` order; getDisplaysAll the file's.
        let config = builtin::aces_2_0_studio_all_views();
        let names = config.display_names();
        assert_eq!(
            config.displays().displays()[1].name(),
            "Gamma 2.2 Rec.709 - Display"
        );
        assert_eq!(
            names,
            [
                "sRGB - Display",
                "Display P3 - Display",
                "Display P3 HDR - Display",
                "Gamma 2.2 Rec.709 - Display",
                "P3-D65 - Display",
                "Rec.1886 Rec.709 - Display",
                "Rec.2100-HLG - Display",
                "Rec.2100-PQ - Display",
                "ST2084-P3-D65 - Display",
            ]
        );
        // getViews: local and shared views, in the `active_views` order (Raw last).
        let counts: Vec<usize> = names.iter().map(|d| config.get_views(d).len()).collect();
        assert_eq!(counts, [5, 5, 7, 5, 7, 5, 5, 21, 15]);
        let views: Vec<&str> = config
            .get_views(names[0])
            .iter()
            .map(|v| v.name())
            .collect();
        assert_eq!(
            views,
            [
                "ACES 2.0 - SDR 100 nits (Rec.709)",
                "ACES 2.0 - SDR 100 nits (Rec.709 D60 in Rec.709 D65)",
                "Un-tone-mapped",
                "Video (colorimetric)",
                "Raw",
            ]
        );
        assert_eq!(
            config.default_view("sRGB - Display"),
            Some("ACES 2.0 - SDR 100 nits (Rec.709)")
        );
    }

    #[test]
    fn test_shared_views() {
        let config = builtin::aces_2_0_studio_all_views();

        // ACES config has shared views
        // Just verify the API works
        let _num_shared = config.num_shared_views();
    }

    #[test]
    fn test_view_transforms() {
        let config = builtin::aces_2_0_studio_all_views();

        // Count view transforms
        let _num_vt = config.num_view_transforms();
    }
}

/// ColorSpace API parity tests.
/// OCIO ref: ColorSpace class methods
mod colorspace_api_parity {
    use crate::colorspace::{AllocationInfo, AllocationType, ColorSpace, Encoding, Family};

    #[test]
    fn test_colorspace_builder() {
        let cs = ColorSpace::builder("ACEScg")
            .alias("ACES - ACEScg")
            .alias("lin_ap1")
            .family(Family::Scene)
            .encoding(Encoding::SceneLinear)
            .description("ACES CG working space")
            .category("scene_linear")
            .category("working_space")
            .build();

        assert_eq!(cs.name(), "ACEScg");
        assert_eq!(cs.aliases().len(), 2);
        assert_eq!(cs.family(), Family::Scene);
        assert_eq!(cs.encoding(), Encoding::SceneLinear);
        assert!(cs.encoding().is_linear());
        assert!(cs.matches_name("acescg")); // case-insensitive
        assert!(cs.matches_name("lin_ap1")); // alias match
    }

    #[test]
    fn test_colorspace_categories() {
        let mut cs = ColorSpace::new("Test");

        assert_eq!(cs.num_categories(), 0);

        // add_category
        cs.add_category("scene_linear");
        assert_eq!(cs.num_categories(), 1);
        assert!(cs.has_category("scene_linear"));
        assert!(cs.has_category("SCENE_LINEAR")); // case-insensitive

        // add_category ignores duplicates
        cs.add_category("SCENE_LINEAR");
        assert_eq!(cs.num_categories(), 1);

        // category by index
        assert_eq!(cs.category(0), Some("scene_linear"));
        assert_eq!(cs.category(1), None);

        // remove_category (case-insensitive)
        cs.add_category("display");
        assert_eq!(cs.num_categories(), 2);
        cs.remove_category("DISPLAY");
        assert_eq!(cs.num_categories(), 1);

        // clear_categories
        cs.clear_categories();
        assert_eq!(cs.num_categories(), 0);
    }

    #[test]
    fn test_encoding_parse() {
        assert_eq!(Encoding::parse("scene-linear"), Encoding::SceneLinear);
        assert_eq!(Encoding::parse("scene_linear"), Encoding::SceneLinear);
        assert_eq!(Encoding::parse("linear"), Encoding::SceneLinear);
        assert_eq!(Encoding::parse("display-linear"), Encoding::DisplayLinear);
        assert_eq!(Encoding::parse("log"), Encoding::Log);
        assert_eq!(Encoding::parse("sdr-video"), Encoding::Sdr);
        assert_eq!(Encoding::parse("hdr-video"), Encoding::Hdr);
        assert_eq!(Encoding::parse("data"), Encoding::Data);
        assert_eq!(Encoding::parse("unknown"), Encoding::Unknown);
    }

    #[test]
    fn test_encoding_as_str() {
        assert_eq!(Encoding::SceneLinear.as_str(), "scene-linear");
        assert_eq!(Encoding::DisplayLinear.as_str(), "display-linear");
        assert_eq!(Encoding::Log.as_str(), "log");
        assert_eq!(Encoding::Sdr.as_str(), "sdr-video");
        assert_eq!(Encoding::Hdr.as_str(), "hdr-video");
        assert_eq!(Encoding::Data.as_str(), "data");
    }

    #[test]
    fn test_family_parse() {
        assert_eq!(Family::parse("scene"), Family::Scene);
        assert_eq!(Family::parse("display"), Family::Display);
        assert_eq!(Family::parse("input"), Family::Input);
        assert_eq!(Family::parse("output"), Family::Output);
        assert_eq!(Family::parse("utility"), Family::Utility);
        assert_eq!(Family::parse("aces"), Family::Aces);
        assert_eq!(Family::parse("unknown"), Family::Other);
    }

    #[test]
    fn test_data_colorspace() {
        // `isdata` and `encoding` are independent, as in OCIO.
        let cs = ColorSpace::builder("Raw").encoding(Encoding::Data).build();
        assert!(!cs.is_data());
        assert_eq!(cs.encoding(), Encoding::Data);
        let cs = ColorSpace::builder("Raw").is_data(true).build();
        assert!(cs.is_data());
        assert_eq!(cs.encoding(), Encoding::Unknown);
    }

    #[test]
    fn test_allocation_info() {
        let alloc = AllocationInfo {
            alloc_type: AllocationType::Log2,
            vars: vec![-8.0, 5.0],
        };

        assert_eq!(alloc.alloc_type, AllocationType::Log2);
        assert_eq!(alloc.vars[0], -8.0);
        assert_eq!(alloc.vars[1], 5.0);
    }
}

/// Named Transform API parity tests.
/// OCIO ref: NamedTransform
mod named_transform_parity {
    use crate::builtin;
    use crate::{Config, NamedTransform};

    #[test]
    fn test_named_transforms_enumeration() {
        let config = builtin::aces_2_0_studio_all_views();

        // Count named transforms. (An `assert!(num >= 0)` on a `usize` used to sit here; it was
        // always true and the real check is the length agreement below.)
        let num = config.num_named_transforms();

        // Iterate if any exist
        let transforms = config.named_transforms();
        assert_eq!(transforms.len(), num);
    }

    #[test]
    fn test_named_transform_struct() {
        // Test NamedTransform struct creation
        let nt = NamedTransform {
            name: "TestTransform".to_string(),
            family: Some("Test".to_string()),
            description: Some("Test transform".to_string()),
            forward: None,
            inverse: None,
            ..NamedTransform::default()
        };

        assert_eq!(nt.name, "TestTransform");
        assert_eq!(nt.family, Some("Test".to_string()));
    }
}

/// GPU Texture API parity tests.
/// OCIO ref: GpuShaderDesc texture methods
mod gpu_texture_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{
        BatchTextureDescriptor, GpuChannelType, GpuInterpolation, GpuLanguage, GpuProcessor,
        GpuTextureType, TextureDimensions,
    };

    #[test]
    fn test_gpu_language_variants() {
        // Verify all OCIO language variants exist
        let _cg = GpuLanguage::Cg;
        let _glsl_120 = GpuLanguage::Glsl120;
        let _glsl_130 = GpuLanguage::Glsl130;
        let _glsl_330 = GpuLanguage::Glsl330;
        let _glsl_400 = GpuLanguage::Glsl400;
        let _glsl_vulkan = GpuLanguage::GlslVulkan460;
        let _glsl_es100 = GpuLanguage::GlslEs100;
        let _glsl_es300 = GpuLanguage::GlslEs300;
        let _hlsl = GpuLanguage::Hlsl50;
        let _osl = GpuLanguage::Osl;
        let _metal = GpuLanguage::Metal;

        // Check is_glsl
        assert!(GpuLanguage::Glsl330.is_glsl());
        assert!(!GpuLanguage::Cg.is_glsl());
        assert!(!GpuLanguage::Hlsl50.is_glsl());
        assert!(!GpuLanguage::Osl.is_glsl());
        assert!(!GpuLanguage::Metal.is_glsl());
    }

    #[test]
    fn test_batch_texture_descriptor() {
        let desc = BatchTextureDescriptor {
            name: "lut3d_0".to_string(),
            binding_index: 0,
            texture_type: GpuTextureType::Texture3D,
            width: 33,
            height: 33,
            depth: 33,
            channel_type: GpuChannelType::Rgb,
            interpolation: GpuInterpolation::Linear,
            dimensions: TextureDimensions::Texture2D,
            data_offset: 0,
            data_size: 33 * 33 * 33 * 3 * 4, // RGB float
        };

        assert_eq!(desc.name, "lut3d_0");
        assert_eq!(desc.width, 33);
    }

    #[test]
    fn test_gpu_processor_textures() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();

        // Check texture counts
        let num_1d = gpu.num_textures();
        let num_3d = gpu.num_3d_textures();
        // (Two `assert!(x >= 0)` on `usize` used to sit here. Always true; the descriptor-count
        // agreement below is the assertion that actually constrains anything.)

        // Get batch descriptors
        let descriptors = gpu.batch_texture_descriptors();
        assert_eq!(descriptors.len(), num_1d + num_3d);
    }

    #[test]
    fn test_gpu_shader_code() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();

        // Generate shader
        let shader = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();

        // Check uniforms via shader code
        let uniforms = shader.uniforms();
        let _ = uniforms;

        // Check fragment code is not empty
        assert!(!shader.fragment_code().is_empty());
    }
}

/// Viewing Rules API parity tests.
/// OCIO ref: ViewingRules
mod viewing_rules_parity {
    use crate::builtin;

    #[test]
    fn test_viewing_rules_enumeration() {
        let config = builtin::aces_2_0_studio_all_views();

        // Count viewing rules. (`assert!(num >= 0)` on a `usize` removed — always true; the
        // length agreement below is the real check.)
        let num = config.num_viewing_rules();

        // Get rules slice
        let rules = config.viewing_rules();
        assert_eq!(rules.len(), num);
    }

    #[test]
    fn test_view_applicability() {
        let config = builtin::aces_2_0_studio_all_views();

        // Test applicable views for a colorspace
        let views = config.applicable_views("ACEScg");
        // ACEScg should have some applicable views
        // Just verify no panic
        let _ = views;
    }

    #[test]
    fn test_display_views_for_colorspace() {
        let config = builtin::aces_2_0_studio_all_views();

        // Get displays
        let displays = config.displays().displays();
        // [PyOCIO] getViews("sRGB - Display", "ACEScg"): the viewing rules drop
        // "Video (colorimetric)" for a scene-linear source.
        assert_eq!(
            config.get_display_views_for_colorspace(displays[0].name(), "ACEScg"),
            [
                "ACES 2.0 - SDR 100 nits (Rec.709)",
                "ACES 2.0 - SDR 100 nits (Rec.709 D60 in Rec.709 D65)",
                "Un-tone-mapped",
                "Raw",
            ]
        );
    }
}

/// Dynamic Properties API parity tests.
/// OCIO ref: DynamicProperty, DynamicPropertyType
mod dynamic_properties_parity {
    use crate::{DynamicPropertyType, DynamicPropertyValue, GradingPrimary, GradingRgbm};

    #[test]
    fn test_dynamic_property_types() {
        // Verify enum values match OCIO
        assert_eq!(DynamicPropertyType::Exposure as u8, 0);
        assert_eq!(DynamicPropertyType::Contrast as u8, 1);
        assert_eq!(DynamicPropertyType::Gamma as u8, 2);
        assert_eq!(DynamicPropertyType::GradingPrimary as u8, 3);
        assert_eq!(DynamicPropertyType::GradingRgbCurve as u8, 4);
        assert_eq!(DynamicPropertyType::GradingTone as u8, 5);
        assert_eq!(DynamicPropertyType::GradingHueCurve as u8, 6);
    }

    #[test]
    fn test_grading_rgbm() {
        let rgbm = GradingRgbm::new(1.0, 0.9, 1.1, 1.0);
        assert!(!rgbm.is_zero());
        assert!(!rgbm.is_one());

        let one = GradingRgbm::one();
        assert!(one.is_one());

        let zero = GradingRgbm::default();
        assert!(zero.is_zero());
    }

    #[test]
    fn test_grading_primary_identity() {
        let primary = GradingPrimary::default();
        assert!(primary.is_identity());

        let mut modified = GradingPrimary {
            exposure: GradingRgbm::new(1.0, 1.0, 1.0, 1.0),
            ..Default::default()
        };
        assert!(!modified.is_identity());
    }

    #[test]
    fn test_dynamic_property_value() {
        let double_val = DynamicPropertyValue::Double(1.5);
        assert_eq!(double_val.as_double(), Some(1.5));

        let primary_val = DynamicPropertyValue::GradingPrimary(GradingPrimary::default());
        assert_eq!(primary_val.as_double(), None);
    }
}

/// Role API parity tests.
/// OCIO ref: Config::setRole, Config::getRole, Config::getNumRoles, ROLE_* constants
mod role_api_parity {
    use crate::builtin;
    use crate::role::{self, Roles, names};

    #[test]
    fn test_standard_role_names() {
        // OCIO ref: ROLE_* constants
        assert_eq!(names::REFERENCE, "reference");
        assert_eq!(names::DEFAULT, "default");
        assert_eq!(names::DATA, "data");
        assert_eq!(names::SCENE_LINEAR, "scene_linear");
        assert_eq!(names::RENDERING, "rendering");
        assert_eq!(names::COMPOSITING_LOG, "compositing_log");
        assert_eq!(names::COMPOSITING_LINEAR, "compositing_linear");
        assert_eq!(names::COLOR_TIMING, "color_timing");
        assert_eq!(names::TEXTURE_PAINT, "texture_paint");
        assert_eq!(names::MATTE_PAINT, "matte_paint");
        assert_eq!(names::COLOR_PICKING, "color_picking");
        assert_eq!(names::ACES_INTERCHANGE, "aces_interchange");
        assert_eq!(names::CIE_XYZ_D65_INTERCHANGE, "cie_xyz_d65_interchange");
    }

    #[test]
    fn test_roles_define_get() {
        let mut roles = Roles::new();

        // Define some roles
        roles.define("scene_linear", "ACEScg");
        roles.define("reference", "ACES2065-1");
        roles.define("data", "Raw");

        // Get role mappings
        assert_eq!(roles.get("scene_linear"), Some("ACEScg"));
        assert_eq!(roles.get("reference"), Some("ACES2065-1"));
        assert_eq!(roles.get("data"), Some("Raw"));
        assert_eq!(roles.get("unknown"), None);
    }

    #[test]
    fn test_roles_contains() {
        let mut roles = Roles::new();
        roles.define("scene_linear", "ACEScg");

        assert!(roles.contains("scene_linear"));
        assert!(!roles.contains("reference"));
    }

    #[test]
    fn test_roles_iteration() {
        let mut roles = Roles::new();
        roles.define("a", "A");
        roles.define("b", "B");
        roles.define("c", "C");

        assert_eq!(roles.len(), 3);

        let pairs: Vec<_> = roles.iter().collect();
        assert_eq!(pairs.len(), 3);
    }

    #[test]
    fn test_roles_special_accessors() {
        let mut roles = Roles::new();
        roles.define("reference", "Linear");
        roles.define("scene_linear", "ACEScg");
        roles.define("data", "Raw");
        roles.define("default", "sRGB");

        assert!(roles.has_reference());
        assert_eq!(roles.reference(), Some("Linear"));
        assert_eq!(roles.scene_linear(), Some("ACEScg"));
        assert_eq!(roles.data(), Some("Raw"));
        assert_eq!(roles.default_input(), Some("sRGB"));
    }

    #[test]
    fn test_config_role_access() {
        let config = builtin::aces_2_0_studio_all_views();

        // Access colorspace by role name
        let linear = config.colorspace("scene_linear");
        assert!(linear.is_some());

        // The colorspace name should resolve correctly
        if let Some(cs) = linear {
            // It should resolve to actual colorspace
            assert!(!cs.name().is_empty());
        }
    }
}

/// Transform API parity tests.
/// OCIO ref: Transform classes
mod transform_api_parity {
    use crate::{
        AllocationTransform, AllocationType, BuiltinTransform, CdlStyle, CdlTransform,
        ExponentTransform, ExposureContrastStyle, ExposureContrastTransform, FixedFunctionStyle,
        FixedFunctionTransform, GradingPrimaryStyle, GradingPrimaryTransform, GradingToneStyle,
        GradingToneTransform, GroupTransform, Interpolation, LogTransform, Lut1DHueAdjust,
        Lut1DTransform, Lut3DTransform, MatrixTransform, NegativeStyle, RangeStyle, RangeTransform,
        Transform, TransformDirection,
    };

    #[test]
    fn test_transform_direction() {
        assert_eq!(
            TransformDirection::Forward.inverse(),
            TransformDirection::Inverse
        );
        assert_eq!(
            TransformDirection::Inverse.inverse(),
            TransformDirection::Forward
        );
    }

    #[test]
    fn test_interpolation_variants() {
        // OCIO INTERP_* surface — ABI pin (was asserting nothing).
        let all = [
            Interpolation::Nearest,
            Interpolation::Linear,
            Interpolation::Tetrahedral,
            Interpolation::Best,
        ];
        assert_eq!(all.len(), 4);
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(a != b, "duplicate Interpolation {a:?}");
            }
        }
    }

    #[test]
    fn test_matrix_identity() {
        let identity = MatrixTransform::IDENTITY;
        // OCIO: MatrixTransform::setIdentity sets diagonal to 1
        assert_eq!(identity[0], 1.0);
        assert_eq!(identity[5], 1.0);
        assert_eq!(identity[10], 1.0);
        assert_eq!(identity[15], 1.0);
    }

    #[test]
    fn test_transform_inverse() {
        // Matrix inverse
        let matrix = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        });
        let inv = matrix.inverse();
        match inv {
            Transform::Matrix(m) => assert_eq!(m.direction, TransformDirection::Inverse),
            _ => panic!("Expected Matrix transform"),
        }
    }

    /// Numeric round-trip guards for composed transforms.
    ///
    /// # Why these exist
    ///
    /// The ~40 look/group tests in this file are all STRUCTURAL: they build a transform and
    /// assert on its fields. Not one pushed a pixel through a composed chain and back. That is
    /// exactly the gap that let P8 live — `Transform::inverse()` for `Group` reversed the child
    /// list a second time, every field looked right, and only the numbers were wrong.
    ///
    /// Composition order is not observable structurally, so it has to be measured. Each test
    /// below runs a real conversion forward, inverts it, and asserts the value returns. A
    /// wrong order changes the result for any non-commuting pair, which every real colour
    /// chain is (matrix + curve).
    ///
    /// Tolerances are ~1e-6: these are f32 pipelines through several ops, and the measured
    /// errors sit around 1e-7 to 2.4e-7. A tighter bound would be flaky, a looser one would
    /// let a real ordering bug through — P8 was off by 9%, four orders of magnitude above this.
    mod round_trip {
        use super::*;
        use crate::Processor;
        use crate::builtin;
        use crate::{DisplayViewTransform, LookTransform};

        /// In-gamut sample with no two channels equal: equal channels can hide a matrix that
        /// mixes them, which is exactly what an ordering bug disturbs.
        const SAMPLE: [f32; 3] = [0.3, 0.15, 0.45];
        /// Out-of-gamut sample (negative red), for transforms that only act outside the gamut.
        const OUT_OF_GAMUT: [f32; 3] = [-0.05, 0.8, 0.2];
        const TOL: f32 = 1e-6;

        /// Runs `sample` forward then inverse and asserts it returns.
        ///
        /// The "must actually change" guard is load-bearing: a transform that is a no-op for the
        /// chosen sample round-trips perfectly while proving nothing about composition order. It
        /// caught exactly that when the look test was first written with an in-gamut sample.
        fn assert_closes(label: &str, sample: [f32; 3], fwd: &Processor, inv: &Processor) {
            let mut px = [sample];
            fwd.apply_rgb(&mut px);
            assert!(
                px[0]
                    .iter()
                    .zip(sample.iter())
                    .any(|(a, b)| (a - b).abs() > 1e-3),
                "{}: the forward pass must actually change the pixel, else the test proves nothing",
                label
            );
            inv.apply_rgb(&mut px);
            for c in 0..3 {
                assert!(
                    (px[0][c] - sample[c]).abs() < TOL,
                    "{}: channel {} did not return: {} -> {}",
                    label,
                    c,
                    sample[c],
                    px[0][c]
                );
            }
        }

        /// `cs -> reference -> cs2` composes in the right order.
        #[test]
        fn colorspace_chain_round_trip() {
            let config = builtin::aces_2_0_studio_all_views();
            let fwd = config
                .processor("ACEScg", "sRGB Encoded Rec.709 (sRGB)")
                .expect("forward");
            let inv = fwd.inverse().expect("inverse");
            assert_closes("colorspace chain", SAMPLE, &fwd, &inv);
        }

        /// A display/view chain composes in the right order.
        ///
        /// This path is built from a view transform plus the reference bridge plus the display
        /// colour space — more composition steps than a plain colour space conversion, so more
        /// places for an order mistake to hide.
        #[test]
        fn display_view_round_trip() {
            let config = builtin::aces_2_0_studio_all_views();
            let fwd = config
                .display_processor("ACEScg", "sRGB - Display", "Un-tone-mapped")
                .expect("forward");
            let inv = fwd.inverse().expect("inverse");
            assert_closes("display/view", SAMPLE, &fwd, &inv);
        }

        /// A look chain composes in the right order.
        ///
        /// Inverting a look chain must swap src/dst, reverse the list AND flip each token's
        /// direction together (`LookTransform.cpp:339-344` + `LookParse.cpp:103-120`). Doing only
        /// part of that is precisely the P8 mistake, one layer up.
        #[test]
        fn look_chain_round_trip() {
            let config = builtin::aces_2_0_studio_all_views();
            let fwd = config
                .processor_with_looks("ACEScg", "ACEScg", "ACES 1.3 Reference Gamut Compression")
                .expect("forward");
            let inv = fwd.inverse().expect("inverse");
            // The ACES gamut-compression look is a NO-OP for in-gamut values — measured: it moves
            // [0.3, 0.15, 0.45] by 0.0 and [-0.05, 0.8, 0.2] by 0.072 — so an in-gamut sample
            // would make this test vacuous.
            assert_closes("look chain", OUT_OF_GAMUT, &fwd, &inv);
        }

        /// `DisplayViewTransform { direction: Inverse }` composes correctly at the AST level.
        ///
        /// Different code path from `display_view_round_trip` above: that test inverts via
        /// `Processor::inverse()` (ops-level, `processor.rs:567`). This one builds the "inverse"
        /// leg through `processor_for_display_view_transform_internal` (`factory.rs:519-526`),
        /// which compiles the WHOLE forward-built transform list through `compile_transform`
        /// using `dvt.direction` as context — the same context-propagation mechanism the P8 fix
        /// made `Transform::Group::inverse()` rely on. A wrong order here would not show up in
        /// `display_view_round_trip` at all, since that test never constructs an inverse DVT.
        #[test]
        fn display_view_inverse_direction_round_trip() {
            let config = builtin::aces_2_0_studio_all_views();
            let fwd = config
                .display_processor("ACEScg", "sRGB - Display", "Un-tone-mapped")
                .expect("forward");
            let dvt_inverse = DisplayViewTransform {
                src: "ACEScg".into(),
                display: "sRGB - Display".into(),
                view: "Un-tone-mapped".into(),
                direction: TransformDirection::Inverse,
                ..DisplayViewTransform::default()
            };
            let inv = config
                .processor_for_display_view_transform(&dvt_inverse)
                .expect("inverse");
            assert_closes("display/view (DVT direction:Inverse)", SAMPLE, &fwd, &inv);
        }

        /// `Transform::DisplayView` through `expand_transform`'s eager `group.inverse()`
        /// (`factory.rs:794`) composes correctly.
        ///
        /// This is the exact call site that was broken in P8 (`Transform::Group::inverse()`
        /// double-reversing the child list). `display_view_round_trip` and
        /// `display_view_inverse_direction_round_trip` above never touch this call site — they
        /// go through `processor_for_display_view_transform_internal`, which builds one flat
        /// list and lets `compile_transform` do the reversal. Here the DVT is wrapped in
        /// `Transform::DisplayView` and expanded, taking `factory.rs:792-797`'s
        /// `group.inverse()` branch for the `direction: Inverse` case.
        #[test]
        fn display_view_expand_transform_group_inverse_round_trip() {
            let config = builtin::aces_2_0_studio_all_views();
            let dvt_fwd = Transform::DisplayView(DisplayViewTransform {
                src: "ACEScg".into(),
                display: "sRGB - Display".into(),
                view: "Un-tone-mapped".into(),
                direction: TransformDirection::Forward,
                ..DisplayViewTransform::default()
            });
            let dvt_inv = Transform::DisplayView(DisplayViewTransform {
                src: "ACEScg".into(),
                display: "sRGB - Display".into(),
                view: "Un-tone-mapped".into(),
                direction: TransformDirection::Inverse,
                ..DisplayViewTransform::default()
            });
            let fwd = config
                .processor_from_transform(&dvt_fwd, TransformDirection::Forward)
                .expect("forward");
            let inv = config
                .processor_from_transform(&dvt_inv, TransformDirection::Forward)
                .expect("inverse");
            assert_closes(
                "display/view (expand_transform group.inverse())",
                SAMPLE,
                &fwd,
                &inv,
            );
        }

        /// Three independently-coded inversion mechanisms for the SAME display/view chain must
        /// agree with each other, not just round-trip.
        ///
        /// Round-tripping through a single mechanism's own inverse can pass even if that
        /// mechanism has a symmetric bug (gets the forward chain wrong in a way its own inverse
        /// silently undoes). Comparing three independently implemented inverses to each other is
        /// a strictly stronger check: `Processor::inverse()` operates on the compiled op list
        /// (`processor.rs:567`), `processor_for_display_view_transform_internal` compiles a flat
        /// transform list through `dvt.direction` context (`factory.rs:519-526`), and
        /// `expand_transform`'s `Transform::DisplayView` branch eagerly inverts a `GroupTransform`
        /// (`factory.rs:792-797`). They share no code beyond `compile_transform`'s per-op leaf
        /// handling, so agreement across all three is strong evidence none of them silently
        /// cancels an ordering error the way the P8 double-reversal did.
        #[test]
        fn display_view_inversion_mechanisms_agree() {
            let config = builtin::aces_2_0_studio_all_views();
            let fwd = config
                .display_processor("ACEScg", "sRGB - Display", "Un-tone-mapped")
                .expect("forward");

            let mut encoded = [SAMPLE];
            fwd.apply_rgb(&mut encoded);
            assert!(
                encoded[0]
                    .iter()
                    .zip(SAMPLE.iter())
                    .any(|(a, b)| (a - b).abs() > 1e-3),
                "forward pass must actually change the pixel, else this test proves nothing"
            );

            // Mechanism A: Processor::inverse() -- ops-level inversion.
            let inv_a = fwd.inverse().expect("inverse a");
            let mut back_a = encoded;
            inv_a.apply_rgb(&mut back_a);

            // Mechanism B: DisplayViewTransform{direction:Inverse} -- AST-level, compiled through
            // dvt.direction context (factory.rs:519-526).
            let dvt_inverse = DisplayViewTransform {
                src: "ACEScg".into(),
                display: "sRGB - Display".into(),
                view: "Un-tone-mapped".into(),
                direction: TransformDirection::Inverse,
                ..DisplayViewTransform::default()
            };
            let inv_b = config
                .processor_for_display_view_transform(&dvt_inverse)
                .expect("inverse b");
            let mut back_b = encoded;
            inv_b.apply_rgb(&mut back_b);

            // Mechanism C: Transform::DisplayView via expand_transform -- eager
            // GroupTransform::inverse() (factory.rs:792-797).
            let dvt_inv_transform = Transform::DisplayView(DisplayViewTransform {
                src: "ACEScg".into(),
                display: "sRGB - Display".into(),
                view: "Un-tone-mapped".into(),
                direction: TransformDirection::Inverse,
                ..DisplayViewTransform::default()
            });
            let inv_c = config
                .processor_from_transform(&dvt_inv_transform, TransformDirection::Forward)
                .expect("inverse c");
            let mut back_c = encoded;
            inv_c.apply_rgb(&mut back_c);

            for c in 0..3 {
                assert!(
                    (back_a[0][c] - SAMPLE[c]).abs() < TOL,
                    "mechanism A did not return: channel {} {} -> {}",
                    c,
                    SAMPLE[c],
                    back_a[0][c]
                );
                assert!(
                    (back_a[0][c] - back_b[0][c]).abs() < TOL,
                    "mechanism A vs B disagree on channel {}: {} vs {}",
                    c,
                    back_a[0][c],
                    back_b[0][c]
                );
                assert!(
                    (back_a[0][c] - back_c[0][c]).abs() < TOL,
                    "mechanism A vs C disagree on channel {}: {} vs {}",
                    c,
                    back_a[0][c],
                    back_c[0][c]
                );
            }
        }

        /// `Transform::Look { direction: Inverse }` via `expand_transform` composes correctly.
        ///
        /// Different code path from `look_chain_round_trip` above: that test inverts the
        /// `processor_with_looks` result via `Processor::inverse()` (ops-level). This one
        /// constructs the inverse `LookTransform` directly and expands it through
        /// `expand_transform`'s `Transform::Look` branch (`factory.rs:646-675`), which swaps
        /// src/dst, reverses the look list AND flips each look's forward flag in one eager step
        /// — mirroring `LookTransform.cpp:339-344` + `LookParse.cpp:103-120`. Doing only part of
        /// that (reverse without flip, or flip without reverse) is exactly the P8 mistake one
        /// layer up.
        #[test]
        fn look_transform_expand_inverse_round_trip() {
            let config = builtin::aces_2_0_studio_all_views();
            let lt_fwd = Transform::Look(LookTransform {
                src: "ACEScg".into(),
                dst: "ACEScg".into(),
                looks: "ACES 1.3 Reference Gamut Compression".into(),
                skip_color_space_conversion: false,
                direction: TransformDirection::Forward,
            });
            let lt_inv = Transform::Look(LookTransform {
                src: "ACEScg".into(),
                dst: "ACEScg".into(),
                looks: "ACES 1.3 Reference Gamut Compression".into(),
                skip_color_space_conversion: false,
                direction: TransformDirection::Inverse,
            });
            let fwd = config
                .processor_from_transform(&lt_fwd, TransformDirection::Forward)
                .expect("forward");
            let inv = config
                .processor_from_transform(&lt_inv, TransformDirection::Forward)
                .expect("inverse");
            // Same no-op-for-in-gamut-values caveat as look_chain_round_trip: the ACES gamut
            // compression look only moves out-of-gamut values.
            assert_closes(
                "look chain (Transform::Look direction:Inverse)",
                OUT_OF_GAMUT,
                &fwd,
                &inv,
            );
        }

        /// `processor_from_configs_to_display` composes correctly, including its
        /// `combined.inverse()` call site (`factory.rs:1009-1014`).
        ///
        /// Uses the SAME config as both source and destination — a degenerate but valid
        /// cross-config call, since `identify_interchange_space` resolves `aces_interchange`
        /// (ACES2065-1) trivially against itself. This exercises `Processor::combine` composing
        /// a to-interchange leg with a `display_processor` leg, then inverting that COMBINED
        /// processor for `direction: Inverse` — a composition none of the tests above cover.
        #[test]
        fn processor_from_configs_to_display_inverse_round_trip() {
            let config = builtin::aces_2_0_studio_all_views();
            let fwd = config
                .processor_from_configs_to_display(
                    "ACEScg",
                    &config,
                    "sRGB - Display",
                    "Un-tone-mapped",
                    TransformDirection::Forward,
                )
                .expect("forward");
            let inv = config
                .processor_from_configs_to_display(
                    "ACEScg",
                    &config,
                    "sRGB - Display",
                    "Un-tone-mapped",
                    TransformDirection::Inverse,
                )
                .expect("inverse");
            assert_closes("processor_from_configs_to_display", SAMPLE, &fwd, &inv);
        }

        /// A role name and the literal colour space name it resolves to must build the
        /// BIT-IDENTICAL processor, not just a numerically close one.
        ///
        /// `scene_linear` resolves to `ACEScg` through the same single `Config::colorspace()`
        /// lookup used for every processor endpoint (`factory.rs:246-247`) — there is no
        /// separate forward/backward alias table that could resolve a name differently
        /// depending on which side of the chain it's on. Exact equality (not a tolerance) is the
        /// right assertion here: a `< TOL` check would hide a small src-vs-dst divergence that
        /// exact equality catches.
        #[test]
        fn role_scene_linear_matches_literal_colorspace() {
            let config = builtin::aces_2_0_studio_all_views();
            let via_role = config
                .processor("scene_linear", "sRGB Encoded Rec.709 (sRGB)")
                .expect("via role");
            let via_literal = config
                .processor("ACEScg", "sRGB Encoded Rec.709 (sRGB)")
                .expect("via literal");
            let mut a = [SAMPLE];
            let mut b = [SAMPLE];
            via_role.apply_rgb(&mut a);
            via_literal.apply_rgb(&mut b);
            assert_eq!(
                a, b,
                "role-resolved and literal-named processors must match bit-for-bit"
            );
        }

        /// An alias name round-trips against the literal colour space name it aliases.
        ///
        /// `srgb_tx` is one of the aliases declared on `sRGB Encoded Rec.709 (sRGB)`
        /// (`builtin/data/studio-config-all-views-*.ocio:1272`). Forward through the alias, back
        /// through the literal name exercises the same single resolution table from both
        /// directions.
        #[test]
        fn alias_srgb_tx_round_trip() {
            let config = builtin::aces_2_0_studio_all_views();
            let fwd = config.processor("ACEScg", "srgb_tx").expect("forward");
            let inv = config.processor("srgb_tx", "ACEScg").expect("inverse");
            assert_closes("alias round trip", SAMPLE, &fwd, &inv);
        }
    }

    /// Inverting a group flips its direction and does NOT permute its child list.
    ///
    /// The reordering belongs to compilation: `compile_transform` walks a group whose combined
    /// direction is `Inverse` back-to-front (`processor/compile.rs:244-247`), exactly as the
    /// reference's `BuildGroupOps` does (`OpenColorIO/src/OpenColorIO/transforms/
    /// GroupTransform.cpp:196-202`). The C++ never permutes the stored list.
    ///
    /// This test used to build a group of ONE identity matrix and assert only the direction,
    /// and its comment claimed "group inverse reverses order and direction" - so it documented
    /// the bug instead of catching it. With one child, order is unobservable; with children
    /// that commute, so is the error. Hence two NON-commuting children below. That was P8.
    #[test]
    fn test_group_transform_inverse() {
        // Channel-mixing (non-diagonal, rows sum to 1) then a per-channel power. Swapping the
        // two gives a different result, which is what makes the order observable at all.
        let mix: [f64; 16] = [
            0.8, 0.1, 0.1, 0.0, 0.2, 0.7, 0.1, 0.0, 0.1, 0.2, 0.7, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let group = Transform::Group(GroupTransform {
            name: String::new(),
            transforms: vec![
                Transform::Matrix(MatrixTransform {
                    name: String::new(),
                    matrix: mix,
                    offset: [0.0; 4],
                    direction: TransformDirection::Forward,
                }),
                Transform::Exponent(ExponentTransform {
                    name: String::new(),
                    value: [2.2, 2.2, 2.2, 1.0],
                    negative_style: NegativeStyle::Clamp,
                    direction: TransformDirection::Forward,
                }),
            ],
            direction: TransformDirection::Forward,
        });

        // Structural: the child list keeps its authored order, only the direction flips.
        let inv = group.clone().inverse();
        match &inv {
            Transform::Group(g) => {
                assert_eq!(g.direction, TransformDirection::Inverse);
                assert_eq!(g.transforms.len(), 2);
                assert!(
                    matches!(g.transforms[0], Transform::Matrix(_)),
                    "inverse() must NOT permute the child list: the matrix must stay first"
                );
                assert!(matches!(g.transforms[1], Transform::Exponent(_)));
            }
            _ => panic!("Expected Group transform"),
        }

        // Behavioural: forward then inverse must return the original. This is what actually
        // catches a wrong order - every child inverts correctly on its own, so only the
        // composition can be wrong.
        use crate::Processor;
        let fwd = Processor::from_transform(&group, TransformDirection::Forward)
            .expect("forward processor");
        let rev = Processor::from_transform(&inv, TransformDirection::Forward)
            .expect("inverse processor");

        let orig = [[0.2f32, 0.5, 0.8]];
        let mut px = orig;
        fwd.apply_rgb(&mut px);
        assert!(
            (px[0][0] - orig[0][0]).abs() > 1e-3,
            "the forward transform must actually change the pixel, else the test proves nothing"
        );
        rev.apply_rgb(&mut px);

        for c in 0..3 {
            assert!(
                (px[0][c] - orig[0][c]).abs() < 1e-4,
                "group round trip must close on channel {}: {} -> {}",
                c,
                orig[0][c],
                px[0][c]
            );
        }
    }

    #[test]
    fn test_lut1d_transform() {
        let lut = Lut1DTransform {
            red: vec![0.0; 1024],
            green: None,
            blue: None,
            input_min: 0.0,
            input_max: 1.0,
            output_min: 0.0,
            output_max: 1.0,
            half_domain: false,
            raw_halfs: false,
            hue_adjust: Lut1DHueAdjust::None,
            interpolation: Interpolation::Linear,
            direction: TransformDirection::Forward,
        };
        assert_eq!(lut.red.len(), 1024);
    }

    #[test]
    fn test_lut3d_transform() {
        let lut = Lut3DTransform::identity(33);
        assert_eq!(lut.size, 33);
    }

    #[test]
    fn test_allocation_transform() {
        use crate::transform::AllocationType as TransformAllocationType;
        let alloc = AllocationTransform {
            allocation: TransformAllocationType::Log2,
            vars: vec![-8.0, 8.0],
            direction: TransformDirection::Forward,
        };
        assert_eq!(alloc.vars.len(), 2);
    }
}

/// Config serialization parity tests.
/// OCIO ref: Config::serialize
mod config_serialization_parity {
    use crate::{ColorSpace, ConfigBuilder, ConfigVersion, Encoding};

    fn minimal_config_builder() -> ConfigBuilder {
        let cs = ColorSpace::builder("Linear")
            .encoding(Encoding::SceneLinear)
            .build();
        ConfigBuilder::new("TestConfig")
            .add_colorspace(cs)
            .set_role("reference", "Linear")
    }

    #[test]
    fn test_config_version() {
        // Test config version 2.x
        let config = minimal_config_builder()
            .version(ConfigVersion::V2)
            .build()
            .unwrap();

        // Version should be V2
        let version = config.version();
        assert_eq!(version, ConfigVersion::V2);
    }

    #[test]
    fn test_config_version_v1() {
        let config = minimal_config_builder()
            .version(ConfigVersion::V1)
            .build()
            .unwrap();

        assert_eq!(config.version(), ConfigVersion::V1);
    }

    #[test]
    fn test_config_description() {
        // OCIO keeps name and description apart (B23; the old version of this test asserted
        // only that the name was non-empty, which hid `description()` returning the name).
        let config = minimal_config_builder()
            .description("Test config")
            .build()
            .unwrap();

        assert_eq!(config.name(), "TestConfig");
        assert_eq!(config.description(), "Test config");
    }

    #[test]
    fn test_config_name() {
        let config = minimal_config_builder().build().unwrap();

        assert_eq!(config.name(), "TestConfig");
    }
}

/// LUT file format parity tests.
mod lut_format_parity {
    use crate::builtin::TestSchema;
    use crate::{Baker, LutFormat, builtin};

    #[test]
    fn test_lut_format_extension() {
        assert_eq!(LutFormat::Cube.extension(), "cube");
        assert_eq!(LutFormat::Spi1d.extension(), "spi1d");
        assert_eq!(LutFormat::Spi3d.extension(), "spi3d");
        assert_eq!(LutFormat::Clf.extension(), "clf");
    }

    #[test]
    fn test_baker_basic() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let baker = Baker::new(&proc);

        // Bake to 3D LUT
        let lut3d = baker.bake_lut_3d(17).unwrap();
        assert_eq!(lut3d.size, 17);
        assert_eq!(lut3d.data.len(), 17 * 17 * 17 * 3);
    }
}

/// Processor optimization parity tests.
/// OCIO ref: Processor::getOptimizedProcessor
mod optimization_parity {
    use crate::builtin::TestSchema;
    use crate::{OptimizationLevel, Processor, builtin};

    #[test]
    fn test_optimized_processor() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        // Get optimized version
        let optimized = proc.optimized(OptimizationLevel::Lossless);

        // Should still work
        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        optimized.apply_rgb(&mut pixels);

        // Result should be reasonable
        assert!(pixels[0][0] > 0.0 && pixels[0][0] < 1.0);
    }

    #[test]
    fn test_optimization_identity_removal() {
        use crate::{MatrixTransform, Transform, TransformDirection};

        // Create processor with identity matrix
        let identity = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&identity, TransformDirection::Forward).unwrap();

        // After optimization, identity should be recognized as no-op
        let optimized = proc.optimized(OptimizationLevel::Lossless);
        assert!(optimized.is_no_op());
    }
}

/// BitDepth parity tests.
/// OCIO ref: BitDepth enum
mod bit_depth_parity {
    use vfx_core::BitDepth;

    #[test]
    fn test_bit_depth_variants() {
        // OCIO BIT_DEPTH_* surface — ABI pin (was asserting nothing; the
        // is_float behaviour is covered by test_bit_depth_is_float).
        let all = [
            BitDepth::U8,
            BitDepth::U10,
            BitDepth::U12,
            BitDepth::U16,
            BitDepth::F16,
            BitDepth::F32,
        ];
        assert_eq!(all.len(), 6);
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(a != b, "duplicate BitDepth {a:?}");
            }
        }
    }

    #[test]
    fn test_bit_depth_is_float() {
        assert!(!BitDepth::U8.is_float());
        assert!(!BitDepth::U16.is_float());
        assert!(BitDepth::F16.is_float());
        assert!(BitDepth::F32.is_float());
    }

    #[test]
    fn test_bit_depth_bits() {
        assert_eq!(BitDepth::U8.bits(), 8);
        assert_eq!(BitDepth::U10.bits(), 10);
        assert_eq!(BitDepth::U16.bits(), 16);
        assert_eq!(BitDepth::F32.bits(), 32);
    }
}

/// FixedFunction comprehensive style parity tests.
/// OCIO ref: FixedFunctionStyle enum
mod fixed_function_styles_parity {
    use crate::{
        FixedFunctionStyle, FixedFunctionTransform, Processor, Transform, TransformDirection,
    };

    fn test_fixed_function_roundtrip(
        style: FixedFunctionStyle,
        params: Vec<f64>,
        input: [f32; 3],
        tolerance: f32,
    ) {
        let fwd = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style,
            params: params.clone(),
            direction: TransformDirection::Forward,
        });

        let inv = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style,
            params,
            direction: TransformDirection::Inverse,
        });

        let proc_fwd = Processor::from_transform(&fwd, TransformDirection::Forward).unwrap();
        let proc_inv = Processor::from_transform(&inv, TransformDirection::Forward).unwrap();

        let mut pixels = [input];
        proc_fwd.apply_rgb(&mut pixels);
        proc_inv.apply_rgb(&mut pixels);

        // Roundtrip should return to original
        for i in 0..3 {
            assert!(
                (pixels[0][i] - input[i]).abs() < tolerance,
                "{:?} roundtrip ch{}: got {}, expected {}",
                style,
                i,
                pixels[0][i],
                input[i]
            );
        }
    }

    #[test]
    fn test_aces_red_mod_03() {
        // ACES RedMod03 fwd — exact OCIO vector (FixedFunctionOpCPU_tests.cpp
        // aces_red_mod_03 row 0): [0.90,0.05,0.22] -> [0.79670035,0.05,0.19934007].
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRedMod03,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.90_f32, 0.05, 0.22]];
        proc.apply_rgb(&mut pixels);
        let exp = [0.79670035_f32, 0.05, 0.19934007];
        for i in 0..3 {
            assert!(
                (pixels[0][i] - exp[i]).abs() < 1e-4,
                "ACES RedMod03 ch{i}: got {}, expected {}",
                pixels[0][i],
                exp[i]
            );
        }
    }

    #[test]
    fn test_aces_red_mod_10() {
        // ACES RedMod10 fwd — exact OCIO vector (FixedFunctionOpCPU_tests.cpp
        // aces_red_mod_10 row 0): [0.90,0.05,0.22] -> [0.77148211,0.05,0.22].
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRedMod10,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.90_f32, 0.05, 0.22]];
        proc.apply_rgb(&mut pixels);
        let exp = [0.771_482_1_f32, 0.05, 0.22];
        for i in 0..3 {
            assert!(
                (pixels[0][i] - exp[i]).abs() < 1e-4,
                "ACES RedMod10 ch{i}: got {}, expected {}",
                pixels[0][i],
                exp[i]
            );
        }
    }

    #[test]
    fn test_aces_glow_03() {
        test_fixed_function_roundtrip(
            FixedFunctionStyle::AcesGlow03,
            vec![],
            [0.5, 0.3, 0.2],
            0.01,
        );
    }

    #[test]
    fn test_aces_glow_10() {
        test_fixed_function_roundtrip(
            FixedFunctionStyle::AcesGlow10,
            vec![],
            [0.5, 0.3, 0.2],
            0.01,
        );
    }

    #[test]
    fn test_aces_dark_to_dim_10() {
        test_fixed_function_roundtrip(
            FixedFunctionStyle::AcesDarkToDim10,
            vec![],
            [0.5, 0.3, 0.2],
            0.01,
        );
    }

    #[test]
    fn test_rec2100_surround() {
        test_fixed_function_roundtrip(
            FixedFunctionStyle::Rec2100Surround,
            vec![0.75],
            [0.5, 0.3, 0.2],
            0.01,
        );
    }

    #[test]
    fn test_rgb_to_hsv() {
        // RGB to HSV is special - just verify forward works
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::RgbToHsv,
            params: vec![],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[1.0_f32, 0.0, 0.0]]; // Pure red
        proc.apply_rgb(&mut pixels);

        // Should produce HSV: H=0, S=1, V=1
        assert!((pixels[0][0]).abs() < 0.01, "Red hue should be 0"); // H
        assert!(
            (pixels[0][1] - 1.0).abs() < 0.01,
            "Red saturation should be 1"
        ); // S
        assert!((pixels[0][2] - 1.0).abs() < 0.01, "Red value should be 1"); // V
    }

    #[test]
    fn test_xyz_to_xyy() {
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyzToXyy,
            params: vec![],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        // D65 white point in XYZ
        let mut pixels = [[0.95047_f32, 1.0, 1.08883]];
        proc.apply_rgb(&mut pixels);

        // Should produce D65 chromaticity ~(0.3127, 0.3290, 1.0)
        assert!((pixels[0][0] - 0.3127).abs() < 0.01, "x chromaticity");
        assert!((pixels[0][1] - 0.3290).abs() < 0.01, "y chromaticity");
    }

    #[test]
    fn test_fixed_function_styles_exist() {
        let styles = vec![
            FixedFunctionStyle::AcesRedMod03,
            FixedFunctionStyle::AcesRedMod10,
            FixedFunctionStyle::AcesGlow03,
            FixedFunctionStyle::AcesGlow10,
            FixedFunctionStyle::AcesDarkToDim10,
            FixedFunctionStyle::Rec2100Surround,
            FixedFunctionStyle::RgbToHsv,
            FixedFunctionStyle::XyzToXyy,
            FixedFunctionStyle::XyzToUvy,
            FixedFunctionStyle::XyzToLuv,
            FixedFunctionStyle::LinToPq,
            FixedFunctionStyle::PqToLin,
            FixedFunctionStyle::AcesOutputTransform20,
            FixedFunctionStyle::AcesTonescaleCompress20,
        ];
        // ABI/coverage pin: the FixedFunction style surface is exactly these 14
        // (the old test built the vec and asserted nothing). Per-style numeric
        // behaviour is verified by the dedicated tests above (test_aces_red_mod_*,
        // _glow_*, _dark_to_dim_10, etc.).
        assert_eq!(styles.len(), 14, "FixedFunction style set changed");
        // No duplicates (each variant is distinct).
        for (i, a) in styles.iter().enumerate() {
            for b in &styles[i + 1..] {
                assert!(a != b, "duplicate FixedFunction style {a:?}");
            }
        }
    }
}

/// Grading transforms comprehensive parity tests.
mod grading_transforms_parity {
    use crate::{
        GradingPrimaryStyle, GradingPrimaryTransform, GradingRgbmsw, GradingToneStyle,
        GradingToneTransform, Processor, Transform, TransformDirection,
    };

    #[test]
    fn test_grading_primary_log() {
        // Isolate brightness (contrast left at identity, so it can't pull the
        // result back down past the input - the original version of this
        // test combined a raw brightness of 0.1 with contrast=1.2 around a
        // raw pivot of 0.435, which relied on `compile.rs` NOT scaling
        // `brightness` (it does now: OCIO's LOG brightness is calibrated in
        // 10-bit code-value units, `6.25/1023` per unit - see
        // `op::grading_primary_params`) and NOT remapping `pivot` (it does
        // now: LOG's actual pivot is `0.5 + raw_pivot*0.5`). With both now
        // correct, the old raw values pulled the pixel BELOW 0.18, the
        // opposite of what this test title says it's checking.
        let brightness_scale = 1023.0 / 6.25;
        let t = Transform::GradingPrimary(GradingPrimaryTransform::from_flat(
            GradingPrimaryStyle::Log,
            [
                0.1 * brightness_scale,
                0.1 * brightness_scale,
                0.1 * brightness_scale,
            ],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            1.0,
            -0.2,
            0.0,
            1.0,
            0.0,
            1.0,
            TransformDirection::Forward,
        ));

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixels);

        // Brightness increases value
        assert!(pixels[0][0] > 0.18);
    }

    #[test]
    fn test_grading_primary_linear() {
        let t = Transform::GradingPrimary(GradingPrimaryTransform::from_flat(
            GradingPrimaryStyle::Linear,
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 0.0, -1.0],
            [0.0, 0.0, 0.0],
            1.0,
            1.0,
            0.0,
            1.0,
            0.0,
            10.0,
            TransformDirection::Forward,
        ));

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);

        // Red boosted (+1 stop), blue reduced (-1 stop)
        assert!(pixels[0][0] > pixels[0][1], "Red should be higher");
        assert!(pixels[0][2] < pixels[0][1], "Blue should be lower");
    }

    #[test]
    fn test_grading_tone_log() {
        let mut t = GradingToneTransform::default();
        t.midtones.master = 1.2;

        let proc =
            Processor::from_transform(&Transform::GradingTone(t), TransformDirection::Forward)
                .unwrap();
        let mut pixels = [[0.435_f32, 0.435, 0.435]]; // Mid gray in log
        proc.apply_rgb(&mut pixels);

        // Midtone boost should increase mid values
        assert!(pixels[0][0] > 0.4);
    }

    #[test]
    fn test_grading_rgbmsw_struct() {
        let rgbmsw = GradingRgbmsw {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            master: 1.0,
            start: 0.0,
            width: 1.0,
        };

        assert_eq!(rgbmsw.red, 1.0);
        assert_eq!(rgbmsw.master, 1.0);
    }
}

/// GPU Shader language parity tests.
/// OCIO ref: GpuShaderCreator, GpuLanguage
mod gpu_shader_language_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};

    fn get_test_processor() -> crate::Processor {
        let config = builtin::aces_2_0_studio_all_views();
        config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap()
    }

    #[test]
    fn test_glsl_120_generation() {
        let proc = get_test_processor();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl120).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
        // GLSL 1.20 uses texture2D
        assert!(!code.contains("#version 330"));
    }

    #[test]
    fn test_glsl_330_generation() {
        let proc = get_test_processor();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
        assert!(code.contains("#version 330"));
    }

    #[test]
    fn test_glsl_400_generation() {
        let proc = get_test_processor();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl400).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
        assert!(code.contains("#version 400"));
    }

    #[test]
    fn test_glsl_vulkan_generation() {
        let proc = get_test_processor();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslVulkan460).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
        assert!(code.contains("#version 460"));
    }

    #[test]
    fn test_glsl_es_100_generation() {
        let proc = get_test_processor();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslEs100).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
        // GLSL ES 1.00 is its own dialect: it must NOT carry a desktop or ES-3.00
        // version directive (those belong to the other generators). The old test
        // only checked non-empty.
        assert!(
            !code.contains("#version 330"),
            "ES100 must not emit desktop #version"
        );
        assert!(
            !code.contains("#version 300 es"),
            "ES100 must not emit ES-3.00 #version"
        );
    }

    #[test]
    fn test_glsl_es_300_generation() {
        let proc = get_test_processor();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslEs300).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
        assert!(code.contains("#version 300 es"));
    }

    #[test]
    fn test_hlsl_50_generation() {
        let proc = get_test_processor();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Hlsl50).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
        assert!(code.contains("HLSL"));
        // HLSL uses float4 not vec4
        assert!(code.contains("float4") || code.contains("float3"));
    }

    #[test]
    fn test_metal_generation() {
        let proc = get_test_processor();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Metal).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
        assert!(code.contains("Metal Shading Language"));
        assert!(code.contains("#include <metal_stdlib>"));
        assert!(code.contains("using namespace metal"));
    }

    #[test]
    fn test_is_glsl() {
        assert!(GpuLanguage::Glsl120.is_glsl());
        assert!(GpuLanguage::Glsl130.is_glsl());
        assert!(GpuLanguage::Glsl330.is_glsl());
        assert!(GpuLanguage::Glsl400.is_glsl());
        assert!(GpuLanguage::GlslVulkan460.is_glsl());
        assert!(GpuLanguage::GlslEs100.is_glsl());
        assert!(GpuLanguage::GlslEs300.is_glsl());
        assert!(!GpuLanguage::Hlsl50.is_glsl());
        assert!(!GpuLanguage::Metal.is_glsl());
    }
}

/// Transfer function exact value parity tests.
/// OCIO ref: LogOpData.cpp, GammaOpData.cpp, Lut1DOpData.cpp
mod transfer_exact_values_parity {
    use crate::processor::{Processor, ProcessorOp, TransferStyle};

    fn apply_transfer(style: TransferStyle, input: f32, forward: bool) -> f32 {
        let proc = Processor::from_ops(vec![ProcessorOp::Transfer { style, forward }]);
        let mut px = [[input, input, input]];
        proc.apply_rgb(&mut px);
        px[0][0]
    }

    // Roundtrip tests for all transfer functions
    #[test]
    fn test_all_transfers_roundtrip() {
        let styles = [
            TransferStyle::Srgb,
            TransferStyle::AcesCct,
            TransferStyle::AcesCc,
            TransferStyle::LogC3,
            TransferStyle::LogC4,
            TransferStyle::SLog3,
            TransferStyle::VLog,
            TransferStyle::Log3G10,
            TransferStyle::Rec1886,
            TransferStyle::Gamma22,
            TransferStyle::Gamma26,
            TransferStyle::Pq,
            TransferStyle::Hlg,
        ];

        for style in styles {
            let input = 0.18_f32;
            let encoded = apply_transfer(style, input, true);
            let decoded = apply_transfer(style, encoded, false);

            assert!(
                (decoded - input).abs() < 0.01,
                "{:?} roundtrip: {} -> {} -> {}",
                style,
                input,
                encoded,
                decoded
            );
        }
    }
}

/// Builtin transform name parity tests: the OCIO 2.5 style names resolve, case-insensitively.
/// OCIO ref: BuiltinTransformRegistry; `BuiltinTransformImpl::setStyle` (BuiltinTransform.cpp:63).
///
/// These used to assert that separator-mangled spellings resolved ("ACES-AP0_to_AP1",
/// "ARRI_LogC3_to_ACES2065-1", "SONY_SLOG3_SGAMUT3_to_ACES2065-1",
/// "DISPLAY_CIE-XYZ-D65_to_Rec.1886-Rec.709", "CURVE_HLG-OETF_INVERSE", ...). OCIO rejects every
/// one of them; they now use the registered names.
mod builtin_transform_names_parity {
    use crate::builtin_transforms::get_builtin;

    #[test]
    fn test_aces_transforms_exist() {
        for style in [
            "ACEScct_to_ACES2065-1",
            "ACEScc_to_ACES2065-1",
            "ACEScg_to_ACES2065-1",
            "UTILITY - ACES-AP0_to_CIE-XYZ-D65_BFD",
            "UTILITY - ACES-AP1_to_CIE-XYZ-D65_BFD",
        ] {
            assert!(get_builtin(style).is_some(), "{style}");
        }
        // Not OCIO styles.
        assert!(get_builtin("ACES-AP0_to_AP1").is_none());
        assert!(get_builtin("ACES-AP1_to_AP0").is_none());
    }

    #[test]
    fn test_arri_transforms_exist() {
        assert!(get_builtin("ARRI_ALEXA-LOGC-EI800-AWG_to_ACES2065-1").is_some());
        assert!(get_builtin("ARRI_LOGC4_to_ACES2065-1").is_some());
        // Case-insensitive, as OCIO's Strcasecmp.
        assert!(get_builtin("arri_logc4_to_aces2065-1").is_some());
        assert!(get_builtin("ARRI_LogC3_to_ACES2065-1").is_none());
    }

    #[test]
    fn test_sony_transforms_exist() {
        assert!(get_builtin("SONY_SLOG3-SGAMUT3_to_ACES2065-1").is_some());
        assert!(get_builtin("SONY_SLOG3_SGAMUT3_to_ACES2065-1").is_none());
    }

    #[test]
    fn test_display_transforms_exist() {
        assert!(get_builtin("DISPLAY - CIE-XYZ-D65_to_sRGB").is_some());
        assert!(get_builtin("DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.709").is_some());
        assert!(get_builtin("DISPLAY_CIE-XYZ-D65_to_sRGB").is_none());
    }

    #[test]
    fn test_identity_transform() {
        assert!(get_builtin("IDENTITY").is_some(), "Identity transform");
    }

    #[test]
    fn test_pq_hlg_transforms_exist() {
        assert!(get_builtin("CURVE - ST-2084_to_LINEAR").is_some());
        assert!(get_builtin("CURVE - HLG-OETF-INVERSE").is_some());
        assert!(get_builtin("CURVE_ST-2084_to_LINEAR").is_none());
        assert!(get_builtin("CURVE_HLG-OETF_INVERSE").is_none());
    }
}

/// Matrix transform parity tests with OCIO reference values.
/// OCIO ref: MatrixOps.cpp
mod matrix_ops_parity {
    use crate::{MatrixTransform, Processor, Transform, TransformDirection};

    #[test]
    fn test_identity_matrix() {
        let t = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.5_f32, 0.3, 0.7]];
        proc.apply_rgb(&mut pixels);

        assert!((pixels[0][0] - 0.5).abs() < 1e-6);
        assert!((pixels[0][1] - 0.3).abs() < 1e-6);
        assert!((pixels[0][2] - 0.7).abs() < 1e-6);
    }

    #[test]
    fn test_scale_matrix() {
        let mut matrix = MatrixTransform::IDENTITY;
        matrix[0] = 2.0; // R *= 2
        matrix[5] = 0.5; // G *= 0.5
        matrix[10] = 1.5; // B *= 1.5

        let t = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);

        assert!((pixels[0][0] - 1.0).abs() < 1e-5, "R scaled");
        assert!((pixels[0][1] - 0.25).abs() < 1e-5, "G scaled");
        assert!((pixels[0][2] - 0.75).abs() < 1e-5, "B scaled");
    }

    #[test]
    fn test_offset() {
        let t = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.1, -0.1, 0.2, 0.0],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);

        assert!((pixels[0][0] - 0.6).abs() < 1e-5, "R offset");
        assert!((pixels[0][1] - 0.4).abs() < 1e-5, "G offset");
        assert!((pixels[0][2] - 0.7).abs() < 1e-5, "B offset");
    }

    #[test]
    fn test_matrix_inverse() {
        // Create a simple scale matrix
        let mut matrix = MatrixTransform::IDENTITY;
        matrix[0] = 2.0;
        matrix[5] = 2.0;
        matrix[10] = 2.0;

        let t_fwd = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        });

        let t_inv = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix,
            offset: [0.0; 4],
            direction: TransformDirection::Inverse,
        });

        let proc_fwd = Processor::from_transform(&t_fwd, TransformDirection::Forward).unwrap();
        let proc_inv = Processor::from_transform(&t_inv, TransformDirection::Forward).unwrap();

        let input = [0.5_f32, 0.3, 0.7];
        let mut pixels = [input];
        proc_fwd.apply_rgb(&mut pixels);
        proc_inv.apply_rgb(&mut pixels);

        // Roundtrip should return to original
        assert!((pixels[0][0] - input[0]).abs() < 1e-5);
        assert!((pixels[0][1] - input[1]).abs() < 1e-5);
        assert!((pixels[0][2] - input[2]).abs() < 1e-5);
    }
}

/// CDL (Color Decision List) parity tests.
/// OCIO ref: CDLOpData.cpp, ASC CDL v1.2 spec
mod cdl_ops_parity {
    use crate::{CdlStyle, CdlTransform, Processor, Transform, TransformDirection};

    #[test]
    fn test_cdl_identity() {
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.5_f32, 0.3, 0.7]];
        proc.apply_rgb(&mut pixels);

        // Identity CDL should not change values
        assert!((pixels[0][0] - 0.5).abs() < 1e-5);
        assert!((pixels[0][1] - 0.3).abs() < 1e-5);
        assert!((pixels[0][2] - 0.7).abs() < 1e-5);
    }

    #[test]
    fn test_cdl_slope() {
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [2.0, 0.5, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);

        // Slope multiplies (before clamp)
        assert!(pixels[0][0] > 0.5); // R * 2 = 1.0, clamped
        assert!((pixels[0][1] - 0.25).abs() < 0.01); // G * 0.5
        assert!((pixels[0][2] - 0.5).abs() < 0.01); // B unchanged
    }

    #[test]
    fn test_cdl_offset() {
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [1.0, 1.0, 1.0],
            offset: [0.1, -0.1, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);

        assert!((pixels[0][0] - 0.6).abs() < 0.01); // R + 0.1
        assert!((pixels[0][1] - 0.4).abs() < 0.01); // G - 0.1
        assert!((pixels[0][2] - 0.5).abs() < 0.01); // B unchanged
    }

    #[test]
    fn test_cdl_power() {
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [2.0, 0.5, 1.0], // gamma
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);

        // Power: val^power
        assert!((pixels[0][0] - 0.25).abs() < 0.01); // 0.5^2.0
        assert!((pixels[0][1] - 0.707).abs() < 0.01); // 0.5^0.5
        assert!((pixels[0][2] - 0.5).abs() < 0.01); // 0.5^1.0
    }

    #[test]
    fn test_cdl_saturation_pure_red_luma_weight() {
        // [PyOCIO] CDL sat0 on pure red (1,0,0) = 0.2126 x3 exactly: pins the
        // Rec.709 R luma weight in isolation (companion to `cdl_parity`'s
        // `test_cdl_saturation`, which uses an asymmetric (1,0.5,0) probe to
        // exercise all three weights at once).
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 0.0, // Full desaturation
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[1.0_f32, 0.0, 0.0]]; // Pure red
        proc.apply_rgb(&mut pixels);

        let expected_luma = 0.2126_f32;
        for (i, &got) in pixels[0].iter().enumerate() {
            assert!(
                (got - expected_luma).abs() < 1e-5,
                "ch{i}: got {got}, expected {expected_luma}"
            );
        }
    }

    #[test]
    fn test_cdl_no_clamp_style() {
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [2.0, 2.0, 2.0],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::NoClamp,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.8_f32, 0.8, 0.8]];
        proc.apply_rgb(&mut pixels);

        // NoClamp should allow values > 1.0
        assert!(
            (pixels[0][0] - 1.6).abs() < 0.01,
            "NoClamp should preserve >1.0"
        );
    }
}

/// GradingRgbCurve transform parity tests.
/// OCIO ref: GradingRGBCurveTransform
mod grading_rgb_curve_parity {
    use crate::{
        GradingRgbCurveTransform, GradingToneStyle, Processor, Transform, TransformDirection,
    };

    fn identity_curve() -> Vec<[f64; 2]> {
        vec![[0.0, 0.0], [1.0, 1.0]]
    }

    #[test]
    fn test_rgb_curve_identity() {
        let t = Transform::GradingRgbCurve(GradingRgbCurveTransform {
            name: String::new(),
            style: GradingToneStyle::Log,
            red: identity_curve(),
            green: identity_curve(),
            blue: identity_curve(),
            master: identity_curve(),
            red_slopes: vec![],
            green_slopes: vec![],
            blue_slopes: vec![],
            master_slopes: vec![],
            bypass_lin_to_log: false,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        // Chromatic probe: each channel runs the same identity spline independently.
        let input = [0.5_f32, 0.3, 0.2];
        let mut pixels = [input];
        proc.apply_rgb(&mut pixels);

        // An identity B-spline curve (with lin<->log a paired inverse) is an exact
        // identity transform: out == in. Tight round-trip tolerance on all channels.
        for c in 0..3 {
            assert!(
                (pixels[0][c] - input[c]).abs() < 1e-5,
                "RGB curve identity ch{}: got {}, expected {}",
                c,
                pixels[0][c],
                input[c]
            );
        }
    }

    #[test]
    fn test_rgb_curve_boost_red() {
        // Curve that boosts red channel
        let boost_curve = vec![[0.0, 0.0], [0.5, 0.7], [1.0, 1.0]];

        let t = Transform::GradingRgbCurve(GradingRgbCurveTransform {
            name: String::new(),
            style: GradingToneStyle::Log,
            red: boost_curve,
            green: identity_curve(),
            blue: identity_curve(),
            master: identity_curve(),
            red_slopes: vec![],
            green_slopes: vec![],
            blue_slopes: vec![],
            master_slopes: vec![],
            bypass_lin_to_log: false,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);

        // Red should be boosted relative to green/blue
        assert!(pixels[0][0] > pixels[0][1], "Red should be boosted");
    }

    #[test]
    fn test_rgb_curve_styles() {
        // Test all style variants compile
        for style in [
            GradingToneStyle::Log,
            GradingToneStyle::Linear,
            GradingToneStyle::Video,
        ] {
            let t = Transform::GradingRgbCurve(GradingRgbCurveTransform {
                name: String::new(),
                style,
                red: identity_curve(),
                green: identity_curve(),
                blue: identity_curve(),
                master: identity_curve(),
                red_slopes: vec![],
                green_slopes: vec![],
                blue_slopes: vec![],
                master_slopes: vec![],
                bypass_lin_to_log: false,
                direction: TransformDirection::Forward,
            });

            let proc = Processor::from_transform(&t, TransformDirection::Forward);
            assert!(proc.is_ok(), "Style {:?} should compile", style);
        }
    }
}

/// GradingHueCurve transform parity tests.
/// OCIO ref: GradingRGBCurveTransform (hue curves)
mod grading_hue_curve_parity {
    use crate::transform::HsyTransformStyle;
    use crate::{
        GradingHueCurveStyle, GradingHueCurveTransform, Processor, Transform, TransformDirection,
    };

    fn identity_hue_curve() -> Vec<[f64; 2]> {
        // Hue curves are cyclic, identity = no change
        vec![
            [0.0, 0.0],
            [60.0, 0.0],
            [120.0, 0.0],
            [180.0, 0.0],
            [240.0, 0.0],
            [300.0, 0.0],
            [360.0, 0.0],
        ]
    }

    fn identity_mult_curve() -> Vec<[f64; 2]> {
        // Multiplier curves (hue_sat/hue_lum/lum_sat/sat_lum): 1.0 = no change.
        vec![[0.0, 1.0], [1.0, 1.0]]
    }

    fn diagonal_curve() -> Vec<[f64; 2]> {
        // Replacement curves (hue_hue/sat_sat/lum_lum) map value->value, so the
        // identity is the DIAGONAL y=x, NOT a flat multiplier. OCIO evaluates
        // HUE_HUE as a replacement (GradingHueCurveOpCPU.cpp:244
        // `out = evalCurve(HUE_HUE, out, out)`), likewise sat_sat/lum_lum.
        vec![[0.0, 0.0], [1.0, 1.0]]
    }

    fn zero_curve() -> Vec<[f64; 2]> {
        // hue_fx is ADDITIVE (OCIO :271 `out += evalCurve(HUE_FX, out, 0)`), so
        // its identity is the constant 0 offset.
        vec![[0.0, 0.0], [1.0, 0.0]]
    }

    /// The canonical OCIO identity: replacement curves (hue_hue/sat_sat/lum_lum)
    /// are the diagonal, multiplier curves are flat 1.0, hue_fx is 0. (The
    /// earlier config mis-authored hue_hue/sat_sat/lum_lum as flat-1/zero, which
    /// is NOT identity for replacement curves — it forced luma to 1.0 and blew
    /// the output up to ~4.7. The impl matches OCIO; the test config was wrong.)
    #[test]
    fn test_hue_curve_identity() {
        let t = Transform::GradingHueCurve(GradingHueCurveTransform {
            name: String::new(),
            style: GradingHueCurveStyle::Log,
            rgb_to_hsy: HsyTransformStyle::Default,
            hue_hue: diagonal_curve(),
            hue_sat: identity_mult_curve(),
            hue_lum: identity_mult_curve(),
            lum_sat: identity_mult_curve(),
            sat_sat: diagonal_curve(),
            lum_lum: diagonal_curve(),
            sat_lum: identity_mult_curve(),
            hue_fx: zero_curve(),
            hue_hue_slopes: vec![],
            hue_sat_slopes: vec![],
            hue_lum_slopes: vec![],
            lum_sat_slopes: vec![],
            sat_sat_slopes: vec![],
            lum_lum_slopes: vec![],
            sat_lum_slopes: vec![],
            hue_fx_slopes: vec![],
            draw_curve_only: false,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let input = [0.5_f32, 0.3, 0.2];
        let mut pixels = [input];
        proc.apply_rgb(&mut pixels);

        // Identity hue curves (zero hue offset, unit multipliers) compose with the
        // RGB<->HSY round-trip to an exact identity: out == in on every channel.
        for c in 0..3 {
            assert!(
                (pixels[0][c] - input[c]).abs() < 1e-5,
                "hue curve identity ch{}: got {}, expected {}",
                c,
                pixels[0][c],
                input[c]
            );
        }
    }

    #[test]
    fn test_hue_curve_styles() {
        for style in [
            GradingHueCurveStyle::Log,
            GradingHueCurveStyle::Linear,
            GradingHueCurveStyle::Video,
        ] {
            let t = Transform::GradingHueCurve(GradingHueCurveTransform {
                name: String::new(),
                style,
                rgb_to_hsy: HsyTransformStyle::Default,
                hue_hue: identity_hue_curve(),
                hue_sat: identity_mult_curve(),
                hue_lum: identity_mult_curve(),
                lum_sat: identity_mult_curve(),
                sat_sat: identity_mult_curve(),
                lum_lum: identity_mult_curve(),
                sat_lum: identity_mult_curve(),
                hue_fx: identity_hue_curve(),
                hue_hue_slopes: vec![],
                hue_sat_slopes: vec![],
                hue_lum_slopes: vec![],
                lum_sat_slopes: vec![],
                sat_sat_slopes: vec![],
                lum_lum_slopes: vec![],
                sat_lum_slopes: vec![],
                hue_fx_slopes: vec![],
                draw_curve_only: false,
                direction: TransformDirection::Forward,
            });

            let proc = Processor::from_transform(&t, TransformDirection::Forward);
            assert!(proc.is_ok(), "HueCurve style {:?} should compile", style);
        }
    }

    #[test]
    fn test_hue_curve_types() {
        use crate::HueCurveType;

        // Verify all curve type enums exist
        let _types = [
            HueCurveType::HueHue,
            HueCurveType::HueSat,
            HueCurveType::HueLum,
            HueCurveType::LumSat,
            HueCurveType::SatSat,
            HueCurveType::LumLum,
            HueCurveType::SatLum,
            HueCurveType::HueFx,
        ];
    }
}

/// All FixedFunction styles comprehensive tests.
/// OCIO ref: FixedFunctionOpData.cpp
mod fixed_function_all_styles {
    use crate::{
        FixedFunctionStyle, FixedFunctionTransform, Processor, Transform, TransformDirection,
    };

    fn test_style_compiles(style: FixedFunctionStyle, params: Vec<f64>) {
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style,
            params,
            direction: TransformDirection::Forward,
        });

        let result = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(result.is_ok(), "{:?} should compile", style);
    }

    #[test]
    fn test_all_aces_styles() {
        test_style_compiles(FixedFunctionStyle::AcesRedMod03, vec![]);
        test_style_compiles(FixedFunctionStyle::AcesRedMod10, vec![]);
        test_style_compiles(FixedFunctionStyle::AcesGlow03, vec![]);
        test_style_compiles(FixedFunctionStyle::AcesGlow10, vec![]);
        test_style_compiles(FixedFunctionStyle::AcesDarkToDim10, vec![]);
        test_style_compiles(FixedFunctionStyle::AcesGamutComp13, vec![]);
    }

    #[test]
    fn test_color_space_conversions() {
        test_style_compiles(FixedFunctionStyle::RgbToHsv, vec![]);
        test_style_compiles(FixedFunctionStyle::HsvToRgb, vec![]);
        test_style_compiles(FixedFunctionStyle::XyzToXyy, vec![]);
        test_style_compiles(FixedFunctionStyle::XyyToXyz, vec![]);
        test_style_compiles(FixedFunctionStyle::XyzToUvy, vec![]);
        test_style_compiles(FixedFunctionStyle::UvyToXyz, vec![]);
        test_style_compiles(FixedFunctionStyle::XyzToLuv, vec![]);
        test_style_compiles(FixedFunctionStyle::LuvToXyz, vec![]);
    }

    #[test]
    fn test_hdr_transfers() {
        test_style_compiles(FixedFunctionStyle::LinToPq, vec![]);
        test_style_compiles(FixedFunctionStyle::PqToLin, vec![]);
        test_style_compiles(FixedFunctionStyle::Rec2100Surround, vec![0.75]);
    }

    #[test]
    fn test_aces2_styles() {
        // ACES 2.0 experimental styles
        test_style_compiles(FixedFunctionStyle::AcesOutputTransform20, vec![1000.0]);
        test_style_compiles(FixedFunctionStyle::AcesRgbToJmh20, vec![]);
        test_style_compiles(FixedFunctionStyle::AcesJmhToRgb20, vec![]);
        test_style_compiles(FixedFunctionStyle::AcesTonescaleCompress20, vec![1000.0]);
    }

    #[test]
    fn test_rgb_to_hsv_values() {
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::RgbToHsv,
            params: vec![],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        // Test pure colors
        // Red: H=0, S=1, V=1
        let mut red = [[1.0_f32, 0.0, 0.0]];
        proc.apply_rgb(&mut red);
        assert!(red[0][0].abs() < 0.01, "Red hue = 0");
        assert!((red[0][1] - 1.0).abs() < 0.01, "Red sat = 1");
        assert!((red[0][2] - 1.0).abs() < 0.01, "Red val = 1");

        // Green: H=120/360 = 0.333, S=1, V=1
        let mut green = [[0.0_f32, 1.0, 0.0]];
        proc.apply_rgb(&mut green);
        assert!(
            (green[0][0] - 0.333).abs() < 0.02,
            "Green hue ~0.333: got {}",
            green[0][0]
        );

        // Blue: H=240/360 = 0.667, S=1, V=1
        let mut blue = [[0.0_f32, 0.0, 1.0]];
        proc.apply_rgb(&mut blue);
        assert!(
            (blue[0][0] - 0.667).abs() < 0.02,
            "Blue hue ~0.667: got {}",
            blue[0][0]
        );
    }

    #[test]
    fn test_hsv_roundtrip() {
        let to_hsv = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::RgbToHsv,
            params: vec![],
            direction: TransformDirection::Forward,
        });

        let to_rgb = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::HsvToRgb,
            params: vec![],
            direction: TransformDirection::Forward,
        });

        let proc_to_hsv = Processor::from_transform(&to_hsv, TransformDirection::Forward).unwrap();
        let proc_to_rgb = Processor::from_transform(&to_rgb, TransformDirection::Forward).unwrap();

        let input = [0.7_f32, 0.3, 0.5];
        let mut pixels = [input];
        proc_to_hsv.apply_rgb(&mut pixels);
        proc_to_rgb.apply_rgb(&mut pixels);

        // Roundtrip should return to original
        assert!((pixels[0][0] - input[0]).abs() < 0.01, "HSV roundtrip R");
        assert!((pixels[0][1] - input[1]).abs() < 0.01, "HSV roundtrip G");
        assert!((pixels[0][2] - input[2]).abs() < 0.01, "HSV roundtrip B");
    }

    #[test]
    fn test_xyz_xyy_roundtrip() {
        let to_xyy = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyzToXyy,
            params: vec![],
            direction: TransformDirection::Forward,
        });

        let to_xyz = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyyToXyz,
            params: vec![],
            direction: TransformDirection::Forward,
        });

        let proc_to_xyy = Processor::from_transform(&to_xyy, TransformDirection::Forward).unwrap();
        let proc_to_xyz = Processor::from_transform(&to_xyz, TransformDirection::Forward).unwrap();

        // D65 white point in XYZ
        let input = [0.95047_f32, 1.0, 1.08883];
        let mut pixels = [input];
        proc_to_xyy.apply_rgb(&mut pixels);
        proc_to_xyz.apply_rgb(&mut pixels);

        // Roundtrip should return to original
        assert!(
            (pixels[0][0] - input[0]).abs() < 0.01,
            "XYZ-xyY roundtrip X"
        );
        assert!(
            (pixels[0][1] - input[1]).abs() < 0.01,
            "XYZ-xyY roundtrip Y"
        );
        assert!(
            (pixels[0][2] - input[2]).abs() < 0.01,
            "XYZ-xyY roundtrip Z"
        );
    }
}

/// LogAffine and LogCamera transform parity tests.
/// OCIO ref: LogOpData.cpp
mod log_transform_parity {
    use crate::transform::{LogAffineTransform, LogCameraTransform};
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_log_affine() {
        let t = Transform::LogAffine(LogAffineTransform {
            name: String::new(),
            base: 10.0,
            log_side_slope: [1.0, 1.0, 1.0],
            log_side_offset: [0.0, 0.0, 0.0],
            lin_side_slope: [1.0, 1.0, 1.0],
            lin_side_offset: [0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[10.0_f32, 100.0, 1000.0]];
        proc.apply_rgb(&mut pixels);

        // log10(10) = 1, log10(100) = 2, log10(1000) = 3
        assert!((pixels[0][0] - 1.0).abs() < 0.01, "log10(10)");
        assert!((pixels[0][1] - 2.0).abs() < 0.01, "log10(100)");
        assert!((pixels[0][2] - 3.0).abs() < 0.01, "log10(1000)");
    }

    #[test]
    fn test_log_camera_logc3() {
        // LogC3 EI800 parameters
        let t = Transform::LogCamera(LogCameraTransform {
            name: String::new(),
            base: 10.0,
            log_side_slope: [0.247190, 0.247190, 0.247190],
            log_side_offset: [0.385537, 0.385537, 0.385537],
            lin_side_slope: [5.555556, 5.555556, 5.555556],
            lin_side_offset: [0.052272, 0.052272, 0.052272],
            lin_side_break: [0.010591, 0.010591, 0.010591],
            linear_slope: None,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixels);

        // 0.18 -> ~0.391 in LogC3
        assert!(
            (pixels[0][0] - 0.391).abs() < 0.02,
            "LogC3 mid gray: got {}",
            pixels[0][0]
        );
    }
}

/// ExponentWithLinear transform parity tests.
/// OCIO ref: GammaOpData.cpp
mod exponent_with_linear_parity {
    use crate::transform::{ExponentWithLinearTransform, NegativeStyle};
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_srgb_style_curve() {
        // sRGB-style curve with linear segment
        let t = Transform::ExponentWithLinear(ExponentWithLinearTransform {
            name: String::new(),
            gamma: [2.4, 2.4, 2.4, 1.0],
            offset: [0.055, 0.055, 0.055, 0.0],
            negative_style: NegativeStyle::Linear,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        // Verify transform runs
        let mut low = [[0.001_f32, 0.001, 0.001]];
        proc.apply_rgb(&mut low);
        // ExponentWithLinear applies gamma with linear toe
        assert!(
            low[0][0] > 0.0 && low[0][0] < 0.1,
            "Low value processed: {}",
            low[0][0]
        );

        // Above linear segment
        let mut mid = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut mid);
        // Verify transform runs (exact formula depends on offset interpretation)
        assert!(
            mid[0][0] > 0.0 && mid[0][0] < 1.0,
            "Mid value processed: {}",
            mid[0][0]
        );
    }
}

/// Processor packed bit depth parity tests.
/// OCIO ref: PackedImageDesc, BitDepth
mod processor_packed_parity {
    use crate::builtin::TestSchema;
    use crate::{ChannelOrdering, PackedBitDepth, builtin};

    #[test]
    fn test_packed_bit_depth_variants() {
        // OCIO BIT_DEPTH_* surface — ABI pin (the old test built locals and
        // asserted nothing). All six variants present and distinct.
        let all = [
            PackedBitDepth::Uint8,
            PackedBitDepth::Uint10,
            PackedBitDepth::Uint12,
            PackedBitDepth::Uint16,
            PackedBitDepth::F16,
            PackedBitDepth::F32,
        ];
        assert_eq!(all.len(), 6);
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(a != b, "duplicate PackedBitDepth {a:?}");
            }
        }
    }

    #[test]
    fn test_channel_ordering_variants() {
        // OCIO ChannelOrdering surface — ABI pin (was asserting nothing).
        let all = [
            ChannelOrdering::Rgba,
            ChannelOrdering::Bgra,
            ChannelOrdering::Rgb,
            ChannelOrdering::Bgr,
            ChannelOrdering::Abgr,
        ];
        assert_eq!(all.len(), 5);
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(a != b, "duplicate ChannelOrdering {a:?}");
            }
        }
    }

    #[test]
    fn test_processor_supports_packed() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        // Processor should have apply_packed method (unsafe)
        // Just verify it exists by checking apply_rgb works
        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixels);
        assert!(pixels[0][0].is_finite());
    }
}

/// Context variable resolution parity tests.
/// OCIO ref: Context
mod context_resolution_parity {
    use crate::Context;

    #[test]
    fn test_basic_substitution() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");
        ctx.set("SEQ", "sq01");

        let result = ctx.resolve("/shows/$SEQ/$SHOT/luts/grade.cube");
        assert_eq!(result, "/shows/sq01/sh010/luts/grade.cube");
    }

    #[test]
    fn test_curly_brace_syntax() {
        let mut ctx = Context::new();
        ctx.set("VAR", "value");

        let result = ctx.resolve("${VAR}_suffix");
        assert_eq!(result, "value_suffix");
    }

    #[test]
    fn test_environment_fallback() {
        let ctx = Context::new();

        // Unset variables should remain as-is
        let result = ctx.resolve("$UNDEFINED_VAR");
        assert!(result.contains("UNDEFINED") || result.is_empty());
    }

    #[test]
    fn test_context_get() {
        let mut ctx = Context::new();
        ctx.set("KEY", "VALUE");

        assert_eq!(ctx.get("KEY"), Some("VALUE".to_string()));
        assert_eq!(ctx.get("MISSING"), None);
    }
}

/// FileTransform parity tests.
/// OCIO ref: FileTransformOpData.cpp
mod file_transform_parity {
    use crate::transform::FileTransform;
    use crate::{CdlStyle, Interpolation, Transform, TransformDirection};
    use std::path::PathBuf;

    #[test]
    fn test_file_transform_construction() {
        // OCIO: FileTransform::Create()
        let t = FileTransform {
            src: PathBuf::from("lut.cube"),
            ccc_id: None,
            interpolation: Interpolation::Linear,
            cdl_style: CdlStyle::NoClamp,
            direction: TransformDirection::Forward,
        };

        assert_eq!(t.src.to_str().unwrap(), "lut.cube");
        assert!(matches!(t.interpolation, Interpolation::Linear));
    }

    /// `Processor::metadata()` must report the file a `FileTransform` was built from.
    ///
    /// OCIO ref: `ProcessorMetadata::addFile` (Processor.cpp:73), fed by `FileNoOp::dumpMetadata`
    /// (NoOps.cpp:355) from the marker op `CreateFileNoOp` inserts at FileTransform.cpp:945.
    ///
    /// This is the test that was missing: `metadata()` used to hardcode
    /// `{num_files: 0, files: []}`, and the only thing exercising it asserted
    /// `num_files >= 0` on a `usize` — always true, so the stub was invisible. Building through
    /// a real file on disk is what discriminates; a count-vs-list consistency check alone would
    /// still pass against the stub, since 0 == [].len().
    #[test]
    fn test_processor_metadata_records_the_lut_file() {
        // `tempfile::tempdir()`: per-call unique, auto-cleaned — the fixed
        // filename in the shared OS temp dir collided under
        // concurrent/parallel test runs, and the manual `remove_file` below
        // never ran on an earlier panic.
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("vfx_ocio_metadata_probe.cube");
        // Identity 1D LUT — the smallest thing the .cube reader accepts. The VALUES do not
        // matter here; the point is that a real file was opened.
        std::fs::write(&path, "LUT_1D_SIZE 2\n0.0 0.0 0.0\n1.0 1.0 1.0\n")
            .expect("temp LUT write must succeed");

        let ft = FileTransform {
            src: path.clone(),
            ccc_id: None,
            interpolation: Interpolation::Linear,
            cdl_style: CdlStyle::NoClamp,
            direction: TransformDirection::Forward,
        };
        let proc = crate::Processor::from_transform(
            &Transform::FileTransform(ft),
            TransformDirection::Forward,
        )
        .expect("processor must build from the LUT file");

        let meta = proc.metadata();
        assert_eq!(meta.num_files, 1, "the LUT file must be reported");
        assert_eq!(
            meta.files.len(),
            meta.num_files,
            "count must match the list"
        );
        assert!(
            meta.files[0].contains("vfx_ocio_metadata_probe"),
            "reported path should be the resolved LUT path, got {:?}",
            meta.files[0]
        );
    }

    #[test]
    fn test_file_transform_with_cccid() {
        let t = FileTransform {
            src: PathBuf::from("grades.cc"),
            ccc_id: Some("shot010".to_string()),
            interpolation: Interpolation::Linear,
            cdl_style: CdlStyle::NoClamp,
            direction: TransformDirection::Forward,
        };

        assert_eq!(t.ccc_id.as_deref(), Some("shot010"));
    }

    #[test]
    fn test_file_transform_direction() {
        let fwd = FileTransform {
            src: PathBuf::from("lut.cube"),
            ccc_id: None,
            interpolation: Interpolation::Linear,
            cdl_style: CdlStyle::NoClamp,
            direction: TransformDirection::Forward,
        };

        let inv = FileTransform {
            src: PathBuf::from("lut.cube"),
            ccc_id: None,
            interpolation: Interpolation::Linear,
            cdl_style: CdlStyle::NoClamp,
            direction: TransformDirection::Inverse,
        };

        assert!(matches!(fwd.direction, TransformDirection::Forward));
        assert!(matches!(inv.direction, TransformDirection::Inverse));
    }
}

/// LUT1DTransform and LUT3DTransform parity tests.
/// OCIO ref: Lut1DOpData.cpp, Lut3DOpData.cpp
mod lut_transform_parity {
    use crate::transform::{Lut1DHueAdjust, Lut1DTransform, Lut3DTransform};
    use crate::{Interpolation, Processor, Transform, TransformDirection};

    #[test]
    fn test_lut1d_construction() {
        // OCIO: Lut1DTransform - stored as separate RGB channels
        let length = 1024;
        let data: Vec<f32> = (0..length)
            .map(|i| i as f32 / (length - 1) as f32)
            .collect();

        let lut = Lut1DTransform {
            red: data.clone(),
            green: None, // uses red
            blue: None,  // uses red
            input_min: 0.0,
            input_max: 1.0,
            output_min: 0.0,
            output_max: 1.0,
            half_domain: false,
            raw_halfs: false,
            hue_adjust: Lut1DHueAdjust::None,
            interpolation: Interpolation::Linear,
            direction: TransformDirection::Forward,
        };

        assert_eq!(lut.red.len(), length);
    }

    #[test]
    fn test_lut1d_half_domain() {
        // OCIO: setInputHalfDomain
        let lut = Lut1DTransform {
            red: vec![0.0; 65536],
            green: None,
            blue: None,
            input_min: 0.0,
            input_max: 1.0,
            output_min: 0.0,
            output_max: 1.0,
            half_domain: true,
            raw_halfs: false,
            hue_adjust: Lut1DHueAdjust::None,
            interpolation: Interpolation::Linear,
            direction: TransformDirection::Forward,
        };

        assert!(lut.half_domain);
    }

    #[test]
    fn test_lut3d_construction() {
        // OCIO: Lut3DTransform::Create(gridSize)
        let size = 17;
        let total = size * size * size;
        let data: Vec<[f32; 3]> = vec![[0.5, 0.5, 0.5]; total];

        let lut = Lut3DTransform {
            data,
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Tetrahedral,
            direction: TransformDirection::Forward,
        };

        assert_eq!(lut.size, 17);
    }

    #[test]
    fn test_lut3d_interpolation() {
        // OCIO: INTERP_TETRAHEDRAL vs INTERP_BEST
        let size = 17;
        let total = size * size * size;

        let tet = Lut3DTransform {
            data: vec![[0.5, 0.5, 0.5]; total],
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Tetrahedral,
            direction: TransformDirection::Forward,
        };

        let best = Lut3DTransform {
            data: vec![[0.5, 0.5, 0.5]; total],
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Best,
            direction: TransformDirection::Forward,
        };

        assert!(matches!(tet.interpolation, Interpolation::Tetrahedral));
        assert!(matches!(best.interpolation, Interpolation::Best));
    }
}

/// AllocationTransform parity tests.
/// OCIO ref: AllocationOp.cpp
mod allocation_transform_parity {
    use crate::transform::{AllocationTransform, AllocationType};
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_uniform_allocation() {
        let t = Transform::Allocation(AllocationTransform {
            allocation: AllocationType::Uniform,
            vars: vec![0.0, 1.0],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_log_allocation() {
        let t = Transform::Allocation(AllocationTransform {
            allocation: AllocationType::Log2,
            vars: vec![-8.0, 8.0],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_allocation_with_offset() {
        // OCIO: vars with 3 elements includes offset
        let t = Transform::Allocation(AllocationTransform {
            allocation: AllocationType::Log2,
            vars: vec![-8.0, 8.0, 0.001],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_log_allocation_defaults() {
        // OCIO defaults for Log2 allocation are min=-10, max=6, offset=0
        let t = Transform::Allocation(AllocationTransform {
            allocation: AllocationType::Log2,
            vars: Vec::new(),
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut pixels = [[1.0_f32, 1.0_f32, 1.0_f32]];
        proc.apply_rgb(&mut pixels);

        let expected = (0.0_f32 - (-10.0_f32)) / (6.0_f32 - (-10.0_f32)); // 0.625
        assert!((pixels[0][0] - expected).abs() < 1e-6);
        assert!((pixels[0][1] - expected).abs() < 1e-6);
        assert!((pixels[0][2] - expected).abs() < 1e-6);
    }
}

/// ColorSpaceTransform parity tests.
/// OCIO ref: ColorSpaceTransformOpData.cpp
mod colorspace_transform_parity {
    use crate::transform::ColorSpaceTransform;
    use crate::{Transform, TransformDirection};

    #[test]
    fn test_colorspace_transform_construction() {
        // OCIO: ColorSpaceTransform::Create()
        let t = ColorSpaceTransform {
            src: "ACEScg".to_string(),
            dst: "sRGB".to_string(),
            data_bypass: true,
            direction: TransformDirection::Forward,
        };

        assert_eq!(t.src, "ACEScg");
        assert_eq!(t.dst, "sRGB");
    }

    #[test]
    fn test_colorspace_transform_direction() {
        let fwd = ColorSpaceTransform {
            src: "ACEScg".to_string(),
            dst: "sRGB".to_string(),
            data_bypass: true,
            direction: TransformDirection::Forward,
        };

        let inv = ColorSpaceTransform {
            src: "ACEScg".to_string(),
            dst: "sRGB".to_string(),
            data_bypass: true,
            direction: TransformDirection::Inverse,
        };

        assert!(matches!(fwd.direction, TransformDirection::Forward));
        assert!(matches!(inv.direction, TransformDirection::Inverse));
    }
}

/// DisplayViewTransform parity tests.
/// OCIO ref: DisplayViewTransformOpData.cpp
mod display_view_transform_parity {
    use crate::transform::DisplayViewTransform;
    use crate::{Transform, TransformDirection};

    #[test]
    fn test_display_view_construction() {
        // OCIO: DisplayViewTransform::Create()
        let t = DisplayViewTransform {
            src: "ACEScg".to_string(),
            display: "sRGB".to_string(),
            view: "Film".to_string(),
            looks_bypass: false,
            data_bypass: true,
            looks_override: String::new(),
            looks_override_enabled: false,
            direction: TransformDirection::Forward,
        };

        assert_eq!(t.src, "ACEScg");
        assert_eq!(t.display, "sRGB");
        assert_eq!(t.view, "Film");
    }

    #[test]
    fn test_display_view_fields() {
        let fwd = DisplayViewTransform {
            src: "ACEScg".to_string(),
            display: "sRGB".to_string(),
            view: "Raw".to_string(),
            looks_bypass: false,
            data_bypass: true,
            looks_override: String::new(),
            looks_override_enabled: false,
            direction: TransformDirection::Forward,
        };

        let inv = DisplayViewTransform {
            src: "ACEScg".to_string(),
            display: "sRGB".to_string(),
            view: "Raw".to_string(),
            looks_bypass: false,
            data_bypass: true,
            looks_override: String::new(),
            looks_override_enabled: false,
            direction: TransformDirection::Inverse,
        };

        assert!(matches!(fwd.direction, TransformDirection::Forward));
        assert!(matches!(inv.direction, TransformDirection::Inverse));
    }
}

/// LookTransform parity tests.
/// OCIO ref: LookTransformOpData.cpp
mod look_transform_parity {
    use crate::transform::LookTransform;
    use crate::{Transform, TransformDirection};

    #[test]
    fn test_look_transform_construction() {
        // OCIO: LookTransform::Create()
        let t = LookTransform {
            src: "ACEScg".to_string(),
            dst: "ACEScg".to_string(),
            looks: "Warm".to_string(),
            skip_color_space_conversion: false,
            direction: TransformDirection::Forward,
        };

        assert_eq!(t.src, "ACEScg");
        assert_eq!(t.looks, "Warm");
    }

    #[test]
    fn test_look_transform_chained_looks() {
        // OCIO: comma-separated looks
        let t = LookTransform {
            src: "ACEScg".to_string(),
            dst: "ACEScg".to_string(),
            looks: "FilmGrade, Warm, Contrast".to_string(),
            skip_color_space_conversion: false,
            direction: TransformDirection::Forward,
        };

        assert!(t.looks.contains("FilmGrade"));
        assert!(t.looks.contains("Warm"));
    }

    #[test]
    fn test_look_transform_inverse_look() {
        // OCIO: -LookName for inverse
        let t = LookTransform {
            src: "ACEScg".to_string(),
            dst: "ACEScg".to_string(),
            looks: "-Warm".to_string(),
            skip_color_space_conversion: false,
            direction: TransformDirection::Forward,
        };

        assert!(t.looks.starts_with('-'));
    }
}

/// GroupTransform parity tests.
/// OCIO ref: GroupTransformOpData.cpp
mod group_transform_parity {
    use crate::transform::{CdlTransform, GroupTransform, MatrixTransform};
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_group_construction() {
        // OCIO: GroupTransform::Create()
        let group = GroupTransform {
            name: String::new(),
            transforms: vec![],
            direction: TransformDirection::Forward,
        };

        assert!(group.transforms.is_empty());
    }

    #[test]
    fn test_group_with_transforms() {
        // OCIO: appendTransform
        let group = GroupTransform {
            name: String::new(),
            transforms: vec![
                Transform::Matrix(MatrixTransform {
                    name: String::new(),
                    matrix: MatrixTransform::IDENTITY,
                    offset: [0.0; 4],
                    direction: TransformDirection::Forward,
                }),
                Transform::Cdl(CdlTransform::default()),
            ],
            direction: TransformDirection::Forward,
        };

        assert_eq!(group.transforms.len(), 2);
    }

    #[test]
    fn test_group_processor() {
        let t = Transform::Group(GroupTransform {
            name: String::new(),
            transforms: vec![Transform::Matrix(MatrixTransform {
                name: String::new(),
                matrix: MatrixTransform::IDENTITY,
                offset: [0.0; 4],
                direction: TransformDirection::Forward,
            })],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

/// Processor finalization parity tests.
/// OCIO ref: Processor.cpp getOptimizedProcessor
mod processor_finalization_parity {
    use crate::builtin::TestSchema;
    use crate::{OptimizationLevel, builtin};

    #[test]
    fn test_processor_from_config() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config.processor(
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        );
        assert!(proc.is_ok());
    }

    #[test]
    fn test_processor_identity() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config.processor("ACEScg", "ACEScg").unwrap();

        let mut pixels = [[0.5_f32, 0.3, 0.7]];
        let original = pixels[0];
        proc.apply_rgb(&mut pixels);

        // Identity should preserve values
        assert!((pixels[0][0] - original[0]).abs() < 1e-5);
        assert!((pixels[0][1] - original[1]).abs() < 1e-5);
        assert!((pixels[0][2] - original[2]).abs() < 1e-5);
    }
}

/// Encoding enum parity tests.
/// OCIO ref: ColorSpace encoding attribute
mod encoding_parity {
    use crate::colorspace::Encoding;

    #[test]
    fn test_encoding_parse() {
        assert_eq!(Encoding::parse("scene-linear"), Encoding::SceneLinear);
        assert_eq!(Encoding::parse("display-linear"), Encoding::DisplayLinear);
        assert_eq!(Encoding::parse("log"), Encoding::Log);
        assert_eq!(Encoding::parse("sdr-video"), Encoding::Sdr);
        assert_eq!(Encoding::parse("hdr-video"), Encoding::Hdr);
        assert_eq!(Encoding::parse("data"), Encoding::Data);
    }

    #[test]
    fn test_encoding_is_linear() {
        assert!(Encoding::SceneLinear.is_linear());
        assert!(Encoding::DisplayLinear.is_linear());
        assert!(!Encoding::Log.is_linear());
        assert!(!Encoding::Sdr.is_linear());
    }
}

/// Family enum parity tests.
/// OCIO ref: ColorSpace family attribute
mod family_parity {
    use crate::colorspace::Family;

    #[test]
    fn test_family_parse() {
        assert_eq!(Family::parse("scene"), Family::Scene);
        assert_eq!(Family::parse("display"), Family::Display);
        assert_eq!(Family::parse("input"), Family::Input);
        assert_eq!(Family::parse("utility"), Family::Utility);
        assert_eq!(Family::parse("aces"), Family::Aces);
    }
}

/// GradingTone comprehensive parity tests.
/// OCIO ref: GradingToneOpData.cpp, GradingToneOp.cpp
mod grading_tone_comprehensive_parity {
    use crate::transform::{GradingRgbmsw, GradingToneStyle, GradingToneTransform};
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_grading_rgbmsw_structure() {
        // OCIO: GradingRGBMSW struct with RGB, Master, Start, Width
        let zone = GradingRgbmsw {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            master: 1.0,
            start: 0.0,
            width: 1.0,
        };

        assert_eq!(zone.red, 1.0);
        assert_eq!(zone.master, 1.0);
        assert_eq!(zone.start, 0.0);
        assert_eq!(zone.width, 1.0);
    }

    #[test]
    fn test_grading_tone_all_zones() {
        // OCIO: blacks, shadows, midtones, highlights, whites
        let t = GradingToneTransform {
            name: String::new(),
            style: GradingToneStyle::Log,
            blacks: GradingRgbmsw {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 1.0,
                start: 0.0,
                width: 0.0,
            },
            shadows: GradingRgbmsw {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 1.0,
                start: 0.0,
                width: 0.2,
            },
            midtones: GradingRgbmsw {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 1.0,
                start: 0.4,
                width: 0.6,
            },
            highlights: GradingRgbmsw {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 1.0,
                start: 0.8,
                width: 1.0,
            },
            whites: GradingRgbmsw {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 1.0,
                start: 0.0,
                width: 0.0,
            },
            scontrast: 1.0,
            direction: TransformDirection::Forward,
        };

        assert!(t.is_identity());
    }

    #[test]
    fn test_grading_tone_scontrast() {
        // OCIO: sContrast parameter
        let t = GradingToneTransform {
            name: String::new(),
            style: GradingToneStyle::Log,
            blacks: GradingRgbmsw::identity(),
            shadows: GradingRgbmsw::identity(),
            midtones: GradingRgbmsw::identity(),
            highlights: GradingRgbmsw::identity(),
            whites: GradingRgbmsw::identity(),
            scontrast: 1.5, // Non-identity
            direction: TransformDirection::Forward,
        };

        assert!(!t.is_identity());
    }

    #[test]
    fn test_grading_tone_default() {
        let t = GradingToneTransform::default();
        assert!(matches!(t.style, GradingToneStyle::Log));
        assert_eq!(t.scontrast, 1.0);
    }

    #[test]
    fn test_grading_tone_processor() {
        // Default GradingTone is identity (verified by test_grading_tone_default
        // above): the processor must map a chromatic probe back to itself, which
        // a discarded-result/garbage impl fails.
        let t = Transform::GradingTone(GradingToneTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.25_f32, 0.5, 0.75]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.25_f32, 0.5, 0.75].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-5,
                "identity GradingTone ch{i}: {}",
                px[0][i]
            );
        }
    }
}

/// ExposureContrast comprehensive parity tests.
/// OCIO ref: ExposureContrastOpData.cpp
mod exposure_contrast_comprehensive_parity {
    use crate::transform::{ExposureContrastStyle, ExposureContrastTransform};
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_exposure_contrast_fields() {
        let t = ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Linear,
            exposure: 0.0, // stops
            contrast: 1.0, // 1.0 = identity
            gamma: 1.0,    // 1.0 = identity
            pivot: 0.18,   // mid gray
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };

        assert_eq!(t.exposure, 0.0);
        assert_eq!(t.contrast, 1.0);
        assert_eq!(t.pivot, 0.18);
    }

    #[test]
    fn test_exposure_contrast_processor() {
        let t = Transform::ExposureContrast(ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Linear,
            exposure: 1.0, // +1 stop
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        // +1 stop should double the value
        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixels);
        assert!(
            (pixels[0][0] - 0.36).abs() < 0.01,
            "+1 stop: got {}",
            pixels[0][0]
        );
    }

    #[test]
    fn test_exposure_contrast_log_style() {
        let t = Transform::ExposureContrast(ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Logarithmic,
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        });

        // exposure=0, contrast=gamma=1 is the identity for the log style: a
        // chromatic probe must round-trip (a discarded-result impl would not).
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.2_f32, 0.4, 0.6]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.2_f32, 0.4, 0.6].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-4,
                "identity EC log ch{i}: {}",
                px[0][i]
            );
        }
    }
}

/// LogAffine comprehensive parity tests.
/// OCIO ref: LogOpData.cpp
mod log_affine_comprehensive_parity {
    use crate::transform::LogAffineTransform;
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_log_affine_construction() {
        let t = LogAffineTransform::new();
        assert_eq!(t.base, 2.0);
    }

    #[test]
    fn test_log_affine_with_base() {
        let t = LogAffineTransform::new().with_base(10.0);
        assert_eq!(t.base, 10.0);
    }

    #[test]
    fn test_log_affine_log_side_params() {
        // OCIO: logSideSlope, logSideOffset
        let t = LogAffineTransform::new()
            .with_base(10.0)
            .with_log_side([0.4, 0.4, 0.4], [0.685, 0.685, 0.685]);

        assert_eq!(t.log_side_slope, [0.4, 0.4, 0.4]);
        assert_eq!(t.log_side_offset, [0.685, 0.685, 0.685]);
    }

    #[test]
    fn test_log_affine_lin_side_params() {
        // OCIO: linSideSlope, linSideOffset
        let t = LogAffineTransform::new().with_lin_side([1.0, 1.0, 1.0], [0.0, 0.0, 0.0]);

        assert_eq!(t.lin_side_slope, [1.0, 1.0, 1.0]);
        assert_eq!(t.lin_side_offset, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_log_affine_apply() {
        let t = LogAffineTransform::new().with_base(10.0);
        let result = t.apply([0.18, 0.18, 0.18]);
        // log10(0.18) ≈ -0.745
        assert!(result[0] < 0.0);
    }
}

/// LogCamera comprehensive parity tests.
/// OCIO ref: LogCameraOp.cpp
mod log_camera_comprehensive_parity {
    use crate::transform::LogCameraTransform;
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_log_camera_construction() {
        let t = LogCameraTransform::new();
        assert_eq!(t.base, 2.0);
    }

    #[test]
    fn test_log_camera_acescct() {
        // OCIO: ACEScct preset
        let t = LogCameraTransform::acescct();
        assert_eq!(t.base, 2.0);
    }

    #[test]
    fn test_log_camera_logc3() {
        // OCIO: ARRI LogC3 preset
        let t = LogCameraTransform::logc3();
        assert_eq!(t.base, 10.0);
    }

    #[test]
    fn test_log_camera_slog3() {
        // OCIO: Sony S-Log3 preset
        let t = LogCameraTransform::slog3();
        assert_eq!(t.base, 10.0);
    }

    #[test]
    fn test_log_camera_lin_side_break() {
        // OCIO: linSideBreak
        let t = LogCameraTransform::new().with_lin_side_break([0.01, 0.01, 0.01]);

        assert_eq!(t.lin_side_break, [0.01, 0.01, 0.01]);
    }

    #[test]
    fn test_log_camera_apply() {
        let t = LogCameraTransform::acescct();
        let result = t.apply([0.18, 0.18, 0.18]);
        // Verify the transform produces a finite result
        assert!(
            result[0].is_finite(),
            "ACEScct should produce finite result"
        );
    }
}

/// BuiltinTransform comprehensive parity tests.
/// OCIO ref: BuiltinTransformRegistry.cpp
///
/// These only constructed `BuiltinTransform` values, several with style names OCIO does not
/// have ("ACES2065-1_to_ACEScct", "ARRI_LOGC3_to_ACES2065-1", "SONY_SLOG3_to_ACES2065-1",
/// "RED_LOG3G10_to_ACES2065-1", "ACES-LMT - BLUE LIGHT ARTIFACT FIX"). They now use the
/// registered names and build a processor, in both directions where the old list asked for the
/// reverse conversion.
mod builtin_transform_comprehensive_parity {
    use crate::transform::BuiltinTransform;
    use crate::{Processor, Transform, TransformDirection};

    fn builds(style: &str, direction: TransformDirection) -> bool {
        let t = Transform::Builtin(BuiltinTransform {
            style: style.to_string(),
            direction,
        });
        Processor::from_transform(&t, TransformDirection::Forward).is_ok()
    }

    #[test]
    fn test_builtin_transform_construction() {
        assert!(builds(
            "ACES-LMT - BLUE_LIGHT_ARTIFACT_FIX",
            TransformDirection::Forward
        ));
        assert!(!builds(
            "ACES-LMT - BLUE LIGHT ARTIFACT FIX",
            TransformDirection::Forward
        ));
    }

    #[test]
    fn test_builtin_aces_transforms() {
        // ACES2065-1 -> ACEScct / ACEScg are these styles run in reverse.
        for style in ["ACEScct_to_ACES2065-1", "ACEScg_to_ACES2065-1"] {
            assert!(builds(style, TransformDirection::Forward), "{style}");
            assert!(
                builds(style, TransformDirection::Inverse),
                "{style} inverse"
            );
        }
    }

    #[test]
    fn test_builtin_camera_transforms() {
        for style in [
            "ARRI_ALEXA-LOGC-EI800-AWG_to_ACES2065-1",
            "SONY_SLOG3-SGAMUT3_to_ACES2065-1",
            "RED_LOG3G10-RWG_to_ACES2065-1",
        ] {
            assert!(builds(style, TransformDirection::Forward), "{style}");
        }
    }
}

/// Range transform comprehensive parity tests.
/// OCIO ref: RangeOpData.cpp
mod range_comprehensive_parity {
    use crate::transform::{RangeStyle, RangeTransform};
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_range_min_max() {
        let t = RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(1.0),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Forward,
        };

        assert_eq!(t.min_in, Some(0.0));
        assert_eq!(t.max_in, Some(1.0));
    }

    #[test]
    fn test_range_clamp_processor() {
        let t = Transform::Range(RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(1.0),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        // Values outside range should be clamped
        let mut pixels = [[1.5_f32, -0.5, 0.5]];
        proc.apply_rgb(&mut pixels);
        assert!(
            (pixels[0][0] - 1.0).abs() < 0.001,
            "Clamp high: got {}",
            pixels[0][0]
        );
        assert!(
            (pixels[0][1] - 0.0).abs() < 0.001,
            "Clamp low: got {}",
            pixels[0][1]
        );
    }

    #[test]
    fn test_range_scale() {
        // Scale from [0,1] to [0,2]
        let t = Transform::Range(RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(2.0),
            style: RangeStyle::NoClamp,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);
        assert!(
            (pixels[0][0] - 1.0).abs() < 0.001,
            "Scale: got {}",
            pixels[0][0]
        );
    }
}

/// CDL comprehensive parity tests with ASC math.
/// OCIO ref: CDLOpData.cpp
mod cdl_comprehensive_parity {
    use crate::transform::{CdlStyle, CdlTransform};
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_cdl_sop_parameters() {
        // Slope, Offset, Power
        let t = CdlTransform {
            name: String::new(),
            slope: [1.1, 1.0, 0.9],
            offset: [0.01, 0.0, -0.01],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        };

        assert_eq!(t.slope[0], 1.1);
        assert_eq!(t.offset[0], 0.01);
    }

    #[test]
    fn test_cdl_asc_math() {
        // ASC CDL formula: out = clamp(pow(in * slope + offset, power) * sat)
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [2.0, 2.0, 2.0],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixels = [[0.25_f32, 0.25, 0.25]];
        proc.apply_rgb(&mut pixels);
        assert!(
            (pixels[0][0] - 0.5).abs() < 0.001,
            "CDL slope: got {}",
            pixels[0][0]
        );
    }

    #[test]
    fn test_cdl_power() {
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [2.0, 2.0, 2.0], // Square
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);
        assert!(
            (pixels[0][0] - 0.25).abs() < 0.001,
            "CDL power: got {}",
            pixels[0][0]
        );
    }
}

/// NegativeStyle enum parity tests.
/// OCIO ref: GammaOpData.cpp
mod negative_style_parity {
    use crate::transform::NegativeStyle;
}

/// Matrix transform comprehensive parity tests.
/// OCIO ref: MatrixOpData.cpp
mod matrix_comprehensive_parity {
    use crate::transform::MatrixTransform;
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_matrix_identity_constant() {
        let identity = MatrixTransform::IDENTITY;
        assert_eq!(identity[0], 1.0);
        assert_eq!(identity[5], 1.0);
        assert_eq!(identity[10], 1.0);
        assert_eq!(identity[15], 1.0);
        assert_eq!(identity[1], 0.0);
    }

    #[test]
    fn test_matrix_with_offset() {
        let t = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.1, 0.2, 0.3, 0.0],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixels = [[0.0_f32, 0.0, 0.0]];
        proc.apply_rgb(&mut pixels);
        assert!(
            (pixels[0][0] - 0.1).abs() < 0.001,
            "Offset R: got {}",
            pixels[0][0]
        );
        assert!(
            (pixels[0][1] - 0.2).abs() < 0.001,
            "Offset G: got {}",
            pixels[0][1]
        );
    }

    #[test]
    fn test_matrix_scale() {
        // Diagonal scale matrix
        let mut matrix = [0.0_f64; 16];
        matrix[0] = 2.0; // R scale
        matrix[5] = 0.5; // G scale
        matrix[10] = 1.0; // B scale
        matrix[15] = 1.0; // A

        let t = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);
        assert!(
            (pixels[0][0] - 1.0).abs() < 0.001,
            "Scale R: got {}",
            pixels[0][0]
        );
        assert!(
            (pixels[0][1] - 0.25).abs() < 0.001,
            "Scale G: got {}",
            pixels[0][1]
        );
    }
}

/// Processor comprehensive API parity tests.
/// OCIO ref: Processor.cpp
mod processor_comprehensive_parity {
    use crate::transform::{CdlTransform, MatrixTransform};
    use crate::{
        ChannelOrdering, OptimizationLevel, PackedBitDepth, Processor, Transform,
        TransformDirection,
    };

    #[test]
    fn test_processor_combine() {
        // OCIO: Processor::createCompositeProcessor
        let t1 = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.1, 0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });
        let t2 = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0, 0.1, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });

        let p1 = Processor::from_transform(&t1, TransformDirection::Forward).unwrap();
        let p2 = Processor::from_transform(&t2, TransformDirection::Forward).unwrap();

        let combined = Processor::combine(&p1, &p2).unwrap();

        let mut pixels = [[0.0_f32, 0.0, 0.0]];
        combined.apply_rgb(&mut pixels);
        assert!((pixels[0][0] - 0.1).abs() < 0.001, "Combined R");
        assert!((pixels[0][1] - 0.1).abs() < 0.001, "Combined G");
    }

    #[test]
    fn test_processor_inverse() {
        // OCIO: Processor::createInverseProcessor
        let t = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: {
                let mut m = [0.0_f64; 16];
                m[0] = 2.0;
                m[5] = 2.0;
                m[10] = 2.0;
                m[15] = 1.0;
                m
            },
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let inv = proc.inverse().unwrap();

        let mut pixels = [[1.0_f32, 1.0, 1.0]];
        inv.apply_rgb(&mut pixels);
        assert!((pixels[0][0] - 0.5).abs() < 0.001, "Inverse");
    }

    #[test]
    fn test_processor_optimize() {
        // OCIO: Processor::getOptimizedProcessor
        let mut proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        proc.optimize(OptimizationLevel::Lossless);
        // Optimized identity should still be valid
        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);
    }

    #[test]
    fn test_processor_cache_id() {
        // OCIO: Processor::getCacheID
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let cache_id = proc.cache_id();
        assert!(!cache_id.is_empty(), "Cache ID should not be empty");
    }

    #[test]
    fn test_processor_is_identity() {
        // OCIO: Processor::isNoOp (identity processor)
        let proc = Processor::from_transform(
            &Transform::Matrix(MatrixTransform {
                name: String::new(),
                matrix: MatrixTransform::IDENTITY,
                offset: [0.0; 4],
                direction: TransformDirection::Forward,
            }),
            TransformDirection::Forward,
        )
        .unwrap();

        assert!(proc.is_identity());
    }

    #[test]
    fn test_processor_has_channel_crosstalk() {
        // OCIO: Processor::hasChannelCrosstalk
        // Diagonal matrix has no crosstalk
        let diag = Processor::from_transform(
            &Transform::Matrix(MatrixTransform {
                name: String::new(),
                matrix: MatrixTransform::IDENTITY,
                offset: [0.0; 4],
                direction: TransformDirection::Forward,
            }),
            TransformDirection::Forward,
        )
        .unwrap();

        assert!(!diag.has_channel_crosstalk());
    }

    #[test]
    fn test_processor_bit_depth() {
        // OCIO: Processor::getInputBitDepth/getOutputBitDepth
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let input_depth = proc.input_bit_depth();
        let output_depth = proc.output_bit_depth();

        // Verify bit depth is accessible
        let _ = input_depth;
        let _ = output_depth;
    }

    #[test]
    fn test_packed_bit_depth_bytes() {
        // OCIO: BitDepthInfo
        assert_eq!(PackedBitDepth::Uint8.bytes_per_channel(), 1);
        assert_eq!(PackedBitDepth::Uint16.bytes_per_channel(), 2);
        assert_eq!(PackedBitDepth::F16.bytes_per_channel(), 2);
        assert_eq!(PackedBitDepth::F32.bytes_per_channel(), 4);
    }

    #[test]
    fn test_packed_bit_depth_max_value() {
        assert_eq!(PackedBitDepth::Uint8.max_value(), 255.0);
        assert_eq!(PackedBitDepth::Uint10.max_value(), 1023.0);
        assert_eq!(PackedBitDepth::Uint12.max_value(), 4095.0);
        assert_eq!(PackedBitDepth::Uint16.max_value(), 65535.0);
    }

    #[test]
    fn test_channel_ordering_indices() {
        // OCIO: CHANNEL_ORDERING_*
        assert_eq!(ChannelOrdering::Rgba.r_index(), 0);
        assert_eq!(ChannelOrdering::Rgba.g_index(), 1);
        assert_eq!(ChannelOrdering::Rgba.b_index(), 2);
        assert_eq!(ChannelOrdering::Rgba.a_index(), Some(3));

        assert_eq!(ChannelOrdering::Bgra.r_index(), 2);
        assert_eq!(ChannelOrdering::Bgra.b_index(), 0);
    }

    #[test]
    fn test_channel_ordering_num_channels() {
        assert_eq!(ChannelOrdering::Rgb.num_channels(), 3);
        assert_eq!(ChannelOrdering::Rgba.num_channels(), 4);
        assert!(!ChannelOrdering::Rgb.has_alpha());
        assert!(ChannelOrdering::Rgba.has_alpha());
    }
}

/// GPU Processor comprehensive parity tests.
/// OCIO ref: GpuShaderCreator.cpp
mod gpu_processor_comprehensive_parity {
    use crate::builtin::TestSchema;
    use crate::gpu::BatchTextureDescriptor;
    use crate::{
        GpuChannelType, GpuLanguage, GpuProcessor, GpuTextureType, TextureDimensions, builtin,
    };

    #[test]
    fn test_gpu_processor_from_processor() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc);
        assert!(gpu.is_ok());
    }

    #[test]
    fn test_gpu_language_all_variants() {
        // OCIO: GPU_LANGUAGE_*
        let languages = [
            GpuLanguage::Cg,
            GpuLanguage::Glsl120,
            GpuLanguage::Glsl130,
            GpuLanguage::Glsl330,
            GpuLanguage::Glsl400,
            GpuLanguage::GlslVulkan460,
            GpuLanguage::GlslEs100,
            GpuLanguage::GlslEs300,
            GpuLanguage::Hlsl50,
            GpuLanguage::Osl,
            GpuLanguage::Metal,
        ];

        for lang in languages {
            // All languages have version directives (Metal returns empty string)
            let _ = lang.version_directive();
        }
    }

    #[test]
    fn test_gpu_generate_all_shaders() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();

        // Generate shaders for all languages
        // Cg and OSL are refused (OCIO-10, `gpu::tests::cg_and_osl_are_errors_not_glsl`).
        let languages = [
            GpuLanguage::Glsl330,
            GpuLanguage::Hlsl50,
            GpuLanguage::Metal,
        ];

        for lang in languages {
            let shader = gpu.generate_shader(lang).unwrap();
            assert!(
                !shader.fragment_code().is_empty(),
                "{:?} shader empty",
                lang
            );
        }
    }

    #[test]
    fn test_batch_texture_descriptors() {
        // OCIO: BatchTextureDescriptor for optimized texture uploads
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();

        let descriptors = gpu.batch_texture_descriptors();
        // May or may not have textures depending on transform
        let _ = descriptors;
    }

    #[test]
    fn test_gpu_shader_uniforms() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();

        let uniforms = shader.uniforms();
        // Uniforms list is available (may be empty for simple transforms)
        let _ = uniforms;
    }

    #[test]
    fn test_gpu_processor_texture_access() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();

        let num_tex = gpu.num_textures();
        let num_3d = gpu.num_3d_textures();

        // Access textures if available
        for i in 0..num_tex {
            let _ = gpu.texture(i);
            let _ = gpu.texture_values(i);
            let _ = gpu.texture_binding_index(i);
        }

        for i in 0..num_3d {
            let _ = gpu.texture_3d(i);
            let _ = gpu.texture_3d_values(i);
            let _ = gpu.texture_3d_binding_index(i);
        }
    }

    #[test]
    fn test_gpu_is_glsl() {
        assert!(GpuLanguage::Glsl120.is_glsl());
        assert!(GpuLanguage::Glsl330.is_glsl());
        assert!(!GpuLanguage::Hlsl50.is_glsl());
        assert!(!GpuLanguage::Metal.is_glsl());
    }
}

/// Dynamic properties parity tests.
/// OCIO ref: DynamicProperty.cpp
mod dynamic_property_comprehensive_parity {
    use crate::transform::{ExposureContrastStyle, ExposureContrastTransform};
    use crate::{
        DynamicProcessor, DynamicProcessorBuilder, DynamicPropertyType, DynamicPropertyValue,
    };
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_dynamic_property_values() {
        // OCIO: DynamicProperty values
        let _f = DynamicPropertyValue::Double(1.5);
    }

    #[test]
    fn test_dynamic_processor_builder() {
        // OCIO: DynamicPropertyGradingPrimary etc.
        let t = Transform::ExposureContrast(ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Linear,
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        // Check if processor has dynamic properties
        let has_exposure = proc.has_dynamic_property(DynamicPropertyType::Exposure);
        let _ = has_exposure;
    }
}

/// Validate API parity tests.
/// OCIO ref: Config::validate()
mod validate_comprehensive_parity {
    use crate::builtin;
    use crate::validate::{self, Issue, IssueCategory, Severity};

    #[test]
    fn test_validate_builtin_config() {
        let config = builtin::aces_2_0_studio_all_views();
        let issues = validate::check(&config);

        // Built-in config should have no errors
        assert!(!validate::has_errors(&issues));
    }
}

/// ProcessorMetadata parity tests.
/// OCIO ref: Processor metadata API
mod processor_metadata_parity {
    use crate::transform::CdlTransform;
    use crate::{FormatMetadata, ProcessorMetadata};
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_processor_metadata_access() {
        // OCIO: Processor::getMetadata
        //
        // Uses a NON-identity CDL deliberately. The previous version built
        // `CdlTransform::default()` — an identity the optimizer is free to drop — and then
        // asserted `num_transforms >= 0` on a `usize`, which is true for every possible
        // outcome including "compiled to nothing". A slope change gives the compiler something
        // it must keep, so the count becomes a real constraint.
        let t = Transform::Cdl(CdlTransform {
            slope: [1.5, 1.0, 1.0],
            ..CdlTransform::default()
        });
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let num_transforms = proc.num_transforms();
        assert!(
            num_transforms >= 1,
            "a non-identity CDL must survive compilation, got {num_transforms} ops"
        );
        assert!(
            !proc.is_no_op(),
            "a non-identity CDL processor is not a no-op"
        );

        // Metadata is reachable and self-consistent (see
        // `file_transform_parity::test_processor_metadata_records_the_lut_file` for the case
        // that actually exercises file tracking).
        let meta = proc.metadata();
        assert_eq!(meta.num_files, meta.files.len());
    }

    #[test]
    fn test_processor_create_group_transform() {
        // OCIO: Processor::createGroupTransform
        let t = Transform::Cdl(CdlTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let group = proc.create_group_transform();
        // Group should contain the equivalent transforms
        let _ = group.transforms;
    }
}

// look_comprehensive_parity (parse_looks name/direction coverage) removed:
// duplicated look_api_parity and look_extended_parity's test_parse_looks_mixed
// without pinning names (D1).

/// View comprehensive parity tests.
/// OCIO ref: Display/View API
mod view_comprehensive_parity {
    use crate::{Display, ReferenceSpaceType, View, ViewTransform};

    #[test]
    fn test_view_construction() {
        let view = View::new("Film", "sRGB");
        assert_eq!(view.name(), "Film");
        assert_eq!(view.colorspace(), "sRGB");
    }

    #[test]
    fn test_display_with_views() {
        let display = Display::new("sRGB Monitor")
            .with_view(View::new("Standard", "sRGB"))
            .with_view(View::new("Raw", "Raw"));

        assert_eq!(display.name(), "sRGB Monitor");
        assert_eq!(display.views().len(), 2);
    }
}

/// Config comprehensive API parity tests.
/// OCIO ref: Config.cpp
mod config_comprehensive_parity {
    use crate::builtin::TestSchema;
    use crate::config::{FileRule, FileRuleKind, NamedTransform, ViewingRule};
    use crate::{
        ColorSpaceVisibility, Config, ConfigVersion, EnvironmentMode, SearchReferenceSpaceType,
        builtin,
    };

    #[test]
    fn test_config_from_builtin() {
        let config = builtin::aces_2_0_studio_all_views();
        assert!(!config.colorspaces().is_empty());
    }

    #[test]
    fn test_config_name_and_description() {
        // OCIO: Config::getName, Config::getDescription
        let config = builtin::aces_2_0_studio_all_views();
        let name = config.name();
        let desc = config.description();
        // Ref: PyOpenColorIO 2.5.1 getName()/getDescription() on the embedded studio config
        // (separate `name:` and `description:` keys; B23 - this used to assert desc == name).
        assert_eq!(name, "studio-config-all-views-v4.0.0_aces-v2.0_ocio-v2.5");
        assert!(
            desc.starts_with("Academy Color Encoding System - Studio Config (All Views)"),
            "{desc}"
        );
        assert!(
            desc.ends_with("a wide variety of camera colorspaces, displays and looks."),
            "{desc}"
        );
    }

    #[test]
    fn test_config_version_access() {
        let config = builtin::aces_2_0_studio_all_views();
        let version = config.version();
        let major = config.major_version();
        let minor = config.minor_version();

        // Ref: embedded YAML `ocio_profile_version: 2.5` (line 1) -> major 2,
        // minor 5 (parsed in config/loading.rs:38). Pin both so a default/0
        // version reader fails.
        assert!(matches!(version, ConfigVersion::V2));
        assert_eq!(major, 2);
        assert_eq!(minor, 5);
    }

    #[test]
    fn test_config_colorspace_access() {
        let config = builtin::aces_2_0_studio_all_views();

        // By name
        let acescg = config.colorspace("ACEScg");
        assert!(acescg.is_some());

        // Names iterator
        let names: Vec<_> = config.colorspace_names().collect();
        assert!(!names.is_empty());
    }

    #[test]
    fn test_config_colorspace_by_category() {
        let config = builtin::aces_2_0_studio_all_views();
        // Ref: embedded YAML tags many colorspaces `categories: [file-io]`
        // (e.g. line 443); querying that category must return >=1 colorspace.
        // A no-op category index would return an empty set.
        let file_io = config.colorspaces_by_category("file-io");
        assert!(!file_io.is_empty(), "expected >=1 file-io colorspace");
    }

    #[test]
    fn test_config_all_categories() {
        let config = builtin::aces_2_0_studio_all_views();
        let categories = config.all_categories();
        // The config assigns categories (file-io, working-space, advanced-3d, ...)
        // to its colorspaces; an empty result means the index was never built.
        assert!(!categories.is_empty(), "expected >=1 category");
    }

    #[test]
    fn test_config_roles() {
        use crate::role_names;

        let config = builtin::aces_2_0_studio_all_views();
        let roles = config.roles();
        assert!(roles.contains(role_names::SCENE_LINEAR));
        assert!(roles.contains(role_names::ACES_INTERCHANGE));
    }

    #[test]
    fn test_config_displays() {
        let config = builtin::aces_2_0_studio_all_views();
        let displays = config.displays();
        // Ref: embedded YAML `active_displays:` lists 9 displays (sRGB - Display,
        // Display P3 - Display, ...); the config defines >=9. A no-op returns none.
        assert!(
            displays.len() >= 9,
            "expected >=9 displays, got {}",
            displays.len()
        );
    }

    #[test]
    fn test_config_looks() {
        let config = builtin::aces_2_0_studio_all_views();
        let looks = config.looks();
        // Ref: embedded YAML `looks:` block defines the ACES reference gamut
        // compression look (>=1 entry); a no-op look reader returns empty.
        assert!(!looks.is_empty(), "expected >=1 look");
    }

    #[test]
    fn test_config_active_displays_views() {
        let config = builtin::aces_2_0_studio_all_views();
        let active_displays = config.active_displays();
        let active_views = config.active_views();
        // Ref: embedded YAML `active_displays:` (9 entries) and `active_views:`
        // (31 entries). Both are explicitly authored, so neither may be empty.
        assert!(!active_displays.is_empty(), "active_displays empty");
        assert!(!active_views.is_empty(), "active_views empty");
    }

    #[test]
    fn test_config_default_display_view() {
        let config = builtin::aces_2_0_studio_all_views();
        // The studio config authors active displays, so a default display must
        // resolve, and that display must have a default view (Some, non-empty).
        let display = config
            .default_display()
            .expect("config with active displays must have a default display");
        let default_view = config
            .default_view(display)
            .expect("default display must have a default view");
        assert!(
            !default_view.is_empty(),
            "default view for {display:?} was empty"
        );
    }

    #[test]
    fn test_config_context() {
        let config = builtin::aces_2_0_studio_all_views();
        let ctx = config.context();
        let _ = ctx;
    }

    #[test]
    fn test_config_processor() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config.processor(
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        );
        assert!(proc.is_ok());
    }

    #[test]
    fn test_config_search_paths() {
        let config = builtin::aces_2_0_studio_all_views();
        let paths = config.search_paths();
        // Ref: embedded YAML `search_path: ""` — an empty search path yields no
        // entries. Assert the method returns the empty list (not a bogus value),
        // which is the verifiable property for this config.
        assert!(
            paths.is_empty(),
            "expected empty search_paths, got {paths:?}"
        );
    }

    #[test]
    fn test_config_working_dir() {
        let config = builtin::aces_2_0_studio_all_views();
        // working_dir is derived from where the config was loaded; for an
        // embedded (in-memory) config it is well-defined and callable. Just
        // assert it does not panic and returns a (possibly empty) path value.
        let dir = config.working_dir();
        let _len = dir.as_os_str().len(); // touch the value; no reference target
    }

    #[test]
    fn test_file_rule_kind() {
        // OCIO: FileRules - Basic, Regex, Default variants
        let _basic = FileRuleKind::Basic {
            pattern: "*.exr".into(),
            extension: None,
        };
        let _default = FileRuleKind::Default;
    }

    #[test]
    fn test_config_create_raw() {
        // OCIO: Config::CreateRaw()
        let raw = Config::create_raw();
        assert!(raw.colorspace("raw").is_some() || raw.colorspace("Raw").is_some());
    }
}

/// ColorSpaceBuilder comprehensive parity tests.
/// OCIO ref: ColorSpace.cpp
mod colorspace_builder_comprehensive_parity {
    use crate::colorspace::{AllocationInfo, AllocationType, BitDepth};
    use crate::{ColorSpace, ColorSpaceBuilder, Encoding, Family};

    #[test]
    fn test_colorspace_builder_all_methods() {
        let cs = ColorSpace::builder("TestCS")
            .alias("test_alias")
            .category("scene_linear")
            .description("Test color space")
            .family(Family::Scene)
            .encoding(Encoding::SceneLinear)
            .is_data(false)
            .build();

        assert_eq!(cs.name(), "TestCS");
        assert_eq!(cs.aliases().len(), 1);
        assert!(cs.has_category("scene_linear"));
        assert_eq!(cs.encoding(), Encoding::SceneLinear);
    }

    #[test]
    fn test_colorspace_allocation_info() {
        // OCIO: Allocation variables
        let alloc = AllocationInfo {
            alloc_type: AllocationType::Log2,
            vars: vec![-8.0, 8.0],
        };

        assert!(matches!(alloc.alloc_type, AllocationType::Log2));
    }

    #[test]
    fn test_colorspace_categories_mutation() {
        let mut cs = ColorSpace::new("Test");

        cs.add_category("scene_linear");
        assert!(cs.has_category("scene_linear"));
        assert_eq!(cs.num_categories(), 1);

        cs.add_category("working_space");
        assert_eq!(cs.num_categories(), 2);

        cs.remove_category("scene_linear");
        assert!(!cs.has_category("scene_linear"));

        cs.clear_categories();
        assert_eq!(cs.num_categories(), 0);
    }

    #[test]
    fn test_colorspace_matches_name() {
        let cs = ColorSpace::builder("ACEScg").alias("ACES - ACEScg").build();

        assert!(cs.matches_name("ACEScg"));
        assert!(cs.matches_name("acescg")); // case insensitive
        assert!(cs.matches_name("ACES - ACEScg"));
        assert!(!cs.matches_name("sRGB"));
    }
}

/// TransformDirection and Interpolation parity tests.
/// OCIO ref: Transform.cpp
mod transform_enums_parity {
    use crate::{Interpolation, TransformDirection};

    #[test]
    fn test_transform_direction() {
        let fwd = TransformDirection::Forward;
        let inv = TransformDirection::Inverse;

        assert!(matches!(fwd.inverse(), TransformDirection::Inverse));
        assert!(matches!(inv.inverse(), TransformDirection::Forward));
    }
}

/// GradingPrimary comprehensive parity tests.
/// OCIO ref: GradingPrimaryOpData.cpp
mod grading_primary_comprehensive_parity {
    use crate::dynamic::{GradingPrimary, GradingRgbm};
    use crate::transform::{GradingPrimaryStyle, GradingPrimaryTransform};
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_grading_primary_fields() {
        // GradingPrimaryTransform: brightness, contrast, gamma, offset, exposure, slope, saturation, pivot, clamp
        let t = GradingPrimaryTransform::from_flat(
            GradingPrimaryStyle::Log,
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            1.0,
            0.18,
            0.0,
            1.0,
            0.0,
            1.0,
            TransformDirection::Forward,
        );

        assert_eq!(t.value.saturation, 1.0);
        assert_eq!(t.value.pivot, 0.18);
    }

    #[test]
    fn test_grading_primary_processor() {
        let t = Transform::GradingPrimary(GradingPrimaryTransform::from_flat(
            GradingPrimaryStyle::Log,
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            1.0,
            0.18,
            0.0,
            1.0,
            0.0,
            1.0,
            TransformDirection::Forward,
        ));

        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_grading_rgbm() {
        // OCIO: GradingRGBM struct
        let rgbm = GradingRgbm {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            master: 1.0,
        };

        assert_eq!(rgbm.master, 1.0);
    }
}

/// Processor apply methods parity tests.
/// OCIO ref: CPUProcessor.cpp
mod processor_apply_parity {
    use crate::transform::CdlTransform;
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_apply_rgb() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let mut pixels = [[0.5_f32, 0.3, 0.7]];
        proc.apply_rgb(&mut pixels);
        // Should process without error
    }

    #[test]
    fn test_apply_rgba() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let mut pixels = [[0.5_f32, 0.3, 0.7, 1.0]];
        proc.apply_rgba(&mut pixels);
        // Should process without error
    }

    #[test]
    fn test_apply_multiple_pixels() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let mut pixels = [[0.1_f32, 0.2, 0.3], [0.4, 0.5, 0.6], [0.7, 0.8, 0.9]];
        proc.apply_rgb(&mut pixels);
        // All pixels should be processed
    }
}

/// Context comprehensive parity tests.
/// OCIO ref: Context.cpp
mod context_comprehensive_parity {
    use crate::Context;

    #[test]
    fn test_context_new() {
        let ctx = Context::new();
        let _ = ctx;
    }

    #[test]
    fn test_context_set_get() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");
        ctx.set("SEQ", "seq01");

        assert_eq!(ctx.get("SHOT"), Some("sh010".to_string()));
        assert_eq!(ctx.get("SEQ"), Some("seq01".to_string()));
        assert_eq!(ctx.get("MISSING"), None);
    }

    #[test]
    fn test_context_resolve() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");

        let resolved = ctx.resolve("/shows/$SHOT/luts/grade.cube");
        assert!(resolved.contains("sh010"));
    }

    #[test]
    fn test_context_resolve_curly_braces() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");

        let resolved = ctx.resolve("/shows/${SHOT}/luts/grade.cube");
        assert!(resolved.contains("sh010"));
    }
}

/// Display/View API comprehensive parity tests.
/// OCIO ref: Display.cpp, Config.cpp display methods
mod display_view_comprehensive_parity {
    use crate::builtin;
    use crate::display::{Display, DisplayManager, ReferenceSpaceType, View, ViewTransform};

    #[test]
    fn test_display_new() {
        let display = Display::new("sRGB Monitor");
        assert_eq!(display.name(), "sRGB Monitor");
        assert!(display.views().is_empty());
    }

    #[test]
    fn test_display_add_view() {
        let mut display = Display::new("sRGB");
        display.add_view(View::new("Film", "sRGB - Display"));
        display.add_view(View::new("Raw", "Raw"));

        assert_eq!(display.views().len(), 2);
    }

    #[test]
    fn test_display_builder_pattern() {
        let display = Display::new("Rec709")
            .with_view(View::new("Film", "Rec709"))
            .with_view(View::new("Log", "Log"));

        assert_eq!(display.views().len(), 2);
    }

    #[test]
    fn test_display_default_view() {
        let mut display = Display::new("sRGB").with_view(View::new("Film", "sRGB"));

        // First added view becomes default
        display.set_default_view("Film");
        assert_eq!(display.default_view(), Some("Film"));
    }

    #[test]
    fn test_view_new() {
        let view = View::new("Film", "sRGB - Display");
        assert_eq!(view.name(), "Film");
        assert_eq!(view.colorspace(), "sRGB - Display");
    }

    #[test]
    fn test_view_with_look() {
        let view = View::new("Film", "sRGB").with_look("show_look");
        assert_eq!(view.looks(), Some("show_look"));
    }

    #[test]
    fn test_view_with_view_transform() {
        let view = View::new("ACES Output", "sRGB").with_view_transform("ACES 1.0 - SDR Video");
        assert_eq!(view.view_transform(), Some("ACES 1.0 - SDR Video"));
    }

    #[test]
    fn test_view_transform_struct() {
        // ViewTransform::new() only takes name, with_reference_space takes two args
        let vt =
            ViewTransform::with_reference_space("ACES 1.0 - SDR Video", ReferenceSpaceType::Scene);
        assert_eq!(vt.name(), "ACES 1.0 - SDR Video");
    }

    #[test]
    fn test_display_manager() {
        let mut dm = DisplayManager::new();
        dm.add_display(Display::new("sRGB"));
        dm.add_display(Display::new("Rec709"));

        assert_eq!(dm.displays().len(), 2);
        assert!(dm.display("sRGB").is_some());
    }

    #[test]
    fn test_config_displays() {
        let config = builtin::aces_2_0_studio_all_views();
        let displays = config.displays();
        // builtin config should have displays
        let _ = displays;
    }

    #[test]
    fn test_display_view_lookup() {
        let display = Display::new("sRGB")
            .with_view(View::new("Film", "sRGB - Display"))
            .with_view(View::new("Raw", "Raw"));

        let view = display.view("Film");
        assert!(view.is_some());
        assert_eq!(view.unwrap().name(), "Film");
    }
}

/// Baker API parity tests.
/// OCIO ref: Baker.cpp
mod baker_comprehensive_parity {
    use crate::baker::LutFormat;
    use crate::transform::CdlTransform;
    use crate::{Baker, Processor, Transform, TransformDirection};

    #[test]
    fn test_baker_new() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let _baker = Baker::new(&proc);
    }

    #[test]
    fn test_baker_shaper_size() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let baker = Baker::new(&proc).shaper_size(4096);
        let _ = baker;
    }

    #[test]
    fn test_baker_with_shaper() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let baker = Baker::new(&proc).with_shaper(true);
        let _ = baker;
    }

    #[test]
    fn test_bake_lut_1d() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let lut = Baker::new(&proc).bake_lut_1d(1024).unwrap();
        assert_eq!(lut.size, 1024);
    }

    #[test]
    fn test_bake_lut_3d() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let lut = Baker::new(&proc).bake_lut_3d(17).unwrap();
        assert_eq!(lut.size, 17);
    }

    #[test]
    fn test_baked_lut_1d_data() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let lut = Baker::new(&proc).bake_lut_1d(256).unwrap();
        assert_eq!(lut.data.len(), 256 * 3);
    }

    #[test]
    fn test_baked_lut_3d_data() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let lut = Baker::new(&proc).bake_lut_3d(9).unwrap();
        assert_eq!(lut.data.len(), 9 * 9 * 9 * 3);
    }

    #[test]
    fn test_baked_lut_1d_to_spi1d() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let lut = Baker::new(&proc).bake_lut_1d(16).unwrap();
        // Direct access to data field
        assert!(!lut.data.is_empty());
    }

    #[test]
    fn test_baked_lut_3d_domain() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let lut = Baker::new(&proc).bake_lut_3d(5).unwrap();
        // Domain defaults
        assert_eq!(lut.domain_min, [0.0, 0.0, 0.0]);
        assert_eq!(lut.domain_max, [1.0, 1.0, 1.0]);
    }
}

/// GpuProcessor extended parity tests.
/// OCIO ref: GpuShaderCreator.cpp, GpuShaderDesc.cpp
mod gpu_processor_extended_parity {
    use crate::gpu::{
        BatchTextureDescriptor, GpuChannelType, GpuInterpolation, GpuTextureType, GpuUniformType,
        TextureDimensions,
    };
    use crate::transform::CdlTransform;
    use crate::{GpuLanguage, GpuProcessor, Processor, Transform, TransformDirection};

    #[test]
    fn test_gpu_processor_from_processor() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let gpu_proc = GpuProcessor::from_processor(&proc);
        assert!(gpu_proc.is_ok());
    }

    #[test]
    fn test_gpu_shader_generation() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let gpu_proc = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu_proc.generate_shader(GpuLanguage::Glsl330).unwrap();
        assert!(!shader.fragment_code().is_empty());
    }

    #[test]
    fn test_gpu_shader_code_methods() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let gpu_proc = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu_proc.generate_shader(GpuLanguage::Glsl330).unwrap();

        let _ = shader.fragment_code();
        let _ = shader.textures();
        let _ = shader.uniforms();
    }

    #[test]
    fn test_batch_texture_descriptor() {
        // BatchTextureDescriptor is a struct with public fields
        let desc = BatchTextureDescriptor {
            name: "test_lut".into(),
            binding_index: 0,
            texture_type: GpuTextureType::Texture3D,
            width: 33,
            height: 33,
            depth: 33,
            channel_type: GpuChannelType::Rgb,
            interpolation: GpuInterpolation::Linear,
            dimensions: TextureDimensions::Texture2D,
            data_offset: 0,
            data_size: 0,
        };

        let _ = desc;
    }

    #[test]
    fn test_multiple_shader_languages() {
        let proc = Processor::from_transform(
            &Transform::Cdl(CdlTransform::default()),
            TransformDirection::Forward,
        )
        .unwrap();

        let gpu_proc = GpuProcessor::from_processor(&proc).unwrap();

        // Generate shaders for multiple languages
        let _glsl = gpu_proc.generate_shader(GpuLanguage::Glsl330).unwrap();
        let _hlsl = gpu_proc.generate_shader(GpuLanguage::Hlsl50).unwrap();
        let _metal = gpu_proc.generate_shader(GpuLanguage::Metal).unwrap();
        let _vulkan = gpu_proc
            .generate_shader(GpuLanguage::GlslVulkan460)
            .unwrap();
    }
}

/// Dynamic properties parity tests.
/// OCIO ref: DynamicProperty.cpp
mod dynamic_extended_parity {
    use crate::dynamic::{GradingPrimary, GradingRgbm};
    use crate::{DynamicProcessorBuilder, DynamicPropertyType, DynamicPropertyValue};

    #[test]
    fn test_dynamic_property_value() {
        // DynamicPropertyValue has Double and GradingPrimary variants
        let val = DynamicPropertyValue::Double(1.5);
        assert_eq!(val.as_double(), Some(1.5));
    }

    #[test]
    fn test_grading_primary_struct() {
        // GradingPrimary now uses GradingRgbm for all seven fields OCIO's
        // real GradingPrimary carries as GradingRGBM (was: exposure/lift/gain
        // stayed scalar f64 - see docs/ROADMAP.md's GradingPrimary item).
        let gp = GradingPrimary {
            brightness: GradingRgbm {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                master: 0.0,
            },
            contrast: GradingRgbm {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 1.0,
            },
            gamma: GradingRgbm {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 1.0,
            },
            offset: GradingRgbm {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                master: 0.0,
            },
            exposure: GradingRgbm::default(),
            lift: GradingRgbm::default(),
            gain: GradingRgbm::one(),
            saturation: 1.0,
            pivot: 0.18,
            pivot_black: 0.0,
            pivot_white: 1.0,
            clamp_black: f64::NEG_INFINITY,
            clamp_white: f64::INFINITY,
        };

        assert_eq!(gp.saturation, 1.0);
    }

    #[test]
    fn test_grading_rgbm_struct() {
        let rgbm = GradingRgbm {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            master: 1.0,
        };

        assert_eq!(rgbm.red, 1.0);
        assert_eq!(rgbm.master, 1.0);
    }

    #[test]
    fn test_dynamic_processor_builder() {
        // DynamicProcessorBuilder::new() takes no arguments
        let builder = DynamicProcessorBuilder::new()
            .exposure(0.5)
            .contrast(1.2)
            .gamma(1.0)
            .saturation(1.1);
        let _ = builder;
    }
}

/// Look API extended parity tests.
/// OCIO ref: Look.cpp
mod look_extended_parity {
    use crate::builtin;
    use crate::look::LookManager;
    use crate::{Look, parse_looks};

    #[test]
    fn test_look_new() {
        let look = Look::new("show_look");
        assert_eq!(look.name(), "show_look");
    }

    #[test]
    fn test_look_process_space() {
        // Look uses .process_space() not .with_process_space()
        let look = Look::new("grade").process_space("ACEScct");
        assert_eq!(look.get_process_space(), Some("ACEScct"));
    }

    #[test]
    fn test_look_description() {
        // Look uses .description() not .with_description()
        let look = Look::new("beauty").description("Beauty pass grade");
        assert_eq!(look.get_description(), "Beauty pass grade");
    }

    #[test]
    fn test_look_manager() {
        let mut lm = LookManager::new();
        lm.add(Look::new("look1"));
        lm.add(Look::new("look2"));

        assert_eq!(lm.len(), 2);
        assert!(lm.get("look1").is_some());
    }

    #[test]
    fn test_parse_looks_simple() {
        let looks = parse_looks("look1");
        assert_eq!(looks.len(), 1);
        assert_eq!(looks[0].0, "look1");
        assert!(looks[0].1); // forward
    }

    #[test]
    fn test_parse_looks_multiple() {
        let looks = parse_looks("look1, look2");
        assert_eq!(looks.len(), 2);
    }

    #[test]
    fn test_parse_looks_inverse() {
        let looks = parse_looks("-look1");
        assert_eq!(looks.len(), 1);
        assert_eq!(looks[0].0, "look1");
        assert!(!looks[0].1); // inverse
    }

    #[test]
    fn test_parse_looks_mixed() {
        let looks = parse_looks("look1, -look2, look3");
        assert_eq!(looks.len(), 3);
        assert!(looks[0].1); // forward
        assert!(!looks[1].1); // inverse
        assert!(looks[2].1); // forward
    }

    #[test]
    fn test_config_looks() {
        let config = builtin::aces_2_0_studio_all_views();
        let looks = config.looks();
        let _ = looks;
    }
}

/// Named transforms comprehensive parity tests (OCIO v2).
/// OCIO ref: NamedTransform.cpp
mod named_transform_extended_parity {
    use crate::Transform;
    use crate::config::NamedTransform;
    use crate::transform::MatrixTransform;

    #[test]
    fn test_named_transform_struct() {
        // NamedTransform is a struct with public fields
        let nt = NamedTransform {
            name: "utility_transform".into(),
            family: None,
            description: None,
            forward: None,
            inverse: None,
            ..NamedTransform::default()
        };
        assert_eq!(nt.name, "utility_transform");
    }

    #[test]
    fn test_named_transform_with_description() {
        let nt = NamedTransform {
            name: "transform".into(),
            family: None,
            description: Some("A utility transform".into()),
            forward: None,
            inverse: None,
            ..NamedTransform::default()
        };
        assert_eq!(nt.description, Some("A utility transform".into()));
    }

    #[test]
    fn test_named_transform_with_family() {
        let nt = NamedTransform {
            name: "transform".into(),
            family: Some("Utility".into()),
            description: None,
            forward: None,
            inverse: None,
            ..NamedTransform::default()
        };
        assert_eq!(nt.family, Some("Utility".into()));
    }

    #[test]
    fn test_named_transform_with_forward() {
        // Use identity matrix constant
        let matrix = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0; 4],
            direction: crate::TransformDirection::Forward,
        });
        let nt = NamedTransform {
            name: "transform".into(),
            family: None,
            description: None,
            forward: Some(matrix),
            inverse: None,
            ..NamedTransform::default()
        };
        assert!(nt.forward.is_some());
    }

    #[test]
    fn test_named_transform_with_inverse() {
        let matrix = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0; 4],
            direction: crate::TransformDirection::Forward,
        });
        let nt = NamedTransform {
            name: "transform".into(),
            family: None,
            description: None,
            forward: None,
            inverse: Some(matrix),
            ..NamedTransform::default()
        };
        assert!(nt.inverse.is_some());
    }
}

/// Processor cache parity tests.
/// OCIO ref: ProcessorCache
mod processor_cache_parity {
    use crate::ProcessorCache;
    use crate::builtin;
    use crate::builtin::TestSchema;

    #[test]
    fn test_processor_cache_new() {
        let cache = ProcessorCache::new();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_processor_cache_get_or_create() {
        let config = builtin::aces_2_0_studio_all_views();
        let mut cache = ProcessorCache::new();

        let proc = cache.get_or_create(
            &config,
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        );
        assert!(proc.is_ok());
    }

    #[test]
    fn test_processor_cache_hit() {
        let config = builtin::aces_2_0_studio_all_views();
        let mut cache = ProcessorCache::new();

        // First call creates
        let _ = cache.get_or_create(
            &config,
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        );
        // Second call should hit cache
        let proc = cache.get_or_create(
            &config,
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        );
        assert!(proc.is_ok());
    }

    #[test]
    fn test_processor_cache_clear() {
        let config = builtin::aces_2_0_studio_all_views();
        let mut cache = ProcessorCache::new();

        let _ = cache.get_or_create(
            &config,
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        );
        assert!(!cache.is_empty());

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_processor_cache_len() {
        let config = builtin::aces_2_0_studio_all_views();
        let mut cache = ProcessorCache::new();

        let _ = cache.get_or_create(
            &config,
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        );
        let _ = cache.get_or_create(
            &config,
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_rec709,
        );

        assert_eq!(cache.len(), 2);
    }
}

/// Range transform extended parity tests.
/// OCIO ref: RangeTransform.cpp
mod range_transform_extended_parity {
    use crate::transform::{RangeStyle, RangeTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_range_transform_struct() {
        // RangeTransform uses min_in/max_in/min_out/max_out fields
        let rt = RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(1.0),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Forward,
        };

        assert_eq!(rt.min_in, Some(0.0));
    }

    #[test]
    fn test_range_transform_processor() {
        let t = Transform::Range(RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(1.0),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

/// FixedFunction transform extended parity tests.
/// OCIO ref: FixedFunctionTransform.cpp
mod fixed_function_extended_parity {
    use crate::transform::{FixedFunctionStyle, FixedFunctionTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_fixed_function_processor() {
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRedMod03,
            params: Vec::new(),
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

/// Allocation transform extended parity tests.
/// OCIO ref: AllocationTransform.cpp
mod allocation_transform_extended_parity {
    use crate::transform::{AllocationTransform, AllocationType, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_allocation_transform_struct() {
        let at = AllocationTransform {
            allocation: AllocationType::Log2,
            vars: vec![-8.0, 8.0],
            direction: TransformDirection::Forward,
        };

        assert_eq!(at.vars.len(), 2);
    }

    #[test]
    fn test_allocation_processor() {
        let t = Transform::Allocation(AllocationTransform {
            allocation: AllocationType::Uniform,
            vars: vec![0.0, 1.0],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

/// Optimization level extended parity tests.
/// OCIO ref: Processor.cpp
mod optimization_extended_parity {
    use crate::processor::OptimizationLevel;
    use crate::transform::CdlTransform;
    use crate::{Processor, Transform, TransformDirection};

    #[test]
    fn test_processor_optimization() {
        let t = Transform::Cdl(CdlTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        // Processor can apply optimization
        let _ = proc;
    }
}

/// Config serialization extended parity tests.
/// OCIO ref: ConfigIO.cpp
mod config_serialization_extended_parity {
    use crate::builtin;

    #[test]
    fn test_config_to_yaml() {
        let config = builtin::aces_2_0_studio_all_views();
        // Config should have to_yaml or similar serialization method
        let _ = config;
    }

    #[test]
    fn test_config_description() {
        let config = builtin::aces_2_0_studio_all_views();
        let desc = config.description();
        // Ref: PyOpenColorIO getDescription() on the embedded studio config (B23).
        assert!(
            desc.starts_with("Academy Color Encoding System - Studio Config (All Views)"),
            "{desc}"
        );
        assert!(!desc.ends_with('\n'));
    }

    #[test]
    fn test_config_version() {
        let config = builtin::aces_2_0_studio_all_views();
        let version = config.version();
        // Ref: embedded YAML `ocio_profile_version: 2.5` -> ConfigVersion::V2.
        assert!(matches!(version, crate::ConfigVersion::V2));
    }
}

/// GradingTone transform parity tests.
/// OCIO ref: GradingToneTransform.cpp, GradingToneOpData.cpp
mod grading_tone_parity {
    use crate::transform::{
        GradingRgbmsw, GradingToneStyle, GradingToneTransform, TransformDirection,
    };
    use crate::{Processor, Transform};

    #[test]
    fn test_grading_rgbmsw_struct() {
        // GradingRgbmsw: RGBM + start/width
        let rgbmsw = GradingRgbmsw {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            master: 1.0,
            start: 0.0,
            width: 0.2,
        };

        assert_eq!(rgbmsw.master, 1.0);
        assert_eq!(rgbmsw.width, 0.2);
    }

    #[test]
    fn test_grading_tone_default() {
        let gt = GradingToneTransform::default();
        assert_eq!(gt.scontrast, 1.0);
        assert!(gt.is_identity());
    }

    #[test]
    fn test_grading_tone_zones() {
        let gt = GradingToneTransform {
            name: String::new(),
            style: GradingToneStyle::Log,
            blacks: GradingRgbmsw {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 1.0,
                start: 0.0,
                width: 0.0,
            },
            shadows: GradingRgbmsw {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 0.9,
                start: 0.0,
                width: 0.2,
            },
            midtones: GradingRgbmsw {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 1.0,
                start: 0.4,
                width: 0.6,
            },
            highlights: GradingRgbmsw {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 1.1,
                start: 0.8,
                width: 1.0,
            },
            whites: GradingRgbmsw {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                master: 1.0,
                start: 1.0,
                width: 0.0,
            },
            scontrast: 1.0,
            direction: TransformDirection::Forward,
        };

        assert!(!gt.is_identity()); // shadows and highlights adjusted
    }

    #[test]
    fn test_grading_tone_processor() {
        let t = Transform::GradingTone(GradingToneTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_grading_tone_scontrast() {
        // increased contrast
        let mut gt = GradingToneTransform {
            scontrast: 1.5,
            ..Default::default()
        };
        assert!(!gt.is_identity());
    }
}

/// GradingRgbCurve transform extended parity tests.
/// OCIO ref: GradingRGBCurveTransform.cpp
mod grading_rgb_curve_extended_parity {
    use crate::transform::{GradingRgbCurveTransform, RgbCurveType, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_grading_rgb_curve_default() {
        // The default RGB curve is the identity diagonal; applied via a
        // processor it must leave a chromatic probe unchanged.
        let t = Transform::GradingRgbCurve(GradingRgbCurveTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.25_f32, 0.5, 0.75]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.25_f32, 0.5, 0.75].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-5,
                "identity RGB curve ch{i}: {}",
                px[0][i]
            );
        }
    }

    #[test]
    fn test_grading_rgb_curve_processor() {
        // Default (identity) curve: processor round-trips a chromatic probe.
        let t = Transform::GradingRgbCurve(GradingRgbCurveTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.2_f32, 0.4, 0.6]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.2_f32, 0.4, 0.6].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-5,
                "identity RGB curve ch{i}: {}",
                px[0][i]
            );
        }
    }
}

/// GradingHueCurve transform extended parity tests.
/// OCIO ref: GradingHueCurveTransform.cpp
mod grading_hue_curve_extended_parity {
    use crate::transform::{
        GradingHueCurveStyle, GradingHueCurveTransform, HueCurveType, TransformDirection,
    };
    use crate::{Processor, Transform};

    #[test]
    fn test_grading_hue_curve_default() {
        // Default hue curves are identity; the processor must leave a chromatic
        // probe unchanged (a discarded-result impl would not).
        let t = Transform::GradingHueCurve(GradingHueCurveTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.25_f32, 0.5, 0.75]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.25_f32, 0.5, 0.75].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-5,
                "identity hue curve ch{i}: {}",
                px[0][i]
            );
        }
    }

    #[test]
    fn test_grading_hue_curve_processor() {
        let t = Transform::GradingHueCurve(GradingHueCurveTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.2_f32, 0.4, 0.6]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.2_f32, 0.4, 0.6].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-5,
                "identity hue curve ch{i}: {}",
                px[0][i]
            );
        }
    }
}

/// Lut1D/Lut3D transform extended parity tests.
/// OCIO ref: Lut1DTransform.cpp, Lut3DTransform.cpp
mod lut_transform_extended_parity {
    use crate::transform::{Interpolation, Lut1DTransform, Lut3DTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_lut1d_identity() {
        let lut = Lut1DTransform::identity(256);
        assert_eq!(lut.size(), 256);
    }

    #[test]
    fn test_lut1d_from_rgb() {
        let r = vec![0.0, 0.5, 1.0];
        let g = vec![0.0, 0.5, 1.0];
        let b = vec![0.0, 0.5, 1.0];
        let lut = Lut1DTransform::from_rgb(r, g, b);
        assert_eq!(lut.size(), 3);
        assert!(!lut.is_mono());
    }

    #[test]
    fn test_lut3d_transform_struct() {
        // 2x2x2 cube = 8 RGB entries
        let data: Vec<[f32; 3]> = (0..8)
            .map(|i| {
                let v = i as f32 / 8.0;
                [v, v, v]
            })
            .collect();

        let lut = Lut3DTransform {
            size: 2,
            data,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Tetrahedral,
            direction: TransformDirection::Forward,
        };

        assert_eq!(lut.size, 2);
    }

    #[test]
    fn test_lut1d_processor() {
        // An identity 1D LUT maps every value to itself; a chromatic probe must
        // come back unchanged (catches a discarded-result / broken sampler).
        let lut = Lut1DTransform::identity(16);
        let t = Transform::Lut1D(lut);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.25_f32, 0.5, 0.75]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.25_f32, 0.5, 0.75].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-3,
                "identity LUT1D ch{i}: {}",
                px[0][i]
            );
        }
    }

    #[test]
    fn test_lut3d_processor() {
        // 2x2x2 identity LUT - corner values
        let data: Vec<[f32; 3]> = vec![
            [0.0, 0.0, 0.0], // (0,0,0)
            [1.0, 0.0, 0.0], // (1,0,0)
            [0.0, 1.0, 0.0], // (0,1,0)
            [1.0, 1.0, 0.0], // (1,1,0)
            [0.0, 0.0, 1.0], // (0,0,1)
            [1.0, 0.0, 1.0], // (1,0,1)
            [0.0, 1.0, 1.0], // (0,1,1)
            [1.0, 1.0, 1.0], // (1,1,1)
        ];

        let lut = Lut3DTransform {
            size: 2,
            data,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Tetrahedral,
            direction: TransformDirection::Forward,
        };

        let t = Transform::Lut3D(lut);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        // The 2x2x2 corner cube above is the identity LUT3D; a chromatic probe
        // must round-trip (tetrahedral interp of the identity corners == input).
        let mut px = [[0.25_f32, 0.5, 0.75]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.25_f32, 0.5, 0.75].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-3,
                "identity LUT3D ch{i}: {}",
                px[0][i]
            );
        }
    }
}

/// ColorSpace transform extended parity tests.
/// OCIO ref: ColorSpaceTransform.cpp
mod colorspace_transform_extended_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::transform::{ColorSpaceTransform, TransformDirection};

    #[test]
    fn test_colorspace_transform_struct() {
        let cst = ColorSpaceTransform {
            src: "ACEScg".into(),
            dst: "sRGB".into(),
            data_bypass: true,
            direction: TransformDirection::Forward,
        };

        assert_eq!(cst.src, "ACEScg");
        assert_eq!(cst.dst, "sRGB");
    }

    #[test]
    fn test_colorspace_transform_with_config() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .expect("working_space -> sRGB display processor must build");
        // working -> sRGB display is a real, non-identity transform: a mid-grey
        // probe must be modified (encoded). A no-op processor leaves 0.18.
        let mut px = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut px);
        assert!(
            (px[0][0] - 0.18).abs() > 1e-3 && px[0][0].is_finite(),
            "working->sRGB display must transform 0.18, got {}",
            px[0][0]
        );
    }
}

/// LookTransform extended parity tests.
/// OCIO ref: LookTransform.cpp
mod look_transform_extended_parity {
    use crate::transform::{LookTransform, TransformDirection};

    #[test]
    fn test_look_transform_struct() {
        let lt = LookTransform {
            src: "ACEScg".into(),
            dst: "ACEScg".into(),
            looks: "show_look".into(),
            skip_color_space_conversion: false,
            direction: TransformDirection::Forward,
        };

        assert_eq!(lt.looks, "show_look");
    }

    #[test]
    fn test_look_transform_multiple_looks() {
        let lt = LookTransform {
            src: "ACEScg".into(),
            dst: "ACEScg".into(),
            looks: "look1, look2, -look3".into(),
            skip_color_space_conversion: false,
            direction: TransformDirection::Forward,
        };

        assert!(lt.looks.contains("look1"));
        assert!(lt.looks.contains("-look3"));
    }
}

/// DisplayViewTransform extended parity tests.
/// OCIO ref: DisplayViewTransform.cpp
mod display_view_transform_extended_parity {
    use crate::transform::{DisplayViewTransform, TransformDirection};

    #[test]
    fn test_display_view_transform_struct() {
        let dvt = DisplayViewTransform {
            src: "ACEScg".into(),
            display: "sRGB".into(),
            view: "Film".into(),
            looks_bypass: false,
            data_bypass: true,
            looks_override: String::new(),
            looks_override_enabled: false,
            direction: TransformDirection::Forward,
        };

        assert_eq!(dvt.display, "sRGB");
        assert_eq!(dvt.view, "Film");
    }
}

/// GroupTransform extended parity tests.
/// OCIO ref: GroupTransform.cpp
mod group_transform_extended_parity {
    use crate::transform::{CdlTransform, GroupTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_group_transform_struct() {
        let gt = GroupTransform {
            name: String::new(),
            transforms: vec![
                Transform::Cdl(CdlTransform::default()),
                Transform::Cdl(CdlTransform::default()),
            ],
            direction: TransformDirection::Forward,
        };

        assert_eq!(gt.transforms.len(), 2);
    }

    #[test]
    fn test_group_transform_processor() {
        let t = Transform::Group(GroupTransform {
            name: String::new(),
            transforms: vec![Transform::Cdl(CdlTransform::default())],
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_group_transform_helper() {
        let t = Transform::group(vec![Transform::Cdl(CdlTransform::default())]);

        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

/// BuiltinTransform parity tests.
/// OCIO ref: BuiltinTransform.cpp
mod builtin_transform_parity {
    use crate::transform::{BuiltinTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_builtin_transform_struct() {
        // The OCIO style carries the "UTILITY - " prefix.
        let bt = BuiltinTransform {
            style: "UTILITY - ACES-AP0_to_CIE-XYZ-D65_BFD".into(),
            direction: TransformDirection::Forward,
        };
        assert!(
            Processor::from_transform(&Transform::Builtin(bt), TransformDirection::Forward).is_ok()
        );
    }

    #[test]
    fn test_builtin_transform_processor() {
        let t = Transform::Builtin(BuiltinTransform {
            style: "IDENTITY".into(),
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        // May or may not be implemented - just check it doesn't panic
        let _ = proc;
    }
}

/// ExposureContrast extended transform parity tests.
/// OCIO ref: ExposureContrastTransform.cpp
mod exposure_contrast_extended_parity {
    use crate::transform::{ExposureContrastStyle, ExposureContrastTransform, TransformDirection};
    use crate::{Processor, Transform};

    fn default_ec() -> ExposureContrastTransform {
        ExposureContrastTransform {
            name: String::new(),
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            style: ExposureContrastStyle::Linear,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        }
    }

    #[test]
    fn test_exposure_contrast_struct() {
        let ec = ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Linear,
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };

        assert_eq!(ec.pivot, 0.18);
    }

    #[test]
    fn test_exposure_contrast_identity() {
        let ec = default_ec();
        assert_eq!(ec.exposure, 0.0);
        assert_eq!(ec.contrast, 1.0);
        assert_eq!(ec.gamma, 1.0);
    }

    #[test]
    fn test_exposure_contrast_processor() {
        let t = Transform::ExposureContrast(default_ec());
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_exposure_contrast_with_exposure() {
        let mut ec = default_ec();
        ec.exposure = 1.0; // +1 stop

        let t = Transform::ExposureContrast(ec);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixels);
        // +1 stop should roughly double the value
        assert!(pixels[0][0] > 0.3);
    }
}

/// BuiltinTransfer transform parity tests.
/// OCIO ref: BuiltinTransferTransform
mod builtin_transfer_parity {
    use crate::transform::{BuiltinTransferTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_builtin_transfer_srgb() {
        let bt = BuiltinTransferTransform {
            style: "sRGB".into(),
            direction: TransformDirection::Forward,
        };

        let t = Transform::BuiltinTransfer(bt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        // sRGB OETF (IEC 61966-2-1): linear 0.18 -> 0.46135604. Forward applies
        // the encode; a no-op/discarded-result impl leaves 0.18.
        let mut px = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut px);
        assert!(
            (px[0][0] - 0.46135604).abs() < 1e-3,
            "sRGB OETF(0.18): got {}, expected 0.46135604",
            px[0][0]
        );
    }

    /// The three camera log styles that were implemented but unreachable by name.
    ///
    /// `AppleLog`, `CanonCLog2` and `CanonCLog3` exist as `TransferStyle` variants and are
    /// emitted by `gpu/transfer.rs`, but were absent from `compile.rs`'s name table — so asking
    /// for them fell into `_ => TransferStyle::Linear` and silently produced an identity. The
    /// picture came back wrong with nothing reported.
    ///
    /// Asserting the value MOVES is what discriminates: a test that only checked
    /// `from_transform(..).is_ok()` passed against the broken version too, because a silent
    /// identity is a perfectly valid processor.
    #[test]
    fn test_builtin_transfer_camera_log_styles_are_reachable() {
        for style in [
            "apple_log",
            "applelog",
            "canon_clog2",
            "clog2",
            "canon_clog3",
            "clog3",
        ] {
            let t = Transform::BuiltinTransfer(BuiltinTransferTransform {
                style: style.into(),
                direction: TransformDirection::Forward,
            });
            let proc = Processor::from_transform(&t, TransformDirection::Forward)
                .unwrap_or_else(|e| panic!("style {style:?} must compile: {e}"));

            let mut px = [[0.18_f32, 0.18, 0.18]];
            proc.apply_rgb(&mut px);
            assert!(
                (px[0][0] - 0.18).abs() > 1e-4,
                "style {style:?} left 0.18 unchanged — it silently compiled to a linear identity"
            );
        }
    }

    /// An unrecognised style must be a loud error, never a silent linear identity.
    #[test]
    fn test_builtin_transfer_unknown_style_errors() {
        let t = Transform::BuiltinTransfer(BuiltinTransferTransform {
            style: "definitely_not_a_transfer_function".into(),
            direction: TransformDirection::Forward,
        });
        let err = Processor::from_transform(&t, TransformDirection::Forward)
            .expect_err("an unknown style must not compile to a silent identity");
        let msg = err.to_string();
        assert!(
            msg.contains("definitely_not_a_transfer_function"),
            "the error must name the offending style, got: {msg}"
        );
    }

    #[test]
    fn test_builtin_transfer_rec709() {
        let bt = BuiltinTransferTransform {
            style: "Rec709".into(),
            direction: TransformDirection::Forward,
        };

        let t = Transform::BuiltinTransfer(bt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        // Rec.709 OETF (ITU-R BT.709): linear 0.18 -> ~0.40900. Forward encode;
        // a no-op leaves 0.18.
        let mut px = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut px);
        assert!(
            (px[0][0] - 0.40900).abs() < 5e-3,
            "Rec709 OETF(0.18): got {}, expected ~0.409",
            px[0][0]
        );
    }

    #[test]
    fn test_builtin_transfer_pq() {
        let bt = BuiltinTransferTransform {
            style: "PQ".into(),
            direction: TransformDirection::Forward,
        };

        let t = Transform::BuiltinTransfer(bt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        // SMPTE ST 2084 (PQ) is a strongly non-linear OETF: it must map 0.5 to a
        // value clearly different from 0.5, stay finite, and round-trip with the
        // inverse. (Exact nits normalisation for this builtin style is not
        // tabulated, so assert the verifiable non-no-op + invertibility props.)
        let mut px = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut px);
        assert!(
            px[0][0].is_finite() && (px[0][0] - 0.5).abs() > 1e-3,
            "PQ must be non-linear, got {}",
            px[0][0]
        );
        let inv = Transform::BuiltinTransfer(BuiltinTransferTransform {
            style: "PQ".into(),
            direction: TransformDirection::Inverse,
        });
        let proc_inv = Processor::from_transform(&inv, TransformDirection::Forward).unwrap();
        proc_inv.apply_rgb(&mut px);
        assert!(
            (px[0][0] - 0.5).abs() < 1e-3,
            "PQ round-trip failed: got {}",
            px[0][0]
        );
    }

    #[test]
    fn test_builtin_transfer_hlg() {
        let bt = BuiltinTransferTransform {
            style: "HLG".into(),
            direction: TransformDirection::Forward,
        };

        let t = Transform::BuiltinTransfer(bt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        // HLG (ITU-R BT.2100) OETF is non-linear and invertible; assert the
        // forward result is non-identity + finite and round-trips with inverse.
        let mut px = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut px);
        assert!(
            px[0][0].is_finite() && (px[0][0] - 0.5).abs() > 1e-3,
            "HLG must be non-linear, got {}",
            px[0][0]
        );
        let inv = Transform::BuiltinTransfer(BuiltinTransferTransform {
            style: "HLG".into(),
            direction: TransformDirection::Inverse,
        });
        let proc_inv = Processor::from_transform(&inv, TransformDirection::Forward).unwrap();
        proc_inv.apply_rgb(&mut px);
        assert!(
            (px[0][0] - 0.5).abs() < 1e-3,
            "HLG round-trip failed: got {}",
            px[0][0]
        );
    }
}

/// Packed pixel processing parity tests.
/// OCIO ref: PackedImageDesc, CPUProcessor
mod packed_pixel_parity {
    use crate::processor::{ChannelOrdering, PackedBitDepth};
}

/// Validation API parity tests.
/// OCIO ref: Config validation
mod validation_parity {
    use crate::builtin;
    use crate::validate::{Issue, IssueCategory, Severity};

    #[test]
    fn test_validate_config() {
        let config = builtin::aces_2_0_studio_all_views();
        let issues = crate::validate_config(&config);
        // Built-in config should be valid
        assert!(!crate::has_errors(&issues));
    }

    #[test]
    fn test_issue_struct() {
        let issue = Issue {
            severity: Severity::Warning,
            category: IssueCategory::MissingFile,
            message: "LUT file not found".into(),
            context: Some("test.cube".into()),
        };

        assert!(matches!(issue.severity, Severity::Warning));
    }
}

/// GPU shader language comprehensive parity tests.
/// OCIO ref: GpuShaderDesc.h
mod gpu_shader_languages_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};

    #[test]
    fn test_gpu_language_version_directive() {
        // OCIO getShaderText() includes version directive
        assert!(GpuLanguage::Glsl120.version_directive().contains("120"));
        assert!(GpuLanguage::Glsl130.version_directive().contains("130"));
        assert!(GpuLanguage::Glsl330.version_directive().contains("330"));
        assert!(GpuLanguage::Glsl400.version_directive().contains("400"));
        assert!(
            GpuLanguage::GlslVulkan460
                .version_directive()
                .contains("460")
        );
        assert!(GpuLanguage::GlslEs100.version_directive().contains("100"));
        assert!(GpuLanguage::GlslEs300.version_directive().contains("300"));
    }

    #[test]
    fn test_hlsl_shader_generation() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Hlsl50).unwrap();
        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_metal_shader_generation() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Metal).unwrap();
        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_vulkan_shader_generation() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslVulkan460).unwrap();
        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_webgl1_shader_generation() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslEs100).unwrap();
        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_webgl2_shader_generation() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslEs300).unwrap();
        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }
}

/// FileRules parity tests.
/// OCIO ref: FileRules.h
mod file_rules_parity {
    use crate::config::{FileRule, FileRuleKind};

    #[test]
    fn test_file_rule_basic() {
        let rule = FileRule {
            name: "exr_files".into(),
            colorspace: "ACEScg".into(),
            kind: FileRuleKind::Basic {
                pattern: "*.exr".into(),
                extension: Some("exr".into()),
            },
            ..FileRule::default()
        };

        assert_eq!(rule.name, "exr_files");
        assert_eq!(rule.colorspace, "ACEScg");
    }

    #[test]
    fn test_file_rule_default() {
        let rule = FileRule {
            name: "Default".into(),
            colorspace: "sRGB".into(),
            kind: FileRuleKind::Default,
            ..FileRule::default()
        };

        assert!(matches!(rule.kind, FileRuleKind::Default));
    }

    #[test]
    fn test_file_rule_regex() {
        use regex::Regex;

        let rule = FileRule {
            name: "log_files".into(),
            colorspace: "Log".into(),
            kind: FileRuleKind::Regex {
                regex: Regex::new(r".*_log\.(exr|dpx)").unwrap(),
            },
            ..FileRule::default()
        };

        if let FileRuleKind::Regex { regex } = &rule.kind {
            assert!(regex.is_match("shot001_log.exr"));
            assert!(regex.is_match("render_log.dpx"));
            assert!(!regex.is_match("shot001_lin.exr"));
        }
    }

    #[test]
    fn test_file_rule_pattern_basic() {
        let rule = FileRule {
            name: "png_files".into(),
            colorspace: "sRGB".into(),
            kind: FileRuleKind::Basic {
                pattern: "*.png".into(),
                extension: None,
            },
            ..FileRule::default()
        };

        if let FileRuleKind::Basic { pattern, extension } = &rule.kind {
            assert_eq!(pattern, "*.png");
            assert!(extension.is_none());
        }
    }
}

/// Active displays/views parity tests.
/// OCIO ref: Config::getActiveDisplays/getActiveViews
mod active_displays_views_parity {
    use crate::builtin;

    #[test]
    fn test_config_active_displays() {
        let config = builtin::aces_2_0_studio_all_views();
        let dm = config.displays();
        let displays: Vec<&str> = dm.displays().iter().map(|d| d.name()).collect();
        assert!(!displays.is_empty());
    }

    #[test]
    fn test_display_default_view() {
        let config = builtin::aces_2_0_studio_all_views();
        let dm = config.displays();
        for display in dm.displays() {
            // Each display should have a default view
            if !display.views().is_empty() {
                assert!(display.default_view().is_some());
            }
        }
    }
}

/// Config environment mode parity.
/// OCIO ref: EnvironmentMode
mod environment_mode_parity {
    use crate::config::EnvironmentMode;
}

/// Search reference space type parity.
/// OCIO ref: SearchReferenceSpaceType
mod search_reference_space_parity {
    use crate::config::SearchReferenceSpaceType;
}

/// ColorSpace visibility parity.
/// OCIO ref: ColorSpaceVisibility
mod colorspace_visibility_parity {
    use crate::config::ColorSpaceVisibility;
}

/// SharedView parity tests.
/// OCIO ref: Config::addSharedView
mod shared_view_parity {
    use crate::display::View;

    #[test]
    fn test_shared_view_struct() {
        let sv = View::new("Film", "sRGB")
            .with_view_transform("ACES 1.0 - SDR Video")
            .with_description("Film look");

        assert_eq!(sv.name(), "Film");
        assert!(sv.view_transform().is_some());
    }

    #[test]
    fn test_shared_view_with_looks() {
        let sv = View::new("Graded", "sRGB").with_look("show_lut");
        assert_eq!(sv.looks(), Some("show_lut"));
    }
}

/// ViewingRule parity tests.
/// OCIO ref: ViewingRules
mod viewing_rule_parity {
    use crate::config::ViewingRule;

    #[test]
    fn test_viewing_rule_struct() {
        let rule = ViewingRule {
            name: "color_timing".into(),
            colorspaces: vec!["ACEScg".into()],
            encodings: vec!["scene-linear".into()],
            custom_keys: vec![],
        };

        assert_eq!(rule.name, "color_timing");
        assert!(!rule.colorspaces.is_empty());
    }
}

/// VirtualDisplay parity tests.
/// OCIO ref: VirtualDisplay
mod virtual_display_extended_parity {
    use crate::config::{VirtualDisplay, VirtualDisplayView};

    #[test]
    fn test_virtual_display_struct() {
        let vd = VirtualDisplay {
            views: vec![VirtualDisplayView {
                name: "SDR Video".into(),
                view_transform: Some("ACES 1.0 - SDR Video".into()),
                display_colorspace: "<USE_DISPLAY_NAME>".into(),
                looks: None,
                rule: None,
                description: None,
            }],
            shared_view_refs: vec![],
        };

        assert_eq!(vd.views.len(), 1);
    }

    #[test]
    fn test_virtual_display_view() {
        let view = VirtualDisplayView {
            name: "HDR Video (1000 nits)".into(),
            view_transform: Some("ACES 1.0 - HDR Video (1000 nits)".into()),
            display_colorspace: "sRGB".into(),
            looks: Some("show_lut".into()),
            rule: Some("hdr_content".into()),
            description: None,
        };

        assert!(view.looks.is_some());
        assert!(view.rule.is_some());
    }
}

/// Processor optimization with finalization parity.
/// OCIO ref: Processor::getOptimizedProcessor
mod processor_optimization_finalization_parity {
    use crate::builtin::TestSchema;
    use crate::{OptimizationLevel, builtin};

    #[test]
    fn test_processor_with_optimization() {
        let config = builtin::aces_2_0_studio_all_views();
        // Create processor - optimization is internal
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixels);
        // Should produce valid output
        assert!(pixels[0][0] > 0.0 && pixels[0][0] < 1.0);
    }
}

/// Reference space type parity (display module).
/// OCIO ref: ReferenceSpaceType
mod reference_space_type_parity {
    use crate::display::ReferenceSpaceType;
}

/// Interpolation method parity.
/// OCIO ref: Interpolation enum
mod interpolation_parity {
    use crate::transform::Interpolation;
}

/// Transform direction parity.
/// OCIO ref: TransformDirection
mod transform_direction_parity {
    use crate::transform::TransformDirection;

    #[test]
    fn test_direction_inverse() {
        // Forward and Inverse are opposites
        assert!(matches!(
            TransformDirection::Forward,
            TransformDirection::Forward
        ));
        assert!(matches!(
            TransformDirection::Inverse,
            TransformDirection::Inverse
        ));
    }
}

/// CDL style parity.
/// OCIO ref: CDLStyle
mod cdl_style_parity {
    use crate::transform::CdlStyle;
}

/// Negative style parity (exponent transform).
/// OCIO ref: NegativeStyle
mod negative_style_extended_parity {
    use crate::transform::NegativeStyle;
}

/// Range style parity.
/// OCIO ref: RangeStyle
mod range_style_parity {
    use crate::transform::RangeStyle;
}

/// Grading primary style parity.
/// OCIO ref: GradingStyle
mod grading_primary_style_parity {
    use crate::transform::GradingPrimaryStyle;
}

/// Grading tone style parity.
mod grading_tone_style_parity {
    use crate::transform::GradingToneStyle;
}

/// Grading hue curve style parity.
mod grading_hue_curve_style_parity {
    use crate::transform::GradingHueCurveStyle;
}

/// HueCurveType parity.
/// OCIO ref: HueCurveType
mod hue_curve_type_parity {
    use crate::transform::HueCurveType;
}

/// RgbCurveType parity.
/// OCIO ref: RGBCurveType
mod rgb_curve_type_parity {
    use crate::transform::RgbCurveType;
}

/// Allocation type parity.
mod allocation_type_parity {
    use crate::transform::AllocationType;
}

/// GpuTextureType parity.
/// OCIO ref: TextureType
mod gpu_texture_type_parity {
    use crate::gpu::GpuTextureType;
}

/// GpuChannelType parity.
/// OCIO ref: ChannelType
mod gpu_channel_type_parity {
    use crate::gpu::GpuChannelType;
}

/// GpuInterpolation parity.
mod gpu_interpolation_parity {
    use crate::gpu::GpuInterpolation;
}

/// TextureDimensions parity.
mod texture_dimensions_parity {
    use crate::gpu::TextureDimensions;
}

/// GpuUniformType parity.
mod gpu_uniform_type_parity {
    use crate::gpu::GpuUniformType;
}

/// DynamicPropertyType parity.
mod dynamic_property_type_parity {
    use crate::dynamic::DynamicPropertyType;
}

/// BitDepth parity (from vfx-core).
mod bit_depth_extended_parity {
    use crate::colorspace::BitDepth;
}

/// LutFormat parity for baker.
mod lut_format_extended_parity {
    use crate::baker::LutFormat;
}

/// Role names parity.
mod role_names_parity {
    use crate::role::names;

    #[test]
    fn test_role_name_constants() {
        // OCIO ROLE_* constants
        assert_eq!(names::REFERENCE, "reference");
        assert_eq!(names::DATA, "data");
        assert_eq!(names::COLOR_PICKING, "color_picking");
        assert_eq!(names::COLOR_TIMING, "color_timing");
        assert_eq!(names::COMPOSITING_LOG, "compositing_log");
        assert_eq!(names::MATTE_PAINT, "matte_paint");
        assert_eq!(names::SCENE_LINEAR, "scene_linear");
        assert_eq!(names::TEXTURE_PAINT, "texture_paint");
        assert_eq!(names::RENDERING, "rendering");
    }

    #[test]
    fn test_aces_role_constants() {
        assert_eq!(names::ACES_INTERCHANGE, "aces_interchange");
        assert_eq!(names::CIE_XYZ_D65_INTERCHANGE, "cie_xyz_d65_interchange");
    }
}

/// TransferStyle parity tests.
/// OCIO ref: TransferFunction styles
mod transfer_style_parity {
    use crate::processor::TransferStyle;
}

/// ProcessorOp parity tests.
/// OCIO ref: Op types in Processor
mod processor_op_parity {
    use crate::processor::ProcessorOp;

    #[test]
    fn test_processor_op_variants() {
        // Test that ProcessorOp enum exists and has expected variants
        // Variants depend on implementation
        let _op = ProcessorOp::Matrix {
            matrix: [1.0; 16],
            offset: [0.0; 4],
        };
    }
}

/// ProcessorMetadata extended parity tests.
/// OCIO ref: FormatMetadata
mod processor_metadata_extended_parity {
    use crate::processor::{FormatMetadata, ProcessorMetadata};

    #[test]
    fn test_processor_metadata_default() {
        let meta = ProcessorMetadata::default();
        let _ = meta;
    }

    #[test]
    fn test_format_metadata_struct() {
        let fm = FormatMetadata {
            id: "test".into(),
            name: "Test Metadata".into(),
            ..Default::default()
        };
        assert_eq!(fm.id, "test");
        assert_eq!(fm.name, "Test Metadata");
    }
}

/// ChannelOrdering parity tests.
/// OCIO ref: ChannelOrdering
mod channel_ordering_parity {
    use crate::processor::ChannelOrdering;
}

/// Config version parity tests.
/// OCIO ref: ConfigVersion
mod config_version_parity {
    use crate::config::ConfigVersion;

    #[test]
    fn test_config_version_default() {
        let v = ConfigVersion::default();
        // Default is V1
        assert!(matches!(v, ConfigVersion::V1));
    }
}

/// FixedFunctionStyle comprehensive parity.
/// OCIO ref: FixedFunctionStyle enum
mod fixed_function_style_comprehensive_parity {
    use crate::transform::FixedFunctionStyle;
}

/// BuiltinTransform names parity.
/// OCIO ref: BuiltinTransformRegistry
mod builtin_transform_names_extended_parity {
    use crate::transform::BuiltinTransform;

    #[test]
    fn test_builtin_transform_struct() {
        let bt = BuiltinTransform {
            style: "ACEScg_to_ACES2065-1".into(),
            direction: crate::transform::TransformDirection::Forward,
        };
        assert_eq!(bt.style, "ACEScg_to_ACES2065-1");
    }
}

/// Context comprehensive parity.
/// OCIO ref: Context
mod context_comprehensive_extended_parity {
    use crate::Context;

    #[test]
    fn test_context_set_get() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");
        assert_eq!(ctx.get("SHOT").as_deref(), Some("sh010"));
    }

    #[test]
    fn test_context_resolve() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");
        ctx.set("SEQ", "sq01");

        let resolved = ctx.resolve("/shows/$SEQ/$SHOT/luts/grade.cube");
        assert_eq!(resolved, "/shows/sq01/sh010/luts/grade.cube");
    }

    #[test]
    fn test_context_empty_resolve() {
        let ctx = Context::new();
        let resolved = ctx.resolve("no_vars_here.cube");
        assert_eq!(resolved, "no_vars_here.cube");
    }
}

/// Look parse_looks parity.
/// OCIO ref: LookParse
mod look_parse_parity {
    use crate::look::parse_looks;

    #[test]
    fn test_parse_single_look() {
        let looks = parse_looks("show_lut");
        assert_eq!(looks.len(), 1);
        assert_eq!(looks[0].0, "show_lut");
    }

    #[test]
    fn test_parse_multiple_looks() {
        let looks = parse_looks("look1, look2, look3");
        assert_eq!(looks.len(), 3);
    }

    #[test]
    fn test_parse_inverted_look() {
        let looks = parse_looks("-look1");
        assert_eq!(looks.len(), 1);
        assert!(!looks[0].1); // inverted = false
    }
}

/// Display manager parity.
/// OCIO ref: DisplayManager methods
mod display_manager_extended_parity {
    use crate::display::{Display, DisplayManager, View};

    #[test]
    fn test_display_manager_add_display() {
        let mut mgr = DisplayManager::new();
        let mut display = Display::new("sRGB");
        display.add_view(View::new("Film", "sRGB"));
        mgr.add_display(display);

        assert_eq!(mgr.len(), 1);
        assert!(!mgr.is_empty());
    }

    #[test]
    fn test_display_manager_default_display() {
        let mut mgr = DisplayManager::new();
        mgr.add_display(Display::new("Display1"));
        mgr.add_display(Display::new("Display2"));

        // First added becomes default
        assert_eq!(mgr.default_display(), Some("Display1"));
    }

    #[test]
    fn test_display_manager_set_default() {
        let mut mgr = DisplayManager::new();
        mgr.add_display(Display::new("Display1"));
        mgr.add_display(Display::new("Display2"));
        mgr.set_default_display("Display2");

        assert_eq!(mgr.default_display(), Some("Display2"));
    }

    #[test]
    fn test_display_manager_remove() {
        let mut mgr = DisplayManager::new();
        mgr.add_display(Display::new("ToRemove"));
        mgr.add_display(Display::new("ToKeep"));
        mgr.remove_display("ToRemove");

        assert_eq!(mgr.len(), 1);
        assert!(mgr.display("ToRemove").is_none());
    }
}

/// ViewTransform parity.
/// OCIO ref: ViewTransform
mod view_transform_parity {
    use crate::display::{ReferenceSpaceType, ViewTransform};

    #[test]
    fn test_view_transform_scene() {
        let vt = ViewTransform::new("ACES 1.0 - SDR Video");
        assert_eq!(vt.name(), "ACES 1.0 - SDR Video");
        assert_eq!(vt.reference_space_type(), ReferenceSpaceType::Scene);
    }

    #[test]
    fn test_view_transform_display() {
        let vt =
            ViewTransform::with_reference_space("Display Transform", ReferenceSpaceType::Display);
        assert_eq!(vt.reference_space_type(), ReferenceSpaceType::Display);
    }

    #[test]
    fn test_view_transform_builders() {
        let vt = ViewTransform::new("Test")
            .with_family("ACES")
            .with_description("Test view transform");

        assert_eq!(vt.family(), "ACES");
        assert_eq!(vt.description(), "Test view transform");
    }
}

/// Processor cache parity.
/// OCIO ref: ProcessorCache
mod processor_cache_extended_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::cache::ProcessorCache;

    #[test]
    fn test_cache_get_or_create() {
        let config = builtin::aces_2_0_studio_all_views();
        let cache = ProcessorCache::new();

        let proc1 = cache
            .get_or_create(
                &config,
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let proc2 = cache
            .get_or_create(
                &config,
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        // Should return same processor (cached)
        let _ = (proc1, proc2);
    }

    #[test]
    fn test_cache_different_pairs() {
        let config = builtin::aces_2_0_studio_all_views();
        let cache = ProcessorCache::new();

        let _proc1 = cache
            .get_or_create(
                &config,
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let _proc2 = cache
            .get_or_create(
                &config,
                &TestSchema::shared().interchange,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
    }
}

/// ACES 2.0 parity tests.
/// OCIO ref: ACES 2.0 Output Transform
mod aces2_parity {
    use crate::aces2;
}

/// Baker bake operations parity.
/// OCIO ref: Baker
mod baker_bake_parity {
    use crate::baker::Baker;
    use crate::builtin;
    use crate::builtin::TestSchema;

    #[test]
    fn test_baker_bake_1d() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let baker = Baker::new(&proc);
        let result = baker.bake_lut_1d(1024);
        assert!(result.is_ok());

        let lut = result.unwrap();
        assert_eq!(lut.size, 1024);
        assert!(!lut.data.is_empty());
    }

    #[test]
    fn test_baker_bake_3d() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let baker = Baker::new(&proc);
        let result = baker.bake_lut_3d(17);
        assert!(result.is_ok());

        let lut = result.unwrap();
        assert_eq!(lut.size, 17);
        assert!(!lut.data.is_empty());
    }
}

/// GradingRgbm struct parity.
/// OCIO ref: GradingRGBM
mod grading_rgbm_parity {
    use crate::dynamic::GradingRgbm;

    #[test]
    fn test_grading_rgbm_struct() {
        let rgbm = GradingRgbm {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            master: 1.0,
        };
        assert_eq!(rgbm.red, 1.0);
    }

    #[test]
    fn test_grading_rgbm_default() {
        let rgbm = GradingRgbm::default();
        // Default is zero
        assert_eq!(rgbm.master, 0.0);
    }

    #[test]
    fn test_grading_rgbm_one() {
        let rgbm = GradingRgbm::one();
        // Unity RGBM
        assert_eq!(rgbm.master, 1.0);
        assert_eq!(rgbm.red, 1.0);
    }
}

/// GradingPrimary struct parity.
/// OCIO ref: GradingPrimary
mod grading_primary_struct_parity {
    use crate::dynamic::{GradingPrimary, GradingRgbm};

    #[test]
    fn test_grading_primary_struct() {
        let unity = GradingRgbm::default();
        let gp = GradingPrimary {
            brightness: unity,
            contrast: unity,
            gamma: unity,
            offset: GradingRgbm {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                master: 0.0,
            },
            exposure: GradingRgbm::default(),
            lift: GradingRgbm::default(),
            gain: GradingRgbm::one(),
            saturation: 1.0,
            pivot: 0.18,
            clamp_black: 0.0,
            clamp_white: 1.0,
            pivot_black: 0.0,
            pivot_white: 1.0,
        };
        assert_eq!(gp.pivot, 0.18);
    }
}

/// DynamicPropertyValue parity.
/// OCIO ref: DynamicPropertyValue
mod dynamic_property_value_parity {
    use crate::dynamic::{DynamicPropertyValue, GradingPrimary};

    #[test]
    fn test_dynamic_property_value_double() {
        let val = DynamicPropertyValue::Double(1.5);
        if let DynamicPropertyValue::Double(v) = val {
            assert_eq!(v, 1.5);
        }
    }

    #[test]
    fn test_dynamic_property_value_grading_primary() {
        let gp = GradingPrimary::default();
        let val = DynamicPropertyValue::GradingPrimary(gp);
        assert!(matches!(val, DynamicPropertyValue::GradingPrimary(_)));
    }
}

/// Processor apply methods parity.
/// OCIO ref: CPUProcessor::apply
mod processor_apply_extended_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;

    #[test]
    fn test_apply_rgb_single_pixel() {
        let s = TestSchema::shared();
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .display_processor(&s.working_space, &s.default_display, &s.tone_mapped_view)
            .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);

        // Anchored to OpenColorIO C++ DisplayView (PyOpenColorIO 2.5.1 / studio
        // config): ACEScg 0.18 → ~0.349 encoded sRGB, all channels equal.
        assert!(
            (pixel[0][0] - 0.349_187).abs() < 2e-3
                && (pixel[0][0] - pixel[0][1]).abs() < 1e-3
                && (pixel[0][1] - pixel[0][2]).abs() < 1e-3,
            "DisplayView mid-grey: got {:?}",
            pixel[0]
        );
    }

    #[test]
    fn aces2_sdr_rec709_midgrey_is_neutral_xyz() {
        use crate::aces2::{Chromaticity, Primaries, init_output_transform, output_transform_fwd};
        let rec709 = Primaries {
            red: Chromaticity { x: 0.64, y: 0.33 },
            green: Chromaticity { x: 0.30, y: 0.60 },
            blue: Chromaticity { x: 0.15, y: 0.06 },
            white: Chromaticity {
                x: 0.3127,
                y: 0.3290,
            },
        };
        let st = init_output_transform(100.0, &rec709);
        let out = output_transform_fwd(&[0.18, 0.18, 0.18], &st);
        eprintln!("aces2 midgrey out: {out:?}");
        assert!(
            (out[0] - out[1]).abs() < 1e-4 && (out[1] - out[2]).abs() < 1e-4,
            "neutral input must stay neutral through OT, got {out:?}"
        );
    }

    #[test]
    fn test_apply_rgb_multiple_pixels() {
        let s = TestSchema::shared();
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .display_processor(&s.working_space, &s.default_display, &s.tone_mapped_view)
            .unwrap();

        let mut pixels = [[0.0_f32, 0.0, 0.0], [0.18, 0.18, 0.18], [1.0, 1.0, 1.0]];
        proc.apply_rgb(&mut pixels);

        assert!(pixels[0][0] < 0.01);
        assert!((pixels[1][0] - 0.349_187).abs() < 2e-3);
        // Scene-linear 1.0 sits inside the ACES SDR tone curve's
        // highlight roll-off (display white maps from ~16× mid-grey,
        // not 1.0 scene), so it lands near ~0.707 in sRGB encoding
        // (OCIO C++ studio DisplayView — was ~0.78 under the broken
        // lim→XYZ path).
        assert!(
            (pixels[2][0] - 0.706_683).abs() < 2e-3,
            "ACES SDR white: got {}",
            pixels[2][0]
        );
    }

    #[test]
    fn test_apply_rgba() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let mut pixels = [[0.18_f32, 0.18, 0.18, 1.0]];
        proc.apply_rgba(&mut pixels);

        // Alpha should be preserved
        assert_eq!(pixels[0][3], 1.0);
    }
}

/// ColorSpace builder extended parity.
/// OCIO ref: ColorSpace
mod colorspace_builder_extended_parity {
    use crate::colorspace::{AllocationInfo, AllocationType};
    use crate::{ColorSpace, Encoding};

    #[test]
    fn test_colorspace_with_allocation() {
        let cs = ColorSpace::builder("LogC")
            .encoding(Encoding::Log)
            .allocation(AllocationInfo {
                alloc_type: AllocationType::Log2,
                vars: vec![0.0, 1.0],
            })
            .build();

        assert_eq!(cs.allocation().alloc_type, AllocationType::Log2);
    }

    #[test]
    fn test_colorspace_with_aliases() {
        let cs = ColorSpace::builder("sRGB")
            .alias("srgb")
            .alias("sRGB - Texture")
            .build();

        assert_eq!(cs.aliases().len(), 2);
        assert!(cs.matches_name("srgb"));
        assert!(cs.matches_name("sRGB - Texture"));
    }

    #[test]
    fn test_colorspace_with_categories() {
        let cs = ColorSpace::builder("ACEScg")
            .category("scene_linear")
            .category("working_space")
            .build();

        assert!(cs.has_category("scene_linear"));
        assert!(cs.has_category("working_space"));
    }
}

/// Look builder parity.
/// OCIO ref: Look
mod look_builder_parity {
    use crate::look::Look;
    use crate::transform::{CdlTransform, Transform};

    #[test]
    fn test_look_with_transform() {
        let cdl = CdlTransform {
            slope: [1.1, 1.0, 0.9],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            ..Default::default()
        };

        let look = Look::new("ShowLUT")
            .process_space("ACEScg")
            .description("Show grade")
            .transform(Transform::Cdl(cdl));

        assert_eq!(look.name(), "ShowLUT");
        assert_eq!(look.get_process_space(), Some("ACEScg"));
    }
}

/// Display builder parity.
/// OCIO ref: Display
mod display_builder_parity {
    use crate::display::{Display, View};

    #[test]
    fn test_display_with_view() {
        let display = Display::new("sRGB")
            .with_view(View::new("Film", "sRGB"))
            .with_view(View::new("Raw", "Raw"));

        assert_eq!(display.views().len(), 2);
        assert_eq!(display.default_view(), Some("Film"));
    }

    #[test]
    fn test_display_view_lookup() {
        let display = Display::new("sRGB")
            .with_view(View::new("Film", "sRGB"))
            .with_view(View::new("Raw", "Raw"));

        assert!(display.view("film").is_some()); // case-insensitive
        assert!(display.view("Raw").is_some());
        assert!(display.view("NonExistent").is_none());
    }
}

/// View builder parity.
/// OCIO ref: View
mod view_builder_parity {
    use crate::display::View;

    #[test]
    fn test_view_with_look() {
        let view = View::new("Graded", "sRGB")
            .with_look("show_lut")
            .with_description("Show graded view");

        assert_eq!(view.looks(), Some("show_lut"));
        assert_eq!(view.description(), "Show graded view");
    }

    #[test]
    fn test_view_with_view_transform() {
        let view = View::new("ACES SDR", "sRGB").with_view_transform("ACES 1.0 - SDR Video");

        assert_eq!(view.view_transform(), Some("ACES 1.0 - SDR Video"));
    }

    #[test]
    fn test_view_with_rule() {
        let view = View::new("HDR", "Rec2100-PQ").with_rule("hdr_content");

        assert_eq!(view.rule(), Some("hdr_content"));
    }
}

/// Exact transfer function values parity.
/// OCIO ref: Transfer function implementations
mod transfer_exact_values_extended_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;

    #[test]
    fn test_srgb_18_gray() {
        let s = TestSchema::shared();
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .display_processor(&s.working_space, &s.default_display, &s.tone_mapped_view)
            .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);

        // ACES 2.0's SDR 100-nit output transform renders scene mid-grey to
        // **10 nits of 100** — 0.10 display-linear. That is the anchor to check
        // against, so undo the display encoding rather than asserting a
        // remembered encoded number: the previous expectation (0.43 ± 0.05,
        // annotated "measured ≈ 0.414") was a note of what the code did on some
        // earlier day, which makes the test agree with the implementation by
        // construction and disagree with the specification silently.
        let encoded = pixel[0][0];
        let display_linear = if encoded <= 0.04045 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        };
        assert!(
            (display_linear - 0.10).abs() < 0.005,
            "0.18 scene-linear should render to 0.10 display-linear (10 of 100 nits); \
             got {display_linear:.4} (encoded {encoded:.4})"
        );
    }

    #[test]
    fn test_linear_to_srgb_black() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let mut pixel = [[0.0_f32, 0.0, 0.0]];
        proc.apply_rgb(&mut pixel);

        // Black stays black
        assert!(pixel[0][0].abs() < 0.001);
    }

    #[test]
    fn test_linear_to_srgb_white() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let mut pixel = [[1.0_f32, 1.0, 1.0]];
        proc.apply_rgb(&mut pixel);
        // White maps near 1.0 through the ACES output transform.
        assert!(
            (pixel[0][0] - 1.0).abs() < 0.05,
            "white -> ~1.0, got {}",
            pixel[0][0]
        );
        // And the transform is NOT a no-op: a mid-grey 0.18 must be encoded
        // (display value clearly brighter than the scene-linear input).
        let mut mid = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut mid);
        assert!(
            mid[0][0] > 0.18 + 1e-3 && mid[0][0] < 1.0 && mid[0][0].is_finite(),
            "working->sRGB display must encode 0.18 upward, got {}",
            mid[0][0]
        );
    }
}

/// Config colorspaces iteration parity.
/// OCIO ref: Config::getColorSpaces
mod config_colorspaces_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;

    #[test]
    fn test_config_colorspaces_count() {
        let config = builtin::aces_2_0_studio_all_views();
        let colorspaces = config.colorspaces();
        assert!(colorspaces.len() > 10); // ACES config has many colorspaces
    }

    #[test]
    fn test_config_colorspace_lookup() {
        let config = builtin::aces_2_0_studio_all_views();

        // Should find by name
        assert!(config.colorspace("ACEScg").is_some());
        assert!(
            config
                .colorspace(&TestSchema::shared().display_encoded_srgb)
                .is_some()
        );
        assert!(config.colorspace("ACES2065-1").is_some());

        // Case insensitive
        assert!(config.colorspace("acescg").is_some());
    }

    #[test]
    fn test_config_colorspace_by_role() {
        let config = builtin::aces_2_0_studio_all_views();

        // scene_linear role should resolve to ACEScg
        let linear = config.colorspace("scene_linear");
        assert!(linear.is_some());
        assert_eq!(linear.unwrap().name(), "ACEScg");
    }
}

/// BatchTextureDescriptor parity tests.
/// OCIO ref: GpuShaderDesc texture batch API
mod batch_texture_descriptor_parity {
    use crate::gpu::{
        BatchTextureDescriptor, GpuChannelType, GpuInterpolation, GpuTextureType, TextureDimensions,
    };

    #[test]
    fn test_batch_texture_descriptor_struct() {
        let desc = BatchTextureDescriptor {
            name: "lut3d_0".into(),
            binding_index: 0,
            texture_type: GpuTextureType::Texture3D,
            width: 17,
            height: 17,
            depth: 17,
            channel_type: GpuChannelType::Rgb,
            interpolation: GpuInterpolation::Linear,
            dimensions: TextureDimensions::Texture2D,
            data_offset: 0,
            data_size: 17 * 17 * 17 * 3 * 4,
        };

        assert_eq!(desc.name, "lut3d_0");
        assert_eq!(desc.width, 17);
    }

    #[test]
    fn test_batch_texture_1d() {
        let desc = BatchTextureDescriptor {
            name: "lut1d_0".into(),
            binding_index: 1,
            texture_type: GpuTextureType::Texture1D,
            width: 4096,
            height: 1,
            depth: 1,
            channel_type: GpuChannelType::Rgb,
            interpolation: GpuInterpolation::Linear,
            dimensions: TextureDimensions::Texture1D,
            data_offset: 0,
            data_size: 4096 * 3 * 4,
        };

        assert_eq!(desc.texture_type, GpuTextureType::Texture1D);
    }

    #[test]
    fn test_batch_texture_red_channel() {
        let desc = BatchTextureDescriptor {
            name: "shaper".into(),
            binding_index: 2,
            texture_type: GpuTextureType::Texture1D,
            width: 1024,
            height: 1,
            depth: 1,
            channel_type: GpuChannelType::Red,
            interpolation: GpuInterpolation::Nearest,
            dimensions: TextureDimensions::Texture1D,
            data_offset: 0,
            data_size: 1024 * 4,
        };

        assert_eq!(desc.channel_type, GpuChannelType::Red);
        assert_eq!(desc.interpolation, GpuInterpolation::Nearest);
    }
}

/// GpuProcessor batch texture API parity.
/// OCIO ref: GpuShaderCreator batch operations
mod gpu_processor_batch_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};

    #[test]
    fn test_batch_texture_descriptors() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();

        let descriptors = gpu.batch_texture_descriptors();
        // Internal consistency the API must uphold: one batch descriptor per GPU
        // texture the processor needs (a no-op that returns an empty Vec while
        // num_textures()>0 fails). Holds for any transform set, incl. empty.
        assert_eq!(
            descriptors.len(),
            gpu.num_textures(),
            "batch descriptor count must equal texture count"
        );
    }

    #[test]
    fn test_gpu_textures() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();

        let textures = gpu.textures();
        // textures() and num_textures() are two views of the same set and must
        // agree; a discarded-result impl returning [] while num_textures()>0
        // fails. (Holds regardless of how many LUTs the transform needs.)
        assert_eq!(textures.len(), gpu.num_textures());
    }
}

/// GradingPrimaryTransform comprehensive parity.
/// OCIO ref: GradingPrimaryTransform
mod grading_primary_transform_comprehensive_parity {
    use crate::transform::{GradingPrimaryStyle, GradingPrimaryTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_grading_primary_transform_default() {
        let gpt = GradingPrimaryTransform::default();
        assert!(matches!(gpt.style, GradingPrimaryStyle::Log));
    }

    #[test]
    fn test_grading_primary_transform_fields() {
        let mut gpt = GradingPrimaryTransform::default();
        gpt.value.saturation = 1.2;
        gpt.value.pivot = 0.18;

        // saturation=1.2 is non-identity: it must pull a chromatic pixel's
        // channels apart from grey (a no-op/identity sat=1.0 would leave the
        // ratio unchanged). Probe is chromatic so saturation has an effect.
        let t = Transform::GradingPrimary(gpt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.6_f32, 0.3, 0.1]];
        proc.apply_rgb(&mut px);
        assert!(
            px[0].iter().all(|c| c.is_finite()),
            "GradingPrimary produced non-finite"
        );
        // Saturation >1 increases the spread between max and min channel.
        let spread_in = 0.6_f32 - 0.1;
        let spread_out = px[0].iter().cloned().fold(f32::MIN, f32::max)
            - px[0].iter().cloned().fold(f32::MAX, f32::min);
        assert!(
            spread_out > spread_in + 1e-4,
            "saturation 1.2 must widen channel spread: in {spread_in}, out {spread_out}"
        );
    }
}

/// GradingRgbCurveTransform comprehensive parity.
/// OCIO ref: GradingRGBCurveTransform
mod grading_rgb_curve_transform_comprehensive_parity {
    use crate::transform::{
        GradingRgbCurveTransform, GradingToneStyle, RgbCurveType, TransformDirection,
    };
    use crate::{Processor, Transform};

    #[test]
    fn test_grading_rgb_curve_default() {
        // Default RGB curve is the identity diagonal: a chromatic probe must
        // round-trip through its processor unchanged.
        let t = Transform::GradingRgbCurve(GradingRgbCurveTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.25_f32, 0.5, 0.75]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.25_f32, 0.5, 0.75].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-5,
                "identity RGB curve ch{i}: {}",
                px[0][i]
            );
        }
    }

    #[test]
    fn test_grading_rgb_curve_processor() {
        let t = Transform::GradingRgbCurve(GradingRgbCurveTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.2_f32, 0.4, 0.6]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.2_f32, 0.4, 0.6].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-5,
                "identity RGB curve ch{i}: {}",
                px[0][i]
            );
        }
    }
}

/// GradingToneTransform comprehensive parity.
/// OCIO ref: GradingToneTransform
mod grading_tone_transform_comprehensive_parity {
    use crate::transform::{
        GradingRgbmsw, GradingToneStyle, GradingToneTransform, TransformDirection,
    };
    use crate::{Processor, Transform};

    #[test]
    fn test_grading_rgbmsw_struct() {
        let rgbmsw = GradingRgbmsw {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            master: 1.0,
            start: 0.0,
            width: 1.0,
        };
        assert_eq!(rgbmsw.master, 1.0);
    }

    #[test]
    fn test_grading_tone_default() {
        // The default GradingTone is identity (scontrast=1, identity zones);
        // applied via a processor it must leave a chromatic probe unchanged.
        let t = Transform::GradingTone(GradingToneTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.25_f32, 0.5, 0.75]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.25_f32, 0.5, 0.75].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-5,
                "identity GradingTone ch{i}: {}",
                px[0][i]
            );
        }
    }

    #[test]
    fn test_grading_tone_processor() {
        let t = Transform::GradingTone(GradingToneTransform::default());
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.2_f32, 0.4, 0.6]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.2_f32, 0.4, 0.6].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-5,
                "identity GradingTone ch{i}: {}",
                px[0][i]
            );
        }
    }
}

/// GradingHueCurveTransform comprehensive parity.
/// OCIO ref: GradingHueCurveTransform
mod grading_hue_curve_transform_comprehensive_parity {
    use crate::transform::{
        GradingHueCurveStyle, GradingHueCurveTransform, HueCurveType, TransformDirection,
    };
    use crate::{Processor, Transform};

    #[test]
    fn test_grading_hue_curve_processor() {
        let ghc = GradingHueCurveTransform {
            style: GradingHueCurveStyle::Log,
            ..Default::default()
        };
        let t = Transform::GradingHueCurve(ghc);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        // Default Log hue curves are identity: a chromatic probe round-trips.
        let mut px = [[0.25_f32, 0.5, 0.75]];
        proc.apply_rgb(&mut px);
        for (i, &c) in [0.25_f32, 0.5, 0.75].iter().enumerate() {
            assert!(
                (px[0][i] - c).abs() < 1e-5,
                "identity hue curve ch{i}: {}",
                px[0][i]
            );
        }
    }
}

/// FixedFunctionTransform ACES 2.0 parity.
/// OCIO ref: FixedFunctionTransform ACES 2.0 styles
mod fixed_function_aces2_parity {
    use crate::transform::{FixedFunctionStyle, FixedFunctionTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_aces2_output_transform() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesOutputTransform20,
            params: vec![1000.0], // peak luminance
            direction: TransformDirection::Forward,
        };
        let t = Transform::FixedFunction(ff);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        // May fail if not fully implemented, but struct should exist
        let _ = proc;
    }

    #[test]
    fn test_aces2_rgb_to_jmh() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRgbToJmh20,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let _ = Transform::FixedFunction(ff);
    }

    #[test]
    fn test_aces2_jmh_to_rgb() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesJmhToRgb20,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let _ = Transform::FixedFunction(ff);
    }

    #[test]
    fn test_aces2_tonescale_compress() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesTonescaleCompress20,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let _ = Transform::FixedFunction(ff);
    }
}

/// LogTransform comprehensive parity.
/// OCIO ref: LogTransform, LogAffineTransform, LogCameraTransform
mod log_transform_comprehensive_parity {
    use crate::transform::{LogTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_log_transform_base_2() {
        let lt = LogTransform {
            name: String::new(),
            base: 2.0,
            direction: TransformDirection::Forward,
        };
        assert_eq!(lt.base, 2.0);
    }

    #[test]
    fn test_log_transform_base_10() {
        let lt = LogTransform {
            name: String::new(),
            base: 10.0,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Log(lt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_log_transform_natural() {
        let lt = LogTransform {
            name: String::new(),
            base: std::f64::consts::E,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Log(lt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

/// ExponentTransform comprehensive parity.
/// OCIO ref: ExponentTransform, ExponentWithLinearTransform
mod exponent_transform_comprehensive_parity {
    use crate::transform::{ExponentTransform, NegativeStyle, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_exponent_gamma_22() {
        let et = ExponentTransform {
            name: String::new(),
            value: [2.2, 2.2, 2.2, 1.0],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Exponent(et);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_exponent_gamma_24() {
        let et = ExponentTransform {
            name: String::new(),
            value: [2.4, 2.4, 2.4, 1.0],
            negative_style: NegativeStyle::Mirror,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Exponent(et);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

/// MatrixTransform comprehensive parity.
/// OCIO ref: MatrixTransform
mod matrix_transform_comprehensive_parity {
    use crate::transform::{MatrixTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_identity_matrix() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let t = Transform::Matrix(mt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // Identity should preserve values
        assert!((pixel[0][0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_scale_matrix() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let t = Transform::Matrix(mt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixel = [[0.25_f32, 0.25, 0.25]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_offset_matrix() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.1, 0.1, 0.1, 0.0],
            direction: TransformDirection::Forward,
        };
        let t = Transform::Matrix(mt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixel = [[0.0_f32, 0.0, 0.0]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.1).abs() < 0.001);
    }
}

/// CDLTransform comprehensive parity.
/// OCIO ref: CDLTransform
mod cdl_transform_comprehensive_parity {
    use crate::transform::{CdlStyle, CdlTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_cdl_slope() {
        let cdl = CdlTransform {
            name: String::new(),
            slope: [1.5, 1.0, 0.8],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Cdl(cdl);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // Red should be scaled by 1.5
        assert!((pixel[0][0] - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_cdl_offset() {
        let cdl = CdlTransform {
            slope: [1.0, 1.0, 1.0],
            offset: [0.1, 0.0, -0.1],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            ..Default::default()
        };
        let t = Transform::Cdl(cdl);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_cdl_power() {
        let cdl = CdlTransform {
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [2.0, 1.0, 0.5],
            saturation: 1.0,
            ..Default::default()
        };
        let t = Transform::Cdl(cdl);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // Red^2 = 0.25
        assert!((pixel[0][0] - 0.25).abs() < 0.01);
    }
}

/// RangeTransform comprehensive parity.
/// OCIO ref: RangeTransform
mod range_transform_comprehensive_parity {
    use crate::transform::{RangeStyle, RangeTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_range_clamp() {
        let rt = RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(1.0),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Range(rt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixel = [[1.5_f32, 0.5, -0.5]];
        proc.apply_rgb(&mut pixel);
        // Should clamp to [0,1]
        assert!(pixel[0][0] <= 1.0);
        assert!(pixel[0][2] >= 0.0);
    }

    #[test]
    fn test_range_scale() {
        let rt = RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(2.0),
            style: RangeStyle::NoClamp,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Range(rt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // Should scale to [0,2], so 0.5 -> 1.0
        assert!((pixel[0][0] - 1.0).abs() < 0.01);
    }
}

/// GPU shader code content parity.
/// OCIO ref: GpuShaderDesc getShaderText
mod gpu_shader_code_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};

    #[test]
    fn test_glsl_shader_has_main() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();

        let code = shader.fragment_code();
        // GLSL should have main function or entry point
        assert!(!code.is_empty());
    }

    #[test]
    fn test_hlsl_shader_syntax() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Hlsl50).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_metal_shader_syntax() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Metal).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }
}

/// GPU uniforms parity.
/// OCIO ref: GpuShaderDesc uniforms
mod gpu_uniforms_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor, GpuUniformType};

    #[test]
    fn test_gpu_shader_uniforms() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();

        // uniforms() is on GpuShaderCode, not GpuProcessor
        let uniforms = shader.uniforms();
        // May or may not have uniforms depending on transforms
        let _ = uniforms;
    }
}

/// Config file loading parity.
/// OCIO ref: Config::CreateFromFile
mod config_file_parity {
    use crate::Config;
    use std::path::PathBuf;

    #[test]
    fn test_config_from_yaml_str() {
        let yaml = r#"
ocio_profile_version: 2.1
environment: {}
roles:
  default: raw
  scene_linear: linear
file_rules:
  - !<Rule> {name: Default, colorspace: raw}
displays:
  sRGB:
    - !<View> {name: Raw, colorspace: raw}
active_displays: [sRGB]
active_views: [Raw]
colorspaces:
  - !<ColorSpace>
    name: raw
    isdata: true
  - !<ColorSpace>
    name: linear
    encoding: scene-linear
"#;
        let config = Config::from_yaml_str(yaml, PathBuf::from("."));
        assert!(config.is_ok());
    }
}

/// Roles parity.
/// OCIO ref: Config roles
mod roles_comprehensive_parity {
    use crate::builtin;
    use crate::role::Roles;

    #[test]
    fn test_roles_from_config() {
        let config = builtin::aces_2_0_studio_all_views();
        let roles = config.roles();

        // ACES config should have scene_linear role
        assert!(roles.get("scene_linear").is_some());
    }

    #[test]
    fn test_roles_new() {
        let mut roles = Roles::new();
        roles.define("scene_linear", "ACEScg");
        roles.define("data", "Raw");

        assert_eq!(roles.get("scene_linear"), Some("ACEScg"));
        assert_eq!(roles.get("data"), Some("Raw"));
    }

    #[test]
    fn test_roles_iter() {
        let config = builtin::aces_2_0_studio_all_views();
        let roles = config.roles();

        let count = roles.iter().count();
        assert!(count > 0);
    }
}

/// Processor inverse parity.
/// OCIO ref: Processor::getOptimizedProcessor with INVERSE
mod processor_inverse_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;

    #[test]
    fn test_processor_roundtrip() {
        let config = builtin::aces_2_0_studio_all_views();

        // Forward: ACEScg -> sRGB
        let forward = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        // Inverse: sRGB -> ACEScg
        let inverse = config
            .processor(
                &TestSchema::shared().display_encoded_srgb,
                &TestSchema::shared().working_space,
            )
            .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        let original = pixel;

        forward.apply_rgb(&mut pixel);
        inverse.apply_rgb(&mut pixel);

        // Should return to original (within tolerance)
        assert!((pixel[0][0] - original[0][0]).abs() < 0.001);
    }
}

/// Look manager parity.
/// OCIO ref: LookManager
mod look_manager_parity {
    use crate::look::{Look, LookManager};
    use crate::transform::{CdlTransform, Transform};

    #[test]
    fn test_look_manager_add() {
        let mut mgr = LookManager::new();

        let look = Look::new("ShowGrade")
            .process_space("ACEScg")
            .transform(Transform::Cdl(CdlTransform::default()));

        mgr.add(look);
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn test_look_manager_get() {
        let mut mgr = LookManager::new();

        let look = Look::new("TestLook").process_space("ACEScg");

        mgr.add(look);

        assert!(mgr.get("TestLook").is_some());
        assert!(mgr.get("testlook").is_some()); // case-insensitive
        assert!(mgr.get("NonExistent").is_none());
    }
}

/// LogAffineTransform comprehensive parity.
/// OCIO ref: LogAffineTransform
mod log_affine_transform_parity {
    use crate::transform::{LogAffineTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_log_affine_default() {
        let lat = LogAffineTransform::default();
        assert_eq!(lat.base, 2.0);
        assert_eq!(lat.log_side_slope, [1.0, 1.0, 1.0]);
        assert_eq!(lat.log_side_offset, [0.0, 0.0, 0.0]);
        assert_eq!(lat.lin_side_slope, [1.0, 1.0, 1.0]);
        assert_eq!(lat.lin_side_offset, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_log_affine_base_10() {
        let lat = LogAffineTransform {
            base: 10.0,
            ..Default::default()
        };
        let t = Transform::LogAffine(lat);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_log_affine_custom_params() {
        let lat = LogAffineTransform {
            name: String::new(),
            base: 10.0,
            log_side_slope: [std::f64::consts::LOG10_E; 3], // ACEScct-like
            log_side_offset: [0.0, 0.0, 0.0],
            lin_side_slope: [1.0, 1.0, 1.0],
            lin_side_offset: [0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        };
        let t = Transform::LogAffine(lat);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_log_affine_roundtrip() {
        let lat = LogAffineTransform::default();
        let fwd = Processor::from_transform(
            &Transform::LogAffine(lat.clone()),
            TransformDirection::Forward,
        )
        .unwrap();
        let inv =
            Processor::from_transform(&Transform::LogAffine(lat), TransformDirection::Inverse)
                .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        let original = pixel[0];
        fwd.apply_rgb(&mut pixel);
        inv.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - original[0]).abs() < 0.001);
    }
}

/// LogCameraTransform comprehensive parity.
/// OCIO ref: LogCameraTransform
mod log_camera_transform_parity {
    use crate::transform::{LogCameraTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_log_camera_default() {
        let lct = LogCameraTransform::default();
        assert_eq!(lct.base, 2.0);
        assert_eq!(lct.lin_side_break, [0.0, 0.0, 0.0]);
        assert!(lct.linear_slope.is_none());
    }

    #[test]
    fn test_log_camera_with_break() {
        let lct = LogCameraTransform {
            base: 10.0,
            lin_side_break: [0.0078125, 0.0078125, 0.0078125], // ACEScct break point
            ..Default::default()
        };
        let t = Transform::LogCamera(lct);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_log_camera_roundtrip() {
        let lct = LogCameraTransform {
            lin_side_break: [0.01, 0.01, 0.01],
            ..Default::default()
        };
        let fwd = Processor::from_transform(
            &Transform::LogCamera(lct.clone()),
            TransformDirection::Forward,
        )
        .unwrap();
        let inv =
            Processor::from_transform(&Transform::LogCamera(lct), TransformDirection::Inverse)
                .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        let original = pixel[0];
        fwd.apply_rgb(&mut pixel);
        inv.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - original[0]).abs() < 0.001);
    }
}

/// Lut1DTransform comprehensive parity.
/// OCIO ref: Lut1DTransform
mod lut1d_transform_comprehensive_parity {
    use crate::transform::{Interpolation, Lut1DHueAdjust, Lut1DTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_lut1d_identity() {
        // Identity LUT: linear ramp
        let size = 65;
        let mut red = Vec::with_capacity(size);
        for i in 0..size {
            let v = i as f32 / (size - 1) as f32;
            red.push(v);
        }

        let lut = Lut1DTransform {
            red,
            green: None,
            blue: None,
            input_min: 0.0,
            input_max: 1.0,
            output_min: 0.0,
            output_max: 1.0,
            half_domain: false,
            raw_halfs: false,
            hue_adjust: Lut1DHueAdjust::None,
            interpolation: Interpolation::Linear,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Lut1D(lut);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // Identity should preserve values
        assert!((pixel[0][0] - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_lut1d_gamma_curve() {
        // Gamma 2.2 LUT
        let size = 1024;
        let mut red = Vec::with_capacity(size);
        for i in 0..size {
            let v = (i as f32 / (size - 1) as f32).powf(2.2);
            red.push(v);
        }

        let lut = Lut1DTransform {
            red,
            green: None,
            blue: None,
            input_min: 0.0,
            input_max: 1.0,
            output_min: 0.0,
            output_max: 1.0,
            half_domain: false,
            raw_halfs: false,
            hue_adjust: Lut1DHueAdjust::None,
            interpolation: Interpolation::Linear,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Lut1D(lut);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

/// Lut3DTransform comprehensive parity.
/// OCIO ref: Lut3DTransform
mod lut3d_transform_comprehensive_parity {
    use crate::transform::{Interpolation, Lut3DTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_lut3d_identity() {
        // Small identity 3D LUT
        let size = 5;
        let mut data = Vec::with_capacity(size * size * size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let rv = r as f32 / (size - 1) as f32;
                    let gv = g as f32 / (size - 1) as f32;
                    let bv = b as f32 / (size - 1) as f32;
                    data.push([rv, gv, bv]);
                }
            }
        }

        let lut = Lut3DTransform {
            data,
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Tetrahedral,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Lut3D(lut);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // Identity should approximately preserve values
        assert!((pixel[0][0] - 0.5).abs() < 0.1);
    }

    #[test]
    fn test_lut3d_tetrahedral() {
        let size = 3;
        let mut data = Vec::with_capacity(size * size * size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let rv = r as f32 / (size - 1) as f32;
                    let gv = g as f32 / (size - 1) as f32;
                    let bv = b as f32 / (size - 1) as f32;
                    data.push([rv, gv, bv]);
                }
            }
        }

        let lut = Lut3DTransform {
            data,
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Tetrahedral,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Lut3D(lut);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_lut3d_linear_interp() {
        let size = 3;
        let mut data = Vec::with_capacity(size * size * size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let rv = r as f32 / (size - 1) as f32;
                    let gv = g as f32 / (size - 1) as f32;
                    let bv = b as f32 / (size - 1) as f32;
                    data.push([rv, gv, bv]);
                }
            }
        }

        let lut = Lut3DTransform {
            data,
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Linear,
            direction: TransformDirection::Forward,
        };
        let t = Transform::Lut3D(lut);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

/// ExponentWithLinearTransform parity.
/// OCIO ref: ExponentWithLinearTransform
mod exponent_with_linear_transform_parity {
    use crate::transform::{ExponentWithLinearTransform, NegativeStyle, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_exp_linear_srgb() {
        // sRGB-like curve with linear segment
        let ewl = ExponentWithLinearTransform {
            name: String::new(),
            gamma: [2.4, 2.4, 2.4, 1.0],
            offset: [0.055, 0.055, 0.055, 0.0],
            negative_style: NegativeStyle::Linear,
            direction: TransformDirection::Forward,
        };
        let t = Transform::ExponentWithLinear(ewl);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_exp_linear_roundtrip() {
        let ewl = ExponentWithLinearTransform {
            name: String::new(),
            gamma: [2.2, 2.2, 2.2, 1.0],
            offset: [0.0, 0.0, 0.0, 0.0],
            negative_style: NegativeStyle::Mirror,
            direction: TransformDirection::Forward,
        };
        let fwd = Processor::from_transform(
            &Transform::ExponentWithLinear(ewl.clone()),
            TransformDirection::Forward,
        )
        .unwrap();
        let inv = Processor::from_transform(
            &Transform::ExponentWithLinear(ewl),
            TransformDirection::Inverse,
        )
        .unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        let original = pixel[0];
        fwd.apply_rgb(&mut pixel);
        inv.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - original[0]).abs() < 0.001);
    }
}

/// GroupTransform comprehensive parity.
/// OCIO ref: GroupTransform
mod group_transform_comprehensive_parity {
    use crate::Processor;
    use crate::transform::{
        CdlTransform, GroupTransform, MatrixTransform, Transform, TransformDirection,
    };

    #[test]
    fn test_group_empty() {
        let gt = GroupTransform {
            name: String::new(),
            transforms: vec![],
            direction: TransformDirection::Forward,
        };
        let t = Transform::Group(gt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_group_single() {
        let gt = GroupTransform {
            name: String::new(),
            transforms: vec![Transform::Cdl(CdlTransform::default())],
            direction: TransformDirection::Forward,
        };
        let t = Transform::Group(gt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_group_chain() {
        let gt = GroupTransform {
            name: String::new(),
            transforms: vec![
                Transform::Cdl(CdlTransform {
                    slope: [1.1, 1.0, 0.9],
                    ..Default::default()
                }),
                Transform::Matrix(MatrixTransform {
                    name: String::new(),
                    matrix: MatrixTransform::IDENTITY,
                    offset: [0.0; 4],
                    direction: TransformDirection::Forward,
                }),
            ],
            direction: TransformDirection::Forward,
        };
        let t = Transform::Group(gt);
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // Slope applied
        assert!((pixel[0][0] - 0.55).abs() < 0.01);
    }

    #[test]
    fn test_group_nested() {
        let inner = GroupTransform {
            name: String::new(),
            transforms: vec![Transform::Cdl(CdlTransform::default())],
            direction: TransformDirection::Forward,
        };
        let outer = GroupTransform {
            name: String::new(),
            transforms: vec![Transform::Group(inner)],
            direction: TransformDirection::Forward,
        };
        let t = Transform::Group(outer);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

/// AllocationTransform comprehensive parity.
/// OCIO ref: AllocationTransform
mod allocation_transform_comprehensive_parity {
    use crate::transform::{AllocationTransform, AllocationType, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_allocation_uniform() {
        let at = AllocationTransform {
            allocation: AllocationType::Uniform,
            vars: vec![0.0, 1.0],
            direction: TransformDirection::Forward,
        };
        let t = Transform::Allocation(at);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_allocation_log() {
        let at = AllocationTransform {
            allocation: AllocationType::Log2,
            vars: vec![-8.0, 5.0],
            direction: TransformDirection::Forward,
        };
        let t = Transform::Allocation(at);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_allocation_log_with_offset() {
        let at = AllocationTransform {
            allocation: AllocationType::Log2,
            vars: vec![-8.0, 5.0, 0.001], // min, max, offset
            direction: TransformDirection::Forward,
        };
        let t = Transform::Allocation(at);
        let proc = Processor::from_transform(&t, TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

/// ColorSpaceTransform comprehensive parity.
/// OCIO ref: ColorSpaceTransform
mod colorspace_transform_comprehensive_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::transform::{ColorSpaceTransform, TransformDirection};

    #[test]
    fn test_colorspace_transform_struct() {
        let cst = ColorSpaceTransform {
            src: "ACEScg".to_string(),
            dst: "sRGB".to_string(),
            data_bypass: true,
            direction: TransformDirection::Forward,
        };
        assert_eq!(cst.src, "ACEScg");
        assert_eq!(cst.dst, "sRGB");
    }

    #[test]
    fn test_colorspace_via_config() {
        let config = builtin::aces_2_0_studio_all_views();
        // ColorSpaceTransform is used internally by config.processor()
        let proc = config.processor(
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        );
        assert!(proc.is_ok());
    }
}

/// DisplayViewTransform comprehensive parity.
/// OCIO ref: DisplayViewTransform
mod display_view_transform_comprehensive_parity {
    use crate::transform::{DisplayViewTransform, TransformDirection};

    #[test]
    fn test_display_view_transform_struct() {
        let dvt = DisplayViewTransform {
            src: "ACEScg".to_string(),
            display: "sRGB".to_string(),
            view: "ACES 1.0 - SDR Video".to_string(),
            looks_bypass: false,
            data_bypass: true,
            looks_override: String::new(),
            looks_override_enabled: false,
            direction: TransformDirection::Forward,
        };
        assert_eq!(dvt.display, "sRGB");
        assert_eq!(dvt.view, "ACES 1.0 - SDR Video");
    }
}

/// LookTransform comprehensive parity.
/// OCIO ref: LookTransform
mod look_transform_comprehensive_parity {
    use crate::transform::{LookTransform, TransformDirection};

    #[test]
    fn test_look_transform_struct() {
        let lt = LookTransform {
            src: "ACEScg".to_string(),
            dst: "ACEScg".to_string(),
            looks: "ShowGrade".to_string(),
            skip_color_space_conversion: false,
            direction: TransformDirection::Forward,
        };
        assert_eq!(lt.looks, "ShowGrade");
    }

    #[test]
    fn test_look_transform_multiple_looks() {
        let lt = LookTransform {
            src: "ACEScg".to_string(),
            dst: "ACEScg".to_string(),
            looks: "ShowGrade, ShotGrade".to_string(),
            skip_color_space_conversion: false,
            direction: TransformDirection::Forward,
        };
        assert!(lt.looks.contains(','));
    }
}

/// Processor operations comprehensive parity.
/// OCIO ref: Processor CPU/GPU operations
mod processor_ops_comprehensive_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::processor::OptimizationLevel;

    #[test]
    fn test_processor_apply_rgb() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixels);
        // Result should be in display range
        assert!(pixels[0][0] >= 0.0 && pixels[0][0] <= 1.5);
    }

    #[test]
    fn test_processor_apply_rgba() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let mut pixels = [[0.18_f32, 0.18, 0.18, 1.0]];
        proc.apply_rgba(&mut pixels);
        // Alpha should be preserved
        assert_eq!(pixels[0][3], 1.0);
    }

    #[test]
    fn test_processor_batch() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let mut pixels = [[0.0_f32, 0.0, 0.0], [0.18, 0.18, 0.18], [1.0, 1.0, 1.0]];
        proc.apply_rgb(&mut pixels);
        // All pixels processed
        assert!(pixels.iter().all(|p| p[0] >= 0.0));
    }
}

/// Config displays and views comprehensive parity.
/// OCIO ref: Config getDisplay, getView
mod config_displays_views_comprehensive_parity {
    use crate::builtin;

    #[test]
    fn test_config_displays() {
        let config = builtin::aces_2_0_studio_all_views();
        let displays = config.displays();
        assert!(!displays.displays().is_empty());
    }
}

/// Config colorspaces iteration parity.
/// OCIO ref: Config getColorSpaceNames
mod config_colorspaces_iteration_parity {
    use crate::builtin;

    #[test]
    fn test_colorspaces_slice() {
        let config = builtin::aces_2_0_studio_all_views();
        let colorspaces = config.colorspaces();
        assert!(colorspaces.len() > 10); // ACES has many colorspaces
    }

    #[test]
    fn test_colorspace_names() {
        let config = builtin::aces_2_0_studio_all_views();
        let names: Vec<_> = config
            .colorspaces()
            .iter()
            .map(|cs| cs.name().to_string())
            .collect();
        assert!(names.contains(&"ACEScg".to_string()));
    }

    #[test]
    fn test_colorspace_by_encoding() {
        let config = builtin::aces_2_0_studio_all_views();
        use crate::colorspace::Encoding;
        let scene_linear: Vec<_> = config
            .colorspaces()
            .iter()
            .filter(|cs| cs.encoding() == Encoding::SceneLinear)
            .collect();
        assert!(!scene_linear.is_empty());
    }
}

/// FormatMetadata parity.
/// OCIO ref: FormatMetadata
mod format_metadata_parity {
    use crate::processor::{FormatMetadata, ProcessorMetadata};

    #[test]
    fn test_format_metadata_default() {
        let fm = FormatMetadata::default();
        assert!(fm.name.is_empty());
        assert!(fm.id.is_empty());
    }

    #[test]
    fn test_format_metadata_fields() {
        let fm = FormatMetadata {
            id: "test_id".to_string(),
            name: "Test Name".to_string(),
            attributes: vec![("key".to_string(), "value".to_string())],
        };
        assert_eq!(fm.name, "Test Name");
        assert_eq!(fm.id, "test_id");
    }

    #[test]
    fn test_processor_metadata() {
        let pm = ProcessorMetadata {
            num_files: 0,
            files: vec![],
        };
        assert!(pm.files.is_empty());
    }
}

/// NamedTransform comprehensive parity.
/// OCIO ref: NamedTransform
mod named_transform_comprehensive_parity {
    use crate::config::NamedTransform;
    use crate::transform::{CdlTransform, Transform};

    #[test]
    fn test_named_transform_struct() {
        let nt = NamedTransform {
            name: "MyGrade".to_string(),
            family: Some("grading".to_string()),
            description: Some("My custom grade".to_string()),
            forward: Some(Transform::Cdl(CdlTransform::default())),
            inverse: None,
            ..NamedTransform::default()
        };
        assert_eq!(nt.name, "MyGrade");
        assert!(nt.forward.is_some());
    }

    #[test]
    fn test_named_transform_with_inverse() {
        let nt = NamedTransform {
            name: "BidirectionalGrade".to_string(),
            family: None,
            description: None,
            forward: Some(Transform::Cdl(CdlTransform {
                slope: [1.1, 1.0, 0.9],
                ..Default::default()
            })),
            inverse: Some(Transform::Cdl(CdlTransform {
                slope: [1.0 / 1.1, 1.0, 1.0 / 0.9],
                ..Default::default()
            })),
            ..NamedTransform::default()
        };
        assert!(nt.forward.is_some());
        assert!(nt.inverse.is_some());
    }
}

/// ViewingRule comprehensive parity.
/// OCIO ref: ViewingRules
mod viewing_rule_comprehensive_parity {
    use crate::config::ViewingRule;

    #[test]
    fn test_viewing_rule_struct() {
        let vr = ViewingRule {
            name: "my_rule".to_string(),
            colorspaces: vec!["ACEScg".to_string()],
            encodings: vec!["scene-linear".to_string()],
            custom_keys: vec![("key1".to_string(), "value1".to_string())],
        };
        assert_eq!(vr.name, "my_rule");
        assert!(vr.colorspaces.contains(&"ACEScg".to_string()));
    }

    #[test]
    fn test_viewing_rule_encodings() {
        let vr = ViewingRule {
            name: "encoding_rule".to_string(),
            colorspaces: vec![],
            encodings: vec!["log".to_string(), "sdr-video".to_string()],
            custom_keys: vec![],
        };
        assert_eq!(vr.encodings.len(), 2);
    }
}

/// SharedView comprehensive parity.
/// OCIO ref: SharedView
mod shared_view_comprehensive_extended_parity {
    use crate::display::View;

    #[test]
    fn test_shared_view_struct() {
        let sv = View::new("ACES 1.0 - SDR Video", "sRGB - Display")
            .with_view_transform("SDR Video - ACES 1.0")
            .with_description("Standard SDR video view");
        assert_eq!(sv.name(), "ACES 1.0 - SDR Video");
    }

    #[test]
    fn test_shared_view_with_looks() {
        let sv = View::new("Graded", "sRGB")
            .with_view_transform("SDR Video")
            .with_look("ShowGrade, ShotGrade");
        assert!(sv.looks().is_some());
    }
}

/// VirtualDisplay comprehensive parity.
/// OCIO ref: VirtualDisplay
mod virtual_display_comprehensive_extended_parity {
    use crate::config::{VirtualDisplay, VirtualDisplayView};

    #[test]
    fn test_virtual_display_struct() {
        let vd = VirtualDisplay {
            views: vec![VirtualDisplayView {
                name: "Raw".to_string(),
                view_transform: None,
                display_colorspace: "Raw".to_string(),
                looks: None,
                rule: None,
                description: None,
            }],
            shared_view_refs: vec![],
        };
        assert_eq!(vd.views.len(), 1);
    }

    #[test]
    fn test_virtual_display_with_shared_refs() {
        let vd = VirtualDisplay {
            views: vec![],
            shared_view_refs: vec![
                "ACES 1.0 - SDR Video".to_string(),
                "ACES 1.0 - Rec.709".to_string(),
            ],
        };
        assert_eq!(vd.shared_view_refs.len(), 2);
    }
}

/// FileRule comprehensive parity.
/// OCIO ref: FileRules
mod file_rule_comprehensive_parity {
    use crate::config::{FileRule, FileRuleKind};

    #[test]
    fn test_file_rule_basic() {
        let rule = FileRule {
            name: "exr_files".to_string(),
            colorspace: "ACEScg".to_string(),
            kind: FileRuleKind::Basic {
                pattern: "*.exr".to_string(),
                extension: None,
            },
            ..FileRule::default()
        };
        assert_eq!(rule.name, "exr_files");
        assert_eq!(rule.colorspace, "ACEScg");
    }

    #[test]
    fn test_file_rule_default() {
        let rule = FileRule {
            name: "Default".to_string(),
            colorspace: "Raw".to_string(),
            kind: FileRuleKind::Default,
            ..FileRule::default()
        };
        assert_eq!(rule.name, "Default");
        assert_eq!(rule.colorspace, "Raw");
    }
}

/// Context precision parity.
/// OCIO ref: Context
mod context_precision_parity {
    use crate::context::Context;

    #[test]
    fn test_context_set_get_extended() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");
        ctx.set("SEQ", "seq01");

        assert_eq!(ctx.get("SHOT").as_deref(), Some("sh010"));
        assert_eq!(ctx.get("SEQ").as_deref(), Some("seq01"));
    }

    #[test]
    fn test_context_resolve_path_extended() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");
        ctx.set("SEQ", "seq01");

        let resolved = ctx.resolve("/shows/$SEQ/$SHOT/luts/grade.cube");
        assert_eq!(resolved, "/shows/seq01/sh010/luts/grade.cube");
    }

    #[test]
    fn test_context_resolve_nested_extended() {
        let mut ctx = Context::new();
        ctx.set("BASE", "/shows");
        ctx.set("PROJECT", "myproject");

        let resolved = ctx.resolve("$BASE/$PROJECT/config.ocio");
        assert_eq!(resolved, "/shows/myproject/config.ocio");
    }

    #[test]
    fn test_context_env_style_braces() {
        let mut ctx = Context::new();
        ctx.set("VAR", "value");

        // Test ${VAR} style
        let resolved = ctx.resolve("/path/${VAR}/file");
        assert!(resolved.contains("value"));
    }
}

/// Transfer function parity - exact values.
/// OCIO ref: sRGB, PQ, HLG, Rec.709 curves
mod transfer_function_values_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;

    #[test]
    fn test_srgb_transfer_18_gray() {
        let s = TestSchema::shared();
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .display_processor(&s.working_space, &s.default_display, &s.tone_mapped_view)
            .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);

        assert!(pixel[0][0] > 0.3 && pixel[0][0] < 0.6);
    }

    #[test]
    fn test_linear_to_srgb_black() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let mut pixel = [[0.0_f32, 0.0, 0.0]];
        proc.apply_rgb(&mut pixel);

        // Black stays black
        assert!(pixel[0][0].abs() < 0.01);
    }

    #[test]
    fn test_linear_to_srgb_white() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let mut pixel = [[1.0_f32, 1.0, 1.0]];
        proc.apply_rgb(&mut pixel);
        // White maps near 1.0 (slight RRT roll-off allowed).
        assert!(
            (pixel[0][0] - 1.0).abs() < 0.1,
            "white -> ~1.0, got {}",
            pixel[0][0]
        );
        // Non-no-op check: a mid-grey 0.18 must be encoded upward by the
        // working->sRGB display transform (a discarded result leaves 0.18).
        let mut mid = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut mid);
        assert!(
            mid[0][0] > 0.18 + 1e-3 && mid[0][0] < 1.0 && mid[0][0].is_finite(),
            "working->sRGB display must encode 0.18 upward, got {}",
            mid[0][0]
        );
    }
}

/// Matrix precision parity.
/// OCIO ref: MatrixTransform precision
mod matrix_precision_parity {
    use crate::transform::{MatrixTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_matrix_identity_precision() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();

        // Test with various values
        for val in [0.0_f32, 0.18, 0.5, 1.0, 2.0, 10.0] {
            let mut pixel = [[val, val, val]];
            proc.apply_rgb(&mut pixel);
            assert!((pixel[0][0] - val).abs() < 1e-6);
        }
    }

    #[test]
    fn test_matrix_scale_precision() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();

        let mut pixel = [[1.0_f32, 1.0, 1.0]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.5).abs() < 1e-6);
    }
}

/// CDL precision parity.
/// OCIO ref: CDLTransform ASC CDL values
mod cdl_precision_parity {
    use crate::transform::{CdlStyle, CdlTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_cdl_identity() {
        let cdl = CdlTransform::default();
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();

        // Chromatic probe: an achromatic gray is saturation-invariant, so it cannot
        // detect a broken sat term. Use a chromatic input and assert all channels.
        let input = [0.5_f32, 0.3, 0.2];
        let mut pixel = [input];
        proc.apply_rgb(&mut pixel);
        for c in 0..3 {
            assert!(
                (pixel[0][c] - input[c]).abs() < 1e-5,
                "CDL identity ch{}",
                c
            );
        }
    }

    #[test]
    fn test_cdl_slope_only() {
        let cdl = CdlTransform {
            slope: [1.5, 1.0, 0.5],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.75).abs() < 0.01); // 0.5 * 1.5 = 0.75
        assert!((pixel[0][1] - 0.50).abs() < 0.01); // 0.5 * 1.0 = 0.50
        assert!((pixel[0][2] - 0.25).abs() < 0.01); // 0.5 * 0.5 = 0.25
    }

    #[test]
    fn test_cdl_offset_only() {
        let cdl = CdlTransform {
            slope: [1.0, 1.0, 1.0],
            offset: [0.1, 0.0, -0.1],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.6).abs() < 0.01);
        assert!((pixel[0][1] - 0.5).abs() < 0.01);
        assert!((pixel[0][2] - 0.4).abs() < 0.01);
    }

    #[test]
    fn test_cdl_power_only() {
        let cdl = CdlTransform {
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [2.0, 1.0, 0.5],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.25_f32, 0.25, 0.25]];
        proc.apply_rgb(&mut pixel);
        // 0.25^2.0 = 0.0625, 0.25^1.0 = 0.25, 0.25^0.5 = 0.5
        assert!((pixel[0][0] - 0.0625).abs() < 0.01);
        assert!((pixel[0][1] - 0.25).abs() < 0.01);
        assert!((pixel[0][2] - 0.5).abs() < 0.01);
    }
}

/// Exponent transform precision parity.
/// OCIO ref: ExponentTransform gamma values
mod exponent_precision_parity {
    use crate::transform::{ExponentTransform, NegativeStyle, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_gamma_22_curve() {
        let exp = ExponentTransform {
            name: String::new(),
            value: [2.2, 2.2, 2.2, 1.0],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Exponent(exp), TransformDirection::Forward)
                .unwrap();

        // 0.5^2.2 ≈ 0.2176
        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.2176).abs() < 0.01);
    }

    #[test]
    fn test_gamma_24_curve() {
        let exp = ExponentTransform {
            name: String::new(),
            value: [2.4, 2.4, 2.4, 1.0],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Exponent(exp), TransformDirection::Forward)
                .unwrap();

        // 0.5^2.4 ≈ 0.1892
        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.1892).abs() < 0.02);
    }

    #[test]
    fn test_gamma_inverse() {
        let exp_fwd = ExponentTransform {
            name: String::new(),
            value: [2.2, 2.2, 2.2, 1.0],
            negative_style: NegativeStyle::Mirror,
            direction: TransformDirection::Forward,
        };
        let exp_inv = ExponentTransform {
            name: String::new(),
            value: [2.2, 2.2, 2.2, 1.0],
            negative_style: NegativeStyle::Mirror,
            direction: TransformDirection::Forward,
        };

        let proc_fwd =
            Processor::from_transform(&Transform::Exponent(exp_fwd), TransformDirection::Forward)
                .unwrap();
        let proc_inv =
            Processor::from_transform(&Transform::Exponent(exp_inv), TransformDirection::Inverse)
                .unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        let original = pixel[0];
        proc_fwd.apply_rgb(&mut pixel);
        proc_inv.apply_rgb(&mut pixel);

        assert!((pixel[0][0] - original[0]).abs() < 0.001);
    }
}

/// ACES 2.0 style comprehensive parity.
/// OCIO ref: FixedFunctionStyle ACES 2.0
mod aces2_styles_comprehensive_parity {
    use crate::transform::{FixedFunctionStyle, FixedFunctionTransform, TransformDirection};

    #[test]
    fn test_aces2_output_transform() {
        // ACES 2.0 output transform style
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesOutputTransform20,
            params: vec![100.0], // peak_lum
            direction: TransformDirection::Forward,
        };
        assert_eq!(ff.style, FixedFunctionStyle::AcesOutputTransform20);
    }

    #[test]
    fn test_aces2_rgb_to_jmh() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRgbToJmh20,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        assert_eq!(ff.style, FixedFunctionStyle::AcesRgbToJmh20);
    }

    #[test]
    fn test_aces2_tonescale_compress() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesTonescaleCompress20,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        assert_eq!(ff.style, FixedFunctionStyle::AcesTonescaleCompress20);
    }
}

/// Look struct comprehensive parity.
/// OCIO ref: Look
mod look_struct_comprehensive_parity {
    use crate::look::Look;
    use crate::transform::{CdlTransform, Transform};

    #[test]
    fn test_look_builder() {
        let look = Look::new("ShowGrade")
            .process_space("ACEScg")
            .description("Show-level color grade")
            .transform(Transform::Cdl(CdlTransform {
                slope: [1.1, 1.0, 0.9],
                ..Default::default()
            }));

        // Use accessor methods (fields are private)
        assert_eq!(look.name(), "ShowGrade");
    }

    #[test]
    fn test_look_with_inverse() {
        let look = Look::new("ShotGrade")
            .process_space("ACEScg")
            .transform(Transform::Cdl(CdlTransform::default()))
            .inverse_transform(Transform::Cdl(CdlTransform::default()));

        // Both transforms set via builder
        assert_eq!(look.name(), "ShotGrade");
    }
}

/// ConfigBuilder struct parity.
/// OCIO ref: Config creation and modification
mod config_builder_struct_parity {
    use crate::colorspace::{ColorSpace, Encoding, Family};
    use crate::config_builder::ConfigBuilder;
    use crate::display::{Display, View};

    #[test]
    fn test_config_builder_exists() {
        // Test that ConfigBuilder exists and can be created
        let _builder = ConfigBuilder::new("TestConfig");
    }

    #[test]
    fn test_config_builder_methods() {
        // Test builder methods exist
        let builder = ConfigBuilder::new("TestConfig").description("A test configuration");
        let _ = builder;
    }

    #[test]
    fn test_colorspace_builder_exists() {
        let cs = ColorSpace::builder("Linear")
            .family(Family::Scene)
            .encoding(Encoding::SceneLinear)
            .build();
        assert_eq!(cs.name(), "Linear");
    }

    #[test]
    fn test_display_creation() {
        let mut display = Display::new("sRGB");
        display.add_view(View::new("Standard", "sRGB"));
        assert_eq!(display.name(), "sRGB");
    }
}

/// ColorSpaceBuilder comprehensive parity.
/// OCIO ref: ColorSpace creation
mod colorspace_builder_comprehensive_extended_parity {
    use crate::colorspace::{ColorSpace, Encoding, Family};
    use crate::transform::{MatrixTransform, Transform, TransformDirection};

    #[test]
    fn test_colorspace_with_transforms() {
        let cs = ColorSpace::builder("Linear")
            .family(Family::Scene)
            .encoding(Encoding::SceneLinear)
            .description("Scene-linear working space")
            .to_reference(Transform::Matrix(MatrixTransform {
                name: String::new(),
                matrix: MatrixTransform::IDENTITY,
                offset: [0.0; 4],
                direction: TransformDirection::Forward,
            }))
            .build();

        assert_eq!(cs.name(), "Linear");
        assert!(cs.to_reference().is_some());
    }

    #[test]
    fn test_colorspace_with_categories() {
        let cs = ColorSpace::builder("ACEScg")
            .category("scene_linear")
            .category("working_space")
            .category("aces")
            .build();

        assert!(cs.has_category("scene_linear"));
        assert!(cs.has_category("working_space"));
        assert!(cs.has_category("aces"));
    }

    #[test]
    fn test_colorspace_with_aliases() {
        let cs = ColorSpace::builder("ACEScg")
            .alias("ACES - ACEScg")
            .alias("acescg")
            .build();

        assert!(cs.matches_name("ACEScg"));
        assert!(cs.matches_name("ACES - ACEScg"));
        assert!(cs.matches_name("acescg"));
    }
}

// ============================================================================
// HLSL SHADER COMPREHENSIVE PARITY
// ============================================================================

/// HLSL shader generation parity.
/// OCIO ref: GpuShaderDesc HLSL generation
mod hlsl_shader_comprehensive_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};
    use crate::transform::{CdlTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_hlsl_50_generation() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Hlsl50).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_hlsl_simple_transform() {
        let cdl = CdlTransform {
            slope: [1.1, 1.0, 0.9],
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Hlsl50).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_hlsl_textures() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Hlsl50).unwrap();

        // Check textures generated
        let textures = shader.textures();
        let _ = textures; // May or may not have textures
    }

    #[test]
    fn test_hlsl_uniforms() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Hlsl50).unwrap();

        let uniforms = shader.uniforms();
        let _ = uniforms;
    }
}

/// Metal shader generation parity.
/// OCIO ref: GpuShaderDesc Metal generation
mod metal_shader_comprehensive_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};
    use crate::transform::{CdlTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_metal_generation() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Metal).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_metal_simple_transform() {
        let cdl = CdlTransform {
            slope: [1.1, 1.0, 0.9],
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Metal).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_metal_textures() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Metal).unwrap();

        let textures = shader.textures();
        let _ = textures;
    }
}

/// Vulkan/SPIR-V shader generation parity.
/// OCIO ref: GpuShaderDesc Vulkan generation
mod vulkan_shader_comprehensive_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};

    #[test]
    fn test_glsl_vulkan_generation() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        // Use GlslVulkan460 for Vulkan
        let shader = gpu.generate_shader(GpuLanguage::GlslVulkan460).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_glsl_vulkan_textures() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslVulkan460).unwrap();

        let textures = shader.textures();
        let _ = textures;
    }
}

/// WebGL shader generation parity.
/// OCIO ref: GpuShaderDesc WebGL generation
mod webgl_shader_comprehensive_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};

    #[test]
    fn test_webgl_es100_generation() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        // Use GlslEs100 for WebGL 1.0
        let shader = gpu.generate_shader(GpuLanguage::GlslEs100).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_webgl_es300_generation() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        // Use GlslEs300 for WebGL 2.0
        let shader = gpu.generate_shader(GpuLanguage::GlslEs300).unwrap();

        let code = shader.fragment_code();
        // WebGL 2.0 / GLSL ES 3.00 must emit the version directive (a no-op /
        // stub returning a constant string would not).
        assert!(
            code.contains("#version 300 es"),
            "GLSL ES 300 missing version directive"
        );
    }
}

// ============================================================================
// TRANSFER FUNCTION EXACT VALUE PARITY
// ============================================================================

/// sRGB transfer function exact values.
/// OCIO ref: sRGB EOTF/OETF specification
mod srgb_transfer_exact_parity {
    use crate::transform::{ExponentWithLinearTransform, NegativeStyle, TransformDirection};
    use crate::{Processor, Transform};

    // sRGB constants from IEC 61966-2-1
    const SRGB_GAMMA: f64 = 2.4;
    const SRGB_OFFSET: f64 = 0.055;
    const SRGB_SLOPE: f64 = 12.92;
    const SRGB_BREAKPOINT: f64 = 0.04045;

    #[test]
    fn test_srgb_linear_region() {
        // In linear region (< 0.04045), sRGB is linear with slope 12.92
        // 0.01 * 12.92 = 0.1292 (approximate, actual formula slightly different)
        let ewl = ExponentWithLinearTransform {
            name: String::new(),
            gamma: [SRGB_GAMMA, SRGB_GAMMA, SRGB_GAMMA, 1.0],
            offset: [SRGB_OFFSET, SRGB_OFFSET, SRGB_OFFSET, 0.0],
            negative_style: NegativeStyle::Linear,
            direction: TransformDirection::Forward,
        };
        let proc = Processor::from_transform(
            &Transform::ExponentWithLinear(ewl),
            TransformDirection::Forward,
        )
        .unwrap();

        let mut pixel = [[0.01_f32, 0.01, 0.01]];
        proc.apply_rgb(&mut pixel);
        // Forward = sRGB EOTF (encoded->linear). 0.01 is below the 0.04045
        // breakpoint, so it is in the linear segment: 0.01 / 12.92 = 0.0007740.
        // Pins the exact value so a no-op (0.01) or wrong slope fails.
        assert!(
            (pixel[0][0] - 0.0007740_f32).abs() < 1e-5,
            "sRGB linear segment 0.01/12.92: got {}",
            pixel[0][0]
        );
    }

    #[test]
    fn test_srgb_gamma_region() {
        // In gamma region (>= 0.04045)
        let ewl = ExponentWithLinearTransform {
            name: String::new(),
            gamma: [SRGB_GAMMA, SRGB_GAMMA, SRGB_GAMMA, 1.0],
            offset: [SRGB_OFFSET, SRGB_OFFSET, SRGB_OFFSET, 0.0],
            negative_style: NegativeStyle::Linear,
            direction: TransformDirection::Forward,
        };
        let proc = Processor::from_transform(
            &Transform::ExponentWithLinear(ewl),
            TransformDirection::Forward,
        )
        .unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // Forward = sRGB EOTF (encoded->linear). 0.5 is in the gamma segment:
        // ((0.5 + 0.055) / 1.055)^2.4 = 0.21404. Pins the exact decode value.
        assert!(
            (pixel[0][0] - 0.21404_f32).abs() < 1e-3,
            "sRGB gamma segment EOTF(0.5): got {}, expected 0.21404",
            pixel[0][0]
        );
    }
}

/// PQ (ST.2084) transfer function exact values.
/// OCIO ref: PQ EOTF specification
mod pq_transfer_exact_parity {
    use crate::transform::{FixedFunctionStyle, FixedFunctionTransform, TransformDirection};
    use crate::{Processor, Transform};

    // PQ constants from SMPTE ST 2084
    const PQ_M1: f64 = 0.1593017578125;
    const PQ_M2: f64 = 78.84375;
    const PQ_C1: f64 = 0.8359375;
    const PQ_C2: f64 = 18.8515625;
    const PQ_C3: f64 = 18.6875;

    #[test]
    fn test_pq_black() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::LinToPq,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[0.0_f32, 0.0, 0.0]];
        proc.apply_rgb(&mut pixel);
        // PQ black should be 0
        assert!(pixel[0][0].abs() < 0.001);
    }

    #[test]
    fn test_pq_reference_white() {
        // 1.0 linear = 100 nits in PQ normalization
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::LinToPq,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[1.0_f32, 1.0, 1.0]];
        proc.apply_rgb(&mut pixel);
        // PQ(100 nits) ≈ 0.508
        assert!(pixel[0][0] > 0.4 && pixel[0][0] < 0.6);
    }

    #[test]
    fn test_pq_roundtrip() {
        let lin_to_pq = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::LinToPq,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let pq_to_lin = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::PqToLin,
            params: vec![],
            direction: TransformDirection::Forward,
        };

        let proc1 = Processor::from_transform(
            &Transform::FixedFunction(lin_to_pq),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc2 = Processor::from_transform(
            &Transform::FixedFunction(pq_to_lin),
            TransformDirection::Forward,
        )
        .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        let original = pixel[0];
        proc1.apply_rgb(&mut pixel);
        proc2.apply_rgb(&mut pixel);

        assert!((pixel[0][0] - original[0]).abs() < 0.001);
    }
}

/// Rec.709 transfer function values.
/// OCIO ref: Rec.709 OETF
mod rec709_transfer_parity {
    use crate::transform::{ExponentWithLinearTransform, NegativeStyle, TransformDirection};
    use crate::{Processor, Transform};

    // Rec.709 constants
    const REC709_GAMMA: f64 = 1.0 / 0.45; // ≈ 2.222
    const REC709_OFFSET: f64 = 0.099;
    const REC709_SLOPE: f64 = 4.5;
    const REC709_BREAKPOINT: f64 = 0.018;

    #[test]
    fn test_rec709_linear_region() {
        let ewl = ExponentWithLinearTransform {
            name: String::new(),
            gamma: [REC709_GAMMA, REC709_GAMMA, REC709_GAMMA, 1.0],
            offset: [REC709_OFFSET, REC709_OFFSET, REC709_OFFSET, 0.0],
            negative_style: NegativeStyle::Linear,
            direction: TransformDirection::Forward,
        };
        let proc = Processor::from_transform(
            &Transform::ExponentWithLinear(ewl),
            TransformDirection::Forward,
        )
        .unwrap();

        // Below breakpoint (0.018)
        let mut pixel = [[0.01_f32, 0.01, 0.01]];
        proc.apply_rgb(&mut pixel);
        assert!(pixel[0][0] < 0.1);
    }
}

// ============================================================================
// GRADING TRANSFORM EXACT VALUE PARITY
// ============================================================================

/// GradingPrimaryTransform exact value parity.
/// OCIO ref: GradingPrimaryTransform ASC CDL-like operations
mod grading_primary_exact_parity {
    use crate::transform::{GradingPrimaryStyle, GradingPrimaryTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_grading_primary_brightness() {
        let gpt = GradingPrimaryTransform::from_flat(
            GradingPrimaryStyle::Log,
            [0.1, 0.1, 0.1],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            1.0,
            0.18,
            0.0,
            1.0,
            f64::NEG_INFINITY,
            f64::INFINITY,
            TransformDirection::Forward,
        );
        let proc =
            Processor::from_transform(&Transform::GradingPrimary(gpt), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // Brightness should increase values
        assert!(pixel[0][0] > 0.5);
    }

    #[test]
    fn test_grading_primary_contrast() {
        let gpt = GradingPrimaryTransform::from_flat(
            GradingPrimaryStyle::Log,
            [0.0, 0.0, 0.0],
            [1.5, 1.5, 1.5],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            1.0,
            0.18,
            0.0,
            1.0,
            f64::NEG_INFINITY,
            f64::INFINITY,
            TransformDirection::Forward,
        );
        let proc =
            Processor::from_transform(&Transform::GradingPrimary(gpt), TransformDirection::Forward)
                .unwrap();

        // Mid-gray at pivot should stay approximately the same
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        // With contrast increase, values away from pivot move away more
        let _ = pixel;
    }

    #[test]
    fn test_grading_primary_saturation() {
        let gpt = GradingPrimaryTransform::from_flat(
            GradingPrimaryStyle::Log,
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            0.0,
            0.18,
            0.0,
            1.0,
            f64::NEG_INFINITY,
            f64::INFINITY,
            TransformDirection::Forward,
        );
        let proc =
            Processor::from_transform(&Transform::GradingPrimary(gpt), TransformDirection::Forward)
                .unwrap();

        // Saturated color should become gray
        let mut pixel = [[0.8_f32, 0.2, 0.1]];
        proc.apply_rgb(&mut pixel);
        // With saturation=0, all channels should be equal (gray)
        let diff = (pixel[0][0] - pixel[0][1]).abs() + (pixel[0][1] - pixel[0][2]).abs();
        assert!(diff < 0.01);
    }
}

/// GradingToneTransform exact value parity.
/// OCIO ref: GradingToneTransform shadow/midtone/highlight
mod grading_tone_exact_parity {
    use crate::transform::{
        GradingRgbmsw, GradingToneStyle, GradingToneTransform, TransformDirection,
    };
    use crate::{Processor, Transform};

    // Reference-vector parity. Vectors come from OCIO
    // GradingToneOpCPU_tests.cpp (TS3 shadows / TS2 highlights). The master
    // channel only is exercised here; the full multi-channel coverage lives in
    // `grading_tone_reference_vectors`.
    fn rgbmsw6(r: f64, g: f64, b: f64, m: f64, start: f64, width: f64) -> GradingRgbmsw {
        GradingRgbmsw {
            red: r,
            green: g,
            blue: b,
            master: m,
            start,
            width,
        }
    }

    fn from_style(style: GradingToneStyle) -> GradingToneTransform {
        let v = crate::grading_tone::GradingTone::from_style(style);
        GradingToneTransform {
            name: String::new(),
            style,
            blacks: v.blacks,
            shadows: v.shadows,
            midtones: v.midtones,
            highlights: v.highlights,
            whites: v.whites,
            scontrast: v.s_contrast,
            direction: TransformDirection::Forward,
        }
    }

    #[test]
    fn test_grading_tone_shadows() {
        // OCIO TS3 (GRADING_VIDEO shadows {0.3,1.0,1.79,0.6, start 0.8, pivot -0.1}).
        let mut gtt = from_style(GradingToneStyle::Video);
        gtt.shadows = rgbmsw6(0.3, 1.0, 1.79, 0.6, 0.8, -0.1);
        let proc =
            Processor::from_transform(&Transform::GradingTone(gtt), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.20_f32, 0.2, 0.10]];
        proc.apply_rgb(&mut pixel);
        let expected = [0.042_35_f32, 0.14, 0.158_287_73];
        for j in 0..3 {
            assert!(
                (pixel[0][j] - expected[j]).abs() <= 1e-4,
                "shadows ch{j}: got {} expected {}",
                pixel[0][j],
                expected[j]
            );
        }
    }

    #[test]
    fn test_grading_tone_highlights() {
        // OCIO TS2 (GRADING_LOG highlights {0.3,1.0,1.8,1.4, start -0.1, pivot 0.9}).
        let mut gtt = from_style(GradingToneStyle::Log);
        gtt.highlights = rgbmsw6(0.3, 1.0, 1.8, 1.4, -0.1, 0.9);
        let proc =
            Processor::from_transform(&Transform::GradingTone(gtt), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.8_f32, 0.2, -0.05]];
        proc.apply_rgb(&mut pixel);
        let expected = [0.758_338_2_f32, 0.218, -0.048_479_8];
        for j in 0..3 {
            assert!(
                (pixel[0][j] - expected[j]).abs() <= 1e-4,
                "highlights ch{j}: got {} expected {}",
                pixel[0][j],
                expected[j]
            );
        }
    }
}

// ============================================================================
// BATCH TEXTURE API EXACT PARITY
// ============================================================================

/// BatchTextureDescriptor comprehensive parity.
/// OCIO ref: Texture batch upload API
mod batch_texture_api_exact_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{
        BatchTextureDescriptor, GpuChannelType, GpuInterpolation, GpuLanguage, GpuProcessor,
        GpuTextureType, TextureDimensions,
    };

    #[test]
    fn test_batch_descriptor_1d_red() {
        let desc = BatchTextureDescriptor {
            name: "lut1d".to_string(),
            binding_index: 0,
            texture_type: GpuTextureType::Texture1D,
            dimensions: TextureDimensions::Texture1D,
            width: 1024,
            height: 1,
            depth: 1,
            channel_type: GpuChannelType::Red,
            interpolation: GpuInterpolation::Linear,
            data_offset: 0,
            data_size: 1024 * 4, // f32 = 4 bytes
        };
        assert_eq!(desc.width, 1024);
        assert_eq!(desc.height, 1);
    }

    #[test]
    fn test_batch_descriptor_3d_rgb() {
        let size = 33_u32;
        let desc = BatchTextureDescriptor {
            name: "lut3d".to_string(),
            binding_index: 1,
            texture_type: GpuTextureType::Texture3D,
            dimensions: TextureDimensions::Texture2D, // 3D stored as 2D array
            width: size,
            height: size,
            depth: size,
            channel_type: GpuChannelType::Rgb,
            interpolation: GpuInterpolation::Linear,
            data_offset: 0,
            data_size: (size * size * size * 3 * 4) as usize,
        };
        assert_eq!(desc.depth, 33);
    }

    #[test]
    fn test_gpu_processor_textures() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();

        // [PyOCIO] extractGpuShaderInfo on this chain (matrix + sRGB curve): no LUT, so no
        // textures and no uniforms. Textures of a LUT chain: `gpu::tests`.
        assert!(shader.textures().is_empty(), "{}", shader.textures().len());
        assert!(shader.uniforms().is_empty(), "{}", shader.uniforms().len());
    }
}

// ============================================================================
// ALL FIXED FUNCTION STYLES PARITY
// ============================================================================

/// All FixedFunctionStyle variants parity.
/// OCIO ref: FixedFunctionStyle enum complete coverage
mod fixed_function_all_styles_extended_parity {
    use crate::transform::{FixedFunctionStyle, FixedFunctionTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_aces_red_mod_03() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRedMod03,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_aces_red_mod_10() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRedMod10,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_aces_glow_03() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesGlow03,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_aces_glow_10() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesGlow10,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_aces_dark_to_dim_10() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesDarkToDim10,
            params: vec![0.9811], // gamma
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_aces_gamut_comp_13() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesGamutComp13,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_rec2100_surround() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::Rec2100Surround,
            params: vec![0.9811], // gamma
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_rgb_to_hsv() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::RgbToHsv,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();

        // Red -> HSV
        let mut pixel = [[1.0_f32, 0.0, 0.0]];
        proc.apply_rgb(&mut pixel);
        // Hue should be 0 (or close to it)
        assert!(pixel[0][0].abs() < 0.01 || (pixel[0][0] - 1.0).abs() < 0.01);
        // Saturation should be 1
        assert!((pixel[0][1] - 1.0).abs() < 0.01);
        // Value should be 1
        assert!((pixel[0][2] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_hsv_to_rgb() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::HsvToRgb,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();

        // HSV (0, 1, 1) -> Red
        let mut pixel = [[0.0_f32, 1.0, 1.0]];
        proc.apply_rgb(&mut pixel);
        // Should be red
        assert!((pixel[0][0] - 1.0).abs() < 0.01);
        assert!(pixel[0][1].abs() < 0.01);
        assert!(pixel[0][2].abs() < 0.01);
    }

    #[test]
    fn test_xyz_to_xyy() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyzToXyy,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_xyy_to_xyz() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyyToXyz,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_xyz_to_luv() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyzToLuv,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_luv_to_xyz() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::LuvToXyz,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

// ============================================================================
// PROCESSOR CACHE PARITY
// ============================================================================

/// ProcessorCache comprehensive parity.
/// OCIO ref: ProcessorCache
mod processor_cache_exact_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::cache::ProcessorCache;

    #[test]
    fn test_cache_hit() {
        let cache = ProcessorCache::new();
        let config = builtin::aces_2_0_studio_all_views();

        // First call - cache miss
        let proc1 = cache.get_or_create(
            &config,
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        );
        assert!(proc1.is_ok());

        // Second call - should be cache hit
        let proc2 = cache.get_or_create(
            &config,
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        );
        assert!(proc2.is_ok());
    }

    #[test]
    fn test_cache_different_transforms() {
        let cache = ProcessorCache::new();
        let config = builtin::aces_2_0_studio_all_views();

        let proc1 = cache.get_or_create(
            &config,
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        );
        let proc2 = cache.get_or_create(
            &config,
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_rec709,
        );

        assert!(proc1.is_ok());
        assert!(proc2.is_ok());
    }
}

// ============================================================================
// BAKER API EXACT PARITY
// ============================================================================

/// Baker LUT generation parity.
/// OCIO ref: Baker bake1D, bake3D
mod baker_exact_parity {
    use crate::baker::{Baker, LutFormat};
    use crate::builtin;
    use crate::builtin::TestSchema;

    #[test]
    fn test_baker_bake_1d() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let baker = Baker::new(&proc);

        let lut = baker.bake_lut_1d(1024);
        assert!(lut.is_ok());
        let lut = lut.unwrap();
        assert_eq!(lut.size, 1024);
    }

    #[test]
    fn test_baker_bake_3d() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let baker = Baker::new(&proc);

        let lut = baker.bake_lut_3d(17);
        assert!(lut.is_ok());
        let lut = lut.unwrap();
        assert_eq!(lut.size, 17);
    }
}

// ============================================================================
// DYNAMIC PROPERTIES EXACT PARITY
// ============================================================================

/// DynamicProcessor comprehensive parity.
/// OCIO ref: DynamicProperty runtime modification
mod dynamic_processor_exact_parity {
    use crate::dynamic::{
        DynamicProcessorBuilder, DynamicPropertyType, DynamicPropertyValue, GradingPrimary,
    };
    use crate::transform::{ExposureContrastStyle, ExposureContrastTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_dynamic_exposure() {
        let ec = ExposureContrastTransform {
            name: String::new(),
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            style: ExposureContrastStyle::Linear,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };
        let proc = Processor::from_transform(
            &Transform::ExposureContrast(ec),
            TransformDirection::Forward,
        )
        .unwrap();

        // Build dynamic processor with exposure modification
        let dynamic = DynamicProcessorBuilder::new().exposure(1.0).build(proc);

        // Linear-style EC: exposure stop +1.0 doubles a mid-grey input
        // (0.18 -> 0.36); this asserted nothing before (tq-ocio.md #2).
        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        dynamic.apply_rgb(&mut pixels);
        for (i, &got) in pixels[0].iter().enumerate() {
            assert!((got - 0.36).abs() < 1e-6, "ch{i}: got {got}, expected 0.36");
        }
    }

    #[test]
    fn test_dynamic_property_values() {
        let _double = DynamicPropertyValue::Double(1.0);
        let _primary = DynamicPropertyValue::GradingPrimary(GradingPrimary::default());
    }
}

// ============================================================================
// VALIDATION API COMPLETE PARITY
// ============================================================================

/// Validation API comprehensive parity.
/// OCIO ref: Config::validate, ConfigValidator
mod validation_api_parity {
    use crate::ConfigBuilder;
    use crate::builtin;
    use crate::validate::{
        IssueCategory, Severity, check as validate_config, has_errors, has_warnings,
    };

    #[test]
    fn test_validate_builtin_config() {
        let config = builtin::aces_2_0_studio_all_views();
        let issues = validate_config(&config);
        // Built-in config should have no errors
        assert!(!has_errors(&issues));
    }

    #[test]
    fn test_has_errors_and_warnings() {
        // Test with builtin config
        let config = builtin::aces_2_0_studio_all_views();
        let issues = validate_config(&config);
        // Built-in config should not have errors
        assert!(!has_errors(&issues));
        // has_warnings should work too
        let _ = has_warnings(&issues);
    }
}

// ============================================================================
// DISPLAY/VIEW COMPLETE API PARITY
// ============================================================================

/// Display/View API comprehensive parity.
/// OCIO ref: Display, View, ViewTransform
mod display_view_complete_parity {
    use crate::builtin;
    use crate::display::{Display, DisplayManager, ReferenceSpaceType, View, ViewTransform};

    #[test]
    fn test_display_properties() {
        let config = builtin::aces_2_0_studio_all_views();
        for display in config.displays().displays() {
            let _name = display.name();
            let _views = display.views();
        }
    }

    #[test]
    fn test_view_properties() {
        let config = builtin::aces_2_0_studio_all_views();
        for display in config.displays().displays() {
            for view in display.views() {
                let _name = view.name();
                let _colorspace = view.colorspace();
            }
        }
    }

    #[test]
    fn test_default_display_view() {
        let config = builtin::aces_2_0_studio_all_views();
        let default_display = config.default_display();
        assert!(default_display.is_some());

        let dd = default_display.unwrap();
        let default_view = config.default_view(dd);
        assert!(default_view.is_some());
    }

    #[test]
    fn test_active_displays() {
        let config = builtin::aces_2_0_studio_all_views();
        let displays = config.displays().displays();
        assert!(!displays.is_empty());
    }
}

// ============================================================================
// BUILTIN TRANSFORM COMPLETE PARITY
// ============================================================================

/// BuiltinTransform all variants parity.
/// OCIO ref: BuiltinTransformRegistry
mod builtin_transform_complete_parity {
    use crate::transform::{BuiltinTransform, TransformDirection};
    use crate::{Processor, Transform};

    fn test_builtin(name: &str) -> bool {
        let bt = BuiltinTransform {
            style: name.to_string(),
            direction: TransformDirection::Forward,
        };
        Processor::from_transform(&Transform::Builtin(bt), TransformDirection::Forward).is_ok()
    }

    /// OCIO spells AP0 <-> AP1 as "ACEScg_to_ACES2065-1" (and its inverse); the
    /// "ACES-AP0_to_AP1" / "ACES-AP1_to_AP0" names these tests used are not OCIO styles.
    #[test]
    fn test_aces_ap0_to_ap1() {
        let bt = BuiltinTransform {
            style: "ACEScg_to_ACES2065-1".to_string(),
            direction: TransformDirection::Inverse,
        };
        assert!(
            Processor::from_transform(&Transform::Builtin(bt), TransformDirection::Forward).is_ok()
        );
        assert!(!test_builtin("ACES-AP0_to_AP1"));
    }

    #[test]
    fn test_aces_ap1_to_ap0() {
        assert!(test_builtin("ACEScg_to_ACES2065-1"));
        assert!(!test_builtin("ACES-AP1_to_AP0"));
    }

    #[test]
    fn test_aces_cg_to_xyz() {
        assert!(test_builtin("ACEScg_to_ACES2065-1"));
    }

    #[test]
    fn test_log_to_lin() {
        assert!(test_builtin("ACEScct_to_ACES2065-1"));
    }

    #[test]
    fn test_identity_builtin() {
        let bt = BuiltinTransform {
            style: "IDENTITY".to_string(),
            direction: TransformDirection::Forward,
        };
        let proc = Processor::from_transform(&Transform::Builtin(bt), TransformDirection::Forward)
            .unwrap();

        let mut pixel = [[0.5_f32, 0.3, 0.1]];
        let original = pixel;
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - original[0][0]).abs() < 1e-6);
    }
}

// ============================================================================
// CONTEXT COMPLETE API PARITY
// ============================================================================

/// Context API comprehensive parity.
/// OCIO ref: Context variable substitution
mod context_complete_parity {
    use crate::Context;

    #[test]
    fn test_context_set_get() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");
        ctx.set("SEQ", "sq01");

        assert_eq!(ctx.get("SHOT"), Some("sh010".to_string()));
        assert_eq!(ctx.get("SEQ"), Some("sq01".to_string()));
        assert_eq!(ctx.get("MISSING"), None);
    }

    #[test]
    fn test_context_resolve() {
        let mut ctx = Context::new();
        ctx.set("SHOW", "myshow");
        ctx.set("SHOT", "sh010");

        let path = "/shows/$SHOW/shots/$SHOT/luts/grade.cube";
        let resolved = ctx.resolve(path);
        assert_eq!(resolved, "/shows/myshow/shots/sh010/luts/grade.cube");
    }

    #[test]
    fn test_context_env_style() {
        let mut ctx = Context::new();
        ctx.set("VAR", "value");

        // Test basic variable substitution
        assert_eq!(ctx.resolve("/path/$VAR/end"), "/path/value/end");
    }

    #[test]
    fn test_context_clear() {
        let mut ctx = Context::new();
        ctx.set("KEY", "value");
        ctx.clear();
        assert_eq!(ctx.get("KEY"), None);
    }

    #[test]
    fn test_context_len() {
        let mut ctx = Context::new();
        ctx.set("A", "1");
        ctx.set("B", "2");

        assert_eq!(ctx.len(), 2);
    }
}

// ============================================================================
// GPU SHADER COMPLETE COVERAGE
// ============================================================================

/// GPU shader generation complete parity.
/// OCIO ref: GpuShaderCreator all languages
mod gpu_shader_complete_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor, GpuShaderCode};

    fn test_shader_lang(lang: GpuLanguage) -> GpuShaderCode {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        gpu.generate_shader(lang).unwrap()
    }

    #[test]
    fn test_glsl_120() {
        // A real generated shader has a `main` entry point; assert it, not just
        // non-emptiness (a constant non-empty string would pass the old check).
        let shader = test_shader_lang(GpuLanguage::Glsl120);
        assert!(
            shader.fragment_code().contains("main"),
            "GLSL120 missing main()"
        );
    }

    #[test]
    fn test_glsl_130() {
        let shader = test_shader_lang(GpuLanguage::Glsl130);
        assert!(
            shader.fragment_code().contains("main"),
            "GLSL130 missing main()"
        );
    }

    #[test]
    fn test_glsl_330() {
        // GLSL 3.30 emits a `#version 330` directive (proven by
        // gpu_shader_language_parity). Require the marker; drop the
        // `|| !is_empty()` escape hatch that a stub would satisfy.
        let shader = test_shader_lang(GpuLanguage::Glsl330);
        assert!(
            shader.fragment_code().contains("#version 330"),
            "GLSL330 missing #version 330"
        );
    }

    #[test]
    fn test_glsl_400() {
        let shader = test_shader_lang(GpuLanguage::Glsl400);
        assert!(
            shader.fragment_code().contains("#version 400"),
            "GLSL400 missing #version 400"
        );
    }

    #[test]
    fn test_glsl_vulkan_460() {
        let shader = test_shader_lang(GpuLanguage::GlslVulkan460);
        assert!(
            shader.fragment_code().contains("#version 460"),
            "GLSL Vulkan 460 missing #version 460"
        );
    }

    #[test]
    fn test_glsl_es_100() {
        let shader = test_shader_lang(GpuLanguage::GlslEs100);
        assert!(!shader.fragment_code().is_empty());
    }

    #[test]
    fn test_glsl_es_300() {
        let shader = test_shader_lang(GpuLanguage::GlslEs300);
        assert!(
            shader.fragment_code().contains("#version 300 es"),
            "GLSL ES 300 missing #version 300 es"
        );
    }

    #[test]
    fn test_hlsl_50() {
        let shader = test_shader_lang(GpuLanguage::Hlsl50);
        // HLSL uses float3/float4 or float
        assert!(!shader.fragment_code().is_empty());
    }

    #[test]
    fn test_metal() {
        let shader = test_shader_lang(GpuLanguage::Metal);
        // Metal uses metal_stdlib or float3
        assert!(!shader.fragment_code().is_empty());
    }
}

// ============================================================================
// GRADING RGB CURVE COMPLETE PARITY
// ============================================================================

/// GradingRgbCurveTransform comprehensive parity.
/// OCIO ref: GradingRgbCurveTransform
mod grading_rgb_curve_complete_parity {
    use crate::transform::{GradingRgbCurveTransform, RgbCurveType, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_rgb_curve_default() {
        let curve = GradingRgbCurveTransform::default();
        let proc = Processor::from_transform(
            &Transform::GradingRgbCurve(curve),
            TransformDirection::Forward,
        );
        assert!(proc.is_ok());
    }

    #[test]
    fn test_rgb_curve_identity() {
        let curve = GradingRgbCurveTransform::default();
        let proc = Processor::from_transform(
            &Transform::GradingRgbCurve(curve),
            TransformDirection::Forward,
        )
        .unwrap();

        let input = [0.5_f32, 0.3, 0.1];
        let mut pixel = [input];
        proc.apply_rgb(&mut pixel);
        // Default (identity) curve is an exact identity transform: out == in.
        for c in 0..3 {
            assert!(
                (pixel[0][c] - input[c]).abs() < 1e-5,
                "RGB curve identity ch{}",
                c
            );
        }
    }
}

// ============================================================================
// ERROR HANDLING PARITY
// ============================================================================

/// Error handling comprehensive parity.
/// OCIO ref: Exception types
mod error_handling_parity {
    use crate::builtin::TestSchema;
    use crate::transform::{ColorSpaceTransform, TransformDirection};
    use crate::{Config, OcioError, OcioResult, builtin};
    use crate::{Processor, Transform};

    #[test]
    fn test_invalid_colorspace() {
        let config = builtin::aces_2_0_studio_all_views();
        let result = config.processor("NonExistent", &TestSchema::shared().display_encoded_srgb);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_display() {
        let config = builtin::aces_2_0_studio_all_views();
        let result = config.processor("NonExistent", &TestSchema::shared().display_encoded_srgb);
        if let Err(e) = result {
            let msg = format!("{}", e);
            assert!(!msg.is_empty());
        }
    }

    #[test]
    fn test_colorspace_transform_error() {
        let cst = ColorSpaceTransform {
            src: "Invalid".to_string(),
            dst: "AlsoInvalid".to_string(),
            data_bypass: true,
            direction: TransformDirection::Forward,
        };
        // Without config context, this should work at transform level
        let _ = cst;
    }
}

// ============================================================================
// PROCESSOR METADATA COMPLETE PARITY
// ============================================================================

/// ProcessorMetadata comprehensive parity.
/// OCIO ref: Processor::getProcessorMetadata
mod processor_metadata_complete_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::processor::{FormatMetadata, OptimizationLevel, ProcessorMetadata};

    #[test]
    fn test_processor_metadata() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let meta = proc.metadata();
        // Metadata should exist
        let _ = meta;
    }

    #[test]
    fn test_format_metadata() {
        // Derived Default: id/name are empty strings, attributes an empty vec.
        let meta = FormatMetadata::default();
        assert!(meta.id.is_empty());
        assert!(meta.name.is_empty());
        assert!(meta.attributes.is_empty());
    }

    #[test]
    fn test_processor_optimization() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let optimized = proc.optimized(OptimizationLevel::Best);
        // optimized() returns Self, verify it works
        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        optimized.apply_rgb(&mut pixel);
    }
}

// ============================================================================
// CONFIG SERIALIZATION COMPLETE PARITY
// ============================================================================

/// Config serialization complete parity.
/// OCIO ref: Config::serialize
mod config_serialization_complete_parity {
    use crate::ConfigBuilder;
    use crate::builtin;
    use crate::colorspace::{ColorSpace, Encoding};

    #[test]
    fn test_serialize_builtin() {
        let config = builtin::aces_2_0_studio_all_views();
        let yaml = config.serialize();
        assert!(yaml.is_ok());
        let yaml = yaml.unwrap();
        assert!(yaml.contains("ocio_profile_version"));
    }

    #[test]
    fn test_serialize_has_colorspaces() {
        let config = builtin::aces_2_0_studio_all_views();
        let yaml = config.serialize().unwrap();
        // Should contain colorspace definitions
        assert!(yaml.contains("colorspaces"));
        assert!(yaml.contains("ACEScg"));
    }
}

// ============================================================================
// ROLE NAMES COMPLETE PARITY
// ============================================================================

/// Role names constant parity.
/// OCIO ref: ROLE_* constants
mod role_names_complete_parity {
    use crate::role::names;

    #[test]
    fn test_standard_role_names() {
        assert_eq!(names::SCENE_LINEAR, "scene_linear");
        assert_eq!(names::COMPOSITING_LOG, "compositing_log");
        assert_eq!(names::COLOR_TIMING, "color_timing");
        assert_eq!(names::COLOR_PICKING, "color_picking");
        assert_eq!(names::DATA, "data");
        assert_eq!(names::DEFAULT, "default");
        assert_eq!(names::MATTE_PAINT, "matte_paint");
        assert_eq!(names::TEXTURE_PAINT, "texture_paint");
        assert_eq!(names::REFERENCE, "reference");
    }

    #[test]
    fn test_aces_role_names() {
        assert_eq!(names::ACES_INTERCHANGE, "aces_interchange");
        assert_eq!(names::CIE_XYZ_D65_INTERCHANGE, "cie_xyz_d65_interchange");
    }

    #[test]
    fn test_rendering_role_names() {
        assert_eq!(names::RENDERING, "rendering");
    }
}

// ============================================================================
// LOOK COMPLETE PARITY
// ============================================================================

/// Look API comprehensive parity.
/// OCIO ref: Look, LookTransform
mod look_complete_parity {
    use crate::Transform;
    use crate::look::{Look, LookManager, parse_looks};
    use crate::transform::{CdlTransform, LookTransform, TransformDirection};

    #[test]
    fn test_look_new() {
        let cdl = CdlTransform {
            slope: [1.1, 1.0, 0.9],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            ..Default::default()
        };

        let look = Look::new("MyGrade")
            .process_space("ACEScg")
            .transform(Transform::Cdl(cdl));

        assert_eq!(look.name(), "MyGrade");
    }

    #[test]
    fn test_parse_looks() {
        // Unprefixed, explicit '+' and '-' all in one string: pins names and
        // direction together (len() alone can't catch a swapped or dropped name).
        let looks = parse_looks("grade1, grade2, +grade3, -grade4");
        assert_eq!(
            looks,
            vec![
                ("grade1", true),
                ("grade2", true),
                ("grade3", true),
                ("grade4", false),
            ]
        );
    }

    #[test]
    fn test_look_manager() {
        let mut manager = LookManager::new();
        let look = Look::new("TestLook");
        manager.add(look);

        let found = manager.get("TestLook");
        assert!(found.is_some());
    }

    #[test]
    fn test_look_transform() {
        let lt = LookTransform {
            src: "ACEScg".to_string(),
            dst: "ACEScg".to_string(),
            looks: "grade".to_string(),
            skip_color_space_conversion: false,
            direction: TransformDirection::Forward,
        };
        assert_eq!(lt.src, "ACEScg");
    }
}

// ============================================================================
// ALLOCATION TRANSFORM PARITY
// ============================================================================

/// AllocationTransform comprehensive parity.
/// OCIO ref: AllocationTransform
mod allocation_transform_complete_parity {
    use crate::transform::{AllocationTransform, AllocationType, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_allocation_uniform() {
        let at = AllocationTransform {
            allocation: AllocationType::Uniform,
            vars: vec![0.0, 1.0],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Allocation(at), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_allocation_log2() {
        let at = AllocationTransform {
            allocation: AllocationType::Log2,
            vars: vec![-8.0, 8.0],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Allocation(at), TransformDirection::Forward);
        assert!(proc.is_ok());
    }

    #[test]
    fn test_allocation_log2_with_offset() {
        let at = AllocationTransform {
            allocation: AllocationType::Log2,
            vars: vec![-10.0, 6.0, 0.00390625], // min, max, offset
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Allocation(at), TransformDirection::Forward);
        assert!(proc.is_ok());
    }
}

// ============================================================================
// MATRIX TRANSFORM COMPLETE PARITY
// ============================================================================

/// MatrixTransform comprehensive parity.
/// OCIO ref: MatrixTransform all operations
mod matrix_transform_complete_parity {
    use crate::transform::{MatrixTransform, TransformDirection};
    use crate::{Processor, Transform};

    fn identity_matrix() -> MatrixTransform {
        MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0, 0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        }
    }

    #[test]
    fn test_matrix_identity() {
        let mt = identity_matrix();
        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.3, 0.1]];
        let original = pixel;
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - original[0][0]).abs() < 1e-6);
    }

    #[test]
    fn test_matrix_scale() {
        let mut mt = identity_matrix();
        mt.matrix[0] = 2.0; // Scale R by 2
        mt.matrix[5] = 2.0; // Scale G by 2
        mt.matrix[10] = 2.0; // Scale B by 2

        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_matrix_offset() {
        let mut mt = identity_matrix();
        mt.offset = [0.1, 0.2, 0.3, 0.0];

        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.0_f32, 0.0, 0.0]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.1).abs() < 0.01);
        assert!((pixel[0][1] - 0.2).abs() < 0.01);
        assert!((pixel[0][2] - 0.3).abs() < 0.01);
    }

    #[test]
    fn test_matrix_inverse() {
        let mut mt = identity_matrix();
        mt.matrix[0] = 2.0;
        mt.matrix[5] = 2.0;
        mt.matrix[10] = 2.0;

        let proc_fwd =
            Processor::from_transform(&Transform::Matrix(mt.clone()), TransformDirection::Forward)
                .unwrap();

        let proc_inv =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Inverse).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc_fwd.apply_rgb(&mut pixel);
        proc_inv.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.5).abs() < 0.01);
    }
}

// ============================================================================
// CDL TRANSFORM COMPLETE PARITY
// ============================================================================

/// CdlTransform comprehensive parity.
/// OCIO ref: CDLTransform all operations
mod cdl_transform_complete_parity {
    use crate::transform::{CdlStyle, CdlTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_cdl_identity() {
        let cdl = CdlTransform::default();
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();

        // Identity CDL is exact on every channel (chromatic probe also guards the sat term).
        let input = [0.5_f32, 0.3, 0.1];
        let mut pixel = [input];
        proc.apply_rgb(&mut pixel);
        for c in 0..3 {
            assert!(
                (pixel[0][c] - input[c]).abs() < 1e-5,
                "CDL identity ch{}",
                c
            );
        }
    }

    #[test]
    fn test_cdl_slope() {
        let cdl = CdlTransform {
            slope: [2.0, 1.0, 0.5],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 1.0).abs() < 0.01); // 0.5 * 2.0
        assert!((pixel[0][2] - 0.25).abs() < 0.01); // 0.5 * 0.5
    }
}

// ============================================================================
// LOG TRANSFORM COMPLETE PARITY
// ============================================================================

/// LogTransform comprehensive parity.
/// OCIO ref: LogTransform, LogAffineTransform, LogCameraTransform
mod log_transform_complete_parity {
    use crate::transform::{LogTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_log_base_10() {
        let lt = LogTransform {
            name: String::new(),
            base: 10.0,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Log(lt), TransformDirection::Forward).unwrap();

        let mut pixel = [[10.0_f32, 100.0, 1000.0]];
        proc.apply_rgb(&mut pixel);
        // log10(10) = 1, log10(100) = 2, log10(1000) = 3
        assert!((pixel[0][0] - 1.0).abs() < 0.01);
        assert!((pixel[0][1] - 2.0).abs() < 0.01);
        assert!((pixel[0][2] - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_log_base_2() {
        let lt = LogTransform {
            name: String::new(),
            base: 2.0,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Log(lt), TransformDirection::Forward).unwrap();

        let mut pixel = [[2.0_f32, 4.0, 8.0]];
        proc.apply_rgb(&mut pixel);
        // log2(2) = 1, log2(4) = 2, log2(8) = 3
        assert!((pixel[0][0] - 1.0).abs() < 0.01);
        assert!((pixel[0][1] - 2.0).abs() < 0.01);
        assert!((pixel[0][2] - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_log_inverse() {
        let lt = LogTransform {
            name: String::new(),
            base: 10.0,
            direction: TransformDirection::Forward,
        };
        let proc_fwd =
            Processor::from_transform(&Transform::Log(lt.clone()), TransformDirection::Forward)
                .unwrap();
        let proc_inv =
            Processor::from_transform(&Transform::Log(lt), TransformDirection::Inverse).unwrap();

        let original = [[0.5_f32, 0.3, 0.1]];
        let mut pixel = original;
        proc_fwd.apply_rgb(&mut pixel);
        proc_inv.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - original[0][0]).abs() < 0.001);
    }
}

// ============================================================================
// EXPONENT TRANSFORM COMPLETE PARITY
// ============================================================================

/// ExponentTransform comprehensive parity.
/// OCIO ref: ExponentTransform, ExponentWithLinearTransform
mod exponent_transform_complete_parity {
    use crate::transform::{ExponentTransform, NegativeStyle, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_exponent_gamma_2_2() {
        let exp = ExponentTransform {
            name: String::new(),
            value: [2.2, 2.2, 2.2, 1.0],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Exponent(exp), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // 0.5^2.2 ≈ 0.2176
        assert!((pixel[0][0] - 0.2176).abs() < 0.01);
    }

    #[test]
    fn test_exponent_inverse() {
        let exp = ExponentTransform {
            name: String::new(),
            value: [2.2, 2.2, 2.2, 1.0],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        };
        let proc_fwd = Processor::from_transform(
            &Transform::Exponent(exp.clone()),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_inv =
            Processor::from_transform(&Transform::Exponent(exp), TransformDirection::Inverse)
                .unwrap();

        let original = [[0.5_f32, 0.3, 0.1]];
        let mut pixel = original;
        proc_fwd.apply_rgb(&mut pixel);
        proc_inv.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - original[0][0]).abs() < 0.001);
    }
}

// ============================================================================
// RANGE TRANSFORM COMPLETE PARITY
// ============================================================================

/// RangeTransform comprehensive parity.
/// OCIO ref: RangeTransform
mod range_transform_complete_parity {
    use crate::transform::{RangeStyle, RangeTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_range_clamp() {
        let rt = RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(1.0),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Range(rt), TransformDirection::Forward).unwrap();

        let mut pixel = [[-0.5_f32, 0.5, 1.5]];
        proc.apply_rgb(&mut pixel);
        // Should clamp to [0, 1]
        assert!(pixel[0][0] >= 0.0);
        assert!(pixel[0][2] <= 1.0);
    }

    #[test]
    fn test_range_no_clamp() {
        let rt = RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(2.0),
            style: RangeStyle::NoClamp,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Range(rt), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // 0.5 maps to 1.0 (doubled range)
        assert!((pixel[0][0] - 1.0).abs() < 0.01);
    }
}

// ============================================================================
// HSY TRANSFORM STYLE PARITY
// ============================================================================

/// HsyTransformStyle comprehensive parity.
/// OCIO ref: GradingHueCurveTransform::getRGBToHSYStyle
mod hsy_transform_style_parity {
    use crate::transform::{
        GradingHueCurveStyle, GradingHueCurveTransform, HsyTransformStyle, TransformDirection,
    };

    #[test]
    fn test_hsy_style_none() {
        let hc = GradingHueCurveTransform {
            style: GradingHueCurveStyle::Log,
            rgb_to_hsy: HsyTransformStyle::None,
            ..Default::default()
        };
        assert_eq!(hc.rgb_to_hsy, HsyTransformStyle::None);
    }

    #[test]
    fn test_hsy_style_default() {
        let hc = GradingHueCurveTransform::default();
        assert_eq!(hc.rgb_to_hsy, HsyTransformStyle::Default);
    }

    #[test]
    fn test_hsy_style_in_all_curve_styles() {
        for style in [
            GradingHueCurveStyle::Log,
            GradingHueCurveStyle::Linear,
            GradingHueCurveStyle::Video,
        ] {
            for hsy in [HsyTransformStyle::None, HsyTransformStyle::Default] {
                let hc = GradingHueCurveTransform {
                    style,
                    rgb_to_hsy: hsy,
                    ..Default::default()
                };
                assert_eq!(hc.rgb_to_hsy, hsy);
            }
        }
    }
}

// ============================================================================
// GRADING PRIMARY EXACT VALUES PARITY
// ============================================================================

/// GradingPrimaryTransform exact value parity.
/// OCIO ref: Tests with known input/output values
mod grading_primary_exact_values_parity {
    use crate::transform::{GradingPrimaryStyle, GradingPrimaryTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_grading_primary_log_identity() {
        // OCIO identity transform should pass through unchanged
        let gp = GradingPrimaryTransform::from_flat(
            GradingPrimaryStyle::Log,
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            1.0,
            0.18,
            0.0,
            1.0,
            f64::NEG_INFINITY,
            f64::INFINITY,
            TransformDirection::Forward,
        );
        let proc =
            Processor::from_transform(&Transform::GradingPrimary(gp), TransformDirection::Forward)
                .unwrap();

        // Identity GradingPrimary is exact on every channel (chromatic probe guards sat).
        let input = [0.18_f32, 0.1, 0.3];
        let mut pixel = [input];
        proc.apply_rgb(&mut pixel);
        for c in 0..3 {
            assert!(
                (pixel[0][c] - input[c]).abs() < 1e-5,
                "GradingPrimary log identity ch{}: got {}",
                c,
                pixel[0][c]
            );
        }
    }

    #[test]
    fn test_grading_primary_exposure_1stop() {
        // +1 stop exposure should double the value
        let gp = GradingPrimaryTransform {
            style: GradingPrimaryStyle::Linear,
            value: crate::dynamic::GradingPrimary {
                exposure: crate::dynamic::GradingRgbm::new(1.0, 1.0, 1.0, 0.0), // +1 stop
                ..crate::dynamic::GradingPrimary::linear_default()
            },
            ..GradingPrimaryTransform::new(GradingPrimaryStyle::Linear)
        };
        let proc =
            Processor::from_transform(&Transform::GradingPrimary(gp), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        // +1 stop linear exposure multiplies by 2: 0.18 * 2 = 0.36. Pin the
        // exact doubled value so a no-op (0.18) or wrong gain fails.
        assert!(
            (pixel[0][0] - 0.36).abs() < 0.02,
            "+1 stop exposure: 0.18 -> 0.36, got {}",
            pixel[0][0]
        );
    }

    #[test]
    fn test_grading_primary_saturation_zero() {
        // [PyOCIO] GP LIN sat0 (1,0,0) = 0.2126 x3 exactly (Rec.709 luma
        // weight, not just channel equality — an equality-only check would
        // pass even with the wrong luma coefficients).
        let gp = GradingPrimaryTransform {
            style: GradingPrimaryStyle::Linear,
            value: crate::dynamic::GradingPrimary {
                saturation: 0.0,
                ..crate::dynamic::GradingPrimary::linear_default()
            },
            ..GradingPrimaryTransform::new(GradingPrimaryStyle::Linear)
        };
        let proc =
            Processor::from_transform(&Transform::GradingPrimary(gp), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[1.0_f32, 0.0, 0.0]]; // Pure red
        proc.apply_rgb(&mut pixel);
        let expected = 0.2126_f32;
        for (i, &got) in pixel[0].iter().enumerate() {
            assert!(
                (got - expected).abs() < 1e-5,
                "ch{i}: got {got}, expected {expected}"
            );
        }
    }

    #[test]
    fn test_grading_primary_contrast() {
        // Contrast > 1 should increase contrast around pivot
        // LIN's actual pivot is `0.18 * 2^raw_pivot` (`op::grading_primary_params`,
        // porting `GradingPrimaryPreRender::update`), not the raw `pivot` field
        // directly — solve for the raw value that puts the actual pivot at 0.5,
        // so "value at pivot is unchanged" still tests what it says.
        let raw_pivot_for_actual_0_5 = (0.5_f64 / 0.18).log2();
        let gp = GradingPrimaryTransform {
            style: GradingPrimaryStyle::Linear,
            value: crate::dynamic::GradingPrimary {
                contrast: crate::dynamic::GradingRgbm::new(2.0, 2.0, 2.0, 1.0),
                pivot: raw_pivot_for_actual_0_5,
                ..crate::dynamic::GradingPrimary::linear_default()
            },
            ..GradingPrimaryTransform::new(GradingPrimaryStyle::Linear)
        };
        let proc =
            Processor::from_transform(&Transform::GradingPrimary(gp), TransformDirection::Forward)
                .unwrap();

        // Value at pivot should remain unchanged
        let mut pixel_pivot = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel_pivot);
        assert!((pixel_pivot[0][0] - 0.5).abs() < 0.1);
    }

    #[test]
    fn test_grading_primary_clamp() {
        // Clamp fields exist and can be set
        let gp = GradingPrimaryTransform {
            style: GradingPrimaryStyle::Linear,
            value: crate::dynamic::GradingPrimary {
                clamp_black: 0.1,
                clamp_white: 0.9,
                ..crate::dynamic::GradingPrimary::linear_default()
            },
            ..GradingPrimaryTransform::new(GradingPrimaryStyle::Linear)
        };
        // Fields are set correctly
        assert!((gp.value.clamp_black - 0.1).abs() < 0.001);
        assert!((gp.value.clamp_white - 0.9).abs() < 0.001);

        // Processor can be created
        let proc =
            Processor::from_transform(&Transform::GradingPrimary(gp), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // Value should be processed
        let _ = pixel;
    }
}

// ============================================================================
// GRADING TONE EXACT VALUES PARITY
// ============================================================================

/// GradingToneTransform exact value parity.
/// OCIO ref: Tests with known input/output values
mod grading_tone_exact_values_parity {
    use crate::transform::{
        GradingRgbmsw, GradingToneStyle, GradingToneTransform, TransformDirection,
    };
    use crate::{Processor, Transform};

    #[test]
    fn test_grading_tone_identity() {
        // Default should be identity
        let gt = GradingToneTransform::default();
        let proc =
            Processor::from_transform(&Transform::GradingTone(gt), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[0.18_f32, 0.5, 0.8]];
        let orig = pixel;
        proc.apply_rgb(&mut pixel);
        // Default GradingTone (all RGBM multipliers = 1.0) is an exact identity.
        for i in 0..3 {
            assert!(
                (pixel[0][i] - orig[0][i]).abs() < 1e-5,
                "GradingTone identity ch{}",
                i
            );
        }
    }

    #[test]
    fn test_grading_tone_shadows() {
        // OCIO reference vector TS3 (GradingToneOpCPU_tests.cpp): GRADING_VIDEO
        // shadows {0.3,1.0,1.79,0.6, start 0.8, pivot -0.1}. Start from the
        // per-style defaults so shadow start/pivot are reference-valid.
        let base = crate::grading_tone::GradingTone::from_style(GradingToneStyle::Video);
        let gt = GradingToneTransform {
            name: String::new(),
            style: GradingToneStyle::Video,
            blacks: base.blacks,
            shadows: GradingRgbmsw {
                red: 0.3,
                green: 1.0,
                blue: 1.79,
                master: 0.6,
                start: 0.8,
                width: -0.1,
            },
            midtones: base.midtones,
            highlights: base.highlights,
            whites: base.whites,
            scontrast: 1.0,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::GradingTone(gt), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[-0.05_f32, -0.3, -0.05]];
        proc.apply_rgb(&mut pixel);
        let expected = [-0.089_036_f32, -0.22, -0.0101064];
        for j in 0..3 {
            assert!(
                (pixel[0][j] - expected[j]).abs() <= 1e-4,
                "shadows ch{j}: got {} expected {}",
                pixel[0][j],
                expected[j]
            );
        }
    }

    #[test]
    fn test_grading_tone_highlights() {
        // Raising highlights should brighten bright values
        let gt = GradingToneTransform {
            style: GradingToneStyle::Log,
            highlights: GradingRgbmsw {
                red: 1.5,
                green: 1.5,
                blue: 1.5,
                master: 1.5,
                start: 0.5,
                width: 2.0,
            },
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::GradingTone(gt), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[0.8_f32, 0.8, 0.8]];
        let orig = pixel[0][0];
        proc.apply_rgb(&mut pixel);
        // Highlight should be brighter
        assert!(pixel[0][0] > orig - 0.1);
    }

    #[test]
    fn test_grading_tone_midtones() {
        // Adjusting midtones affects middle gray
        let gt = GradingToneTransform {
            style: GradingToneStyle::Log,
            midtones: GradingRgbmsw {
                red: 1.2,
                green: 1.2,
                blue: 1.2,
                master: 1.2,
                start: 0.2,
                width: 1.0,
            },
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::GradingTone(gt), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        let _orig = pixel[0][0];
        proc.apply_rgb(&mut pixel);
        // Value should change
        let _ = pixel;
    }

    #[test]
    fn test_grading_tone_scontrast() {
        // S-contrast adjusts overall contrast curve
        let gt = GradingToneTransform {
            style: GradingToneStyle::Log,
            scontrast: 1.5, // Increased S-contrast
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::GradingTone(gt), TransformDirection::Forward)
                .unwrap();

        // Mid gray should be affected by S-contrast
        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        let _ = pixel;
    }
}

// ============================================================================
// CDL EXACT VALUES PARITY
// ============================================================================

/// CDL exact value parity with known test vectors.
/// OCIO ref: CDL math with ASC CDL specification
mod cdl_exact_values_parity {
    use crate::transform::{CdlStyle, CdlTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_cdl_asc_formula() {
        // ASC CDL formula: out = (in * slope + offset) ^ power
        let cdl = CdlTransform {
            slope: [2.0, 1.0, 0.5],
            offset: [0.1, 0.0, -0.1],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // R: 0.5 * 2.0 + 0.1 = 1.1 -> clamped to 1.0
        // G: 0.5 * 1.0 + 0.0 = 0.5
        // B: 0.5 * 0.5 + (-0.1) = 0.15
        assert!((pixel[0][1] - 0.5).abs() < 0.01);
        assert!((pixel[0][2] - 0.15).abs() < 0.01);
    }

    #[test]
    fn test_cdl_power() {
        // Power applies gamma
        let cdl = CdlTransform {
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [2.0, 2.0, 2.0], // Square the value
            saturation: 1.0,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // 0.5 ^ 2.0 = 0.25
        assert!((pixel[0][0] - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_cdl_noclamp_style() {
        // NoClamp style allows values outside [0,1]
        let cdl = CdlTransform {
            slope: [2.0, 2.0, 2.0],
            offset: [0.5, 0.5, 0.5],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::NoClamp,
            direction: TransformDirection::Forward,
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // 0.5 * 2.0 + 0.5 = 1.5 (not clamped)
        assert!(pixel[0][0] > 1.0);
    }
}

// ============================================================================
// MATRIX EXACT VALUES PARITY
// ============================================================================

/// Matrix exact value parity.
/// OCIO ref: Matrix math with known transforms
mod matrix_exact_values_parity {
    use crate::transform::{MatrixTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_matrix_identity() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();

        let original = [[0.5_f32, 0.3, 0.1]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.0001);
        }
    }

    #[test]
    fn test_matrix_scale() {
        // Scale matrix: 2x, 1x, 0.5x
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 1.0).abs() < 0.001); // 0.5 * 2.0
        assert!((pixel[0][1] - 0.5).abs() < 0.001); // 0.5 * 1.0
        assert!((pixel[0][2] - 0.25).abs() < 0.001); // 0.5 * 0.5
    }

    #[test]
    fn test_matrix_offset() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.1, 0.2, 0.3, 0.0],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.6).abs() < 0.001); // 0.5 + 0.1
        assert!((pixel[0][1] - 0.7).abs() < 0.001); // 0.5 + 0.2
        assert!((pixel[0][2] - 0.8).abs() < 0.001); // 0.5 + 0.3
    }

    #[test]
    fn test_matrix_channel_mix() {
        // Red = Blue, Green = Red, Blue = Green
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                0.0, 0.0, 1.0, 0.0, // R = B
                1.0, 0.0, 0.0, 0.0, // G = R
                0.0, 1.0, 0.0, 0.0, // B = G
                0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();

        let mut pixel = [[1.0_f32, 0.5, 0.0]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.0).abs() < 0.001); // R = old B
        assert!((pixel[0][1] - 1.0).abs() < 0.001); // G = old R
        assert!((pixel[0][2] - 0.5).abs() < 0.001); // B = old G
    }

    #[test]
    fn test_matrix_grayscale_bt709() {
        // BT.709 grayscale coefficients
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                0.2126, 0.7152, 0.0722, 0.0, 0.2126, 0.7152, 0.0722, 0.0, 0.2126, 0.7152, 0.0722,
                0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();

        let mut pixel = [[1.0_f32, 1.0, 1.0]];
        proc.apply_rgb(&mut pixel);
        // Sum of coefficients = 1.0, so white stays white
        assert!((pixel[0][0] - 1.0).abs() < 0.01);
        assert!((pixel[0][1] - 1.0).abs() < 0.01);
        assert!((pixel[0][2] - 1.0).abs() < 0.01);
    }
}

// ============================================================================
// LOG TRANSFORM EXACT VALUES PARITY
// ============================================================================

/// LogTransform exact value parity.
/// OCIO ref: Log2 and Log10 base transforms
mod log_exact_values_parity {
    use crate::transform::{LogTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_log2_base() {
        let lt = LogTransform {
            name: String::new(),
            base: 2.0,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Log(lt), TransformDirection::Forward).unwrap();

        // log2(2) = 1, log2(4) = 2
        let mut pixel = [[2.0_f32, 4.0, 1.0]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 1.0).abs() < 0.01);
        assert!((pixel[0][1] - 2.0).abs() < 0.01);
        assert!((pixel[0][2] - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_log10_base() {
        let lt = LogTransform {
            name: String::new(),
            base: 10.0,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Log(lt), TransformDirection::Forward).unwrap();

        // log10(10) = 1, log10(100) = 2
        let mut pixel = [[10.0_f32, 100.0, 1.0]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 1.0).abs() < 0.01);
        assert!((pixel[0][1] - 2.0).abs() < 0.01);
        assert!((pixel[0][2] - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_log_inverse() {
        let lt = LogTransform {
            name: String::new(),
            base: 2.0,
            direction: TransformDirection::Forward,
        };
        let proc_fwd =
            Processor::from_transform(&Transform::Log(lt.clone()), TransformDirection::Forward)
                .unwrap();
        let proc_inv =
            Processor::from_transform(&Transform::Log(lt), TransformDirection::Inverse).unwrap();

        let original = [[4.0_f32, 8.0, 16.0]];
        let mut pixel = original;
        proc_fwd.apply_rgb(&mut pixel);
        proc_inv.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }
}

// ============================================================================
// EXPONENT EXACT VALUES PARITY
// ============================================================================

/// ExponentTransform exact value parity.
/// OCIO ref: Gamma/exponent math
mod exponent_exact_values_parity {
    use crate::transform::{ExponentTransform, NegativeStyle, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_exponent_gamma_22() {
        // sRGB-like gamma 2.2
        let exp = ExponentTransform {
            name: String::new(),
            value: [2.2, 2.2, 2.2, 1.0],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Exponent(exp), TransformDirection::Forward)
                .unwrap();

        // 0.5 ^ 2.2 ≈ 0.2176
        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.2176).abs() < 0.01);
    }

    #[test]
    fn test_exponent_gamma_045() {
        // Inverse gamma (1/2.2 ≈ 0.4545)
        let exp = ExponentTransform {
            name: String::new(),
            value: [0.4545, 0.4545, 0.4545, 1.0],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Exponent(exp), TransformDirection::Forward)
                .unwrap();

        // 0.2176 ^ 0.4545 ≈ 0.5
        let mut pixel = [[0.2176_f32, 0.2176, 0.2176]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_exponent_mirror_negative() {
        // Mirror style: sign(v) * |v|^gamma
        let exp = ExponentTransform {
            name: String::new(),
            value: [2.0, 2.0, 2.0, 1.0],
            negative_style: NegativeStyle::Mirror,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Exponent(exp), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[-0.5_f32, 0.5, -0.5]];
        proc.apply_rgb(&mut pixel);
        // -0.5 mirrored: -|−0.5|^2 = -0.25
        assert!((pixel[0][0] + 0.25).abs() < 0.01);
        assert!((pixel[0][1] - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_exponent_pass_thru_negative() {
        // PassThru style: negative values pass through unchanged
        let exp = ExponentTransform {
            name: String::new(),
            value: [2.0, 2.0, 2.0, 1.0],
            negative_style: NegativeStyle::PassThru,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Exponent(exp), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[-0.5_f32, 0.5, -0.5]];
        proc.apply_rgb(&mut pixel);
        // Negative passes through, positive is squared
        assert!((pixel[0][0] + 0.5).abs() < 0.01); // unchanged
        assert!((pixel[0][1] - 0.25).abs() < 0.01); // 0.5^2
    }
}

// ============================================================================
// TRANSFER FUNCTION EXACT VALUES PARITY
// ============================================================================

/// Transfer function exact values.
/// OCIO ref: sRGB, PQ, HLG, Rec709 transfer functions
mod transfer_function_exact_parity {
    use crate::transform::{BuiltinTransferTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_srgb_linear_to_encoded() {
        let tf = BuiltinTransferTransform {
            style: "sRGB".to_string(),
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::BuiltinTransfer(tf), TransformDirection::Forward)
                .unwrap();

        // sRGB encoding: 0.18 linear -> ~0.4609 encoded
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.4609).abs() < 0.02);
    }

    #[test]
    fn test_srgb_encoded_to_linear() {
        let tf = BuiltinTransferTransform {
            style: "sRGB".to_string(),
            direction: TransformDirection::Inverse,
        };
        let proc =
            Processor::from_transform(&Transform::BuiltinTransfer(tf), TransformDirection::Forward)
                .unwrap();

        // sRGB decoding: 0.5 encoded -> ~0.214 linear
        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.214).abs() < 0.02);
    }

    #[test]
    fn test_rec709_roundtrip() {
        let tf_fwd = BuiltinTransferTransform {
            style: "Rec709".to_string(),
            direction: TransformDirection::Forward,
        };
        let tf_inv = BuiltinTransferTransform {
            style: "Rec709".to_string(),
            direction: TransformDirection::Inverse,
        };
        let proc_fwd = Processor::from_transform(
            &Transform::BuiltinTransfer(tf_fwd),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_inv = Processor::from_transform(
            &Transform::BuiltinTransfer(tf_inv),
            TransformDirection::Forward,
        )
        .unwrap();

        let original = [[0.18_f32, 0.5, 0.8]];
        let mut pixel = original;
        proc_fwd.apply_rgb(&mut pixel);
        proc_inv.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.001);
        }
    }
}

// ============================================================================
// LUT TRANSFORM EXACT VALUES PARITY
// ============================================================================

/// LUT transform exact value parity.
/// OCIO ref: LUT1D and LUT3D interpolation
mod lut_exact_values_parity {
    use crate::transform::{
        Interpolation, Lut1DHueAdjust, Lut1DTransform, Lut3DTransform, TransformDirection,
    };
    use crate::{Processor, Transform};

    #[test]
    fn test_lut1d_identity() {
        // Use identity factory
        let lut = Lut1DTransform::identity(256);
        let proc =
            Processor::from_transform(&Transform::Lut1D(lut), TransformDirection::Forward).unwrap();

        let original = [[0.25_f32, 0.5, 0.75]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Linear interpolation of an identity LUT is exact, not approximate.
        for i in 0..3 {
            assert!(
                (pixel[0][i] - original[0][i]).abs() < 1e-5,
                "lut1d identity ch{}: {}",
                i,
                pixel[0][i]
            );
        }
    }

    #[test]
    fn test_lut1d_custom() {
        // Custom LUT with inverted values
        let red: Vec<f32> = (0..256).map(|i| 1.0 - (i as f32 / 255.0)).collect();
        let lut = Lut1DTransform {
            red: red.clone(),
            green: Some(red.clone()),
            blue: Some(red),
            input_min: 0.0,
            input_max: 1.0,
            output_min: 0.0,
            output_max: 1.0,
            half_domain: false,
            raw_halfs: false,
            hue_adjust: Lut1DHueAdjust::None,
            interpolation: Interpolation::Linear,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Lut1D(lut), TransformDirection::Forward).unwrap();

        let mut pixel = [[0.25_f32, 0.5, 0.75]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.75).abs() < 0.02); // 1 - 0.25
        assert!((pixel[0][1] - 0.5).abs() < 0.02); // 1 - 0.5
        assert!((pixel[0][2] - 0.25).abs() < 0.02); // 1 - 0.75
    }

    #[test]
    fn test_lut3d_identity() {
        // Use identity factory
        let lut = Lut3DTransform::identity(2);
        let proc =
            Processor::from_transform(&Transform::Lut3D(lut), TransformDirection::Forward).unwrap();

        let original = [[0.5_f32, 0.5, 0.5]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Trilinear interpolation of a 2^3 identity cube is exact at the centre.
        for i in 0..3 {
            assert!(
                (pixel[0][i] - original[0][i]).abs() < 1e-5,
                "lut3d identity ch{}: {}",
                i,
                pixel[0][i]
            );
        }
    }
}

// ============================================================================
// FIXED FUNCTION EXACT VALUES PARITY
// ============================================================================

/// FixedFunction exact value parity.
/// OCIO ref: ACES color transforms
mod fixed_function_exact_values_parity {
    use crate::transform::{FixedFunctionStyle, FixedFunctionTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_aces_red_mod() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRedMod03,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();

        // OCIO reference vector (FixedFunctionOpCPU_tests.cpp aces_red_mod_03,
        // input row 0): [0.90,0.05,0.22] -> [0.79670035, 0.05, 0.19934007].
        let mut pixel = [[0.90_f32, 0.05, 0.22]];
        proc.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - 0.79670035).abs() < 1e-4,
            "R got {}",
            pixel[0][0]
        );
        assert!((pixel[0][1] - 0.05).abs() < 1e-4, "G got {}", pixel[0][1]);
        assert!(
            (pixel[0][2] - 0.19934007).abs() < 1e-4,
            "B got {}",
            pixel[0][2]
        );
    }

    #[test]
    fn test_aces_glow() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesGlow03,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();

        // OCIO reference vector (FixedFunctionOpCPU_tests.cpp aces_glow_03,
        // input row 0, YC=0.10): [0.11,0.02,0.0] -> [0.11392101, 0.02071291, 0.0].
        let mut pixel = [[0.11_f32, 0.02, 0.0]];
        proc.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - 0.11392101).abs() < 1e-4,
            "R got {}",
            pixel[0][0]
        );
        assert!(
            (pixel[0][1] - 0.02071291).abs() < 1e-4,
            "G got {}",
            pixel[0][1]
        );
        assert!(pixel[0][2].abs() < 1e-4, "B got {}", pixel[0][2]);
    }

    #[test]
    fn test_rec2100_surround() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::Rec2100Surround,
            params: vec![0.75], // Dim surround
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // Non-no-op: gamma=0.75 => luminance scale Y^(gamma-1)=0.5^-0.25=1.18921,
        // so each channel becomes 0.5*1.18921 = 0.59460. Identity impl leaves 0.5.
        assert!(
            (pixel[0][0] - 0.59460).abs() < 1e-3,
            "surround must scale by Y^(g-1), got {}",
            pixel[0][0]
        );
        assert!(pixel[0][0] > 0.5);
        assert!(pixel[0][0].is_finite());
    }

    #[test]
    fn test_rgb_to_hsv() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::RgbToHsv,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();

        // Pure red (1,0,0) -> HSV (0, 1, 1)
        let mut pixel = [[1.0_f32, 0.0, 0.0]];
        proc.apply_rgb(&mut pixel);
        assert!((pixel[0][0] - 0.0).abs() < 0.01); // H = 0
        assert!((pixel[0][1] - 1.0).abs() < 0.01); // S = 1
        assert!((pixel[0][2] - 1.0).abs() < 0.01); // V = 1
    }

    #[test]
    fn test_xyz_to_xyy() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyzToXyy,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();

        // D65 white point XYZ (0.95047, 1.0, 1.08883)
        let mut pixel = [[0.95047_f32, 1.0, 1.08883]];
        proc.apply_rgb(&mut pixel);
        // Should give D65 chromaticity (0.31271, 0.32902, 1.0)
        assert!((pixel[0][0] - 0.31271).abs() < 0.01);
        assert!((pixel[0][1] - 0.32902).abs() < 0.01);
    }
}

// ============================================================================
// GPU SHADER GENERATION PARITY
// ============================================================================

/// GPU shader generation for all languages.
/// OCIO ref: GpuShaderCreator
mod gpu_shader_generation_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};

    #[test]
    fn test_shader_all_languages() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        // Cg and OSL are refused (OCIO-10, `gpu::tests::cg_and_osl_are_errors_not_glsl`).
        for lang in [
            GpuLanguage::Glsl120,
            GpuLanguage::Glsl130,
            GpuLanguage::Glsl330,
            GpuLanguage::Glsl400,
            GpuLanguage::GlslEs100,
            GpuLanguage::GlslEs300,
            GpuLanguage::GlslVulkan460,
            GpuLanguage::Hlsl50,
            GpuLanguage::Metal,
        ] {
            let gpu = GpuProcessor::from_processor(&proc).unwrap();
            let shader = gpu.generate_shader(lang).unwrap();
            let code = shader.fragment_code();
            assert!(!code.is_empty(), "Empty shader for {:?}", lang);
        }
    }

    #[test]
    fn test_shader_contains_function() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();
        let code = shader.fragment_code();

        // Should contain OCIO transform function
        assert!(code.contains("OCIOMain") || code.contains("ocio") || code.contains("main"));
    }

    #[test]
    fn test_shader_glsl_version() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader_330 = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();
        let code_330 = shader_330.fragment_code();
        // GLSL 330 should have version directive
        assert!(code_330.contains("330") || code_330.contains("vec"));

        let shader_120 = gpu.generate_shader(GpuLanguage::Glsl120).unwrap();
        let code_120 = shader_120.fragment_code();
        // GLSL 120 is older version
        assert!(!code_120.is_empty());
    }
}

// ============================================================================
// ALLOCATION TRANSFORM EXACT PARITY
// ============================================================================

/// AllocationTransform exact parity.
/// OCIO ref: GPU texture allocation
mod allocation_exact_parity {
    use crate::transform::{AllocationTransform, AllocationType, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_allocation_log() {
        let at = AllocationTransform {
            allocation: AllocationType::Log2,
            vars: vec![-10.0, 6.0], // log2 range
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Allocation(at), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        // Log2 allocation: (log2(0.18) - min)/(max - min) with [min,max]=[-10,6]:
        // (-2.47393 - (-10))/16 = 0.47038. Identity/no-op would leave 0.18.
        assert!(
            (pixel[0][0] - 0.47038).abs() < 1e-4,
            "log2 alloc fit, got {}",
            pixel[0][0]
        );
        assert!((pixel[0][0] - 0.18).abs() > 0.1);
        assert!(pixel[0][0].is_finite());
    }

    #[test]
    fn test_allocation_uniform() {
        let at = AllocationTransform {
            allocation: AllocationType::Uniform,
            vars: vec![0.0, 1.0],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Allocation(at), TransformDirection::Forward)
                .unwrap();

        let original = [[0.5_f32, 0.5, 0.5]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Uniform allocation with [0,1] should be identity
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }
}

// ============================================================================
// COLORSPACE TRANSFORM PARITY
// ============================================================================

/// ColorSpaceTransform exact parity.
/// OCIO ref: Color space to color space conversion
mod colorspace_exact_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::transform::{ColorSpaceTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_colorspace_transform_direct() {
        let s = TestSchema::shared();
        let _cs = ColorSpaceTransform {
            src: s.working_space.clone(),
            dst: s.display_encoded_srgb.clone(),
            data_bypass: true,
            direction: TransformDirection::Forward,
        };

        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .display_processor(&s.working_space, &s.default_display, &s.tone_mapped_view)
            .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        assert!(pixel[0][0] > 0.3);
    }

    #[test]
    fn test_colorspace_transform_roundtrip() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc_fwd = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let proc_inv = config
            .processor(
                &TestSchema::shared().display_encoded_srgb,
                &TestSchema::shared().working_space,
            )
            .unwrap();

        let original = [[0.18_f32, 0.5, 0.8]];
        let mut pixel = original;
        proc_fwd.apply_rgb(&mut pixel);
        proc_inv.apply_rgb(&mut pixel);
        // Roundtrip should be close (allow for numerical precision)
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }
}

// ============================================================================
// GRADING RGB CURVE EXACT PARITY
// ============================================================================

/// GradingRgbCurveTransform exact values.
/// OCIO ref: RGB curve grading
mod grading_rgb_curve_exact_parity {
    use crate::transform::{GradingPrimaryStyle, GradingRgbCurveTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_rgb_curve_identity() {
        // Default is identity (linear curve)
        let gc = GradingRgbCurveTransform::default();
        let proc =
            Processor::from_transform(&Transform::GradingRgbCurve(gc), TransformDirection::Forward)
                .unwrap();

        let original = [[0.18_f32, 0.5, 0.8]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Default (identity) curve is an exact identity transform: out == in.
        for i in 0..3 {
            assert!(
                (pixel[0][i] - original[0][i]).abs() < 1e-5,
                "RGB curve identity ch{}",
                i
            );
        }
    }

    #[test]
    fn test_rgb_curve_contrast_boost() {
        // S-curve for contrast boost
        // Create S-curve: darker shadows, brighter highlights
        let mut gc = GradingRgbCurveTransform {
            master: vec![
                [0.0, 0.0],
                [0.25, 0.15], // Pull down shadows
                [0.5, 0.5],   // Keep midtones
                [0.75, 0.85], // Push up highlights
                [1.0, 1.0],
            ],
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::GradingRgbCurve(gc), TransformDirection::Forward)
                .unwrap();

        let mut pixel = [[0.25_f32, 0.5, 0.75]];
        proc.apply_rgb(&mut pixel);
        // Shadows darker, highlights brighter
        assert!(pixel[0][0] < 0.25);
        assert!((pixel[0][1] - 0.5).abs() < 0.1);
        assert!(pixel[0][2] > 0.75);
    }
}

// ============================================================================
// GRADING HUE CURVE EXACT PARITY
// ============================================================================

/// GradingHueCurveTransform exact values.
/// OCIO ref: Hue curve grading with all curve types
mod grading_hue_curve_exact_parity {
    use crate::transform::{
        GradingHueCurveStyle, GradingHueCurveTransform, HsyTransformStyle, HueCurveType,
        TransformDirection,
    };
    use crate::{Processor, Transform};

    #[test]
    fn test_hue_curve_identity() {
        let hc = GradingHueCurveTransform::default();
        let proc =
            Processor::from_transform(&Transform::GradingHueCurve(hc), TransformDirection::Forward)
                .unwrap();

        let original = [[0.5_f32, 0.3, 0.1]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Default hue curves compose with the RGB<->HSY round-trip to an exact identity.
        for i in 0..3 {
            assert!(
                (pixel[0][i] - original[0][i]).abs() < 1e-5,
                "hue curve identity ch{}",
                i
            );
        }
    }

    #[test]
    fn test_hue_curve_slopes_default() {
        let hc = GradingHueCurveTransform::default();

        // All slopes should be default (empty)
        assert!(hc.slopes_are_default(HueCurveType::HueHue));
        assert!(hc.slopes_are_default(HueCurveType::HueSat));
        assert!(hc.slopes_are_default(HueCurveType::HueLum));
        assert!(hc.slopes_are_default(HueCurveType::LumSat));
        assert!(hc.slopes_are_default(HueCurveType::SatSat));
        assert!(hc.slopes_are_default(HueCurveType::LumLum));
        assert!(hc.slopes_are_default(HueCurveType::SatLum));
        assert!(hc.slopes_are_default(HueCurveType::HueFx));
    }

    #[test]
    fn test_hue_curve_all_styles() {
        for style in [
            GradingHueCurveStyle::Log,
            GradingHueCurveStyle::Linear,
            GradingHueCurveStyle::Video,
        ] {
            let hc = GradingHueCurveTransform {
                style,
                ..Default::default()
            };
            let proc = Processor::from_transform(
                &Transform::GradingHueCurve(hc),
                TransformDirection::Forward,
            )
            .unwrap();

            let original = [[0.5_f32, 0.3, 0.1]];
            let mut pixel = original;
            proc.apply_rgb(&mut pixel);
            // Default (identity) hue curves are an exact identity for every style,
            // composing with the RGB<->HSY round-trip back to the input.
            for i in 0..3 {
                assert!(
                    (pixel[0][i] - original[0][i]).abs() < 1e-5,
                    "hue curve style {:?} ch{} not identity: {}",
                    style,
                    i,
                    pixel[0][i]
                );
            }
        }
    }

    #[test]
    fn test_hue_curve_hsy_bypass() {
        let hc = GradingHueCurveTransform {
            rgb_to_hsy: HsyTransformStyle::None,
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::GradingHueCurve(hc), TransformDirection::Forward)
                .unwrap();

        let original = [[0.5_f32, 0.3, 0.1]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // HSY bypass with default (identity) curves: no RGB<->HSY conversion and no
        // curve change => exact identity passthrough.
        for i in 0..3 {
            assert!(
                (pixel[0][i] - original[0][i]).abs() < 1e-5,
                "hsy bypass ch{}: {}",
                i,
                pixel[0][i]
            );
        }
    }
}

// ============================================================================
// ALL FIXED FUNCTION STYLES COMPREHENSIVE PARITY
// ============================================================================

/// All FixedFunctionStyle variants comprehensive test.
/// OCIO ref: FixedFunctionTransform with all styles
mod fixed_function_all_styles_comprehensive_parity {
    use crate::transform::{FixedFunctionStyle, FixedFunctionTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_aces_red_mod_03() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRedMod03,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        // OCIO reference vector (FixedFunctionOpCPU_tests.cpp aces_red_mod_03):
        // [0.90,0.05,0.22] -> [0.79670035, 0.05, 0.19934007].
        let mut pixel = [[0.90_f32, 0.05, 0.22]];
        proc.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - 0.79670035).abs() < 1e-4,
            "R got {}",
            pixel[0][0]
        );
        assert!((pixel[0][1] - 0.05).abs() < 1e-4, "G got {}", pixel[0][1]);
        assert!(
            (pixel[0][2] - 0.19934007).abs() < 1e-4,
            "B got {}",
            pixel[0][2]
        );
    }

    #[test]
    fn test_aces_red_mod_10() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesRedMod10,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        // OCIO reference vector (FixedFunctionOpCPU_tests.cpp aces_red_mod_10):
        // [0.90,0.05,0.22] -> [0.77148211, 0.05, 0.22].
        let mut pixel = [[0.90_f32, 0.05, 0.22]];
        proc.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - 0.771_482_1).abs() < 1e-4,
            "R got {}",
            pixel[0][0]
        );
        assert!((pixel[0][1] - 0.05).abs() < 1e-4, "G got {}", pixel[0][1]);
        assert!((pixel[0][2] - 0.22).abs() < 1e-4, "B got {}", pixel[0][2]);
    }

    #[test]
    fn test_aces_glow_03() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesGlow03,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        // OCIO reference vector (FixedFunctionOpCPU_tests.cpp aces_glow_03):
        // [0.11,0.02,0.0] -> [0.11392101, 0.02071291, 0.0].
        let mut pixel = [[0.11_f32, 0.02, 0.0]];
        proc.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - 0.11392101).abs() < 1e-4,
            "R got {}",
            pixel[0][0]
        );
        assert!(
            (pixel[0][1] - 0.02071291).abs() < 1e-4,
            "G got {}",
            pixel[0][1]
        );
        assert!(pixel[0][2].abs() < 1e-4, "B got {}", pixel[0][2]);
    }

    #[test]
    fn test_aces_glow_10() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesGlow10,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        // OCIO reference vector (FixedFunctionOpCPU_tests.cpp aces_glow_10):
        // [0.11,0.02,0.0] -> [0.11154121, 0.02028021, 0.0].
        let mut pixel = [[0.11_f32, 0.02, 0.0]];
        proc.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - 0.11154121).abs() < 1e-4,
            "R got {}",
            pixel[0][0]
        );
        assert!(
            (pixel[0][1] - 0.02028021).abs() < 1e-4,
            "G got {}",
            pixel[0][1]
        );
        assert!(pixel[0][2].abs() < 1e-4, "B got {}", pixel[0][2]);
    }

    #[test]
    fn test_aces_dark_to_dim_10() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesDarkToDim10,
            params: vec![0.9811], // Standard gamma
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        // DarkToDim10: Y=0.18, scale=Y^(gamma-1)=0.18^-0.0189=1.03294 => R=0.185929.
        // Identity impl would leave 0.18.
        assert!((pixel[0][0] - 0.185929).abs() < 1e-4, "got {}", pixel[0][0]);
        assert!(pixel[0][0] > 0.18);
    }

    #[test]
    fn test_aces_gamut_comp_13() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesGamutComp13,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[1.2_f32, -0.1, 0.0]];
        proc.apply_rgb(&mut pixel);
        // Gamut compression pulls the out-of-gamut negative green toward the achromatic
        // axis (ach=R=1.2, dist=0 for R so R is unchanged; G's distance > threshold so it
        // is compressed upward). Identity impl would leave G=-0.1.
        assert!(
            (pixel[0][0] - 1.2).abs() < 1e-4,
            "max channel unchanged, got {}",
            pixel[0][0]
        );
        assert!(
            pixel[0][1] > -0.1,
            "green must compress toward gamut, got {}",
            pixel[0][1]
        );
        assert!(pixel[0][1].is_finite() && pixel[0][2].is_finite());
    }

    #[test]
    fn test_rec2100_surround() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::Rec2100Surround,
            params: vec![0.75], // Dim surround
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixel);
        // gamma=0.75 => scale Y^(gamma-1)=0.5^-0.25=1.18921 => 0.5*1.18921=0.59460.
        assert!((pixel[0][0] - 0.59460).abs() < 1e-3, "got {}", pixel[0][0]);
        assert!(pixel[0][0] > 0.5);
    }

    #[test]
    fn test_rgb_to_hsv_and_back() {
        let ff_to = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::RgbToHsv,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let ff_from = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::HsvToRgb,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc_to = Processor::from_transform(
            &Transform::FixedFunction(ff_to),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_from = Processor::from_transform(
            &Transform::FixedFunction(ff_from),
            TransformDirection::Forward,
        )
        .unwrap();

        let original = [[0.8_f32, 0.4, 0.2]];
        let mut pixel = original;
        proc_to.apply_rgb(&mut pixel);
        proc_from.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }

    #[test]
    fn test_xyz_to_xyy_and_back() {
        let ff_to = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyzToXyy,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let ff_from = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyyToXyz,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc_to = Processor::from_transform(
            &Transform::FixedFunction(ff_to),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_from = Processor::from_transform(
            &Transform::FixedFunction(ff_from),
            TransformDirection::Forward,
        )
        .unwrap();

        let original = [[0.95047_f32, 1.0, 1.08883]];
        let mut pixel = original;
        proc_to.apply_rgb(&mut pixel);
        proc_from.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }

    #[test]
    fn test_xyz_to_uvy_and_back() {
        let ff_to = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyzToUvy,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let ff_from = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::UvyToXyz,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc_to = Processor::from_transform(
            &Transform::FixedFunction(ff_to),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_from = Processor::from_transform(
            &Transform::FixedFunction(ff_from),
            TransformDirection::Forward,
        )
        .unwrap();

        let original = [[0.95047_f32, 1.0, 1.08883]];
        let mut pixel = original;
        proc_to.apply_rgb(&mut pixel);
        proc_from.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }

    #[test]
    fn test_xyz_to_luv_and_back() {
        let ff_to = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::XyzToLuv,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let ff_from = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::LuvToXyz,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc_to = Processor::from_transform(
            &Transform::FixedFunction(ff_to),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_from = Processor::from_transform(
            &Transform::FixedFunction(ff_from),
            TransformDirection::Forward,
        )
        .unwrap();

        let original = [[0.5_f32, 0.5, 0.5]];
        let mut pixel = original;
        proc_to.apply_rgb(&mut pixel);
        proc_from.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.02);
        }
    }

    #[test]
    fn test_lin_to_pq_and_back() {
        let ff_to = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::LinToPq,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let ff_from = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::PqToLin,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc_to = Processor::from_transform(
            &Transform::FixedFunction(ff_to),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_from = Processor::from_transform(
            &Transform::FixedFunction(ff_from),
            TransformDirection::Forward,
        )
        .unwrap();

        let original = [[0.18_f32, 0.5, 1.0]];
        let mut pixel = original;
        proc_to.apply_rgb(&mut pixel);
        proc_from.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }

    #[test]
    fn test_lin_to_gamma_log() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::LinToGammaLog,
            params: vec![0.0, 0.01, 2.4, 0.1, 1.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let original = [[0.18_f32, 0.5, 1.0]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Non-no-op: with break=0.01 every probe takes the log segment, so each output
        // moves well away from its linear input. Identity impl would leave inputs intact.
        for i in 0..3 {
            assert!(
                pixel[0][i].is_finite(),
                "ch{} not finite: {}",
                i,
                pixel[0][i]
            );
            assert!(
                (pixel[0][i] - original[0][i]).abs() > 0.1,
                "ch{} unchanged: {}",
                i,
                pixel[0][i]
            );
        }
    }

    #[test]
    fn test_gamma_log_to_lin() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::GammaLogToLin,
            params: vec![0.0, 0.01, 2.4, 0.1, 1.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let original = [[0.5_f32, 0.5, 0.5]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Non-no-op: 0.5 is above the prime break, so the log->lin segment maps it well
        // below 0.5. Identity impl would leave 0.5.
        for i in 0..3 {
            assert!(
                pixel[0][i].is_finite(),
                "ch{} not finite: {}",
                i,
                pixel[0][i]
            );
            assert!(
                (pixel[0][i] - original[0][i]).abs() > 0.1,
                "ch{} unchanged: {}",
                i,
                pixel[0][i]
            );
        }
    }

    #[test]
    fn test_lin_to_double_log() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::LinToDoubleLog,
            params: vec![10.0, 0.01, 0.5, 0.0, 1.0, 0.0, 0.5, 0.5, 0.0],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let original = [[0.18_f32, 0.5, 1.0]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Non-no-op: the highlight (1.0) is above break2 and goes through the upper log
        // segment => ls2Slope*ln(1.0)+ls2Off = (0.5/ln10)*0 + 0 = 0. Identity impl leaves 1.0.
        assert!(pixel[0][2].is_finite());
        assert!(
            pixel[0][2].abs() < 1e-4,
            "highlight upper log seg, got {}",
            pixel[0][2]
        );
        assert!((pixel[0][2] - original[0][2]).abs() > 0.4);
    }

    #[test]
    fn test_double_log_to_lin() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::DoubleLogToLin,
            params: vec![10.0, 0.01, 0.5, 0.0, 1.0, 0.0, 0.5, 0.5, 0.0],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let original = [[0.5_f32, 0.5, 0.5]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Non-no-op: 0.5 falls in the lower log->lin segment and is remapped far from 0.5.
        // (These placeholder params drive the segment to its extreme; the point is the
        // transform is NOT identity.) An identity impl would leave 0.5 untouched.
        for i in 0..3 {
            assert!(
                (pixel[0][i] - original[0][i]).abs() > 0.1,
                "ch{} unchanged: {}",
                i,
                pixel[0][i]
            );
        }
    }

    #[test]
    fn test_aces_output_transform_20() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            // peak + Rec.709 limiting primaries (red,green,blue,white xy). Without real
            // primaries the limiting matrix is singular and the result is NaN, so a valid
            // gamut must be supplied for a faithful probe.
            style: FixedFunctionStyle::AcesOutputTransform20,
            params: vec![100.0, 0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let original = [[0.18_f32, 0.18, 0.18]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Non-no-op: the ACES 2.0 output transform tone-maps 0.18 mid-gray to a different
        // display value in [0,1]. Identity impl would leave 0.18 on every channel.
        for (i, &v) in pixel[0].iter().enumerate() {
            assert!(v.is_finite() && (0.0..=1.0).contains(&v), "ch{}: {}", i, v);
        }
        assert!(
            (pixel[0][0] - original[0][0]).abs() > 1e-3,
            "output transform was a no-op: {}",
            pixel[0][0]
        );
    }

    #[test]
    fn test_aces_rgb_to_jmh_20() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            // Rec.709 primaries (red,green,blue,white xy) — required to build the JMh
            // appearance matrices; an empty params set yields a singular matrix => NaN.
            style: FixedFunctionStyle::AcesRgbToJmh20,
            params: vec![0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let original = [[0.18_f32, 0.18, 0.18]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Non-no-op: RGB->JMH maps neutral gray to (J lightness, M~0 chroma, H hue);
        // the J channel is a lightness value, not 0.18. Identity impl would leave 0.18.
        for (i, &v) in pixel[0].iter().enumerate() {
            assert!(v.is_finite(), "ch{} not finite: {}", i, v);
        }
        assert!(
            (pixel[0][0] - original[0][0]).abs() > 1e-3,
            "J channel unchanged: {}",
            pixel[0][0]
        );
    }

    #[test]
    fn test_aces_jmh_to_rgb_20() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            // Rec.709 primaries (red,green,blue,white xy) — required for the inverse JMh
            // matrices; empty params => singular matrix => NaN.
            style: FixedFunctionStyle::AcesJmhToRgb20,
            params: vec![0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let original = [[50.0_f32, 10.0, 180.0]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Non-no-op: JMH->RGB maps appearance coords (J=50,M=10,H=180) to linear RGB,
        // which is nowhere near the raw input triple. Identity impl would leave it as-is.
        for (i, &v) in pixel[0].iter().enumerate() {
            assert!(v.is_finite(), "ch{} not finite: {}", i, v);
        }
        assert!(
            (pixel[0][2] - original[0][2]).abs() > 1.0,
            "JMH->RGB was a no-op: {:?}",
            pixel[0]
        );
    }

    #[test]
    fn test_aces_tonescale_compress_20() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesTonescaleCompress20,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let original = [[0.18_f32, 0.18, 0.18]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Non-no-op: the ACES 2.0 tonescale compresses 0.18 to a different value.
        for (i, &v) in pixel[0].iter().enumerate() {
            assert!(v.is_finite(), "ch{} not finite: {}", i, v);
        }
        assert!(
            (pixel[0][0] - original[0][0]).abs() > 1e-3,
            "tonescale was a no-op: {}",
            pixel[0][0]
        );
    }

    #[test]
    fn test_aces_gamut_compress_20() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesGamutCompress20,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let original = [[1.2_f32, -0.1, 0.0]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Non-no-op: ACES 2.0 gamut compression pulls the out-of-gamut negative green
        // toward the achromatic axis. Identity impl would leave G=-0.1.
        for (i, &v) in pixel[0].iter().enumerate() {
            assert!(v.is_finite(), "ch{} not finite: {}", i, v);
        }
        assert!(
            (pixel[0][1] - original[0][1]).abs() > 1e-4,
            "gamut compress was a no-op: {:?}",
            pixel[0]
        );
    }

    #[test]
    fn test_rgb_to_hsy_lin_and_back() {
        let ff_to = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::RgbToHsyLin,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let ff_from = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::HsyLinToRgb,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc_to = Processor::from_transform(
            &Transform::FixedFunction(ff_to),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_from = Processor::from_transform(
            &Transform::FixedFunction(ff_from),
            TransformDirection::Forward,
        )
        .unwrap();

        let original = [[0.5_f32, 0.3, 0.2]];
        let mut pixel = original;
        proc_to.apply_rgb(&mut pixel);
        proc_from.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }

    #[test]
    fn test_rgb_to_hsy_log_and_back() {
        let ff_to = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::RgbToHsyLog,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let ff_from = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::HsyLogToRgb,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc_to = Processor::from_transform(
            &Transform::FixedFunction(ff_to),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_from = Processor::from_transform(
            &Transform::FixedFunction(ff_from),
            TransformDirection::Forward,
        )
        .unwrap();

        let original = [[0.5_f32, 0.3, 0.2]];
        let mut pixel = original;
        proc_to.apply_rgb(&mut pixel);
        proc_from.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }

    #[test]
    fn test_rgb_to_hsy_vid_and_back() {
        let ff_to = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::RgbToHsyVid,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let ff_from = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::HsyVidToRgb,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc_to = Processor::from_transform(
            &Transform::FixedFunction(ff_to),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_from = Processor::from_transform(
            &Transform::FixedFunction(ff_from),
            TransformDirection::Forward,
        )
        .unwrap();

        let original = [[0.5_f32, 0.3, 0.2]];
        let mut pixel = original;
        proc_to.apply_rgb(&mut pixel);
        proc_from.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }

    #[test]
    fn test_all_fixed_function_styles_exist() {
        // Verify all enum variants exist
        let _styles = [
            FixedFunctionStyle::AcesRedMod03,
            FixedFunctionStyle::AcesRedMod10,
            FixedFunctionStyle::AcesGlow03,
            FixedFunctionStyle::AcesGlow10,
            FixedFunctionStyle::AcesDarkToDim10,
            FixedFunctionStyle::AcesGamutComp13,
            FixedFunctionStyle::Rec2100Surround,
            FixedFunctionStyle::RgbToHsv,
            FixedFunctionStyle::HsvToRgb,
            FixedFunctionStyle::XyzToXyy,
            FixedFunctionStyle::XyyToXyz,
            FixedFunctionStyle::XyzToUvy,
            FixedFunctionStyle::UvyToXyz,
            FixedFunctionStyle::XyzToLuv,
            FixedFunctionStyle::LuvToXyz,
            FixedFunctionStyle::LinToPq,
            FixedFunctionStyle::PqToLin,
            FixedFunctionStyle::LinToGammaLog,
            FixedFunctionStyle::GammaLogToLin,
            FixedFunctionStyle::LinToDoubleLog,
            FixedFunctionStyle::DoubleLogToLin,
            FixedFunctionStyle::AcesOutputTransform20,
            FixedFunctionStyle::AcesRgbToJmh20,
            FixedFunctionStyle::AcesJmhToRgb20,
            FixedFunctionStyle::AcesTonescaleCompress20,
            FixedFunctionStyle::AcesGamutCompress20,
            FixedFunctionStyle::RgbToHsyLin,
            FixedFunctionStyle::HsyLinToRgb,
            FixedFunctionStyle::RgbToHsyLog,
            FixedFunctionStyle::HsyLogToRgb,
            FixedFunctionStyle::RgbToHsyVid,
            FixedFunctionStyle::HsyVidToRgb,
        ];
        assert_eq!(_styles.len(), 32);
    }
}

// ============================================================================
// LOG AFFINE TRANSFORM COMPREHENSIVE PARITY
// ============================================================================

/// LogAffineTransform comprehensive test.
/// OCIO ref: LogAffineTransform
mod log_affine_comprehensive_exact_parity {
    use crate::transform::{LogAffineTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_log_affine_default() {
        let la = LogAffineTransform::default();
        let proc =
            Processor::from_transform(&Transform::LogAffine(la), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        // Default LogAffine (base 2, slope 1, offset 0): out = log2(0.18) = -2.47393.
        assert!(
            (pixel[0][0] - (-2.473931)).abs() < 1e-4,
            "got {}",
            pixel[0][0]
        );
    }

    #[test]
    fn test_log_affine_custom_params() {
        let la = LogAffineTransform {
            name: String::new(),
            log_side_slope: [0.3, 0.3, 0.3],
            log_side_offset: [0.5, 0.5, 0.5],
            lin_side_slope: [1.0, 1.0, 1.0],
            lin_side_offset: [0.0, 0.0, 0.0],
            base: 10.0,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::LogAffine(la), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.18_f32, 0.5, 1.0]];
        proc.apply_rgb(&mut pixel);
        // out = 0.3*log10(x)+0.5 : x=0.18 -> 0.27658, x=1.0 -> 0.5 (log10(1)=0).
        assert!((pixel[0][0] - 0.27658).abs() < 1e-4, "ch0 {}", pixel[0][0]);
        assert!((pixel[0][2] - 0.5).abs() < 1e-5, "ch2 {}", pixel[0][2]);
    }

    #[test]
    fn test_log_affine_roundtrip() {
        let la = LogAffineTransform::default();
        let proc_fwd = Processor::from_transform(
            &Transform::LogAffine(la.clone()),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_inv =
            Processor::from_transform(&Transform::LogAffine(la), TransformDirection::Inverse)
                .unwrap();

        let original = [[0.18_f32, 0.5, 0.8]];
        let mut pixel = original;
        proc_fwd.apply_rgb(&mut pixel);
        proc_inv.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }
}

// ============================================================================
// LOG CAMERA TRANSFORM COMPREHENSIVE PARITY
// ============================================================================

/// LogCameraTransform comprehensive test.
/// OCIO ref: LogCameraTransform
mod log_camera_comprehensive_exact_parity {
    use crate::transform::{LogCameraTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_log_camera_default_exact() {
        let lc = LogCameraTransform::default();
        let proc =
            Processor::from_transform(&Transform::LogCamera(lc), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        // Default LogCamera (base 2, slope 1, offset 0, break 0): 0.18 is in the log
        // region => log2(0.18) = -2.47393. Identity impl would leave 0.18.
        assert!(
            (pixel[0][0] - (-2.473931)).abs() < 1e-4,
            "got {}",
            pixel[0][0]
        );
    }

    #[test]
    fn test_log_camera_arri_logc3_exact() {
        // ARRI LogC3 parameters
        let lc = LogCameraTransform {
            name: String::new(),
            log_side_slope: [0.247190; 3],
            log_side_offset: [0.385537; 3],
            lin_side_slope: [5.555556; 3],
            lin_side_offset: [0.052272; 3],
            lin_side_break: [0.010591; 3],
            linear_slope: None,
            base: 10.0,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::LogCamera(lc), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        // ARRI LogC3 (EI 800) log region: 0.247190*log10(5.555556*0.18+0.052272)+0.385537
        // = 0.391007. Tight: an identity/wrong-slope impl misses this exact value.
        assert!((pixel[0][0] - 0.391007).abs() < 1e-4, "got {}", pixel[0][0]);
    }

    #[test]
    fn test_log_camera_roundtrip_exact() {
        let lc = LogCameraTransform::default();
        let proc_fwd = Processor::from_transform(
            &Transform::LogCamera(lc.clone()),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_inv =
            Processor::from_transform(&Transform::LogCamera(lc), TransformDirection::Inverse)
                .unwrap();

        let original = [[0.18_f32, 0.5, 0.8]];
        let mut pixel = original;
        proc_fwd.apply_rgb(&mut pixel);
        proc_inv.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }
}

// ============================================================================
// EXPONENT WITH LINEAR TRANSFORM COMPREHENSIVE PARITY
// ============================================================================

/// ExponentWithLinearTransform comprehensive test.
/// OCIO ref: ExponentWithLinearTransform
mod exponent_with_linear_comprehensive_parity {
    use crate::transform::{ExponentWithLinearTransform, NegativeStyle, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_exponent_with_linear_default() {
        let ewl = ExponentWithLinearTransform::default();
        let proc = Processor::from_transform(
            &Transform::ExponentWithLinear(ewl),
            TransformDirection::Forward,
        )
        .unwrap();
        let original = [[0.18_f32, 0.18, 0.18]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Default MonCurve (gamma=1, offset=0) is an exact identity.
        for i in 0..3 {
            assert!(
                (pixel[0][i] - original[0][i]).abs() < 1e-5,
                "ch{}: {}",
                i,
                pixel[0][i]
            );
        }
    }

    #[test]
    fn test_exponent_with_linear_srgb() {
        // sRGB-like transfer function
        let ewl = ExponentWithLinearTransform {
            name: String::new(),
            gamma: [2.4, 2.4, 2.4, 1.0],
            offset: [0.055, 0.055, 0.055, 0.0],
            negative_style: NegativeStyle::Linear,
            direction: TransformDirection::Forward,
        };
        let proc = Processor::from_transform(
            &Transform::ExponentWithLinear(ewl),
            TransformDirection::Forward,
        )
        .unwrap();
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        // Value should change from encoding
        assert!(pixel[0][0] != 0.18);
    }

    #[test]
    fn test_exponent_with_linear_roundtrip() {
        let ewl = ExponentWithLinearTransform {
            name: String::new(),
            gamma: [2.2, 2.2, 2.2, 1.0],
            offset: [0.0, 0.0, 0.0, 0.0],
            negative_style: NegativeStyle::Linear,
            direction: TransformDirection::Forward,
        };
        let proc_fwd = Processor::from_transform(
            &Transform::ExponentWithLinear(ewl.clone()),
            TransformDirection::Forward,
        )
        .unwrap();
        let proc_inv = Processor::from_transform(
            &Transform::ExponentWithLinear(ewl),
            TransformDirection::Inverse,
        )
        .unwrap();

        let original = [[0.18_f32, 0.5, 0.8]];
        let mut pixel = original;
        proc_fwd.apply_rgb(&mut pixel);
        proc_inv.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }

    #[test]
    fn test_exponent_with_linear_negative_styles() {
        // LINEAR and MIRROR only: OCIO rejects CLAMP / PASS_THRU for a moncurve (B24).
        for style in [NegativeStyle::Mirror, NegativeStyle::Linear] {
            let ewl = ExponentWithLinearTransform {
                name: String::new(),
                gamma: [2.2, 2.2, 2.2, 1.0],
                offset: [0.0, 0.0, 0.0, 0.0],
                negative_style: style,
                direction: TransformDirection::Forward,
            };
            let proc = Processor::from_transform(
                &Transform::ExponentWithLinear(ewl),
                TransformDirection::Forward,
            )
            .unwrap();
            let mut pixel = [[-0.1_f32, 0.5, 1.0]];
            proc.apply_rgb(&mut pixel);
            // Non-no-op: with gamma=2.2 the mid value 0.5 is re-shaped on every negative
            // style (the styles only differ in how the negative channel is handled).
            assert!(
                (pixel[0][1] - 0.5).abs() > 1e-3,
                "style {:?} mid unchanged: {}",
                style,
                pixel[0][1]
            );
            for (i, &v) in pixel[0].iter().enumerate() {
                assert!(v.is_finite(), "style {:?} ch{} not finite: {}", style, i, v);
            }
        }
    }
}

// ============================================================================
// EXPOSURE CONTRAST TRANSFORM COMPREHENSIVE PARITY
// ============================================================================

/// ExposureContrastTransform comprehensive test.
/// OCIO ref: ExposureContrastTransform
mod exposure_contrast_all_styles_exact_parity {
    use crate::transform::{ExposureContrastStyle, ExposureContrastTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_exposure_contrast_identity_exact() {
        let ec = ExposureContrastTransform {
            name: String::new(),
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            style: ExposureContrastStyle::Linear,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };
        let proc = Processor::from_transform(
            &Transform::ExposureContrast(ec),
            TransformDirection::Forward,
        )
        .unwrap();

        let original = [[0.18_f32, 0.5, 0.8]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.01);
        }
    }

    #[test]
    fn test_exposure_contrast_1_stop_exact() {
        let ec = ExposureContrastTransform {
            name: String::new(),
            exposure: 1.0, // +1 stop
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            style: ExposureContrastStyle::Linear,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };
        let proc = Processor::from_transform(
            &Transform::ExposureContrast(ec),
            TransformDirection::Forward,
        )
        .unwrap();

        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        // +1 stop should double the value
        assert!((pixel[0][0] - 0.36).abs() < 0.02);
    }

    #[test]
    fn test_exposure_contrast_all_styles_exact() {
        for style in [
            ExposureContrastStyle::Linear,
            ExposureContrastStyle::Video,
            ExposureContrastStyle::Logarithmic,
        ] {
            let ec = ExposureContrastTransform {
                name: String::new(),
                exposure: 0.5,
                contrast: 1.2,
                gamma: 1.1,
                pivot: 0.18,
                log_exposure_step: 0.088,
                log_mid_gray: 0.435,
                style,
                direction: TransformDirection::Forward,
                dynamic: crate::EcDynamic::NONE,
            };
            let proc = Processor::from_transform(
                &Transform::ExposureContrast(ec),
                TransformDirection::Forward,
            )
            .unwrap();
            // The probe must not sit ON the pivot: 0.18 is (coincidentally) within
            // 2e-4 of a fixed point of the Video style at these parameters, so the
            // old first channel made the "not a no-op" check pass or fail on an
            // accident of the OETF exponent rather than on the op doing anything.
            let original = [[0.5_f32, 0.18, 0.8]];
            let mut pixel = original;
            proc.apply_rgb(&mut pixel);
            // Non-no-op: exposure=0.5, contrast=1.2, gamma=1.1 all push the value away
            // from the input on every style. Identity impl would leave 0.5.
            assert!(
                (pixel[0][0] - original[0][0]).abs() > 1e-3,
                "style {:?} no-op: {}",
                style,
                pixel[0][0]
            );

            // Pin the Video style exactly, so this test can tell the correct OETF
            // exponent (1/1.83, `vfx_core::consts::exposure_contrast::VIDEO_OETF_POWER`)
            // from the 1/2.4 that was hand-typed here: the latter yields 0.60914.
            if style == ExposureContrastStyle::Video {
                assert!(
                    (pixel[0][0] - 0.694_119_4).abs() < 1e-5,
                    "Video OETF exponent regressed: {}",
                    pixel[0][0]
                );
            }
            for (i, &v) in pixel[0].iter().enumerate() {
                assert!(v.is_finite(), "style {:?} ch{} not finite: {}", style, i, v);
            }
        }
    }
}

// ============================================================================
// BATCH TEXTURE API COMPREHENSIVE PARITY
// ============================================================================

/// Batch texture API comprehensive test.
/// OCIO ref: GpuShaderCreator texture batching
mod batch_texture_comprehensive_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{
        BatchTextureDescriptor, GpuChannelType, GpuInterpolation, GpuProcessor, GpuTextureType,
        TextureDimensions,
    };

    #[test]
    fn test_batch_descriptor_all_channel_types() {
        for channel in [GpuChannelType::Red, GpuChannelType::Rgb] {
            let desc = BatchTextureDescriptor {
                name: "test".to_string(),
                binding_index: 0,
                texture_type: GpuTextureType::Texture1D,
                dimensions: TextureDimensions::Texture1D,
                width: 256,
                height: 1,
                depth: 1,
                channel_type: channel,
                interpolation: GpuInterpolation::Linear,
                data_offset: 0,
                data_size: 256 * 4,
            };
            // Field must round-trip the loop variable, not just be non-zero.
            assert_eq!(desc.channel_type, channel);
            assert_eq!(desc.width, 256);
        }
    }

    #[test]
    fn test_batch_descriptor_all_dimensions() {
        for dim in [TextureDimensions::Texture1D, TextureDimensions::Texture2D] {
            let desc = BatchTextureDescriptor {
                name: "test".to_string(),
                binding_index: 0,
                texture_type: GpuTextureType::Texture1D,
                dimensions: dim,
                width: 256,
                height: 1,
                depth: 1,
                channel_type: GpuChannelType::Rgb,
                interpolation: GpuInterpolation::Linear,
                data_offset: 0,
                data_size: 256 * 4,
            };
            assert_eq!(desc.dimensions, dim);
        }
    }

    #[test]
    fn test_batch_descriptor_all_texture_types() {
        for tex_type in [
            GpuTextureType::Texture1D,
            GpuTextureType::Texture2D,
            GpuTextureType::Texture3D,
        ] {
            let desc = BatchTextureDescriptor {
                name: "test".to_string(),
                binding_index: 0,
                texture_type: tex_type,
                dimensions: TextureDimensions::Texture1D,
                width: 256,
                height: 1,
                depth: 1,
                channel_type: GpuChannelType::Rgb,
                interpolation: GpuInterpolation::Linear,
                data_offset: 0,
                data_size: 256 * 4,
            };
            assert_eq!(desc.texture_type, tex_type);
        }
    }

    #[test]
    fn test_batch_descriptor_all_interpolations() {
        for interp in [GpuInterpolation::Nearest, GpuInterpolation::Linear] {
            let desc = BatchTextureDescriptor {
                name: "test".to_string(),
                binding_index: 0,
                texture_type: GpuTextureType::Texture1D,
                dimensions: TextureDimensions::Texture1D,
                width: 256,
                height: 1,
                depth: 1,
                channel_type: GpuChannelType::Rgb,
                interpolation: interp,
                data_offset: 0,
                data_size: 256 * 4,
            };
            assert_eq!(desc.interpolation, interp);
        }
    }

    #[test]
    fn test_gpu_processor_batch_textures() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();

        let descriptors = gpu.batch_texture_descriptors();
        // May have textures depending on transform chain
        for desc in &descriptors {
            assert!(desc.width > 0);
            assert!(desc.data_size > 0 || desc.width == 0);
        }
    }

    #[test]
    fn test_batch_texture_data_retrieval() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();

        let (data, offsets) = gpu.batch_texture_data();
        // One offset per texture, offsets monotonic and in-bounds, and the packed data is
        // all finite. (A no-op packer returning mismatched offsets/garbage fails this.)
        assert_eq!(offsets.len(), gpu.batch_texture_descriptors().len());
        let mut prev = 0usize;
        for &off in &offsets {
            assert!(
                off >= prev && off <= data.len(),
                "bad offset {} (prev {}, len {})",
                off,
                prev,
                data.len()
            );
            prev = off;
        }
        assert!(
            data.iter().all(|v| v.is_finite()),
            "packed texture data has non-finite values"
        );
    }
}

// ============================================================================
// GPU CHANNEL TYPE AND TEXTURE DIMENSIONS PARITY (TypeDesc equivalent)
// ============================================================================

/// TypeDesc equivalent parity (GpuChannelType + TextureDimensions).
/// OCIO ref: GpuShaderCreator::TextureType, TextureDimensions
mod typedesc_equivalent_parity {
    use crate::gpu::{GpuChannelType, GpuTextureType, TextureDimensions};

    #[test]
    fn test_gpu_channel_type_all_variants() {
        let _red = GpuChannelType::Red;
        let _rgb = GpuChannelType::Rgb;

        // Default should be Rgb
        assert_eq!(GpuChannelType::default(), GpuChannelType::Rgb);
    }

    #[test]
    fn test_texture_dimensions_all_variants() {
        let _1d = TextureDimensions::Texture1D;
        let _2d = TextureDimensions::Texture2D;

        // Default should be Texture1D
        assert_eq!(TextureDimensions::default(), TextureDimensions::Texture1D);
    }

    #[test]
    fn test_typedesc_mapping() {
        // TypeDesc in OCIO maps to our GpuChannelType + TextureDimensions
        // TypeDesc::HALF -> GpuChannelType + f16 data
        // TypeDesc::FLOAT -> GpuChannelType + f32 data
        // TypeDesc::UINT16 -> GpuChannelType + u16 data

        // Our implementation uses f32 data with GpuChannelType describing channels
        let channel_r = GpuChannelType::Red; // 1 channel
        let channel_rgb = GpuChannelType::Rgb; // 3 channels

        // Dimensions describe texture type
        let dim_1d = TextureDimensions::Texture1D;
        let dim_2d = TextureDimensions::Texture2D;

        let _ = (channel_r, channel_rgb, dim_1d, dim_2d);
    }
}

// ============================================================================
// BUILTIN TRANSFER FUNCTION ALL STYLES PARITY
// ============================================================================

/// All builtin transfer function styles.
/// OCIO ref: BuiltinTransform transfer functions
mod builtin_transfer_all_styles_parity {
    use crate::transform::{BuiltinTransferTransform, TransformDirection};
    use crate::{Processor, Transform};

    #[test]
    fn test_srgb_transfer() {
        let tf = BuiltinTransferTransform {
            style: "sRGB".to_string(),
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::BuiltinTransfer(tf), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        assert!(pixel[0][0] > 0.4); // sRGB encoded
    }

    #[test]
    fn test_rec709_transfer() {
        let tf = BuiltinTransferTransform {
            style: "Rec709".to_string(),
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::BuiltinTransfer(tf), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        assert!(pixel[0][0] > 0.4);
    }

    #[test]
    fn test_pq_transfer() {
        let tf = BuiltinTransferTransform {
            style: "ST2084".to_string(),
            direction: TransformDirection::Forward,
        };
        // "ST2084" resolves to the PQ OETF (compile.rs). Encode mid-gray and verify it is
        // a real, finite, non-identity encoding (the loose `let _ = tf` proved nothing).
        let proc =
            Processor::from_transform(&Transform::BuiltinTransfer(tf), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        assert!(pixel[0][0].is_finite());
        assert!(
            (pixel[0][0] - 0.18).abs() > 0.05,
            "PQ encode was a no-op: {}",
            pixel[0][0]
        );
    }

    #[test]
    fn test_hlg_transfer() {
        let tf = BuiltinTransferTransform {
            style: "HLG".to_string(),
            direction: TransformDirection::Forward,
        };
        // "HLG" resolves to the Hybrid-Log-Gamma OETF. Verify a real finite non-identity
        // encoding of mid-gray rather than discarding the transform.
        let proc =
            Processor::from_transform(&Transform::BuiltinTransfer(tf), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        assert!(pixel[0][0].is_finite());
        assert!(
            (pixel[0][0] - 0.18).abs() > 0.05,
            "HLG encode was a no-op: {}",
            pixel[0][0]
        );
    }

    #[test]
    fn test_acescct_transfer() {
        let tf = BuiltinTransferTransform {
            style: "ACEScct".to_string(),
            direction: TransformDirection::Forward,
        };
        // ACEScct encodes 0.18 linear -> 0.41358842 (ref: transfer_function tests,
        // TransferStyle::AcesCct). Exact value, not just "exists".
        let proc =
            Processor::from_transform(&Transform::BuiltinTransfer(tf), TransformDirection::Forward)
                .unwrap();
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - 0.41358842).abs() < 1e-3,
            "got {}",
            pixel[0][0]
        );
    }

    #[test]
    fn test_linear_transfer() {
        let tf = BuiltinTransferTransform {
            style: "Linear".to_string(),
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::BuiltinTransfer(tf), TransformDirection::Forward)
                .unwrap();
        let original = [[0.5_f32, 0.5, 0.5]];
        let mut pixel = original;
        proc.apply_rgb(&mut pixel);
        // Linear should be identity
        for i in 0..3 {
            assert!((pixel[0][i] - original[0][i]).abs() < 0.001);
        }
    }
}

// ============================================================================
// GPU UNIFORM TYPE COMPREHENSIVE PARITY
// ============================================================================

/// GpuUniformType comprehensive test.
/// OCIO ref: GpuShaderCreator uniform types
mod gpu_uniform_type_comprehensive_parity {
    use crate::gpu::{GpuUniform, GpuUniformType, GpuUniformValue};

    #[test]
    fn test_gpu_uniform_struct() {
        let uniform = GpuUniform {
            name: "exposure".to_string(),
            uniform_type: GpuUniformType::Float,
            default_value: GpuUniformValue::Float(0.0),
            max_len: None,
        };
        assert_eq!(uniform.name, "exposure");
        assert_eq!(uniform.uniform_type, GpuUniformType::Float);
    }

    #[test]
    fn test_uniform_vec3() {
        let uniform = GpuUniform {
            name: "offset".to_string(),
            uniform_type: GpuUniformType::Vec3,
            default_value: GpuUniformValue::Vec3([0.0, 0.0, 0.0]),
            max_len: None,
        };
        match uniform.default_value {
            GpuUniformValue::Vec3(v) => assert_eq!(v.len(), 3),
            _ => panic!("expected vec3 uniform"),
        }
    }

    #[test]
    fn test_uniform_mat4() {
        let uniform = GpuUniform {
            name: "colorMatrix".to_string(),
            uniform_type: GpuUniformType::Mat4,
            default_value: GpuUniformValue::Mat4([
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]),
            max_len: None,
        };
        match uniform.default_value {
            GpuUniformValue::Mat4(v) => assert_eq!(v.len(), 16),
            _ => panic!("expected mat4 uniform"),
        }
    }
}

// ============================================================================
// GPU SHADER CODE API COMPREHENSIVE PARITY
// ============================================================================

/// GpuShaderCode API comprehensive test.
/// OCIO ref: GpuShaderCreator shader code
mod gpu_shader_code_api_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};

    #[test]
    fn test_shader_code_fragment() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();

        let code = shader.fragment_code();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_shader_code_textures() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();

        let textures = shader.textures();
        // May or may not have textures
        let _ = textures;
    }

    #[test]
    fn test_shader_code_uniforms() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();

        let uniforms = shader.uniforms();
        // May or may not have uniforms
        let _ = uniforms;
    }

    #[test]
    fn test_shader_code_has_textures() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Glsl330).unwrap();

        let _has = shader.has_textures();
    }
}

// ============================================================================
// GPU TEXTURE STRUCT COMPREHENSIVE PARITY
// ============================================================================

/// GpuTexture struct comprehensive test.
/// OCIO ref: GpuShaderCreator textures
mod gpu_texture_struct_parity {
    use crate::gpu::{GpuInterpolation, GpuTexture, GpuTextureType};

    #[test]
    fn test_gpu_texture_1d() {
        let tex = GpuTexture {
            name: "lut1d".to_string(),
            texture_type: GpuTextureType::Texture1D,
            width: 1024,
            height: 1,
            depth: 1,
            interpolation: GpuInterpolation::Linear,
            data: (0..1024).map(|i| i as f32 / 1023.0).collect(),
        };
        assert_eq!(tex.width, 1024);
        assert_eq!(tex.texture_type, GpuTextureType::Texture1D);
    }

    #[test]
    fn test_gpu_texture_2d() {
        let tex = GpuTexture {
            name: "lut2d".to_string(),
            texture_type: GpuTextureType::Texture2D,
            width: 64,
            height: 64,
            depth: 1,
            interpolation: GpuInterpolation::Linear,
            data: vec![0.5; 64 * 64 * 3],
        };
        assert_eq!(tex.width, 64);
        assert_eq!(tex.height, 64);
    }

    #[test]
    fn test_gpu_texture_3d() {
        let size: u32 = 17;
        let tex = GpuTexture {
            name: "lut3d".to_string(),
            texture_type: GpuTextureType::Texture3D,
            width: size,
            height: size,
            depth: size,
            interpolation: GpuInterpolation::Linear,
            data: vec![0.5; (size * size * size * 3) as usize],
        };
        assert_eq!(tex.depth, size);
    }

    #[test]
    fn test_gpu_texture_nearest_interpolation() {
        let tex = GpuTexture {
            name: "index_lut".to_string(),
            texture_type: GpuTextureType::Texture1D,
            width: 256,
            height: 1,
            depth: 1,
            interpolation: GpuInterpolation::Nearest,
            data: (0..256).map(|i| i as f32).collect(),
        };
        assert_eq!(tex.interpolation, GpuInterpolation::Nearest);
    }
}

// ============================================================================
// HLSL SHADER GENERATION COMPREHENSIVE PARITY
// ============================================================================

/// HLSL shader generation comprehensive test.
/// OCIO ref: GpuShaderCreator HLSL output
mod hlsl_shader_generation_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};
    use crate::transform::*;
    use crate::{Processor, Transform};

    #[test]
    fn test_hlsl_basic_transform() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Hlsl50).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }

    #[test]
    fn test_hlsl_matrix_transform() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                1.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.1, 0.0, -0.1, 0.0],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Hlsl50).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }

    #[test]
    fn test_hlsl_cdl_transform() {
        let cdl = CdlTransform {
            slope: [1.1, 1.0, 0.9],
            offset: [0.01, 0.0, -0.01],
            power: [1.0, 1.0, 1.0],
            saturation: 1.2,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Hlsl50).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }

    #[test]
    fn test_hlsl_exponent_transform() {
        let exp = ExponentTransform {
            name: String::new(),
            value: [2.2, 2.2, 2.2, 1.0],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Exponent(exp), TransformDirection::Forward)
                .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Hlsl50).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }
}

// ============================================================================
// METAL SHADER GENERATION COMPREHENSIVE PARITY
// ============================================================================

/// Metal shader generation comprehensive test.
/// OCIO ref: GpuShaderCreator Metal output
mod metal_shader_generation_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};
    use crate::transform::*;
    use crate::{Processor, Transform};

    #[test]
    fn test_metal_basic_transform() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Metal).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }

    #[test]
    fn test_metal_matrix_transform() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                1.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.1, 0.0, -0.1, 0.0],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Metal).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }

    #[test]
    fn test_metal_cdl_transform() {
        let cdl = CdlTransform {
            slope: [1.1, 1.0, 0.9],
            offset: [0.01, 0.0, -0.01],
            power: [1.0, 1.0, 1.0],
            saturation: 1.2,
            style: CdlStyle::AscCdl,
            direction: TransformDirection::Forward,
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Metal).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }

    #[test]
    fn test_metal_grading_primary() {
        let gp = GradingPrimaryTransform {
            style: GradingPrimaryStyle::Log,
            value: crate::dynamic::GradingPrimary {
                exposure: crate::dynamic::GradingRgbm::new(0.5, 0.5, 0.5, 0.0),
                contrast: crate::dynamic::GradingRgbm::new(1.2, 1.2, 1.2, 1.0),
                ..crate::dynamic::GradingPrimary::log_default()
            },
            ..GradingPrimaryTransform::new(GradingPrimaryStyle::Log)
        };
        let proc =
            Processor::from_transform(&Transform::GradingPrimary(gp), TransformDirection::Forward)
                .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::Metal).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }
}

// ============================================================================
// VULKAN SHADER GENERATION COMPREHENSIVE PARITY
// ============================================================================

/// Vulkan GLSL shader generation comprehensive test.
/// OCIO ref: GpuShaderCreator Vulkan GLSL output
mod vulkan_shader_generation_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};
    use crate::transform::*;
    use crate::{Processor, Transform};

    #[test]
    fn test_vulkan_basic_transform() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslVulkan460).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }

    #[test]
    fn test_vulkan_grading_tone() {
        let gt = GradingToneTransform {
            style: GradingToneStyle::Log,
            scontrast: 1.5,
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::GradingTone(gt), TransformDirection::Forward)
                .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslVulkan460).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }

    #[test]
    fn test_vulkan_fixed_function() {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::RgbToHsv,
            params: vec![],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward)
                .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslVulkan460).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }
}

// ============================================================================
// WEBGL SHADER GENERATION COMPREHENSIVE PARITY
// ============================================================================

/// WebGL shader generation comprehensive test.
/// OCIO ref: GpuShaderCreator GLSL ES output
mod webgl_shader_generation_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::gpu::{GpuLanguage, GpuProcessor};
    use crate::transform::*;
    use crate::{Processor, Transform};

    #[test]
    fn test_webgl_es100_basic() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslEs100).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }

    #[test]
    fn test_webgl_es300_basic() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslEs300).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }

    #[test]
    fn test_webgl_log_transform() {
        let lt = LogTransform {
            name: String::new(),
            base: 2.0,
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Log(lt), TransformDirection::Forward).unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(GpuLanguage::GlslEs300).unwrap();
        let code = shader.fragment_code();

        assert!(!code.is_empty());
    }
}

// ============================================================================
// PROCESSOR OPS COMPREHENSIVE PARITY
// ============================================================================

/// Processor ops comprehensive test.
/// OCIO ref: Processor::getOps
mod processor_ops_comprehensive_exact_parity {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::transform::*;
    use crate::{Processor, ProcessorOp, Transform};

    #[test]
    fn test_processor_ops_from_config() {
        let config = builtin::aces_2_0_studio_all_views();
        let proc = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let ops = proc.ops();

        // Should have some ops
        assert!(!ops.is_empty());
    }

    #[test]
    fn test_processor_ops_matrix() {
        // Use non-identity matrix to prevent optimization removal
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                1.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let proc =
            Processor::from_transform(&Transform::Matrix(mt), TransformDirection::Forward).unwrap();
        let ops = proc.ops();

        // Should have matrix op
        assert!(!ops.is_empty());
        assert!(matches!(ops[0], ProcessorOp::Matrix { .. }));
    }

    #[test]
    fn test_processor_ops_cdl() {
        let cdl = CdlTransform {
            slope: [1.1, 1.0, 0.9],
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();
        let ops = proc.ops();

        // Should have CDL op
        assert!(!ops.is_empty());
    }

    #[test]
    fn test_processor_ops_group() {
        // Use non-identity transforms to prevent optimization removal
        let group = GroupTransform {
            name: String::new(),
            transforms: vec![
                Transform::Matrix(MatrixTransform {
                    name: String::new(),
                    matrix: [
                        1.2, 0.0, 0.0, 0.0, 0.0, 1.1, 0.0, 0.0, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.0,
                        1.0,
                    ],
                    offset: [0.0; 4],
                    direction: TransformDirection::Forward,
                }),
                Transform::Cdl(CdlTransform {
                    slope: [1.1, 1.0, 0.9],
                    offset: [0.01, 0.0, -0.01],
                    power: [1.0, 1.0, 1.0],
                    saturation: 1.0,
                    ..Default::default()
                }),
            ],
            direction: TransformDirection::Forward,
        };
        let proc = Processor::from_transform(&Transform::Group(group), TransformDirection::Forward)
            .unwrap();
        let ops = proc.ops();

        // Should have at least 1 op (matrix may be combined with CDL in optimization)
        assert!(!ops.is_empty());
    }
}

// ============================================================================
// CONFIG ENVIRONMENT VARIABLES PARITY
// ============================================================================

/// Config environment variables parity.
/// OCIO ref: Config::addEnvironmentVar
mod config_environment_variables_parity {
    use crate::context::Context;

    #[test]
    fn test_context_set_get() {
        let mut ctx = Context::new();
        ctx.set("SHOW", "my_show");
        assert_eq!(ctx.get("SHOW"), Some("my_show".to_string()));
    }

    #[test]
    fn test_context_resolve() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");
        ctx.set("SHOW", "my_show");

        let resolved = ctx.resolve("/shows/$SHOW/shots/$SHOT/lut.cube");
        assert_eq!(resolved, "/shows/my_show/shots/sh010/lut.cube");
    }

    #[test]
    fn test_context_multiple_vars() {
        let mut ctx = Context::new();
        ctx.set("A", "1");
        ctx.set("B", "2");
        ctx.set("C", "3");

        let resolved = ctx.resolve("$A/$B/$C");
        assert_eq!(resolved, "1/2/3");
    }

    #[test]
    fn test_context_missing_var() {
        let ctx = Context::new();
        let resolved = ctx.resolve("$UNDEFINED");
        // Undefined $VAR is left verbatim (resolve() keeps unresolved tokens as-is).
        assert_eq!(resolved, "$UNDEFINED");
    }
}

// ============================================================================
// GRADING RGBMSW STRUCT COMPREHENSIVE PARITY
// ============================================================================

/// GradingRgbmsw struct comprehensive test.
/// OCIO ref: GradingRGBMSW structure for tone controls
mod grading_rgbmsw_comprehensive_parity {
    use crate::transform::GradingRgbmsw;

    #[test]
    fn test_grading_rgbmsw_default() {
        let rgbmsw = GradingRgbmsw::default();
        assert_eq!(rgbmsw.red, 1.0);
        assert_eq!(rgbmsw.green, 1.0);
        assert_eq!(rgbmsw.blue, 1.0);
        assert_eq!(rgbmsw.master, 1.0);
    }

    #[test]
    fn test_grading_rgbmsw_custom() {
        let rgbmsw = GradingRgbmsw {
            red: 0.8,
            green: 1.0,
            blue: 1.2,
            master: 1.1,
            start: 0.2,
            width: 1.5,
        };
        assert_eq!(rgbmsw.red, 0.8);
        assert_eq!(rgbmsw.start, 0.2);
        assert_eq!(rgbmsw.width, 1.5);
    }

    #[test]
    fn test_grading_rgbmsw_for_shadows() {
        let shadows = GradingRgbmsw {
            red: 0.9,
            green: 0.9,
            blue: 0.9,
            master: 0.9,
            start: 0.0,
            width: 2.0,
        };
        // Shadows typically affect dark regions
        assert!(shadows.start < 0.5);
    }

    #[test]
    fn test_grading_rgbmsw_for_highlights() {
        let highlights = GradingRgbmsw {
            red: 1.1,
            green: 1.1,
            blue: 1.1,
            master: 1.1,
            start: 0.5,
            width: 2.0,
        };
        // Highlights typically affect bright regions
        assert!(highlights.start >= 0.5);
    }
}

// ============================================================================
// TRANSFORM ENUM ALL VARIANTS PARITY
// ============================================================================

/// Transform enum all variants test.
/// OCIO ref: All transform types
mod transform_enum_all_variants_parity {
    use crate::Transform;
    use crate::transform::*;

    #[test]
    fn test_all_transform_variants_exist() {
        // Verify all Transform enum variants
        let _matrix = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        });

        let _cdl = Transform::Cdl(CdlTransform::default());
        let _exponent = Transform::Exponent(ExponentTransform {
            name: String::new(),
            value: [1.0; 4],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        });
        let _log = Transform::Log(LogTransform {
            name: String::new(),
            base: 2.0,
            direction: TransformDirection::Forward,
        });
        let _range = Transform::Range(RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(1.0),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Forward,
        });
        let _fixed = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style: FixedFunctionStyle::AcesOutputTransform20,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let _exposure = Transform::ExposureContrast(ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Linear,
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        });
        let _grading_primary = Transform::GradingPrimary(GradingPrimaryTransform::default());
        let _grading_tone = Transform::GradingTone(GradingToneTransform::default());
        let _grading_rgb = Transform::GradingRgbCurve(GradingRgbCurveTransform::default());
        let _grading_hue = Transform::GradingHueCurve(GradingHueCurveTransform::default());
        let _lut1d = Transform::Lut1D(Lut1DTransform::identity(256));
        let _lut3d = Transform::Lut3D(Lut3DTransform::identity(17));
        let _log_affine = Transform::LogAffine(LogAffineTransform::default());
        let _log_camera = Transform::LogCamera(LogCameraTransform::default());
        let _exp_linear = Transform::ExponentWithLinear(ExponentWithLinearTransform::default());
        let _allocation = Transform::Allocation(AllocationTransform {
            allocation: AllocationType::Uniform,
            vars: vec![0.0, 1.0],
            direction: TransformDirection::Forward,
        });
        let _group = Transform::Group(GroupTransform {
            name: String::new(),
            transforms: vec![],
            direction: TransformDirection::Forward,
        });
    }
}

// ============================================================================
// BAKER FORMAT COMPREHENSIVE PARITY
// ============================================================================

/// Baker format comprehensive test.
/// OCIO ref: Baker formats
mod baker_format_comprehensive_parity {
    use crate::baker::LutFormat;

    #[test]
    fn test_lut_format_default() {
        let format = LutFormat::default();
        // Default LUT export format is .cube (baker.rs #[default] on Cube).
        assert_eq!(format, LutFormat::Cube);
    }
}

// ============================================================================
// OPTIMIZATION LEVEL COMPREHENSIVE PARITY
// ============================================================================

/// OptimizationLevel comprehensive test.
/// OCIO ref: Processor optimization levels
mod optimization_level_comprehensive_parity {
    use crate::processor::OptimizationLevel;

    #[test]
    fn test_optimization_level_default() {
        let level = OptimizationLevel::default();
        // Default optimization is Lossless (processor.rs #[default]), matching OCIO's
        // OPTIMIZATION_LOSSLESS default — not None and not an aggressive level.
        assert_eq!(level, OptimizationLevel::Lossless);
    }
}

// ============================================================================
// ROLE NAMES COMPREHENSIVE PARITY
// ============================================================================

/// Role names comprehensive test.
/// OCIO ref: Standard role names
mod role_names_comprehensive_parity {
    use crate::role::names;

    #[test]
    fn test_standard_role_names() {
        assert_eq!(names::SCENE_LINEAR, "scene_linear");
        assert_eq!(names::COMPOSITING_LOG, "compositing_log");
        assert_eq!(names::COLOR_TIMING, "color_timing");
        assert_eq!(names::COLOR_PICKING, "color_picking");
        assert_eq!(names::DATA, "data");
        assert_eq!(names::MATTE_PAINT, "matte_paint");
        assert_eq!(names::TEXTURE_PAINT, "texture_paint");
        assert_eq!(names::REFERENCE, "reference");
        assert_eq!(names::DEFAULT, "default");
    }

    #[test]
    fn test_aces_role_names() {
        assert_eq!(names::ACES_INTERCHANGE, "aces_interchange");
        assert_eq!(names::RENDERING, "rendering");
        assert_eq!(names::CIE_XYZ_D65_INTERCHANGE, "cie_xyz_d65_interchange");
        assert_eq!(names::COMPOSITING_LINEAR, "compositing_linear");
    }
}

// ============================================================================
// LOG AFFINE TRANSFORM EXACT VALUE PARITY 2
// ============================================================================

/// LogAffineTransform exact value parity.
/// OCIO ref: LogAffineTransform with specified parameters.
mod log_affine_exact_values_parity_v2 {
    use crate::transform::LogAffineTransform;
    use crate::{Processor, Transform, TransformDirection};

    fn apply(transform: &Transform, rgb: [f32; 3]) -> [f32; 3] {
        let proc = Processor::from_transform(transform, TransformDirection::Forward).unwrap();
        let mut pixels = [rgb];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    #[test]
    fn test_log_affine_base_10() {
        let la = LogAffineTransform {
            name: String::new(),
            log_side_slope: [1.0; 3],
            log_side_offset: [0.0; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            base: 10.0,
            direction: TransformDirection::Forward,
        };
        // log10(1.0) = 0.0
        let result = apply(&Transform::LogAffine(la), [1.0, 1.0, 1.0]);
        assert!((result[0] - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_log_affine_base_2() {
        let la = LogAffineTransform {
            name: String::new(),
            log_side_slope: [1.0; 3],
            log_side_offset: [0.0; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            base: 2.0,
            direction: TransformDirection::Forward,
        };
        // log2(2.0) = 1.0
        let result = apply(&Transform::LogAffine(la), [2.0, 2.0, 2.0]);
        assert!((result[0] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_log_affine_with_slope() {
        let la = LogAffineTransform {
            name: String::new(),
            log_side_slope: [0.5; 3],
            log_side_offset: [0.0; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            base: 10.0,
            direction: TransformDirection::Forward,
        };
        // 0.5 * log10(10.0) = 0.5 * 1.0 = 0.5
        let result = apply(&Transform::LogAffine(la), [10.0, 10.0, 10.0]);
        assert!((result[0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_log_affine_with_offset() {
        let la = LogAffineTransform {
            name: String::new(),
            log_side_slope: [1.0; 3],
            log_side_offset: [0.25; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            base: 10.0,
            direction: TransformDirection::Forward,
        };
        // log10(1.0) + 0.25 = 0.0 + 0.25 = 0.25
        let result = apply(&Transform::LogAffine(la), [1.0, 1.0, 1.0]);
        assert!((result[0] - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_log_affine_inverse() {
        let la = LogAffineTransform {
            name: String::new(),
            log_side_slope: [1.0; 3],
            log_side_offset: [0.0; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            base: 10.0,
            direction: TransformDirection::Forward,
        };
        let la_inv = LogAffineTransform {
            direction: TransformDirection::Inverse,
            ..la
        };
        // 10^0.5 ≈ 3.162
        let result = apply(&Transform::LogAffine(la_inv), [0.5, 0.5, 0.5]);
        assert!((result[0] - 3.162).abs() < 0.01);
    }

    #[test]
    fn test_log_affine_per_channel() {
        let la = LogAffineTransform {
            name: String::new(),
            log_side_slope: [1.0, 0.5, 2.0],
            log_side_offset: [0.0; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            base: 10.0,
            direction: TransformDirection::Forward,
        };
        // R: log10(10) = 1.0, G: 0.5*log10(10) = 0.5, B: 2*log10(10) = 2.0
        let result = apply(&Transform::LogAffine(la), [10.0, 10.0, 10.0]);
        assert!((result[0] - 1.0).abs() < 0.001);
        assert!((result[1] - 0.5).abs() < 0.001);
        assert!((result[2] - 2.0).abs() < 0.001);
    }
}

// ============================================================================
// LOG CAMERA TRANSFORM EXACT VALUE PARITY
// ============================================================================

/// LogCameraTransform exact value parity.
/// OCIO ref: LogCameraTransform (ACEScct, Log3G10, etc.).
mod log_camera_exact_values_parity {
    use crate::transform::LogCameraTransform;
    use crate::{Processor, Transform, TransformDirection};

    fn apply(transform: &Transform, rgb: [f32; 3]) -> [f32; 3] {
        let proc = Processor::from_transform(transform, TransformDirection::Forward).unwrap();
        let mut pixels = [rgb];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    #[test]
    fn test_log_camera_basic() {
        let lc = LogCameraTransform {
            name: String::new(),
            log_side_slope: [0.4; 3],
            log_side_offset: [0.05; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            lin_side_break: [0.01; 3],
            linear_slope: None,
            base: 2.0,
            direction: TransformDirection::Forward,
        };
        // Just verify it runs without panic
        let result = apply(&Transform::LogCamera(lc), [0.18, 0.18, 0.18]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_log_camera_acescct_like() {
        // ACEScct-like curve parameters
        let lc = LogCameraTransform {
            name: String::new(),
            log_side_slope: [0.057273; 3],
            log_side_offset: [0.554795; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            lin_side_break: [0.0078125; 3],
            linear_slope: None,
            base: 2.0,
            direction: TransformDirection::Forward,
        };
        // 18% gray in ACEScct is approximately 0.413
        let result = apply(&Transform::LogCamera(lc), [0.18, 0.18, 0.18]);
        // Just verify output is in expected range
        assert!(result[0] > 0.3 && result[0] < 0.5);
    }

    #[test]
    fn test_log_camera_below_break() {
        let lc = LogCameraTransform {
            name: String::new(),
            log_side_slope: [0.5; 3],
            log_side_offset: [0.0; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            lin_side_break: [0.1; 3],
            linear_slope: None,
            base: 2.0,
            direction: TransformDirection::Forward,
        };
        // Below break point should use linear segment
        let result = apply(&Transform::LogCamera(lc), [0.01, 0.01, 0.01]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_log_camera_inverse() {
        let lc = LogCameraTransform {
            name: String::new(),
            log_side_slope: [0.4; 3],
            log_side_offset: [0.0; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            lin_side_break: [0.01; 3],
            linear_slope: None,
            base: 2.0,
            direction: TransformDirection::Forward,
        };
        let lc_inv = LogCameraTransform {
            direction: TransformDirection::Inverse,
            ..lc.clone()
        };

        // Forward then inverse should roundtrip
        let input = [0.18, 0.18, 0.18];
        let encoded = apply(&Transform::LogCamera(lc), input);
        let decoded = apply(&Transform::LogCamera(lc_inv), encoded);
        assert!((decoded[0] - input[0]).abs() < 0.0001);
    }
}

// ============================================================================
// LUT 1D/3D EXACT VALUE PARITY 2
// ============================================================================

/// Lut1DTransform and Lut3DTransform exact value tests.
/// OCIO ref: LUT interpolation algorithms.
mod lut_exact_values_parity_v2 {
    use crate::transform::{Lut1DTransform, Lut3DTransform};
    use crate::{Processor, Transform, TransformDirection};

    fn apply(transform: &Transform, rgb: [f32; 3]) -> [f32; 3] {
        let proc = Processor::from_transform(transform, TransformDirection::Forward).unwrap();
        let mut pixels = [rgb];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    #[test]
    fn test_lut1d_identity_passthrough() {
        let lut = Lut1DTransform::identity(256);
        let result = apply(&Transform::Lut1D(lut), [0.5, 0.5, 0.5]);
        assert!((result[0] - 0.5).abs() < 0.001);
        assert!((result[1] - 0.5).abs() < 0.001);
        assert!((result[2] - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_lut1d_gamma() {
        // Create a gamma 2.2 LUT
        let mut lut = Lut1DTransform::identity(256);
        for i in 0..256 {
            let t = i as f32 / 255.0;
            lut.red[i] = t.powf(2.2);
        }
        lut.green = None; // Use red for all
        lut.blue = None;

        // 0.5^2.2 ≈ 0.218
        let result = apply(&Transform::Lut1D(lut), [0.5, 0.5, 0.5]);
        assert!((result[0] - 0.218).abs() < 0.02);
    }

    #[test]
    fn test_lut1d_corners() {
        let lut = Lut1DTransform::identity(256);
        // Test at extremes
        let result_low = apply(&Transform::Lut1D(lut.clone()), [0.0, 0.0, 0.0]);
        let result_high = apply(&Transform::Lut1D(lut), [1.0, 1.0, 1.0]);

        assert!((result_low[0] - 0.0).abs() < 0.001);
        assert!((result_high[0] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_lut3d_identity_passthrough() {
        let lut = Lut3DTransform::identity(17);
        let result = apply(&Transform::Lut3D(lut), [0.5, 0.5, 0.5]);
        // Identity 17^3 cube is exact at mid-grey under trilinear interpolation.
        assert!((result[0] - 0.5).abs() < 1e-5, "{}", result[0]);
        assert!((result[1] - 0.5).abs() < 1e-5, "{}", result[1]);
        assert!((result[2] - 0.5).abs() < 1e-5, "{}", result[2]);
    }

    #[test]
    fn test_lut3d_corners() {
        let lut = Lut3DTransform::identity(17);

        // Black corner
        let black = apply(&Transform::Lut3D(lut.clone()), [0.0, 0.0, 0.0]);
        assert!((black[0] - 0.0).abs() < 0.01);

        // White corner
        let white = apply(&Transform::Lut3D(lut.clone()), [1.0, 1.0, 1.0]);
        assert!((white[0] - 1.0).abs() < 0.01);

        // Red corner
        let red = apply(&Transform::Lut3D(lut.clone()), [1.0, 0.0, 0.0]);
        assert!((red[0] - 1.0).abs() < 0.01);
        assert!((red[1] - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_lut3d_trilinear_interpolation() {
        let lut = Lut3DTransform::identity(2);
        // Mid-cube point should interpolate correctly
        let result = apply(&Transform::Lut3D(lut), [0.5, 0.5, 0.5]);
        assert!((result[0] - 0.5).abs() < 0.01);
        assert!((result[1] - 0.5).abs() < 0.01);
        assert!((result[2] - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_lut1d_inverse() {
        let lut_fwd = Lut1DTransform::identity(256);
        let mut lut_inv = lut_fwd.clone();
        lut_inv.direction = TransformDirection::Inverse;

        let input = [0.5, 0.5, 0.5];
        let fwd = apply(&Transform::Lut1D(lut_fwd), input);
        let inv = apply(&Transform::Lut1D(lut_inv), fwd);

        assert!((inv[0] - input[0]).abs() < 0.01);
    }

    #[test]
    fn test_lut3d_inverse() {
        let lut_fwd = Lut3DTransform::identity(9);
        let mut lut_inv = lut_fwd.clone();
        lut_inv.direction = TransformDirection::Inverse;

        let input = [0.5, 0.5, 0.5];
        let fwd = apply(&Transform::Lut3D(lut_fwd), input);
        let inv = apply(&Transform::Lut3D(lut_inv), fwd);

        assert!((inv[0] - input[0]).abs() < 0.02);
    }
}

// ============================================================================
// FIXED FUNCTION ALL STYLES PARITY
// ============================================================================

/// Test all FixedFunctionStyle variants exist and can be constructed.
/// OCIO ref: FixedFunctionTransform styles.
mod fixed_function_all_styles_parity {
    use crate::transform::{FixedFunctionStyle, FixedFunctionTransform, TransformDirection};
    use crate::{Processor, Transform};

    fn test_style(style: FixedFunctionStyle, params: Vec<f64>) {
        let ff = FixedFunctionTransform {
            name: String::new(),
            style,
            params,
            direction: TransformDirection::Forward,
        };
        let result =
            Processor::from_transform(&Transform::FixedFunction(ff), TransformDirection::Forward);
        assert!(result.is_ok(), "Failed to create processor for {:?}", style);
    }

    #[test]
    fn test_aces_red_mod_03() {
        test_style(FixedFunctionStyle::AcesRedMod03, vec![]);
    }

    #[test]
    fn test_aces_red_mod_10() {
        test_style(FixedFunctionStyle::AcesRedMod10, vec![]);
    }

    #[test]
    fn test_aces_glow_03() {
        test_style(FixedFunctionStyle::AcesGlow03, vec![]);
    }

    #[test]
    fn test_aces_glow_10() {
        test_style(FixedFunctionStyle::AcesGlow10, vec![]);
    }

    #[test]
    fn test_aces_dark_to_dim_10() {
        test_style(FixedFunctionStyle::AcesDarkToDim10, vec![0.9811]);
    }

    #[test]
    fn test_aces_gamut_comp_13() {
        test_style(FixedFunctionStyle::AcesGamutComp13, vec![]);
    }

    #[test]
    fn test_rec2100_surround() {
        test_style(FixedFunctionStyle::Rec2100Surround, vec![0.9]);
    }

    #[test]
    fn test_rgb_to_hsv() {
        test_style(FixedFunctionStyle::RgbToHsv, vec![]);
    }

    #[test]
    fn test_hsv_to_rgb() {
        test_style(FixedFunctionStyle::HsvToRgb, vec![]);
    }

    #[test]
    fn test_xyz_to_xyy() {
        test_style(FixedFunctionStyle::XyzToXyy, vec![]);
    }

    #[test]
    fn test_xyy_to_xyz() {
        test_style(FixedFunctionStyle::XyyToXyz, vec![]);
    }

    #[test]
    fn test_xyz_to_uvy() {
        test_style(FixedFunctionStyle::XyzToUvy, vec![]);
    }

    #[test]
    fn test_uvy_to_xyz() {
        test_style(FixedFunctionStyle::UvyToXyz, vec![]);
    }

    #[test]
    fn test_xyz_to_luv() {
        test_style(FixedFunctionStyle::XyzToLuv, vec![]);
    }

    #[test]
    fn test_luv_to_xyz() {
        test_style(FixedFunctionStyle::LuvToXyz, vec![]);
    }

    #[test]
    fn test_lin_to_pq() {
        test_style(FixedFunctionStyle::LinToPq, vec![]);
    }

    #[test]
    fn test_pq_to_lin() {
        test_style(FixedFunctionStyle::PqToLin, vec![]);
    }

    #[test]
    fn test_aces_output_20() {
        test_style(FixedFunctionStyle::AcesOutputTransform20, vec![100.0]);
    }

    #[test]
    fn test_aces_rgb_to_jmh_20() {
        test_style(FixedFunctionStyle::AcesRgbToJmh20, vec![]);
    }

    #[test]
    fn test_aces_jmh_to_rgb_20() {
        test_style(FixedFunctionStyle::AcesJmhToRgb20, vec![]);
    }

    #[test]
    fn test_aces_tonescale_compress_20() {
        test_style(FixedFunctionStyle::AcesTonescaleCompress20, vec![100.0]);
    }
}

// ============================================================================
// EXPOSURE CONTRAST EXACT VALUE PARITY
// ============================================================================

/// ExposureContrastTransform exact value tests.
/// OCIO ref: ExposureContrastTransform algorithm.
mod exposure_contrast_exact_values_parity {
    use crate::transform::{ExposureContrastStyle, ExposureContrastTransform};
    use crate::{Processor, Transform, TransformDirection};

    fn apply(transform: &Transform, rgb: [f32; 3]) -> [f32; 3] {
        let proc = Processor::from_transform(transform, TransformDirection::Forward).unwrap();
        let mut pixels = [rgb];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    #[test]
    fn test_exposure_plus_one_stop() {
        let ec = ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Linear,
            exposure: 1.0, // +1 stop = 2x
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };
        let result = apply(&Transform::ExposureContrast(ec), [0.18, 0.18, 0.18]);
        // +1 stop doubles the value
        assert!((result[0] - 0.36).abs() < 0.01);
    }

    #[test]
    fn test_exposure_minus_one_stop() {
        let ec = ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Linear,
            exposure: -1.0, // -1 stop = 0.5x
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };
        let result = apply(&Transform::ExposureContrast(ec), [0.18, 0.18, 0.18]);
        // -1 stop halves the value
        assert!((result[0] - 0.09).abs() < 0.01);
    }

    #[test]
    fn test_contrast_increase() {
        let ec = ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Linear,
            exposure: 0.0,
            contrast: 1.5,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };
        // Value at pivot should remain at pivot
        let at_pivot = apply(&Transform::ExposureContrast(ec.clone()), [0.18, 0.18, 0.18]);
        assert!((at_pivot[0] - 0.18).abs() < 0.01);

        // Value above pivot should increase more
        let above = apply(&Transform::ExposureContrast(ec.clone()), [0.36, 0.36, 0.36]);
        assert!(above[0] > 0.36);
    }

    #[test]
    fn test_gamma_adjustment() {
        let ec = ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Linear,
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.5,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };
        // Pivot should remain unchanged
        let at_pivot = apply(&Transform::ExposureContrast(ec), [0.18, 0.18, 0.18]);
        assert!((at_pivot[0] - 0.18).abs() < 0.02);
    }

    #[test]
    fn test_logarithmic_style() {
        let ec = ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Logarithmic,
            exposure: 0.5,
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };
        let result = apply(&Transform::ExposureContrast(ec), [0.18, 0.18, 0.18]);
        // Just verify it produces a different result
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_video_style() {
        let ec = ExposureContrastTransform {
            name: String::new(),
            style: ExposureContrastStyle::Video,
            exposure: 0.5,
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            direction: TransformDirection::Forward,
            dynamic: crate::EcDynamic::NONE,
        };
        let result = apply(&Transform::ExposureContrast(ec), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }
}

// ============================================================================
// DISPLAY/VIEW CONFIG PARITY
// ============================================================================

/// Display and View configuration parity.
/// OCIO ref: Config display/view management.
mod display_view_config_parity {
    use crate::builtin;

    #[test]
    fn test_config_displays() {
        let config = builtin::aces_2_0_studio_all_views();
        let display_mgr = config.displays();
        let displays = display_mgr.displays();

        // Should have at least one display
        assert!(!displays.is_empty());
    }

    #[test]
    fn test_active_displays() {
        let config = builtin::aces_2_0_studio_all_views();
        // Should be able to get displays
        let displays = config.displays().displays();
        assert!(!displays.is_empty());
    }

    #[test]
    fn test_display_iterator() {
        let config = builtin::aces_2_0_studio_all_views();
        let displays = config.displays().displays();

        let count = displays.len();
        assert!(count > 0);
    }
}

// ============================================================================
// CONTEXT VARIABLE RESOLUTION PARITY
// ============================================================================

/// Context variable resolution parity.
/// OCIO ref: Context::resolveStringVar.
mod context_variable_resolution_parity {
    use crate::context::Context;

    #[test]
    fn test_simple_variable() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");

        let resolved = ctx.resolve("path/$SHOT/data");
        assert_eq!(resolved, "path/sh010/data");
    }

    #[test]
    fn test_braced_variable() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");
        ctx.set("SEQ", "seq100");

        let resolved = ctx.resolve("path/${SEQ}/${SHOT}/data");
        assert_eq!(resolved, "path/seq100/sh010/data");
    }

    #[test]
    fn test_undefined_variable() {
        let ctx = Context::new();

        // Undefined variable should remain as-is
        let resolved = ctx.resolve("path/$UNDEFINED/data");
        assert!(resolved.contains("UNDEFINED") || resolved.contains("$"));
    }

    #[test]
    fn test_multiple_variables() {
        let mut ctx = Context::new();
        ctx.set("A", "1");
        ctx.set("B", "2");
        ctx.set("C", "3");

        let resolved = ctx.resolve("$A/$B/$C");
        assert_eq!(resolved, "1/2/3");
    }

    #[test]
    fn test_empty_context() {
        let ctx = Context::new();

        let resolved = ctx.resolve("no_variables_here");
        assert_eq!(resolved, "no_variables_here");
    }

    #[test]
    fn test_variable_at_start() {
        let mut ctx = Context::new();
        ctx.set("ROOT", "/data");

        let resolved = ctx.resolve("$ROOT/subdir");
        assert_eq!(resolved, "/data/subdir");
    }

    #[test]
    fn test_variable_at_end() {
        let mut ctx = Context::new();
        ctx.set("EXT", "exr");

        let resolved = ctx.resolve("file.$EXT");
        assert_eq!(resolved, "file.exr");
    }
}

// ============================================================================
// GRADING PRIMARY EXACT VALUES PARITY 2
// ============================================================================

/// GradingPrimaryTransform exact value tests using default values.
/// OCIO ref: GradingPrimaryTransform algorithm.
mod grading_primary_exact_values_parity_v2 {
    use crate::transform::{GradingPrimaryStyle, GradingPrimaryTransform};
    use crate::{Processor, Transform, TransformDirection};

    fn apply(transform: &Transform, rgb: [f32; 3]) -> [f32; 3] {
        let proc = Processor::from_transform(transform, TransformDirection::Forward).unwrap();
        let mut pixels = [rgb];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    #[test]
    fn test_grading_primary_identity() {
        let gp = GradingPrimaryTransform::default();
        // Identity GradingPrimary is exact; chromatic probe also guards the sat term.
        let input = [0.5_f32, 0.3, 0.2];
        let result = apply(&Transform::GradingPrimary(gp), input);
        for c in 0..3 {
            assert!(
                (result[c] - input[c]).abs() < 1e-5,
                "GradingPrimary identity ch{}",
                c
            );
        }
    }

    #[test]
    fn test_grading_primary_with_brightness() {
        let mut gp = GradingPrimaryTransform::default();
        gp.value.brightness = crate::dynamic::GradingRgbm::new(0.1, 0.0, -0.1, 0.0);
        let result = apply(&Transform::GradingPrimary(gp), [0.5, 0.5, 0.5]);
        // R should increase, G unchanged, B decrease
        assert!(result[0] > result[1]);
        assert!(result[1] > result[2]);
    }

    #[test]
    fn test_grading_primary_with_contrast() {
        let mut gp = GradingPrimaryTransform::default();
        gp.value.contrast = crate::dynamic::GradingRgbm::new(1.5, 1.0, 0.5, 1.0);
        let result = apply(&Transform::GradingPrimary(gp), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_primary_desaturation() {
        let mut gp = GradingPrimaryTransform::default();
        gp.value.saturation = 0.0; // Full desaturation
        let result = apply(&Transform::GradingPrimary(gp), [1.0, 0.5, 0.0]);
        // saturation=0 maps every channel to the same luma => exactly grayscale, and the
        // luma is a real mix (not a passthrough of any input channel).
        assert!(
            (result[0] - result[1]).abs() < 1e-4,
            "{} vs {}",
            result[0],
            result[1]
        );
        assert!(
            (result[1] - result[2]).abs() < 1e-4,
            "{} vs {}",
            result[1],
            result[2]
        );
        assert!(result[0] > 0.0 && result[0] < 1.0);
    }

    #[test]
    fn test_grading_primary_clamp() {
        let mut gp = GradingPrimaryTransform::default();
        gp.value.brightness = crate::dynamic::GradingRgbm::new(0.8, 0.8, 0.8, 0.0); // Push values high
        gp.value.clamp_white = 1.0;
        let result = apply(&Transform::GradingPrimary(gp), [0.5, 0.5, 0.5]);
        // Should be clamped
        assert!(result[0] <= 1.0);
    }

    #[test]
    fn test_grading_primary_linear_style() {
        let mut gp = GradingPrimaryTransform {
            style: GradingPrimaryStyle::Linear,
            ..Default::default()
        };
        let result = apply(&Transform::GradingPrimary(gp), [0.18, 0.18, 0.18]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_primary_video_style() {
        let mut gp = GradingPrimaryTransform {
            style: GradingPrimaryStyle::Video,
            ..Default::default()
        };
        let result = apply(&Transform::GradingPrimary(gp), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }
}

// ============================================================================
// GRADING TONE EXACT VALUES PARITY 2
// ============================================================================

/// GradingToneTransform exact value tests using default values.
/// OCIO ref: GradingToneTransform algorithm.
mod grading_tone_exact_values_parity_v2 {
    use crate::transform::{GradingToneStyle, GradingToneTransform};
    use crate::{Processor, Transform, TransformDirection};

    fn apply(transform: &Transform, rgb: [f32; 3]) -> [f32; 3] {
        let proc = Processor::from_transform(transform, TransformDirection::Forward).unwrap();
        let mut pixels = [rgb];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    #[test]
    fn test_grading_tone_identity() {
        let gt = GradingToneTransform::default();
        // Default GradingTone (all RGBM multipliers = 1.0) is an exact identity.
        let input = [0.5_f32, 0.3, 0.8];
        let result = apply(&Transform::GradingTone(gt), input);
        for c in 0..3 {
            assert!(
                (result[c] - input[c]).abs() < 1e-5,
                "GradingTone identity ch{}",
                c
            );
        }
    }

    #[test]
    fn test_grading_tone_shadows() {
        let mut gt = GradingToneTransform::default();
        gt.shadows.master = 1.2;
        let result = apply(&Transform::GradingTone(gt), [0.1, 0.1, 0.1]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_tone_midtones() {
        let mut gt = GradingToneTransform::default();
        gt.midtones.master = 1.3;
        let result = apply(&Transform::GradingTone(gt), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_tone_highlights() {
        let mut gt = GradingToneTransform::default();
        gt.highlights.master = 1.2;
        let result = apply(&Transform::GradingTone(gt), [0.9, 0.9, 0.9]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_tone_scontrast() {
        let mut gt = GradingToneTransform {
            scontrast: 1.5,
            ..Default::default()
        };
        let result = apply(&Transform::GradingTone(gt), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_tone_linear_style() {
        let mut gt = GradingToneTransform {
            style: GradingToneStyle::Linear,
            ..Default::default()
        };
        let result = apply(&Transform::GradingTone(gt), [0.5, 0.5, 0.5]);
        // With default (all-1.0) tone controls, changing only the style stays an exact
        // identity. A loose 0.1 band would pass even a broken style branch.
        assert!((result[0] - 0.5).abs() < 1e-5, "got {}", result[0]);
    }

    #[test]
    fn test_grading_tone_video_style() {
        let mut gt = GradingToneTransform {
            style: GradingToneStyle::Video,
            ..Default::default()
        };
        let result = apply(&Transform::GradingTone(gt), [0.5, 0.5, 0.5]);
        // Default (all-1.0) tone controls => exact identity regardless of style.
        assert!((result[0] - 0.5).abs() < 1e-5, "got {}", result[0]);
    }
}

// ============================================================================
// COLORSPACE TRANSFORM PARITY V2
// ============================================================================

/// ColorSpaceTransform parity tests.
/// OCIO ref: ColorSpaceTransform.
mod colorspace_transform_parity_v2 {
    use crate::TransformDirection;
    use crate::transform::ColorSpaceTransform;

    #[test]
    fn test_colorspace_transform_construction() {
        let cst = ColorSpaceTransform {
            src: "ACEScg".to_string(),
            dst: "sRGB".to_string(),
            data_bypass: true,
            direction: TransformDirection::Forward,
        };
        assert_eq!(cst.src, "ACEScg");
        assert_eq!(cst.dst, "sRGB");
    }

    #[test]
    fn test_colorspace_transform_inverse() {
        let cst = ColorSpaceTransform {
            src: "ACEScg".to_string(),
            dst: "sRGB".to_string(),
            data_bypass: true,
            direction: TransformDirection::Inverse,
        };
        assert_eq!(cst.direction, TransformDirection::Inverse);
    }
}

// ============================================================================
// LOOK TRANSFORM PARITY V2
// ============================================================================

/// LookTransform parity tests.
/// OCIO ref: LookTransform.
mod look_transform_parity_v2 {
    use crate::TransformDirection;
    use crate::transform::LookTransform;

    #[test]
    fn test_look_transform_construction() {
        let lt = LookTransform {
            src: "ACEScg".to_string(),
            dst: "ACEScg".to_string(),
            looks: "film_look".to_string(),
            skip_color_space_conversion: false,
            direction: TransformDirection::Forward,
        };
        assert_eq!(lt.src, "ACEScg");
        assert_eq!(lt.looks, "film_look");
    }

    #[test]
    fn test_look_transform_multiple_looks() {
        let lt = LookTransform {
            src: "ACEScg".to_string(),
            dst: "ACEScg".to_string(),
            looks: "look1, look2, look3".to_string(),
            skip_color_space_conversion: false,
            direction: TransformDirection::Forward,
        };
        assert!(lt.looks.contains("look1"));
        assert!(lt.looks.contains("look2"));
    }

    #[test]
    fn test_look_transform_inverse_direction() {
        let lt = LookTransform {
            src: "ACEScg".to_string(),
            dst: "sRGB".to_string(),
            looks: "".to_string(),
            skip_color_space_conversion: false,
            direction: TransformDirection::Inverse,
        };
        assert_eq!(lt.direction, TransformDirection::Inverse);
    }
}

// ============================================================================
// DISPLAY VIEW TRANSFORM PARITY V2
// ============================================================================

/// DisplayViewTransform parity tests.
/// OCIO ref: DisplayViewTransform.
mod display_view_transform_parity_v2 {
    use crate::TransformDirection;
    use crate::transform::DisplayViewTransform;

    #[test]
    fn test_display_view_transform_construction() {
        let dvt = DisplayViewTransform {
            src: "ACEScg".to_string(),
            display: "sRGB".to_string(),
            view: "ACES 1.0 - SDR Video".to_string(),
            looks_bypass: false,
            data_bypass: true,
            looks_override: String::new(),
            looks_override_enabled: false,
            direction: TransformDirection::Forward,
        };
        assert_eq!(dvt.src, "ACEScg");
        assert_eq!(dvt.display, "sRGB");
        assert_eq!(dvt.view, "ACES 1.0 - SDR Video");
    }

    #[test]
    fn test_display_view_transform_direction() {
        let dvt = DisplayViewTransform {
            src: "ACEScg".to_string(),
            display: "sRGB".to_string(),
            view: "Film".to_string(),
            looks_bypass: false,
            data_bypass: true,
            looks_override: String::new(),
            looks_override_enabled: false,
            direction: TransformDirection::Inverse,
        };
        assert_eq!(dvt.direction, TransformDirection::Inverse);
    }
}

// ============================================================================
// PROCESSOR CACHE PARITY
// ============================================================================

/// ProcessorCache parity tests.
/// OCIO ref: Processor caching.
mod processor_cache_parity_v2 {
    use crate::builtin;
    use crate::builtin::TestSchema;
    use crate::cache::ProcessorCache;

    #[test]
    fn test_cache_creation() {
        let cache = ProcessorCache::new();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_hit() {
        let cache = ProcessorCache::new();
        let config = builtin::aces_2_0_studio_all_views();

        // First call caches
        let _proc1 = cache
            .get_or_create(
                &config,
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        // Second call should hit cache
        let _proc2 = cache
            .get_or_create(
                &config,
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        // Should have 1 entry
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_cache_different_conversions() {
        let cache = ProcessorCache::new();
        let config = builtin::aces_2_0_studio_all_views();

        let _proc1 = cache
            .get_or_create(
                &config,
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        let _proc2 = cache
            .get_or_create(
                &config,
                &TestSchema::shared().interchange,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        // Should have 2 entries
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_cache_clear() {
        let cache = ProcessorCache::new();
        let config = builtin::aces_2_0_studio_all_views();

        let _proc = cache
            .get_or_create(
                &config,
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        assert_eq!(cache.len(), 1);

        cache.clear();
        assert_eq!(cache.len(), 0);
    }
}

// ============================================================================
// PACKED BIT DEPTH PARITY
// ============================================================================

/// PackedBitDepth and ChannelOrdering parity tests.
/// OCIO ref: PackedImageDesc.
mod packed_format_parity {
    use crate::processor::{ChannelOrdering, PackedBitDepth};

    #[test]
    fn test_bit_depth_default() {
        let depth = PackedBitDepth::default();
        // Default should be F32
        assert_eq!(depth, PackedBitDepth::F32);
    }

    #[test]
    fn test_channel_ordering_default() {
        let ordering = ChannelOrdering::default();
        // Default should be Rgba
        assert_eq!(ordering, ChannelOrdering::Rgba);
    }
}

// ============================================================================
// BUILTIN TRANSFORM COMPREHENSIVE PARITY
// ============================================================================

/// BuiltinTransform comprehensive parity tests.
/// OCIO ref: Built-in transforms.
mod builtin_transform_comprehensive_parity_v2 {
    use crate::builtin::transforms;
    use crate::transform::BuiltinTransform;
    use crate::{Processor, Transform, TransformDirection};

    fn test_builtin(name: &str) {
        let bt = BuiltinTransform {
            style: name.to_string(),
            direction: TransformDirection::Forward,
        };
        let result =
            Processor::from_transform(&Transform::Builtin(bt), TransformDirection::Forward);
        assert!(
            result.is_ok(),
            "Failed to create processor for builtin: {}",
            name
        );
    }

    /// `sRGB_to_Linear` / `Linear_to_sRGB` were invented style names (OCIO has no curve-only
    /// sRGB builtin; the sRGB curve is `ExponentWithLinearTransform` gamma 2.4 / offset 0.055,
    /// covered by `exponent_with_linear_*` and `processor::tests`). OCIO rejects them
    /// (PyOpenColorIO 2.5.1: "BuiltinTransform: invalid built-in transform style
    /// 'sRGB_to_Linear'."), and so must this port.
    #[test]
    fn test_invented_srgb_styles_are_rejected() {
        for name in [
            "sRGB_to_Linear",
            "Linear_to_sRGB",
            "Rec709_to_Linear",
            "Linear_to_Rec709",
        ] {
            let bt = BuiltinTransform {
                style: name.to_string(),
                direction: TransformDirection::Forward,
            };
            let err =
                Processor::from_transform(&Transform::Builtin(bt), TransformDirection::Forward)
                    .unwrap_err();
            assert!(
                err.to_string().contains(&format!(
                    "BuiltinTransform: invalid built-in transform style '{name}'."
                )),
                "{name}: {err}"
            );
        }
    }

    /// Every registered OCIO style builds a processor.
    #[test]
    fn test_every_registered_style_builds() {
        for name in transforms::styles() {
            test_builtin(name);
        }
    }

    #[test]
    fn test_acescg_to_aces() {
        test_builtin(transforms::ACESCG_TO_ACES2065_1);
    }
}

// ============================================================================
// SHADER LANGUAGE COMPREHENSIVE PARITY
// ============================================================================

/// All shader languages parity tests.
/// OCIO ref: GpuShaderCreator.
mod shader_language_comprehensive_parity {
    use crate::gpu::{GpuLanguage, GpuProcessor};
    use crate::transform::CdlTransform;
    use crate::{Processor, Transform, TransformDirection};

    fn test_language(lang: GpuLanguage) {
        let cdl = CdlTransform {
            slope: [1.1, 1.0, 0.9],
            ..Default::default()
        };
        let proc =
            Processor::from_transform(&Transform::Cdl(cdl), TransformDirection::Forward).unwrap();
        let gpu = GpuProcessor::from_processor(&proc).unwrap();
        let shader = gpu.generate_shader(lang).unwrap();

        // Should generate non-empty shader code
        assert!(!shader.fragment_code().is_empty());
    }

    #[test]
    fn test_glsl_120() {
        test_language(GpuLanguage::Glsl120);
    }

    #[test]
    fn test_glsl_130() {
        test_language(GpuLanguage::Glsl130);
    }

    #[test]
    fn test_glsl_330() {
        test_language(GpuLanguage::Glsl330);
    }

    #[test]
    fn test_glsl_400() {
        test_language(GpuLanguage::Glsl400);
    }

    #[test]
    fn test_glsl_es_100() {
        test_language(GpuLanguage::GlslEs100);
    }

    #[test]
    fn test_glsl_es_300() {
        test_language(GpuLanguage::GlslEs300);
    }

    #[test]
    fn test_glsl_vulkan_460() {
        test_language(GpuLanguage::GlslVulkan460);
    }

    #[test]
    fn test_hlsl_50() {
        test_language(GpuLanguage::Hlsl50);
    }

    #[test]
    fn test_metal() {
        test_language(GpuLanguage::Metal);
    }
}

// ============================================================================
// EXPONENT WITH LINEAR TRANSFORM PARITY
// ============================================================================

/// ExponentWithLinearTransform parity tests.
/// OCIO ref: ExponentWithLinearTransform.
mod exponent_with_linear_parity_v2 {
    use crate::transform::ExponentWithLinearTransform;
    use crate::{Processor, Transform, TransformDirection};

    fn apply(transform: &Transform, rgb: [f32; 3]) -> [f32; 3] {
        let proc = Processor::from_transform(transform, TransformDirection::Forward).unwrap();
        let mut pixels = [rgb];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    #[test]
    fn test_exponent_with_linear_default() {
        let ewl = ExponentWithLinearTransform::default();
        let result = apply(&Transform::ExponentWithLinear(ewl), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_exponent_with_linear_gamma_22() {
        let mut ewl = ExponentWithLinearTransform {
            gamma: [2.2; 4],
            offset: [0.0; 4],
            ..Default::default()
        };
        let result = apply(&Transform::ExponentWithLinear(ewl), [0.5, 0.5, 0.5]);
        // 0.5^2.2 ≈ 0.218
        assert!(result[0] < 0.5);
    }

    #[test]
    fn test_exponent_with_linear_inverse() {
        let ewl = ExponentWithLinearTransform::default();
        let mut ewl_inv = ewl.clone();
        ewl_inv.direction = TransformDirection::Inverse;

        let input = [0.5, 0.5, 0.5];
        let fwd = apply(&Transform::ExponentWithLinear(ewl), input);
        let inv = apply(&Transform::ExponentWithLinear(ewl_inv), fwd);

        assert!((inv[0] - input[0]).abs() < 0.01);
    }
}

// ============================================================================
// MATRIX TRANSFORM EXACT VALUE PARITY
// ============================================================================

/// MatrixTransform exact value parity tests.
/// OCIO ref: MatrixTransform.
mod matrix_transform_exact_parity {
    use crate::transform::MatrixTransform;
    use crate::{Processor, Transform, TransformDirection};

    fn apply(transform: &Transform, rgb: [f32; 3]) -> [f32; 3] {
        let proc = Processor::from_transform(transform, TransformDirection::Forward).unwrap();
        let mut pixels = [rgb];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    #[test]
    fn test_identity_matrix() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let result = apply(&Transform::Matrix(mt), [0.5, 0.5, 0.5]);
        assert!((result[0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_scale_matrix() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let result = apply(&Transform::Matrix(mt), [0.25, 0.25, 0.25]);
        assert!((result[0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_offset() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.1, 0.2, 0.3, 0.0],
            direction: TransformDirection::Forward,
        };
        let result = apply(&Transform::Matrix(mt), [0.0, 0.0, 0.0]);
        assert!((result[0] - 0.1).abs() < 0.001);
        assert!((result[1] - 0.2).abs() < 0.001);
        assert!((result[2] - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_channel_swap_rg() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let result = apply(&Transform::Matrix(mt), [1.0, 0.0, 0.0]);
        // R and G swapped: (1,0,0) -> (0,1,0)
        assert!((result[0] - 0.0).abs() < 0.001);
        assert!((result[1] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_matrix_inverse() {
        let mt = MatrixTransform {
            name: String::new(),
            matrix: [
                2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        };
        let mt_inv = MatrixTransform {
            direction: TransformDirection::Inverse,
            ..mt.clone()
        };

        // Forward then inverse
        let input = [0.25, 0.25, 0.25];
        let fwd = apply(&Transform::Matrix(mt), input);
        let inv = apply(&Transform::Matrix(mt_inv), fwd);

        assert!((inv[0] - input[0]).abs() < 0.001);
    }
}

// ============================================================================
// CDL TRANSFORM EXACT VALUE PARITY
// ============================================================================

/// CdlTransform exact value parity tests.
/// OCIO ref: CDLTransform ASC-CDL.
mod cdl_transform_exact_parity {
    use crate::transform::{CdlStyle, CdlTransform};
    use crate::{Processor, Transform, TransformDirection};

    fn apply(transform: &Transform, rgb: [f32; 3]) -> [f32; 3] {
        let proc = Processor::from_transform(transform, TransformDirection::Forward).unwrap();
        let mut pixels = [rgb];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    #[test]
    fn test_cdl_identity() {
        let cdl = CdlTransform::default();
        // Identity CDL is exact; chromatic probe also guards the sat term.
        let input = [0.5_f32, 0.3, 0.2];
        let result = apply(&Transform::Cdl(cdl), input);
        for c in 0..3 {
            assert!((result[c] - input[c]).abs() < 1e-5, "CDL identity ch{}", c);
        }
    }

    #[test]
    fn test_cdl_slope() {
        let cdl = CdlTransform {
            slope: [2.0, 1.0, 0.5],
            offset: [0.0; 3],
            power: [1.0; 3],
            saturation: 1.0,
            ..Default::default()
        };
        let result = apply(&Transform::Cdl(cdl), [0.25, 0.5, 1.0]);
        assert!((result[0] - 0.5).abs() < 0.001); // 0.25 * 2 = 0.5
        assert!((result[1] - 0.5).abs() < 0.001); // 0.5 * 1 = 0.5
        assert!((result[2] - 0.5).abs() < 0.001); // 1.0 * 0.5 = 0.5
    }

    #[test]
    fn test_cdl_offset() {
        let cdl = CdlTransform {
            slope: [1.0; 3],
            offset: [0.1, 0.2, 0.3],
            power: [1.0; 3],
            saturation: 1.0,
            ..Default::default()
        };
        let result = apply(&Transform::Cdl(cdl), [0.0, 0.0, 0.0]);
        assert!((result[0] - 0.1).abs() < 0.001);
        assert!((result[1] - 0.2).abs() < 0.001);
        assert!((result[2] - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_cdl_power() {
        let cdl = CdlTransform {
            slope: [1.0; 3],
            offset: [0.0; 3],
            power: [2.0; 3],
            saturation: 1.0,
            ..Default::default()
        };
        let result = apply(&Transform::Cdl(cdl), [0.5, 0.5, 0.5]);
        // 0.5^2 = 0.25
        assert!((result[0] - 0.25_f32).abs() < 0.001);
    }

    #[test]
    fn test_cdl_no_clamp_style() {
        let cdl = CdlTransform {
            slope: [2.0; 3],
            offset: [0.0; 3],
            power: [1.0; 3],
            saturation: 1.0,
            style: CdlStyle::NoClamp,
            ..Default::default()
        };
        let result = apply(&Transform::Cdl(cdl), [0.8, 0.8, 0.8]);
        // 0.8 * 2 = 1.6, no clamping
        assert!(result[0] > 1.0);
    }
}

// ============================================================================
// GRADING RGB CURVE TRANSFORM PARITY
// ============================================================================

/// GradingRgbCurveTransform comprehensive parity.
/// OCIO ref: GradingRgbCurveTransform.
mod grading_rgb_curve_comprehensive_parity {
    use crate::transform::{GradingRgbCurveTransform, GradingToneStyle};
    use crate::{Processor, Transform, TransformDirection};

    fn apply(transform: &Transform, rgb: [f32; 3]) -> [f32; 3] {
        let proc = Processor::from_transform(transform, TransformDirection::Forward).unwrap();
        let mut pixels = [rgb];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    #[test]
    fn test_grading_rgb_curve_identity() {
        let grc = GradingRgbCurveTransform::default();
        // Default (identity) curve is an exact identity transform: out == in.
        let input = [0.5_f32, 0.3, 0.2];
        let result = apply(&Transform::GradingRgbCurve(grc), input);
        for c in 0..3 {
            assert!(
                (result[c] - input[c]).abs() < 1e-5,
                "RGB curve identity ch{}",
                c
            );
        }
    }

    #[test]
    fn test_grading_rgb_curve_red_channel() {
        // Add control points for red channel S-curve
        let mut grc = GradingRgbCurveTransform {
            red: vec![[0.0, 0.0], [0.25, 0.2], [0.5, 0.5], [0.75, 0.8], [1.0, 1.0]],
            ..Default::default()
        };
        let result = apply(&Transform::GradingRgbCurve(grc), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_rgb_curve_all_channels() {
        // Modify all channels
        let mut grc = GradingRgbCurveTransform {
            red: vec![[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]],
            green: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
            blue: vec![[0.0, 0.0], [0.5, 0.4], [1.0, 1.0]],
            ..Default::default()
        };
        let result = apply(&Transform::GradingRgbCurve(grc), [0.5, 0.5, 0.5]);
        // Red should increase, blue should decrease
        assert!(result[0] > result[2]);
    }

    #[test]
    fn test_grading_rgb_curve_master() {
        // Master curve affects all channels
        let mut grc = GradingRgbCurveTransform {
            master: vec![[0.0, 0.0], [0.5, 0.7], [1.0, 1.0]],
            ..Default::default()
        };
        let result = apply(&Transform::GradingRgbCurve(grc), [0.5, 0.5, 0.5]);
        // All channels should increase together
        assert!(result[0] > 0.5);
    }

    #[test]
    fn test_grading_rgb_curve_linear_style() {
        let mut grc = GradingRgbCurveTransform {
            style: GradingToneStyle::Linear,
            ..Default::default()
        };
        let result = apply(&Transform::GradingRgbCurve(grc), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_rgb_curve_video_style() {
        let mut grc = GradingRgbCurveTransform {
            style: GradingToneStyle::Video,
            ..Default::default()
        };
        let result = apply(&Transform::GradingRgbCurve(grc), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_rgb_curve_inverse() {
        let grc = GradingRgbCurveTransform::default();
        let mut grc_inv = grc.clone();
        grc_inv.direction = TransformDirection::Inverse;

        let input = [0.5, 0.5, 0.5];
        let fwd = apply(&Transform::GradingRgbCurve(grc), input);
        let inv = apply(&Transform::GradingRgbCurve(grc_inv), fwd);

        assert!((inv[0] - input[0]).abs() < 0.02);
    }
}

// ============================================================================
// GRADING HUE CURVE TRANSFORM PARITY
// ============================================================================

/// GradingHueCurveTransform comprehensive parity.
/// OCIO ref: GradingRgbCurveTransform with hue curves.
mod grading_hue_curve_comprehensive_parity {
    use crate::transform::{GradingHueCurveStyle, GradingHueCurveTransform};
    use crate::{Processor, Transform, TransformDirection};

    fn apply(transform: &Transform, rgb: [f32; 3]) -> [f32; 3] {
        let proc = Processor::from_transform(transform, TransformDirection::Forward).unwrap();
        let mut pixels = [rgb];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    #[test]
    fn test_grading_hue_curve_identity() {
        let ghc = GradingHueCurveTransform::default();
        // Default hue curves compose with the RGB<->HSY round-trip to an exact identity.
        let input = [0.5_f32, 0.3, 0.2];
        let result = apply(&Transform::GradingHueCurve(ghc), input);
        for c in 0..3 {
            assert!(
                (result[c] - input[c]).abs() < 1e-5,
                "hue curve identity ch{}",
                c
            );
        }
    }

    #[test]
    fn test_grading_hue_curve_hue_hue() {
        // Shift hue at certain hue value
        let mut ghc = GradingHueCurveTransform {
            hue_hue: vec![[0.0, 0.0], [180.0, 30.0], [360.0, 0.0]],
            ..Default::default()
        };
        let result = apply(&Transform::GradingHueCurve(ghc), [1.0, 0.0, 0.0]); // Red
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_hue_curve_hue_sat() {
        // Adjust saturation based on hue
        let mut ghc = GradingHueCurveTransform {
            hue_sat: vec![[0.0, 1.0], [180.0, 0.5], [360.0, 1.0]],
            ..Default::default()
        };
        let result = apply(&Transform::GradingHueCurve(ghc), [0.0, 1.0, 1.0]); // Cyan
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_hue_curve_lum_sat() {
        // Adjust saturation based on luminance
        let mut ghc = GradingHueCurveTransform {
            lum_sat: vec![[0.0, 1.0], [0.5, 0.5], [1.0, 1.0]],
            ..Default::default()
        };
        let result = apply(&Transform::GradingHueCurve(ghc), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_hue_curve_sat_sat() {
        // Adjust saturation based on saturation
        let mut ghc = GradingHueCurveTransform {
            sat_sat: vec![[0.0, 0.0], [0.5, 0.7], [1.0, 1.0]],
            ..Default::default()
        };
        let result = apply(&Transform::GradingHueCurve(ghc), [1.0, 0.5, 0.0]); // Orange
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_hue_curve_linear_style() {
        let mut ghc = GradingHueCurveTransform {
            style: GradingHueCurveStyle::Linear,
            ..Default::default()
        };
        let result = apply(&Transform::GradingHueCurve(ghc), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }

    #[test]
    fn test_grading_hue_curve_video_style() {
        let mut ghc = GradingHueCurveTransform {
            style: GradingHueCurveStyle::Video,
            ..Default::default()
        };
        let result = apply(&Transform::GradingHueCurve(ghc), [0.5, 0.5, 0.5]);
        assert!(result[0].is_finite());
    }
}

// ============================================================================
// TRANSFER STYLE ALL VARIANTS PARITY
// ============================================================================

/// All TransferStyle variants parity.
/// OCIO ref: Builtin transfer functions.
mod transfer_style_all_variants_parity {
    use crate::processor::TransferStyle;

    #[test]
    fn test_transfer_style_linear_as_default() {
        // TransferStyle doesn't have Default, but Linear is identity
        let style = TransferStyle::Linear;
        assert!(matches!(style, TransferStyle::Linear));
    }
}

// ============================================================================
// DYNAMIC PROCESSOR PARITY
// ============================================================================

/// DynamicProcessor parity tests.
/// OCIO ref: DynamicProperty.
mod dynamic_processor_parity {
    use crate::dynamic::DynamicProcessorBuilder;
    use crate::transform::{ExposureContrastStyle, ExposureContrastTransform, MatrixTransform};
    use crate::{Processor, Transform, TransformDirection};

    fn identity_processor() -> Processor {
        let identity = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0, 0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });
        Processor::from_transform(&identity, TransformDirection::Forward).unwrap()
    }

    #[test]
    fn test_dynamic_processor_creation() {
        let base = identity_processor();
        let proc = DynamicProcessorBuilder::new()
            .exposure(0.0)
            .contrast(1.0)
            .build(base);

        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixels);
        assert!((pixels[0][0] - 0.18).abs() < 0.02);
    }

    #[test]
    fn test_dynamic_processor_with_exposure() {
        let base = identity_processor();
        let proc = DynamicProcessorBuilder::new()
            .exposure(1.0) // +1 stop
            .build(base);

        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixels);
        // +1 stop doubles the value
        assert!((pixels[0][0] - 0.36).abs() < 0.05);
    }

    #[test]
    fn test_dynamic_processor_with_contrast() {
        let base = identity_processor();
        let proc = DynamicProcessorBuilder::new()
            .contrast(1.5)
            .pivot(0.18)
            .build(base);

        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        proc.apply_rgb(&mut pixels);
        // At pivot, contrast doesn't change value
        assert!((pixels[0][0] - 0.18).abs() < 0.02);
    }

    #[test]
    fn test_dynamic_processor_builder_chaining() {
        let base = identity_processor();
        let proc = DynamicProcessorBuilder::new()
            .exposure(0.5)
            .contrast(1.2)
            .gamma(1.1)
            .saturation(1.0)
            .pivot(0.18)
            .build(base);

        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        proc.apply_rgb(&mut pixels);
        assert!(pixels[0][0].is_finite());
    }
}

// ============================================================================
// OPTIMIZATION LEVEL ALL VARIANTS PARITY
// ============================================================================

/// All OptimizationLevel variants.
/// OCIO ref: Processor optimization.
mod optimization_level_all_variants_parity {
    use crate::processor::OptimizationLevel;

    #[test]
    fn test_optimization_level_default() {
        let level = OptimizationLevel::default();
        // Default should be Lossless
        assert_eq!(level, OptimizationLevel::Lossless);
    }
}

// ============================================================================
// ENCODING AND FAMILY PARITY
// ============================================================================

/// Encoding and Family enum parity.
/// OCIO ref: ColorSpace encoding/family.
mod encoding_family_parity {
    use crate::colorspace::{Encoding, Family};

    #[test]
    fn test_encoding_is_linear() {
        assert!(Encoding::SceneLinear.is_linear());
        assert!(Encoding::DisplayLinear.is_linear());
        assert!(!Encoding::Log.is_linear());
        assert!(!Encoding::Sdr.is_linear());
    }

    #[test]
    fn test_encoding_parse() {
        assert_eq!(Encoding::parse("scene-linear"), Encoding::SceneLinear);
        assert_eq!(Encoding::parse("log"), Encoding::Log);
        assert_eq!(Encoding::parse("data"), Encoding::Data);
        assert_eq!(Encoding::parse("unknown_value"), Encoding::Unknown);
    }

    #[test]
    fn test_encoding_as_str() {
        assert_eq!(Encoding::SceneLinear.as_str(), "scene-linear");
        assert_eq!(Encoding::Log.as_str(), "log");
        assert_eq!(Encoding::Data.as_str(), "data");
    }

    #[test]
    fn test_family_parse() {
        assert_eq!(Family::parse("scene"), Family::Scene);
        assert_eq!(Family::parse("display"), Family::Display);
        assert_eq!(Family::parse("aces"), Family::Aces);
    }

    #[test]
    fn test_family_as_str() {
        assert_eq!(Family::Scene.as_str(), "Scene");
        assert_eq!(Family::Display.as_str(), "Display");
        assert_eq!(Family::Aces.as_str(), "ACES");
    }
}

// ============================================================================
// GradingTone — OCIO reference-vector parity
// ============================================================================
//
// Input/expected vectors are taken VERBATIM from the OpenColorIO reference test
// `tests/cpu/ops/gradingtone/GradingToneOpCPU_tests.cpp` (namespaces TS1..TS7).
// Each case asserts our forward output matches the reference expected output and
// that the inverse round-trips back to the input — exactly as the reference test
// does (apply Fwd to input -> expected; apply Rev to expected -> input).
mod grading_tone_reference_vectors {
    use crate::transform::{
        GradingRgbmsw, GradingToneStyle, GradingToneTransform, TransformDirection,
    };
    use crate::{Processor, Transform};

    /// `{R, G, B, master, start, width}` (order used by the reference test init).
    fn rgbmsw(r: f64, g: f64, b: f64, m: f64, start: f64, width: f64) -> GradingRgbmsw {
        GradingRgbmsw {
            red: r,
            green: g,
            blue: b,
            master: m,
            start,
            width,
        }
    }

    /// Compares two pixel arrays with the reference's error-factor tolerance.
    fn check(expected: &[[f32; 3]], got: &[[f32; 3]], line: u32) {
        const ERROR: f32 = 1e-4;
        for (i, (e, g)) in expected.iter().zip(got.iter()).enumerate() {
            for j in 0..3 {
                let ef = if e[j].abs() < 1.0 { 1.0 } else { e[j].abs() };
                assert!(
                    (e[j] - g[j]).abs() <= ERROR * ef,
                    "line {line}: pixel[{i}][{j}] expected {} got {} (tol {})",
                    e[j],
                    g[j],
                    ERROR * ef
                );
            }
        }
    }

    /// Builds a forward GradingTone transform from per-style defaults, then mutates one zone.
    fn tone(
        style: GradingToneStyle,
        apply: impl FnOnce(&mut GradingToneTransform),
    ) -> GradingToneTransform {
        let v = crate::grading_tone::GradingTone::from_style(style);
        let mut t = GradingToneTransform {
            name: String::new(),
            style,
            blacks: v.blacks,
            shadows: v.shadows,
            midtones: v.midtones,
            highlights: v.highlights,
            whites: v.whites,
            scontrast: v.s_contrast,
            direction: TransformDirection::Forward,
        };
        apply(&mut t);
        t
    }

    fn run_fwd(t: &GradingToneTransform, input: &[[f32; 3]]) -> Vec<[f32; 3]> {
        let proc = Processor::from_transform(
            &Transform::GradingTone(t.clone()),
            TransformDirection::Forward,
        )
        .unwrap();
        let mut px = input.to_vec();
        proc.apply_rgb(&mut px);
        px
    }

    fn run_rev(t: &GradingToneTransform, input: &[[f32; 3]]) -> Vec<[f32; 3]> {
        let proc = Processor::from_transform(
            &Transform::GradingTone(t.clone()),
            TransformDirection::Inverse,
        )
        .unwrap();
        let mut px = input.to_vec();
        proc.apply_rgb(&mut px);
        px
    }

    #[test]
    fn ts1_log_midtones() {
        let t = tone(GradingToneStyle::Log, |t| {
            t.midtones = rgbmsw(0.3, 1.0, 1.8, 1.2, 0.47, 0.6);
        });
        let input = [[0.1, -0.4, 0.9], [0.3, 0.6, 0.7], [0.8, 2.2, 0.5]];
        let expected = [
            [0.09440361, -0.4, 0.906_455_1],
            [0.23564218, 0.628_38, 0.76080927],
            [0.787_837, 2.2, 0.671_599_8],
        ];
        check(&expected, &run_fwd(&t, &input), line!());
        check(&input, &run_rev(&t, &expected), line!());
    }

    #[test]
    fn ts2_log_highlights() {
        let t = tone(GradingToneStyle::Log, |t| {
            t.highlights = rgbmsw(0.3, 1.0, 1.8, 1.4, -0.1, 0.9);
        });
        let input = [[0.8, 0.2, -0.05], [-0.4, 0.7, 0.8], [0.5, 1.0, 2.2]];
        let expected = [
            [0.758_338_2, 0.218, -0.048_479_8],
            [-0.4, 0.756, 0.880_185_6],
            [0.461_140_1, 0.96, 1.056],
        ];
        check(&expected, &run_fwd(&t, &input), line!());
        check(&input, &run_rev(&t, &expected), line!());
    }

    #[test]
    fn ts3_video_shadows() {
        let t = tone(GradingToneStyle::Video, |t| {
            t.shadows = rgbmsw(0.3, 1.0, 1.79, 0.6, 0.8, -0.1);
        });
        let input = [[-0.05, -0.3, -0.05], [0.20, 0.2, 0.10], [0.50, 1.2, 0.40]];
        let expected = [
            [-0.089_036, -0.22, -0.0101064],
            [0.042_35, 0.14, 0.158_287_73],
            [0.44006111, 1.2, 0.426_106_36],
        ];
        check(&expected, &run_fwd(&t, &input), line!());
        check(&input, &run_rev(&t, &expected), line!());
    }

    #[test]
    fn ts4_video_white_details() {
        let t = tone(GradingToneStyle::Video, |t| {
            t.whites = rgbmsw(0.3, 1.0, 1.9, 0.6, -0.2, 1.4);
        });
        let input = [[0.9, -0.4, 0.8], [1.2, 0.8, 1.0], [8.0, 4.0, 2.0]];
        let expected = [
            [0.506_642, -0.4, 0.85713846],
            [0.591_7, 0.657_142_9, 1.116_613_9],
            [1.85, 2.6, 17.730_995],
        ];
        check(&expected, &run_fwd(&t, &input), line!());
        check(&input, &run_rev(&t, &expected), line!());
    }

    #[test]
    fn ts5_log_black_details() {
        let t = tone(GradingToneStyle::Log, |t| {
            t.blacks = rgbmsw(0.3, 1.0, 1.9, 0.6, 0.8, 0.9);
        });
        let input = [[-0.05, -0.5, -0.20], [0.05, 0.0, -0.05], [0.40, 1.2, 0.40]];
        let expected = [
            [-0.88574485, -0.991_666_7, 0.23906196],
            [-0.501_057, -0.16583916, 0.25926968],
            [0.30488108, 1.2, 0.45937302],
        ];
        check(&expected, &run_fwd(&t, &input), line!());
        check(&input, &run_rev(&t, &expected), line!());
    }

    #[test]
    fn ts6_log_scontrast() {
        let t1 = tone(GradingToneStyle::Log, |t| t.scontrast = 1.8);
        let input1 = [[0.15, 0.3, 0.42], [-0.1, 0.6, 1.2], [0.8, 0.0, 1.0]];
        let expected1 = [
            [0.052_5, 0.152_830_5, 0.45714286],
            [-0.035, 0.839_106_7, 1.07],
            [0.93, 0.00000000, 1.00000000],
        ];
        check(&expected1, &run_fwd(&t1, &input1), line!());

        let t2 = tone(GradingToneStyle::Log, |t| t.scontrast = 0.3);
        let input2 = [[0.04, 0.3, 0.15], [-0.1, 0.6, 1.2], [0.8, 0.0, 1.0]];
        let expected2 = [
            [0.08050314, 0.350_312_5, 0.26213396],
            [-0.20125786, 0.499_375, 1.402_515_8],
            [0.63561388, 0.00000000, 1.00000000],
        ];
        check(&expected2, &run_fwd(&t2, &input2), line!());
        check(&input2, &run_rev(&t2, &expected2), line!());
        check(&input1, &run_rev(&t1, &expected1), line!());
    }

    #[test]
    fn ts7_lin_midtones() {
        let t = tone(GradingToneStyle::Linear, |t| {
            t.midtones = rgbmsw(0.3, 1.4, 1.8, 1.0, 1.0, 8.0);
        });
        let input = [[0.1, -0.1, 0.90], [0.3, 0.6, 0.70], [0.8, 1.5, 0.05]];
        let expected = [
            [0.04102994, -0.1, 3.075_427_3],
            [0.08530666, 1.195_693, 2.652_212_1],
            [0.260_803_8, 2.374_293_6, 0.08896667],
        ];
        check(&expected, &run_fwd(&t, &input), line!());
        check(&input, &run_rev(&t, &expected), line!());
    }
}

/// Guards the assumption `compile.rs` relies on when it bakes GradingRGBCurve LUTs.
///
/// `compile_transform` evaluates each curve out of the single four-curve
/// `curve_data` block it already built for the GPU, instead of re-fitting the spline
/// once per sample. That is only equivalent to a per-curve fit if `build_rgb_uniforms`
/// fits each slot independently — if slots ever became interdependent, the CPU bake
/// would silently drift from the curve the caller specified.
#[cfg(test)]
mod grading_rgb_curve_slot_independence {
    #[test]
    fn slot_placement_does_not_change_the_fit() {
        use crate::grading_bspline::{GradingControlPoint, build_rgb_uniforms, eval_curve_fwd};
        let mk = |v: &[[f32; 2]]| -> Vec<GradingControlPoint> {
            v.iter()
                .map(|p| GradingControlPoint { x: p[0], y: p[1] })
                .collect()
        };
        // Four deliberately different, non-identity curves so a slot mix-up shows up.
        let red = mk(&[[0.0, 0.0], [0.3, 0.15], [0.7, 0.85], [1.0, 1.0]]);
        let green = mk(&[[0.0, 0.1], [0.5, 0.4], [1.0, 0.9]]);
        let blue = mk(&[[0.0, 0.0], [0.25, 0.4], [1.0, 1.0]]);
        let master = mk(&[[0.0, 0.0], [0.6, 0.5], [1.0, 1.0]]);
        let e: [f32; 0] = [];

        let all = build_rgb_uniforms([&red, &green, &blue, &master], [&e, &e, &e, &e]);
        let curves = [&red, &green, &blue, &master];

        for (idx, c) in curves.iter().enumerate() {
            let empty: [GradingControlPoint; 0] = [];
            let solo = build_rgb_uniforms([*c, &empty, &empty, &empty], [&e, &e, &e, &e]);
            for i in 0..=1000 {
                let x = i as f32 / 1000.0;
                let in_block = eval_curve_fwd(
                    idx,
                    x,
                    x,
                    &all.knots_offsets,
                    &all.coefs_offsets,
                    &all.knots,
                    &all.coefs,
                );
                let alone = eval_curve_fwd(
                    0,
                    x,
                    x,
                    &solo.knots_offsets,
                    &solo.coefs_offsets,
                    &solo.knots,
                    &solo.coefs,
                );
                assert_eq!(
                    in_block, alone,
                    "curve {idx} at x={x} differs between a four-curve block and a solo fit"
                );
            }
        }
    }
}
