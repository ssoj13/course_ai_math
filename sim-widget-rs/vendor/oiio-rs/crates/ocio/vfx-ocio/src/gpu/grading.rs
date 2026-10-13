//! The grading ops' GPU bodies and helpers, written once for every language (ROADMAP G6).
//!
//! Ports of OCIO `GradingRGBCurveOpGPU.cpp`, `GradingHueCurveOpGPU.cpp` and the B-spline curve
//! evaluators of `GradingBSplineCurve.cpp` (`AddShaderEvalFwd/Rev/RevHue`), over [`Dialect`].
//! As in OCIO, every curve op gets its own evaluators (`evalBSplineCurve_<op>`, ...), reading that
//! op's knot and coefficient arrays directly ([`Dialect::arr_at`]); the lin <-> log conversions are
//! OCIO's `AddLinToLogShader` / `AddLogToLinShader` (`GpuShaderUtils.cpp`).

use super::GpuOp;
use super::dialect::{Dialect, Ty};
use super::types::OpUniforms;
use crate::transform::{FixedFunctionStyle, GradingToneStyle, HsyTransformStyle};

/// The four arrays and the bypass flag of one curve op.
pub(super) struct CurveNames<'a> {
    pub knots_offsets: &'a str,
    pub knots: &'a str,
    pub coefs_offsets: &'a str,
    pub coefs: &'a str,
    pub local_bypass: &'a str,
}

impl<'a> CurveNames<'a> {
    /// The names a curve op's uniforms were registered under.
    pub(super) fn of(uniforms: Option<&'a OpUniforms>) -> Option<Self> {
        match uniforms {
            Some(OpUniforms::GradingRgbCurve {
                knots_offsets,
                knots,
                coefs_offsets,
                coefs,
                local_bypass,
            })
            | Some(OpUniforms::GradingHueCurve {
                knots_offsets,
                knots,
                coefs_offsets,
                coefs,
                local_bypass,
            }) => Some(Self {
                knots_offsets,
                knots,
                coefs_offsets,
                coefs,
                local_bypass,
            }),
            _ => None,
        }
    }
}

/// The evaluators' names for op `idx`.
fn eval_fn(idx: usize) -> String {
    format!("evalBSplineCurve_{idx}")
}
fn eval_rev_fn(idx: usize) -> String {
    format!("evalBSplineCurveRev_{idx}")
}
fn eval_rev_hue_fn(idx: usize) -> String {
    format!("evalBSplineCurveRevHue_{idx}")
}

/// The helper functions op `idx` needs (declared before the transform function): its curve
/// evaluators for a curve op, nothing otherwise.
pub(super) fn helpers(
    d: &Dialect,
    idx: usize,
    op: &GpuOp,
    uniforms: Option<&OpUniforms>,
) -> Vec<String> {
    let Some(n) = CurveNames::of(uniforms) else {
        return vec![];
    };
    let (forward, hue) = match op {
        GpuOp::GradingRgbCurve { forward, .. } => (*forward, false),
        GpuOp::GradingHueCurve { forward, .. } => (*forward, true),
        _ => return vec![],
    };
    let mut out = Vec::new();
    // The forward evaluator is used in both directions (the inverse evaluates gains with it).
    out.push(d.fn_begin(
        Ty::F,
        &eval_fn(idx),
        &[(Ty::I, "curveIdx"), (Ty::F, "x"), (Ty::F, "identity_x")],
    ));
    out.extend(indent(eval_fwd_body(d, &n)));
    out.push("}".into());
    if !forward {
        out.push(d.fn_begin(
            Ty::F,
            &eval_rev_fn(idx),
            &[(Ty::I, "curveIdx"), (Ty::F, "x")],
        ));
        out.extend(indent(eval_rev_body(d, &n, false)));
        out.push("}".into());
        if hue {
            out.push(d.fn_begin(
                Ty::F,
                &eval_rev_hue_fn(idx),
                &[(Ty::I, "curveIdx"), (Ty::F, "x")],
            ));
            out.extend(indent(eval_rev_body(d, &n, true)));
            out.push("}".into());
        }
    }
    out
}

fn indent(lines: Vec<String>) -> Vec<String> {
    lines.into_iter().map(|l| format!("    {l}")).collect()
}

/// The shared prologue of the three evaluators: the curve's offsets, counts and coefficient sets.
fn eval_prologue(d: &Dialect, n: &CurveNames, identity: &str) -> Vec<String> {
    let ko = |i: &str| d.arr_at(n.knots_offsets, true, i);
    let co = |i: &str| d.arr_at(n.coefs_offsets, true, i);
    vec![
        d.let_(Ty::I, "knotsOffs", &ko("curveIdx * 2")),
        d.let_(Ty::I, "knotsCnt", &ko("curveIdx * 2 + 1")),
        d.let_(Ty::I, "coefsOffs", &co("curveIdx * 2")),
        d.let_(Ty::I, "coefsCnt", &co("curveIdx * 2 + 1")),
        d.let_(Ty::I, "coefsSets", "coefsCnt / 3"),
        // Identity curve: no coefficients.
        format!("if (coefsSets == 0) {{ return {identity}; }}"),
        d.let_(Ty::F, "knStart", &d.arr_at(n.knots, false, "knotsOffs")),
        d.let_(
            Ty::F,
            "knEnd",
            &d.arr_at(n.knots, false, "knotsOffs + knotsCnt - 1"),
        ),
    ]
}

/// `AddShaderEvalFwd`: linear extrapolation outside the knots, the segment's quadratic inside.
fn eval_fwd_body(d: &Dialect, n: &CurveNames) -> Vec<String> {
    let k = |i: &str| d.arr_at(n.knots, false, i);
    let c = |i: &str| d.arr_at(n.coefs, false, i);
    let mut out = eval_prologue(d, n, "identity_x");
    out.extend([
        "if (x <= knStart) {".to_string(),
        format!("    {}", d.let_(Ty::F, "B", &c("coefsOffs + coefsSets"))),
        format!(
            "    {}",
            d.let_(Ty::F, "C", &c("coefsOffs + coefsSets * 2"))
        ),
        "    return (x - knStart) * B + C;".into(),
        "} else if (x >= knEnd) {".into(),
        format!(
            "    {}",
            d.let_(Ty::F, "A", &c("coefsOffs + coefsSets - 1"))
        ),
        format!(
            "    {}",
            d.let_(Ty::F, "B", &c("coefsOffs + coefsSets * 2 - 1"))
        ),
        format!(
            "    {}",
            d.let_(Ty::F, "C", &c("coefsOffs + coefsSets * 3 - 1"))
        ),
        format!(
            "    {}",
            d.let_(Ty::F, "kn", &k("knotsOffs + knotsCnt - 2"))
        ),
        format!("    {}", d.let_(Ty::F, "t", "knEnd - kn")),
        format!("    {}", d.let_(Ty::F, "slope", "2.0 * A * t + B")),
        format!("    {}", d.let_(Ty::F, "offs", "(A * t + B) * t + C")),
        "    return (x - knEnd) * slope + offs;".into(),
        "}".into(),
        d.var_(Ty::I, "i", "0"),
    ]);
    out.extend(d.find_index(
        "i",
        "knotsCnt - 2",
        &[],
        &format!("x < {}", k("knotsOffs + i + 1")),
    ));
    out.extend([
        d.let_(Ty::F, "A", &c("coefsOffs + i")),
        d.let_(Ty::F, "B", &c("coefsOffs + coefsSets + i")),
        d.let_(Ty::F, "C", &c("coefsOffs + coefsSets * 2 + i")),
        d.let_(Ty::F, "kn", &k("knotsOffs + i")),
        d.let_(Ty::F, "t", "x - kn"),
        "return (A * t + B) * t + C;".into(),
    ]);
    out
}

/// `AddShaderEvalRev` / `AddShaderEvalRevHue`: the inverse, solving the segment's quadratic.
/// The hue variant wraps `x` into the curve's range and handles the HUE-FX curve (index 7), whose
/// coefficients are stored relative to the identity.
fn eval_rev_body(d: &Dialect, n: &CurveNames, hue: bool) -> Vec<String> {
    let k = |i: &str| d.arr_at(n.knots, false, i);
    let c = |i: &str| d.arr_at(n.coefs, false, i);
    let mut out = eval_prologue(d, n, "x");
    out.push(d.let_(Ty::F, "knStartY", &c("coefsOffs + coefsSets * 2")));
    out.push(d.var_uninit(Ty::F, "knEndY"));
    out.push("{".into());
    out.extend(
        [
            d.let_(Ty::F, "A", &c("coefsOffs + coefsSets - 1")),
            d.let_(Ty::F, "B", &c("coefsOffs + coefsSets * 2 - 1")),
            d.let_(Ty::F, "C", &c("coefsOffs + coefsSets * 3 - 1")),
            d.let_(Ty::F, "kn", &k("knotsOffs + knotsCnt - 2")),
            d.let_(Ty::F, "t", "knEnd - kn"),
            "knEndY = (A * t + B) * t + C;".into(),
        ]
        .map(|l| format!("    {l}")),
    );
    if hue {
        out.push(format!(
            "    knEndY = {};",
            d.sel("curveIdx == 7", "knEndY + knEnd", "knEndY")
        ));
    }
    out.push("}".into());
    let x = if hue {
        // The argument is not assignable in every language: work on a copy.
        out.push(d.var_(Ty::F, "xw", "x"));
        out.push("if (xw < knStartY) {".into());
        out.push("    xw = xw + ceil(knStartY - xw);".into());
        out.push("} else if (xw > knEndY) {".into());
        out.push("    xw = xw - ceil(xw - knEndY);".into());
        out.push("}".into());
        "xw"
    } else {
        out.extend([
            "if (x <= knStartY) {".to_string(),
            format!("    {}", d.let_(Ty::F, "B", &c("coefsOffs + coefsSets"))),
            format!(
                "    {}",
                d.let_(Ty::F, "C", &c("coefsOffs + coefsSets * 2"))
            ),
            format!(
                "    return {};",
                d.sel("abs(B) < 1e-5", "knStart", "(x - C) / B + knStart")
            ),
            "} else if (x >= knEndY) {".into(),
            format!(
                "    {}",
                d.let_(Ty::F, "A", &c("coefsOffs + coefsSets - 1"))
            ),
            format!(
                "    {}",
                d.let_(Ty::F, "B", &c("coefsOffs + coefsSets * 2 - 1"))
            ),
            format!(
                "    {}",
                d.let_(Ty::F, "C", &c("coefsOffs + coefsSets * 3 - 1"))
            ),
            format!(
                "    {}",
                d.let_(Ty::F, "kn", &k("knotsOffs + knotsCnt - 2"))
            ),
            format!("    {}", d.let_(Ty::F, "t", "knEnd - kn")),
            format!("    {}", d.let_(Ty::F, "slope", "2.0 * A * t + B")),
            format!("    {}", d.let_(Ty::F, "offs", "(A * t + B) * t + C")),
            format!(
                "    return {};",
                d.sel("abs(slope) < 1e-5", "knEnd", "(x - offs) / slope + knEnd")
            ),
            "}".into(),
        ]);
        "x"
    };
    out.push(d.var_(Ty::I, "i", "0"));
    if hue {
        let pre = [
            d.var_(Ty::F, "curve_x", &c("coefsOffs + coefsSets * 2 + i + 1")),
            format!(
                "curve_x = {};",
                d.sel(
                    "curveIdx == 7",
                    &format!("curve_x + {}", k("knotsOffs + i + 1")),
                    "curve_x"
                )
            ),
        ];
        out.extend(d.find_index("i", "knotsCnt - 2", &pre, &format!("{x} < curve_x")));
    } else {
        out.extend(d.find_index(
            "i",
            "knotsCnt - 2",
            &[],
            &format!("{x} < {}", c("coefsOffs + coefsSets * 2 + i + 1")),
        ));
    }
    out.extend([
        d.let_(Ty::F, "A", &c("coefsOffs + i")),
        d.var_(Ty::F, "B", &c("coefsOffs + coefsSets + i")),
        d.var_(Ty::F, "C", &c("coefsOffs + coefsSets * 2 + i")),
        d.let_(Ty::F, "kn", &k("knotsOffs + i")),
    ]);
    if hue {
        out.extend([
            "if (curveIdx == 7) {".to_string(),
            "    C = C + kn;".into(),
            "    B = B + 1.0;".into(),
            "}".into(),
        ]);
    }
    out.extend([
        d.let_(Ty::F, "C0", &format!("C - {x}")),
        d.let_(Ty::F, "discrim", "sqrt(B * B - 4.0 * A * C0)"),
        d.let_(Ty::F, "denom", "discrim + B"),
        // GradingBSplineCurve.cpp:1157-1161: a flat segment at its minimum.
        format!(
            "if (abs(denom) < 1e-5) {{ return {}; }}",
            d.sel("abs(B) < 1e-5", "kn", "kn + (-C0 / B)")
        ),
        "return kn + (-2.0 * C0) / denom;".into(),
    ]);
    out
}

/// OCIO `AddLinToLogShader` (all channels) / `AddLinToLogShaderChannelBlue` (`blue_only`).
pub(super) fn lin_to_log(d: &Dialect, blue_only: bool) -> Vec<String> {
    let mut out = vec!["{ // Convert from lin to log.".to_string()];
    out.push(d.decl_f("xbrk", "0.0041318374739483946"));
    out.push(d.decl_f("shift", "-0.000157849851665374"));
    out.push(d.decl_f("m", "1.0 / (0.18 + shift)"));
    out.push(d.decl_f("base2", "1.4426950408889634"));
    out.push(d.decl_f("gain", "363.034608563"));
    out.push(d.decl_f("offs", "-7.0"));
    out.push(d.decl("ylin", "color.rgb * gain + offs"));
    out.push(d.decl(
        "ylog",
        &format!("base2 * log((color.rgb + {}) * m)", d.splat("shift")),
    ));
    for (c, v) in [("r", "x"), ("g", "y"), ("b", "z")] {
        if blue_only && c != "b" {
            continue;
        }
        out.push(format!(
            "    color.{c} = {};",
            d.sel(
                &format!("color.{c} < xbrk"),
                &format!("ylin.{v}"),
                &format!("ylog.{v}")
            )
        ));
    }
    out.push("}".into());
    out
}

/// OCIO `AddLogToLinShader` / `AddLogToLinShaderChannelBlue` (`blue_only`).
pub(super) fn log_to_lin(d: &Dialect, blue_only: bool) -> Vec<String> {
    let mut out = vec!["{ // Convert from log to lin.".to_string()];
    out.push(d.decl_f("ybrk", "-5.5"));
    out.push(d.decl_f("shift", "-0.000157849851665374"));
    out.push(d.decl_f("gain", "363.034608563"));
    out.push(d.decl_f("offs", "-7.0"));
    out.push(d.decl("xlin", &format!("(color.rgb - {}) / gain", d.splat("offs"))));
    out.push(d.decl(
        "xlog",
        &format!(
            "pow({}, color.rgb) * (0.18 + shift) - {}",
            d.k(2.0),
            d.splat("shift")
        ),
    ));
    for (c, v) in [("r", "x"), ("g", "y"), ("b", "z")] {
        if blue_only && c != "b" {
            continue;
        }
        out.push(format!(
            "    color.{c} = {};",
            d.sel(
                &format!("color.{c} < ybrk"),
                &format!("xlin.{v}"),
                &format!("xlog.{v}")
            )
        ));
    }
    out.push("}".into());
    out
}

/// Lines nested one level deeper.
fn nest(lines: Vec<String>) -> Vec<String> {
    lines.into_iter().map(|l| format!("    {l}")).collect()
}

/// OCIO `GradingRGBCurveOpGPU.cpp`: in the linear style (unless bypassed) the curves run in the
/// log domain; the master curve (index 3) after (forward) / before (inverse) the per-channel ones.
pub(super) fn rgb_curve(
    d: &Dialect,
    idx: usize,
    style: GradingToneStyle,
    bypass_lin_to_log: bool,
    forward: bool,
    uniforms: Option<&OpUniforms>,
) -> Vec<String> {
    let Some(n) = CurveNames::of(uniforms) else {
        return vec!["// GradingRgbCurve without uniforms".into()];
    };
    let lin = matches!(style, GradingToneStyle::Linear) && !bypass_lin_to_log;
    let mut body = Vec::new();
    if lin {
        body.extend(lin_to_log(d, false));
    }
    let order: [(usize, &str); 6] = if forward {
        [(0, "r"), (1, "g"), (2, "b"), (3, "r"), (3, "g"), (3, "b")]
    } else {
        [(3, "r"), (3, "g"), (3, "b"), (0, "r"), (1, "g"), (2, "b")]
    };
    for (curve, c) in order {
        body.push(if forward {
            format!(
                "color.{c} = {}({curve}, color.{c}, color.{c});",
                eval_fn(idx)
            )
        } else {
            format!("color.{c} = {}({curve}, color.{c});", eval_rev_fn(idx))
        });
    }
    if lin {
        body.extend(log_to_lin(d, false));
    }
    let mut out = vec!["{ // GradingRgbCurve".to_string()];
    out.push(format!("    if (!{}) {{", d.uniform_bool(n.local_bypass)));
    out.extend(nest(nest(body)));
    out.push("    }".into());
    out.push("}".into());
    out
}

/// OCIO `GradingHueCurveOpGPU.cpp` `AddGCForwardShader` / `AddGCInverseShader`: RGB to HSY with the
/// style's HSY (the FixedFunction body, `hsy.rs`), the luma to log in the linear style, the eight
/// curves, and back.
///
/// EMIT-14, deliberate divergence: the luminance gain is additive unless the style is Linear (OCIO's
/// GPU keys it on `style == LOG`, its CPU on `m_isLinear`; our CPU and every language key on
/// `!Linear`, see `tests/red_gate_hue_curve_video.rs`).
pub(super) fn hue_curve(
    d: &Dialect,
    idx: usize,
    style: crate::grading_hue_curve::GradingStyle,
    rgb_to_hsy: HsyTransformStyle,
    draw_curve_only: bool,
    forward: bool,
    uniforms: Option<&OpUniforms>,
) -> Vec<String> {
    use crate::grading_hue_curve::GradingStyle;
    let Some(n) = CurveNames::of(uniforms) else {
        return vec!["// GradingHueCurve without uniforms".into()];
    };
    let ev = eval_fn(idx);
    let mut out = vec![format!(
        "{{ // GradingHueCurve {}",
        if forward { "Forward" } else { "Inverse" }
    )];
    if draw_curve_only {
        // Not bypassed: the horizontal curves' default is 1, not the identity.
        for c in ["r", "g", "b"] {
            out.push(format!("    color.{c} = {ev}(1, color.{c}, 1.0);"));
        }
        out.push("}".into());
        return out;
    }
    let (to_hsy, from_hsy) = match style {
        GradingStyle::Linear => (
            FixedFunctionStyle::RgbToHsyLin,
            FixedFunctionStyle::HsyLinToRgb,
        ),
        GradingStyle::Log => (
            FixedFunctionStyle::RgbToHsyLog,
            FixedFunctionStyle::HsyLogToRgb,
        ),
        GradingStyle::Video => (
            FixedFunctionStyle::RgbToHsyVid,
            FixedFunctionStyle::HsyVidToRgb,
        ),
    };
    let hsy = matches!(rgb_to_hsy, HsyTransformStyle::Default);
    let lin = matches!(style, GradingStyle::Linear);
    let additive = !lin;
    let mut b = Vec::new();
    if hsy {
        b.extend(super::hsy::emit(to_hsy, d).expect("an HSY style"));
    }
    if forward {
        if lin {
            b.extend(lin_to_log(d, true));
        }
        b.push(d.var_(
            Ty::F,
            "hueSatGain",
            &format!("max(0.0, {ev}(1, color.r, 1.0))"),
        ));
        b.push(d.var_(
            Ty::F,
            "hueLumGain",
            &format!("max(0.0, {ev}(2, color.r, 1.0))"),
        ));
        b.push(format!("color.r = {ev}(0, color.r, color.r);"));
        b.push(format!("color.g = max(0.0, {ev}(4, color.g, color.g));"));
        b.push(d.let_(
            Ty::F,
            "lumSatGain",
            &format!("max(0.0, {ev}(3, color.b, 1.0))"),
        ));
        b.push(d.let_(Ty::F, "satGain", "lumSatGain * hueSatGain"));
        b.push("color.g = satGain * color.g;".into());
        b.push(d.let_(
            Ty::F,
            "satLumGain",
            &format!("max(0.0, {ev}(6, color.g, 1.0))"),
        ));
        b.push(format!("color.b = {ev}(5, color.b, color.b);"));
        if lin {
            b.extend(log_to_lin(d, true));
        }
        b.push("hueLumGain = 1.0 - (1.0 - hueLumGain) * min(1.0, color.g);".into());
        b.push(if additive {
            "color.b = color.b + (hueLumGain + satLumGain - 2.0) * 0.1;".into()
        } else {
            "color.b = color.b * hueLumGain * satLumGain;".into()
        });
        b.push("color.r = color.r - floor(color.r);".into());
        b.push(format!("color.r = color.r + {ev}(7, color.r, 0.0);"));
    } else {
        let rev = eval_rev_fn(idx);
        let rev_hue = eval_rev_hue_fn(idx);
        b.push(format!("color.r = {rev_hue}(7, color.r);"));
        b.push(format!("color.r = {rev_hue}(0, color.r);"));
        b.push("color.r = color.r - floor(color.r);".into());
        b.push(d.let_(
            Ty::F,
            "hueSatGain",
            &format!("max(0.0, {ev}(1, color.r, 1.0))"),
        ));
        b.push(d.var_(
            Ty::F,
            "hueLumGain",
            &format!("max(0.0, {ev}(2, color.r, 1.0))"),
        ));
        b.push("color.g = max(0.0, color.g);".into());
        b.push(d.let_(
            Ty::F,
            "satLumGain",
            &format!("max(0.0, {ev}(6, color.g, 1.0))"),
        ));
        b.push("hueLumGain = 1.0 - (1.0 - hueLumGain) * min(1.0, color.g);".into());
        b.push(if additive {
            "color.b = color.b - (hueLumGain + satLumGain - 2.0) * 0.1;".into()
        } else {
            "color.b = color.b / max(0.01, hueLumGain * satLumGain);".into()
        });
        if lin {
            b.extend(lin_to_log(d, true));
        }
        b.push(format!("color.b = {rev}(5, color.b);"));
        b.push(d.let_(
            Ty::F,
            "lumSatGain",
            &format!("max(0.0, {ev}(3, color.b, 1.0))"),
        ));
        if lin {
            b.extend(log_to_lin(d, true));
        }
        b.push(d.let_(Ty::F, "satGain", "max(0.01, lumSatGain * hueSatGain)"));
        b.push("color.g = color.g / satGain;".into());
        b.push(format!("color.g = max(0.0, {rev}(4, color.g));"));
    }
    if hsy {
        b.extend(super::hsy::emit(from_hsy, d).expect("an HSY style"));
    }
    out.push(format!("    if (!{}) {{", d.uniform_bool(n.local_bypass)));
    out.extend(nest(nest(b)));
    out.push("    }".into());
    out.push("}".into());
    out
}
