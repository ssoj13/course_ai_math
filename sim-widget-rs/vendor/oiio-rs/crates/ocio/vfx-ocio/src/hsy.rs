//! RGB <-> HSY, the CPU side. One implementation, for the whole workspace.
//!
//! # Why this lives in `vfx-ocio`
//!
//! There used to be two. `vfx-ops/fixed_function/color_models.rs` held a correct port;
//! `vfx-ocio/grading_hue_curve.rs` held a private copy that was **plain HSV with Rec.709 luma**
//! wearing HSY's name, and `processor/apply.rs` called *that* one — so every hue curve authored
//! against an OCIO config ran through the wrong colour model. It got three things wrong:
//!
//! 1. **Saturation** from HSV's `(max-min)/max` instead of the L1 distance to luma.
//! 2. **Hue zero at red** (sector offsets 0/2/4) instead of at **magenta** (offsets 1/3/5).
//!    Upstream places it there deliberately — `FixedFunctionOpGPU.cpp:1735` notes it "allows for
//!    better placement of red when manipulating curves in a UI". Every authored hue landed rotated.
//! 3. **No per-style saturation.** Lin/Log/Vid collapsed to a single path, silently dropping the
//!    x1.4 / x4 / x1.25 scales.
//!
//! `vfx-ops` depends on `vfx-ocio`, not the reverse — which is *why* the duplicate existed: the
//! `vfx-ocio` side could not call outward without a dependency cycle. So the single copy lives
//! here, next to the GPU emitter it must agree with (`gpu/hsy.rs`), and `vfx-ops` re-exports it.
//! It deliberately does **not** go down into `vfx-core`/`vfx-math`: HSY is colour science, not a
//! numeric primitive, and those crates must not grow an OCIO-shaped dependency.
//!
//! # Authority and the CPU/GPU pin
//!
//! Reference: OCIO `src/OpenColorIO/ops/fixedfunction/FixedFunctionOpCPU.cpp`
//! (`applyRGBToHSY` / `applyHSYToRGB`), cross-read against `FixedFunctionOpGPU.cpp`
//! `Add_RGB_TO_HSY` (:1703-1744) / `Add_HSY_TO_RGB` (:1747-1800).
//!
//! `gpu/hsy.rs` emits shader text for the same six styles. Nothing in the type system keeps the
//! two from drifting apart again, so `gpu::hsy`'s `cpu_matches_emitter` test evaluates the
//! *emitted expressions* and compares them against these functions across all three styles and
//! both directions. If you change the maths here, change it there, or that test fails — which is
//! the point.
//!
//! # Asymmetry that is not a bug
//!
//! `rgb_to_hsy` has **no** negative-luma hue rotation; `hsy_to_rgb` does. The reference pair is
//! asymmetric in exactly this way (`applyRGBToHSY` has no `luma < 0` branch at all; the 180-degree
//! turn lives only in `applyHSYToRGB`). A forward rotation was present in an earlier version of
//! the `vfx-ops` copy and made the two compound, putting negative-luma pixels — routine in
//! scene-linear plates after a matrix conversion — a full half-turn off the reference hue. Filed
//! as OPS-06. Do not "restore symmetry".

use vfx_core::consts::luma::{REC709_LUMA_B, REC709_LUMA_G, REC709_LUMA_R};

/// Which grading style an HSY conversion belongs to.
///
/// The three differ only in how saturation is scaled, but they differ in *both* directions, and
/// the Linear inverse is not a reciprocal — so the distinction is carried as a type rather than as
/// a bare scale factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HsyVariant {
    /// Scene-linear: saturation blends a shadow and a highlight estimate, then scales by 1.4.
    Lin,
    /// Log-encoded: saturation is the L1 distance scaled by 4.
    Log,
    /// Video/display: saturation is the L1 distance scaled by 1.25.
    Vid,
}

/// Constants of the `HSY_LIN` saturation model, under the reference's own names.
///
/// Verified digit-exact against the reference by the constants sweep
/// (`.bughunt/critic_color_constants.md`). Kept together so the forward blend and its quadratic
/// inverse below demonstrably read the same numbers.
mod lin {
    /// Denominator offset in the highlight saturation estimate.
    pub const K: f32 = 0.15;
    /// Multiplier of the shadow saturation estimate.
    pub const LO_GAIN: f32 = 5.0;
    /// Upper luma of the shadow/highlight cross-fade.
    pub const MAX_LUM: f32 = 0.01;
    /// Lower luma of the cross-fade.
    pub const MIN_LUM: f32 = MAX_LUM * 0.1;
    /// Overall saturation scale applied after the blend.
    pub const SCALE: f32 = 1.4;
}

/// Saturation scale for the Log style.
const LOG_SAT_SCALE: f32 = 4.0;
/// Saturation scale for the Video style.
const VID_SAT_SCALE: f32 = 1.25;

/// Rec.709 luma of an RGB triple.
#[inline]
fn luma_of(rgb: [f32; 3]) -> f32 {
    REC709_LUMA_R * rgb[0] + REC709_LUMA_G * rgb[1] + REC709_LUMA_B * rgb[2]
}

/// L1 distance from `rgb` to the achromatic axis at level `s`.
///
/// This, not HSV's `(max-min)/max`, is the reference's chroma measure. Named so the forward and
/// the inverse are visibly using the same expression.
#[inline]
fn l1_dist(rgb: [f32; 3], s: f32) -> f32 {
    (rgb[0] - s).abs() + (rgb[1] - s).abs() + (rgb[2] - s).abs()
}

/// RGB -> HSY. Hue is on `[0,1]` with **magenta at 0**; Y is Rec.709 luma.
#[inline]
pub fn rgb_to_hsy(rgb: [f32; 3], variant: HsyVariant) -> [f32; 3] {
    let (red, grn, blu) = (rgb[0], rgb[1], rgb[2]);

    let rgb_min = red.min(grn).min(blu);
    let rgb_max = red.max(grn).max(blu);
    let luma = luma_of(rgb);
    let dist_rgb = l1_dist(rgb, luma);

    let sat = match variant {
        HsyVariant::Lin => {
            // No single estimate behaves both near black and in the highlights, so the reference
            // blends two and cross-fades over luma [MIN_LUM, MAX_LUM].
            let sum_rgb = red + grn + blu;
            let sat_hi = dist_rgb / (0.07 * dist_rgb + 1e-6_f32).max(lin::K + sum_rgb);
            let sat_lo = dist_rgb * lin::LO_GAIN;
            let alpha = lin_blend_alpha(luma);
            (sat_lo + alpha * (sat_hi - sat_lo)) * lin::SCALE
        }
        HsyVariant::Log => dist_rgb * LOG_SAT_SCALE,
        HsyVariant::Vid => dist_rgb * VID_SAT_SCALE,
    };

    // Hue zero at MAGENTA -- hence 1/3/5, not the 0/2/4 a conventional HSV derivation uses.
    let hue = if rgb_min != rgb_max {
        let delta = rgb_max - rgb_min;
        if red == rgb_max {
            1.0 + (grn - blu) / delta
        } else if grn == rgb_max {
            3.0 + (blu - red) / delta
        } else {
            5.0 + (red - grn) / delta
        }
    } else {
        0.0
    } * (1.0 / 6.0);

    [hue, sat, luma]
}

/// HSY -> RGB. Exact inverse of [`rgb_to_hsy`] for the same `variant`.
#[inline]
pub fn hsy_to_rgb(hsy: [f32; 3], variant: HsyVariant) -> [f32; 3] {
    let (h_in, sat, luma) = (hsy[0], hsy[1], hsy[2]);

    // Undo the magenta-at-zero offset, then turn the hue half a rotation for negative luma so it
    // stays meaningful below black instead of folding onto itself. See the module doc: the
    // forward has no counterpart to this, deliberately.
    let mut hue = h_in - 1.0 / 6.0;
    if luma < 0.0 {
        hue += 0.5;
    }
    hue = (hue - hue.floor()) * 6.0;

    // Clamped triangle wave: the hue reconstructed at unit amplitude.
    let base = [
        ((hue - 3.0).abs() - 1.0).clamp(0.0, 1.0),
        (2.0 - (hue - 2.0).abs()).clamp(0.0, 1.0),
        (2.0 - (hue - 4.0).abs()).clamp(0.0, 1.0),
    ];

    // Renormalise to carry the requested luma. `curr_y` cannot be zero: for any finite hue at
    // least one of the three clamped triangles is >= 1, so `curr_y >= REC709_LUMA_B`. No guard,
    // because there is no case to guard against -- and the emitter divides unguarded too.
    let curr_y = luma_of(base);
    let scale = luma / curr_y;
    let rgb0 = [base[0] * scale, base[1] * scale, base[2] * scale];

    let dist_rgb = l1_dist(rgb0, luma);

    let gain_s = match variant {
        HsyVariant::Lin => lin_inv_sat(sat, luma, dist_rgb, rgb0[0] + rgb0[1] + rgb0[2]),
        HsyVariant::Log => sat / (dist_rgb * LOG_SAT_SCALE).max(1e-10),
        HsyVariant::Vid => sat / (dist_rgb * VID_SAT_SCALE).max(1e-10),
    };

    [
        luma + (rgb0[0] - luma) * gain_s,
        luma + (rgb0[1] - luma) * gain_s,
        luma + (rgb0[2] - luma) * gain_s,
    ]
}

/// Cross-fade weight between the shadow and highlight saturation estimates of the Linear style.
#[inline]
fn lin_blend_alpha(luma: f32) -> f32 {
    ((luma - lin::MIN_LUM) / (lin::MAX_LUM - lin::MIN_LUM)).clamp(0.0, 1.0)
}

/// Inverts the Linear style's saturation blend.
///
/// `sat_hi` depends on the gained result, so inverting `sat_lo + alpha*(sat_hi - sat_lo)` is a
/// quadratic in the gain rather than a division. The reference takes the numerically-stable root
/// `2c / (-discrim - b)` and switches to the conjugate form when that comes out negative, instead
/// of the textbook formula; that choice is deliberate upstream and is kept. The two endpoints are
/// taken from the closed forms, where the quadratic is singular.
#[inline]
fn lin_inv_sat(sat: f32, luma: f32, dist_rgb: f32, sum_rgb: f32) -> f32 {
    let sat = sat / lin::SCALE;
    let alpha = lin_blend_alpha(luma);

    if alpha == 1.0 {
        let tmp = (-sat * sum_rgb + sat * 3.0 * luma + dist_rgb).max(1e-6);
        (sat * (lin::K + 3.0 * luma) / tmp).min(50.0)
    } else if alpha == 0.0 {
        sat / (dist_rgb * lin::LO_GAIN).max(1e-10)
    } else {
        let a = dist_rgb * lin::LO_GAIN * (1.0 - alpha) * (sum_rgb - 3.0 * luma);
        let b = dist_rgb * lin::LO_GAIN * (1.0 - alpha) * (lin::K + 3.0 * luma) + dist_rgb * alpha
            - sat * (sum_rgb - 3.0 * luma);
        let c = -sat * (lin::K + 3.0 * luma);

        // Floored so a negative discriminant cannot produce a NaN that then survives both root
        // choices below. The reference leaves this unguarded; the GPU emitter carries the same
        // floor, and the two must agree.
        let discrim = (b * b - 4.0 * a * c).max(0.0).sqrt();
        let denom = -discrim - b;
        let gain_s = (2.0 * c) / denom;

        if gain_s >= 0.0 {
            gain_s
        } else {
            (2.0 * c) / (denom + discrim * 2.0)
        }
    }
}

/// Convert an RGB triple to HSY in place.
#[inline]
pub fn apply_rgb_to_hsy(rgb: &mut [f32; 3], variant: HsyVariant) {
    *rgb = rgb_to_hsy(*rgb, variant);
}

/// Convert an HSY triple to RGB in place.
#[inline]
pub fn apply_hsy_to_rgb(hsy: &mut [f32; 3], variant: HsyVariant) {
    *hsy = hsy_to_rgb(*hsy, variant);
}

/// Convert an interleaved RGBA buffer to HSY in place, leaving alpha untouched.
pub fn apply_rgb_to_hsy_rgba(pixels: &mut [f32], variant: HsyVariant) {
    for chunk in pixels.as_chunks_mut::<4>().0 {
        let hsy = rgb_to_hsy([chunk[0], chunk[1], chunk[2]], variant);
        chunk[..3].copy_from_slice(&hsy);
    }
}

/// Convert an interleaved HSY+alpha buffer to RGB in place, leaving alpha untouched.
pub fn apply_hsy_to_rgb_rgba(pixels: &mut [f32], variant: HsyVariant) {
    for chunk in pixels.as_chunks_mut::<4>().0 {
        let rgb = hsy_to_rgb([chunk[0], chunk[1], chunk[2]], variant);
        chunk[..3].copy_from_slice(&rgb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VARIANTS: [HsyVariant; 3] = [HsyVariant::Lin, HsyVariant::Log, HsyVariant::Vid];

    /// A spread that exercises every hue sector, the achromatic axis, sub-black luma, and both
    /// sides of the Linear style's [0.001, 0.01] cross-fade.
    const COLOURS: [[f32; 3]; 12] = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
        [0.18, 0.18, 0.18],
        [0.6, 0.3, 0.1],
        [0.05, 0.4, 0.9],
        [0.004, 0.002, 0.001],
        [0.0004, 0.0002, 0.0001],
        [4.0, 1.5, 0.25],
    ];

    /// Magenta sits at hue 0 and red a sixth of a turn on. This is the assertion the replaced
    /// implementation failed: it put red at 0.
    #[test]
    fn hue_zero_is_magenta() {
        let m = rgb_to_hsy([1.0, 0.0, 1.0], HsyVariant::Log)[0];
        assert!(m.abs() < 1e-6, "magenta hue was {m}, expected 0");
        let r = rgb_to_hsy([1.0, 0.0, 0.0], HsyVariant::Log)[0];
        assert!(
            (r - 1.0 / 6.0).abs() < 1e-6,
            "red hue was {r}, expected 1/6"
        );
        let g = rgb_to_hsy([0.0, 1.0, 0.0], HsyVariant::Log)[0];
        assert!((g - 0.5).abs() < 1e-6, "green hue was {g}, expected 1/2");
    }

    /// Saturation is the L1 distance to luma, scaled per style -- not HSV's `(max-min)/max`,
    /// which is what the replaced implementation returned and which is style-independent.
    #[test]
    fn saturation_scales_differ_per_style() {
        let c = [0.6_f32, 0.3, 0.1];
        let luma = luma_of(c);
        let d = l1_dist(c, luma);

        let log = rgb_to_hsy(c, HsyVariant::Log)[1];
        let vid = rgb_to_hsy(c, HsyVariant::Vid)[1];
        let lin = rgb_to_hsy(c, HsyVariant::Lin)[1];

        assert!((log - d * 4.0).abs() < 1e-6, "log sat {log} != distRGB*4");
        assert!(
            (vid - d * 1.25).abs() < 1e-6,
            "vid sat {vid} != distRGB*1.25"
        );
        assert!(
            log != vid && lin != log && lin != vid,
            "styles collapsed: {lin} {log} {vid}"
        );

        // HSV saturation for this colour, which is what the old code produced regardless of style.
        let hsv_sat = (0.6_f32 - 0.1) / 0.6;
        assert!(
            (log - hsv_sat).abs() > 0.1,
            "log sat is suspiciously HSV-like"
        );
    }

    /// The inverse must invert the forward, for every style. The replaced implementation returned
    /// (0.547, 0.122, 0.122) for pure red under Lin.
    #[test]
    fn inverse_inverts_forward() {
        for variant in VARIANTS {
            for c in COLOURS {
                let back = hsy_to_rgb(rgb_to_hsy(c, variant), variant);
                for i in 0..3 {
                    let tol = 1e-5 * c[i].abs().max(1.0);
                    assert!(
                        (back[i] - c[i]).abs() < tol,
                        "{variant:?} round-trip of {c:?} gave {back:?} (channel {i})"
                    );
                }
            }
        }
    }

    /// The forward has no negative-luma hue rotation (OPS-06). A colour and its sub-black
    /// counterpart must not sit half a turn apart in the forward direction, and the inverse must
    /// still round-trip them -- which is only true if exactly one of the two rotates.
    #[test]
    fn negative_luma_rotation_is_inverse_only() {
        let neg = [-0.4_f32, -0.1, -0.2];
        let hsy = rgb_to_hsy(neg, HsyVariant::Log);
        // Same hue as the sign-flipped colour would give under a symmetric implementation only if
        // the forward rotated; it must not.
        let pos_hue = rgb_to_hsy([0.4, 0.1, 0.2], HsyVariant::Log)[0];
        assert!(
            (hsy[0] - pos_hue).abs() > 0.4,
            "forward appears to rotate negative luma: {} vs {pos_hue}",
            hsy[0]
        );
        let back = hsy_to_rgb(hsy, HsyVariant::Log);
        for i in 0..3 {
            assert!(
                (back[i] - neg[i]).abs() < 1e-5,
                "negative-luma round-trip {neg:?} -> {back:?}"
            );
        }
    }

    /// Grey has zero saturation and its luma unchanged, in every style.
    #[test]
    fn achromatic_is_fixed() {
        for variant in VARIANTS {
            let hsy = rgb_to_hsy([0.42, 0.42, 0.42], variant);
            assert!(
                hsy[1].abs() < 1e-6,
                "{variant:?} grey saturation {}",
                hsy[1]
            );
            assert!(
                (hsy[2] - 0.42).abs() < 1e-6,
                "{variant:?} grey luma {}",
                hsy[2]
            );
        }
    }

    /// No input in the spread may produce a non-finite component, in either direction.
    #[test]
    fn stays_finite() {
        for variant in VARIANTS {
            for c in COLOURS {
                let hsy = rgb_to_hsy(c, variant);
                let rgb = hsy_to_rgb(hsy, variant);
                assert!(
                    hsy.iter().chain(rgb.iter()).all(|v| v.is_finite()),
                    "{variant:?} {c:?} -> {hsy:?} -> {rgb:?}"
                );
            }
        }
    }

    #[test]
    fn rgba_helpers_leave_alpha_alone() {
        let mut px = [0.6_f32, 0.3, 0.1, 0.25];
        apply_rgb_to_hsy_rgba(&mut px, HsyVariant::Vid);
        assert_eq!(px[3], 0.25);
        apply_hsy_to_rgb_rgba(&mut px, HsyVariant::Vid);
        assert_eq!(px[3], 0.25);
        for (got, want) in px[..3].iter().zip([0.6_f32, 0.3, 0.1]) {
            assert!((got - want).abs() < 1e-5, "{px:?}");
        }
    }
}
