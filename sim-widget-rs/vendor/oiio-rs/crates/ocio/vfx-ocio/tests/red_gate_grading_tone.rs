//! Red-gate: GradingTone validate (C10 Tone remainder).
//!
//! The wb `setOnLimit` (`>=`) port is covered by rustdoc on `compute_wb_fwd`/`compute_wb_rev`:
//! at the real precomputed knots the gain-mapped and HDR-extrapolation pieces meet to ~1e-7,
//! so a pixel gate at equality is vacuous (same class as OPS-09). The comparison still matches
//! the reference and is kept; do not reintroduce `>`.

use vfx_ocio::grading_tone::GradingTone;
use vfx_ocio::{GradingToneStyle, GradingToneTransform, Processor, Transform, TransformDirection};

#[test]
fn validate_accepts_style_defaults() {
    for style in [
        GradingToneStyle::Log,
        GradingToneStyle::Linear,
        GradingToneStyle::Video,
    ] {
        assert!(
            GradingTone::from_style(style).validate().is_ok(),
            "{style:?} defaults must validate"
        );
    }
}

#[test]
fn validate_rejects_out_of_range() {
    let mut tone = GradingTone::from_style(GradingToneStyle::Log);
    tone.highlights.red = 2.5;
    assert!(
        tone.validate().is_err(),
        "highlights.red=2.5 out of SH bound"
    );

    let mut tone = GradingTone::from_style(GradingToneStyle::Log);
    tone.shadows.master = 1.85;
    assert!(
        tone.validate().is_err(),
        "shadows.master=1.85 out of SH bound"
    );

    let mut tone = GradingTone::from_style(GradingToneStyle::Log);
    tone.whites.master = 1.85;
    assert!(
        tone.validate().is_ok(),
        "whites.master=1.85 is inside BMW [0.1,1.9]"
    );

    let mut tone = GradingTone::from_style(GradingToneStyle::Log);
    tone.s_contrast = 2.5;
    assert!(tone.validate().is_err());

    let mut tone = GradingTone::from_style(GradingToneStyle::Log);
    tone.blacks.width = 0.001;
    assert!(tone.validate().is_err(), "blacks.width floor");

    let mut tone = GradingTone::from_style(GradingToneStyle::Log);
    tone.shadows.width = 0.0;
    assert!(
        tone.validate().is_ok(),
        "shadows.width=0 is the LOG default pivot"
    );

    let mut tone = GradingTone::from_style(GradingToneStyle::Log);
    tone.shadows.width = tone.shadows.start;
    assert!(tone.validate().is_err(), "shadow pivot must clear start");
}

#[test]
fn compile_rejects_invalid_tone() {
    let mut t = GradingToneTransform::default();
    t.highlights.red = 2.5;
    let err = Processor::from_transform(&Transform::GradingTone(t), TransformDirection::Forward);
    assert!(err.is_err(), "compile must surface validate failure");
}
