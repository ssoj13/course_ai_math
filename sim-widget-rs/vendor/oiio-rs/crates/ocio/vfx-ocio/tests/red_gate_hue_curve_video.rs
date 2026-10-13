//! Red-gate for finding **EMIT-14** — `HueCurve` Video-style luminance gain, CPU vs GPU.
//!
//! # What this pins
//!
//! For `GradingHueCurveStyle::Video`, the CPU and the four shader emitters must apply the
//! **same** luminance-gain operator. They currently do not:
//!
//! | | Log | Linear | Video |
//! |---|---|---|---|
//! | CPU (`grading_hue_curve.rs:246-250`, keys on `is_linear`) | additive | multiplicative | **additive** |
//! | GPU (`gpu/glsl.rs:1208-1217` + 3 siblings, keys on `style == Log`) | additive | multiplicative | **multiplicative** |
//!
//! where *additive* is `y += (hueLumGain + satLumGain - 2) * 0.1` and *multiplicative* is
//! `y *= hueLumGain * satLumGain`.
//!
//! # This is a deliberate divergence from OCIO — do not "fix" it by copying a golden
//!
//! Upstream OCIO disagrees with **itself** here: its CPU path branches on `m_isLinear` while its
//! GPU path branches on `style == GRADING_LOG`, so upstream Video pixels differ between its own
//! two backends. We ported that inconsistency faithfully. The decision recorded for this repair
//! is to be **correct rather than bit-parity with OCIO**, so this file deliberately does *not*
//! carry an OCIO golden — importing one would re-assert the upstream bug.
//!
//! ## Why *additive* is the correct branch
//!
//! The operator is chosen by the encoding the luminance channel is *currently in*, not by the
//! style label. `Linear` style is the only one that runs `lin_to_log` on entry and `log_to_lin`
//! on exit (`grading_hue_curve.rs:216-240`), so it — and only it — holds a genuinely linear
//! luminance at the point the gain is applied, where scaling is the meaningful operation. `Log`
//! and `Video` both hold a non-linear, roughly logarithmic luminance, where a gain is correctly
//! expressed as an offset; the `* 0.1` factor is that log-domain scaling.
//!
//! Keying on `is_linear` (our CPU) therefore states the real precondition. Keying on
//! `style == Log` (our GPU, inherited from OCIO) is an enumeration of one case that happens to
//! omit `Video`. **The GPU emitters are the side to repair.**
//!
//! # Why the existing tests miss it
//!
//! `gpu/tests.rs` exercises the hue-curve emitters for shader-text wellformedness and for the
//! Log and Linear styles, where CPU and GPU happen to agree. No test compares the Video branch
//! across backends, so the one style where they disagree is the one style nobody checks.
//!
//! # Assertion strategy, and its honest limitation
//!
//! There is no in-tree harness that executes emitted GLSL/HLSL/Metal/WGSL, so this file cannot
//! compare CPU and GPU *pixels* directly. It instead asserts at the source level: the
//! luminance-gain statement emitted for `Video` must be identical to the one emitted for `Log`,
//! because the CPU treats those two styles identically (`is_linear` is false for both). That is
//! a proxy for numeric agreement, and it is stated here rather than left implicit. A test that
//! merely checked the shader parses would be exactly the blindness being repaired.
//!
//! # Expected to fail
//!
//! Expected to **fail** until EMIT-14 is repaired in `gpu/{glsl,hlsl,metal,wgsl}.rs`.

use vfx_ocio::{
    GpuLanguage, GpuProcessor, GradingHueCurveStyle, GradingHueCurveTransform, Processor,
    Transform, TransformDirection,
};

/// Every language whose emitter carries the EMIT-14 branch. All four share the defect, so all
/// four are pinned.
const LANGUAGES: [(GpuLanguage, &str); 4] = [
    (GpuLanguage::Glsl330, "GLSL"),
    (GpuLanguage::Hlsl50, "HLSL"),
    (GpuLanguage::Metal, "Metal"),
    (GpuLanguage::Wgsl, "WGSL"),
];

/// Builds a hue-curve transform in `style` with a **non-identity** HUE_LUM curve.
///
/// Non-identity matters twice over: an identity curve set trips the emitter's `local_bypass`
/// path and produces no gain statement at all, and a unit gain makes additive and multiplicative
/// numerically indistinguishable (`y*1*1 == y + (1+1-2)*0.1`). Either would make the test vacuous
/// — the same failure mode as the identity matrix in `gpu/tests.rs`.
/// The transform itself is always built Forward; the direction under test is applied once, by
/// `Processor::from_transform`. Setting it in both places would compose to Forward again and the
/// "inverse" test would silently re-check the forward emitter.
fn hue_curve_transform(style: GradingHueCurveStyle) -> Transform {
    let mut t = GradingHueCurveTransform {
        style,
        direction: TransformDirection::Forward,
        ..Default::default()
    };
    // Boost luminance in the reds, cut it in the cyans: gains well away from 1.0.
    t.hue_lum = vec![
        [0.0, 1.4],
        [1.0 / 6.0, 1.2],
        [2.0 / 6.0, 1.0],
        [0.5, 0.7],
        [4.0 / 6.0, 0.8],
        [5.0 / 6.0, 1.1],
    ];
    Transform::GradingHueCurve(t)
}

fn shader_for(
    style: GradingHueCurveStyle,
    direction: TransformDirection,
    language: GpuLanguage,
) -> Result<String, Box<dyn std::error::Error>> {
    let transform = hue_curve_transform(style);
    let processor = Processor::from_transform(&transform, direction)?;
    let gpu = GpuProcessor::from_processor(&processor)?;
    Ok(gpu
        .generate_shader(language)
        .unwrap()
        .fragment_code()
        .to_string())
}

/// Returns the emitted statements that combine both gain variables — i.e. the luminance-gain
/// operator, whichever form it took.
///
/// Matching on the two variable names rather than on a literal expression keeps this
/// language-agnostic: it does not assume GLSL's `color.b`, HLSL's swizzles, or WGSL's `outColor`
/// spelling, only that both gains meet in one statement.
fn luma_gain_statements(shader: &str) -> Vec<String> {
    shader
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("hueLumGain") && line.contains("satLumGain"))
        .map(str::to_owned)
        .collect()
}

/// Core assertion, shared by the forward and inverse tests.
///
/// `Log` is the reference because CPU and GPU already agree on it; `Video` must match it,
/// since the CPU applies one and the same branch to both.
fn assert_video_matches_log(
    direction: TransformDirection,
    label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut divergent = Vec::new();

    for (language, name) in LANGUAGES {
        let log_stmts =
            luma_gain_statements(&shader_for(GradingHueCurveStyle::Log, direction, language)?);
        let video_stmts = luma_gain_statements(&shader_for(
            GradingHueCurveStyle::Video,
            direction,
            language,
        )?);

        assert!(
            !log_stmts.is_empty(),
            "{name} {label}: no luminance-gain statement emitted for Log style. The probe is \
             broken (likely local_bypass), not the code under test."
        );
        assert!(
            !video_stmts.is_empty(),
            "{name} {label}: no luminance-gain statement emitted for Video style. The probe is \
             broken, not the code under test."
        );

        if log_stmts != video_stmts {
            divergent.push(format!(
                "  {name} {label}:\n    Log   (== CPU, correct): {log_stmts:?}\n    \
                 Video (actual, wrong):   {video_stmts:?}"
            ));
        }
    }

    assert!(
        divergent.is_empty(),
        "EMIT-14: Video-style luminance gain diverges from the CPU on {} of {} backends.\n{}\n\
         The CPU applies the additive form to Video (grading_hue_curve.rs:246-250 keys on \
         is_linear, which is false for Video). The emitters key on `style == Log` and so fall \
         through to the multiplicative form. Repair the emitters, not the CPU.",
        divergent.len(),
        LANGUAGES.len(),
        divergent.join("\n")
    );

    Ok(())
}

/// Forward direction. Expected to FAIL: all four emitters take the multiplicative branch for
/// Video while the CPU takes the additive one.
#[test]
fn red_gate_hue_curve_video_luma_gain_emit_14_forward() -> Result<(), Box<dyn std::error::Error>> {
    assert_video_matches_log(TransformDirection::Forward, "forward")
}

/// Inverse direction. The same `style == Log` test guards the inverse gain
/// (`gpu/glsl.rs:1246-1259`), so the defect is symmetric and must be pinned on both sides —
/// repairing only the forward branch would leave the round-trip broken.
#[test]
fn red_gate_hue_curve_video_luma_gain_emit_14_inverse() -> Result<(), Box<dyn std::error::Error>> {
    assert_video_matches_log(TransformDirection::Inverse, "inverse")
}

/// CONTROL — `Linear` style genuinely *should* use the multiplicative form, on both CPU and GPU,
/// and is **expected to pass**.
///
/// Without this, a lazy repair could satisfy the two tests above by making every style additive.
/// This pins the other side of the branch so the correct distinction survives the fix.
#[test]
fn control_hue_curve_linear_luma_gain_is_multiplicative() -> Result<(), Box<dyn std::error::Error>>
{
    for (language, name) in LANGUAGES {
        let stmts = luma_gain_statements(&shader_for(
            GradingHueCurveStyle::Linear,
            TransformDirection::Forward,
            language,
        )?);
        assert!(
            !stmts.is_empty(),
            "{name}: no luminance-gain statement emitted for Linear style; probe is broken."
        );
        let joined = stmts.join(" ");
        assert!(
            !joined.contains("0.1"),
            "{name} [control]: Linear style must keep the multiplicative luminance gain, but the \
             emitted statement carries the additive log-domain form: {stmts:?}"
        );
    }
    Ok(())
}
