//! Shared, language-independent emission of OCIO's curve-shaped FixedFunctions:
//! `LIN_TO_PQ`/`PQ_TO_LIN`, `LIN_TO_GAMMA_LOG`/`GAMMA_LOG_TO_LIN` and
//! `LIN_TO_DOUBLE_LOG`/`DOUBLE_LOG_TO_LIN`.
//!
//! These used to be written four times, once per backend, and had drifted: GLSL emitted a
//! comment and NOTHING for the four gamma-log / double-log styles (a silent identity), the
//! other three reproduced the mirror sign with `sign()`, which is 0 at the mirror point where
//! OCIO's `copysign(1, x)` is 1 (so Apple Log decoded a clamped negative to 0 instead of
//! `R_0`), and the PQ constants were retyped in each. The parameters now come from the same
//! decoders the CPU apply uses ([`GammaLogParams`], [`DoubleLogParams`], `vfx_transfer::pq`'s
//! constants) and the body is written once here, parameterized by [`Dialect`].

use super::dialect::Dialect;
use crate::processor::op::{DoubleLogParams, GammaLogParams, resolve_fixed_function_direction};
use crate::transform::FixedFunctionStyle;

/// `copysign(1, x)` per component: `+1` for `x >= 0` (including the mirror point), `-1` below.
fn copysign_one(d: &Dialect, x: &str) -> String {
    d.mix(&d.k(-1.0), &d.k(1.0), &format!("step({}, {x})", d.k(0.0)))
}

/// Emits the shader body for one of the six curve-shaped styles, or `None` for any other.
///
/// Returned lines carry their own inner indentation and include the enclosing braces; callers
/// add their own outer indentation (the same contract as [`super::transfer::emit`]).
pub fn emit(
    style: FixedFunctionStyle,
    params: &[f32],
    forward: bool,
    d: &Dialect,
) -> Option<Vec<String>> {
    // Normally already resolved at compile time; resolving again keeps a hand-built op honest.
    let (style, _) = resolve_fixed_function_direction(style, forward);
    use FixedFunctionStyle as F;
    let body = match style {
        F::LinToPq => lin_to_pq(d),
        F::PqToLin => pq_to_lin(d),
        F::LinToGammaLog => lin_to_gamma_log(&GammaLogParams::from_params(params), d),
        F::GammaLogToLin => gamma_log_to_lin(&GammaLogParams::from_params(params), d),
        F::LinToDoubleLog => lin_to_double_log(&DoubleLogParams::from_params(params), d),
        F::DoubleLogToLin => double_log_to_lin(&DoubleLogParams::from_params(params), d),
        _ => return None,
    };
    Some(body)
}

/// `Renderer_LIN_TO_PQ` (`FixedFunctionOpCPU.cpp:2089-2116`), = `vfx_transfer::pq::ocio_lin_to_pq`.
fn lin_to_pq(d: &Dialect) -> Vec<String> {
    let mut body = vec!["{ // LIN_TO_PQ (1.0 = 100 nits, mirrored)".to_string()];
    body.extend(super::pq::encode(d, "abs(color.rgb) * 0.01", "npq"));
    body.push(d.set(&format!("npq * {}", copysign_one(d, "color.rgb"))));
    body.push("}".to_string());
    body
}

/// `Renderer_PQ_TO_LIN`, = `vfx_transfer::pq::ocio_pq_to_lin`. No clamp above 1, like OCIO.
fn pq_to_lin(d: &Dialect) -> Vec<String> {
    let mut body = vec!["{ // PQ_TO_LIN (1.0 = 100 nits, mirrored)".to_string()];
    body.extend(super::pq::decode(d, "abs(color.rgb)", "y"));
    body.push(d.set(&format!("100.0 * y * {}", copysign_one(d, "color.rgb"))));
    body.push("}".to_string());
    body
}

/// `Renderer_LIN_TO_GAMMA_LOG::apply` (`FixedFunctionOpCPU.cpp:2248-2277`).
fn lin_to_gamma_log(p: &GammaLogParams, d: &Dialect) -> Vec<String> {
    let k = |v: f32| d.k(v as f64);
    vec![
        "{ // LIN_TO_GAMMA_LOG".to_string(),
        d.decl("mir", &format!("color.rgb - {}", k(p.mirror))),
        d.decl("e", &format!("abs(mir) + {}", k(p.mirror))),
        d.decl(
            "gam",
            &format!(
                "{} * pow(max(e + {}, {}), {})",
                k(p.gamma_slope),
                k(p.gamma_off),
                d.k(0.0),
                k(p.power)
            ),
        ),
        d.decl(
            "lg",
            &format!(
                "{} * {} + {}",
                k(p.log_slope),
                d.ln(&format!("{} * e + {}", k(p.lin_slope), k(p.lin_off))),
                k(p.log_off)
            ),
        ),
        // E < break -> gamma segment, otherwise log.
        d.set(&format!(
            "{} * {}",
            d.mix("gam", "lg", &format!("step({}, e)", k(p.brk))),
            copysign_one(d, "mir")
        )),
        "}".to_string(),
    ]
}

/// `Renderer_GAMMA_LOG_TO_LIN::apply` (`FixedFunctionOpCPU.cpp:2290-2320`).
fn gamma_log_to_lin(p: &GammaLogParams, d: &Dialect) -> Vec<String> {
    let k = |v: f32| d.k(v as f64);
    vec![
        "{ // GAMMA_LOG_TO_LIN".to_string(),
        d.decl("mir", &format!("color.rgb - {}", k(p.prime_mirror))),
        d.decl("ep", &format!("abs(mir) + {}", k(p.prime_mirror))),
        d.decl(
            "gam",
            &format!(
                "pow(max(ep / {}, {}), {}) - {}",
                k(p.gamma_slope),
                d.k(0.0),
                k(1.0 / p.power),
                k(p.gamma_off)
            ),
        ),
        d.decl(
            "lg",
            &format!(
                "({} - {}) / {}",
                d.exp(&format!("(ep - {}) / {}", k(p.log_off), k(p.log_slope))),
                k(p.lin_off),
                k(p.lin_slope)
            ),
        ),
        d.set(&format!(
            "{} * {}",
            d.mix("gam", "lg", &format!("step({}, ep)", k(p.prime_brk))),
            copysign_one(d, "mir")
        )),
        "}".to_string(),
    ]
}

/// A log segment forward, `log_slope * ln(lin_slope * x + lin_off) + log_off`, floored so the
/// branch `mix` discards can never be a NaN.
fn seg_fwd(d: &Dialect, s: &crate::processor::op::LogSegment, x: &str) -> String {
    let k = |v: f32| d.k(v as f64);
    format!(
        "{} * {} + {}",
        k(s.log_slope),
        d.ln(&format!("{} * {x} + {}", k(s.lin_slope), k(s.lin_off))),
        k(s.log_off)
    )
}

/// A log segment inverse, with the exponent capped.
fn seg_inv(d: &Dialect, s: &crate::processor::op::LogSegment, y: &str) -> String {
    let k = |v: f32| d.k(v as f64);
    format!(
        "({} - {}) / {}",
        d.exp(&format!("({y} - {}) / {}", k(s.log_off), k(s.log_slope))),
        k(s.lin_off),
        k(s.lin_slope)
    )
}

/// `Renderer_LIN_TO_DOUBLE_LOG::apply` (`FixedFunctionOpCPU.cpp:2346-2376`):
/// `x <= break1` log segment 1, `x < break2` linear, else log segment 2.
fn lin_to_double_log(p: &DoubleLogParams, d: &Dialect) -> Vec<String> {
    let k = |v: f32| d.k(v as f64);
    vec![
        "{ // LIN_TO_DOUBLE_LOG".to_string(),
        d.decl("x", "color.rgb"),
        d.decl("l1", &seg_fwd(d, &p.seg1, "x")),
        d.decl("l2", &seg_fwd(d, &p.seg2, "x")),
        d.decl("li", &format!("{} * x + {}", k(p.lin_slope), k(p.lin_off))),
        d.set(&d.mix(
            &d.mix("li", "l2", &format!("step({}, x)", k(p.brk2))),
            "l1",
            &format!("step(x, {})", k(p.brk1)),
        )),
        "}".to_string(),
    ]
}

/// `Renderer_DOUBLE_LOG_TO_LIN::apply` (`FixedFunctionOpCPU.cpp:2387-2420`).
fn double_log_to_lin(p: &DoubleLogParams, d: &Dialect) -> Vec<String> {
    let k = |v: f32| d.k(v as f64);
    // A zero linear slope means "no linear segment" (break1 == break2 in log space as well);
    // dividing by it would put an inf in the branch `mix` discards, and `inf * 0` is NaN.
    let li = if p.lin_slope == 0.0 {
        d.k(0.0)
    } else {
        format!("(y - {}) / {}", k(p.lin_off), k(p.lin_slope))
    };
    vec![
        "{ // DOUBLE_LOG_TO_LIN".to_string(),
        d.decl("y", "color.rgb"),
        d.decl("e1", &seg_inv(d, &p.seg1, "y")),
        d.decl("e2", &seg_inv(d, &p.seg2, "y")),
        d.decl("li", &li),
        d.set(&d.mix(
            &d.mix("li", "e2", &format!("step({}, y)", k(p.brk2_log))),
            "e1",
            &format!("step(y, {})", k(p.brk1_log)),
        )),
        "}".to_string(),
    ]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::dialect::{GLSL, HLSL, METAL, WGSL};
    use super::*;

    /// HLG (`Displays.cpp` non-LUT params) and Canon Log 2 (`CanonCameras.cpp` non-LUT params).
    pub(crate) fn cases() -> Vec<(FixedFunctionStyle, Vec<f32>)> {
        let hlg = vec![
            0.0,
            0.25,
            0.5,
            1.0,
            0.0,
            std::f32::consts::E,
            0.178_832_77,
            0.559_910_7,
            1.0,
            -0.023_722_41,
        ];
        let clog2 = vec![
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
        ];
        vec![
            (FixedFunctionStyle::LinToPq, vec![]),
            (FixedFunctionStyle::PqToLin, vec![]),
            (FixedFunctionStyle::LinToGammaLog, hlg.clone()),
            (FixedFunctionStyle::GammaLogToLin, hlg),
            (FixedFunctionStyle::LinToDoubleLog, clog2.clone()),
            (FixedFunctionStyle::DoubleLogToLin, clog2),
        ]
    }

    #[test]
    fn wgsl_bodies_parse() {
        for (style, params) in cases() {
            let body = emit(style, &params, true, &WGSL).unwrap().join("\n");
            let src = format!(
                "@fragment\nfn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {{\n\
                 var color = vec4<f32>(uv.x, uv.y, 0.5, 1.0);\n{body}\nreturn color;\n}}\n"
            );
            if let Err(e) = naga::front::wgsl::parse_str(&src) {
                panic!("naga rejected {style:?}:\n{e}\n\n{src}");
            }
        }
    }

    #[test]
    fn all_dialects_emit_the_same_statement_count() {
        for (style, params) in cases() {
            let counts: Vec<usize> = [&GLSL, &HLSL, &METAL, &WGSL]
                .iter()
                .map(|d| emit(style, &params, true, d).unwrap().len())
                .collect();
            assert!(
                counts.windows(2).all(|w| w[0] == w[1]),
                "{style:?}: {counts:?}"
            );
        }
    }

    /// The inverse of a pair is its partner, even if an op arrives unresolved.
    #[test]
    fn unresolved_inverse_emits_the_partner() {
        assert_eq!(
            emit(FixedFunctionStyle::LinToPq, &[], false, &GLSL),
            emit(FixedFunctionStyle::PqToLin, &[], true, &GLSL)
        );
    }

    /// The mirror sign is `copysign(1, x)`, never `sign(x)` (0 at the mirror point).
    #[test]
    fn mirror_sign_is_copysign_not_sign() {
        for (style, params) in cases() {
            let body = emit(style, &params, true, &GLSL).unwrap().join("\n");
            assert!(!body.contains("sign("), "{style:?}:\n{body}");
        }
    }
}
