//! `Config` -> YAML: `Config::serialize`, as OCIO `OCIOYaml.cpp` `save(Config)`.
//!
//! Every section and every transform is written with OCIO's keys, in OCIO's order, omitting
//! exactly what OCIO omits (default values), so the text reads back here and in OCIO to the same
//! config, and parses to the same YAML tree as OCIO's own `serialize` (the gate compares the
//! trees; the text differs only in number spelling, quoting and flow/block layout). Values
//! OCIO's YAML cannot hold are refused rather than dropped: inline LUTs
//! (`Lut1DTransform`/`Lut3DTransform`), `BuiltinTransferTransform` (this port's own), and the
//! runtime-only LookTransform / DisplayViewTransform settings. An ExposureContrast value is written
//! unless it is dynamic ("Do not save dynamic values", `OCIOYaml.cpp:1298`), so a missing key
//! reads back dynamic and an explicit one, default or not, reads back static.

use super::*;
use crate::dynamic::{GradingPrimary, GradingRgbm};
use std::fmt::Write as _;

/// A string for a flow context (`{...}`, `[...]`): plain when YAML reads it back as that same
/// string, else double-quoted.
pub(super) fn flow_str(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let special_start = s.starts_with([
        '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@',
        '`',
    ]);
    let plain = !s.is_empty()
        && s == s.trim()
        && !special_start
        && !s.contains([',', '[', ']', '{', '}', '#', '\n', '\t', '"', '\\'])
        && !s.contains(": ")
        && !s.ends_with(':')
        && s.parse::<f64>().is_err()
        && !matches!(
            lower.as_str(),
            "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
        );
    if plain { s.to_string() } else { dq(s) }
}

/// A double-quoted YAML scalar.
fn dq(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A number that reads back to the same `f64` (`1.0`, `0.1`, `1e-7`, `1.7976931348623157e308`).
fn num(v: f64) -> String {
    format!("{v:?}")
}

fn nums(v: &[f64]) -> String {
    format!(
        "[{}]",
        v.iter().map(|&x| num(x)).collect::<Vec<_>>().join(", ")
    )
}

/// A value OCIO keeps as `float` (allocation vars, curve points), at `f32` precision.
fn nums_f32(v: impl IntoIterator<Item = f64>) -> String {
    format!(
        "[{}]",
        v.into_iter()
            .map(|x| format!("{:?}", x as f32))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn names(v: &[String]) -> String {
    format!(
        "[{}]",
        v.iter().map(|s| flow_str(s)).collect::<Vec<_>>().join(", ")
    )
}

/// `EmitBaseTransformKeyValues`.
fn push_direction(p: &mut Vec<(&'static str, String)>, direction: TransformDirection) {
    if direction == TransformDirection::Inverse {
        p.push(("direction", "inverse".into()));
    }
}

/// `EmitTransformName`: the FormatMetadata name first, when set (`emit`: OCIO writes it only
/// for v2 configs on some transforms).
fn named(
    mut p: Vec<(&'static str, String)>,
    name: &str,
    emit: bool,
) -> Vec<(&'static str, String)> {
    if emit && !name.is_empty() {
        p.insert(0, ("name", flow_str(name)));
    }
    p
}

/// A flow mapping under an OCIO tag: `!<Tag> {k: v, ...}` (direction already in `pairs`).
fn tagged(tag: &str, pairs: Vec<(&str, String)>) -> String {
    let body: Vec<String> = pairs
        .into_iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect();
    format!("!<{tag}> {{{}}}", body.join(", "))
}

/// `NegativeStyleToString`.
fn negative_style_name(s: NegativeStyle) -> &'static str {
    match s {
        NegativeStyle::Clamp => "clamp",
        NegativeStyle::Mirror => "mirror",
        NegativeStyle::PassThru => "pass_thru",
        NegativeStyle::Linear => "linear",
    }
}

/// `InterpolationToString`.
fn interpolation_name(i: Interpolation) -> &'static str {
    match i {
        Interpolation::Default => "default",
        Interpolation::Nearest => "nearest",
        Interpolation::Linear => "linear",
        Interpolation::Tetrahedral => "tetrahedral",
        Interpolation::Best => "best",
    }
}

/// `GradingStyleToString`.
fn grading_style_name(linear: bool, video: bool) -> &'static str {
    if linear {
        "linear"
    } else if video {
        "video"
    } else {
        "log"
    }
}

/// OCIO `FixedFunctionStyleToString` and whether the style is the inverse of that name.
fn fixed_function_name(s: FixedFunctionStyle) -> (&'static str, bool) {
    use FixedFunctionStyle as F;
    match s {
        F::AcesRedMod03 => ("ACES_RedMod03", false),
        F::AcesRedMod10 => ("ACES_RedMod10", false),
        F::AcesGlow03 => ("ACES_Glow03", false),
        F::AcesGlow10 => ("ACES_Glow10", false),
        F::AcesDarkToDim10 => ("ACES_DarkToDim10", false),
        F::AcesGamutComp13 => ("ACES_GamutComp13", false),
        F::Rec2100Surround => ("REC2100_Surround", false),
        F::RgbToHsv => ("RGB_TO_HSV", false),
        F::HsvToRgb => ("RGB_TO_HSV", true),
        F::XyzToXyy => ("XYZ_TO_xyY", false),
        F::XyyToXyz => ("XYZ_TO_xyY", true),
        F::XyzToUvy => ("XYZ_TO_uvY", false),
        F::UvyToXyz => ("XYZ_TO_uvY", true),
        F::XyzToLuv => ("XYZ_TO_LUV", false),
        F::LuvToXyz => ("XYZ_TO_LUV", true),
        F::LinToPq => ("Lin_TO_PQ", false),
        F::PqToLin => ("Lin_TO_PQ", true),
        F::LinToGammaLog => ("Lin_TO_GammaLog", false),
        F::GammaLogToLin => ("Lin_TO_GammaLog", true),
        F::LinToDoubleLog => ("Lin_TO_DoubleLog", false),
        F::DoubleLogToLin => ("Lin_TO_DoubleLog", true),
        F::AcesOutputTransform20 => ("ACES2_OutputTransform", false),
        F::AcesRgbToJmh20 => ("ACES2_RGB_TO_JMh", false),
        F::AcesJmhToRgb20 => ("ACES2_RGB_TO_JMh", true),
        F::AcesTonescaleCompress20 => ("ACES2_TonescaleCompress", false),
        F::AcesGamutCompress20 => ("ACES2_GamutCompress", false),
        F::RgbToHsyLin => ("RGB_TO_HSY_LIN", false),
        F::HsyLinToRgb => ("RGB_TO_HSY_LIN", true),
        F::RgbToHsyLog => ("RGB_TO_HSY_LOG", false),
        F::HsyLogToRgb => ("RGB_TO_HSY_LOG", true),
        F::RgbToHsyVid => ("RGB_TO_HSY_VID", false),
        F::HsyVidToRgb => ("RGB_TO_HSY_VID", true),
    }
}

/// `saveLogParam`: one value when the three are equal (omitted at `default`), else three.
fn push_log_param(
    p: &mut Vec<(&'static str, String)>,
    key: &'static str,
    v: [f64; 3],
    default: Option<f64>,
) {
    if v[0] == v[1] && v[0] == v[2] {
        if Some(v[0]) != default {
            p.push((key, num(v[0])));
        }
    } else {
        p.push((key, nums(&v)));
    }
}

/// A GradingRGBM `{rgb: [...], master: m}` when it differs from the default.
fn push_rgbm(
    p: &mut Vec<(&'static str, String)>,
    key: &'static str,
    v: &GradingRgbm,
    default: &GradingRgbm,
) {
    if v != default {
        p.push((
            key,
            format!(
                "{{rgb: {}, master: {}}}",
                nums(&[v.red, v.green, v.blue]),
                num(v.master)
            ),
        ));
    }
}

fn push_scalar(p: &mut Vec<(&'static str, String)>, key: &'static str, v: f64, default: f64) {
    if v != default {
        p.push((key, num(v)));
    }
}

/// A B-spline curve `{control_points: [...], slopes: [...]}` (float values).
fn curve_yaml(points: &[[f64; 2]], slopes: &[f64]) -> String {
    let mut s = format!(
        "{{control_points: {}",
        nums_f32(points.iter().flat_map(|p| [p[0], p[1]]))
    );
    if slopes.iter().any(|&v| v as f32 != 0.0) {
        let _ = write!(s, ", slopes: {}", nums_f32(slopes.iter().copied()));
    }
    s.push('}');
    s
}

/// Curves compare as OCIO stores them, in `float`.
fn same_curve(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x[0] as f32 == y[0] as f32 && x[1] as f32 == y[1] as f32)
}

impl Config {
    /// One transform as OCIO writes it (`OCIOYaml.cpp` `save(..., Const*TransformRcPtr)`).
    fn transform_yaml(&self, t: &Transform) -> OcioResult<String> {
        let refuse = |what: &str| {
            Err(OcioError::Validation(format!(
                "serialize: {what} has no OCIO YAML form"
            )))
        };
        let major2 = self.version != ConfigVersion::V1;
        Ok(match t {
            Transform::Matrix(m) => {
                let mut p = Vec::new();
                if m.matrix != MatrixTransform::IDENTITY {
                    p.push(("matrix", nums(&m.matrix)));
                }
                if m.offset != [0.0; 4] {
                    p.push(("offset", nums(&m.offset)));
                }
                push_direction(&mut p, m.direction);
                tagged("MatrixTransform", named(p, &m.name, major2))
            }
            Transform::FileTransform(f) => {
                // As the config wrote it (the loader keeps it unresolved).
                let mut p = vec![("src", flow_str(&f.src.to_string_lossy()))];
                if let Some(id) = f.ccc_id.as_deref().filter(|s| !s.is_empty()) {
                    p.push(("cccid", flow_str(id)));
                }
                if f.cdl_style != CdlStyle::NoClamp {
                    p.push(("cdl_style", "asc".into()));
                }
                let interp = match f.interpolation {
                    Interpolation::Default if !major2 => Interpolation::Linear,
                    i => i,
                };
                if interp != Interpolation::Default {
                    p.push(("interpolation", interpolation_name(interp).into()));
                }
                push_direction(&mut p, f.direction);
                tagged("FileTransform", p)
            }
            Transform::Exponent(e) => {
                let v = e.value;
                let mut p = vec![(
                    "value",
                    if major2 && v[0] == v[1] && v[0] == v[2] && v[3] == 1.0 {
                        num(v[0])
                    } else {
                        nums(&v)
                    },
                )];
                if e.negative_style != NegativeStyle::Clamp {
                    p.push(("style", negative_style_name(e.negative_style).into()));
                }
                push_direction(&mut p, e.direction);
                tagged("ExponentTransform", named(p, &e.name, major2))
            }
            Transform::ExponentWithLinear(e) => {
                let (g, o) = (e.gamma, e.offset);
                let mut p = vec![
                    (
                        "gamma",
                        if g[0] == g[1] && g[0] == g[2] && g[3] == 1.0 {
                            num(g[0])
                        } else {
                            nums(&g)
                        },
                    ),
                    (
                        "offset",
                        if o[0] == o[1] && o[0] == o[2] && o[3] == 0.0 {
                            num(o[0])
                        } else {
                            nums(&o)
                        },
                    ),
                ];
                if e.negative_style != NegativeStyle::Linear {
                    p.push(("style", negative_style_name(e.negative_style).into()));
                }
                push_direction(&mut p, e.direction);
                tagged("ExponentWithLinearTransform", named(p, &e.name, true))
            }
            Transform::Log(l) => {
                let mut p = Vec::new();
                if l.base != 2.0 || !major2 {
                    p.push(("base", num(l.base)));
                }
                push_direction(&mut p, l.direction);
                tagged("LogTransform", named(p, &l.name, major2))
            }
            Transform::LogAffine(l) => {
                let mut p = Vec::new();
                if l.base != 2.0 {
                    p.push(("base", num(l.base)));
                }
                push_log_param(&mut p, "log_side_slope", l.log_side_slope, Some(1.0));
                push_log_param(&mut p, "log_side_offset", l.log_side_offset, Some(0.0));
                push_log_param(&mut p, "lin_side_slope", l.lin_side_slope, Some(1.0));
                push_log_param(&mut p, "lin_side_offset", l.lin_side_offset, Some(0.0));
                push_direction(&mut p, l.direction);
                tagged("LogAffineTransform", named(p, &l.name, true))
            }
            Transform::LogCamera(l) => {
                let mut p = Vec::new();
                if l.base != 2.0 {
                    p.push(("base", num(l.base)));
                }
                push_log_param(&mut p, "log_side_slope", l.log_side_slope, Some(1.0));
                push_log_param(&mut p, "log_side_offset", l.log_side_offset, Some(0.0));
                push_log_param(&mut p, "lin_side_slope", l.lin_side_slope, Some(1.0));
                push_log_param(&mut p, "lin_side_offset", l.lin_side_offset, Some(0.0));
                push_log_param(&mut p, "lin_side_break", l.lin_side_break, None);
                if let Some(s) = l.linear_slope {
                    push_log_param(&mut p, "linear_slope", s, None);
                }
                push_direction(&mut p, l.direction);
                tagged("LogCameraTransform", named(p, &l.name, true))
            }
            Transform::Cdl(c) => {
                let mut p = Vec::new();
                if c.slope != [1.0; 3] {
                    p.push(("slope", nums(&c.slope)));
                }
                if c.offset != [0.0; 3] {
                    p.push(("offset", nums(&c.offset)));
                }
                if c.power != [1.0; 3] {
                    p.push(("power", nums(&c.power)));
                }
                push_scalar(&mut p, "sat", c.saturation, 1.0);
                if c.style != CdlStyle::NoClamp {
                    p.push(("style", "asc".into()));
                }
                push_direction(&mut p, c.direction);
                tagged("CDLTransform", named(p, &c.name, major2))
            }
            Transform::Range(r) => {
                let mut p = Vec::new();
                for (k, v) in [
                    ("min_in_value", r.min_in),
                    ("max_in_value", r.max_in),
                    ("min_out_value", r.min_out),
                    ("max_out_value", r.max_out),
                ] {
                    if let Some(v) = v {
                        p.push((k, num(v)));
                    }
                }
                if r.style == RangeStyle::NoClamp {
                    p.push(("style", "noClamp".into()));
                }
                push_direction(&mut p, r.direction);
                tagged("RangeTransform", named(p, &r.name, true))
            }
            Transform::FixedFunction(f) => {
                let (name, inverse) = fixed_function_name(f.style);
                let mut p = vec![("style", name.to_string())];
                if !f.params.is_empty() {
                    p.push(("params", nums(&f.params)));
                }
                push_direction(
                    &mut p,
                    if inverse {
                        f.direction.inverse()
                    } else {
                        f.direction
                    },
                );
                tagged("FixedFunctionTransform", named(p, &f.name, true))
            }
            Transform::ExposureContrast(e) => {
                let style = match e.style {
                    ExposureContrastStyle::Linear => "linear",
                    ExposureContrastStyle::Video => "video",
                    ExposureContrastStyle::Logarithmic => "log",
                };
                let mut p = vec![("style", style.to_string())];
                for (key, value, dynamic) in [
                    ("exposure", e.exposure, e.dynamic.exposure),
                    ("contrast", e.contrast, e.dynamic.contrast),
                    ("gamma", e.gamma, e.dynamic.gamma),
                ] {
                    if !dynamic {
                        p.push((key, num(value)));
                    }
                }
                p.push(("pivot", num(e.pivot)));
                push_scalar(&mut p, "log_exposure_step", e.log_exposure_step, 0.088);
                push_scalar(&mut p, "log_midway_gray", e.log_mid_gray, 0.435);
                push_direction(&mut p, e.direction);
                tagged("ExposureContrastTransform", named(p, &e.name, true))
            }
            Transform::ColorSpace(c) => {
                let mut p = vec![("src", flow_str(&c.src)), ("dst", flow_str(&c.dst))];
                if !c.data_bypass {
                    p.push(("data_bypass", "false".into()));
                }
                push_direction(&mut p, c.direction);
                tagged("ColorSpaceTransform", p)
            }
            Transform::Look(l) => {
                if l.skip_color_space_conversion {
                    return refuse(
                        "LookTransform with skip_color_space_conversion (a runtime setting)",
                    );
                }
                let mut p = vec![
                    ("src", flow_str(&l.src)),
                    ("dst", flow_str(&l.dst)),
                    ("looks", flow_str(&l.looks)),
                ];
                push_direction(&mut p, l.direction);
                tagged("LookTransform", p)
            }
            Transform::DisplayView(d) => {
                if d.looks_override_enabled {
                    return refuse(
                        "DisplayViewTransform with a looks override (a runtime setting)",
                    );
                }
                let mut p = vec![
                    ("src", flow_str(&d.src)),
                    ("display", flow_str(&d.display)),
                    ("view", flow_str(&d.view)),
                ];
                if d.looks_bypass {
                    p.push(("looks_bypass", "true".into()));
                }
                if !d.data_bypass {
                    p.push(("data_bypass", "false".into()));
                }
                push_direction(&mut p, d.direction);
                tagged("DisplayViewTransform", p)
            }
            Transform::Group(g) => {
                // OCIO writes the direction before the children.
                let mut p = Vec::new();
                push_direction(&mut p, g.direction);
                let children = g
                    .transforms
                    .iter()
                    .map(|c| self.transform_yaml(c))
                    .collect::<OcioResult<Vec<_>>>()?;
                p.push(("children", format!("[{}]", children.join(", "))));
                tagged("GroupTransform", named(p, &g.name, major2))
            }
            Transform::Builtin(b) => {
                let mut p = vec![("style", flow_str(&b.style))];
                push_direction(&mut p, b.direction);
                tagged("BuiltinTransform", p)
            }
            Transform::Allocation(a) => {
                let mut p = vec![("allocation", a.allocation.name().to_string())];
                if !a.vars.is_empty() {
                    p.push(("vars", nums_f32(a.vars.iter().copied())));
                }
                push_direction(&mut p, a.direction);
                tagged("AllocationTransform", p)
            }
            Transform::GradingPrimary(g) => {
                let v = &g.value;
                let d: GradingPrimary = GradingPrimaryTransform::new(g.style).value;
                let mut p = vec![(
                    "style",
                    grading_style_name(
                        g.style == GradingPrimaryStyle::Linear,
                        g.style == GradingPrimaryStyle::Video,
                    )
                    .to_string(),
                )];
                // `savePivot`: `contrast` when forced, black/white when not at their defaults.
                let pivot = |p: &mut Vec<(&'static str, String)>,
                             contrast: Option<f64>,
                             black: (f64, f64),
                             white: (f64, f64)| {
                    if contrast.is_some() || black.0 != black.1 || white.0 != white.1 {
                        let mut m = Vec::new();
                        if let Some(c) = contrast {
                            m.push(format!("contrast: {}", num(c)));
                        }
                        if black.0 != black.1 {
                            m.push(format!("black: {}", num(black.0)));
                        }
                        if white.0 != white.1 {
                            m.push(format!("white: {}", num(white.0)));
                        }
                        p.push(("pivot", format!("{{{}}}", m.join(", "))));
                    }
                };
                match g.style {
                    GradingPrimaryStyle::Log => {
                        push_rgbm(&mut p, "brightness", &v.brightness, &d.brightness);
                        push_rgbm(&mut p, "contrast", &v.contrast, &d.contrast);
                        push_rgbm(&mut p, "gamma", &v.gamma, &d.gamma);
                        push_scalar(&mut p, "saturation", v.saturation, d.saturation);
                        let force = v.contrast != d.contrast || v.pivot != d.pivot;
                        pivot(
                            &mut p,
                            force.then_some(v.pivot),
                            (v.pivot_black, d.pivot_black),
                            (v.pivot_white, d.pivot_white),
                        );
                    }
                    GradingPrimaryStyle::Linear => {
                        push_rgbm(&mut p, "offset", &v.offset, &d.offset);
                        push_rgbm(&mut p, "exposure", &v.exposure, &d.exposure);
                        push_rgbm(&mut p, "contrast", &v.contrast, &d.contrast);
                        push_scalar(&mut p, "saturation", v.saturation, d.saturation);
                        let force = v.contrast != d.contrast || v.pivot != d.pivot;
                        pivot(&mut p, force.then_some(v.pivot), (0.0, 0.0), (0.0, 0.0));
                    }
                    GradingPrimaryStyle::Video => {
                        push_rgbm(&mut p, "lift", &v.lift, &d.lift);
                        push_rgbm(&mut p, "gamma", &v.gamma, &d.gamma);
                        push_rgbm(&mut p, "gain", &v.gain, &d.gain);
                        push_rgbm(&mut p, "offset", &v.offset, &d.offset);
                        push_scalar(&mut p, "saturation", v.saturation, d.saturation);
                        pivot(
                            &mut p,
                            None,
                            (v.pivot_black, d.pivot_black),
                            (v.pivot_white, d.pivot_white),
                        );
                    }
                }
                if v.clamp_black != d.clamp_black || v.clamp_white != d.clamp_white {
                    let mut m = Vec::new();
                    if v.clamp_black != d.clamp_black {
                        m.push(format!("black: {}", num(v.clamp_black)));
                    }
                    if v.clamp_white != d.clamp_white {
                        m.push(format!("white: {}", num(v.clamp_white)));
                    }
                    p.push(("clamp", format!("{{{}}}", m.join(", "))));
                }
                push_direction(&mut p, g.direction);
                tagged("GradingPrimaryTransform", named(p, &g.name, true))
            }
            Transform::GradingRgbCurve(g) => {
                let d = GradingRgbCurveTransform::default_curve(g.style);
                let mut p = vec![(
                    "style",
                    grading_style_name(
                        g.style == GradingToneStyle::Linear,
                        g.style == GradingToneStyle::Video,
                    )
                    .to_string(),
                )];
                if g.bypass_lin_to_log {
                    p.push(("lintolog_bypass", "true".into()));
                }
                for (key, pts, slopes) in [
                    ("red", &g.red, &g.red_slopes),
                    ("green", &g.green, &g.green_slopes),
                    ("blue", &g.blue, &g.blue_slopes),
                    ("master", &g.master, &g.master_slopes),
                ] {
                    if !same_curve(pts, &d) || slopes.iter().any(|&s| s as f32 != 0.0) {
                        p.push((key, curve_yaml(pts, slopes)));
                    }
                }
                push_direction(&mut p, g.direction);
                tagged("GradingRGBCurveTransform", named(p, &g.name, true))
            }
            Transform::GradingHueCurve(g) => {
                let d = GradingHueCurveTransform::new(g.style);
                let mut p = vec![(
                    "style",
                    grading_style_name(
                        g.style == GradingHueCurveStyle::Linear,
                        g.style == GradingHueCurveStyle::Video,
                    )
                    .to_string(),
                )];
                if g.rgb_to_hsy == HsyTransformStyle::None {
                    p.push(("hsy_transform", "none".into()));
                }
                for (key, pts, slopes, dpts) in [
                    ("hue_hue", &g.hue_hue, &g.hue_hue_slopes, &d.hue_hue),
                    ("hue_sat", &g.hue_sat, &g.hue_sat_slopes, &d.hue_sat),
                    ("hue_lum", &g.hue_lum, &g.hue_lum_slopes, &d.hue_lum),
                    ("lum_sat", &g.lum_sat, &g.lum_sat_slopes, &d.lum_sat),
                    ("sat_sat", &g.sat_sat, &g.sat_sat_slopes, &d.sat_sat),
                    ("lum_lum", &g.lum_lum, &g.lum_lum_slopes, &d.lum_lum),
                    ("sat_lum", &g.sat_lum, &g.sat_lum_slopes, &d.sat_lum),
                    ("hue_fx", &g.hue_fx, &g.hue_fx_slopes, &d.hue_fx),
                ] {
                    if !same_curve(pts, dpts) || slopes.iter().any(|&s| s as f32 != 0.0) {
                        p.push((key, curve_yaml(pts, slopes)));
                    }
                }
                push_direction(&mut p, g.direction);
                tagged("GradingHueCurveTransform", named(p, &g.name, true))
            }
            Transform::GradingTone(g) => {
                let d = crate::grading_tone::GradingTone::from_style(g.style);
                let mut p = vec![(
                    "style",
                    grading_style_name(
                        g.style == GradingToneStyle::Linear,
                        g.style == GradingToneStyle::Video,
                    )
                    .to_string(),
                )];
                for (key, z, dz, center, pivot) in [
                    ("blacks", &g.blacks, &d.blacks, false, false),
                    ("shadows", &g.shadows, &d.shadows, false, true),
                    ("midtones", &g.midtones, &d.midtones, true, false),
                    ("highlights", &g.highlights, &d.highlights, false, true),
                    ("whites", &g.whites, &d.whites, false, false),
                ] {
                    if z != dz {
                        p.push((
                            key,
                            format!(
                                "{{rgb: {}, master: {}, {}: {}, {}: {}}}",
                                nums(&[z.red, z.green, z.blue]),
                                num(z.master),
                                if center { "center" } else { "start" },
                                num(z.start),
                                if pivot { "pivot" } else { "width" },
                                num(z.width)
                            ),
                        ));
                    }
                }
                push_scalar(&mut p, "s_contrast", g.scontrast, 1.0);
                push_direction(&mut p, g.direction);
                tagged("GradingToneTransform", named(p, &g.name, true))
            }
            Transform::BuiltinTransfer(_) => {
                return refuse("BuiltinTransferTransform (not an OCIO transform)");
            }
            Transform::Lut1D(_) | Transform::Lut3D(_) => {
                return refuse("an inline LUT (OCIO's YAML names LUT files through FileTransform)");
            }
        })
    }

    /// `key: <transform>` at `indent`, when the slot is set.
    fn write_slot(
        &self,
        out: &mut String,
        indent: &str,
        key: &str,
        t: Option<&Transform>,
    ) -> OcioResult<()> {
        if let Some(t) = t {
            let _ = writeln!(out, "{indent}{key}: {}", self.transform_yaml(t)?);
        }
        Ok(())
    }

    /// `saveInterchangeAttributes` (OCIO keeps them in a sorted map).
    fn write_interchange<'a>(
        out: &mut String,
        indent: &str,
        attrs: impl Iterator<Item = (&'a str, &'a str)>,
    ) {
        let mut attrs: Vec<_> = attrs.collect();
        if attrs.is_empty() {
            return;
        }
        attrs.sort();
        let _ = writeln!(out, "{indent}interchange:");
        for (k, v) in attrs {
            let _ = writeln!(
                out,
                "{indent}  {}: {}",
                flow_str(k),
                dq(sanitize_description(v).as_str())
            );
        }
    }

    /// `save(YAML::Emitter&, const View&)`.
    fn view_yaml(
        name: &str,
        view_transform: Option<&str>,
        colorspace: &str,
        looks: Option<&str>,
        rule: Option<&str>,
        description: &str,
    ) -> String {
        let mut p = vec![format!("name: {}", flow_str(name))];
        match view_transform.filter(|vt| !vt.is_empty()) {
            Some(vt) => {
                p.push(format!("view_transform: {}", flow_str(vt)));
                p.push(format!("display_colorspace: {}", flow_str(colorspace)));
            }
            None => p.push(format!("colorspace: {}", flow_str(colorspace))),
        }
        if let Some(looks) = looks.filter(|l| !l.is_empty()) {
            p.push(format!("looks: {}", flow_str(looks)));
        }
        if let Some(rule) = rule.filter(|r| !r.is_empty()) {
            p.push(format!("rule: {}", flow_str(rule)));
        }
        let description = sanitize_description(description);
        if !description.is_empty() {
            p.push(format!("description: {}", flow_str(&description)));
        }
        format!("!<View> {{{}}}", p.join(", "))
    }

    fn write_view(out: &mut String, indent: &str, view: &View) {
        let cs = if view.use_display_name() {
            "<USE_DISPLAY_NAME>"
        } else {
            view.colorspace()
        };
        let _ = writeln!(
            out,
            "{indent}- {}",
            Self::view_yaml(
                view.name(),
                view.view_transform(),
                cs,
                view.looks(),
                view.rule(),
                view.description()
            )
        );
    }

    /// `save(YAML::Emitter&, ConstColorSpaceRcPtr, majorVersion)`.
    fn write_colorspace(&self, out: &mut String, cs: &ColorSpace) -> OcioResult<()> {
        let major2 = self.version != ConfigVersion::V1;
        out.push_str("  - !<ColorSpace>\n");
        let _ = writeln!(out, "    name: {}", flow_str(cs.name()));
        if major2 && !cs.aliases().is_empty() {
            let _ = writeln!(out, "    aliases: {}", names(cs.aliases()));
        }
        if !cs.interop_id().is_empty() {
            let _ = writeln!(out, "    interop_id: {}", flow_str(cs.interop_id()));
        }
        let _ = writeln!(out, "    family: {}", dq_if_empty(cs.family_name()));
        let _ = writeln!(
            out,
            "    equalitygroup: {}",
            dq_if_empty(cs.equality_group())
        );
        let _ = writeln!(
            out,
            "    bitdepth: {}",
            bit_depth_to_ocio_str(cs.bit_depth())
        );
        write_description(out, "    ", cs.description());
        let _ = writeln!(out, "    isdata: {}", cs.is_data());
        if !cs.categories().is_empty() {
            let _ = writeln!(out, "    categories: {}", names(cs.categories()));
        }
        let encoding = if cs.encoding_name().is_empty() {
            cs.encoding().as_str()
        } else {
            cs.encoding_name()
        };
        if !encoding.is_empty() {
            let _ = writeln!(out, "    encoding: {}", flow_str(encoding));
        }
        Self::write_interchange(
            out,
            "    ",
            cs.interchange_attribute_names()
                .filter_map(|k| cs.interchange_attribute(k).map(|v| (k, v))),
        );
        let alloc = cs.allocation();
        let _ = writeln!(out, "    allocation: {}", alloc.alloc_type.name());
        if !alloc.vars.is_empty() {
            let _ = writeln!(
                out,
                "    allocationvars: {}",
                nums_f32(alloc.vars.iter().map(|&v| v as f64))
            );
        }
        if cs.reference_space_type() == ReferenceSpaceType::Display {
            let to = cs.to_display_reference().or(cs.to_reference());
            let from = cs.from_display_reference().or(cs.from_reference());
            self.write_slot(out, "    ", "to_display_reference", to)?;
            self.write_slot(out, "    ", "from_display_reference", from)?;
        } else {
            let (to_key, from_key) = if major2 {
                ("to_scene_reference", "from_scene_reference")
            } else {
                ("to_reference", "from_reference")
            };
            self.write_slot(out, "    ", to_key, cs.to_reference())?;
            self.write_slot(out, "    ", from_key, cs.from_reference())?;
        }
        out.push('\n');
        Ok(())
    }

    /// Serializes the config to a YAML string (OCIO `Config::serialize`).
    ///
    /// Sections, keys and omitted defaults as OCIO's writer; the result reads back (here and in
    /// OCIO) to the same config. A transform with no OCIO YAML form (see the module doc) is an
    /// error rather than a silently dropped slot.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use vfx_ocio::Config;
    ///
    /// let config = Config::create_raw();
    /// let yaml = config.serialize()?;
    /// println!("{}", yaml);
    /// # Ok(())
    /// # }
    /// ```
    pub fn serialize(&self) -> OcioResult<String> {
        let major2 = self.version != ConfigVersion::V1;
        let mut out = String::new();

        let version = match self.version {
            ConfigVersion::V1 => "1".to_string(),
            ConfigVersion::V2 if self.version_minor == 0 => "2".to_string(),
            ConfigVersion::V2 => format!("2.{}", self.version_minor),
        };
        let _ = writeln!(out, "ocio_profile_version: {version}\n");

        // v2 always writes the environment section, even empty.
        if major2 || !self.environment_vars.is_empty() {
            if self.environment_vars.is_empty() {
                out.push_str("environment:\n  {}\n");
            } else {
                out.push_str("environment:\n");
                for (name, value) in &self.environment_vars {
                    let _ = writeln!(out, "  {}: {}", flow_str(name), flow_str(value));
                }
            }
        }
        // The loader keeps resolved search paths; those under the working directory are written
        // relative to it again.
        let paths: Vec<String> = self
            .search_paths
            .iter()
            .map(|p| {
                p.strip_prefix(&self.working_dir)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        if !major2 {
            let _ = writeln!(out, "search_path: {}", dq_if_empty(&paths.join(":")));
        } else {
            match paths.len() {
                0 => out.push_str("search_path: \"\"\n"),
                1 => {
                    let _ = writeln!(out, "search_path: {}", flow_str(&paths[0]));
                }
                _ => {
                    let _ = writeln!(out, "search_path: {}", names(&paths));
                }
            }
        }
        let _ = writeln!(out, "strictparsing: {}", self.strict_parsing);
        if major2 && self.family_separator != "/" {
            let _ = writeln!(
                out,
                "family_separator: {}",
                flow_str(&self.family_separator)
            );
        }
        let luma: Vec<f64> = self.default_luma_coefs.iter().map(|&v| v as f64).collect();
        let _ = writeln!(out, "luma: {}", nums_f32(luma));
        if major2 && !self.name.is_empty() {
            let _ = writeln!(out, "name: {}", flow_str(&self.name));
        }
        write_description(&mut out, "", &self.description);
        out.push('\n');

        // OCIO keeps roles sorted by name.
        let mut roles: Vec<(&str, &str)> = self.roles.iter().collect();
        roles.sort_by_key(|(r, _)| r.to_lowercase());
        if roles.is_empty() {
            out.push_str("roles:\n  {}\n\n");
        } else {
            out.push_str("roles:\n");
            for (role, cs) in roles {
                let _ = writeln!(out, "  {}: {}", flow_str(role), flow_str(cs));
            }
            out.push('\n');
        }

        if major2 {
            out.push_str("file_rules:\n");
            if self.file_rules.is_empty() {
                // OCIO's rules of a config without any: the default rule on the default role.
                out.push_str("  - !<Rule> {name: Default, colorspace: default}\n");
            }
            for rule in &self.file_rules {
                let mut p = vec![format!("name: {}", flow_str(&rule.name))];
                if !rule.colorspace.is_empty() {
                    p.push(format!("colorspace: {}", flow_str(&rule.colorspace)));
                }
                match &rule.kind {
                    FileRuleKind::Default | FileRuleKind::PathSearch => {}
                    FileRuleKind::Regex { regex } => {
                        // The loader anchors the regex as `^(?:...)$` for a full match.
                        let r = regex.as_str();
                        let r = r
                            .strip_prefix("^(?:")
                            .and_then(|r| r.strip_suffix(")$"))
                            .unwrap_or(r);
                        p.push(format!("regex: {}", flow_str(r)));
                    }
                    FileRuleKind::Basic { pattern, extension } => {
                        if !pattern.is_empty() {
                            p.push(format!("pattern: {}", flow_str(pattern)));
                        }
                        if let Some(ext) = extension.as_deref().filter(|e| !e.is_empty()) {
                            p.push(format!("extension: {}", flow_str(ext)));
                        }
                    }
                }
                if !rule.custom_keys.is_empty() {
                    p.push(format!("custom: {}", custom_map(&rule.custom_keys)));
                }
                let _ = writeln!(out, "  - !<Rule> {{{}}}", p.join(", "));
            }
            out.push('\n');
        }

        if major2 && !self.viewing_rules.is_empty() {
            out.push_str("viewing_rules:\n");
            for rule in &self.viewing_rules {
                let mut p = vec![format!("name: {}", flow_str(&rule.name))];
                let one_or_list = |v: &[String]| {
                    if v.len() == 1 {
                        flow_str(&v[0])
                    } else {
                        names(v)
                    }
                };
                if !rule.colorspaces.is_empty() {
                    p.push(format!("colorspaces: {}", one_or_list(&rule.colorspaces)));
                }
                if !rule.encodings.is_empty() {
                    p.push(format!("encodings: {}", one_or_list(&rule.encodings)));
                }
                if !rule.custom_keys.is_empty() {
                    p.push(format!("custom: {}", custom_map(&rule.custom_keys)));
                }
                let _ = writeln!(out, "  - !<Rule> {{{}}}", p.join(", "));
            }
            out.push('\n');
        }

        if !self.shared_views.is_empty() {
            out.push_str("shared_views:\n");
            for view in &self.shared_views {
                Self::write_view(&mut out, "  ", view);
            }
            out.push('\n');
        }

        let displays: Vec<_> = self
            .displays
            .displays()
            .iter()
            .filter(|d| !d.is_temporary())
            .collect();
        if displays.is_empty() {
            out.push_str("displays:\n  {}\n");
        } else {
            out.push_str("displays:\n");
            for display in displays {
                let _ = writeln!(out, "  {}:", flow_str(display.name()));
                for view in display.views() {
                    Self::write_view(&mut out, "    ", view);
                }
                if !display.shared_view_refs().is_empty() {
                    let _ = writeln!(out, "    - !<Views> {}", names(display.shared_view_refs()));
                }
            }
        }
        out.push('\n');

        if major2
            && let Some(vd) = self
                .virtual_display
                .as_ref()
                .filter(|vd| !vd.views.is_empty() || !vd.shared_view_refs.is_empty())
        {
            out.push_str("virtual_display:\n");
            for v in &vd.views {
                let _ = writeln!(
                    out,
                    "  - {}",
                    Self::view_yaml(
                        &v.name,
                        v.view_transform.as_deref(),
                        &v.display_colorspace,
                        v.looks.as_deref(),
                        v.rule.as_deref(),
                        v.description.as_deref().unwrap_or("")
                    )
                );
            }
            if !vd.shared_view_refs.is_empty() {
                let _ = writeln!(out, "  - !<Views> {}", names(&vd.shared_view_refs));
            }
            out.push('\n');
        }

        let _ = writeln!(out, "active_displays: {}", names(&self.active_displays));
        let _ = writeln!(out, "active_views: {}", names(&self.active_views));
        if !self.inactive_colorspaces.is_empty() {
            let _ = writeln!(
                out,
                "inactive_colorspaces: {}",
                names(&self.inactive_colorspaces)
            );
        }
        out.push('\n');

        if !self.looks.all().is_empty() {
            out.push_str("looks:\n");
            for look in self.looks.all() {
                out.push_str("  - !<Look>\n");
                let _ = writeln!(out, "    name: {}", flow_str(look.name()));
                let _ = writeln!(
                    out,
                    "    process_space: {}",
                    dq_if_empty(look.get_process_space().unwrap_or(""))
                );
                write_description(&mut out, "    ", look.get_description());
                Self::write_interchange(
                    &mut out,
                    "    ",
                    look.interchange_attribute_names()
                        .filter_map(|k| look.interchange_attribute(k).map(|v| (k, v))),
                );
                self.write_slot(&mut out, "    ", "transform", look.get_transform())?;
                self.write_slot(
                    &mut out,
                    "    ",
                    "inverse_transform",
                    look.get_inverse_transform(),
                )?;
                out.push('\n');
            }
        }

        if let Some(vt) = self
            .default_view_transform
            .as_deref()
            .filter(|v| !v.is_empty())
        {
            let _ = writeln!(out, "default_view_transform: {}\n", flow_str(vt));
        }

        if !self.view_transforms().is_empty() {
            out.push_str("view_transforms:\n");
            for vt in self.view_transforms() {
                out.push_str("  - !<ViewTransform>\n");
                let _ = writeln!(out, "    name: {}", flow_str(vt.name()));
                if !vt.family().is_empty() {
                    let _ = writeln!(out, "    family: {}", flow_str(vt.family()));
                }
                write_description(&mut out, "    ", vt.description());
                Self::write_interchange(
                    &mut out,
                    "    ",
                    vt.interchange_attribute_names()
                        .filter_map(|k| vt.interchange_attribute(k).map(|v| (k, v))),
                );
                if !vt.categories().is_empty() {
                    let _ = writeln!(out, "    categories: {}", names(vt.categories()));
                }
                if vt.reference_space_type() == ReferenceSpaceType::Display {
                    let to = vt.to_display_reference().or(vt.to_scene_reference());
                    let from = vt.from_display_reference().or(vt.from_scene_reference());
                    self.write_slot(&mut out, "    ", "to_display_reference", to)?;
                    self.write_slot(&mut out, "    ", "from_display_reference", from)?;
                } else {
                    let to = vt.to_scene_reference().or(vt.to_display_reference());
                    let from = vt.from_scene_reference().or(vt.from_display_reference());
                    self.write_slot(&mut out, "    ", "to_scene_reference", to)?;
                    self.write_slot(&mut out, "    ", "from_scene_reference", from)?;
                }
                out.push('\n');
            }
        }

        let (display_cs, scene_cs): (Vec<&ColorSpace>, Vec<&ColorSpace>) = self
            .colorspaces
            .iter()
            .partition(|cs| cs.reference_space_type() == ReferenceSpaceType::Display);
        if !display_cs.is_empty() {
            out.push_str("display_colorspaces:\n");
            for cs in display_cs {
                self.write_colorspace(&mut out, cs)?;
            }
        }
        out.push_str("colorspaces:\n");
        for cs in scene_cs {
            self.write_colorspace(&mut out, cs)?;
        }

        if !self.named_transforms.is_empty() {
            out.push_str("named_transforms:\n");
            for nt in &self.named_transforms {
                out.push_str("  - !<NamedTransform>\n");
                let _ = writeln!(out, "    name: {}", flow_str(&nt.name));
                if major2 && !nt.aliases.is_empty() {
                    let _ = writeln!(out, "    aliases: {}", names(&nt.aliases));
                }
                if let Some(desc) = &nt.description {
                    write_description(&mut out, "    ", desc);
                }
                if let Some(family) = nt.family.as_deref().filter(|f| !f.is_empty()) {
                    let _ = writeln!(out, "    family: {}", flow_str(family));
                }
                if !nt.categories.is_empty() {
                    let _ = writeln!(out, "    categories: {}", names(&nt.categories));
                }
                if let Some(enc) = nt.encoding.as_deref().filter(|e| !e.is_empty()) {
                    let _ = writeln!(out, "    encoding: {}", flow_str(enc));
                }
                self.write_slot(&mut out, "    ", "transform", nt.forward.as_ref())?;
                self.write_slot(&mut out, "    ", "inverse_transform", nt.inverse.as_ref())?;
                out.push('\n');
            }
        }

        Ok(out)
    }
}

/// A scalar that may be empty (`""`).
fn dq_if_empty(s: &str) -> String {
    if s.is_empty() {
        "\"\"".into()
    } else {
        flow_str(s)
    }
}

/// A rule's `custom: {key: value, ...}` map.
fn custom_map(keys: &[(String, String)]) -> String {
    let body: Vec<String> = keys
        .iter()
        .map(|(k, v)| format!("{}: {}", flow_str(k), flow_str(v)))
        .collect();
    format!("{{{}}}", body.join(", "))
}
