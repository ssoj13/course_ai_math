//! Every GPU op in every style, and the compilers that check the emitted text (ROADMAP G6).
//!
//! [`corpus`] is one [`GpuProcessor`] per op variant x style x direction (and per LUT kind, with
//! its texture), built the way the processor builds them where that is possible
//! ([`GpuProcessor::from_processor`] over a `ProcessorOp` or a `Transform`), directly otherwise.
//!
//! Two uses:
//! - `emitted_shaders_compile`: every corpus entry in every GLSL version through glslang
//!   (`OCIO_RS_GLSLANG` = path to `glslangValidator`) and in HLSL through fxc (`OCIO_RS_FXC` =
//!   path to `fxc.exe`, shader model 5.0); `#[ignore]`d without the tools. Metal has no compiler
//!   on Windows.
//! - `wgsl_validates`: every entry's WGSL through naga's full validator (always runs).
//! - `write_snapshot` (`OCIO_RS_SHADER_SNAPSHOT` = a directory): writes every entry in every
//!   language, so a refactor of the emitters can be diffed against the text before it.

use std::path::{Path, PathBuf};

use super::*;
use crate::grading_bspline::CurveUniformData;
use crate::processor::ProcessorOp;
use crate::transform::{
    AllocationType, CdlStyle, ExposureContrastStyle, FixedFunctionStyle, GradingPrimaryStyle,
    GradingToneStyle, HsyTransformStyle, Lut1DHueAdjust, NegativeStyle,
};

const TRANSFERS: [TransferStyle; 21] = [
    TransferStyle::Srgb,
    TransferStyle::Rec709,
    TransferStyle::Rec2020,
    TransferStyle::Gamma22,
    TransferStyle::Gamma24,
    TransferStyle::Gamma26,
    TransferStyle::Rec1886,
    TransferStyle::Linear,
    TransferStyle::Pq,
    TransferStyle::Hlg,
    TransferStyle::AcesCct,
    TransferStyle::AcesCc,
    TransferStyle::Log3G10,
    TransferStyle::LogC3,
    TransferStyle::LogC4,
    TransferStyle::SLog3,
    TransferStyle::VLog,
    TransferStyle::BmdFilmGen5,
    TransferStyle::AppleLog,
    TransferStyle::CanonCLog2,
    TransferStyle::CanonCLog3,
];

/// ACES 2.0 display: peak luminance, then Rec.709 primaries and D65 white.
const ACES2_OUTPUT: [f32; 9] = [1000.0, 0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290];

/// Parameters of a FixedFunction style (OCIO's ACES 1.3 gamut compression defaults, the ITU
/// surround gamma, the HLG and Canon Log 2 curves of `ff_curves`' tests, the ACES 2.0 display).
fn ff_params(style: FixedFunctionStyle) -> Vec<f32> {
    match style {
        FixedFunctionStyle::AcesGamutComp13 => vec![1.147, 1.264, 1.312, 0.815, 0.803, 0.880, 1.2],
        FixedFunctionStyle::Rec2100Surround => vec![0.78],
        FixedFunctionStyle::LinToGammaLog
        | FixedFunctionStyle::GammaLogToLin
        | FixedFunctionStyle::LinToDoubleLog
        | FixedFunctionStyle::DoubleLogToLin => super::ff_curves::tests::cases()
            .into_iter()
            .find(|(s, _)| *s == style)
            .map(|(_, p)| p)
            .unwrap_or_default(),
        FixedFunctionStyle::AcesOutputTransform20
        | FixedFunctionStyle::AcesTonescaleCompress20
        | FixedFunctionStyle::AcesGamutCompress20 => ACES2_OUTPUT.to_vec(),
        FixedFunctionStyle::AcesRgbToJmh20 | FixedFunctionStyle::AcesJmhToRgb20 => {
            ACES2_OUTPUT[1..].to_vec()
        }
        _ => vec![],
    }
}

const FIXED_FUNCTIONS: [FixedFunctionStyle; 32] = [
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

fn single(op: GpuOp) -> GpuProcessor {
    GpuProcessor {
        ops: vec![op],
        textures: vec![],
    }
}

fn tex(name: &str, texture_type: GpuTextureType, (w, h, d): (u32, u32, u32)) -> GpuTexture {
    GpuTexture {
        name: name.to_string(),
        texture_type,
        width: w,
        height: h,
        depth: d,
        data: vec![0.5; (w * h * d * 4) as usize],
        interpolation: GpuInterpolation::Linear,
    }
}

/// Every op variant x style x direction, named.
pub(crate) fn corpus() -> Vec<(String, GpuProcessor)> {
    let mut out: Vec<(String, GpuProcessor)> = Vec::new();
    let mut push = |name: String, p: GpuProcessor| out.push((name, p));
    let dirs = [true, false];

    let m = Mat4::<RowMajor>::from_rows([
        [0.9, 0.1, 0.0, 0.0],
        [0.05, 0.8, 0.15, 0.0],
        [0.0, 0.2, 0.7, 0.1],
        [0.0, 0.0, 0.1, 0.9],
    ]);
    push(
        "matrix".into(),
        single(GpuOp::Matrix {
            matrix: m,
            offset: [0.01, 0.02, 0.03, 0.0],
        }),
    );

    for style in [CdlStyle::AscCdl, CdlStyle::NoClamp] {
        push(
            format!("cdl_{style:?}"),
            single(GpuOp::Cdl {
                slope: [1.1, 1.0, 0.9],
                offset: [0.01, 0.0, -0.01],
                power: [1.2, 1.0, 0.8],
                saturation: 0.9,
                style,
            }),
        );
    }
    for negative_style in [
        NegativeStyle::Clamp,
        NegativeStyle::Mirror,
        NegativeStyle::PassThru,
        NegativeStyle::Linear,
    ] {
        push(
            format!("exponent_{negative_style:?}"),
            single(GpuOp::Exponent {
                value: [2.2, 2.0, 1.8, 1.0],
                negative_style,
            }),
        );
    }
    for forward in dirs {
        push(
            format!("log_{forward}"),
            single(GpuOp::Log {
                base: 10.0,
                forward,
            }),
        );
    }
    push(
        "range".into(),
        single(GpuOp::Range {
            scale: 1.2,
            offset: -0.1,
            clamp_min: Some(0.0),
            clamp_max: Some(1.0),
        }),
    );
    push(
        "range_open".into(),
        single(GpuOp::Range {
            scale: 1.2,
            offset: -0.1,
            clamp_min: None,
            clamp_max: None,
        }),
    );
    for hue_adjust in [Lut1DHueAdjust::None, Lut1DHueAdjust::Dw3] {
        push(
            format!("lut1d_{hue_adjust:?}"),
            GpuProcessor {
                ops: vec![GpuOp::Lut1D {
                    texture_idx: 0,
                    domain_min: [0.0, -0.1, 0.0],
                    domain_max: [1.0, 1.0, 2.0],
                    hue_adjust,
                }],
                textures: vec![tex("ocio_lut1d_0", GpuTextureType::Texture1D, (17, 1, 1))],
            },
        );
    }
    push(
        "lut1d_half".into(),
        GpuProcessor {
            ops: vec![GpuOp::Lut1DHalf {
                texture_idx: 0,
                width: 4096,
                height: 17,
            }],
            textures: vec![tex(
                "ocio_lut1d_half_0",
                GpuTextureType::Texture2D,
                (4096, 17, 1),
            )],
        },
    );
    for tetrahedral in [false, true] {
        push(
            format!("lut3d_tetra_{tetrahedral}"),
            GpuProcessor {
                ops: vec![GpuOp::Lut3D {
                    texture_idx: 0,
                    domain_min: [0.0; 3],
                    domain_max: [1.0; 3],
                    tetrahedral,
                }],
                textures: vec![tex("ocio_lut3d_0", GpuTextureType::Texture3D, (5, 5, 5))],
            },
        );
    }
    for forward in dirs {
        push(
            format!("log_affine_{forward}"),
            single(GpuOp::LogAffine {
                base: 2.0,
                log_side_slope: [0.25, 0.25, 0.25],
                log_side_offset: [0.4, 0.4, 0.4],
                lin_side_slope: [1.1, 1.0, 0.9],
                lin_side_offset: [0.01, 0.01, 0.01],
                forward,
            }),
        );
        push(
            format!("log_camera_{forward}"),
            single(GpuOp::LogCamera {
                base: 2.0,
                log_side_slope: [0.25; 3],
                log_side_offset: [0.4; 3],
                lin_side_slope: [1.0; 3],
                lin_side_offset: [0.0; 3],
                lin_side_break: [0.01; 3],
                linear_slope: [5.0; 3],
                log_side_break: [0.01f32.log2() * 0.25 + 0.4; 3],
                linear_offset: [0.01f32.log2() * 0.25 + 0.4 - 0.05; 3],
                forward,
            }),
        );
        for mirror in [false, true] {
            push(
                format!("exponent_with_linear_{mirror}_{forward}"),
                single(GpuOp::ExponentWithLinear {
                    gamma: [2.4, 2.2, 2.0, 1.0],
                    offset: [0.055, 0.05, 0.045, 0.0],
                    mirror,
                    forward,
                }),
            );
        }
        for style in TRANSFERS {
            push(
                format!("transfer_{style:?}_{forward}"),
                single(GpuOp::Transfer { style, forward }),
            );
        }
        for allocation in [AllocationType::Uniform, AllocationType::Log2] {
            push(
                format!("allocation_{allocation:?}_{forward}"),
                single(GpuOp::Allocation {
                    allocation,
                    min_val: -8.0,
                    max_val: 6.0,
                    lin_offset: 0.0001,
                    forward,
                }),
            );
        }
    }
    for style in [
        ExposureContrastStyle::Linear,
        ExposureContrastStyle::Video,
        ExposureContrastStyle::Logarithmic,
    ] {
        for forward in [true, false] {
            push(
                format!(
                    "exposure_contrast_{style:?}_{}",
                    if forward { "fwd" } else { "rev" }
                ),
                single(GpuOp::ExposureContrast {
                    exposure: 0.25,
                    contrast: 1.1,
                    gamma: 0.95,
                    pivot: 0.18,
                    log_exposure_step: 0.088,
                    log_mid_gray: 0.435,
                    style,
                    dynamic: crate::EcDynamic::NONE,
                    forward,
                }),
            );
        }
    }
    for style in [
        GradingPrimaryStyle::Log,
        GradingPrimaryStyle::Linear,
        GradingPrimaryStyle::Video,
    ] {
        for forward in dirs {
            push(
                format!("grading_primary_{style:?}_{forward}"),
                single(GpuOp::GradingPrimary {
                    style,
                    brightness: [0.02, 0.0, -0.01],
                    contrast: [1.05, 1.0, 0.95],
                    gamma: [1.1, 1.0, 0.9],
                    exposure: [0.1, 0.0, -0.1],
                    offset: [0.01, 0.0, -0.01],
                    slope: [1.1, 1.0, 0.9],
                    saturation: 1.1,
                    pivot: 0.18,
                    pivot_black: 0.0,
                    pivot_white: 1.0,
                    clamp_black: -1.0,
                    clamp_white: 2.0,
                    forward,
                    local_bypass: false,
                }),
            );
        }
    }
    for style in [
        GradingToneStyle::Log,
        GradingToneStyle::Linear,
        GradingToneStyle::Video,
    ] {
        for forward in dirs {
            push(
                format!("grading_tone_{style:?}_{forward}"),
                single(GpuOp::GradingTone {
                    style,
                    blacks: [1.1, 1.0, 0.9, 1.05, 0.0, 0.2],
                    shadows: [1.1, 1.0, 0.9, 1.05, 0.2, 0.2],
                    midtones: [1.1, 1.0, 0.9, 1.05, 0.4, 0.2],
                    highlights: [1.1, 1.0, 0.9, 1.05, 0.6, 0.2],
                    whites: [1.1, 1.0, 0.9, 1.05, 0.8, 0.2],
                    scontrast: 1.2,
                    forward,
                }),
            );
            push(
                format!("grading_rgb_curve_{style:?}_{forward}"),
                single(GpuOp::GradingRgbCurve {
                    style,
                    curve_data: CurveUniformData::new(),
                    bypass_lin_to_log: false,
                    forward,
                }),
            );
        }
    }
    for style in [
        crate::grading_hue_curve::GradingStyle::Log,
        crate::grading_hue_curve::GradingStyle::Linear,
        crate::grading_hue_curve::GradingStyle::Video,
    ] {
        for rgb_to_hsy in [HsyTransformStyle::None, HsyTransformStyle::Default] {
            for forward in dirs {
                push(
                    format!("grading_hue_curve_{style:?}_{rgb_to_hsy:?}_{forward}"),
                    single(GpuOp::GradingHueCurve {
                        style,
                        rgb_to_hsy,
                        curve_data: CurveUniformData::new(),
                        draw_curve_only: false,
                        forward,
                    }),
                );
            }
        }
    }
    // FixedFunction through the processor, as a config builds it (the ACES 2.0 styles become
    // their own GPU ops there).
    for style in FIXED_FUNCTIONS {
        for forward in dirs {
            let processor = Processor::from_ops(vec![ProcessorOp::FixedFunction {
                style,
                params: ff_params(style),
                forward,
            }]);
            match GpuProcessor::from_processor(&processor) {
                Ok(gpu) => push(format!("ff_{style:?}_{forward}"), gpu),
                Err(e) => panic!("corpus: {style:?} forward={forward}: {e}"),
            }
        }
    }
    out
}

/// The languages emitted, with a file extension for the snapshot.
pub(crate) const LANGUAGES: [(GpuLanguage, &str); 10] = [
    (GpuLanguage::Glsl120, "120.frag"),
    (GpuLanguage::Glsl130, "130.frag"),
    (GpuLanguage::Glsl330, "330.frag"),
    (GpuLanguage::Glsl400, "400.frag"),
    (GpuLanguage::GlslVulkan460, "vk460.frag"),
    (GpuLanguage::GlslEs100, "es100.frag"),
    (GpuLanguage::GlslEs300, "es300.frag"),
    (GpuLanguage::Hlsl50, "hlsl"),
    (GpuLanguage::Metal, "metal"),
    (GpuLanguage::Wgsl, "wgsl"),
];

/// A directory under the system temp directory, unique to this process.
fn scratch_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vfx_ocio_{label}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(tool: &Path, args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new(tool)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", tool.display()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// Every corpus entry, in every GLSL version and HLSL, through the real compilers.
#[test]
#[ignore = "needs OCIO_RS_GLSLANG (glslangValidator) and OCIO_RS_FXC (fxc.exe)"]
fn emitted_shaders_compile() {
    let glslang = PathBuf::from(std::env::var("OCIO_RS_GLSLANG").expect("OCIO_RS_GLSLANG"));
    let fxc = PathBuf::from(std::env::var("OCIO_RS_FXC").expect("OCIO_RS_FXC"));
    let dir = scratch_dir("shader_compile");
    let mut failures: Vec<String> = Vec::new();
    let mut compiled = 0usize;
    for (name, gpu) in corpus() {
        for (lang, ext) in LANGUAGES {
            if matches!(lang, GpuLanguage::Metal | GpuLanguage::Wgsl) {
                continue;
            }
            let code = match gpu.generate_shader(lang) {
                Ok(c) => c,
                Err(e) => {
                    failures.push(format!("{name} {lang:?}: generate_shader: {e}"));
                    continue;
                }
            };
            let path = dir.join(format!("{name}.{ext}"));
            std::fs::write(&path, code.fragment_code()).unwrap();
            let p = path.to_str().unwrap();
            let result = if lang == GpuLanguage::Hlsl50 {
                run(
                    &fxc,
                    &["/nologo", "/T", "ps_5_0", "/E", "main", "/Fo", "NUL", p],
                )
            } else if lang == GpuLanguage::GlslVulkan460 {
                // Vulkan GLSL all the way to SPIR-V: the binding layouts are checked too.
                run(&glslang, &["-V", "-S", "frag", "-o", "NUL", p])
            } else {
                run(&glslang, &["-S", "frag", p])
            };
            compiled += 1;
            if let Err(msg) = result {
                let first: Vec<&str> = msg
                    .lines()
                    .filter(|l| l.contains("ERROR") || l.contains("error"))
                    .take(3)
                    .collect();
                failures.push(format!("{name} {lang:?}: {}", first.join(" | ")));
            }
        }
    }
    eprintln!("{compiled} shaders compiled, {} failed", failures.len());
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        failures.is_empty(),
        "{} of {compiled} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Every corpus entry's WGSL through naga's validator.
#[test]
fn wgsl_validates() {
    let mut failures = Vec::new();
    for (name, gpu) in corpus() {
        match gpu.generate_shader(GpuLanguage::Wgsl) {
            Ok(code) => {
                if let Err(e) = super::tests::validate_wgsl_naga(code.fragment_code()) {
                    failures.push(format!("{name}: {e}"));
                }
            }
            Err(e) => failures.push(format!("{name}: generate_shader: {e}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} WGSL failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Writes every corpus entry in every language to `OCIO_RS_SHADER_SNAPSHOT`.
#[test]
#[ignore = "needs OCIO_RS_SHADER_SNAPSHOT (an output directory)"]
fn write_snapshot() {
    let dir =
        PathBuf::from(std::env::var("OCIO_RS_SHADER_SNAPSHOT").expect("OCIO_RS_SHADER_SNAPSHOT"));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, gpu) in corpus() {
        for (lang, ext) in LANGUAGES {
            let text = match gpu.generate_shader(lang) {
                Ok(c) => c.fragment_code().to_string(),
                Err(e) => format!("ERROR: {e}\n"),
            };
            std::fs::write(dir.join(format!("{name}.{ext}")), text).unwrap();
        }
    }
}
