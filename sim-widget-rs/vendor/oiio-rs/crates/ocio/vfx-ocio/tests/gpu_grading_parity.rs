//! GPU == CPU for the grading ops and every FixedFunction style, in every style and direction
//! (ROADMAP G6). The primary, tone and curves are compared in every style and direction.
//!
//! The emitted WGSL runs on the shared device (`apply_gpu`) and is compared with the CPU processor
//! (`Processor::apply_rgba`, the oracle) on a small image of non-neutral colours. Before this test
//! the GPU grading curves, tone and most FixedFunction styles were only checked as shader text;
//! the hue curve's RGB <-> HSY had been a different formula from the CPU's (and OCIO's) with
//! nothing to notice it. Every mismatch is collected and reported together.
#![cfg(feature = "gpu")]

use vfx_ocio::{
    FixedFunctionStyle, FixedFunctionTransform, GradingHueCurveStyle, GradingHueCurveTransform,
    GradingPrimaryStyle, GradingPrimaryTransform, GradingRgbCurveTransform, GradingToneStyle,
    GradingToneTransform, HsyTransformStyle, Processor, Transform, TransformDirection, apply_gpu,
};

fn have_gpu(test: &str) -> bool {
    let present = gpu_mem::shared_device().is_some();
    if !present {
        vfx_core::test_support::gpu_unavailable(test, "gpu_mem::shared_device() is None");
    }
    present
}

/// 4x2 non-neutral colours in `[0.02, 1.6]` (every op here is defined there).
fn image() -> Vec<[f32; 4]> {
    vec![
        [0.10, 0.20, 0.30, 1.0],
        [0.90, 0.15, 0.40, 1.0],
        [0.25, 0.75, 0.05, 1.0],
        [0.60, 0.55, 0.50, 1.0],
        [1.50, 0.66, 0.99, 1.0],
        [0.02, 0.48, 0.80, 1.0],
        [0.35, 0.05, 0.70, 1.0],
        [0.80, 0.80, 0.20, 1.0],
    ]
}

/// The largest relative difference over RGB (`|g - c| / max(1, |c|)`; both NaN counts as equal).
fn worst(gpu: &[[f32; 4]], cpu: &[[f32; 4]]) -> f32 {
    let mut m = 0.0f32;
    for (g, c) in gpu.iter().zip(cpu) {
        for ch in 0..3 {
            if g[ch].is_nan() && c[ch].is_nan() {
                continue;
            }
            let d = (g[ch] - c[ch]).abs() / c[ch].abs().max(1.0);
            m = m.max(if d.is_nan() { f32::INFINITY } else { d });
        }
    }
    m
}

/// Runs `transform` on CPU and GPU; `Err` describes a mismatch above `eps`.
fn compare(label: &str, transform: &Transform, eps: f32) -> Result<(), String> {
    let processor = Processor::from_transform(transform, TransformDirection::Forward)
        .map_err(|e| format!("{label}: processor: {e}"))?;
    let input = image();
    let mut cpu = input.clone();
    processor.apply_rgba(&mut cpu);
    let gpu =
        apply_gpu(&processor, 4, 2, &input).map_err(|e| format!("{label}: apply_gpu: {e}"))?;
    let w = worst(&gpu, &cpu);
    if w <= eps {
        Ok(())
    } else {
        let i = (0..gpu.len())
            .max_by(|&a, &b| {
                worst(&gpu[a..=a], &cpu[a..=a]).total_cmp(&worst(&gpu[b..=b], &cpu[b..=b]))
            })
            .unwrap();
        Err(format!(
            "{label}: max rel diff {w:.3e} at pixel {i}: gpu {:?} cpu {:?}",
            gpu[i], cpu[i]
        ))
    }
}

const DIRS: [TransformDirection; 2] = [TransformDirection::Forward, TransformDirection::Inverse];
const EPS: f32 = 5e-3;

fn report(failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn rgb_curve_gpu_equals_cpu() {
    if !have_gpu("rgb_curve_gpu_equals_cpu") {
        return;
    }
    let mut failures = Vec::new();
    for style in [
        GradingToneStyle::Log,
        GradingToneStyle::Linear,
        GradingToneStyle::Video,
    ] {
        for direction in DIRS {
            let mut t = GradingRgbCurveTransform::new(style);
            if style == GradingToneStyle::Linear {
                t.red = vec![[-7.0, -7.0], [-2.0, -1.5], [0.0, 0.3], [7.0, 7.0]];
                t.master = vec![[-7.0, -7.0], [-1.0, -0.8], [7.0, 7.0]];
            } else {
                t.red = vec![[0.0, 0.0], [0.4, 0.55], [1.0, 1.0]];
                t.green = vec![[0.0, 0.05], [0.5, 0.45], [1.0, 0.95]];
                t.master = vec![[0.0, 0.0], [0.3, 0.35], [0.7, 0.72], [1.0, 1.0]];
            }
            t.direction = direction;
            if let Err(e) = compare(
                &format!("RgbCurve {style:?} {direction:?}"),
                &Transform::GradingRgbCurve(t),
                EPS,
            ) {
                failures.push(e);
            }
        }
    }
    report(failures);
}

#[test]
fn hue_curve_gpu_equals_cpu() {
    if !have_gpu("hue_curve_gpu_equals_cpu") {
        return;
    }
    let mut failures = Vec::new();
    for style in [
        GradingHueCurveStyle::Log,
        GradingHueCurveStyle::Linear,
        GradingHueCurveStyle::Video,
    ] {
        for rgb_to_hsy in [HsyTransformStyle::Default, HsyTransformStyle::None] {
            for direction in DIRS {
                let mut t = GradingHueCurveTransform::new(style);
                t.rgb_to_hsy = rgb_to_hsy;
                for p in t.hue_sat.iter_mut().step_by(2) {
                    p[1] = 1.2;
                }
                for p in t.hue_lum.iter_mut().skip(1).step_by(3) {
                    p[1] = 0.9;
                }
                if let Some(p) = t.sat_sat.get_mut(1) {
                    p[1] *= 1.1;
                }
                if let Some(p) = t.lum_lum.get_mut(1) {
                    p[1] += 0.05;
                }
                if let Some(p) = t.hue_hue.get_mut(2) {
                    p[1] += 0.03;
                }
                t.direction = direction;
                let label = format!("HueCurve {style:?} {rgb_to_hsy:?} {direction:?}");
                if let Err(e) = compare(&label, &Transform::GradingHueCurve(t), EPS) {
                    failures.push(e);
                }
            }
        }
    }
    report(failures);
}

#[test]
fn primary_gpu_equals_cpu() {
    if !have_gpu("primary_gpu_equals_cpu") {
        return;
    }
    let mut failures = Vec::new();
    for style in [
        GradingPrimaryStyle::Log,
        GradingPrimaryStyle::Linear,
        GradingPrimaryStyle::Video,
    ] {
        for direction in DIRS {
            let mut t = GradingPrimaryTransform::new(style);
            let v = &mut t.value;
            v.brightness.red = 0.05;
            v.contrast.master = 1.15;
            v.contrast.green = 0.9;
            v.gamma.blue = 1.2;
            v.gamma.master = 0.9;
            v.offset.red = 0.02;
            v.exposure.master = 0.3;
            v.lift.green = 0.03;
            v.gain.red = 1.1;
            v.saturation = 1.2;
            t.direction = direction;
            if let Err(e) = compare(
                &format!("Primary {style:?} {direction:?}"),
                &Transform::GradingPrimary(t),
                EPS,
            ) {
                failures.push(e);
            }
        }
    }
    report(failures);
}

#[test]
fn tone_gpu_equals_cpu() {
    if !have_gpu("tone_gpu_equals_cpu") {
        return;
    }
    let mut failures = Vec::new();
    for style in [
        GradingToneStyle::Log,
        GradingToneStyle::Linear,
        GradingToneStyle::Video,
    ] {
        for direction in DIRS {
            let v = vfx_ocio::grading_tone::GradingTone::from_style(style);
            let mut t = GradingToneTransform {
                style,
                blacks: v.blacks,
                shadows: v.shadows,
                midtones: v.midtones,
                highlights: v.highlights,
                whites: v.whites,
                scontrast: 1.1,
                direction,
                ..GradingToneTransform::default()
            };
            t.midtones.red = 1.2;
            t.shadows.master = 0.9;
            t.highlights.green = 1.1;
            t.whites.master = 1.05;
            t.blacks.blue = 0.95;
            if let Err(e) = compare(
                &format!("Tone {style:?} {direction:?}"),
                &Transform::GradingTone(t),
                EPS,
            ) {
                failures.push(e);
            }
        }
    }
    report(failures);
}

/// Parameters of a style: OCIO's ACES 1.3 gamut compression defaults, the ITU surround gamma, an
/// HLG-like gamma-log and a Canon-Log-2-like double-log curve, a Rec.709 ACES 2.0 display.
fn ff_params(style: FixedFunctionStyle) -> Vec<f64> {
    let aces2 = vec![1000.0, 0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290];
    match style {
        FixedFunctionStyle::AcesGamutComp13 => vec![1.147, 1.264, 1.312, 0.815, 0.803, 0.880, 1.2],
        FixedFunctionStyle::Rec2100Surround => vec![0.78],
        FixedFunctionStyle::LinToGammaLog | FixedFunctionStyle::GammaLogToLin => {
            vec![
                0.0,
                0.25,
                0.5,
                1.0,
                0.0,
                std::f64::consts::E,
                0.17883277,
                0.807825,
                0.28466892,
                1.0,
                0.0,
            ]
        }
        FixedFunctionStyle::LinToDoubleLog | FixedFunctionStyle::DoubleLogToLin => vec![
            10.0,
            0.0,
            0.0,
            -0.241_360_77,
            0.092_864_125,
            -96.777_08,
            1.0,
            0.241_360_77,
            0.092_864_125,
            96.777_08,
            1.0,
            1.0,
            0.0,
        ],
        FixedFunctionStyle::AcesOutputTransform20
        | FixedFunctionStyle::AcesTonescaleCompress20
        | FixedFunctionStyle::AcesGamutCompress20 => aces2,
        FixedFunctionStyle::AcesRgbToJmh20 | FixedFunctionStyle::AcesJmhToRgb20 => {
            aces2[1..].to_vec()
        }
        _ => vec![],
    }
}

#[test]
fn fixed_function_gpu_equals_cpu() {
    if !have_gpu("fixed_function_gpu_equals_cpu") {
        return;
    }
    use FixedFunctionStyle as S;
    let mut failures = Vec::new();
    for style in [
        S::AcesRedMod03,
        S::AcesRedMod10,
        S::AcesGlow03,
        S::AcesGlow10,
        S::AcesDarkToDim10,
        S::AcesGamutComp13,
        S::Rec2100Surround,
        S::RgbToHsv,
        S::HsvToRgb,
        S::XyzToXyy,
        S::XyyToXyz,
        S::XyzToUvy,
        S::UvyToXyz,
        S::XyzToLuv,
        S::LuvToXyz,
        S::LinToPq,
        S::PqToLin,
        S::LinToGammaLog,
        S::GammaLogToLin,
        S::LinToDoubleLog,
        S::DoubleLogToLin,
        S::AcesOutputTransform20,
        S::AcesRgbToJmh20,
        S::AcesJmhToRgb20,
        S::AcesTonescaleCompress20,
        S::AcesGamutCompress20,
        S::RgbToHsyLin,
        S::HsyLinToRgb,
        S::RgbToHsyLog,
        S::HsyLogToRgb,
        S::RgbToHsyVid,
        S::HsyVidToRgb,
    ] {
        for direction in DIRS {
            let t = Transform::FixedFunction(FixedFunctionTransform {
                name: String::new(),
                style,
                params: ff_params(style),
                direction,
            });
            if let Err(e) = compare(&format!("FixedFunction {style:?} {direction:?}"), &t, EPS) {
                failures.push(e);
            }
        }
    }
    report(failures);
}
