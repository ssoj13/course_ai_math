//! ROADMAP C4: ACES 2.0 FixedFunction ops with degenerate primaries from a config are an error,
//! not a panic.
//!
//! The primaries of `ACES_RGB_TO_JMH_20`, `ACES_OUTPUT_TRANSFORM_20` and
//! `ACES_GAMUT_COMPRESS_20` come from the config's `params`. Collinear chromaticities make the
//! RGB-to-XYZ matrix singular; building the processor used to panic in `init_jmh_params`. It now
//! returns an error. All-zero placeholder primaries still build: they give NaN on purpose
//! (`aces2.rs`, `rgb_to_xyz_f64`), and ops that ignore the JMh params rely on that.

use vfx_ocio::{
    FixedFunctionStyle, FixedFunctionTransform, Processor, Transform, TransformDirection,
};

/// Three chromaticities on the line x = y: every column of the XYZ matrix has X == Y, so it is
/// exactly singular.
const COLLINEAR: [f64; 8] = [0.3, 0.3, 0.2, 0.2, 0.1, 0.1, 0.3127, 0.3290];
/// Rec.709 primaries, D65.
const REC709: [f64; 8] = [0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290];

fn build(style: FixedFunctionStyle, params: Vec<f64>) -> Result<Processor, vfx_ocio::OcioError> {
    let t = Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style,
        params,
        direction: TransformDirection::Forward,
    });
    Processor::from_transform(&t, TransformDirection::Forward)
}

fn with_peak(prims: [f64; 8]) -> Vec<f64> {
    let mut v = vec![100.0];
    v.extend(prims);
    v
}

#[test]
fn rgb_to_jmh_with_collinear_primaries_is_an_error() {
    let err = build(FixedFunctionStyle::AcesRgbToJmh20, COLLINEAR.to_vec())
        .expect_err("collinear primaries must be refused, not panic");
    assert!(!err.to_string().is_empty());
    build(FixedFunctionStyle::AcesRgbToJmh20, REC709.to_vec()).expect("Rec.709 builds");
}

#[test]
fn output_transform_with_collinear_limiting_primaries_is_an_error() {
    build(
        FixedFunctionStyle::AcesOutputTransform20,
        with_peak(COLLINEAR),
    )
    .expect_err("collinear limiting primaries must be refused, not panic");
    build(FixedFunctionStyle::AcesOutputTransform20, with_peak(REC709)).expect("Rec.709 builds");
}

#[test]
fn gamut_compress_with_collinear_limiting_primaries_is_an_error() {
    build(
        FixedFunctionStyle::AcesGamutCompress20,
        with_peak(COLLINEAR),
    )
    .expect_err("collinear limiting primaries must be refused, not panic");
    build(FixedFunctionStyle::AcesGamutCompress20, with_peak(REC709)).expect("Rec.709 builds");
}

/// The all-zero placeholder keeps building (NaN on purpose), so the fix did not turn it into an
/// error.
#[test]
fn all_zero_placeholder_primaries_still_build() {
    build(FixedFunctionStyle::AcesRgbToJmh20, vec![0.0; 8]).expect("zero placeholder builds");
    build(
        FixedFunctionStyle::AcesOutputTransform20,
        with_peak([0.0; 8]),
    )
    .expect("zero placeholder builds");
}
