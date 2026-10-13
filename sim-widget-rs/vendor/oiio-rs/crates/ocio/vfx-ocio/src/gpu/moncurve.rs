//! Shared, language-independent emission of OCIO's moncurve (`GpuOp::ExponentWithLinear`,
//! `GammaOpData::MONCURVE_*`).
//!
//! The four backends each carried their own copy of this op, each re-deriving the break point
//! and linear slope with its own algebra, and all four treated OCIO's default `LINEAR` negative
//! style as a mirror. That is only equivalent while `|x|` stays below the break: OCIO's
//! `GammaMoncurveOpCPUFwd/Rev::apply` (`GammaOpCPU.cpp:525-645`) sends EVERY value `<= break`,
//! so every negative, down the linear segment. The parameters now come from
//! `vfx_transfer::gamma::moncurve_params` — the same function the CPU apply uses — and the shader
//! body is written once here, parameterized by [`Dialect`].

use super::dialect::{Dialect, n};
use crate::processor::{MoncurveParams, moncurve_params};

/// A per-channel constant vector.
fn v3(d: &Dialect, v: [f32; 3]) -> String {
    format!(
        "{}({}, {}, {})",
        d.v3,
        n(v[0] as f64),
        n(v[1] as f64),
        n(v[2] as f64)
    )
}

/// Emits the shader body for one `ExponentWithLinear` op.
///
/// Returned lines carry their own inner indentation and include the enclosing braces; callers
/// add their own outer indentation (the same contract as [`super::transfer::emit`]).
pub fn emit(
    gamma: [f32; 4],
    offset: [f32; 4],
    mirror: bool,
    forward: bool,
    d: &Dialect,
) -> Vec<String> {
    let p: [MoncurveParams; 3] =
        std::array::from_fn(|i| moncurve_params(gamma[i] as f64, offset[i] as f64, forward));
    let pick = |f: fn(&MoncurveParams) -> f32| -> [f32; 3] { std::array::from_fn(|i| f(&p[i])) };

    let mut out = vec![
        format!(
            "{{ // ExponentWithLinear {} ({})",
            if forward { "forward" } else { "inverse" },
            if mirror { "mirror" } else { "linear" }
        ),
        d.decl("brk", &v3(d, pick(|p| p.break_pnt))),
        d.decl("slp", &v3(d, pick(|p| p.slope))),
        d.decl("scl", &v3(d, pick(|p| p.scale))),
        d.decl("off", &v3(d, pick(|p| p.offset))),
        d.decl("g", &v3(d, pick(|p| p.gamma))),
    ];

    // The power leg is always evaluated on a non-negative argument, so the branch that `mix`
    // discards can never be a NaN (`0 * NaN` would still poison the result).
    let power = |x: &str| -> String {
        let x = format!("max({x}, {})", d.k(0.0));
        if forward {
            format!("pow({x} * scl + off, g)")
        } else {
            format!("pow({x}, g) * scl - off")
        }
    };
    // `step(brk, x)` is 1 at and above the break: OCIO's `x <= brk ? linear : power` differs only
    // AT the break, where the two legs are equal by construction.
    let curve =
        |x: &str| -> String { d.mix(&format!("{x} * slp"), &power(x), &format!("step(brk, {x})")) };

    if mirror {
        // MONCURVE_MIRROR: sign(x) * f(|x|).
        out.push(d.decl("ax", "abs(color.rgb)"));
        out.push(d.set(&format!("sign(color.rgb) * {}", curve("ax"))));
    } else {
        // MONCURVE: negatives are below the break, so they take the linear segment.
        out.push(d.decl("x", "color.rgb"));
        out.push(d.set(&curve("x")));
    }
    // Alpha has its own parameters (OCIO's default 1 / 0 is the identity, emitted as nothing).
    if gamma[3] != 1.0 || offset[3] != 0.0 {
        let p = moncurve_params(gamma[3] as f64, offset[3] as f64, forward);
        let x = if mirror { "abs(color.a)" } else { "color.a" };
        let xp = format!("max({x}, 0.0)");
        let power = if forward {
            format!(
                "pow({xp} * {} + {}, {})",
                n(p.scale as f64),
                n(p.offset as f64),
                n(p.gamma as f64)
            )
        } else {
            format!(
                "pow({xp}, {}) * {} - {}",
                n(p.gamma as f64),
                n(p.scale as f64),
                n(p.offset as f64)
            )
        };
        let a = d.sel(
            &format!("{x} <= {}", n(p.break_pnt as f64)),
            &format!("{x} * {}", n(p.slope as f64)),
            &power,
        );
        out.push(if mirror {
            format!("    color.a = sign(color.a) * {a};")
        } else {
            format!("    color.a = {a};")
        });
    }
    out.push("}".to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::super::dialect::{GLSL, HLSL, METAL, WGSL};
    use super::*;

    /// `mirror`: LINEAR and MIRROR, the only moncurve negative styles OCIO has.
    const STYLES: [bool; 2] = [false, true];
    const SRGB_G: [f32; 4] = [2.4, 2.4, 2.4, 1.0];
    const SRGB_O: [f32; 4] = [0.055, 0.055, 0.055, 0.0];

    #[test]
    fn wgsl_bodies_parse() {
        for style in STYLES {
            for forward in [true, false] {
                let body = emit(SRGB_G, SRGB_O, style, forward, &WGSL).join("\n");
                let src = format!(
                    "@fragment\nfn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {{\n\
                     var color = vec4<f32>(uv.x, uv.y, 0.5, 1.0);\n{body}\nreturn color;\n}}\n"
                );
                if let Err(e) = naga::front::wgsl::parse_str(&src) {
                    panic!("naga rejected {style:?} forward={forward}:\n{e}\n\n{src}");
                }
            }
        }
    }

    #[test]
    fn all_dialects_emit_the_same_statement_count() {
        for style in STYLES {
            for forward in [true, false] {
                let counts: Vec<usize> = [&GLSL, &HLSL, &METAL, &WGSL]
                    .iter()
                    .map(|d| emit(SRGB_G, SRGB_O, style, forward, d).len())
                    .collect();
                assert!(
                    counts.windows(2).all(|w| w[0] == w[1]),
                    "{style:?}: {counts:?}"
                );
            }
        }
    }

    /// The LINEAR style must not mirror: the emitted body evaluates `x * slp` on the signed
    /// value, never `abs(...)`.
    #[test]
    fn linear_style_keeps_the_sign_inside_the_linear_leg() {
        let body = emit(SRGB_G, SRGB_O, false, false, &GLSL).join("\n");
        assert!(!body.contains("abs("), "{body}");
        assert!(body.contains("x * slp"), "{body}");
    }
}
