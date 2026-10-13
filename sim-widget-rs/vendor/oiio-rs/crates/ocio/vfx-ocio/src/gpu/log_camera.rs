//! Shared, language-independent emission of OCIO's camera log (`GpuOp::LogCamera`,
//! `LogOpData` with a `linSideBreak`).
//!
//! The four backends each carried a copy that emitted the linear toe as `linear_slope * x` with
//! no offset and placed the decode break at `linear_slope * lin_side_break` — so every camera
//! curve (ACEScct, ALEXA LogC, LogC4, S-Log3, V-Log, Log3G10) was wrong below its break on the
//! GPU, e.g. ACEScct 0.02 decoded to +0.0019 against the CPU's (and OCIO's) -0.0050. The toe's
//! offset and the encoded-domain break are now the ones `processor/compile.rs` derives once
//! (OCIO `LogUtil::GetLinearSlope` / `GetLinearOffset`, `LogUtils.cpp:255-286`) and carries on
//! the op, and the body is written once here, parameterized by [`Dialect`].

use super::dialect::{Dialect, n};

/// The camera-log parameters as the processor compiled them (per channel).
pub struct LogCameraParams {
    pub base: f32,
    pub log_side_slope: [f32; 3],
    pub log_side_offset: [f32; 3],
    pub lin_side_slope: [f32; 3],
    pub lin_side_offset: [f32; 3],
    pub lin_side_break: [f32; 3],
    pub linear_slope: [f32; 3],
    /// `log_side_slope * log(lin_side_slope * lin_side_break + lin_side_offset) + log_side_offset`.
    pub log_side_break: [f32; 3],
    /// `log_side_break - linear_slope * lin_side_break`.
    pub linear_offset: [f32; 3],
}

fn v3(d: &Dialect, v: [f32; 3]) -> String {
    format!(
        "{}({}, {}, {})",
        d.v3,
        n(v[0] as f64),
        n(v[1] as f64),
        n(v[2] as f64)
    )
}

/// Emits the shader body. `forward` is lin -> log (encode), as `ProcessorOp::LogCamera`.
///
/// Returned lines carry their own inner indentation and include the enclosing braces; callers
/// add their own outer indentation (the same contract as [`super::transfer::emit`]).
pub fn emit(p: &LogCameraParams, forward: bool, d: &Dialect) -> Vec<String> {
    let ln_base = (p.base as f64).ln();
    // log_base(x) = ln(x) / ln(base): fold 1/ln(base) into the log-side slope.
    let log_scale: [f32; 3] = p.log_side_slope.map(|s| (s as f64 / ln_base) as f32);
    if forward {
        // x >= lin_side_break: log segment; below: linear toe (LogOpCPU.cpp, CameraL2A).
        vec![
            "{ // LogCamera encode".to_string(),
            d.decl(
                "lg",
                &format!(
                    "{} * {} + {}",
                    v3(d, log_scale),
                    d.ln_log_op(&format!(
                        "{} * color.rgb + {}",
                        v3(d, p.lin_side_slope),
                        v3(d, p.lin_side_offset)
                    )),
                    v3(d, p.log_side_offset)
                ),
            ),
            d.decl(
                "li",
                &format!(
                    "{} * color.rgb + {}",
                    v3(d, p.linear_slope),
                    v3(d, p.linear_offset)
                ),
            ),
            d.set(&d.mix(
                "li",
                "lg",
                &format!("step({}, color.rgb)", v3(d, p.lin_side_break)),
            )),
            "}".to_string(),
        ]
    } else {
        // y >= log_side_break: inverse log; below: inverse linear toe (CameraA2L).
        let exponent = format!(
            "(color.rgb - {}) / {} * {}",
            v3(d, p.log_side_offset),
            v3(d, p.log_side_slope),
            n(ln_base)
        );
        vec![
            "{ // LogCamera decode".to_string(),
            d.decl(
                "lg",
                &format!(
                    "({} - {}) / {}",
                    d.exp(&exponent),
                    v3(d, p.lin_side_offset),
                    v3(d, p.lin_side_slope)
                ),
            ),
            d.decl(
                "li",
                &format!(
                    "(color.rgb - {}) / {}",
                    v3(d, p.linear_offset),
                    v3(d, p.linear_slope)
                ),
            ),
            d.set(&d.mix(
                "li",
                "lg",
                &format!("step({}, color.rgb)", v3(d, p.log_side_break)),
            )),
            "}".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::super::dialect::{GLSL, HLSL, METAL, WGSL};
    use super::*;

    /// ACEScct (`ACES.cpp` `ACEScct_to_LINEAR`), with the toe the processor derives.
    fn acescct() -> LogCameraParams {
        let (lss, lso, brk) = (1.0f32 / 17.52, 9.72f32 / 17.52, 0.0078125f32);
        let log_brk = lss * brk.log2() + lso;
        let lin_slope = lss / (brk * std::f32::consts::LN_2);
        LogCameraParams {
            base: 2.0,
            log_side_slope: [lss; 3],
            log_side_offset: [lso; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            lin_side_break: [brk; 3],
            linear_slope: [lin_slope; 3],
            log_side_break: [log_brk; 3],
            linear_offset: [log_brk - lin_slope * brk; 3],
        }
    }

    #[test]
    fn wgsl_bodies_parse() {
        for forward in [true, false] {
            let body = emit(&acescct(), forward, &WGSL).join("\n");
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
                .map(|d| emit(&acescct(), forward, d).len())
                .collect();
            assert!(counts.windows(2).all(|w| w[0] == w[1]), "{counts:?}");
        }
    }
}
