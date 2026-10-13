//! Tests for the OCIO Processor pipeline (op compilation, transfer functions,
//! optimization, LUT inversion, and CDL/grading processors).
//!
//! Extracted from `processor.rs` during the STRUCT.2 split (TODO6.md). The
//! tests module sits as a child of the implicit `processor` module, so
//! `use super::*` resolves to the same items it did inline.

use super::*;

#[test]
fn exposure_contrast_processor() {
    let ec = Transform::ExposureContrast(ExposureContrastTransform {
        name: String::new(),
        exposure: 1.0, // +1 stop
        contrast: 1.0,
        gamma: 1.0,
        pivot: 0.18,
        log_exposure_step: 0.088, // Default (Cineon)
        log_mid_gray: 0.435,      // Default
        style: ExposureContrastStyle::Linear,
        direction: TransformDirection::Forward,
        dynamic: crate::EcDynamic::NONE,
    });

    let processor = Processor::from_transform(&ec, TransformDirection::Forward).unwrap();
    assert_eq!(processor.num_ops(), 1);

    let mut pixels = [[0.18_f32, 0.18, 0.18]];
    processor.apply_rgb(&mut pixels);

    // +1 stop should double the value
    assert!((pixels[0][0] - 0.36).abs() < 0.01);
}

#[test]
fn grading_primary_log_processor() {
    // Test LOG style with brightness adjustment. LOG `brightness` is scaled
    // by `6.25/1023` (OCIO: `GradingPrimaryPreRender::update`,
    // `GradingPrimary.cpp:120-122` - calibrated in 10-bit code-value units,
    // not raw [0,1] deltas), so a raw brightness of `0.1*(1023/6.25)`
    // produces the same ~0.1 pixel delta this test's assertions expect
    // (this scale factor didn't exist before the GradingPrimary live-path
    // fix - `compile.rs` used to copy `brightness` straight through with
    // no scaling at all).
    let brightness_scale = 1023.0 / 6.25;
    let gp = Transform::GradingPrimary(GradingPrimaryTransform::from_flat(
        GradingPrimaryStyle::Log,
        [0.1 * brightness_scale, 0.0, -0.1 * brightness_scale],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
        1.0,
        0.5,
        0.0,
        1.0,
        f64::NEG_INFINITY,
        f64::INFINITY,
        TransformDirection::Forward,
    ));

    let processor = Processor::from_transform(&gp, TransformDirection::Forward).unwrap();

    let mut pixels = [[0.5_f32, 0.5, 0.5]];
    processor.apply_rgb(&mut pixels);

    // Red boosted by brightness, green unchanged, blue reduced
    assert!((pixels[0][0] - 0.6).abs() < 0.01);
    assert!((pixels[0][1] - 0.5).abs() < 0.01);
    assert!((pixels[0][2] - 0.4).abs() < 0.01);
}

#[test]
fn grading_primary_linear_processor() {
    // Test LINEAR style with exposure adjustment
    let gp = Transform::GradingPrimary(GradingPrimaryTransform::from_flat(
        GradingPrimaryStyle::Linear,
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.0, -1.0],
        [0.0, 0.0, 0.0],
        1.0,
        0.18,
        0.0,
        1.0,
        f64::NEG_INFINITY,
        f64::INFINITY,
        TransformDirection::Forward,
    ));

    let processor = Processor::from_transform(&gp, TransformDirection::Forward).unwrap();

    let mut pixels = [[0.18_f32, 0.18, 0.18]];
    processor.apply_rgb(&mut pixels);

    // Red doubled (+1 stop), green unchanged, blue halved (-1 stop)
    assert!((pixels[0][0] - 0.36).abs() < 0.01);
    assert!((pixels[0][1] - 0.18).abs() < 0.01);
    assert!((pixels[0][2] - 0.09).abs() < 0.01);
}

#[test]
fn grading_primary_video_processor() {
    // Test VIDEO style with a gain adjustment. `slope` is no longer directly
    // settable - VIDEO's slope is DERIVED from lift/gain/pivot_black/
    // pivot_white (`op::grading_primary_params`); at the default pivots
    // (pivot_black=0, pivot_white=1) and identity lift, `slope[i]` reduces
    // to exactly `gain[i]`, so this reproduces the old test's `slope:
    // [1.5, 1.0, 0.5]` bit-for-bit via `gain` instead.
    let gp = Transform::GradingPrimary(GradingPrimaryTransform {
        name: String::new(),
        style: GradingPrimaryStyle::Video,
        value: crate::dynamic::GradingPrimary {
            gain: crate::dynamic::GradingRgbm::new(1.5, 1.0, 0.5, 1.0), // Boost red, reduce blue
            ..crate::dynamic::GradingPrimary::video_default()
        },
        direction: TransformDirection::Forward,
    });

    let processor = Processor::from_transform(&gp, TransformDirection::Forward).unwrap();

    let mut pixels = [[0.5_f32, 0.5, 0.5]];
    processor.apply_rgb(&mut pixels);

    // Red boosted by slope, green unchanged, blue reduced
    assert!((pixels[0][0] - 0.75).abs() < 0.01);
    assert!((pixels[0][1] - 0.5).abs() < 0.01);
    assert!((pixels[0][2] - 0.25).abs() < 0.01);
}

#[test]
fn rgb_curve_processor() {
    use crate::transform::GradingToneStyle;
    let gc = Transform::GradingRgbCurve(GradingRgbCurveTransform {
        name: String::new(),
        style: GradingToneStyle::Log, // Log style = no lin-to-log conversion
        // S-curve on red: lift shadows, pull highlights
        red: vec![[0.0, 0.1], [0.5, 0.5], [1.0, 0.9]],
        green: vec![[0.0, 0.0], [1.0, 1.0]], // Identity
        blue: vec![[0.0, 0.0], [1.0, 1.0]],
        master: vec![[0.0, 0.0], [1.0, 1.0]],
        red_slopes: vec![],
        green_slopes: vec![],
        blue_slopes: vec![],
        master_slopes: vec![],
        bypass_lin_to_log: false,
        direction: TransformDirection::Forward,
    });

    let processor = Processor::from_transform(&gc, TransformDirection::Forward).unwrap();

    let mut pixels = [[0.0_f32, 0.5, 1.0]];
    processor.apply_rgb(&mut pixels);

    // Red: 0.0 -> 0.1 (lifted), green: 0.5 unchanged, blue: 1.0 unchanged
    assert!((pixels[0][0] - 0.1).abs() < 0.01);
    assert!((pixels[0][1] - 0.5).abs() < 0.01);
}

#[test]
fn rgb_curve_bspline_processor() {
    // Test B-spline interpolation with 3+ control points.
    // OCIO ref: GradingRGBCurveOpCPU uses cubic Hermite splines.
    use crate::transform::GradingToneStyle;

    // Curve with 3 points - should use B-spline (smooth), not linear
    let gc = Transform::GradingRgbCurve(GradingRgbCurveTransform {
        name: String::new(),
        style: GradingToneStyle::Log,
        red: vec![[0.0, 0.0], [0.5, 0.7], [1.0, 1.0]], // Gamma-like curve
        green: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]], // Identity
        blue: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
        master: vec![[0.0, 0.0], [1.0, 1.0]],
        red_slopes: vec![], // Auto-compute slopes
        green_slopes: vec![],
        blue_slopes: vec![],
        master_slopes: vec![],
        bypass_lin_to_log: false,
        direction: TransformDirection::Forward,
    });

    let processor = Processor::from_transform(&gc, TransformDirection::Forward).unwrap();

    // Test at x=0.25 (between first two control points)
    let mut pixels = [[0.25_f32, 0.25, 0.25]];
    processor.apply_rgb(&mut pixels);

    // With B-spline, the curve should be smooth:
    // Linear would give: 0.25 * 0.7 / 0.5 = 0.35
    // B-spline gives a smoother transition, typically closer to 0.35-0.45
    assert!(
        pixels[0][0] > 0.25,
        "Red should increase due to curve shape"
    );
    assert!(
        pixels[0][0] < 0.6,
        "Red should not exceed curve midpoint too much"
    );
    // Green should be identity
    assert!(
        (pixels[0][1] - 0.25).abs() < 0.02,
        "Green should be near identity"
    );
}

#[test]
fn rgb_curve_custom_slopes() {
    // Test custom slopes override auto-computed ones.
    use crate::transform::GradingToneStyle;

    // Curve with custom slopes that create a specific shape
    let gc = Transform::GradingRgbCurve(GradingRgbCurveTransform {
        name: String::new(),
        style: GradingToneStyle::Log,
        red: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
        green: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
        blue: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
        master: vec![[0.0, 0.0], [1.0, 1.0]],
        red_slopes: vec![2.0, 0.5, 0.5], // Steep start, gentle middle/end
        green_slopes: vec![],            // Auto-compute
        blue_slopes: vec![],
        master_slopes: vec![],
        bypass_lin_to_log: false,
        direction: TransformDirection::Forward,
    });

    let processor = Processor::from_transform(&gc, TransformDirection::Forward).unwrap();

    // With steep slope at start, values near 0 should increase faster
    let mut pixels = [[0.1_f32, 0.1, 0.1]];
    processor.apply_rgb(&mut pixels);

    // Red with slope=2 at start should be higher than linear
    // Green with auto-slope should be closer to identity
    assert!(
        pixels[0][0] > pixels[0][1],
        "Custom steep slope should push red higher"
    );
}

#[test]
fn grading_tone_log_processor() {
    // GradingTone LOG style with a NON-identity whites adjustment. The old test
    // set whites.master=1.0 (identity) and merely asserted the output was in
    // (0,2) — it passed on a no-op processor. Here we drive a real per-channel
    // whites gain (red boosted, blue cut, non-unit master) so the result MUST
    // diverge from the input, and the divergence must be directional (red rises
    // above blue). A no-op / identity implementation fails both assertions.
    let mut gt = GradingToneTransform::default();
    gt.whites = GradingRgbmsw {
        red: 1.2,
        green: 1.0,
        blue: 0.8,
        master: 1.2,
        start: gt.whites.start,
        width: gt.whites.width,
    };

    let processor =
        Processor::from_transform(&Transform::GradingTone(gt), TransformDirection::Forward)
            .unwrap();

    // Pick an input clearly INSIDE the whites zone. The whites zone start is
    // resolved through the highlight curve (GradingTone.cpp `update`); for the
    // Log defaults it lands near 0.5, so an input AT 0.5 sits at the zone start
    // where the curve is identity (faithful OCIO behaviour — `if t < x0 { t }`).
    // 0.8 is above the resolved start, so the non-identity gain must move it.
    let input = 0.8_f32;
    let mut pixels = [[input, input, input]];
    processor.apply_rgb(&mut pixels);

    // Finite + sane display range (catches NaN / runaway output).
    for c in pixels[0] {
        assert!(
            c.is_finite() && c > 0.0 && c < 2.0,
            "channel out of range: {c}"
        );
    }
    // The non-identity whites gain (master=1.2) must actually move an in-zone
    // value off the input — catches a no-op / identity implementation. We do
    // NOT assert a per-channel direction (that would be an unproven guess);
    // authoritative per-zone vectors live in parity_tests.rs TS1..TS7.
    let moved = (pixels[0][0] - input)
        .abs()
        .max((pixels[0][2] - input).abs());
    assert!(
        moved > 0.01,
        "whites master gain 1.2 must change in-zone input {input}; got {:?} (no-op?)",
        pixels[0]
    );
}

#[test]
fn grading_tone_scontrast_processor() {
    // Test GradingTone with S-contrast adjustment
    // Increased S-contrast
    let gt = GradingToneTransform {
        scontrast: 1.5,
        ..Default::default()
    };

    let processor =
        Processor::from_transform(&Transform::GradingTone(gt), TransformDirection::Forward)
            .unwrap();

    // Midtone value (0.4 = pivot)
    let mut pixels = [[0.4_f32, 0.4, 0.4]];
    processor.apply_rgb(&mut pixels);

    // At pivot (0.4), S-contrast should leave values relatively unchanged
    assert!(
        (pixels[0][0] - 0.4).abs() < 0.1,
        "Pivot should be relatively stable"
    );
}

#[test]
fn allocation_processor() {
    let alloc = Transform::Allocation(AllocationTransform {
        allocation: AllocationType::Log2,
        vars: vec![-8.0, 4.0], // Log2 range: -8 to +4 stops
        direction: TransformDirection::Forward,
    });

    let processor = Processor::from_transform(&alloc, TransformDirection::Forward).unwrap();

    // 0.18 is roughly 18% gray, log2(0.18) ~ -2.47
    let mut pixels = [[0.18_f32, 0.18, 0.18]];
    processor.apply_rgb(&mut pixels);

    // Should be normalized to [0, 1] range based on -8 to +4 log2 range
    // log2(0.18) = -2.47, normalized = (-2.47 - (-8)) / (4 - (-8)) = 5.53 / 12 = 0.46
    assert!(pixels[0][0] > 0.4 && pixels[0][0] < 0.5);
}

#[test]
fn fixed_function_rgb_to_hsv() {
    let ff = Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style: FixedFunctionStyle::RgbToHsv,
        params: vec![],
        direction: TransformDirection::Forward,
    });

    let processor = Processor::from_transform(&ff, TransformDirection::Forward).unwrap();

    // Pure red: H=0, S=1, V=1
    let mut pixels = [[1.0_f32, 0.0, 0.0]];
    processor.apply_rgb(&mut pixels);

    assert!((pixels[0][0] - 0.0).abs() < 0.01); // Hue = 0 (red)
    assert!((pixels[0][1] - 1.0).abs() < 0.01); // Saturation = 1
    assert!((pixels[0][2] - 1.0).abs() < 0.01); // Value = 1
}

#[test]
fn cdl_processor() {
    let cdl = Transform::Cdl(CdlTransform {
        name: String::new(),
        slope: [1.1, 1.0, 0.9],
        offset: [0.01, 0.0, -0.01],
        power: [1.0, 1.0, 1.0],
        saturation: 1.0,
        style: CdlStyle::AscCdl,
        direction: TransformDirection::Forward,
    });

    let processor = Processor::from_transform(&cdl, TransformDirection::Forward).unwrap();
    // A CDL without power is simplified to its slope/offset matrix and the ASC clamp
    // (OCIO SIMPLIFY_OPS; PyOCIO 2.5.1 LOSSLESS: Matrix, Range).
    let names: Vec<&str> = processor.ops().iter().map(|o| o.variant_name()).collect();
    assert_eq!(names, ["Matrix", "Range"]);

    let mut pixels = [[0.18_f32, 0.18, 0.18]];
    processor.apply_rgb(&mut pixels);

    assert!((pixels[0][0] - 0.208).abs() < 0.001); // 0.18 * 1.1 + 0.01
    assert!((pixels[0][1] - 0.18).abs() < 0.001);
    assert!((pixels[0][2] - 0.152).abs() < 0.001); // 0.18 * 0.9 - 0.01
}

#[test]
fn exponent_processor() {
    let exp = Transform::Exponent(ExponentTransform {
        name: String::new(),
        value: [2.2, 2.2, 2.2, 1.0],
        negative_style: NegativeStyle::Clamp,
        direction: TransformDirection::Forward,
    });

    let processor = Processor::from_transform(&exp, TransformDirection::Forward).unwrap();

    let mut pixels = [[0.5_f32, 0.5, 0.5]];
    processor.apply_rgb(&mut pixels);

    let expected = 0.5_f32.powf(2.2);
    assert!((pixels[0][0] - expected).abs() < 0.0001);
}

#[test]
fn group_processor() {
    let group = Transform::group(vec![
        Transform::Cdl(CdlTransform::default()),
        Transform::Exponent(ExponentTransform {
            name: String::new(),
            value: [1.0, 1.0, 1.0, 1.0],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        }),
    ]);

    // Use OptimizationLevel::None to test compilation without removal of identities
    let processor = Processor::from_transform_with_opts(
        &group,
        TransformDirection::Forward,
        OptimizationLevel::None,
    )
    .unwrap();
    assert_eq!(processor.num_ops(), 2);
}

/// The exact 3D LUT inverse (OCIO `InvLut3DRenderer` port) round-trips a 0.5x gain LUT.
#[test]
fn lut3d_inversion() {
    let size = 17;
    let mut data = Vec::with_capacity(size * size * size);
    // Blue-fastest, as `vfx_lut::Lut3D::data`.
    for r in 0..size {
        for g in 0..size {
            for b in 0..size {
                let f = |i: usize| i as f32 / (size - 1) as f32 * 0.5;
                data.push([f(r), f(g), f(b)]);
            }
        }
    }
    let inv = super::inv_lut::InvLut3d::new(&data, size, [0.0; 3], [1.0; 3]);
    let back = inv.eval([0.2, 0.3, 0.4]);
    for (g, w) in back.iter().zip([0.4, 0.6, 0.8]) {
        assert!((g - w).abs() < 1e-5, "{back:?}");
    }
}

#[test]
fn optimization_removes_identity() {
    // A CDL with unit SOP parameters is only a true identity in the NoClamp style.
    // The v1.2 (AscCdl) style clamps to [0,1] regardless of its parameters, so the
    // optimizer must KEEP it — asserted separately below. This test previously used
    // `CdlTransform::default()`, whose style is AscCdl, and so required exactly the
    // clamp-dropping behaviour that made out-of-range values survive the CDL.
    let identity_cdl = Transform::Cdl(CdlTransform {
        style: crate::transform::CdlStyle::NoClamp,
        ..Default::default()
    });

    // Without optimization
    let proc_none = Processor::from_transform_with_opts(
        &identity_cdl,
        TransformDirection::Forward,
        OptimizationLevel::None,
    )
    .unwrap();
    assert_eq!(
        proc_none.num_ops(),
        1,
        "Without optimization, identity CDL should remain"
    );

    // With Lossless optimization
    let proc_opt = Processor::from_transform_with_opts(
        &identity_cdl,
        TransformDirection::Forward,
        OptimizationLevel::Lossless,
    )
    .unwrap();
    assert_eq!(
        proc_opt.num_ops(),
        0,
        "With optimization, identity NoClamp CDL should be removed"
    );

    // The clamping style with the very same unit parameters must survive, and must
    // still clamp. Without this the test cannot tell "drops identities" from
    // "drops CDLs".
    let clamping_cdl = Transform::Cdl(CdlTransform {
        style: crate::transform::CdlStyle::AscCdl,
        ..CdlTransform::default()
    });
    let proc_clamp = Processor::from_transform_with_opts(
        &clamping_cdl,
        TransformDirection::Forward,
        OptimizationLevel::Lossless,
    )
    .unwrap();
    assert_eq!(
        proc_clamp.num_ops(),
        1,
        "Unit-parameter ASC CDL still clamps to [0,1] and must not be optimized away"
    );
    let mut pixel = [[-0.5_f32, 0.5, 1.7]];
    proc_clamp.apply_rgb(&mut pixel);
    assert_eq!(pixel[0], [0.0, 0.5, 1.0], "ASC CDL clamp was lost");
}

#[test]
fn optimization_combines_matrices() {
    // Create two matrix transforms
    let m1 = Transform::Matrix(MatrixTransform {
        name: String::new(),
        matrix: [
            2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
        offset: [0.0, 0.0, 0.0, 0.0],
        direction: TransformDirection::Forward,
    });
    let m2 = Transform::Matrix(MatrixTransform {
        name: String::new(),
        matrix: [
            0.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
        offset: [0.0, 0.0, 0.0, 0.0],
        direction: TransformDirection::Forward,
    });

    let group = Transform::group(vec![m1, m2]);

    // Without optimization
    let proc_none = Processor::from_transform_with_opts(
        &group,
        TransformDirection::Forward,
        OptimizationLevel::None,
    )
    .unwrap();
    assert_eq!(
        proc_none.num_ops(),
        2,
        "Without optimization should have 2 matrices"
    );

    // With Lossless optimization the matrices combine into diag(1), an identity, which the next
    // pass removes (OCIO's optimize loop does the same: nothing is left).
    let proc_opt = Processor::from_transform_with_opts(
        &group,
        TransformDirection::Forward,
        OptimizationLevel::Lossless,
    )
    .unwrap();
    assert_eq!(
        proc_opt.num_ops(),
        0,
        "With optimization, the matrices combine to an identity and are removed"
    );

    // Verify combined matrix produces correct output (2.0 * 0.5 = 1.0 = identity)
    let mut pixels = [[0.5_f32, 0.5, 0.5]];
    proc_opt.apply_rgb(&mut pixels);
    assert!(
        (pixels[0][0] - 0.5).abs() < 0.0001,
        "Combined matrix should be identity"
    );
}

// =========================================================================
// OCIO Transfer Function Parity Tests
// Reference values verified against OCIO 2.3 implementation.
// =========================================================================

#[test]
fn transfer_srgb_roundtrip() {
    // sRGB encode -> decode should be identity
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "sRGB".to_string(),
        direction: TransformDirection::Forward,
    });
    let decode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "sRGB".to_string(),
        direction: TransformDirection::Inverse,
    });

    let proc_enc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();
    let proc_dec = Processor::from_transform(&decode, TransformDirection::Forward).unwrap();

    let test_values: Vec<f32> = vec![0.0, 0.01, 0.1, 0.18, 0.5, 1.0];
    for &v in &test_values {
        let mut pixel = [[v, v, v]];
        proc_enc.apply_rgb(&mut pixel);
        proc_dec.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - v).abs() < 1e-5,
            "sRGB roundtrip failed at {}",
            v
        );
    }
}

#[test]
fn transfer_srgb_reference_values() {
    // Reference values from OCIO/IEC 61966-2-1
    // Linear -> sRGB
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "sRGB".to_string(),
        direction: TransformDirection::Forward,
    });
    let proc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();

    // Linear 0.0 -> sRGB 0.0
    let mut p = [[0.0_f32, 0.0, 0.0]];
    proc.apply_rgb(&mut p);
    assert!((p[0][0] - 0.0).abs() < 1e-5, "sRGB(0.0) = 0.0");

    // Linear 0.0031308 is the breakpoint
    // Below: linear region, Above: gamma region
    let mut p = [[0.0031308_f32, 0.0, 0.0]];
    proc.apply_rgb(&mut p);
    assert!(
        (p[0][0] - 0.04045).abs() < 0.001,
        "sRGB breakpoint ~0.04045"
    );

    // Linear 0.18 (mid-gray) -> sRGB ~0.4614
    let mut p = [[0.18_f32, 0.18, 0.18]];
    proc.apply_rgb(&mut p);
    assert!((p[0][0] - 0.4614).abs() < 0.01, "sRGB(0.18) ~= 0.4614");

    // Linear 1.0 -> sRGB 1.0
    let mut p = [[1.0_f32, 1.0, 1.0]];
    proc.apply_rgb(&mut p);
    assert!((p[0][0] - 1.0).abs() < 1e-5, "sRGB(1.0) = 1.0");
}

#[test]
fn transfer_acescct_roundtrip() {
    // ACEScct encode -> decode
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "ACEScct".to_string(),
        direction: TransformDirection::Forward,
    });
    let decode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "ACEScct".to_string(),
        direction: TransformDirection::Inverse,
    });

    let proc_enc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();
    let proc_dec = Processor::from_transform(&decode, TransformDirection::Forward).unwrap();

    // Test positive values (ACEScct has extended range for negatives)
    let test_values: Vec<f32> = vec![0.0001, 0.01, 0.18, 0.5, 1.0, 2.0];
    for &v in &test_values {
        let mut pixel = [[v, v, v]];
        proc_enc.apply_rgb(&mut pixel);
        proc_dec.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - v).abs() < 1e-4,
            "ACEScct roundtrip failed at {}",
            v
        );
    }
}

#[test]
fn transfer_acescct_reference_values() {
    // Reference from ACES TB-2014-004.4
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "ACEScct".to_string(),
        direction: TransformDirection::Forward,
    });
    let proc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();

    // 18% gray (0.18) -> ACEScct ~0.4135
    let mut p = [[0.18_f32, 0.18, 0.18]];
    proc.apply_rgb(&mut p);
    assert!((p[0][0] - 0.4135).abs() < 0.01, "ACEScct(0.18) ~= 0.4135");

    // 1.0 -> ACEScct ~0.5549
    let mut p = [[1.0_f32, 1.0, 1.0]];
    proc.apply_rgb(&mut p);
    assert!((p[0][0] - 0.5549).abs() < 0.01, "ACEScct(1.0) ~= 0.5549");
}

#[test]
fn transfer_logc4_roundtrip() {
    // ARRI LogC4 encode -> decode
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "LogC4".to_string(),
        direction: TransformDirection::Forward,
    });
    let decode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "LogC4".to_string(),
        direction: TransformDirection::Inverse,
    });

    let proc_enc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();
    let proc_dec = Processor::from_transform(&decode, TransformDirection::Forward).unwrap();

    let test_values: Vec<f32> = vec![0.01, 0.18, 0.5, 1.0, 5.0];
    for &v in &test_values {
        let mut pixel = [[v, v, v]];
        proc_enc.apply_rgb(&mut pixel);
        proc_dec.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - v).abs() < 1e-4,
            "LogC4 roundtrip failed at {}",
            v
        );
    }
}

#[test]
fn transfer_logc4_reference_values() {
    // Reference from ARRI LogC4 White Paper
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "LogC4".to_string(),
        direction: TransformDirection::Forward,
    });
    let proc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();

    // 18% gray -> LogC4 ~0.278 (normalized code value)
    let mut p = [[0.18_f32, 0.18, 0.18]];
    proc.apply_rgb(&mut p);
    // LogC4 uses different normalization than LogC3
    assert!(
        p[0][0] > 0.2 && p[0][0] < 0.4,
        "LogC4(0.18) should be ~0.278"
    );
}

#[test]
fn transfer_pq_roundtrip() {
    // PQ (ST 2084) encode -> decode
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "PQ".to_string(),
        direction: TransformDirection::Forward,
    });
    let decode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "PQ".to_string(),
        direction: TransformDirection::Inverse,
    });

    let proc_enc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();
    let proc_dec = Processor::from_transform(&decode, TransformDirection::Forward).unwrap();

    // PQ operates on [0, 1] where 1 = 10000 nits
    let test_values: Vec<f32> = vec![0.0, 0.01, 0.1, 0.5, 1.0];
    for &v in &test_values {
        let mut pixel = [[v, v, v]];
        proc_enc.apply_rgb(&mut pixel);
        proc_dec.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - v).abs() < 1e-4,
            "PQ roundtrip failed at {}",
            v
        );
    }
}

#[test]
fn transfer_pq_reference_values() {
    // SMPTE ST 2084 (PQ) reference values, derived from the published spec
    // OETF formula. This BuiltinTransfer "PQ" path takes ABSOLUTE luminance in
    // cd/m^2 normalised by L_MAX = 10000 (vfx-transfer/src/pq.rs), so the
    // expected codes are computed straight from the ST-2084 constants
    // (m1=2610/16384, m2=2523/4096*128, c1=3424/4096, c2=2413/4096*32,
    // c3=2392/4096*32) with Y = nits/10000 — NOT from running our code. The
    // previous test only checked monotonicity, which any wrong-but-increasing
    // curve also satisfies.
    //
    // Spec OETF(nits):  0 -> 0,  100 -> 0.50808,  1000 -> 0.75183,  10000 -> 1.0.
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "PQ".to_string(),
        direction: TransformDirection::Forward,
    });
    let proc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();

    // (absolute luminance in nits, expected PQ code from ST-2084).
    let cases = [
        (0.0_f32, 0.0_f32),
        (100.0, 0.50808),
        (1000.0, 0.75183),
        (10000.0, 1.0),
    ];
    for (nits, expected_pq) in cases {
        let mut p = [[nits, nits, nits]];
        proc.apply_rgb(&mut p);
        assert!(
            (p[0][0] - expected_pq).abs() < 1e-3,
            "PQ({nits} nits) expected {expected_pq} (ST-2084 spec), got {}",
            p[0][0]
        );
    }
}

#[test]
fn transfer_hlg_roundtrip() {
    // HLG encode -> decode
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "HLG".to_string(),
        direction: TransformDirection::Forward,
    });
    let decode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "HLG".to_string(),
        direction: TransformDirection::Inverse,
    });

    let proc_enc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();
    let proc_dec = Processor::from_transform(&decode, TransformDirection::Forward).unwrap();

    let test_values: Vec<f32> = vec![0.0, 0.01, 0.1, 0.5, 1.0];
    for &v in &test_values {
        let mut pixel = [[v, v, v]];
        proc_enc.apply_rgb(&mut pixel);
        proc_dec.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - v).abs() < 1e-4,
            "HLG roundtrip failed at {}",
            v
        );
    }
}

#[test]
fn transfer_slog3_roundtrip() {
    // Sony S-Log3 encode -> decode
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "SLog3".to_string(),
        direction: TransformDirection::Forward,
    });
    let decode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "SLog3".to_string(),
        direction: TransformDirection::Inverse,
    });

    let proc_enc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();
    let proc_dec = Processor::from_transform(&decode, TransformDirection::Forward).unwrap();

    let test_values: Vec<f32> = vec![0.01, 0.18, 0.5, 1.0, 5.0];
    for &v in &test_values {
        let mut pixel = [[v, v, v]];
        proc_enc.apply_rgb(&mut pixel);
        proc_dec.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - v).abs() < 1e-4,
            "SLog3 roundtrip failed at {}",
            v
        );
    }
}

#[test]
fn transfer_vlog_roundtrip() {
    // Panasonic V-Log encode -> decode
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "VLog".to_string(),
        direction: TransformDirection::Forward,
    });
    let decode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "VLog".to_string(),
        direction: TransformDirection::Inverse,
    });

    let proc_enc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();
    let proc_dec = Processor::from_transform(&decode, TransformDirection::Forward).unwrap();

    let test_values: Vec<f32> = vec![0.01, 0.18, 0.5, 1.0, 5.0];
    for &v in &test_values {
        let mut pixel = [[v, v, v]];
        proc_enc.apply_rgb(&mut pixel);
        proc_dec.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - v).abs() < 1e-4,
            "VLog roundtrip failed at {}",
            v
        );
    }
}

#[test]
fn transfer_rec1886_roundtrip() {
    // Rec.1886 (gamma 2.4) encode -> decode
    let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "Rec1886".to_string(),
        direction: TransformDirection::Forward,
    });
    let decode = Transform::BuiltinTransfer(BuiltinTransferTransform {
        style: "Rec1886".to_string(),
        direction: TransformDirection::Inverse,
    });

    let proc_enc = Processor::from_transform(&encode, TransformDirection::Forward).unwrap();
    let proc_dec = Processor::from_transform(&decode, TransformDirection::Forward).unwrap();

    let test_values: Vec<f32> = vec![0.0, 0.1, 0.18, 0.5, 1.0];
    for &v in &test_values {
        let mut pixel = [[v, v, v]];
        proc_enc.apply_rgb(&mut pixel);
        proc_dec.apply_rgb(&mut pixel);
        assert!(
            (pixel[0][0] - v).abs() < 1e-5,
            "Rec1886 roundtrip failed at {}",
            v
        );
    }
}

#[test]
fn transfer_all_styles_consistency() {
    // Verify all transfer styles produce consistent results
    let styles = [
        "sRGB", "Rec709", "ACEScct", "ACEScc", "LogC3", "LogC4", "SLog3", "VLog", "Log3G10", "PQ",
        "HLG", "Rec1886",
    ];

    for style in &styles {
        let encode = Transform::BuiltinTransfer(BuiltinTransferTransform {
            style: style.to_string(),
            direction: TransformDirection::Forward,
        });

        let proc = Processor::from_transform(&encode, TransformDirection::Forward);
        assert!(proc.is_ok(), "Failed to create processor for {}", style);

        // Apply to mid-gray. Every style listed here is a non-identity log /
        // gamma / PQ / HLG encode, so each MUST move 0.18 by a clear margin.
        // The previous test only checked the output was in (0, 1.5), which an
        // identity / no-op transfer (output == 0.18) would also satisfy.
        let mut pixel = [[0.18_f32, 0.18, 0.18]];
        proc.unwrap().apply_rgb(&mut pixel);

        for c in pixel[0] {
            assert!(
                c.is_finite() && c > 0.0 && c < 1.5,
                "{style} mapped 0.18 out of range: {c}"
            );
        }
        // Smallest real movement in the set is PQ (0.18 -> ~0.348, delta ~0.17);
        // a >0.05 gate is comfortably below that yet fails any no-op encode.
        assert!(
            (pixel[0][0] - 0.18).abs() > 0.05,
            "{style} must encode 0.18 away from linear (no-op?), got {}",
            pixel[0][0]
        );
    }
}

/// Golden-value numeric tests ported directly from OCIO's own
/// `GradingPrimaryOpCPU_tests.cpp` (`GradingPrimaryOpCPU, log`, `TS1`
/// namespace) — not just internal self-consistency, actual reference
/// output values. Part of the GradingPrimary live-path fix
/// (`op::grading_primary_params`, `docs/ROADMAP.md`).
mod grading_primary_red_gate {
    use super::*;
    use crate::dynamic::{GradingPrimary, GradingRgbm};

    fn ts1_value() -> GradingPrimary {
        GradingPrimary {
            brightness: GradingRgbm::new(-10.0, 45.0, -5.0, 50.0),
            contrast: GradingRgbm::new(0.9, 1.4, 0.7, 0.75),
            gamma: GradingRgbm::new(1.1, 0.7, 1.05, 1.15),
            pivot: -0.3,
            saturation: 1.21,
            ..GradingPrimary::log_default()
        }
    }

    #[test]
    fn log_forward_matches_ocio_ts1() {
        let t = Transform::GradingPrimary(GradingPrimaryTransform {
            name: String::new(),
            style: GradingPrimaryStyle::Log,
            value: ts1_value(),
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let inputs = [[0.1_f32, 0.9, 1.2], [-0.4, 0.2, 1.2]];
        let expected = [
            [0.23327083_f32, 1.773_843_8, 0.860_277],
            [-0.10117631, 0.790_168_4, 1.020_519_3],
        ];

        for (input, want) in inputs.iter().zip(expected.iter()) {
            let mut pixels = [*input];
            proc.apply_rgb(&mut pixels);
            for c in 0..3 {
                assert!(
                    (pixels[0][c] - want[c]).abs() < 1e-4,
                    "TS1 forward {input:?} channel {c}: got {}, want {}",
                    pixels[0][c],
                    want[c]
                );
            }
        }
    }

    #[test]
    fn log_inverse_matches_ocio_ts1() {
        // OCIO's own inverse test feeds the FORWARD test's expected output
        // back through the inverse op and checks it recovers the original
        // input (`GradingPrimaryOpCPU_tests.cpp` `log`, "Test in inverse
        // direction").
        let t = Transform::GradingPrimary(GradingPrimaryTransform {
            name: String::new(),
            style: GradingPrimaryStyle::Log,
            value: ts1_value(),
            direction: TransformDirection::Inverse,
        });
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let forward_outputs = [
            [0.23327083_f32, 1.773_843_8, 0.860_277],
            [-0.10117631, 0.790_168_4, 1.020_519_3],
        ];
        let expected_inputs = [[0.1_f32, 0.9, 1.2], [-0.4, 0.2, 1.2]];

        for (input, want) in forward_outputs.iter().zip(expected_inputs.iter()) {
            let mut pixels = [*input];
            proc.apply_rgb(&mut pixels);
            for c in 0..3 {
                assert!(
                    (pixels[0][c] - want[c]).abs() < 1e-4,
                    "TS1 inverse {input:?} channel {c}: got {}, want {}",
                    pixels[0][c],
                    want[c]
                );
            }
        }
    }

    /// `ProcessorOp::inverse()` (used by `Processor::inverse()`, e.g. to
    /// build an inverse processor from an already-compiled forward one) had
    /// a regression introduced by this same pass: it used to just flip
    /// `forward` and keep the numeric fields unchanged, which was correct
    /// ONLY while `apply.rs` derived both directions from raw values at
    /// apply time. Once `op::grading_primary_params` started precomputing
    /// direction-resolved values, that shortcut silently broke. Fixed by
    /// inverting each field (negate additive, reciprocate multiplicative)
    /// — proven here via a real round-trip through `Processor::inverse()`,
    /// not just `Transform`-level compilation (which was already correct
    /// and wouldn't have caught this).
    #[test]
    fn op_level_inverse_round_trips() {
        let t = Transform::GradingPrimary(GradingPrimaryTransform {
            name: String::new(),
            style: GradingPrimaryStyle::Log,
            value: ts1_value(),
            direction: TransformDirection::Forward,
        });
        let forward = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let inverse = forward
            .inverse()
            .expect("GradingPrimary must be invertible");

        for original in [[0.1_f32, 0.9, 1.2], [-0.4, 0.2, 1.2], [0.5, 0.5, 0.5]] {
            let mut pixels = [original];
            forward.apply_rgb(&mut pixels);
            inverse.apply_rgb(&mut pixels);
            for c in 0..3 {
                assert!(
                    (pixels[0][c] - original[c]).abs() < 1e-3,
                    "op-level inverse round-trip diverged at {original:?} channel {c}: got {:?}",
                    pixels[0]
                );
            }
        }
    }

    /// `op_level_inverse_round_trips` (above) only proves fwd-then-inv
    /// recovers the original pixel — it would pass even if the op-level
    /// inverse computed a DIFFERENT-but-still-invertible transform than the
    /// compile-time one (e.g. off by a monotonic reparameterization). This
    /// test compares the two derivations directly, numerically, the same
    /// way `op::range_inverse_tests::compile_time_inverse_matches_op_level_inverse`
    /// does for Range: build `Processor A` from `Transform::GradingPrimary`
    /// with `direction: Inverse` (the compile-time path,
    /// `grading_primary_params(style, false, &value)`), build `Processor B`
    /// by compiling FORWARD then calling `.inverse()` (the op-level path,
    /// now ALSO `grading_primary_params(style, false, &value)` via the `raw`
    /// field after this pass's single-derivation-path fix) — they must
    /// produce IDENTICAL pixels, not just each independently round-trip.
    /// Covers all three styles, since the two derivation paths could agree
    /// for one style's formula shape and diverge for another's.
    #[test]
    fn compile_time_inverse_matches_op_level_inverse_for_all_styles() {
        let cases: [(GradingPrimaryStyle, GradingPrimary); 3] = [
            (GradingPrimaryStyle::Log, ts1_value()),
            (GradingPrimaryStyle::Linear, ts2_value()),
            (GradingPrimaryStyle::Video, ts3_value()),
        ];

        for (style, value) in cases {
            let t_fwd = Transform::GradingPrimary(GradingPrimaryTransform {
                name: String::new(),
                style,
                value: value.clone(),
                direction: TransformDirection::Forward,
            });
            let t_inv = Transform::GradingPrimary(GradingPrimaryTransform {
                name: String::new(),
                style,
                value: value.clone(),
                direction: TransformDirection::Inverse,
            });

            let via_compile_inverse =
                Processor::from_transform(&t_inv, TransformDirection::Forward)
                    .unwrap_or_else(|e| panic!("{style:?}: compile-time inverse failed: {e}"));
            let forward = Processor::from_transform(&t_fwd, TransformDirection::Forward)
                .unwrap_or_else(|e| panic!("{style:?}: forward compile failed: {e}"));
            let via_op_inverse = forward
                .inverse()
                .unwrap_or_else(|e| panic!("{style:?}: op-level inverse failed: {e}"));

            for input in [[0.1_f32, 0.9, 1.2], [-0.4, 0.2, 1.2], [0.5, 0.5, 0.5]] {
                let mut a = [input];
                let mut b = [input];
                via_compile_inverse.apply_rgb(&mut a);
                via_op_inverse.apply_rgb(&mut b);
                for c in 0..3 {
                    assert!(
                        (a[0][c] - b[0][c]).abs() < 1e-5,
                        "{style:?} at {input:?} channel {c}: compile-time inverse gave {}, \
                         op-level inverse gave {}",
                        a[0][c],
                        b[0][c]
                    );
                }
            }
        }
    }

    /// LIN-style golden values, ported directly from
    /// `GradingPrimaryOpCPU_tests.cpp`'s `TS2` namespace (`GradingPrimaryOpCPU,
    /// lin`). Closes the "only LOG has dedicated OCIO-golden coverage" gap
    /// noted in the GradingPrimary Part 2 checkpoint.
    fn ts2_value() -> GradingPrimary {
        GradingPrimary {
            exposure: GradingRgbm::new(0.5, -0.2, 0.4, -0.25),
            offset: GradingRgbm::new(-0.03, 0.02, 0.1, -0.1),
            contrast: GradingRgbm::new(0.9, 1.4, 0.7, 0.75),
            pivot: 0.5,
            saturation: 1.33,
            ..GradingPrimary::linear_default()
        }
    }

    #[test]
    fn lin_forward_matches_ocio_ts2() {
        let t = Transform::GradingPrimary(GradingPrimaryTransform {
            name: String::new(),
            style: GradingPrimaryStyle::Linear,
            value: ts2_value(),
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let inputs = [[0.1_f32, 0.9, 1.2], [-0.1, 0.9, 3.2]];
        let expected = [
            [-0.24746465_f32, 0.675_755, 0.64940625],
            [-0.508_714_9, 0.680_024_1, 1.197_218_5],
        ];

        for (input, want) in inputs.iter().zip(expected.iter()) {
            let mut pixels = [*input];
            proc.apply_rgb(&mut pixels);
            for c in 0..3 {
                assert!(
                    (pixels[0][c] - want[c]).abs() < 1e-4,
                    "TS2 forward {input:?} channel {c}: got {}, want {}",
                    pixels[0][c],
                    want[c]
                );
            }
        }
    }

    #[test]
    fn lin_forward_clamped_matches_ocio_ts2() {
        let t = Transform::GradingPrimary(GradingPrimaryTransform {
            name: String::new(),
            style: GradingPrimaryStyle::Linear,
            value: GradingPrimary {
                clamp_black: -0.40,
                clamp_white: 1.05,
                ..ts2_value()
            },
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let inputs = [[0.1_f32, 0.9, 1.2], [-0.1, 0.9, 3.2]];
        let expected = [
            [-0.24746465_f32, 0.675_755, 0.64940625],
            [-0.4, 0.680_024_1, 1.05],
        ];

        for (input, want) in inputs.iter().zip(expected.iter()) {
            let mut pixels = [*input];
            proc.apply_rgb(&mut pixels);
            for c in 0..3 {
                assert!(
                    (pixels[0][c] - want[c]).abs() < 1e-4,
                    "TS2 clamped forward {input:?} channel {c}: got {}, want {}",
                    pixels[0][c],
                    want[c]
                );
            }
        }
    }

    #[test]
    fn lin_inverse_matches_ocio_ts2() {
        // OCIO's own inverse test feeds the unclamped forward test's expected
        // output back through the inverse op (with clamp effectively disabled
        // via -100/100 bounds) and checks it recovers the original input.
        let t = Transform::GradingPrimary(GradingPrimaryTransform {
            name: String::new(),
            style: GradingPrimaryStyle::Linear,
            value: GradingPrimary {
                clamp_black: -100.0,
                clamp_white: 100.0,
                ..ts2_value()
            },
            direction: TransformDirection::Inverse,
        });
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let forward_outputs = [
            [-0.24746465_f32, 0.675_755, 0.64940625],
            [-0.508_714_9, 0.680_024_1, 1.197_218_5],
        ];
        let expected_inputs = [[0.1_f32, 0.9, 1.2], [-0.1, 0.9, 3.2]];

        for (input, want) in forward_outputs.iter().zip(expected_inputs.iter()) {
            let mut pixels = [*input];
            proc.apply_rgb(&mut pixels);
            for c in 0..3 {
                assert!(
                    (pixels[0][c] - want[c]).abs() < 1e-4,
                    "TS2 inverse {input:?} channel {c}: got {}, want {}",
                    pixels[0][c],
                    want[c]
                );
            }
        }
    }

    /// VIDEO-style golden values, ported directly from
    /// `GradingPrimaryOpCPU_tests.cpp`'s `TS3` namespace (`GradingPrimaryOpCPU,
    /// video`).
    fn ts3_value() -> GradingPrimary {
        GradingPrimary {
            lift: GradingRgbm::new(0.05, -0.04, 0.02, 0.05),
            gamma: GradingRgbm::new(0.9, 1.4, 0.7, 0.75),
            gain: GradingRgbm::new(1.2, 1.1, 1.25, 0.8),
            offset: GradingRgbm::new(-0.03, 0.02, 0.1, -0.1),
            saturation: 1.2,
            ..GradingPrimary::video_default()
        }
    }

    #[test]
    fn video_forward_matches_ocio_ts3() {
        let t = Transform::GradingPrimary(GradingPrimaryTransform {
            name: String::new(),
            style: GradingPrimaryStyle::Video,
            value: ts3_value(),
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let inputs = [[0.1_f32, 0.9, 1.2], [-0.1, 0.9, 1.2]];
        let expected = [
            [-0.106_677_6_f32, 0.75643484, 1.537_295],
            [-0.17148458, 0.758_815_5, 1.539_675_7],
        ];

        for (input, want) in inputs.iter().zip(expected.iter()) {
            let mut pixels = [*input];
            proc.apply_rgb(&mut pixels);
            for c in 0..3 {
                assert!(
                    (pixels[0][c] - want[c]).abs() < 1e-4,
                    "TS3 forward {input:?} channel {c}: got {}, want {}",
                    pixels[0][c],
                    want[c]
                );
            }
        }
    }

    #[test]
    fn video_forward_clamped_matches_ocio_ts3() {
        let t = Transform::GradingPrimary(GradingPrimaryTransform {
            name: String::new(),
            style: GradingPrimaryStyle::Video,
            value: GradingPrimary {
                clamp_black: -0.15,
                clamp_white: 1.50,
                ..ts3_value()
            },
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let inputs = [[0.1_f32, 0.9, 1.2], [-0.1, 0.9, 1.2]];
        let expected = [
            [-0.106_677_6_f32, 0.75643484, 1.5],
            [-0.15, 0.758_815_5, 1.5],
        ];

        for (input, want) in inputs.iter().zip(expected.iter()) {
            let mut pixels = [*input];
            proc.apply_rgb(&mut pixels);
            for c in 0..3 {
                assert!(
                    (pixels[0][c] - want[c]).abs() < 1e-4,
                    "TS3 clamped forward {input:?} channel {c}: got {}, want {}",
                    pixels[0][c],
                    want[c]
                );
            }
        }
    }

    #[test]
    fn video_pivot_bw_matches_ocio_ts3() {
        // OCIO's third `video` sub-case: clamp effectively disabled, but
        // `pivotBlack`/`pivotWhite` set to non-default values.
        let t = Transform::GradingPrimary(GradingPrimaryTransform {
            name: String::new(),
            style: GradingPrimaryStyle::Video,
            value: GradingPrimary {
                clamp_black: -100.0,
                clamp_white: 100.0,
                pivot_black: 0.05,
                pivot_white: 0.9,
                ..ts3_value()
            },
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&t, TransformDirection::Forward).unwrap();

        let inputs = [[0.1_f32, 0.9, 1.2], [-0.1, 0.9, 1.2]];
        let expected = [
            [-0.06553329_f32, 0.749_846_4, 1.677_412_9],
            [-0.14759934, 0.752_861_1, 1.680_427_6],
        ];

        for (input, want) in inputs.iter().zip(expected.iter()) {
            let mut pixels = [*input];
            proc.apply_rgb(&mut pixels);
            for c in 0..3 {
                assert!(
                    (pixels[0][c] - want[c]).abs() < 1e-4,
                    "TS3 pivot-bw forward {input:?} channel {c}: got {}, want {}",
                    pixels[0][c],
                    want[c]
                );
            }
        }
    }
}

/// An image that does not fit its buffer, or has an unknown depth, is an error before any pixel
/// is touched (these were undefined behaviour through the raw-pointer API); a bottom-up image
/// (negative row stride from the last row) is fine.
#[test]
fn packed_images_are_bounds_checked_and_may_be_bottom_up() {
    let proc = Processor::from_ops(vec![ProcessorOp::Matrix {
        matrix: [
            2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
        offset: [0.0; 4],
    }]);
    let mut buf = [10u8, 0, 0, 20, 0, 0];
    let before = buf;
    let rgb = ChannelOrdering::Rgb;
    let u8d = PackedBitDepth::Uint8;
    assert!(
        proc.apply_packed(&mut buf, 0, 1, 3, rgb, u8d, 1, 3, 3)
            .is_err()
    );
    assert!(
        proc.apply_packed(&mut buf, 1, 1, 2, rgb, u8d, 1, 3, 3)
            .is_err()
    );
    assert!(
        proc.apply_packed(&mut buf, 0, 1, 2, rgb, PackedBitDepth::Unknown, 1, 3, 3)
            .is_err()
    );
    assert_eq!(buf, before, "a refused image is not touched");
    // Two rows, the first stored last.
    proc.apply_packed(&mut buf, 3, 1, 2, rgb, u8d, 1, 3, -3)
        .unwrap();
    assert_eq!(buf, [20, 0, 0, 40, 0, 0]);
}

/// `apply_packed` on an unaligned, BGRA-ordered uint8 buffer: channel indices, unaligned
/// access, OCIO's round-to-nearest on write (`BitDepthUtils.h` `Converter::CastValue`), and
/// alpha left as stored.
#[test]
fn apply_packed_uint8_bgra_unaligned_rounds_and_keeps_alpha() {
    // out.R = 0.5 * in.G, out.G = in.B, out.B = in.R (row-major 4x4; asymmetric on purpose).
    #[rustfmt::skip]
    let matrix = [
        0.0, 0.5, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        1.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ];
    let proc = Processor::from_ops(vec![ProcessorOp::Matrix {
        matrix,
        offset: [0.0; 4],
    }]);

    // Two pixels, stored B, G, R, A, starting one byte past an aligned address.
    let px_in: [[u8; 4]; 2] = [[10, 127, 255, 77], [200, 3, 40, 255]];
    let mut buf = vec![0u8; 1 + 8];
    for (i, p) in px_in.iter().enumerate() {
        buf[1 + i * 4..1 + i * 4 + 4].copy_from_slice(p);
    }
    // The image starts at byte 1 (unaligned for every depth).
    proc.apply_packed(
        &mut buf,
        1,
        2,
        1,
        ChannelOrdering::Bgra,
        PackedBitDepth::Uint8,
        1,
        4,
        8,
    )
    .unwrap();
    // Pixel 0: in R=255 G=127 B=10 -> out R=63.5 -> 64 (rounded, truncation gives 63),
    // G=10, B=255; stored B,G,R,A with A untouched.
    assert_eq!(&buf[1..5], &[255, 10, 64, 77]);
    // Pixel 1: in R=40 G=3 B=200 -> out R=1.5 -> 2, G=200, B=40.
    assert_eq!(&buf[5..9], &[40, 200, 2, 255]);
    assert_eq!(buf[0], 0, "byte before the image must not be written");
}

/// 3D LUT interpolation against PyOpenColorIO 2.5.1 on a non-separable 3x3x3 LUT
/// (`value(i,j,k) = ((i^2 + 0.3jk)/4, (j + 0.5ik^2)/3, (0.25k^2 + 0.2ij)/1.5)`) at off-diagonal
/// points where trilinear and tetrahedral differ. Also pins OCIO v2's rule that `Nearest`
/// evaluates as trilinear, and edge clamping. Before the shared kernel, the processor evaluated
/// `Tetrahedral`/`Best` as trilinear.
#[test]
fn lut3d_interpolation_matches_pyocio() {
    const N: usize = 3;
    let f = |i: f32, j: f32, k: f32| {
        [
            (i * i + 0.3 * j * k) / 4.0,
            (j + 0.5 * i * k * k) / 3.0,
            (k * k * 0.25 + 0.2 * i * j) / 1.5,
        ]
    };
    // Lut3DTransform's own layout: red fastest.
    let mut data = Vec::with_capacity(N * N * N);
    for b in 0..N {
        for g in 0..N {
            for r in 0..N {
                data.push(f(r as f32, g as f32, b as f32));
            }
        }
    }
    let probes = [
        [0.3, 0.7, 0.1],
        [0.9, 0.2, 0.55],
        [0.1, 0.4, 0.8],
        [0.6, 0.6, 0.2],
        [1.2, -0.1, 0.5],
    ];
    #[rustfmt::skip]
    let tetra = [
        [0.18, 0.5, 0.166_666_66], [0.887_499_9, 0.533_333_36, 0.323_333_35],
        [0.155, 0.400_000_04, 0.493_333_4], [0.445_000_05, 0.50000006, 0.280_000_03],
        [1.0, 0.333_333_34, 0.166_666_67],
    ];
    #[rustfmt::skip]
    let linear = [
        [0.171, 0.486_666_68, 0.14533335], [0.883_000_1, 0.523_333_4, 0.312_666_7],
        [0.146_000_01, 0.36, 0.488_000_04], [0.436_000_05, 0.480_000_08, 0.258_666_7],
        [1.0, 0.333_333_34, 0.166_666_67],
    ];
    for (interp, want) in [
        (Interpolation::Tetrahedral, &tetra),
        (Interpolation::Best, &tetra),
        (Interpolation::Linear, &linear),
        (Interpolation::Nearest, &linear),
    ] {
        let t = Lut3DTransform::from_data(data.clone(), N)
            .unwrap()
            .with_interpolation(interp);
        let direct: Vec<[f32; 3]> = probes.iter().map(|p| t.apply(*p)).collect();
        let proc =
            Processor::from_transform(&Transform::Lut3D(t), TransformDirection::Forward).unwrap();
        let mut px = probes;
        proc.apply_rgb(&mut px);
        for (k, w) in want.iter().enumerate() {
            for c in 0..3 {
                assert!(
                    (px[k][c] - w[c]).abs() < 1e-6,
                    "{interp:?} processor probe {k} ch {c}: {} vs OCIO {}",
                    px[k][c],
                    w[c]
                );
                assert!(
                    (direct[k][c] - w[c]).abs() < 1e-6,
                    "{interp:?} Lut3DTransform::apply probe {k} ch {c}: {} vs OCIO {}",
                    direct[k][c],
                    w[c]
                );
            }
        }
    }
}

/// OCIO's default `NEGATIVE_LINEAR` moncurve sends every value below the break — every
/// negative — down the linear segment; only `NEGATIVE_MIRROR` reflects the power curve.
/// Expected values: PyOpenColorIO 2.5.1, `ExponentWithLinearTransform` gamma 2.4 / offset 0.055
/// applied to `[-0.5, -0.001, 0.5]` (CPU processor, both directions, both styles).
#[test]
fn exponent_with_linear_negative_styles_match_ocio() {
    let cases: [(NegativeStyle, TransformDirection, [f32; 3]); 4] = [
        (
            NegativeStyle::Linear,
            TransformDirection::Forward,
            [-0.038_690_08, -7.738_016e-5, 0.214_036_87],
        ),
        (
            NegativeStyle::Linear,
            TransformDirection::Inverse,
            [-6.461_605, -0.012_923_211, 0.735_358_06],
        ),
        (
            NegativeStyle::Mirror,
            TransformDirection::Forward,
            [-0.214_036_87, -7.738_016e-5, 0.214_036_87],
        ),
        (
            NegativeStyle::Mirror,
            TransformDirection::Inverse,
            [-0.735_358_06, -0.012_923_211, 0.735_358_06],
        ),
    ];
    for (style, dir, want) in cases {
        let t = Transform::ExponentWithLinear(ExponentWithLinearTransform {
            name: String::new(),
            gamma: [2.4, 2.4, 2.4, 1.0],
            offset: [0.055, 0.055, 0.055, 0.0],
            negative_style: style,
            direction: dir,
        });
        let p = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[-0.5_f32, -0.001, 0.5]];
        p.apply_rgb(&mut px);
        // 1e-4 relative: OCIO's CPU moncurve runs the SSE fast-pow approximation
        // (`GammaOpCPU.cpp` `*SSE` renderers), ~2e-5 off the exact power at 0.5.
        for c in 0..3 {
            assert!(
                (px[0][c] - want[c]).abs() <= 1e-4 * want[c].abs().max(1.0),
                "{style:?} {dir:?} ch{c}: got {} want {}",
                px[0][c],
                want[c]
            );
        }
    }
}

/// GradingRGBCurve evaluates its fitted spline unbounded, in both directions, like OCIO's
/// `GradingRGBCurveOpCPU` — it used to bake a [0, 1] LUT and clamp, which is a different
/// function for every LOG-style input outside [0, 1].
///
/// The master curve is the ACES 1.x RRT shaper (`ACES.cpp` `Generate_tonecurve_ops`) over
/// log10 values, the red curve a small S. Expected values: PyOpenColorIO 2.5.1,
/// `GradingRGBCurveTransform` (GRADING_LOG forward/inverse, GRADING_LIN forward).
#[test]
fn grading_rgb_curve_matches_ocio_outside_unit_range() {
    use crate::transform::GradingToneStyle;
    let curve = |style, direction| {
        Transform::GradingRgbCurve(GradingRgbCurveTransform {
            name: String::new(),
            style,
            red: vec![[0.0, 0.1], [0.5, 0.5], [1.0, 0.9]],
            green: vec![[0.0, 0.0], [1.0, 1.0]],
            blue: vec![[0.0, 0.0], [1.0, 1.0]],
            master: vec![
                [-5.26017743, -4.0],
                [-3.75502745, -3.57868829],
                [-2.24987747, -1.82131329],
                [-0.74472749, 0.68124124],
                [1.06145248, 2.87457742],
                [2.86763245, 3.83406206],
                [4.67381243, 4.0],
            ],
            red_slopes: vec![],
            green_slopes: vec![],
            blue_slopes: vec![],
            master_slopes: vec![
                0.0, 0.55982688, 1.77532247, 1.55, 0.8787017, 0.18374463, 0.0,
            ],
            bypass_lin_to_log: false,
            direction,
        })
    };
    /// (style, direction, two input pixels, the two expected output pixels)
    type ToneCase = (
        GradingToneStyle,
        TransformDirection,
        [[f32; 3]; 2],
        [[f32; 3]; 2],
    );
    let cases: [ToneCase; 3] = [
        (
            GradingToneStyle::Log,
            TransformDirection::Forward,
            [[-6.0, -1.0, 0.25], [1.5, 3.0, 6.0]],
            [
                [-3.941_642_8, 0.280_691_27, 2.039_189_6],
                [3.073_256_5, 3.857_384_4, 4.0],
            ],
        ),
        (
            GradingToneStyle::Log,
            TransformDirection::Inverse,
            [[-6.0, -1.0, 0.25], [1.5, 3.0, 6.0]],
            [
                [-6.700_221, -1.777_856, -1.019_306_8],
                [-0.347_483_84, 1.208_945_3, 4.673_812_4],
            ],
        ),
        (
            GradingToneStyle::Linear,
            TransformDirection::Forward,
            [[0.18, 0.5, 2.0], [0.18, 0.5, 2.0]],
            [
                [0.652_096_2, 1.658_519_7, 2.735_056_6],
                [0.652_096_2, 1.658_519_7, 2.735_056_6],
            ],
        ),
    ];
    for (style, dir, input, want) in cases {
        let p = Processor::from_transform(&curve(style, dir), TransformDirection::Forward).unwrap();
        let mut px = input;
        p.apply_rgb(&mut px);
        for k in 0..2 {
            for c in 0..3 {
                assert!(
                    (px[k][c] - want[k][c]).abs() <= 1e-4 * want[k][c].abs().max(1.0),
                    "{style:?} {dir:?} in {:?} ch{c}: got {} want {}",
                    input[k],
                    px[k][c],
                    want[k][c]
                );
            }
        }
    }

    // OCIO rejects a curve with fewer than two points (GradingBSplineCurve.cpp:659-663); the
    // fitter would otherwise skip it as identity.
    let mut one_point = curve(GradingToneStyle::Log, TransformDirection::Forward);
    if let Transform::GradingRgbCurve(g) = &mut one_point {
        g.red = vec![[0.0, 0.7]];
    }
    assert!(Processor::from_transform(&one_point, TransformDirection::Forward).is_err());
}

/// A half-domain Lut1D is indexed by the input's half code and interpolated between the two
/// bracketing codes (`Lut1DRendererHalfCode`); the flag used to be dropped at compile, so the
/// 65536 entries were read as a uniform table over [0, 1].
///
/// Table: `f(x) = 3*sign(x)*sqrt|x| + x/4` at every half code (NaN codes -> f(0), infinities ->
/// f(+/-HALF_MAX), as OCIO's `CreateHalfLut` fills them). Expected values: PyOpenColorIO 2.5.1,
/// `Lut1DTransform(65536, inputHalfDomain=True)` with the same table.
#[test]
fn half_domain_lut1d_matches_ocio() {
    let f = |x: f64| -> f64 {
        let x = if x.is_nan() {
            0.0
        } else if x.is_infinite() {
            65504.0_f64.copysign(x)
        } else {
            x
        };
        x.abs().sqrt().copysign(x) * 3.0 + 0.25 * x
    };
    let table: Vec<f32> = (0..=u16::MAX)
        .map(|bits| f(half::f16::from_bits(bits).to_f64()) as f32)
        .collect();
    let mut lut = Lut1DTransform::from_data(table);
    lut.half_domain = true;
    let p = Processor::from_transform(&Transform::Lut1D(lut.clone()), TransformDirection::Forward)
        .unwrap();
    let mut px = [
        [0.3_f32, -0.7, 1234.5],
        [-1e-6, 0.0, 70000.0],
        [65519.0, -2.5e-7, 0.18],
    ];
    let want = [
        [1.718_167_7, -2.684_98, 414.031_37],
        [-0.003_000_015_4, 0.0, 17143.812],
        [17143.812, -0.001_498_501_6, 1.317_792_2],
    ];
    let direct: Vec<[f32; 3]> = px.iter().map(|p| lut.apply_rgb(*p)).collect();
    p.apply_rgb(&mut px);
    for k in 0..3 {
        for c in 0..3 {
            for (label, got) in [
                ("processor", px[k][c]),
                ("Lut1DTransform::apply_rgb", direct[k][c]),
            ] {
                assert!(
                    (got - want[k][c]).abs() <= 1e-5 * want[k][c].abs().max(1.0),
                    "{label} probe {k} ch{c}: got {got} want {}",
                    want[k][c]
                );
            }
        }
    }

    // The inverse: OCIO `InvLut1DRendererHalfCode` (each half of the code space searched on its
    // own, the fraction spread over the two half values). PyOpenColorIO 2.5.1 LOSSLESS (the
    // exact renderer; red before: refused).
    lut.direction = TransformDirection::Inverse;
    let p = Processor::from_transform(&Transform::Lut1D(lut), TransformDirection::Forward).unwrap();
    let mut px = [
        [1.7181677_f32, -2.68498, 414.03137],
        [-0.003, 0.0, 17143.812],
        [20000.0, -0.0015, 1.3177922],
        [-20000.0, 1e-9, -5.0],
    ];
    p.apply_rgb(&mut px);
    let want = [
        [0.3_f32, -0.699_999_9, 1234.5],
        [-9.999_899e-7, 0.0, 65504.0],
        [65504.0, -2.505_164_8e-7, 0.179_999_99],
        [-65504.0, 8.137_855e-14, -2.200_223],
    ];
    for k in 0..4 {
        for c in 0..3 {
            let (got, w) = (px[k][c], want[k][c]);
            assert!(
                (got - w).abs() <= 1e-5 * w.abs().max(1e-6),
                "inverse probe {k} ch{c}: {got} vs {w}"
            );
        }
    }
}

/// The half-domain inverse of a DECREASING three-channel table with a flat run past `-3 - c`
/// (OCIO's sign flip, the per-half effective domains, the bisect point at `f(+0)`).
/// `f_c(x) = max(-sign(x) |x|^(0.5 + 0.1c) + 0.1c, -3 - c)` at every half code; values:
/// PyOpenColorIO 2.5.1 `Lut1DTransform(65536, inputHalfDomain=True)` inverse, LOSSLESS.
#[test]
fn half_domain_lut1d_inverse_decreasing_matches_ocio() {
    let f = |x: f64, c: usize| -> f32 {
        let x = if x.is_nan() {
            0.0
        } else if x.is_infinite() {
            65504.0_f64.copysign(x)
        } else {
            x
        };
        let y = -(x.abs().powf(0.5 + 0.1 * c as f64).copysign(x)) + 0.1 * c as f64;
        y.max(-3.0 - c as f64) as f32
    };
    let codes = || (0..=u16::MAX).map(|b| half::f16::from_bits(b).to_f64());
    let mut lut = Lut1DTransform::from_data(codes().map(|x| f(x, 0)).collect());
    lut.green = Some(codes().map(|x| f(x, 1)).collect());
    lut.blue = Some(codes().map(|x| f(x, 2)).collect());
    lut.half_domain = true;
    lut.direction = TransformDirection::Inverse;
    let p = Processor::from_transform(&Transform::Lut1D(lut), TransformDirection::Forward).unwrap();
    let mut px = [
        [-0.5_f32, -0.5, -0.5],
        [0.5, 0.7, 0.9],
        [-2.5, -3.5, -4.5],
        [-10.0, 10.0, 0.0],
        [0.05, 0.15, 0.25],
    ];
    p.apply_rgb(&mut px);
    let want = [
        [0.25_f32, 0.426_827_2, 0.600_774_5],
        [-0.25, -0.426_827_2, -0.600_774_5],
        [6.25, 8.456_115, 9.123_045],
        [9.0, -45.644_87, 0.100_339_38],
        [-0.002_500_000_2, -0.006_786_046_5, -0.013_847_958],
    ];
    for k in 0..5 {
        for c in 0..3 {
            let (got, w) = (px[k][c], want[k][c]);
            assert!(
                (got - w).abs() <= 1e-5 * w.abs().max(1e-3),
                "probe {k} ch{c}: {got} vs {w}"
            );
        }
    }
}

/// A paired FixedFunction style run in reverse is its partner run forward
/// (`FixedFunctionOpData::invert`). The CPU apply and the GPU emitters implement these styles
/// one way and ignore the direction flag, so the inverse used to repeat the FORWARD function.
/// Expected values on `[0.5, 0.4, 0.3]`, PyOpenColorIO 2.5.1. XYZ/HSV: `FixedFunctionTransform`
/// both directions. PQ: the `CURVE - LINEAR_to_ST-2084` / `CURVE - ST-2084_to_LINEAR` rows of
/// `tests/data/ocio_builtins_pyocio.json` — the same function, which OCIO evaluates through an
/// exactly-generated half LUT, where its FixedFunction CPU renderer uses an SSE fast-pow
/// approximation that is up to 5e-4 off at these inputs.
#[test]
fn paired_fixed_function_inverse_is_the_partner_style() {
    let cases: [(FixedFunctionStyle, TransformDirection, [f32; 3]); 6] = [
        (
            FixedFunctionStyle::LinToPq,
            TransformDirection::Forward,
            [0.440_281_57, 0.419_284_1, 0.392_876_18],
        ),
        (
            FixedFunctionStyle::LinToPq,
            TransformDirection::Inverse,
            [0.922_457_1, 0.324_479_46, 0.100_382_34],
        ),
        (
            FixedFunctionStyle::XyzToXyy,
            TransformDirection::Forward,
            [0.416_666_66, 0.333_333_34, 0.4],
        ),
        (
            FixedFunctionStyle::XyzToXyy,
            TransformDirection::Inverse,
            [0.375, 0.3, 0.075],
        ),
        (
            FixedFunctionStyle::RgbToHsv,
            TransformDirection::Forward,
            [0.083_333_336, 0.4, 0.5],
        ),
        (
            FixedFunctionStyle::RgbToHsv,
            TransformDirection::Inverse,
            [0.18, 0.3, 0.3],
        ),
    ];
    for (style, direction, want) in cases {
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style,
            params: vec![],
            direction,
        });
        let p = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [[0.5_f32, 0.4, 0.3]];
        p.apply_rgb(&mut px);
        // And the op-level inverse of the forward op must agree with compiling the inverse.
        let fwd = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style,
            params: vec![],
            direction: TransformDirection::Forward,
        });
        let fwd_p = Processor::from_transform(&fwd, TransformDirection::Forward).unwrap();
        let op_inv = fwd_p.inverse().unwrap();
        // The builtin-table tolerance: 1e-4 relative, scaled by max(1, |v|).
        for c in 0..3 {
            assert!(
                (px[0][c] - want[c]).abs() <= 1e-4 * want[c].abs().max(1.0),
                "{style:?} {direction:?} ch{c}: got {} want {}",
                px[0][c],
                want[c]
            );
        }
        if direction == TransformDirection::Inverse {
            let mut q = [[0.5_f32, 0.4, 0.3]];
            op_inv.apply_rgb(&mut q);
            for c in 0..3 {
                assert!(
                    (q[0][c] - want[c]).abs() <= 1e-4 * want[c].abs().max(1.0),
                    "{style:?} Processor::inverse ch{c}: got {} want {}",
                    q[0][c],
                    want[c]
                );
            }
        }
    }
}

/// B24: an ExponentWithLinear (moncurve) takes only LINEAR or MIRROR negatives. OCIO throws for
/// the other two (`GammaOpData::ConvertStyleMonCurve`); PyOpenColorIO 2.5.1:
/// ```text
/// t = PyOpenColorIO.ExponentWithLinearTransform(); t.getNegativeStyle()  -> NEGATIVE_LINEAR
/// t.setNegativeStyle(NEGATIVE_CLAMP)     -> Exception: Clamp negative extrapolation is not valid for MonCurve exponent style.
/// t.setNegativeStyle(NEGATIVE_PASS_THRU) -> Exception: Pass thru negative extrapolation is not valid for MonCurve exponent style.
/// ```
#[test]
fn exponent_with_linear_rejects_clamp_and_pass_thru() {
    let make = |style| {
        Transform::ExponentWithLinear(ExponentWithLinearTransform {
            name: String::new(),
            gamma: [2.4, 2.4, 2.4, 1.0],
            offset: [0.055, 0.055, 0.055, 0.0],
            negative_style: style,
            direction: TransformDirection::Forward,
        })
    };
    for (style, msg) in [
        (
            NegativeStyle::Clamp,
            "Clamp negative extrapolation is not valid for MonCurve exponent style.",
        ),
        (
            NegativeStyle::PassThru,
            "Pass thru negative extrapolation is not valid for MonCurve exponent style.",
        ),
    ] {
        let err = Processor::from_transform(&make(style), TransformDirection::Forward)
            .err()
            .unwrap_or_else(|| panic!("{style:?} was accepted"));
        assert!(err.to_string().contains(msg), "{style:?}: {err}");
        let mut t = ExponentWithLinearTransform::default();
        assert!(t.set_negative_style(style).is_err());
        assert_eq!(t.negative_style, NegativeStyle::Linear);
    }
    for style in [NegativeStyle::Linear, NegativeStyle::Mirror] {
        assert!(Processor::from_transform(&make(style), TransformDirection::Forward).is_ok());
        let mut t = ExponentWithLinearTransform::default();
        t.set_negative_style(style).unwrap();
        assert_eq!(t.negative_style, style);
    }
    // OCIO's default, and the convenience constructors, are LINEAR.
    for t in [
        ExponentWithLinearTransform::default(),
        ExponentWithLinearTransform::new([2.4; 4], [0.055; 4]),
        ExponentWithLinearTransform::srgb(),
        ExponentWithLinearTransform::rec709(),
    ] {
        assert_eq!(t.negative_style, NegativeStyle::Linear);
    }
}

/// B30: a CLF `<Range>` is read and applied as OCIO does (`CTFReaderRangeElt`,
/// `RangeOpData::validate`/`normalize`/`fillScaleOffset`): scalar bounds scaled by the in/out
/// bit depths, `style="noClamp"` = no clamp, one-sided = pure clamp, per-file inverse. Values:
/// PyOpenColorIO 2.5.1 `FileTransform(<file>, direction)` on the same bytes.
#[test]
fn clf_range_matches_pyocio() {
    const PX: [[f32; 3]; 2] = [[-0.5, 0.15, 2.0], [0.05, 0.3, 0.95]];
    let range = |attrs: &str, body: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ProcessList id=\"r\" compCLFversion=\"3\">\n  <Range {attrs}>{body}</Range>\n</ProcessList>\n"
        )
    };
    let full = "<minInValue>0.1</minInValue><maxInValue>0.9</maxInValue>\
                <minOutValue>-0.2</minOutValue><maxOutValue>1.3</maxOutValue>";
    let f32_depths = "inBitDepth=\"32f\" outBitDepth=\"32f\"";
    /// (name, CLF body, forward output pixels, inverse output pixels)
    type ClfCase = (&'static str, String, [[f32; 3]; 2], [[f32; 3]; 2]);
    #[rustfmt::skip]
    let cases: [ClfCase; 5] = [
        ("clamp_32f", range(f32_depths, full),
            [[-0.2, -0.10625, 1.3], [-0.2, 0.175, 1.3]],
            [[0.1, 0.2866667, 0.9], [0.2333333, 0.3666667, 0.7133333]]),
        ("clamp_10i", range("inBitDepth=\"10i\" outBitDepth=\"12i\"",
            "<minInValue>64</minInValue><maxInValue>940</maxInValue>\
             <minOutValue>256</minOutValue><maxOutValue>3760</maxOutValue>"),
            [[0.0625153, 0.1498901, 0.9181929], [0.0625153, 0.2997802, 0.9181929]],
            [[0.0625611, 0.15011, 0.9188661], [0.0625611, 0.30022, 0.9188661]]),
        ("noclamp", range(&format!("{f32_depths} style=\"noClamp\""), full),
            [[-1.325, -0.10625, 3.3625], [-0.29375, 0.175, 1.39375]],
            [[-0.06, 0.2866667, 1.2733334], [0.2333333, 0.3666667, 0.7133333]]),
        ("min_only", range(f32_depths, "<minInValue>0.25</minInValue><minOutValue>0.25</minOutValue>"),
            [[0.25, 0.25, 2.0], [0.25, 0.3, 0.95]],
            [[0.25, 0.25, 2.0], [0.25, 0.3, 0.95]]),
        ("max_only", range(&format!("{f32_depths} style=\"Clamp\""),
            "<maxInValue>0.75</maxInValue><maxOutValue>0.75</maxOutValue>"),
            [[-0.5, 0.15, 0.75], [0.05, 0.3, 0.75]],
            [[-0.5, 0.15, 0.75], [0.05, 0.3, 0.75]]),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (name, xml, fwd, inv) in cases {
        let path = dir.path().join(format!("{name}.clf"));
        std::fs::write(&path, xml).unwrap();
        for (direction, want) in [
            (TransformDirection::Forward, fwd),
            (TransformDirection::Inverse, inv),
        ] {
            let proc = Processor::from_transform(&Transform::file(&path), direction)
                .unwrap_or_else(|e| panic!("{name} {direction:?}: {e}"));
            let mut px = PX;
            proc.apply_rgb(&mut px);
            for (got, want) in px.iter().zip(want.iter()) {
                for c in 0..3 {
                    assert!(
                        (got[c] - want[c]).abs() < 2e-6,
                        "{name} {direction:?}: got {got:?}, PyOCIO {want:?}"
                    );
                }
            }
        }
    }

    // OCIO's load errors: one-sided bounds must match, and every bound is a single value.
    #[rustfmt::skip]
    let bad = [
        (range(f32_depths, "<minInValue>0.1</minInValue><minOutValue>0.2</minOutValue>"),
            "In and out minimum limits must be equal if maximum values are missing in Range."),
        (range(f32_depths, "<minInValue>0.1 0.2 0.3</minInValue><maxInValue>0.9</maxInValue>\
                            <minOutValue>0</minOutValue><maxOutValue>1</maxOutValue>"),
            "Range element: non-single value."),
    ];
    for (i, (xml, msg)) in bad.into_iter().enumerate() {
        let path = dir.path().join(format!("bad{i}.clf"));
        std::fs::write(&path, xml).unwrap();
        let err = Processor::from_transform(&Transform::file(&path), TransformDirection::Forward)
            .err()
            .unwrap_or_else(|| panic!("bad{i} was accepted"));
        assert!(err.to_string().contains(msg), "bad{i}: {err}");
    }

    // The RangeTransform goes through the same `RangeOpData` rules. PyOCIO 2.5.1
    // `RangeTransform(minIn=0.1, minOut=0.2)` -> "RangeTransform validation failed: In and out
    // minimum limits must be equal if maximum values are missing in Range." (this used to
    // compile to an offset of 0.1).
    let one_sided = Transform::Range(RangeTransform {
        name: String::new(),
        min_in: Some(0.1),
        max_in: None,
        min_out: Some(0.2),
        max_out: None,
        style: RangeStyle::Clamp,
        direction: TransformDirection::Forward,
    });
    let err = Processor::from_transform(&one_sided, TransformDirection::Forward)
        .err()
        .unwrap();
    assert!(
        err.to_string().contains(
            "In and out minimum limits must be equal if maximum values are missing in Range."
        ),
        "{err}"
    );
}

/// B28: the processor has OCIO's RGBA semantics. Only two OCIO CPU ops touch alpha: the 4x4
/// matrix (alpha column into RGB, alpha row + offset into alpha; `MatrixOpCPU.cpp`) and the
/// gamma op (`ExponentTransform` / `ExponentWithLinearTransform` evaluate channel 3 with their
/// own alpha parameters; `GammaOpCPU.cpp`); every other op passes alpha through. An image
/// without alpha behaves as alpha 0 (PyOCIO 2.5.1 RGB images and `applyRGB`).
/// Reference: PyOpenColorIO 2.5.1 on this group (`applyRGBA`, `PackedImageDesc` 4-ch f32 and
/// u8, `PlanarImageDesc` with an alpha plane, 3-ch packed for RGB, inverse processor).
#[test]
fn rgba_matches_pyocio() {
    #[rustfmt::skip]
    let m = [0.9, 0.05, 0.02, 0.1,  0.03, 1.1, 0.0, -0.05,  0.01, 0.02, 0.8, 0.2,  0.25, -0.1, 0.05, 0.7];
    let group = Transform::group(vec![
        Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: m,
            offset: [0.01, -0.02, 0.03, 0.05],
            direction: TransformDirection::Forward,
        }),
        Transform::Exponent(ExponentTransform {
            name: String::new(),
            value: [1.2, 1.1, 0.9, 2.0],
            negative_style: NegativeStyle::Mirror,
            direction: TransformDirection::Forward,
        }),
        Transform::ExponentWithLinear(ExponentWithLinearTransform::new(
            [2.4, 2.2, 2.0, 1.8],
            [0.055, 0.099, 0.1, 0.2],
        )),
        // CLAMP style, alpha exponent 1.5: negative alpha -> 0.
        Transform::Exponent(ExponentTransform {
            name: String::new(),
            value: [1.0, 1.0, 1.0, 1.5],
            negative_style: NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        }),
        Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [1.1, 0.9, 1.0],
            offset: [0.01, 0.0, -0.01],
            power: [1.0, 1.2, 0.8],
            saturation: 0.9,
            style: CdlStyle::NoClamp,
            direction: TransformDirection::Forward,
        }),
    ]);
    const IN: [[f32; 4]; 3] = [
        [0.18, 0.5, 0.9, 0.75],
        [-0.2, 0.05, 1.4, -0.3],
        [0.7, 0.3, 0.1, 1.0],
    ];
    #[rustfmt::skip]
    const RGBA: [[f32; 4]; 3] = [
        [0.0688492, 0.1542237, 0.8088238, 0.1392729],
        [0.01740743, 0.01061609, 1.008034, 0.0],
        [0.4913212, 0.05360487, 0.2300346, 0.6278983],
    ];
    #[rustfmt::skip]
    const RGB: [[f32; 3]; 3] = [
        [0.04889596, 0.18046163, 0.64371896],
        [0.01792721, 0.01020108, 1.0826046],
        [0.34167087, 0.06593923, 0.08431989],
    ];
    // 5e-5: OCIO's CPU pow is the SSE approximation (`ssePower`); an f64 evaluation of the same
    // chain lands on our values (row 1 B: f64 1.0080559, ours 1.008056, PyOCIO 1.008034).
    let close = |got: &[f32], want: &[f32], what: &str| {
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 5e-5, "{what}: got {got:?}, PyOCIO {want:?}");
        }
    };
    let proc = Processor::from_transform(&group, TransformDirection::Forward).unwrap();

    let mut px = IN;
    proc.apply_rgba(&mut px);
    for (g, w) in px.iter().zip(&RGBA) {
        close(g, w, "apply_rgba");
    }

    let mut rgb = IN.map(|p| [p[0], p[1], p[2]]);
    proc.apply_rgb(&mut rgb);
    for (g, w) in rgb.iter().zip(&RGB) {
        close(g, w, "apply_rgb (alpha 0)");
    }

    // Packed f32 RGBA.
    let mut packed: Vec<f32> = IN.iter().flatten().copied().collect();
    proc.apply_packed(
        bytemuck::cast_slice_mut(&mut packed),
        0,
        3,
        1,
        ChannelOrdering::Rgba,
        PackedBitDepth::F32,
        4,
        16,
        48,
    )
    .unwrap();
    for (g, w) in packed.chunks(4).zip(&RGBA) {
        close(g, w, "apply_packed f32 RGBA");
    }

    // Planar f32 with an alpha plane.
    let mut planes: [Vec<f32>; 4] = std::array::from_fn(|c| IN.iter().map(|p| p[c]).collect());
    let [r, g, b, a] = &mut planes;
    proc.apply_planar(
        [r, g, b].map(|p| bytemuck::cast_slice_mut(p.as_mut_slice())),
        Some(bytemuck::cast_slice_mut(a.as_mut_slice())),
        0,
        3,
        1,
        PackedBitDepth::F32,
        4,
        12,
    )
    .unwrap();
    for (i, w) in RGBA.iter().enumerate() {
        close(
            &planes.each_ref().map(|p| p[i]),
            w,
            "apply_planar with alpha",
        );
    }

    // u8, stored B, G, R, A. PyOCIO (u8 in/out processor, RGBA order):
    // [46,128,230,191, 0,13,255,77, 179,77,26,255] -> [18,40,207,35, 5,2,197,4, 126,14,59,160].
    let mut bgra: [u8; 12] = [230, 128, 46, 191, 255, 13, 0, 77, 26, 77, 179, 255];
    proc.apply_packed(
        &mut bgra,
        0,
        3,
        1,
        ChannelOrdering::Bgra,
        PackedBitDepth::Uint8,
        1,
        4,
        12,
    )
    .unwrap();
    assert_eq!(bgra, [207, 40, 18, 35, 197, 2, 5, 4, 59, 14, 126, 160]);

    // Inverse: back to the input, alpha included (PyOCIO inverse of RGBA row 0:
    // [0.18000324, 0.50000376, 0.90004712, 0.74998814]).
    let inv = Processor::from_transform(&group, TransformDirection::Inverse).unwrap();
    let mut back = [RGBA[0]];
    inv.apply_rgba(&mut back);
    close(
        &back[0],
        &[0.18000324, 0.50000376, 0.900_047_1, 0.74998814],
        "inverse apply_rgba",
    );

    // A CDL never touches alpha, also inverted with the v1.2 clamps (whose power step is built
    // from an exponent op). PyOCIO: CDL_ASC inverse of [0.3, 0.6, 0.2, -0.3] ->
    // [0.24269155, 0.7362833, 0.11583986, -0.3].
    let cdl = Transform::Cdl(CdlTransform {
        name: String::new(),
        slope: [1.1, 0.9, 1.0],
        offset: [0.01, 0.0, -0.01],
        power: [1.0, 1.2, 0.8],
        saturation: 0.9,
        style: CdlStyle::AscCdl,
        direction: TransformDirection::Forward,
    });
    let inv = Processor::from_transform(&cdl, TransformDirection::Inverse).unwrap();
    let mut px = [[0.3, 0.6, 0.2, -0.3]];
    inv.apply_rgba(&mut px);
    close(
        &px[0],
        &[0.24269155, 0.7362833, 0.11583986, -0.3],
        "inverse ASC CDL",
    );
}

/// B28: a Nuke `.vf` `global_transform` is a full 4x4 applied to RGBA with no offset
/// (`FileFormatVF.cpp`: `CreateMatrixOp(m44)`; its fourth column multiplies alpha), and the
/// inverse file runs inverse LUT, then inverse matrix. It used to be read as a 3x3 plus an
/// offset column (alpha taken as 1), with an alpha offset of `m[15]`, and "inverted" by applying
/// the forward matrix first. PyOpenColorIO 2.5.1 on the same file (identity 2^3 LUT).
#[test]
fn vf_global_transform_is_a_4x4_on_rgba() {
    let rows: Vec<String> = (0..8)
        .map(|i| format!("{}.0 {}.0 {}.0", i >> 2, (i >> 1) & 1, i & 1))
        .collect();
    let vf = format!(
        "#Inventor V2.1 ascii\ngrid_size 2 2 2\nglobal_transform 0.45 0.025 0.01 0.1  \
         0.015 0.55 0.0 -0.05  0.005 0.01 0.4 0.2  0.125 -0.05 0.025 0.7\ndata\n{}\n",
        rows.join("\n")
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.vf");
    std::fs::write(&path, vf).unwrap();
    #[rustfmt::skip]
    let cases = [
        (TransformDirection::Forward,
            [0.362, 0.189, 0.247, 0.62], [0.282, 0.229, 0.087]),
        (TransformDirection::Inverse,
            [0.2003981, 0.2270479, -0.1620046, 1.1152936], [0.3324876, 0.1679797, 0.142883]),
    ];
    for (direction, rgba, rgb) in cases {
        let proc = Processor::from_transform(&Transform::file(&path), direction).unwrap();
        let mut px = [[0.3_f32, 0.2, 0.1, 0.8]];
        proc.apply_rgba(&mut px);
        let mut px3 = [[0.3_f32, 0.2, 0.1]];
        proc.apply_rgb(&mut px3);
        for (got, want) in [(&px[0][..], &rgba[..]), (&px3[0][..], &rgb[..])] {
            for (g, w) in got.iter().zip(want) {
                assert!(
                    (g - w).abs() < 1e-4,
                    "{direction:?}: got {got:?}, PyOCIO {want:?}"
                );
            }
        }
    }
}

/// OPS-11 remainder: the ACES RedMod inverse solves its quadratic without clamping the
/// discriminant (`FixedFunctionOpCPU.cpp:634,737`), so a negative discriminant is a NaN in OCIO.
/// Clamping it at 0 made up a value OCIO never produces. PyOpenColorIO 2.5.1 inverse processor.
#[test]
fn red_mod_inverse_matches_pyocio_including_nan() {
    #[rustfmt::skip]
    let cases = [
        (FixedFunctionStyle::AcesRedMod03, [-0.1_f32, -1.0, -1.0], [f32::NAN, f32::NAN, -1.0]),
        (FixedFunctionStyle::AcesRedMod03, [-0.1, -1.0, -0.5], [-0.011922561, -1.0, -0.45106804]),
        (FixedFunctionStyle::AcesRedMod03, [0.9, 0.3, 0.1], [0.998_376_4, 0.32459414, 0.1]),
        (FixedFunctionStyle::AcesRedMod10, [-0.1, -1.0, -1.0], [f32::NAN, -1.0, -1.0]),
        (FixedFunctionStyle::AcesRedMod10, [-0.1, -1.0, -0.5], [f32::NAN, -1.0, -0.5]),
        (FixedFunctionStyle::AcesRedMod10, [0.9, 0.3, 0.1], [1.029646, 0.3, 0.1]),
    ];
    for (style, input, want) in cases {
        let t = Transform::FixedFunction(FixedFunctionTransform {
            name: String::new(),
            style,
            params: vec![],
            direction: TransformDirection::Inverse,
        });
        let p = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
        let mut px = [input];
        p.apply_rgb(&mut px);
        for c in 0..3 {
            let (g, w) = (px[0][c], want[c]);
            assert!(
                if w.is_nan() {
                    g.is_nan()
                } else {
                    (g - w).abs() < 2e-5
                },
                "{style:?} {input:?}: got {:?}, PyOCIO {want:?}",
                px[0]
            );
        }
    }
}

/// CLF `<ASC_CDL>`, `<Exponent>`, `<Log>` and 3x4 `<Matrix>` nodes, forward and inverse, against
/// PyOpenColorIO 2.5.1 `FileTransform` on the same bytes. The reader dropped the first three,
/// `compile_clf` read a 3x4 matrix as 3x3 + trailing offsets, and an inverse CLF only reversed
/// the node order without inverting Matrix/CDL/Exponent/Log nodes.
#[test]
#[expect(
    clippy::approx_constant,
    reason = "PyOpenColorIO's recorded outputs (antiLog2 of 0.5 = 1.414213): golden data, not constants"
)]
fn clf_cdl_exponent_log_matrix_nodes_match_pyocio() {
    const PX: [[f32; 3]; 2] = [[0.18, 0.5, 0.9], [-0.2, 0.05, 1.3]];
    let f32d = r#"inBitDepth="32f" outBitDepth="32f""#;
    let sop = "<SOPNode><Slope>1.1 0.9 1.2</Slope><Offset>0.01 -0.02 0.03</Offset>\
               <Power>1.2 0.8 1.1</Power></SOPNode>";
    let sat = "<SatNode><Saturation>0.8</Saturation></SatNode>";
    let m34 = "<Array dim=\"3 4\">0.9 0.05 0.02 0.1 0.03 1.1 0.0 -0.05 0.01 0.02 0.8 0.2</Array>";
    let cam = r#"base="2" logSideSlope="0.07" logSideOffset="0.55" linSideSlope="1" linSideOffset="0.0078" linSideBreak="0.0078""#;
    /// (name, CLF body, forward output pixels, inverse output pixels)
    type ClfCase = (&'static str, String, [[f32; 3]; 2], [[f32; 3]; 2]);
    #[rustfmt::skip]
    let cases: Vec<ClfCase> = vec![
        ("cdl_fwd", format!("<ASC_CDL {f32d} style=\"Fwd\">{sop}{sat}</ASC_CDL>"),
            [[0.2152724, 0.5009744, 0.8937281], [0.02191858, 0.06374411, 0.8219285]],
            [[0.1351452, 0.5008455, 0.8083423], [0.0, 0.0393498, 0.8083423]]),
        ("cdl_revnc", format!("<ASC_CDL {f32d} style=\"RevNoClamp\">{sop}{sat}</ASC_CDL>"),
            [[0.1351452, 0.5008455, 0.8157482], [-0.2561591, 0.04254987, 1.254901]],
            [[0.2170287, 0.5027307, 0.9927861], [-0.1454015, 0.06442405, 1.354971]]),
        ("exp_basic", format!("<Exponent {f32d} style=\"basicFwd\"><ExponentParams exponent=\"2.2\"/></Exponent>"),
            [[0.02299277, 0.2176421, 0.7931175], [0.0, 0.001373198, 1.78107]],
            [[0.458656, 0.7297417, 0.9532418], [0.0, 0.2562254, 1.126661]]),
        ("exp_mirror_rev_ch", format!("<Exponent {f32d} style=\"basicMirrorRev\"><ExponentParams channel=\"R\" exponent=\"2.2\"/><ExponentParams channel=\"G\" exponent=\"1.8\"/><ExponentParams channel=\"B\" exponent=\"2.6\"/></Exponent>"),
            [[0.458656, 0.6803988, 0.9602905], [-0.4811572, 0.1893234, 1.106177]],
            [[0.02299277, 0.2871793, 0.7603883], [-0.02899123, 0.004551411, 1.978144]]),
        ("exp_passthru", format!("<Exponent {f32d} style=\"basicPassThruFwd\"><ExponentParams exponent=\"2.4\"/></Exponent>"),
            [[0.01631717, 0.1894682, 0.7765805], [-0.2, 0.0007542687, 1.877026]],
            [[0.4894357, 0.7491546, 0.9570534], [-0.2, 0.2870148, 1.115519]]),
        ("exp_moncurve", format!("<Exponent {f32d} style=\"monCurveFwd\"><ExponentParams exponent=\"2.4\" offset=\"0.055\"/></Exponent>"),
            [[0.02721228, 0.2140369, 0.7874182], [-0.01547603, 0.003935934, 1.823293]],
            [[0.4613546, 0.7353581, 0.9546914], [-2.584642, 0.2478006, 1.121872]]),
        ("exp_moncurve_mirror_rev", format!("<Exponent {f32d} style=\"monCurveMirrorRev\"><ExponentParams exponent=\"2.2\" offset=\"0.099\"/></Exponent>"),
            [[0.4050629, 0.7029861, 0.9486128], [-0.4297917, 0.1825917, 1.1392]],
            [[0.04899294, 0.2631097, 0.8106847], [-0.05705275, 0.01153054, 1.700634]]),
        ("log_log10", format!("<Log {f32d} style=\"log10\"/>"),
            [[-0.7447309, -0.3010261, -0.04575518], [-37.92978, -1.30103, 0.1139455]],
            [[1.513557, 3.16227, 7.94327], [0.630959, 1.122018, 19.95267]]),
        ("log_antilog2", format!("<Log {f32d} style=\"antiLog2\"></Log>"),
            [[1.132884, 1.414213, 1.866071], [0.8705512, 1.035263, 2.462295]],
            [[-2.473942, -0.9999871, -0.1519954], [-126.0, -4.321929, 0.3785188]]),
        ("log_lin2log", format!("<Log {f32d} style=\"linToLog\"><LogParams base=\"10\" logSideSlope=\"0.29\" logSideOffset=\"0.62\" linSideSlope=\"1.1\" linSideOffset=\"0.01\"/></Log>"),
            [[0.4222391, 0.5469743, 0.6200011], [-10.37963, 0.275744, 0.6659241]],
            [[0.01853816, 0.34151, 8.38792], [-0.007738774, 0.0007512437, 201.1015]]),
        ("log_camera", format!("<Log {f32d} style=\"cameraLinToLog\"><LogParams {cam}/></Log>"),
            [[0.381108, 0.4815633, 0.5402316], [-1.215382, 0.2621049, 0.5771004]],
            [[0.01783544, 0.6017083, 31.99211], [-0.04315104, -0.004532843, 1680.035]]),
        ("log_camera_ls", format!("<Log {f32d} style=\"cameraLogToLin\"><LogParams {cam} linearSlope=\"5.2\"/></Log>"),
            [[0.01783544, 0.6017083, 31.99211], [-0.05563044, -0.007553518, 1680.035]],
            [[0.381108, 0.4815633, 0.5402316], [-0.9507217, 0.2621049, 0.5771004]]),
        ("mtx34", format!("<Matrix {f32d}>{m34}</Matrix>"),
            [[0.305, 0.5054, 0.9317999], [-0.0515, -0.0009999983, 1.239]],
            [[0.04201916, 0.498854, 0.8620034], [-0.3695458, 0.1009876, 1.377095]]),
        ("chain", format!("<Matrix {f32d}>{m34}</Matrix><Exponent {f32d} style=\"monCurveFwd\"><ExponentParams exponent=\"2.4\" offset=\"0.055\"/></Exponent><ASC_CDL {f32d} style=\"FwdNoClamp\">{sop}</ASC_CDL>"),
            [[0.05806664, 0.2504353, 1.057517], [0.001992255, -0.02006964, 2.130541]],
            [[0.3810687, 0.6971734, 0.8172024], [-2.894862, 0.3461894, 1.045386]]),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (name, body, fwd, inv) in cases {
        let path = dir.path().join(format!("{name}.clf"));
        std::fs::write(
            &path,
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ProcessList id=\"t\" compCLFversion=\"3\">\n{body}\n</ProcessList>\n"),
        )
        .unwrap();
        for (direction, want) in [
            (TransformDirection::Forward, fwd),
            (TransformDirection::Inverse, inv),
        ] {
            let proc = Processor::from_transform(&Transform::file(&path), direction)
                .unwrap_or_else(|e| panic!("{name} {direction:?}: {e}"));
            let mut px = PX;
            proc.apply_rgb(&mut px);
            for (got, want) in px.iter().zip(want.iter()) {
                for c in 0..3 {
                    // Relative 5e-5: OCIO's CPU pow/log are its SSE approximations.
                    let tol = 5e-5 * want[c].abs().max(1.0);
                    assert!(
                        (got[c] - want[c]).abs() < tol,
                        "{name} {direction:?}: got {got:?}, PyOCIO {want:?}"
                    );
                }
            }
        }
    }
}

/// `.vf` with a non-identity 3D LUT (inverse = inverse LUT, then inverse 4x4). PyOpenColorIO
/// 2.5.1 `FileTransform` (linear interpolation) on the same file.
fn vf_non_identity_case(directions: &[TransformDirection]) {
    let n = 5;
    let mut rows = Vec::new();
    for r in 0..n {
        for g in 0..n {
            for b in 0..n {
                let (r, g, b) = (r as f64 / 4.0, g as f64 / 4.0, b as f64 / 4.0);
                rows.push(format!(
                    "{:.6} {:.6} {:.6}",
                    r.powf(1.5) * 0.9 + 0.05 * g,
                    g.powf(0.8),
                    0.1 + 0.8 * b.powf(1.2)
                ));
            }
        }
    }
    let vf = format!(
        "#Inventor V2.1 ascii\ngrid_size 5 5 5\nglobal_transform 0.225 0.0125 0.005 0.1  \
         0.0075 0.275 0.0 -0.05  0.0025 0.005 0.2 0.2  0.0625 -0.025 0.0125 0.7\ndata\n{}\n",
        rows.join("\n")
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nl.vf");
    std::fs::write(&path, vf).unwrap();
    let input = [[0.3_f32, 0.2, 0.1, 0.8], [0.5, 0.6, 0.4, 1.0]];
    #[rustfmt::skip]
    let cases = [
        (TransformDirection::Forward,
            [[0.274972_f32, 0.324929, 0.266321, 0.635], [0.581635, 0.830395, 0.554063, 0.80625]]),
        // PyOCIO at OPTIMIZATION_LOSSLESS (exact `InvLut3DRenderer`; its default level bakes
        // the inverse on a 48^3 grid instead: R 0.3249 here, see the GPU gate).
        (TransformDirection::Inverse,
            [[0.3208723, 0.1394399, -0.2162608, 1.0438197], [0.4318461, 0.4201863, 0.1635522, 1.2962134]]),
    ];
    for (direction, want) in cases.into_iter().filter(|(d, _)| directions.contains(d)) {
        let proc = Processor::from_transform(&Transform::file(&path), direction).unwrap();
        let mut px = input;
        proc.apply_rgba(&mut px);
        for (got, want) in px.iter().zip(want.iter()) {
            for c in 0..4 {
                assert!(
                    (got[c] - want[c]).abs() < 1e-4,
                    "{direction:?}: got {got:?}, PyOCIO {want:?}"
                );
            }
        }
    }
}

#[test]
fn vf_non_identity_lut_forward_matches_pyocio() {
    vf_non_identity_case(&[TransformDirection::Forward]);
}

/// The inverse is OCIO's exact per-pixel `InvLut3DRenderer` (`processor/inv_lut.rs`). It used to
/// be a Newton-baked grid of the forward LUT's own size, off by 0.09 (B of pixel 0: -0.116).
#[test]
fn vf_non_identity_lut_inverse_matches_pyocio() {
    vf_non_identity_case(&[TransformDirection::Inverse]);
}

/// A `.3dl` with a non-identity shaper line is shaper (1D) then 3D LUT, and the inverse is the
/// inverse 3D LUT then the inverse shaper (`FileFormat3DL.cpp` `buildFileOps`). FileTransform used
/// `read_3dl`, which refuses a shaper. PyOpenColorIO 2.5.1 (linear) on the same file; the 3D LUT
/// is an identity so the inverse does not depend on the Lut3D inverse gap.
fn threedl_shaper_case(directions: &[TransformDirection]) {
    let mut lines = vec!["0 100 400 700 1023".to_string()];
    for r in [0, 4095] {
        for g in [0, 4095] {
            for b in [0, 4095] {
                lines.push(format!("{r} {g} {b}"));
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sh.3dl");
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    const IN: [[f32; 3]; 2] = [[0.3, 0.2, 0.1], [0.5, 0.6, 0.9]];
    #[rustfmt::skip]
    let cases = [
        (TransformDirection::Forward, [[0.156403_f32, 0.078201, 0.039101], [0.391007, 0.508309, 0.873705]]),
        (TransformDirection::Inverse, [[0.422417, 0.337167, 0.251917], [0.592917, 0.678167, 0.92082]]),
    ];
    for (direction, want) in cases.into_iter().filter(|(d, _)| directions.contains(d)) {
        let proc = Processor::from_transform(&Transform::file(&path), direction)
            .unwrap_or_else(|e| panic!("{direction:?}: {e}"));
        let mut px = IN;
        proc.apply_rgb(&mut px);
        for (got, want) in px.iter().zip(want.iter()) {
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() < 1e-5,
                    "{direction:?}: got {got:?}, PyOCIO {want:?}"
                );
            }
        }
    }
}

#[test]
fn threedl_shaper_forward_matches_pyocio() {
    threedl_shaper_case(&[TransformDirection::Forward]);
}

/// The inverse shaper is OCIO's exact per-pixel `InvLut1DRenderer` (`processor/inv_lut.rs`). It
/// used to be a table with the forward LUT's own 5 entries (G of pixel 0: 0.3038 vs 0.3372).
#[test]
fn threedl_shaper_inverse_matches_pyocio() {
    threedl_shaper_case(&[TransformDirection::Inverse]);
}

/// OCIO-11: a LUT next to its own inverse is removed and replaced by the clamp the round trip
/// imposes (`OpOptimizers.cpp` `RemoveInverseOps`, `Lut1DOpData::getPairIdentityReplacement`,
/// `Lut3DOpData::getIdentityReplacement`); it used to run the LUT and then its baked inverse.
/// PyOpenColorIO 2.5.1 default processor on the same groups: exact values.
#[test]
fn lut_next_to_its_inverse_is_a_clamp_as_in_ocio() {
    let dir = tempfile::tempdir().unwrap();
    let flat = dir.path().join("flat1d.cube");
    std::fs::write(
        &flat,
        "LUT_1D_SIZE 5\n0 0 0\n0 0 0\n0.3 0.3 0.3\n0.7 0.7 0.7\n1 1 1\n",
    )
    .unwrap();
    let mut rows = Vec::new();
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                let (r, g, b) = (r as f64, g as f64, b as f64);
                rows.push(format!(
                    "{:.6} {:.6} {:.6}",
                    0.1 + 0.8 * r,
                    0.9 * g + 0.05 * r,
                    0.2 + 0.7 * b
                ));
            }
        }
    }
    let lut3 = dir.path().join("nl3d.cube");
    std::fs::write(&lut3, format!("LUT_3D_SIZE 2\n{}\n", rows.join("\n"))).unwrap();
    let file = |p: &std::path::Path, inverse: bool| {
        let mut t = Transform::file(p);
        if let Transform::FileTransform(f) = &mut t {
            f.interpolation = Interpolation::Linear;
            if inverse {
                f.direction = TransformDirection::Inverse;
            }
        }
        t
    };
    const IN: [[f32; 3]; 2] = [[0.1, 0.5, 1.2], [-0.3, 0.26, 0.9]];
    #[rustfmt::skip]
    let cases = [
        // Forward then inverse: the flat start (entries 0 and 1 equal) narrows the clamp to 0.25.
        ("1d fwd,inv", vec![file(&flat, false), file(&flat, true)], [[0.25_f32, 0.5, 1.0], [0.25, 0.26, 0.9]]),
        ("1d inv,fwd", vec![file(&flat, true), file(&flat, false)], [[0.1, 0.5, 1.0], [0.0, 0.26, 0.9]]),
        ("3d fwd,inv", vec![file(&lut3, false), file(&lut3, true)], [[0.1, 0.5, 1.0], [0.0, 0.26, 0.9]]),
    ];
    for (name, group, want) in cases {
        let p = Processor::from_transform(&Transform::group(group), TransformDirection::Forward)
            .unwrap();
        assert!(
            p.ops()
                .iter()
                .all(|op| !matches!(op, ProcessorOp::Lut1d { .. } | ProcessorOp::Lut3d { .. })),
            "{name}: LUT ops left: {:?}",
            p.ops().iter().map(|o| o.variant_name()).collect::<Vec<_>>()
        );
        let mut px = IN;
        p.apply_rgb(&mut px);
        for (got, want) in px.iter().zip(want.iter()) {
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() < 1e-6,
                    "{name}: got {got:?}, PyOCIO {want:?}"
                );
            }
        }
    }
}

/// OCIO-11: the rest of `RemoveInverseOps` this port can recognise (Log, LogAffine, moncurve
/// pairs, nested `A, B, B', A'`) and COMP_RANGE, iterated to a fixpoint. PyOpenColorIO 2.5.1
/// default processor: the values, and what its optimised group keeps (in the comments).
#[test]
fn optimizer_pairs_and_ranges_match_ocio() {
    let (fwd, inv) = (TransformDirection::Forward, TransformDirection::Inverse);
    let ewl = |d| {
        let mut t =
            ExponentWithLinearTransform::new([2.4, 2.4, 2.4, 1.0], [0.055, 0.055, 0.055, 0.0]);
        t.direction = d;
        Transform::ExponentWithLinear(t)
    };
    let log = |base, d| {
        Transform::Log(LogTransform {
            name: String::new(),
            base,
            direction: d,
        })
    };
    let la = |d| {
        Transform::LogAffine(LogAffineTransform {
            name: String::new(),
            base: 10.0,
            lin_side_slope: [1.1; 3],
            lin_side_offset: [0.02; 3],
            log_side_slope: [0.3; 3],
            log_side_offset: [0.6; 3],
            direction: d,
        })
    };
    let range = |a, b, c, d| {
        Transform::Range(RangeTransform {
            name: String::new(),
            min_in: Some(a),
            max_in: Some(b),
            min_out: Some(c),
            max_out: Some(d),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Forward,
        })
    };
    const IN: [[f32; 3]; 2] = [[-0.5, 0.3, 2.0], [0.05, 0.01, -0.02]];
    /// (name, transform chain, expected op count, expected output pixels)
    type ChainCase = (&'static str, Vec<Transform>, usize, [[f32; 3]; 2]);
    #[rustfmt::skip]
    let cases: Vec<ChainCase> = vec![
        // OCIO keeps one Range.
        ("log2 fwd,inv", vec![log(2.0, fwd), log(2.0, inv)], 1, [[0.0, 0.3, 2.0], [0.05, 0.01, 0.0]]),
        // OCIO keeps nothing.
        ("log2 inv,fwd", vec![log(2.0, inv), log(2.0, fwd)], 0, IN),
        ("logaffine fwd,inv", vec![la(fwd), la(inv)], 1, [[-0.01818182, 0.3, 2.0], [0.05, 0.01, -0.01818182]]),
        ("ewl fwd,inv", vec![ewl(fwd), ewl(inv)], 0, IN),
        ("nested", vec![ewl(fwd), log(10.0, inv), log(10.0, fwd), ewl(inv)], 0, IN),
        ("range,range", vec![range(0.1, 0.9, -0.2, 1.3), range(0.0, 1.0, 0.2, 0.7)], 1,
            [[0.2, 0.2875, 0.7], [0.2, 0.2, 0.2]]),
    ];
    for (name, group, n_ops, want) in cases {
        let p = Processor::from_transform(&Transform::group(group), TransformDirection::Forward)
            .unwrap();
        assert_eq!(
            p.ops().len(),
            n_ops,
            "{name}: {:?}",
            p.ops().iter().map(|o| o.variant_name()).collect::<Vec<_>>()
        );
        let mut px = IN;
        p.apply_rgb(&mut px);
        for (got, want) in px.iter().zip(want.iter()) {
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() < 1e-6,
                    "{name}: got {got:?}, PyOCIO {want:?}"
                );
            }
        }
    }
}

/// COLOR-10(b): `.icc` / `.icm` / `.pf` Matrix/TRC profiles as a FileTransform, built as OCIO
/// `FileFormatICC::buildFileOps` (forward = PCS XYZ -> device). The fixtures are OCIO's own test
/// profiles (single-entry `curv` gammas, `curv` tables, `para` types 0-4). PyOpenColorIO 2.5.1,
/// OPTIMIZATION_LOSSLESS, both directions.
#[test]
fn icc_matrix_trc_profiles_match_pyocio() {
    const IN: [[f32; 3]; 3] = [[0.3, 0.2, 0.1], [0.9, 0.5, 1.4], [-0.1, 0.02, 0.6]];
    /// (name, three input pixels, the three expected output pixels)
    type IccCase = (&'static str, [[f32; 3]; 3], [[f32; 3]; 3]);
    #[rustfmt::skip]
    let cases: [IccCase; 7] = [
        ("icc-test-1.icc",
         [[0.7059765, 0.3321534, 0.3204792], [1.032258, 0.3868131, 1.155478], [-0.6757176, 0.4338402, 0.7959542]],
         [[0.04740816, 0.03974067, 0.01023249], [0.8923125, 0.5302705, 2.114614], [0.05759494, 0.02271975, 0.3221993]]),
        ("icc-test-2.pf",
         [[0.7578229, 0.3162973, 0.318941], [1.132707, 0.3655056, 1.172588], [-0.752239, 0.4443286, 0.7996886]],
         [[0.04512274, 0.03956564, 0.01112579], [0.8004435, 0.4866628, 2.041463], [0.05298197, 0.01965658, 0.3172416]]),
        ("icc-test-3.icm",
         [[0.8065006, 0.3293146, 0.3163029], [1.0, 0.3869379, 1.0], [0.0, 0.4358512, 0.8119968]],
         [[0.04385626, 0.03997731, 0.01489451], [0.5817975, 0.3927016, 0.9915218], [0.05805502, 0.02410252, 0.3030554]]),
        ("icc-test-pc1.icc",
         [[0.8332605, 0.4220642, 0.4108572], [1.0, 0.4717357, 1.0], [0.09090909, 0.5138735, 0.8379995]],
         [[0.0143269, 0.01065934, 0.001318067], [0.5449241, 0.3381749, 0.9829385], [0.04489102, 0.01795345, 0.2364464]]),
        ("icc-test-pc2.icc",
         [[0.8120484, 0.09481916, 0.09481916], [1.0, 0.2942331, 1.0], [0.09481916, 0.3863094, 0.8175132]],
         [[0.1074586, 0.1091342, 0.1100525], [0.5845839, 0.4030539, 0.9933732], [0.1351515, 0.1160372, 0.3201055]]),
        ("icc-test-pc3.icc",
         [[0.8064966, 0.3292974, 0.3162913], [1.0, 0.3869419, 1.0], [0.0, 0.4358434, 0.8119961]],
         [[0.04385493, 0.03997394, 0.01489287], [0.5817991, 0.3927022, 0.9915219], [0.05805647, 0.02410431, 0.3030593]]),
        ("icc-test-pc4.icc",
         [[0.7804344, 0.0, 0.0], [1.0, 0.1755662, 1.0], [0.0, 0.2831225, 0.7868179]],
         [[0.1351049, 0.1365659, 0.1227222], [0.6192389, 0.4542735, 1.001435], [0.1475482, 0.1218508, 0.3827004]]),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/icc");
    let mut bad = Vec::new();
    for (name, fwd, inv) in cases {
        for (direction, want) in [
            (TransformDirection::Forward, fwd),
            (TransformDirection::Inverse, inv),
        ] {
            let proc = Processor::from_transform(&Transform::file(dir.join(name)), direction)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut px = IN;
            proc.apply_rgb(&mut px);
            for (got, want) in px.iter().zip(want.iter()) {
                if (0..3).any(|c| (got[c] - want[c]).abs() > 1e-4 * want[c].abs().max(1.0)) {
                    bad.push(format!(
                        "{name} {direction:?}: got {got:?}, PyOCIO {want:?}"
                    ));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// COLOR-10(b): what OCIO's ICC reader rejects, with its message.
#[test]
fn icc_rejects_non_icc_data_as_ocio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.icc");
    let mut data = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/icc/icc-test-3.icm"),
    )
    .unwrap();
    data[36..40].copy_from_slice(b"xxxx");
    std::fs::write(&path, &data).unwrap();
    let err = Processor::from_transform(&Transform::file(&path), TransformDirection::Forward)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Error parsing .icc file (") && err.contains(").  Wrong magic number."),
        "{err}"
    );
    std::fs::write(&path, b"short").unwrap();
    let err = Processor::from_transform(&Transform::file(&path), TransformDirection::Forward)
        .unwrap_err()
        .to_string();
    assert!(err.contains("Error loading header."), "{err}");
}

/// CLF 4x5 (alpha and offsets) and 4x4 (alpha) Matrix nodes, bit-depth scaled, as OCIO
/// `CTFReaderMatrixElt_1_3` (they were refused as Unsupported); and a node's `<Description>`
/// does not replace the ProcessList's (OCIO keeps "top" as the list description).
/// PyOpenColorIO 2.5.1 FileTransform, both directions, RGBA.
#[test]
fn clf_alpha_matrices_and_descriptions_match_pyocio() {
    let clf = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="m45" compCLFversion="3.0">
  <Description>top</Description>
  <Matrix inBitDepth="10i" outBitDepth="32f">
    <Description>node</Description>
    <Array dim="4 5">
      1.1 0.1 0.0 0.05 0.01
      0.0 0.9 0.1 0.0 -0.02
      0.2 0.0 1.0 0.0 0.03
      0.1 0.0 0.0 0.8 0.1
    </Array>
  </Matrix>
  <Matrix inBitDepth="32f" outBitDepth="32f">
    <Array dim="4 4">
      1 0 0 0.2
      0 1 0 0
      0 0 1 0
      0.3 0 0 0.5
    </Array>
  </Matrix>
</ProcessList>
"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m45.clf");
    std::fs::write(&path, clf).unwrap();
    let pl = vfx_lut::read_clf(&path).unwrap();
    assert_eq!(pl.description.as_deref(), Some("top"));
    const IN: [[f32; 4]; 2] = [[0.3, 0.5, 0.7, 0.5], [1.0, 0.0, 0.1, 1.0]];
    #[rustfmt::skip]
    let cases = [
        (TransformDirection::Forward, [[502.323, 531.94, 777.51, 344.2925], [1360.62, 10.21, 306.93, 813.338]]),
        (TransformDirection::Inverse, [[1.172319e-06, 0.000492043, 0.000654702, 0.001016249],
                                       [0.0005148077, 2.555986e-05, -3.453536e-05, 0.001757386]]),
    ];
    for (direction, want) in cases {
        let proc = Processor::from_transform(&Transform::file(&path), direction).unwrap();
        let mut px = IN;
        proc.apply_rgba(&mut px);
        for (got, want) in px.iter().zip(want.iter()) {
            for c in 0..4 {
                assert!(
                    (got[c] - want[c]).abs() <= 1e-5 * want[c].abs().max(1e-3),
                    "{direction:?}: got {got:?}, PyOCIO {want:?}"
                );
            }
        }
    }
}

/// The optimizer changes no pixel of a real chain: every pair below is built unoptimized and
/// compared with its `Lossless` and `Best` (OCIO VERY_GOOD, its default) forms on probes that
/// include negatives and values above 1. Only identity removal and matrix combining were tested
/// before, on synthetic chains.
#[test]
fn optimized_processor_matches_unoptimized_on_studio_chains() {
    let config = crate::builtin::aces_2_0_studio_all_views();
    let chains = [
        ("ACEScg", "ACEScct"),
        ("ARRI LogC3 (EI800)", "ACEScg"),
        ("S-Log3 S-Gamut3.Cine", "ACES2065-1"),
        ("V-Log V-Gamut", "sRGB Encoded Rec.709 (sRGB)"),
        ("Log3G10 REDWideGamutRGB", "ACEScct"),
        ("ACEScg", "sRGB - Display"),
        ("ACES2065-1", "Rec.2100-PQ - Display"),
        ("sRGB Encoded P3-D65", "sRGB Encoded AP1"),
    ];
    let probes: Vec<[f32; 3]> = [-0.05f32, 0.0, 0.01, 0.18, 0.5, 0.9, 1.0, 4.0]
        .iter()
        .flat_map(|&a| [[a, 0.18, 0.5], [0.3, a, 0.05], [0.7, 0.2, a]])
        .collect();
    let mut worst = Vec::new();
    let mut ops_removed = 0;
    for (src, dst) in chains {
        let reference = config
            .processor_with_opts(src, dst, OptimizationLevel::None)
            .unwrap_or_else(|e| panic!("{src} -> {dst}: {e}"));
        let mut want = probes.clone();
        reference.apply_rgb(&mut want);
        for level in [OptimizationLevel::Lossless, OptimizationLevel::Best] {
            let optimized = reference.optimized(level);
            ops_removed += reference.num_ops() - optimized.num_ops();
            let mut got = probes.clone();
            optimized.apply_rgb(&mut got);
            let err = got
                .iter()
                .flatten()
                .zip(want.iter().flatten())
                .map(|(g, w)| (g - w).abs() / w.abs().max(1.0))
                .fold(0.0f32, f32::max);
            worst.push((src, dst, level, err));
        }
    }
    // The optimizer must have had work to do, or the comparison says nothing.
    assert!(ops_removed > 0);
    let bad: Vec<_> = worst.iter().filter(|w| w.3 > 1e-5).collect();
    assert!(bad.is_empty(), "{bad:#?}\nall: {worst:#?}");
}
