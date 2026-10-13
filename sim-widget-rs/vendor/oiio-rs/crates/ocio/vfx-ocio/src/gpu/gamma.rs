//! Shared, language-independent emission of OCIO's basic gamma op (`GpuOp::Exponent`,
//! `GammaOpData::BASIC_*`) and of the alpha channel of the ops that touch it.
//!
//! OCIO's ops see RGBA. Only two of them change alpha (`MatrixOpCPU.cpp`, `GammaOpCPU.cpp`): the
//! 4x4 matrix (its alpha column feeds RGB, its alpha row and offset make the new alpha) and the
//! gamma op (channel 3 goes through the same curve with its own parameter). The CPU apply does
//! this in `processor/apply.rs` `apply_op_to_rgba`; the shader bodies here are its GPU side.
//! Before this module the four backends each carried their own RGB-only `Exponent` body, and
//! they disagreed: WGSL treated PASS_THRU as a mirror.

use super::dialect::{Dialect, n};
use crate::transform::NegativeStyle;

/// One channel of the basic gamma op as a scalar expression of `x` with exponent `g`, the same
/// rule as `processor/apply.rs` `exponent_channel`.
fn basic_scalar(d: &Dialect, x: &str, g: f32, style: NegativeStyle) -> String {
    let g = n(g as f64);
    let pos = format!("pow(max({x}, 0.0), {g})");
    match style {
        NegativeStyle::Clamp => pos,
        NegativeStyle::Mirror | NegativeStyle::Linear => {
            format!("sign({x}) * pow(abs({x}), {g})")
        }
        NegativeStyle::PassThru => d.sel(&format!("{x} >= 0.0"), &pos, x),
    }
}

/// Emits the shader body for one `Exponent` op: RGB with `value[0..3]`, alpha with `value[3]`
/// when that changes anything (an exponent other than 1, or CLAMP, which zeroes a negative alpha).
///
/// Returned lines carry their own inner indentation and include the enclosing braces; callers
/// add their own outer indentation (the same contract as [`super::moncurve::emit`]).
pub fn exponent(value: [f32; 4], style: NegativeStyle, d: &Dialect) -> Vec<String> {
    let g = format!(
        "{}({}, {}, {})",
        d.v3,
        n(value[0] as f64),
        n(value[1] as f64),
        n(value[2] as f64)
    );
    let pos = format!("pow(max(color.rgb, {}), {g})", d.k(0.0));
    let rgb = match style {
        NegativeStyle::Clamp => pos,
        NegativeStyle::Mirror | NegativeStyle::Linear => {
            format!("sign(color.rgb) * pow(abs(color.rgb), {g})")
        }
        // `step(0, x)` is 1 for x >= 0: the power there, the input below.
        NegativeStyle::PassThru => d.mix("color.rgb", &pos, &d.step(0.0, "color.rgb")),
    };
    let mut out = vec![format!("{{ // Exponent ({style:?})"), d.set(&rgb)];
    if value[3] != 1.0 || style == NegativeStyle::Clamp {
        out.push(format!(
            "    color.a = {};",
            basic_scalar(d, "color.a", value[3], style)
        ));
    }
    out.push("}".to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::super::dialect::{GLSL, HLSL, METAL, WGSL};
    use super::*;
    use vfx_math::layout::{Mat4, RowMajor};

    const STYLES: [NegativeStyle; 4] = [
        NegativeStyle::Clamp,
        NegativeStyle::Mirror,
        NegativeStyle::PassThru,
        NegativeStyle::Linear,
    ];

    #[test]
    fn wgsl_bodies_parse() {
        #[rustfmt::skip]
        let m = Mat4::<RowMajor>::from_array([
            0.9, 0.05, 0.02, 0.1,  0.03, 1.1, 0.0, -0.05,  0.01, 0.02, 0.8, 0.2,  0.25, -0.1, 0.05, 0.7,
        ]);
        let matrix_body = super::super::ops::matrix(&WGSL, &m, [0.0, 0.0, 0.0, 0.05]).join("\n");
        for style in STYLES {
            for alpha in [1.0, 2.0] {
                let body = exponent([1.2, 1.1, 0.9, alpha], style, &WGSL).join("\n");
                let src = format!(
                    "@fragment\nfn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {{\n\
                     var color = vec4<f32>(uv.x, uv.y, 0.5, 1.0);\n{body}\n{matrix_body}\nreturn color;\n}}\n"
                );
                if let Err(e) = naga::front::wgsl::parse_str(&src) {
                    panic!("naga rejected {style:?}:\n{e}\n\n{src}");
                }
            }
        }
    }

    #[test]
    fn all_dialects_emit_the_same_statement_count() {
        for style in STYLES {
            let counts: Vec<usize> = [&GLSL, &HLSL, &METAL, &WGSL]
                .iter()
                .map(|d| exponent([1.2, 1.1, 0.9, 2.0], style, d).len())
                .collect();
            assert!(
                counts.windows(2).all(|w| w[0] == w[1]),
                "{style:?}: {counts:?}"
            );
        }
    }

    /// The exponent's alpha line only when it acts.
    #[test]
    fn alpha_term_only_when_it_acts() {
        let mirror = exponent([2.0, 2.0, 2.0, 1.0], NegativeStyle::Mirror, &GLSL).join("\n");
        assert!(!mirror.contains("color.a ="), "{mirror}");
        let clamp = exponent([2.0, 2.0, 2.0, 1.0], NegativeStyle::Clamp, &GLSL).join("\n");
        assert!(clamp.contains("color.a ="), "{clamp}");
    }
}
