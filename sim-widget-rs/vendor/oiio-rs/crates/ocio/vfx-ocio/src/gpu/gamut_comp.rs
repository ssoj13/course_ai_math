//! Shared, language-independent emission of OCIO's `ACES_GAMUT_COMP_13` FixedFunction (the
//! "ACES-LMT - ACES 1.3 Reference Gamut Compression" builtin).
//!
//! The four backends each carried an invented curve instead: they read `params[0]` / `params[1]`
//! as a single threshold and limit (they are the cyan and magenta LIMITS; the thresholds are
//! `params[3..6]`), compressed each channel's VALUE rather than its distance from the achromatic
//! axis, and ignored the power. The GPU LMT therefore changed in-gamut colours (0.02 -> 0.167
//! on the CPU/OCIO, left at 0.02 on the GPU) and flattened HDR values to ~1.2. This is a port of
//! OCIO's own shader (`Add_GamutComp_13_Shader`, `FixedFunctionOpGPU.cpp:217-302`) with the
//! parameters from [`GamutComp13Params`], the decoder the CPU apply uses.

use super::dialect::{Dialect, n};
use crate::processor::op::GamutComp13Params;

fn v3(d: &Dialect, v: [f32; 3]) -> String {
    format!(
        "{}({}, {}, {})",
        d.v3,
        n(v[0] as f64),
        n(v[1] as f64),
        n(v[2] as f64)
    )
}

/// Emits the shader body; `forward` compresses, inverse uncompresses.
///
/// Returned lines carry their own inner indentation and include the enclosing braces; callers
/// add their own outer indentation (the same contract as [`super::transfer::emit`]).
pub fn emit(params: &[f32], forward: bool, d: &Dialect) -> Vec<String> {
    let p = GamutComp13Params::from_params(params);
    let (thr, scl) = (v3(d, p.thr), v3(d, p.scale));
    let power = d.k(p.power as f64);
    let inv_power = d.k(1.0 / p.power as f64);
    let zero = d.k(0.0);
    let mut out = vec![
        format!(
            "{{ // ACES_GAMUT_COMP_13 {}",
            if forward { "compress" } else { "uncompress" }
        ),
        // Achromatic axis, and each channel's distance from it (inverse RGB ratios).
        d.decl_f("ach", "max(color.r, max(color.g, color.b))"),
        d.decl(
            "dist",
            &format!(
                "({} - color.rgb) / {}",
                d.splat("ach"),
                d.splat("max(abs(ach), 1e-30)")
            ),
        ),
    ];
    if forward {
        // Compress at and above the threshold.
        out.push(d.decl("nd", &format!("max(dist - {thr}, {zero}) / {scl}")));
        out.push(d.decl(
            "cd",
            &format!(
                "{thr} + {scl} * nd / pow({} + pow(nd, {power}), {inv_power})",
                d.k(1.0)
            ),
        ));
        out.push(d.decl("sel", &format!("step({thr}, dist)")));
    } else {
        // Uncompress between the threshold and the asymptote at thr + scale; above it the
        // distance is left alone (the reference's "avoid singularity").
        out.push(d.decl(
            "nd",
            &format!("clamp((dist - {thr}) / {scl}, {zero}, {})", d.k(0.999999)),
        ));
        out.push(d.decl("pn", &format!("pow(nd, {power})")));
        out.push(d.decl(
            "cd",
            &format!("{thr} + {scl} * pow(pn / ({} - pn), {inv_power})", d.k(1.0)),
        ));
        out.push(d.decl(
            "sel",
            &format!(
                "step({thr}, dist) * ({} - step({thr} + {scl}, dist))",
                d.k(1.0)
            ),
        ));
    }
    out.push(d.decl("cdist", &d.mix("dist", "cd", "sel")));
    // Recalculate RGB from the compressed distance; black stays black (ach == 0).
    out.push(d.set(&d.sel(
        "ach == 0.0",
        &zero,
        &format!("{} - cdist * {}", d.splat("ach"), d.splat("abs(ach)")),
    )));
    out.push("}".to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::super::dialect::{GLSL, HLSL, METAL, WGSL};
    use super::*;

    const PARAMS: [f32; 7] = [1.147, 1.264, 1.312, 0.815, 0.803, 0.880, 1.2];

    #[test]
    fn wgsl_bodies_parse() {
        for forward in [true, false] {
            let body = emit(&PARAMS, forward, &WGSL).join("\n");
            let src = format!(
                "@fragment\nfn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {{\n\
                 var color = vec4<f32>(uv.x, uv.y, 0.5, 1.0);\n{body}\nreturn color;\n}}\n"
            );
            if let Err(e) = naga::front::wgsl::parse_str(&src) {
                panic!("naga rejected forward={forward}:\n{e}\n\n{src}");
            }
        }
    }

    #[test]
    fn all_dialects_emit_the_same_statement_count() {
        for forward in [true, false] {
            let counts: Vec<usize> = [&GLSL, &HLSL, &METAL, &WGSL]
                .iter()
                .map(|d| emit(&PARAMS, forward, d).len())
                .collect();
            assert!(counts.windows(2).all(|w| w[0] == w[1]), "{counts:?}");
        }
    }
}
