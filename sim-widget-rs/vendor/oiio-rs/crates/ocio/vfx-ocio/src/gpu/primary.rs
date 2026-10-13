//! GradingPrimary for every shading language: OCIO `GradingPrimaryOpGPU.cpp` over [`Dialect`]
//! (ROADMAP G6).
//!
//! The values are OCIO's computed ones (`GradingPrimaryPreRender`, here
//! `processor::grading_primary_params`): already resolved for the direction, so the inverse
//! applies them as they are, like the reference.

use super::dialect::{Dialect, nf};
use super::types::OpUniforms;
use crate::transform::GradingPrimaryStyle;

/// OCIO `GPProperties`: every value as a shader expression, a uniform or (without uniforms, OCIO's
/// non-dynamic path) a literal.
struct Props {
    brightness: String,
    contrast: String,
    gamma: String,
    exposure: String,
    offset: String,
    slope: String,
    saturation: String,
    pivot: String,
    pivot_black: String,
    pivot_white: String,
    clamp_black: String,
    clamp_white: String,
}

/// The primary's parameters, as `GpuOp::GradingPrimary` holds them.
pub(super) struct Values<'a> {
    pub style: GradingPrimaryStyle,
    pub brightness: &'a [f32; 3],
    pub contrast: &'a [f32; 3],
    pub gamma: &'a [f32; 3],
    pub exposure: &'a [f32; 3],
    pub offset: &'a [f32; 3],
    pub slope: &'a [f32; 3],
    pub saturation: f32,
    pub pivot: f32,
    pub pivot_black: f32,
    pub pivot_white: f32,
    pub clamp_black: f32,
    pub clamp_white: f32,
    pub forward: bool,
}

/// `color.rgb = expr;` in dialect `d`.
fn set(d: &Dialect, expr: &str) -> String {
    d.set(expr).trim_start().to_string()
}

/// The gamma block shared by the log and video styles.
fn gamma(d: &Dialect, p: &Props, out: &mut Vec<String>) {
    let (pb, pw) = (&p.pivot_black, &p.pivot_white);
    out.push(format!("if ({}) {{", d.any_ne(&p.gamma, &d.k(1.0))));
    out.push(format!(
        "    {}",
        d.decl(
            "normalizedOut",
            &format!("abs(color.rgb - {pb}) / ({pw} - {pb})")
        )
        .trim_start()
    ));
    // NB: sign() is per channel, preserving each channel's sign.
    out.push(format!(
        "    {}",
        d.decl("scale", &format!("sign(color.rgb - {pb}) * ({pw} - {pb})"))
            .trim_start()
    ));
    out.push(format!(
        "    {}",
        set(
            d,
            &format!("pow(normalizedOut, {}) * scale + {pb}", p.gamma)
        )
    ));
    out.push("}".into());
}

/// The linear style's contrast block.
fn lin_contrast(d: &Dialect, p: &Props, out: &mut Vec<String>) {
    out.push(format!("if ({}) {{", d.any_ne(&p.contrast, &d.k(1.0))));
    out.push(format!(
        "    {}",
        set(
            d,
            &format!(
                "pow(abs(color.rgb / {pv}), {c}) * sign(color.rgb) * {pv}",
                pv = p.pivot,
                c = p.contrast
            )
        )
    ));
    out.push("}".into());
}

fn lumawgts(d: &Dialect) -> [String; 2] {
    [
        d.decl("lumaWgts", &format!("{}(0.2126, 0.7152, 0.0722)", d.v3))
            .trim_start()
            .to_string(),
        d.decl_f("luma", "dot(color.rgb, lumaWgts)")
            .trim_start()
            .to_string(),
    ]
}

/// `AddGP{Log,Lin,Video}ForwardShader`.
fn forward(d: &Dialect, style: GradingPrimaryStyle, p: &Props, out: &mut Vec<String>) {
    match style {
        GradingPrimaryStyle::Log => {
            out.push(set(d, &format!("color.rgb + {}", p.brightness)));
            out.push(set(
                d,
                &format!("(color.rgb - {pv}) * {} + {pv}", p.contrast, pv = p.pivot),
            ));
            gamma(d, p, out);
        }
        GradingPrimaryStyle::Linear => {
            out.push(set(d, &format!("color.rgb + {}", p.offset)));
            out.push(set(d, &format!("color.rgb * {}", p.exposure)));
            lin_contrast(d, p, out);
        }
        GradingPrimaryStyle::Video => {
            out.push(set(d, &format!("color.rgb + {}", p.offset)));
            out.push(set(
                d,
                &format!(
                    "(color.rgb - {pb}) * {} + {pb}",
                    p.slope,
                    pb = p.pivot_black
                ),
            ));
            gamma(d, p, out);
        }
    }
    out.extend(lumawgts(d));
    out.push(set(
        d,
        &format!("luma + {} * (color.rgb - luma)", p.saturation),
    ));
    out.push(set(
        d,
        &format!(
            "clamp(color.rgb, {}, {})",
            d.splat(&p.clamp_black),
            d.splat(&p.clamp_white)
        ),
    ));
}

/// `AddGP{Log,Lin,Video}InverseShader`.
fn inverse(d: &Dialect, style: GradingPrimaryStyle, p: &Props, out: &mut Vec<String>) {
    out.push(set(
        d,
        &format!(
            "clamp(color.rgb, {}, {})",
            d.splat(&p.clamp_black),
            d.splat(&p.clamp_white)
        ),
    ));
    out.push(format!(
        "if ({s} != 0.0 && {s} != 1.0) {{",
        s = p.saturation
    ));
    for l in lumawgts(d) {
        out.push(format!("    {l}"));
    }
    out.push(format!(
        "    {}",
        set(d, &format!("luma + (color.rgb - luma) / {}", p.saturation))
    ));
    out.push("}".into());
    match style {
        GradingPrimaryStyle::Log => {
            gamma(d, p, out);
            out.push(set(
                d,
                &format!("(color.rgb - {pv}) * {} + {pv}", p.contrast, pv = p.pivot),
            ));
            out.push(set(d, &format!("color.rgb + {}", p.brightness)));
        }
        GradingPrimaryStyle::Linear => {
            lin_contrast(d, p, out);
            out.push(set(d, &format!("color.rgb * {}", p.exposure)));
            out.push(set(d, &format!("color.rgb + {}", p.offset)));
        }
        GradingPrimaryStyle::Video => {
            gamma(d, p, out);
            out.push(set(
                d,
                &format!(
                    "(color.rgb - {pb}) * {} + {pb}",
                    p.slope,
                    pb = p.pivot_black
                ),
            ));
            out.push(set(d, &format!("color.rgb + {}", p.offset)));
        }
    }
}

/// OCIO `GetGradingPrimaryGPUShaderProgram`: the style's forward or inverse body inside the
/// local-bypass test.
pub(super) fn emit(d: &Dialect, v: &Values, uniforms: Option<&OpUniforms>) -> Vec<String> {
    let lit3 = |x: &[f32; 3]| format!("{}({}, {}, {})", d.v3, nf(x[0]), nf(x[1]), nf(x[2]));
    let (p, bypass) = match uniforms {
        Some(OpUniforms::GradingPrimary {
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
            local_bypass,
        }) => (
            Props {
                brightness: d.uniform_v3(brightness),
                contrast: d.uniform_v3(contrast),
                gamma: d.uniform_v3(gamma),
                exposure: d.uniform_v3(exposure),
                offset: d.uniform_v3(offset),
                slope: d.uniform_v3(slope),
                saturation: d.uniform(saturation),
                pivot: d.uniform(pivot),
                pivot_black: d.uniform(pivot_black),
                pivot_white: d.uniform(pivot_white),
                clamp_black: d.uniform(clamp_black),
                clamp_white: d.uniform(clamp_white),
            },
            Some(d.uniform_bool(local_bypass)),
        ),
        _ => (
            Props {
                brightness: lit3(v.brightness),
                contrast: lit3(v.contrast),
                gamma: lit3(v.gamma),
                exposure: lit3(v.exposure),
                offset: lit3(v.offset),
                slope: lit3(v.slope),
                saturation: nf(v.saturation),
                pivot: nf(v.pivot),
                pivot_black: nf(v.pivot_black),
                pivot_white: nf(v.pivot_white),
                clamp_black: nf(v.clamp_black),
                clamp_white: nf(v.clamp_white),
            },
            None,
        ),
    };
    let mut body = Vec::new();
    if v.forward {
        forward(d, v.style, &p, &mut body);
    } else {
        inverse(d, v.style, &p, &mut body);
    }
    let style = match v.style {
        GradingPrimaryStyle::Log => "log",
        GradingPrimaryStyle::Linear => "linear",
        GradingPrimaryStyle::Video => "video",
    };
    let mut out = vec![format!(
        "{{ // GradingPrimary '{style}' {}",
        if v.forward { "forward" } else { "inverse" }
    )];
    match bypass {
        Some(b) => {
            out.push(format!("    if (!{b}) {{"));
            out.extend(body.into_iter().map(|l| format!("        {l}")));
            out.push("    }".into());
        }
        None => out.extend(body.into_iter().map(|l| format!("    {l}"))),
    }
    out.push("}".into());
    out
}
