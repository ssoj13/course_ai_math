//! RGB <-> HSY conversion for `GradingHueCurve`, emitted once for all four shading languages.
//!
//! # What was wrong (EMIT-13)
//!
//! Three separate defects, all four languages identically:
//!
//! 1. **A different algorithm entirely.** The emitters computed HSV-style saturation
//!    (`(max-min)/max`) and reconstructed RGB from a chroma/hue-sector triangle. The reference
//!    derives saturation from the **L1 distance to luma** (`dot(abs(rgb - luma), 1)`) and
//!    reconstructs through a clamped triangle wave renormalised to the target luma.
//! 2. **The hue zero-point differed.** The reference places hue zero at **magenta**, not red —
//!    the sector offsets are 1/3/5, not 0/2/4 (`FixedFunctionOpGPU.cpp:1735`, which notes this
//!    "allows for better placement of red when manipulating curves in a UI"). Hue curves authored
//!    against an OCIO config therefore landed on rotated hues.
//! 3. **The three styles were collapsed into one body.** `RgbToHsyLin`/`Log`/`Vid` mapped to
//!    identical emitted code, silently discarding the per-style saturation model: Linear blends a
//!    low- and high-luma saturation estimate and scales by 1.4, Log scales by 4, Video by 1.25.
//!
//! The old inverse was also not the inverse of its own forward — round-tripping pure red
//! `(1,0,0)` returned `(0.547, 0.122, 0.122)`.
//!
//! # Authority
//!
//! Transcribed from OCIO `src/OpenColorIO/ops/fixedfunction/FixedFunctionOpGPU.cpp`,
//! `Add_RGB_TO_HSY` (:1703-1744) and `Add_HSY_TO_RGB` (:1747-1800). The CPU (`src/hsy.rs`) is the
//! same algorithm, from the CPU renderer of the same file.
//!
//! The Linear inverse is the intricate part and is reproduced structurally rather than
//! paraphrased: inverting the low/high saturation blend is a quadratic in the saturation gain, and
//! the reference solves it with the numerically-stable root choice (`2c/(-discrim - b)`, falling
//! back to the conjugate form when that root comes out negative) rather than the textbook formula.
//! That choice is deliberate upstream and is preserved here.

use super::dialect::{Dialect, n};
use crate::transform::FixedFunctionStyle;

/// Rec.709 luma weights, matching `vfx_core::consts::luma` and the reference's own literals.
const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];

/// Which of the three grading styles an HSY conversion belongs to.
///
/// The styles differ only in how saturation is scaled, but they differ in *both* directions and
/// the inverse of the Linear case is not a simple reciprocal — so the distinction is carried as a
/// type rather than as a scale factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HsyStyle {
    /// Scene-linear: saturation blends a shadow and a highlight estimate, then scales by 1.4.
    Lin,
    /// Log: saturation is the L1 distance scaled by 4.
    Log,
    /// Video: saturation is the L1 distance scaled by 1.25.
    Vid,
}

impl HsyStyle {
    /// The style's label, for the emitted shader comment.
    fn label(self) -> &'static str {
        match self {
            Self::Lin => "LIN",
            Self::Log => "LOG",
            Self::Vid => "VID",
        }
    }
}

/// Emits the shader body for one of the six HSY fixed-function styles, or `None` if `style` is not
/// an HSY conversion.
///
/// Returned lines carry their own inner indentation and include the enclosing braces; callers add
/// their own outer indentation.
pub fn emit(style: FixedFunctionStyle, d: &Dialect) -> Option<Vec<String>> {
    let lines = match style {
        FixedFunctionStyle::RgbToHsyLin => rgb_to_hsy(HsyStyle::Lin, d),
        FixedFunctionStyle::RgbToHsyLog => rgb_to_hsy(HsyStyle::Log, d),
        FixedFunctionStyle::RgbToHsyVid => rgb_to_hsy(HsyStyle::Vid, d),
        FixedFunctionStyle::HsyLinToRgb => hsy_to_rgb(HsyStyle::Lin, d),
        FixedFunctionStyle::HsyLogToRgb => hsy_to_rgb(HsyStyle::Log, d),
        FixedFunctionStyle::HsyVidToRgb => hsy_to_rgb(HsyStyle::Vid, d),
        _ => return None,
    };
    Some(lines)
}

/// `dot(v, lumaWeights)` written out, so no `const vec3` declaration is needed per dialect.
fn luma_of(v: &str) -> String {
    format!(
        "({v}.r * {} + {v}.g * {} + {v}.b * {})",
        n(LUMA[0]),
        n(LUMA[1]),
        n(LUMA[2])
    )
}

/// `dot(abs(v - s), vec3(1))` — the L1 distance from `v` to the achromatic axis at level `s`.
///
/// This, not `(max-min)/max`, is the reference's saturation measure; writing it as a named helper
/// keeps the forward and inverse using demonstrably the same expression.
fn l1_dist(v: &str, s: &str) -> String {
    format!("(abs({v}.r - {s}) + abs({v}.g - {s}) + abs({v}.b - {s}))")
}

/// RGB -> HSY. Reference: `FixedFunctionOpGPU.cpp:1703-1744`.
fn rgb_to_hsy(style: HsyStyle, d: &Dialect) -> Vec<String> {
    let mut out = vec![format!("{{ // RGB to HSY ({})", style.label())];

    out.push(d.decl_f("luma", &luma_of("color")));
    out.push(d.decl_f("minRGB", "min(color.r, min(color.g, color.b))"));
    out.push(d.decl_f("maxRGB", "max(color.r, max(color.g, color.b))"));
    out.push(d.decl_f("distRGB", &l1_dist("color", "luma")));

    match style {
        HsyStyle::Lin => {
            // The linear style has no single saturation estimate that behaves near black and in
            // the highlights at once, so the reference blends two and cross-fades between them
            // over luma [0.001, 0.01].
            out.push(d.decl_f("sumRGB", "(color.r + color.g + color.b)"));
            out.push(d.decl_f(
                "sat_hi",
                "distRGB / max(0.07 * distRGB + 1e-6, 0.15 + sumRGB)",
            ));
            out.push(d.decl_f("sat_lo", "distRGB * 5.0"));
            out.push(d.decl_f("alpha", "clamp((luma - 0.001) / (0.01 - 0.001), 0.0, 1.0)"));
            out.push(d.decl_f("sat", "(sat_lo + alpha * (sat_hi - sat_lo)) * 1.4"));
        }
        HsyStyle::Log => out.push(d.decl_f("sat", "distRGB * 4.0")),
        HsyStyle::Vid => out.push(d.decl_f("sat", "distRGB * 1.25")),
    }

    // Hue zero is at MAGENTA, not red -- hence the 1/3/5 sector offsets rather than the 0/2/4 a
    // conventional HSV derivation uses. Reference: FixedFunctionOpGPU.cpp:1735-1742.
    out.push(d.var_f("hue", "0.0"));
    out.push("    if (minRGB != maxRGB) {".to_string());
    out.push(format!(
        "    {}",
        d.decl_f("invRange", "1.0 / (maxRGB - minRGB)")
    ));
    out.push(
        "        if (maxRGB == color.r) { hue = 1.0 + (color.g - color.b) * invRange; }"
            .to_string(),
    );
    out.push(
        "        else if (maxRGB == color.g) { hue = 3.0 + (color.b - color.r) * invRange; }"
            .to_string(),
    );
    out.push("        else { hue = 5.0 + (color.r - color.g) * invRange; }".to_string());
    out.push("    }".to_string());

    out.push(d.set(&format!("{}(hue * (1.0 / 6.0), sat, luma)", d.v3)));
    out.push("}".to_string());
    out
}

/// HSY -> RGB. Reference: `FixedFunctionOpGPU.cpp:1747-1800`.
fn hsy_to_rgb(style: HsyStyle, d: &Dialect) -> Vec<String> {
    let mut out = vec![format!("{{ // HSY to RGB ({})", style.label())];

    out.push(d.decl_f("luma", "color.b"));
    out.push(d.decl_f("sat", "color.g"));
    // Undo the magenta-at-zero offset, and flip by half a turn for negative luma so the hue stays
    // meaningful below black rather than folding onto itself.
    out.push(d.var_f("hue", "color.r - (1.0 / 6.0)"));
    out.push(format!(
        "    hue = {};",
        d.sel("luma < 0.0", "hue + 0.5", "hue")
    ));
    out.push("    hue = (hue - floor(hue)) * 6.0;".to_string());

    // Triangle-wave reconstruction, clamped to [0,1]: the reference's replacement for the
    // six-branch hue-sector chain the old emitters used. The clamp bounds are splatted because
    // WGSL requires all three `clamp` operands to share a type.
    let triangle = format!(
        "{}(abs(hue - 3.0) - 1.0, 2.0 - abs(hue - 2.0), 2.0 - abs(hue - 4.0))",
        d.v3
    );
    out.push(d.var3(
        "RGB0",
        &format!("clamp({triangle}, {}, {})", d.k(0.0), d.k(1.0)),
    ));
    // Renormalise the reconstructed hue triangle to carry the requested luma. `currY` is zero only
    // for a degenerate triangle, which `clamp` above cannot produce for any finite hue.
    out.push(d.decl_f("currY", &luma_of("RGB0")));
    out.push("    RGB0 = RGB0 * (luma / currY);".to_string());
    out.push(d.decl_f("distRGB", &l1_dist("RGB0", "luma")));

    match style {
        HsyStyle::Log => out.push(d.decl_f("gainS", "sat / max(1e-10, distRGB * 4.0)")),
        HsyStyle::Vid => out.push(d.decl_f("gainS", "sat / max(1e-10, distRGB * 1.25)")),
        HsyStyle::Lin => {
            // Inverting the forward blend `sat_lo + alpha*(sat_hi - sat_lo)` is a quadratic in the
            // gain, because sat_hi itself depends on the gained result. The reference solves it
            // with the numerically-stable root `2c / (-discrim - b)` and switches to the conjugate
            // form when that comes out negative; the endpoints alpha==0 and alpha==1 are taken
            // from the closed forms s0/s1 rather than the quadratic, which is singular there.
            out.push(d.decl_f("sumRGB", "(RGB0.r + RGB0.g + RGB0.b)"));
            out.push(d.decl_f("satL", "sat / 1.4"));
            out.push(d.decl_f(
                "tmp",
                "max(1e-6, -satL * sumRGB + satL * 3.0 * luma + distRGB)",
            ));
            out.push(d.decl_f("s1", "min(satL * (0.15 + 3.0 * luma) / tmp, 50.0)"));
            out.push(d.decl_f("s0", "satL / max(1e-10, distRGB * 5.0)"));
            out.push(d.decl_f("alpha", "clamp((luma - 0.001) / (0.01 - 0.001), 0.0, 1.0)"));
            out.push(d.decl_f(
                "qa",
                "distRGB * 5.0 * (1.0 - alpha) * (sumRGB - 3.0 * luma)",
            ));
            out.push(d.decl_f(
                "qb",
                "distRGB * 5.0 * (1.0 - alpha) * (0.15 + 3.0 * luma) + distRGB * alpha \
                 - satL * (sumRGB - 3.0 * luma)",
            ));
            out.push(d.decl_f("qc", "-satL * (0.15 + 3.0 * luma)"));
            // Floored so a negative discriminant cannot produce a NaN that then survives the
            // selects below; the reference leaves this unguarded.
            out.push(d.decl_f("discrim", "sqrt(max(qb * qb - 4.0 * qa * qc, 0.0))"));
            out.push(d.decl_f("denom", "-discrim - qb"));
            out.push(d.decl_f("smA", "(2.0 * qc) / denom"));
            out.push(d.decl_f("smB", "(2.0 * qc) / (denom + discrim * 2.0)"));
            out.push(d.decl_f("sm", &d.sel("smA >= 0.0", "smA", "smB")));
            out.push(d.decl_f(
                "gainS",
                &d.sel("alpha == 1.0", "s1", &d.sel("alpha == 0.0", "s0", "sm")),
            ));
        }
    }

    out.push(d.set("luma + gainS * (RGB0 - luma)"));
    out.push("}".to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::dialect::{GLSL, HLSL, METAL, WGSL};

    const FWD: [FixedFunctionStyle; 3] = [
        FixedFunctionStyle::RgbToHsyLin,
        FixedFunctionStyle::RgbToHsyLog,
        FixedFunctionStyle::RgbToHsyVid,
    ];
    const INV: [FixedFunctionStyle; 3] = [
        FixedFunctionStyle::HsyLinToRgb,
        FixedFunctionStyle::HsyLogToRgb,
        FixedFunctionStyle::HsyVidToRgb,
    ];

    fn text(style: FixedFunctionStyle) -> String {
        emit(style, &GLSL).expect("HSY style must emit").join("\n")
    }

    /// All four dialects must emit the same number of statements for every HSY style.
    #[test]
    fn dialects_agree_structurally() {
        for style in FWD.into_iter().chain(INV) {
            let expect = emit(style, &GLSL).expect("emits").len();
            for (name, d) in [("HLSL", &HLSL), ("Metal", &METAL), ("WGSL", &WGSL)] {
                assert_eq!(
                    emit(style, d).expect("emits").len(),
                    expect,
                    "{name} emits a different statement count for {style:?}"
                );
            }
        }
    }

    /// The three styles must NOT emit identical bodies -- that collapse was the core of EMIT-13.
    /// Each carries its own saturation scale: Lin 1.4, Log 4, Video 1.25.
    #[test]
    fn the_three_styles_are_distinct() {
        let bodies: Vec<String> = FWD.into_iter().map(text).collect();
        assert_ne!(bodies[0], bodies[1], "Lin and Log must differ");
        assert_ne!(bodies[1], bodies[2], "Log and Video must differ");
        assert_ne!(bodies[0], bodies[2], "Lin and Video must differ");

        assert!(
            bodies[0].contains("1.4"),
            "Lin saturation scale:\n{}",
            bodies[0]
        );
        assert!(
            bodies[1].contains("distRGB * 4.0"),
            "Log scale:\n{}",
            bodies[1]
        );
        assert!(
            bodies[2].contains("distRGB * 1.25"),
            "Video scale:\n{}",
            bodies[2]
        );
    }

    /// Hue zero sits at MAGENTA, so the sector offsets are 1/3/5 -- not the 0/2/4 a conventional
    /// HSV derivation uses, which is what the old emitters had.
    /// Reference: `FixedFunctionOpGPU.cpp:1735-1742`.
    #[test]
    fn hue_zero_is_at_magenta() {
        let fwd_src = text(FixedFunctionStyle::RgbToHsyLin);
        for off in ["hue = 1.0 +", "hue = 3.0 +", "hue = 5.0 +"] {
            assert!(
                fwd_src.contains(off),
                "missing magenta-zero offset {off}:\n{fwd_src}"
            );
        }
        let inv_src = text(FixedFunctionStyle::HsyLinToRgb);
        assert!(
            inv_src.contains("color.r - (1.0 / 6.0)"),
            "inverse must undo the offset:\n{inv_src}"
        );
    }

    /// Saturation is the L1 distance to luma, not HSV's `(max-min)/max`.
    #[test]
    fn saturation_is_l1_distance_to_luma() {
        for style in FWD {
            let s = text(style);
            assert!(
                s.contains("distRGB"),
                "{style:?} must use the L1 distance:\n{s}"
            );
            assert!(
                !s.contains("(mx - mn) / mx"),
                "{style:?} still uses the HSV saturation:\n{s}"
            );
        }
    }

    // ---------------------------------------------------------------------
    // Numeric round-trip
    //
    // These reimplement, in Rust, exactly the expressions emitted above. That proves
    // SELF-CONSISTENCY -- that the inverse inverts the forward -- which is precisely what
    // EMIT-13 measured when it round-tripped pure red and got (0.547, 0.122, 0.122). It is not
    // independent evidence of fidelity to OCIO: there is no shader runtime in this tree, so
    // fidelity rests on the transcription plus the structural assertions above. Stated here
    // rather than left implied.
    // ---------------------------------------------------------------------

    fn luma_f(v: [f64; 3]) -> f64 {
        v[0] * LUMA[0] + v[1] * LUMA[1] + v[2] * LUMA[2]
    }

    fn dist_f(v: [f64; 3], s: f64) -> f64 {
        (v[0] - s).abs() + (v[1] - s).abs() + (v[2] - s).abs()
    }

    fn eval_fwd(rgb: [f64; 3], style: HsyStyle) -> [f64; 3] {
        let y = luma_f(rgb);
        let mn = rgb[0].min(rgb[1]).min(rgb[2]);
        let mx = rgb[0].max(rgb[1]).max(rgb[2]);
        let d = dist_f(rgb, y);
        let sat = match style {
            HsyStyle::Lin => {
                let sum = rgb[0] + rgb[1] + rgb[2];
                let hi = d / (0.07 * d + 1e-6).max(0.15 + sum);
                let lo = d * 5.0;
                let a = ((y - 0.001) / (0.01 - 0.001)).clamp(0.0, 1.0);
                (lo + a * (hi - lo)) * 1.4
            }
            HsyStyle::Log => d * 4.0,
            HsyStyle::Vid => d * 1.25,
        };
        let mut hue = 0.0;
        if mn != mx {
            let inv_range = 1.0 / (mx - mn);
            hue = if mx == rgb[0] {
                1.0 + (rgb[1] - rgb[2]) * inv_range
            } else if mx == rgb[1] {
                3.0 + (rgb[2] - rgb[0]) * inv_range
            } else {
                5.0 + (rgb[0] - rgb[1]) * inv_range
            };
        }
        [hue / 6.0, sat, y]
    }

    fn eval_inv(hsy: [f64; 3], style: HsyStyle) -> [f64; 3] {
        let y = hsy[2];
        let sat = hsy[1];
        let mut hue = hsy[0] - 1.0 / 6.0;
        if y < 0.0 {
            hue += 0.5;
        }
        hue = (hue - hue.floor()) * 6.0;
        let mut rgb0 = [
            ((hue - 3.0).abs() - 1.0).clamp(0.0, 1.0),
            (2.0 - (hue - 2.0).abs()).clamp(0.0, 1.0),
            (2.0 - (hue - 4.0).abs()).clamp(0.0, 1.0),
        ];
        let cur = luma_f(rgb0);
        for c in &mut rgb0 {
            *c *= y / cur;
        }
        let d = dist_f(rgb0, y);
        let gain = match style {
            HsyStyle::Log => sat / (1e-10_f64).max(d * 4.0),
            HsyStyle::Vid => sat / (1e-10_f64).max(d * 1.25),
            HsyStyle::Lin => {
                let sum = rgb0[0] + rgb0[1] + rgb0[2];
                let sl = sat / 1.4;
                let tmp = (1e-6_f64).max(-sl * sum + sl * 3.0 * y + d);
                let s1 = (sl * (0.15 + 3.0 * y) / tmp).min(50.0);
                let s0 = sl / (1e-10_f64).max(d * 5.0);
                let a = ((y - 0.001) / (0.01 - 0.001)).clamp(0.0, 1.0);
                let qa = d * 5.0 * (1.0 - a) * (sum - 3.0 * y);
                let qb = d * 5.0 * (1.0 - a) * (0.15 + 3.0 * y) + d * a - sl * (sum - 3.0 * y);
                let qc = -sl * (0.15 + 3.0 * y);
                let disc = (qb * qb - 4.0 * qa * qc).max(0.0).sqrt();
                let den = -disc - qb;
                let sm_a = (2.0 * qc) / den;
                let sm = if sm_a >= 0.0 {
                    sm_a
                } else {
                    (2.0 * qc) / (den + disc * 2.0)
                };
                if a == 1.0 {
                    s1
                } else if a == 0.0 {
                    s0
                } else {
                    sm
                }
            }
        };
        [
            y + gain * (rgb0[0] - y),
            y + gain * (rgb0[1] - y),
            y + gain * (rgb0[2] - y),
        ]
    }

    /// The inverse must invert the forward. The old emitters failed this on the very first
    /// colour anyone would try.
    #[test]
    fn inverse_inverts_forward() {
        let colours = [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
            [0.18, 0.18, 0.18],
            [0.6, 0.3, 0.1],
            [0.05, 0.4, 0.9],
        ];
        for style in [HsyStyle::Lin, HsyStyle::Log, HsyStyle::Vid] {
            for c in colours {
                let back = eval_inv(eval_fwd(c, style), style);
                for i in 0..3 {
                    assert!(
                        (back[i] - c[i]).abs() < 1e-6,
                        "{} round-trip of {c:?} gave {back:?} (component {i})",
                        style.label()
                    );
                }
            }
        }
    }

    /// The specific regression EMIT-13 recorded, pinned on its own.
    #[test]
    fn pure_red_round_trips_exactly() {
        let back = eval_inv(eval_fwd([1.0, 0.0, 0.0], HsyStyle::Lin), HsyStyle::Lin);
        for (i, want) in [1.0, 0.0, 0.0].into_iter().enumerate() {
            assert!(
                (back[i] - want).abs() < 1e-6,
                "pure red round-tripped to {back:?}; the pre-fix emitters gave (0.547, 0.122, 0.122)"
            );
        }
    }

    /// **The one test that keeps the CPU and the emitter from re-diverging.**
    ///
    /// `eval_fwd`/`eval_inv` above are a faithful Rust transcription of the *emitted shader
    /// expressions*. `crate::hsy` is the CPU that `processor/apply.rs` runs. They are two hand-
    /// written copies of the same algorithm with nothing structural forcing them to agree — which
    /// is exactly how EMIT-13 arose (an HSV wearing HSY's name on the CPU side). This asserts the
    /// two compute the same thing, in both directions, for all three styles, where they would
    /// disagree if either drifted: the hue offset, the per-style saturation scale, and the Linear
    /// quadratic inverse are all exercised. Tolerance is loose enough only for the f64(emitter) vs
    /// f32(CPU) evaluation gap, far tighter than any of the defects this guards against (hue
    /// rotation, a 1.4/4/1.25 scale swap, an HSV `(max-min)/max` saturation).
    #[test]
    fn cpu_matches_emitter() {
        use crate::hsy::{self, HsyVariant};

        fn variant(s: HsyStyle) -> HsyVariant {
            match s {
                HsyStyle::Lin => HsyVariant::Lin,
                HsyStyle::Log => HsyVariant::Log,
                HsyStyle::Vid => HsyVariant::Vid,
            }
        }

        // Kept moderate: the Linear inverse is a quadratic whose conditioning worsens far outside
        // [0,1], where f32-vs-f64 divergence would swamp the algorithmic agreement this measures.
        let colours = [
            [1.0f32, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [0.18, 0.18, 0.18],
            [0.6, 0.3, 0.1],
            [0.05, 0.4, 0.9],
            [0.9, 0.85, 0.2],
        ];
        let tol = 2e-4;

        for style in [HsyStyle::Lin, HsyStyle::Log, HsyStyle::Vid] {
            let v = variant(style);
            for c in colours {
                // Forward: CPU rgb->hsy must equal the emitter's rgb->hsy.
                let cpu_f = hsy::rgb_to_hsy(c, v);
                let emit_f = eval_fwd([c[0] as f64, c[1] as f64, c[2] as f64], style);
                for i in 0..3 {
                    assert!(
                        (cpu_f[i] as f64 - emit_f[i]).abs() < tol,
                        "{style:?} forward channel {i}: CPU {} vs emitter {} for {c:?}",
                        cpu_f[i],
                        emit_f[i]
                    );
                }

                // Inverse: feed BOTH sides the emitter's forward output, so any forward mismatch
                // does not mask an inverse mismatch. CPU hsy->rgb must equal the emitter's.
                let hsy_in = [emit_f[0] as f32, emit_f[1] as f32, emit_f[2] as f32];
                let cpu_i = hsy::hsy_to_rgb(hsy_in, v);
                let emit_i = eval_inv(emit_f, style);
                for i in 0..3 {
                    assert!(
                        (cpu_i[i] as f64 - emit_i[i]).abs() < tol,
                        "{style:?} inverse channel {i}: CPU {} vs emitter {} (hsy {hsy_in:?})",
                        cpu_i[i],
                        emit_i[i]
                    );
                }
            }
        }
    }

    /// Every emitted WGSL body must actually parse.
    ///
    /// WGSL is the dialect where this module's abstraction can go wrong in ways the C-family
    /// three cannot: it has no ternary, it splits `let` from `var` and rejects assignment to a
    /// `let`, it forbids assigning through a swizzle, and it requires matching operand types for
    /// `clamp`/`min`/`max`. Eyeballing catches none of those reliably, so run the real front end
    /// over each body wrapped in a minimal shader.
    #[test]
    fn wgsl_bodies_parse() {
        for style in FWD.into_iter().chain(INV) {
            let body = emit(style, &WGSL).expect("emits").join("\n");
            let src = format!(
                "@fragment\nfn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {{\n\
                 var color = vec4<f32>(uv.x, uv.y, 0.5, 1.0);\n{body}\nreturn color;\n}}\n"
            );
            if let Err(e) = naga::front::wgsl::parse_str(&src) {
                panic!("naga rejected the WGSL for {style:?}:\n{e}\n\n{src}");
            }
        }
    }

    /// Magenta must land on hue 0, which is the whole point of the 1/3/5 offsets.
    #[test]
    fn magenta_is_hue_zero() {
        let h = eval_fwd([1.0, 0.0, 1.0], HsyStyle::Log)[0];
        assert!(
            h.abs() < 1e-9 || (h - 1.0).abs() < 1e-9,
            "magenta hue was {h}, expected 0 (or 1)"
        );
        // Red sits a sixth of the way round from magenta.
        let hr = eval_fwd([1.0, 0.0, 0.0], HsyStyle::Log)[0];
        assert!(
            (hr - 1.0 / 6.0).abs() < 1e-9,
            "red hue was {hr}, expected 1/6"
        );
    }
}
