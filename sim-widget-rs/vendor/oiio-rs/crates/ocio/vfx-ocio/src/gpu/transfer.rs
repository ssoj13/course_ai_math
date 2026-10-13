//! Language-independent transfer-curve shader emission.
//!
//! # Why this module exists
//!
//! The GLSL, HLSL, Metal and WGSL emitters each carried their own hand-written copy of every
//! transfer curve. The copies were *textually equivalent* — which sounds reassuring and was in
//! fact the problem: when a curve was transcribed wrongly it was transcribed wrongly four times,
//! so cross-language comparison could never surface it, and a constant audit could not either
//! (the literals were individually plausible; it was the *shape of the formula* that was wrong).
//! Seven curves shipped visibly incorrect that way — see the per-style docs below.
//!
//! Nothing about a transfer curve is language-specific. Only the surface syntax differs, and the
//! differences are small enough to name explicitly ([`Dialect`]). So the math lives here exactly
//! once, and each backend supplies its dialect.
//!
//! # Authority
//!
//! Every curve here is derived from the corresponding `vfx-transfer` CPU module, which is the
//! verified side for all of these styles. Where the CPU derives a value (LogC4's linear segment,
//! the `LogCameraTransform` C1 extension) this module runs the *same* derivation in `f64` at
//! emission time rather than restating the result as a literal — a literal is precisely what rots,
//! and re-deriving is what keeps GPU bit-comparable to CPU when a parameter is corrected.
//!
//! # Scope
//!
//! [`emit`] covers every [`TransferStyle`], with no catch-all arm, so each backend's
//! `generate_transfer_*` is now a handful of lines that render whatever this module returns. A new
//! style is a compile error here — as opposed to the previous shape, where it could be added to
//! the enum and silently emit nothing in all four languages at once.
//!
//! # NaN discipline
//!
//! These curves are selected branchlessly with `mix`/`lerp`, so **both** sides of every selection
//! are always evaluated. `0.0 * inf` is `NaN`, so an unguarded `log` or an overflowing `exp` on the
//! *unselected* branch still poisons the result. That is how ACEScc and HLG came to produce NaN on
//! black. Every logarithm here is therefore floored ([`LOG_FLOOR`](super::dialect::LOG_FLOOR)) and every exponential is capped
//! ([`Dialect::exp`], [`Dialect::exp2`], [`Dialect::pow10`]), whether or not the guarded branch is
//! reachable for in-range input.

use super::dialect::{Dialect, n};
#[cfg(test)]
use super::dialect::{GLSL, HLSL, METAL, WGSL};
use crate::processor::TransferStyle;

// ---------------------------------------------------------------------------
// Generic LogCamera curve
// ---------------------------------------------------------------------------

/// The OCIO `LogCameraTransform` parameter set, mirroring
/// `vfx_transfer::log_camera::LogCameraTransform`.
///
/// Kept as the *primary* parameters, with the linear extension re-derived in [`log_camera`]
/// exactly as `LogCameraTransform::build` does, so the two sides cannot drift.
struct LogCamera {
    base: f64,
    log_slope: f64,
    log_offset: f64,
    lin_slope: f64,
    lin_offset: f64,
    lin_break: f64,
}

/// Emits an OCIO `LogCameraTransform` curve.
///
/// Both RED Log3G10 and Blackmagic Film Gen5 are exactly this transform on the CPU
/// (`red_log.rs`, `bmd_film.rs`), differing only in parameters and log base, so they share one
/// emitted code path here for the same reason they share one CPU struct.
///
/// The C1-continuous linear extension below `lin_break` is re-derived here rather than restated:
/// its slope is the derivative of the log leg at the break, and its offset follows from C0
/// continuity. Both previous emitters had invented their own linear leg instead, which is how
/// Log3G10 lost RED's published 0.09155 black point and BMD Film's toe ended up ~90x too steep.
fn log_camera(p: &LogCamera, forward: bool, d: &Dialect, label: &str) -> Vec<String> {
    let ln_base = p.base.ln();
    let lin_at_break = p.lin_slope * p.lin_break + p.lin_offset;
    let log_break = p.log_slope * lin_at_break.ln() / ln_base + p.log_offset;
    let ext_slope = p.log_slope * p.lin_slope / (ln_base * lin_at_break);
    let ext_offset = log_break - ext_slope * p.lin_break;

    // Folded so the shader does one multiply instead of a divide by ln(base) per pixel.
    let log_scale = p.log_slope / ln_base;

    if forward {
        let inner = format!("color.rgb * {} + {}", n(p.lin_slope), n(p.lin_offset));
        vec![
            format!("{{ // {label} encode"),
            d.decl(
                "lg",
                &format!("{} * {} + {}", n(log_scale), d.ln(&inner), n(p.log_offset)),
            ),
            d.decl(
                "ext",
                &format!("color.rgb * {} + {}", n(ext_slope), n(ext_offset)),
            ),
            d.set(&d.mix("ext", "lg", &d.step(p.lin_break, "color.rgb"))),
            "}".to_string(),
        ]
    } else {
        let exponent = format!("(color.rgb - {}) / {}", n(p.log_offset), n(log_scale));
        vec![
            format!("{{ // {label} decode"),
            d.decl(
                "lg",
                &format!(
                    "({} - {}) / {}",
                    d.exp(&exponent),
                    n(p.lin_offset),
                    n(p.lin_slope)
                ),
            ),
            d.decl(
                "ext",
                &format!("(color.rgb - {}) / {}", n(ext_offset), n(ext_slope)),
            ),
            d.set(&d.mix("ext", "lg", &d.step(log_break, "color.rgb"))),
            "}".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Emits the shader body for `style` in the given direction.
///
/// Returned lines carry their own inner indentation and include the enclosing braces; callers add
/// their own outer indentation. `Linear` is the identity and correctly yields no lines.
///
/// The match is exhaustive by construction — no catch-all arm — so adding a `TransferStyle` is a
/// compile error here rather than a style that silently emits nothing in every shading language.
pub fn emit(style: TransferStyle, forward: bool, d: &Dialect) -> Vec<String> {
    match style {
        TransferStyle::Linear => Vec::new(),
        TransferStyle::Srgb => toe_power(&SRGB, forward, d, "sRGB"),
        TransferStyle::Rec709 => toe_power(&REC709, forward, d, "Rec.709"),
        TransferStyle::Rec2020 => toe_power(&REC2020, forward, d, "Rec.2020"),
        TransferStyle::Gamma22 => pure_gamma(2.2, forward, d),
        TransferStyle::Gamma24 => pure_gamma(2.4, forward, d),
        TransferStyle::Gamma26 => pure_gamma(2.6, forward, d),
        TransferStyle::Rec1886 => pure_gamma(2.4, forward, d),
        TransferStyle::Pq => pq(forward, d),
        TransferStyle::Hlg => hlg(forward, d),
        TransferStyle::AcesCct => aces_cct(forward, d),
        TransferStyle::AcesCc => aces_cc(forward, d),
        TransferStyle::LogC3 => log_c3(forward, d),
        TransferStyle::LogC4 => log_c4(forward, d),
        TransferStyle::SLog3 => s_log3(forward, d),
        TransferStyle::VLog => v_log(forward, d),
        TransferStyle::Log3G10 => log3g10(forward, d),
        TransferStyle::BmdFilmGen5 => bmd_film_gen5(forward, d),
        TransferStyle::AppleLog => apple_log(forward, d),
        TransferStyle::CanonCLog2 => canon_clog2(forward, d),
        TransferStyle::CanonCLog3 => canon_clog3(forward, d),
    }
}

// ---------------------------------------------------------------------------
// Display curves: pure gamma, and linear-toe-plus-power
// ---------------------------------------------------------------------------

/// A pure power law, `y = x^(1/g)` forward.
///
/// Covers Gamma 2.2/2.4/2.6 and Rec.1886 (which is gamma 2.4 for display purposes).
///
/// Mirrored (PLAN9.md O2): `vfx_transfer::gamma::{gamma_oetf,gamma_eotf}` is the CPU oracle and
/// mirrors negative input (`sign(x) * |x|^e`) rather than clamping it to zero - ACEScg legitimately
/// carries negative components (§3 rule 2: carried, not clamped), so clamping here disagreed with
/// the oracle on exactly the domain that matters.
fn pure_gamma(g: f64, forward: bool, d: &Dialect) -> Vec<String> {
    let e = if forward { 1.0 / g } else { g };
    vec![d.set(&format!(
        "sign(color.rgb) * pow(abs(color.rgb), {})",
        d.k(e)
    ))]
}

/// Parameters of a "linear toe below a break, offset power law above" display curve.
struct ToePower {
    /// Slope of the linear toe.
    toe_slope: f64,
    /// Gain on the power leg.
    gain: f64,
    /// Exponent of the power leg, in the forward (encode) direction.
    power: f64,
    /// Break point in the linear domain.
    lin_break: f64,
}

/// sRGB (IEC 61966-2-1).
const SRGB: ToePower = ToePower {
    toe_slope: 12.92,
    gain: 1.055,
    power: 1.0 / 2.4,
    lin_break: 0.0031308,
};

/// ITU-R BT.709-6.
const REC709: ToePower = ToePower {
    toe_slope: 4.5,
    gain: 1.099,
    power: 0.45,
    lin_break: 0.018,
};

/// ITU-R BT.2020-2, with the higher-precision alpha/beta the 12-bit variant specifies.
const REC2020: ToePower = ToePower {
    toe_slope: 4.5,
    gain: 1.09929682680944,
    power: 0.45,
    lin_break: 0.018053968510807,
};

/// Emits a [`ToePower`] curve in either direction.
///
/// Mirrored (PLAN9.md O2): the CPU oracle (`vfx_transfer::srgb`/`rec709`/`bt2020`) is
/// `if x < 0 { -f(-x) } else { f(x) }`, so branch selection must key off `abs(color.rgb)`, not the
/// raw signed value. Selecting on the raw value (as this used to) picks the wrong branch for any
/// negative input past the toe break - e.g. sRGB OETF at `-0.5` came out `-6.46` (linear branch)
/// against the oracle's `-0.735` (mirrored power branch), an 8x divergence measured at the parity
/// test's proposed extended domain.
fn toe_power(p: &ToePower, forward: bool, d: &Dialect, label: &str) -> Vec<String> {
    let offset = p.gain - 1.0;
    if forward {
        vec![
            format!("{{ // {label} OETF"),
            d.decl("ac", "abs(color.rgb)"),
            d.decl("sgn", "sign(color.rgb)"),
            d.decl("lo", &format!("ac * {}", n(p.toe_slope))),
            d.decl(
                "hi",
                &format!("{} * pow(ac, {}) - {}", n(p.gain), d.k(p.power), n(offset)),
            ),
            d.set(&format!(
                "sgn * {}",
                d.mix("lo", "hi", &d.step(p.lin_break, "ac"))
            )),
            "}".to_string(),
        ]
    } else {
        vec![
            format!("{{ // {label} EOTF"),
            d.decl("ac", "abs(color.rgb)"),
            d.decl("sgn", "sign(color.rgb)"),
            d.decl("lo", &format!("ac / {}", n(p.toe_slope))),
            d.decl(
                "hi",
                &format!(
                    "pow(max((ac + {}) / {}, {}), {})",
                    n(offset),
                    n(p.gain),
                    d.k(0.0),
                    d.k(1.0 / p.power)
                ),
            ),
            d.set(&format!(
                "sgn * {}",
                d.mix("lo", "hi", &d.step(p.lin_break * p.toe_slope, "ac"))
            )),
            "}".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// PQ
// ---------------------------------------------------------------------------

/// SMPTE ST 2084 (PQ) in absolute cd/m2 — `vfx_transfer::pq::{oetf, eotf}`, over the rationals in
/// [`super::pq`].
///
/// The domain wrapper is the CPU's: encode clamps `L / 10000` to `[0, 1]` and maps `L <= 0` to
/// exactly 0 (the bare rational gives `c1^m2 ~ 7.4e-7` there); decode clamps the signal to
/// `[0, 1]`, so `v <= 0` is 0 and `v > 1` is 10000 rather than NaN.
fn pq(forward: bool, d: &Dialect) -> Vec<String> {
    // `step(0, -x)` is 1 exactly where `x <= 0`.
    let at_or_below_zero = format!("step({}, -color.rgb)", d.k(0.0));
    let mut body = Vec::new();
    if forward {
        body.push("{ // PQ OETF".to_string());
        body.extend(super::pq::encode(
            d,
            &format!("clamp(color.rgb / 10000.0, {}, {})", d.k(0.0), d.k(1.0)),
            "e",
        ));
        body.push(d.set(&d.mix("e", &d.k(0.0), &at_or_below_zero)));
    } else {
        body.push("{ // PQ EOTF".to_string());
        body.extend(super::pq::decode(
            d,
            &format!("clamp(color.rgb, {}, {})", d.k(0.0), d.k(1.0)),
            "y",
        ));
        body.push(d.set(&d.mix("10000.0 * y", &d.k(0.0), &at_or_below_zero)));
    }
    body.push("}".to_string());
    body
}

// ---------------------------------------------------------------------------
// ACEScct, LogC3, V-Log
// ---------------------------------------------------------------------------

/// ACEScct, per `vfx_transfer::acescct`. Constants verified exact; carried over unchanged.
fn aces_cct(forward: bool, d: &Dialect) -> Vec<String> {
    const CUT: f64 = 0.0078125;
    const A: f64 = 10.5402377416545;
    const B: f64 = 0.0729055341958355;
    const LOG_CUT: f64 = 0.155251141552511;

    if forward {
        vec![
            "{ // ACEScct encode".to_string(),
            d.decl("lo", &format!("{} * color.rgb + {}", n(A), n(B))),
            d.decl("hi", &format!("({} + 9.72) / 17.52", d.log2("color.rgb"))),
            d.set(&d.mix("lo", "hi", &d.step(CUT, "color.rgb"))),
            "}".to_string(),
        ]
    } else {
        vec![
            "{ // ACEScct decode".to_string(),
            d.decl("lo", &format!("(color.rgb - {}) / {}", n(B), n(A))),
            d.decl("hi", &d.exp2("color.rgb * 17.52 - 9.72")),
            d.set(&d.mix("lo", "hi", &d.step(LOG_CUT, "color.rgb"))),
            "}".to_string(),
        ]
    }
}

/// ARRI LogC3 at EI 800, per `vfx_transfer::log_c`. Formulas verified against CPU; carried over.
fn log_c3(forward: bool, d: &Dialect) -> Vec<String> {
    const CUT: f64 = 0.010591;
    const A: f64 = 5.555556;
    const B: f64 = 0.052272;
    const C: f64 = 0.247190;
    const D: f64 = 0.385537;
    const E: f64 = 5.367655;
    const F: f64 = 0.092809;

    if forward {
        vec![
            "{ // LogC3 encode".to_string(),
            d.decl("lo", &format!("{} * color.rgb + {}", n(E), n(F))),
            d.decl(
                "hi",
                &format!(
                    "{} * {} + {}",
                    n(C),
                    d.log10(&format!("{} * color.rgb + {}", n(A), n(B))),
                    n(D)
                ),
            ),
            d.set(&d.mix("lo", "hi", &d.step(CUT, "color.rgb"))),
            "}".to_string(),
        ]
    } else {
        vec![
            "{ // LogC3 decode".to_string(),
            d.decl("lo", &format!("(color.rgb - {}) / {}", n(F), n(E))),
            d.decl(
                "hi",
                &format!(
                    "({} - {}) / {}",
                    d.pow10(&format!("(color.rgb - {}) / {}", n(D), n(C))),
                    n(B),
                    n(A)
                ),
            ),
            d.set(&d.mix("lo", "hi", &d.step(E * CUT + F, "color.rgb"))),
            "}".to_string(),
        ]
    }
}

/// Panasonic V-Log, per `vfx_transfer::v_log`. Constants verified against CPU; carried over.
fn v_log(forward: bool, d: &Dialect) -> Vec<String> {
    const CUT: f64 = 0.01;
    const B: f64 = 0.00873;
    const C: f64 = 0.241514;
    const D: f64 = 0.598206;
    const TOE_SLOPE: f64 = 5.6;
    const TOE_OFFSET: f64 = 0.125;

    if forward {
        vec![
            "{ // V-Log encode".to_string(),
            d.decl(
                "lo",
                &format!("{} * color.rgb + {}", n(TOE_SLOPE), n(TOE_OFFSET)),
            ),
            d.decl(
                "hi",
                &format!(
                    "{} * {} + {}",
                    n(C),
                    d.log10(&format!("color.rgb + {}", n(B))),
                    n(D)
                ),
            ),
            d.set(&d.mix("lo", "hi", &d.step(CUT, "color.rgb"))),
            "}".to_string(),
        ]
    } else {
        vec![
            "{ // V-Log decode".to_string(),
            d.decl(
                "lo",
                &format!("(color.rgb - {}) / {}", n(TOE_OFFSET), n(TOE_SLOPE)),
            ),
            d.decl(
                "hi",
                &format!(
                    "{} - {}",
                    d.pow10(&format!("(color.rgb - {}) / {}", n(D), n(C))),
                    n(B)
                ),
            ),
            d.set(&d.mix(
                "lo",
                "hi",
                &d.step(TOE_SLOPE * CUT + TOE_OFFSET, "color.rgb"),
            )),
            "}".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// Sony S-Log3
// ---------------------------------------------------------------------------

/// Sony S-Log3, per `vfx_transfer::s_log3`.
///
/// The previous emitters had `(420 + log10(x * 261.5) * 261.5) / 1023`: the `261.5` log-side slope
/// had been substituted for the `(x + 0.01) / 0.19` lin-side transform, and the linear leg's slope
/// was the raw code-value delta `76.21` with Sony's `/ 0.01125` dropped. `encode(0.18)` came out
/// 0.838 against the specified 0.411, and decode was off by a factor of ~260.
///
/// S-Log3's linear leg is deliberately *not* C1-continuous with the log leg (it is a Sony design
/// choice, see `s_log3.rs`), so this cannot go through [`log_camera`].
fn s_log3(forward: bool, d: &Dialect) -> Vec<String> {
    const CUT: f64 = 0.01125;
    const CV_BREAK: f64 = 171.2102946929;
    const CV_BLACK: f64 = 95.0;
    // Sony's hardcoded linear-leg slope in code values per linear unit.
    let lin_slope = (CV_BREAK - CV_BLACK) / CUT;

    if forward {
        vec![
            "{ // S-Log3 encode".to_string(),
            d.decl(
                "lo",
                &format!("(color.rgb * {} + {}) / 1023.0", n(lin_slope), n(CV_BLACK)),
            ),
            d.decl(
                "hi",
                &format!(
                    "(420.0 + 261.5 * {}) / 1023.0",
                    d.log10("(color.rgb + 0.01) / 0.19")
                ),
            ),
            d.set(&d.mix("lo", "hi", &d.step(CUT, "color.rgb"))),
            "}".to_string(),
        ]
    } else {
        vec![
            "{ // S-Log3 decode".to_string(),
            d.decl("cv", "color.rgb * 1023.0"),
            // The linear leg is `(cv - 95) * 0.01125 / 76.2102946929`; the constants are folded
            // into the single divisor `lin_slope` used by the encode direction.
            d.decl("lo", &format!("(cv - {}) / {}", n(CV_BLACK), n(lin_slope))),
            d.decl(
                "hi",
                &format!("{} * 0.19 - 0.01", d.pow10("(cv - 420.0) / 261.5")),
            ),
            d.set(&d.mix("lo", "hi", &format!("step({}, cv)", d.k(CV_BREAK)))),
            "}".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// ARRI LogC4
// ---------------------------------------------------------------------------

/// ARRI LogC4, per `vfx_transfer::log_c4`.
///
/// The previous emitters had `0.9071 * ln(2231.82 * x + 1) / 8.735 + 0.0929`. ARRI's curve is
/// `0.0647954 * log2(2231.826 * x + 64) - 0.295908`: the lin-side offset is **64**, not 1, the log
/// is base 2, and the `/ 8.735` matches nothing derivable. The linear toe below the break was
/// missing entirely, replaced by a `max(x * A, 0)` clamp. `encode(0.18)` came out 0.716 against
/// 0.278.
///
/// LogC4 does not use [`log_camera`] for the same reason the CPU does not: base 2 and an
/// independently specified linear-side break.
fn log_c4(forward: bool, d: &Dialect) -> Vec<String> {
    const LIN_SLOPE: f64 = 2231.82630906769;
    const LIN_OFFSET: f64 = 64.0;
    const LOG_SLOPE: f64 = 0.0647954196341293;
    const LOG_OFFSET: f64 = -0.295908392682586;
    const LIN_BREAK: f64 = -0.0180569961199113;

    // Same derivation as `log_c4::compute_linear_segment`.
    let lin_at_break = LIN_SLOPE * LIN_BREAK + LIN_OFFSET;
    let log_break = LOG_SLOPE * lin_at_break.log2() + LOG_OFFSET;
    let toe_slope = LOG_SLOPE * LIN_SLOPE / (lin_at_break * std::f64::consts::LN_2);
    let toe_offset = log_break - toe_slope * LIN_BREAK;

    if forward {
        let inner = format!("color.rgb * {} + {}", n(LIN_SLOPE), n(LIN_OFFSET));
        vec![
            "{ // LogC4 encode".to_string(),
            d.decl(
                "lg",
                &format!("{} * {} + {}", n(LOG_SLOPE), d.log2(&inner), n(LOG_OFFSET)),
            ),
            d.decl(
                "toe",
                &format!("color.rgb * {} + {}", n(toe_slope), n(toe_offset)),
            ),
            d.set(&d.mix("toe", "lg", &d.step(LIN_BREAK, "color.rgb"))),
            "}".to_string(),
        ]
    } else {
        let exponent = format!("(color.rgb - {}) / {}", n(LOG_OFFSET), n(LOG_SLOPE));
        vec![
            "{ // LogC4 decode".to_string(),
            d.decl(
                "lg",
                &format!(
                    "({} - {}) / {}",
                    d.exp2(&exponent),
                    n(LIN_OFFSET),
                    n(LIN_SLOPE)
                ),
            ),
            d.decl(
                "toe",
                &format!("(color.rgb - {}) / {}", n(toe_offset), n(toe_slope)),
            ),
            d.set(&d.mix("toe", "lg", &d.step(log_break, "color.rgb"))),
            "}".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// RED Log3G10 / Blackmagic Film Gen5
// ---------------------------------------------------------------------------

/// RED Log3G10, per `vfx_transfer::red_log`.
///
/// The previous emitters applied the `0.01` lin-side offset to the curve's **output** and mirrored
/// the whole curve around zero, giving `encode(0) = 0.01`. The offset belongs *inside* the log
/// (`log10(155.975 * x + 2.5598)`), which is what produces RED's published 0.09155 black point,
/// and below the `-0.01` break the curve continues along its C1 linear extension rather than
/// reflecting.
fn log3g10(forward: bool, d: &Dialect) -> Vec<String> {
    const LIN_SLOPE: f64 = 155.975327;
    let p = LogCamera {
        base: 10.0,
        log_slope: 0.224282,
        log_offset: 0.0,
        lin_slope: LIN_SLOPE,
        // OCIO states this as `0.01 * linSideSlope + 1.0`; keeping the expression is what makes
        // the input-side placement of the 0.01 legible at a glance.
        lin_offset: 0.01 * LIN_SLOPE + 1.0,
        lin_break: -0.01,
    };
    log_camera(&p, forward, d, "Log3G10")
}

/// Blackmagic Design Film Gen5, per `vfx_transfer::bmd_film`.
///
/// The previous emitters had the constants scrambled across roles: the linear-side offset
/// `0.09246575` was used as both the toe's slope and its offset, and the log-side *offset*
/// `0.53001334` was used as the log-side *slope* (true slope `0.08692876`). The toe came out
/// roughly 90x off and `encode(0.18)` gave 0.412 against 0.384.
fn bmd_film_gen5(forward: bool, d: &Dialect) -> Vec<String> {
    let p = LogCamera {
        base: std::f64::consts::E,
        log_slope: 0.0869287606549122,
        log_offset: 0.530013339229194,
        lin_slope: 1.0,
        lin_offset: 0.00549407243225781,
        lin_break: 0.005,
    };
    log_camera(&p, forward, d, "BMD Film Gen5")
}

// ---------------------------------------------------------------------------
// Apple Log
// ---------------------------------------------------------------------------

/// Apple Log, per `vfx_transfer::apple_log`.
///
/// The previous emitters used a *linear* toe and `0.2568629 * ln(x - R0 + 1) + 0.486665` for the
/// log leg — constants that appear in no Apple document. The specified toe is the quadratic
/// `47.28711236 * (x - R0)^2` and the log leg is base 2. `encode(0.18)` came out 0.541 against
/// 0.488.
///
/// Both directions collapse Apple's three segments into two branchless selections: below the
/// mirror point `R_0` the `max(..., 0)` inside the quadratic already yields the specified constant
/// result, so no third branch is needed.
fn apple_log(forward: bool, d: &Dialect) -> Vec<String> {
    const R0: f64 = -0.05641088;
    const RT: f64 = 0.01;
    const C: f64 = 47.28711236;
    const BETA: f64 = 0.00964052;
    const GAMMA: f64 = 0.08550479;
    const DELTA: f64 = 0.69336945;
    let pt = C * (RT - R0) * (RT - R0);

    if forward {
        vec![
            "{ // Apple Log encode".to_string(),
            // max(..., 0) makes the quadratic leg evaluate to exactly 0 at and below R_0, which is
            // the specified value there -- so the "below mirror point" segment needs no branch.
            d.decl("t", &format!("max(color.rgb - {}, {})", n(R0), d.k(0.0))),
            d.decl("toe", &format!("{} * t * t", n(C))),
            d.decl(
                "lg",
                &format!(
                    "{} * {} + {}",
                    n(GAMMA),
                    d.log2(&format!("color.rgb + {}", n(BETA))),
                    n(DELTA)
                ),
            ),
            d.set(&d.mix("toe", "lg", &d.step(RT, "color.rgb"))),
            "}".to_string(),
        ]
    } else {
        vec![
            "{ // Apple Log decode".to_string(),
            // Likewise: max(color.rgb, 0) makes the sqrt leg return R_0 for negative input, which
            // is the specified clamp, folding Apple's lower two segments into one expression.
            d.decl(
                "toe",
                &format!("sqrt(max(color.rgb, {}) / {}) + {}", d.k(0.0), n(C), n(R0)),
            ),
            d.decl(
                "lg",
                &format!(
                    "{} - {}",
                    d.exp2(&format!("(color.rgb - {}) / {}", n(DELTA), n(GAMMA))),
                    n(BETA)
                ),
            ),
            d.set(&d.mix("toe", "lg", &d.step(pt, "color.rgb"))),
            "}".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// Canon Log 2 / Log 3
// ---------------------------------------------------------------------------

/// Canon Log 2, per `vfx_transfer::canon_log`.
///
/// The previous emitters had the slope and offset transposed (`CUT` 0.0928 used as the log slope,
/// `LOG_SLOPE` 0.2414 used as the offset), used the natural log where Canon specifies base 10,
/// dropped the `/ 0.9` reflectance normalisation, and had no negative mirroring at all.
/// `encode(0.18)` came out 0.503 against 0.398.
///
/// Canon mirrors the curve about the cut point, so `sign`/`abs` expresses both of the CPU's
/// branches in one expression. At `x == 0` the mirrored term is zero, so `sign(0) == 0` is
/// harmless.
fn canon_clog2(forward: bool, d: &Dialect) -> Vec<String> {
    const CUT: f64 = 0.092864125;
    const LOG_SLOPE: f64 = 0.24136077;
    const LIN_SCALE: f64 = 87.099375;
    const NORM: f64 = 0.9;

    if forward {
        vec![
            "{ // Canon C-Log2 encode".to_string(),
            d.decl("x", &format!("color.rgb / {}", n(NORM))),
            d.decl(
                "m",
                &format!(
                    "{} * {}",
                    n(LOG_SLOPE),
                    d.log10(&format!("abs(x) * {} + 1.0", n(LIN_SCALE)))
                ),
            ),
            d.set(&format!("{} + sign(x) * m", n(CUT))),
            "}".to_string(),
        ]
    } else {
        vec![
            "{ // Canon C-Log2 decode".to_string(),
            d.decl("dv", &format!("color.rgb - {}", n(CUT))),
            d.decl(
                "m",
                &format!(
                    "({} - 1.0) / {}",
                    d.pow10(&format!("abs(dv) / {}", n(LOG_SLOPE))),
                    n(LIN_SCALE)
                ),
            ),
            d.set(&format!("sign(dv) * m * {}", n(NORM))),
            "}".to_string(),
        ]
    }
}

/// Canon Log 3, per `vfx_transfer::canon_log`.
///
/// The previous emitters emitted a two-segment curve with constants (`A = 0.07623`, `B = 0.11603`,
/// `C = 0.31185`) matching nothing in the specification except `LIN_SCALE`. Canon Log 3 has
/// **three** segments — a mirrored log leg below `-0.014`, a genuine linear leg through `+-0.014`,
/// and a log leg above — with `/ 0.9` normalisation and base-10 logs throughout.
fn canon_clog3(forward: bool, d: &Dialect) -> Vec<String> {
    const CUT_LOW: f64 = 0.097465473;
    const CUT_HIGH: f64 = 0.15277891;
    const LOG_OFFSET_NEG: f64 = 0.12783901;
    const LOG_OFFSET_POS: f64 = 0.12240537;
    const LOG_SLOPE: f64 = 0.36726845;
    const LIN_SCALE: f64 = 14.98325;
    const LIN_SLOPE: f64 = 1.9754798;
    const LIN_OFFSET: f64 = 0.12512219;
    const NORM: f64 = 0.9;
    const LIN_BREAK: f64 = 0.014;

    if forward {
        vec![
            "{ // Canon C-Log3 encode".to_string(),
            d.decl("x", &format!("color.rgb / {}", n(NORM))),
            d.decl(
                "lg",
                &format!(
                    "{} * {}",
                    n(LOG_SLOPE),
                    d.log10(&format!("abs(x) * {} + 1.0", n(LIN_SCALE)))
                ),
            ),
            d.decl(
                "curve",
                &d.mix(
                    &format!("{} - lg", n(LOG_OFFSET_NEG)),
                    &format!("{} + lg", n(LOG_OFFSET_POS)),
                    &format!("step({}, x)", d.k(0.0)),
                ),
            ),
            d.decl("lin", &format!("x * {} + {}", n(LIN_SLOPE), n(LIN_OFFSET))),
            // `step(abs(x), LIN_BREAK)` is `|x| <= 0.014`, matching the CPU's inclusive linear
            // segment (its log legs are the strict `< -0.014` / `> 0.014` cases).
            d.set(&d.mix("curve", "lin", &format!("step(abs(x), {})", d.k(LIN_BREAK)))),
            "}".to_string(),
        ]
    } else {
        vec![
            "{ // Canon C-Log3 decode".to_string(),
            d.decl(
                "neg",
                &format!(
                    "-({} - 1.0) / {}",
                    d.pow10(&format!(
                        "({} - color.rgb) / {}",
                        n(LOG_OFFSET_NEG),
                        n(LOG_SLOPE)
                    )),
                    n(LIN_SCALE)
                ),
            ),
            d.decl(
                "pos",
                &format!(
                    "({} - 1.0) / {}",
                    d.pow10(&format!(
                        "(color.rgb - {}) / {}",
                        n(LOG_OFFSET_POS),
                        n(LOG_SLOPE)
                    )),
                    n(LIN_SCALE)
                ),
            ),
            d.decl(
                "lin",
                &format!("(color.rgb - {}) / {}", n(LIN_OFFSET), n(LIN_SLOPE)),
            ),
            d.decl(
                "curve",
                &d.mix("neg", "pos", &d.step(CUT_HIGH, "color.rgb")),
            ),
            // Inside [CUT_LOW, CUT_HIGH] inclusive, matching the CPU's segment boundaries.
            d.decl(
                "inLin",
                &format!(
                    "{} * step(color.rgb, {})",
                    d.step(CUT_LOW, "color.rgb"),
                    d.k(CUT_HIGH)
                ),
            ),
            d.set(&format!("{} * {}", d.mix("curve", "lin", "inLin"), n(NORM))),
            "}".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// ACEScc
// ---------------------------------------------------------------------------

/// ACEScc, per `vfx_transfer::acescc`.
///
/// The math was already correct; the encode direction produced **NaN for every pixel <= 0**, which
/// is the single most common pixel value there is. `log2(0)` is `-inf`, and because the segments
/// are selected with `mix`, the unselected `-inf` branch is still multiplied by a zero weight —
/// `0 * -inf` is NaN, which then propagates. Flooring the logarithm argument (see the module's NaN
/// discipline note) makes the unselected branch finite; the selection itself was already picking
/// the correct `-0.3584474886` constant at black.
fn aces_cc(forward: bool, d: &Dialect) -> Vec<String> {
    if forward {
        vec![
            "{ // ACEScc encode".to_string(),
            d.decl("t", &format!("max(color.rgb, {})", d.k(0.0))),
            d.decl(
                "lo",
                &format!(
                    "({} + 9.72) / 17.52",
                    d.log2(&format!("{} + t * 0.5", d.k(2.0_f64.powi(-16))))
                ),
            ),
            d.decl("hi", &format!("({} + 9.72) / 17.52", d.log2("t"))),
            d.decl("seg", &d.mix("lo", "hi", &d.step(2.0_f64.powi(-15), "t"))),
            d.set(&d.mix(&d.k(-0.3584474886), "seg", &d.step(0.0, "color.rgb"))),
            "}".to_string(),
        ]
    } else {
        vec![
            "{ // ACEScc decode".to_string(),
            d.decl("p", &d.exp2("color.rgb * 17.52 - 9.72")),
            d.decl("lo", &format!("(p - {}) * 2.0", n(2.0_f64.powi(-16)))),
            d.set(&d.mix("lo", "p", &d.step(-0.3013698630, "color.rgb"))),
            "}".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// HLG
// ---------------------------------------------------------------------------

/// Hybrid Log-Gamma, per `vfx_transfer::hlg`.
///
/// Two defects, both on the unselected branch of a `mix`:
///
/// * **OETF produced NaN for every pixel below 0.0237, black included.** `A * log(12x - B)` has a
///   negative argument for `x < B/12 = 0.02372`; the resulting NaN survives the zero-weighted
///   `mix`. Every sibling curve floored its logarithm; HLG alone did not.
/// * **EOTF flipped the sign of negative input.** `x * x / 3` is positive for negative `x`, where
///   the CPU (`hlg.rs:60`) returns 0. Clamping before squaring restores that.
fn hlg(forward: bool, d: &Dialect) -> Vec<String> {
    const A: f64 = 0.17883277;
    const B: f64 = 0.28466892;
    const C: f64 = 0.55991073;

    if forward {
        vec![
            "{ // HLG OETF".to_string(),
            d.decl("lo", &format!("sqrt(3.0 * max(color.rgb, {}))", d.k(0.0))),
            d.decl(
                "hi",
                &format!(
                    "{} * {} + {}",
                    n(A),
                    d.ln(&format!("12.0 * color.rgb - {}", n(B))),
                    n(C)
                ),
            ),
            d.set(&d.mix("lo", "hi", &d.step(1.0 / 12.0, "color.rgb"))),
            "}".to_string(),
        ]
    } else {
        vec![
            "{ // HLG EOTF".to_string(),
            d.decl("e", &format!("max(color.rgb, {})", d.k(0.0))),
            d.decl("lo", "e * e / 3.0"),
            d.decl(
                "hi",
                &format!(
                    "({} + {}) / 12.0",
                    d.exp(&format!("(color.rgb - {}) / {}", n(C), n(A))),
                    n(B)
                ),
            ),
            d.set(&d.mix("lo", "hi", &d.step(0.5, "color.rgb"))),
            "}".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders a style in one direction as a single string, for structural assertions.
    fn text(style: TransferStyle, forward: bool) -> String {
        emit(style, forward, &GLSL).join("\n")
    }

    const ALL: [TransferStyle; 21] = [
        TransferStyle::Linear,
        TransferStyle::Srgb,
        TransferStyle::Rec709,
        TransferStyle::Rec2020,
        TransferStyle::Gamma22,
        TransferStyle::Gamma24,
        TransferStyle::Gamma26,
        TransferStyle::Rec1886,
        TransferStyle::Pq,
        TransferStyle::Hlg,
        TransferStyle::AcesCct,
        TransferStyle::AcesCc,
        TransferStyle::LogC3,
        TransferStyle::LogC4,
        TransferStyle::SLog3,
        TransferStyle::VLog,
        TransferStyle::Log3G10,
        TransferStyle::BmdFilmGen5,
        TransferStyle::AppleLog,
        TransferStyle::CanonCLog2,
        TransferStyle::CanonCLog3,
    ];

    /// All four dialects must emit the same number of statements for every style and direction.
    ///
    /// This is exactly the property the old four-copy layout was assumed to have and did not.
    /// Generating from one source makes it cheap to assert, and a failure would mean a dialect
    /// helper had introduced a structural difference between backends.
    #[test]
    fn dialects_agree_structurally() {
        for style in ALL {
            for forward in [true, false] {
                let expect = emit(style, forward, &GLSL).len();
                for (name, d) in [("HLSL", &HLSL), ("Metal", &METAL), ("WGSL", &WGSL)] {
                    assert_eq!(
                        emit(style, forward, d).len(),
                        expect,
                        "{name} emits a different statement count for {style:?} forward={forward}"
                    );
                }
            }
        }
    }

    /// Every emitted WGSL body must actually parse.
    ///
    /// WGSL is the dialect where this module's abstraction can go wrong in ways the C-family
    /// three cannot: it forbids assigning through a swizzle and requires matching operand types
    /// for `min`/`max`/`step`/`pow`/`clamp`, so an unsplatted scalar is a compile error rather
    /// than a portability nicety. Run the real front end over each body rather than trusting the
    /// splat helper by inspection.
    #[test]
    fn wgsl_bodies_parse() {
        for style in ALL {
            for forward in [true, false] {
                let body = emit(style, forward, &WGSL).join("\n");
                if body.is_empty() {
                    continue; // Linear is the identity and emits nothing.
                }
                let src = format!(
                    "@fragment\nfn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {{\n\
                     var color = vec4<f32>(uv.x, uv.y, 0.5, 1.0);\n{body}\nreturn color;\n}}\n"
                );
                if let Err(e) = naga::front::wgsl::parse_str(&src) {
                    panic!("naga rejected the WGSL for {style:?} forward={forward}:\n{e}\n\n{src}");
                }
            }
        }
    }

    /// LogC4's linear toe is derived here and in `vfx_transfer::log_c4` independently; they must
    /// agree, or the GPU toe is simply a different curve from the CPU toe.
    #[test]
    fn log_c4_toe_matches_cpu_derivation() {
        const LIN_SLOPE: f64 = 2231.82630906769;
        const LIN_OFFSET: f64 = 64.0;
        const LOG_SLOPE: f64 = 0.0647954196341293;
        const LOG_OFFSET: f64 = -0.295908392682586;
        const LIN_BREAK: f64 = -0.0180569961199113;

        let lin_at_break = LIN_SLOPE * LIN_BREAK + LIN_OFFSET;
        let log_break = LOG_SLOPE * lin_at_break.log2() + LOG_OFFSET;
        let cpu = vfx_transfer::log_c4::log_break();
        assert!(
            (log_break - cpu).abs() < 1e-12,
            "GPU log_break {log_break} != CPU {cpu}"
        );

        let fwd = text(TransferStyle::LogC4, true);
        assert!(
            fwd.contains("64.0"),
            "lin-side offset 64 must appear:\n{fwd}"
        );
        assert!(fwd.contains("log2"), "LogC4 is a base-2 curve:\n{fwd}");
    }

    /// The generic LogCamera C1 extension derived in [`log_camera`] must match
    /// `LogCameraTransform::build`, which is what the CPU uses for both Log3G10 and BMD Gen5.
    #[test]
    fn log_camera_extension_matches_cpu() {
        let cases = [
            (
                "Log3G10",
                10.0,
                0.224282,
                0.0,
                155.975327,
                0.01 * 155.975327 + 1.0,
                -0.01,
            ),
            (
                "BMD Gen5",
                std::f64::consts::E,
                0.0869287606549122,
                0.530013339229194,
                1.0,
                0.00549407243225781,
                0.005,
            ),
        ];
        for (label, base, log_slope, log_offset, lin_slope, lin_offset, lin_break) in cases {
            let cpu = vfx_transfer::log_camera::LogCameraTransform::build(
                base, log_slope, log_offset, lin_slope, lin_offset, lin_break,
            );

            let ln_base = base.ln();
            let lin_at_break = lin_slope * lin_break + lin_offset;
            let log_break = log_slope * lin_at_break.ln() / ln_base + log_offset;
            let ext_slope = log_slope * lin_slope / (ln_base * lin_at_break);
            let ext_offset = log_break - ext_slope * lin_break;

            assert!(
                (log_break - cpu.log_break).abs() < 1e-12,
                "{label} log_break"
            );
            assert!(
                (ext_slope - cpu.lin_ext_slope).abs() < 1e-9,
                "{label} lin_ext_slope"
            );
            assert!(
                (ext_offset - cpu.lin_ext_offset).abs() < 1e-12,
                "{label} lin_ext_offset"
            );
        }
    }

    /// RED publishes 0.09155 as Log3G10's black point. The old emitter produced 0.01 because it
    /// added the 0.01 offset to the curve's OUTPUT instead of its input, so pin the input-side
    /// placement by requiring the emitted lin-side offset to be `0.01 * linSideSlope + 1`.
    #[test]
    fn log3g10_offset_is_on_the_input_side() {
        let expected = n(0.01 * 155.975327 + 1.0);
        let fwd = text(TransferStyle::Log3G10, true);
        assert!(
            fwd.contains(&expected),
            "Log3G10 must fold 0.01 into the lin-side offset ({expected}); emitted:\n{fwd}"
        );
        let black = vfx_transfer::red_log::log3g10_encode(0.0);
        assert!(
            (black - 0.09155).abs() < 1e-4,
            "CPU anchor moved: log3g10_encode(0) = {black}"
        );
    }

    /// Every logarithm must be floored and every exponential capped, in every style and both
    /// directions.
    ///
    /// This is the invariant ACEScc and HLG each violated. Because the segments are selected with
    /// `mix`, an unguarded call on the *unselected* branch still poisons the result
    /// (`0 * inf == NaN`), so "that branch can't be taken for valid input" is not a defence.
    /// Asserting it structurally is what keeps it from returning in a style nobody is looking at.
    #[test]
    fn every_log_and_exp_is_guarded() {
        for style in ALL {
            for forward in [true, false] {
                let src = text(style, forward);
                for (call, guard) in [
                    ("log(", "max("),
                    ("log2(", "max("),
                    ("exp(", "min("),
                    ("exp2(", "min("),
                ] {
                    let mut from = 0;
                    while let Some(idx) = src[from..].find(call) {
                        let at = from + idx;
                        // `log(` also matches inside `log2(`/`log10(`, and `exp(` inside `exp2(`;
                        // those are scanned under their own entry, so skip them here.
                        let shadowed = call == "log(" && !src[at..].starts_with("log(")
                            || call == "exp(" && !src[at..].starts_with("exp(");
                        if !shadowed {
                            let arg = &src[at + call.len()..];
                            assert!(
                                arg.starts_with(guard),
                                "{style:?} forward={forward}: unguarded `{call}` -- expected a \
                                 leading `{guard}` (see the module's NaN discipline note), got:\n\
                                 ...{}",
                                &arg[..arg.len().min(70)]
                            );
                        }
                        from = at + call.len();
                    }
                }
            }
        }
    }

    /// Canon Log 3 has three segments; the old emitter had two, with constants matching nothing
    /// in the specification. Pin all three plus the `/0.9` normalisation it had dropped.
    #[test]
    fn canon_clog3_has_three_segments() {
        let fwd = text(TransferStyle::CanonCLog3, true);
        for marker in [&n(0.12783901), &n(0.12240537), &n(1.9754798)] {
            assert!(
                fwd.contains(marker),
                "C-Log3 forward is missing {marker}:\n{fwd}"
            );
        }
        assert!(
            fwd.contains(&format!("/ {}", n(0.9))),
            "C-Log3 must normalise by 0.9:\n{fwd}"
        );
    }

    /// Apple Log's toe is the specified quadratic `47.287 * (x - R0)^2`, not the linear ramp the
    /// old emitter used.
    #[test]
    fn apple_log_toe_is_quadratic() {
        let fwd = text(TransferStyle::AppleLog, true);
        assert!(
            fwd.contains("t * t"),
            "Apple Log toe must be quadratic:\n{fwd}"
        );
        assert!(
            fwd.contains(&n(47.28711236)),
            "Apple Log toe scale C:\n{fwd}"
        );
    }

    /// S-Log3's linear leg carries Sony's `/0.01125`, which the old emitter dropped -- it used the
    /// bare code-value delta 76.21 as the slope, ~600x too shallow.
    #[test]
    fn s_log3_linear_leg_includes_sony_divisor() {
        let expected = n((171.2102946929 - 95.0) / 0.01125);
        let fwd = text(TransferStyle::SLog3, true);
        assert!(
            fwd.contains(&expected),
            "S-Log3 linear slope must be {expected}:\n{fwd}"
        );
        let grey = vfx_transfer::s_log3::encode(0.18);
        assert!(
            (grey - 0.41).abs() < 0.02,
            "CPU anchor moved: s_log3 encode(0.18) = {grey}"
        );
    }
}
