//! Op bodies written once for every shading language (ROADMAP G6).
//!
//! OCIO writes each op's GPU code once (`ops/*/…OpGPU.cpp` over `GpuShaderText`); the language
//! only changes spellings (`GpuShaderUtils.cpp`). This module is that for the ops whose body is
//! short; the longer ones have their own module (`transfer`, `moncurve`, `gamma`, `log_camera`,
//! `lut1d_half`, `lut3d`, `aces2`, ...). [`emit`] is the one dispatch every language calls.
//!
//! Returned lines follow the shared modules' contract: the block's braces at column 0, its body
//! indented by four spaces; the caller adds the enclosing indentation.

use super::dialect::{Dialect, n, nf};
use super::types::OpUniforms;
use super::{GpuOp, GpuProcessor, GpuTextureType};
use crate::transform::{AllocationType, CdlStyle, ExposureContrastStyle};
use vfx_core::consts::exposure_contrast::{MIN_CONTRAST, VIDEO_OETF_POWER};
use vfx_math::layout::{Mat4, RowMajor};

/// `vec3(a, b, c)` of `v`.
fn v3(d: &Dialect, v: [f32; 3]) -> String {
    format!("{}({}, {}, {})", d.v3, nf(v[0]), nf(v[1]), nf(v[2]))
}

/// The four-component vector type.
fn v4(d: &Dialect) -> &'static str {
    match d.v3 {
        "vec3" => "vec4",
        "float3" => "float4",
        _ => "vec4<f32>",
    }
}

/// `vec4(a, b, c, d)` of `v`.
fn v4_lit(d: &Dialect, v: [f32; 4]) -> String {
    format!(
        "{}({}, {}, {}, {})",
        v4(d),
        nf(v[0]),
        nf(v[1]),
        nf(v[2]),
        nf(v[3])
    )
}

/// An immutable four-component binding.
fn decl4(d: &Dialect, name: &str, expr: &str) -> String {
    let kw = if d.decl_kw == "let " {
        "let ".to_string()
    } else {
        format!("{} ", v4(d))
    };
    format!("    {kw}{name} = {expr};")
}

/// The transform function's body: every op of `p` in order, each under an `// Op <i>` line.
pub(super) fn body(
    p: &GpuProcessor,
    d: &Dialect,
    uniform_bindings: &[Option<OpUniforms>],
) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, op) in p.ops.iter().enumerate() {
        out.push(format!("// Op {idx}"));
        out.extend(emit(
            p,
            d,
            idx,
            op,
            uniform_bindings.get(idx).and_then(|u| u.as_ref()),
        ));
    }
    out
}

/// The body of `op` in dialect `d`.
fn emit(
    p: &GpuProcessor,
    d: &Dialect,
    idx: usize,
    op: &GpuOp,
    uniforms: Option<&OpUniforms>,
) -> Vec<String> {
    let wgsl = d.lang == super::GpuLanguage::Wgsl;
    match op {
        GpuOp::Matrix { matrix, offset } => self::matrix(d, matrix, *offset),
        GpuOp::Cdl {
            slope,
            offset,
            power,
            saturation,
            style,
        } => cdl(d, *slope, *offset, *power, *saturation, *style),
        GpuOp::Exponent {
            value,
            negative_style,
        } => super::gamma::exponent(*value, *negative_style, d),
        GpuOp::Log { base, forward } => log(d, *base, *forward),
        GpuOp::Range {
            scale,
            offset,
            clamp_min,
            clamp_max,
        } => range(d, *scale, *offset, *clamp_min, *clamp_max),
        GpuOp::Lut1D {
            texture_idx,
            domain_min,
            domain_max,
            hue_adjust,
        } => {
            let tex = &p.textures[*texture_idx];
            lut1d(
                d,
                &tex.name,
                tex.texture_type,
                tex.width,
                *domain_min,
                *domain_max,
                *hue_adjust,
            )
        }
        GpuOp::Lut1DHalf {
            texture_idx,
            width,
            height,
        } => {
            let tex = &p.textures[*texture_idx];
            let sample = |uv: &str| d.sample(&tex.name, tex.texture_type, uv);
            super::lut1d_half::emit(d, *width, *height, &sample)
        }
        GpuOp::Lut3D {
            texture_idx,
            domain_min,
            domain_max,
            tetrahedral,
        } => {
            let tex = &p.textures[*texture_idx];
            let sample = |c: &str| d.sample(&tex.name, tex.texture_type, c);
            super::lut3d::emit(
                d,
                tex.width as usize,
                *domain_min,
                *domain_max,
                *tetrahedral,
                &sample,
            )
        }
        GpuOp::LogAffine {
            base,
            log_side_slope,
            log_side_offset,
            lin_side_slope,
            lin_side_offset,
            forward,
        } => log_affine(
            d,
            *base,
            *log_side_slope,
            *log_side_offset,
            *lin_side_slope,
            *lin_side_offset,
            *forward,
        ),
        GpuOp::LogCamera {
            base,
            log_side_slope,
            log_side_offset,
            lin_side_slope,
            lin_side_offset,
            lin_side_break,
            linear_slope,
            log_side_break,
            linear_offset,
            forward,
        } => {
            let params = super::log_camera::LogCameraParams {
                base: *base,
                log_side_slope: *log_side_slope,
                log_side_offset: *log_side_offset,
                lin_side_slope: *lin_side_slope,
                lin_side_offset: *lin_side_offset,
                lin_side_break: *lin_side_break,
                linear_slope: *linear_slope,
                log_side_break: *log_side_break,
                linear_offset: *linear_offset,
            };
            super::log_camera::emit(&params, *forward, d)
        }
        GpuOp::ExponentWithLinear {
            gamma,
            offset,
            mirror,
            forward,
        } => super::moncurve::emit(*gamma, *offset, *mirror, *forward, d),
        GpuOp::Transfer { style, forward } => super::transfer::emit(*style, *forward, d),
        GpuOp::ExposureContrast {
            exposure,
            contrast,
            gamma,
            pivot,
            log_exposure_step,
            log_mid_gray,
            style,
            forward,
            ..
        } => {
            let names = match uniforms {
                Some(OpUniforms::ExposureContrast {
                    exposure,
                    contrast,
                    gamma,
                }) => [exposure.as_str(), contrast.as_str(), gamma.as_str()],
                _ => ["", "", ""],
            };
            exposure_contrast(
                d,
                [*exposure, *contrast, *gamma],
                names,
                &EcShader {
                    style: *style,
                    forward: *forward,
                    pivot: *pivot,
                    log_exposure_step: *log_exposure_step,
                    log_mid_gray: *log_mid_gray,
                },
            )
        }
        GpuOp::Allocation {
            allocation,
            min_val,
            max_val,
            lin_offset,
            forward,
        } => self::allocation(d, *allocation, *min_val, *max_val, *lin_offset, *forward),
        GpuOp::Aces2RgbJmh { forward, .. } => super::aces2::emit_op(*forward, uniforms, d, wgsl),
        GpuOp::Aces2TonescaleCompress { forward, .. } => {
            super::aces2::emit_tonescale_op(*forward, uniforms, d, wgsl)
        }
        GpuOp::Aces2GamutCompress { forward, .. } => {
            super::aces2::emit_gamut_op(*forward, uniforms, d, wgsl)
        }
        GpuOp::Aces2OutputTransform { forward, .. } => {
            super::aces2::emit_output_op(*forward, uniforms, d, wgsl)
        }
        GpuOp::GradingRgbCurve {
            style,
            bypass_lin_to_log,
            forward,
            ..
        } => super::grading::rgb_curve(d, idx, *style, *bypass_lin_to_log, *forward, uniforms),
        GpuOp::GradingHueCurve {
            style,
            rgb_to_hsy,
            draw_curve_only,
            forward,
            ..
        } => super::grading::hue_curve(
            d,
            idx,
            *style,
            *rgb_to_hsy,
            *draw_curve_only,
            *forward,
            uniforms,
        ),
        GpuOp::GradingTone {
            style,
            blacks,
            shadows,
            midtones,
            highlights,
            whites,
            scontrast,
            forward,
        } => super::tone::emit(
            d, *style, blacks, shadows, midtones, highlights, whites, *scontrast, *forward,
            uniforms,
        ),
        GpuOp::GradingPrimary {
            style,
            brightness,
            contrast,
            gamma,
            exposure,
            offset,
            slope,
            saturation,
            pivot,
            pivot_black,
            pivot_white,
            clamp_black,
            clamp_white,
            forward,
            local_bypass,
        } => {
            // OCIO: a non-dynamic primary at its identity emits nothing.
            if uniforms.is_none() && *local_bypass {
                return Vec::new();
            }
            let v = super::primary::Values {
                style: *style,
                brightness,
                contrast,
                gamma,
                exposure,
                offset,
                slope,
                saturation: *saturation,
                pivot: *pivot,
                pivot_black: *pivot_black,
                pivot_white: *pivot_white,
                clamp_black: *clamp_black,
                clamp_white: *clamp_white,
                forward: *forward,
            };
            super::primary::emit(d, &v, uniforms)
        }
        GpuOp::FixedFunction {
            style,
            params,
            forward,
        } => super::fixed_function::emit(d, *style, params, *forward),
    }
}

/// The helper functions every op of `p` needs, declared before the transform function (OCIO's
/// `addToHelperShaderCode`): the curve ops' evaluators.
pub(super) fn helpers(
    p: &GpuProcessor,
    d: &Dialect,
    uniform_bindings: &[Option<OpUniforms>],
) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, op) in p.ops.iter().enumerate() {
        let uniforms = uniform_bindings.get(idx).and_then(|u| u.as_ref());
        let lines = super::grading::helpers(d, idx, op, uniforms);
        if !lines.is_empty() {
            out.extend(lines);
            out.push(String::new());
        }
    }
    out
}

/// OCIO `MatrixOpGPU.cpp`: `res = M * vec4(rgb, a) + offset` over all four channels. Written as a
/// dot product per output row, so no language's matrix-constructor convention is involved (the
/// transpose trap of OCIO-M6 cannot occur); a diagonal matrix scales, a unity one is skipped.
pub(super) fn matrix(d: &Dialect, m: &Mat4<RowMajor>, offset: [f32; 4]) -> Vec<String> {
    let rows = m.rows();
    let is_unity = (0..4).all(|r| (0..4).all(|c| rows[r][c] == if r == c { 1.0 } else { 0.0 }));
    let is_diag = (0..4).all(|r| (0..4).all(|c| r == c || rows[r][c] == 0.0));
    let mut out = vec!["{ // Matrix".to_string()];
    let px = format!("{}(color.rgb, color.a)", v4(d));
    let res = if is_unity {
        px
    } else if is_diag {
        format!(
            "{} * {px}",
            v4_lit(d, [rows[0][0], rows[1][1], rows[2][2], rows[3][3]])
        )
    } else {
        out.push(decl4(d, "px", &px));
        let dots: Vec<String> = rows
            .iter()
            .map(|r| format!("dot({}, px)", v4_lit(d, *r)))
            .collect();
        format!("{}({})", v4(d), dots.join(", "))
    };
    let res = if offset.iter().any(|&o| o != 0.0) {
        format!("{res} + {}", v4_lit(d, offset))
    } else {
        res
    };
    out.push(format!("    color = {res};"));
    out.push("}".into());
    out
}

/// OCIO `CDLOpGPU.cpp`, forward (the inverse runs as its op sequence, `GpuProcessor::gpu_sequence`):
/// slope, offset, then the power (clamped to [0, 1] for the ASC styles; NoClamp passes negatives
/// through), the saturation with Rec.709 luma weights, and the ASC post-clamp.
pub(super) fn cdl(
    d: &Dialect,
    slope: [f32; 3],
    offset: [f32; 3],
    power: [f32; 3],
    saturation: f32,
    style: CdlStyle,
) -> Vec<String> {
    let clamp = matches!(style, CdlStyle::AscCdl);
    let mut out = vec!["{ // CDL".to_string()];
    out.push(d.set(&format!("color.rgb * {} + {}", v3(d, slope), v3(d, offset))));
    if clamp {
        out.push(d.set(&format!("clamp(color.rgb, {}, {})", d.k(0.0), d.k(1.0))));
        out.push(d.set(&format!("pow(color.rgb, {})", v3(d, power))));
    } else {
        out.push(d.decl("posPix", &format!("step({}, color.rgb)", d.k(0.0))));
        out.push(d.decl(
            "pixPower",
            &format!("pow(abs(color.rgb), {})", v3(d, power)),
        ));
        out.push(d.set(&d.mix("color.rgb", "pixPower", "posPix")));
    }
    out.push(d.decl_f(
        "luma",
        &format!("dot(color.rgb, {})", v3(d, [0.2126, 0.7152, 0.0722])),
    ));
    out.push(d.set(&format!(
        "{} + {} * (color.rgb - {})",
        d.splat("luma"),
        nf(saturation),
        d.splat("luma")
    )));
    if clamp {
        out.push(d.set(&format!("clamp(color.rgb, {}, {})", d.k(0.0), d.k(1.0))));
    }
    out.push("}".into());
    out
}

/// OCIO `LogOpGPU.cpp` `AddLogShader` / `AddAntiLogShader`: forward `log2` for base 2, else
/// `log(x) / ln(base)`, over `max(x, FLT_MIN)`; inverse `pow(base, x)`.
pub(super) fn log(d: &Dialect, base: f32, forward: bool) -> Vec<String> {
    let body = if forward {
        let x = format!("max({}, color.rgb)", d.splat(&super::dialect::log_min()));
        if base == 2.0 {
            format!("log2({x})")
        } else {
            format!("log({x}) * {}", d.k(1.0 / (base as f64).ln()))
        }
    } else {
        format!("pow({}, color.rgb)", d.kf(base))
    };
    vec!["{ // Log".to_string(), d.set(&body), "}".into()]
}

/// OCIO `LogOpGPU.cpp` `AddLinToLogShader` / `AddLogToLinShader`: forward
/// `logSlope / ln(base) * log(max(FLT_MIN, x * linSlope + linOffset)) + logOffset`, inverse
/// `(pow(base, (x - logOffset) / logSlope) - linOffset) / linSlope`.
pub(super) fn log_affine(
    d: &Dialect,
    base: f32,
    log_slope: [f32; 3],
    log_offset: [f32; 3],
    lin_slope: [f32; 3],
    lin_offset: [f32; 3],
    forward: bool,
) -> Vec<String> {
    let ln_base = (base as f64).ln();
    let mut out = vec!["{ // LogAffine".to_string()];
    if forward {
        let slope = log_slope.map(|s| (s as f64 / ln_base) as f32);
        out.push(d.set(&format!(
            "max({}, color.rgb * {} + {})",
            d.splat(&super::dialect::log_min()),
            v3(d, lin_slope),
            v3(d, lin_offset)
        )));
        out.push(d.set(&format!(
            "{} * log(color.rgb) + {}",
            v3(d, slope),
            v3(d, log_offset)
        )));
    } else {
        out.push(d.set(&format!(
            "(color.rgb - {}) * {}",
            v3(d, log_offset),
            v3(d, log_slope.map(|s| 1.0 / s))
        )));
        out.push(d.set(&format!("pow({}, color.rgb)", d.kf(base))));
        out.push(d.set(&format!(
            "{} * (color.rgb - {})",
            v3(d, lin_slope.map(|s| 1.0 / s)),
            v3(d, lin_offset)
        )));
    }
    out.push("}".into());
    out
}

/// OCIO `RangeOpGPU.cpp`: `x * scale + offset`, then the lower and upper bounds that are set.
pub(super) fn range(
    d: &Dialect,
    scale: f32,
    offset: f32,
    min: Option<f32>,
    max: Option<f32>,
) -> Vec<String> {
    let mut out = vec!["{ // Range".to_string()];
    out.push(d.set(&format!("color.rgb * {} + {}", d.kf(scale), d.kf(offset))));
    if let Some(lo) = min {
        out.push(d.set(&format!("max({}, color.rgb)", d.kf(lo))));
    }
    if let Some(hi) = max {
        out.push(d.set(&format!("min({}, color.rgb)", d.kf(hi))));
    }
    out.push("}".into());
    out
}

/// OCIO `Lut1DOpGPU.cpp` for a lookup-domain table (the half-domain one is [`super::lut1d_half`]):
/// the input mapped through the table's domain (as `processor/apply.rs` does) and clamped to
/// [0, 1], moved onto the texel centres, and one sample per channel; DW3's hue adjust keeps the
/// middle channel's position between the smallest and largest one.
pub(super) fn lut1d(
    d: &Dialect,
    name: &str,
    kind: GpuTextureType,
    width: u32,
    domain_min: [f32; 3],
    domain_max: [f32; 3],
    hue_adjust: crate::transform::Lut1DHueAdjust,
) -> Vec<String> {
    let dw3 = matches!(hue_adjust, crate::transform::Lut1DHueAdjust::Dw3);
    let mut out = vec![format!("{{ // Lut1D {name}")];
    if dw3 {
        out.push(d.decl("maxval", "max(color.rgb, max(color.gbr, color.brg))"));
        out.push(d.decl("minval", "min(color.rgb, min(color.gbr, color.brg))"));
        out.push(d.decl_f("oldChroma", "max(1e-8, maxval.r - minval.r)"));
        out.push(d.decl("delta", "color.rgb - minval"));
    }
    out.push(d.var3(
        "t",
        &format!(
            "clamp({}, {}, {})",
            d.lut1d_domain_t(domain_min, domain_max),
            d.k(0.0),
            d.k(1.0)
        ),
    ));
    out.push(format!("    {}", d.lut1d_texel_center_remap(width as f32)));
    let s = |c: &str| format!("{}.{c}", d.sample(name, kind, &format!("t.{c}")));
    out.push(d.set(&format!("{}({}, {}, {})", d.v3, s("r"), s("g"), s("b"))));
    if dw3 {
        out.push(d.decl("maxval2", "max(color.rgb, max(color.gbr, color.brg))"));
        out.push(d.decl("minval2", "min(color.rgb, min(color.gbr, color.brg))"));
        out.push(d.decl_f("newChroma", "maxval2.r - minval2.r"));
        out.push(d.set(&format!(
            "{} + delta * newChroma / oldChroma",
            d.splat("minval2.r")
        )));
    }
    out.push("}".into());
    out
}

/// The fixed (non-dynamic) parameters of one ExposureContrast shader.
pub(super) struct EcShader {
    pub(super) style: ExposureContrastStyle,
    /// `false` emits the reverse shader.
    pub(super) forward: bool,
    pub(super) pivot: f32,
    pub(super) log_exposure_step: f32,
    pub(super) log_mid_gray: f32,
}

/// OCIO `ExposureContrastOpGPU.cpp`: exposure, contrast and gamma are dynamic (uniforms when
/// `names` has them, literals otherwise); linear and video styles scale and raise about the pivot
/// (video in the video-OETF domain), the logarithmic style offsets. `forward == false` is OCIO's
/// reverse shader (`AddECLinearRevShader`, `AddECVideoRevShader`, `AddECLogarithmicRevShader`),
/// fed the same FORWARD uniform values, so a host moving a uniform needs no inversion.
pub(super) fn exposure_contrast(
    d: &Dialect,
    values: [f32; 3],
    names: [&str; 3],
    shader: &EcShader,
) -> Vec<String> {
    let EcShader {
        style,
        forward,
        pivot,
        log_exposure_step,
        log_mid_gray,
    } = *shader;
    let exposure = d.uf(names[0], values[0] as f64);
    let contrast = d.uf(names[1], values[1] as f64);
    let gamma = d.uf(names[2], values[2] as f64);
    let mut out = vec!["{ // ExposureContrast".to_string()];
    let clamped = format!("max({}, {contrast} * {gamma})", n(MIN_CONTRAST as f64));
    match style {
        ExposureContrastStyle::Linear | ExposureContrastStyle::Video => {
            let (scale, pivot) = if style == ExposureContrastStyle::Linear {
                (format!("exp2({exposure})"), pivot as f64)
            } else {
                (
                    format!("pow(exp2({exposure}), {})", n(VIDEO_OETF_POWER as f64)),
                    (pivot as f64).powf(VIDEO_OETF_POWER as f64),
                )
            };
            // The reverse raises to 1 / contrast (OCIO: `contrast = 1. / max(..)`).
            out.push(d.decl_f(
                "ec_contrast",
                &if forward {
                    clamped
                } else {
                    format!("{} / {clamped}", n(1.0))
                },
            ));
            out.push(d.decl_f("ec_scale", &scale));
            // OCIO's shader scales first and raises about the pivot only when the contrast is
            // not exactly 1, so an exposure-only grade keeps negative values (the CPU does too).
            if forward {
                out.push(d.set("color.rgb * ec_scale"));
            }
            out.push(format!("if (ec_contrast != {}) {{", n(1.0)));
            out.push(d.set(&format!(
                "pow(max({}, color.rgb / {}), {}) * {}",
                d.k(0.0),
                n(pivot),
                d.splat("ec_contrast"),
                n(pivot)
            )));
            out.push("}".into());
            if !forward {
                out.push(d.set("color.rgb / ec_scale"));
            }
        }
        ExposureContrastStyle::Logarithmic => {
            let log_pivot = (((pivot / 0.18).max(0.0001) as f64).log2() * log_exposure_step as f64
                + log_mid_gray as f64)
                .max(0.0);
            out.push(d.decl_f("ec_contrast", &clamped));
            out.push(d.decl_f(
                "ec_offset",
                &if forward {
                    format!(
                        "({exposure} * {} - {}) * ec_contrast + {}",
                        nf(log_exposure_step),
                        n(log_pivot),
                        n(log_pivot)
                    )
                } else {
                    format!(
                        "{} - {} / ec_contrast - {exposure} * {}",
                        n(log_pivot),
                        n(log_pivot),
                        nf(log_exposure_step)
                    )
                },
            ));
            out.push(d.set(&if forward {
                format!("color.rgb * ec_contrast + {}", d.splat("ec_offset"))
            } else {
                format!("color.rgb / ec_contrast + {}", d.splat("ec_offset"))
            }));
        }
    }
    out.push("}".into());
    out
}
/// The Allocation transform: uniform `(x - min) / (max - min)`, or log2
/// `(log2(max(x + offset, FLT_MIN)) - min) / (max - min)`, and their inverses
/// (`processor/apply.rs`, OCIO `AllocationOp.cpp`'s ops).
pub(super) fn allocation(
    d: &Dialect,
    allocation: AllocationType,
    min: f32,
    max: f32,
    lin_offset: f32,
    forward: bool,
) -> Vec<String> {
    let range = nf(max - min);
    let body = match (allocation, forward) {
        (AllocationType::Uniform, true) => format!("(color.rgb - {}) / {range}", d.kf(min)),
        (AllocationType::Uniform, false) => format!("color.rgb * {range} + {}", d.kf(min)),
        (AllocationType::Log2, true) => format!(
            "(log2(max(color.rgb + {}, {})) - {}) / {range}",
            d.kf(lin_offset),
            d.splat(&super::dialect::log_min()),
            d.kf(min),
        ),
        (AllocationType::Log2, false) => {
            format!(
                "exp2(color.rgb * {range} + {}) - {}",
                d.kf(min),
                d.kf(lin_offset)
            )
        }
        (AllocationType::Unknown, _) => unreachable!("refused when the transform compiles"),
    };
    vec!["{ // Allocation".to_string(), d.set(&body), "}".into()]
}
