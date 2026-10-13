//! Grading hue-curve types and apply logic for vfx-ocio.
//!
//! Avoids a cyclic dependency on `vfx-ops` by owning the control-point carrier here.
//! Curve *evaluation* is not duplicated: production CPU and GPU both read the fitted
//! [`crate::grading_bspline::CurveUniformData`] that `processor/compile.rs` already
//! builds into `ProcessorOp::GradingHueCurve`. Reference: OCIO `GradingHueCurveOpCPU.cpp`.

use crate::grading_bspline::{
    CurveUniformData, HUE_CURVE_COUNT, eval_curve_fwd, eval_curve_rev, eval_curve_rev_hue,
};
#[cfg(test)]
use crate::grading_bspline::{GradingControlPoint, build_hue_uniforms};
use crate::hsy::{HsyVariant, hsy_to_rgb, rgb_to_hsy};
use vfx_core::consts::loglin as lin_log;

/// Fitted knot/coef payload for the eight hue curves (same layout GPU uniforms use).
pub type HueCurveData = CurveUniformData<{ HUE_CURVE_COUNT * 2 }>;

/// Curve slot indices — must match `build_hue_uniforms` / `HueCurveType::ALL` order.
const HUE_HUE: usize = 0;
const HUE_SAT: usize = 1;
const HUE_LUM: usize = 2;
const LUM_SAT: usize = 3;
const SAT_SAT: usize = 4;
const LUM_LUM: usize = 5;
const SAT_LUM: usize = 6;
const HUE_FX: usize = 7;

/// Control point on a hue curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HueControlPoint {
    pub hue: f32,
    pub value: f32,
}

impl HueControlPoint {
    #[inline]
    pub fn new(hue: f32, value: f32) -> Self {
        Self { hue, value }
    }
}

/// Control points for one hue curve. Evaluation lives on fitted [`HueCurveData`], not here.
#[derive(Debug, Clone)]
pub struct HueCurve {
    pub points: Vec<HueControlPoint>,
}

impl HueCurve {
    pub fn new(mut points: Vec<HueControlPoint>) -> Self {
        points.sort_by(|a, b| a.hue.partial_cmp(&b.hue).unwrap());
        Self { points }
    }
}

/// Grading style (HSY variant selector).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradingStyle {
    #[default]
    Log,
    Linear,
    Video,
}

impl GradingStyle {
    /// The HSY saturation model this grading style converts through.
    #[inline]
    pub fn hsy_variant(self) -> HsyVariant {
        match self {
            Self::Log => HsyVariant::Log,
            Self::Linear => HsyVariant::Lin,
            Self::Video => HsyVariant::Vid,
        }
    }
}

/// Full set of 8 grading hue curves (control-point metadata).
#[derive(Debug, Clone)]
pub struct GradingHueCurves {
    pub style: GradingStyle,
    pub hue_hue: HueCurve,
    pub hue_sat: HueCurve,
    pub hue_lum: HueCurve,
    pub lum_sat: HueCurve,
    pub sat_sat: HueCurve,
    pub lum_lum: HueCurve,
    pub sat_lum: HueCurve,
    pub hue_fx: HueCurve,
}

impl GradingHueCurves {
    /// Control-point identity heuristic (for dynamic-property UI). Prefer
    /// [`HueCurveData::local_bypass`] on the production apply path.
    pub fn is_identity(&self) -> bool {
        let diag = |c: &HueCurve| c.points.iter().all(|p| (p.value - p.hue).abs() < 1e-6);
        let horiz = |c: &HueCurve, v: f32| c.points.iter().all(|p| (p.value - v).abs() < 1e-6);
        diag(&self.hue_hue)
            && horiz(&self.hue_sat, 1.0)
            && horiz(&self.hue_lum, 1.0)
            && horiz(&self.lum_sat, 1.0)
            && diag(&self.sat_sat)
            && diag(&self.lum_lum)
            && horiz(&self.sat_lum, 1.0)
            && horiz(&self.hue_fx, 0.0)
    }
}

#[inline]
fn lin_to_log(lum: f32) -> f32 {
    if lum < lin_log::XBRK {
        lum * lin_log::GAIN + lin_log::OFFS
    } else {
        lin_log::BASE2 * ((lum + lin_log::SHIFT) * lin_log::M).ln()
    }
}

#[inline]
fn log_to_lin(lum: f32) -> f32 {
    if lum < lin_log::YBRK {
        (lum - lin_log::OFFS) / lin_log::GAIN
    } else {
        (2.0_f32).powf(lum) * (0.18 + lin_log::SHIFT) - lin_log::SHIFT
    }
}

#[cfg(test)]
fn to_bspline_points(curve: &HueCurve) -> Vec<GradingControlPoint> {
    curve
        .points
        .iter()
        .map(|p| GradingControlPoint {
            x: p.hue,
            y: p.value,
        })
        .collect()
}

/// Fit eight curves with estimated slopes (no user slopes). Test/convenience only —
/// production must use the `curve_data` compile already built with user slopes.
#[cfg(test)]
pub fn fit_curves(curves: &GradingHueCurves) -> HueCurveData {
    let points: [Vec<GradingControlPoint>; HUE_CURVE_COUNT] = [
        to_bspline_points(&curves.hue_hue),
        to_bspline_points(&curves.hue_sat),
        to_bspline_points(&curves.hue_lum),
        to_bspline_points(&curves.lum_sat),
        to_bspline_points(&curves.sat_sat),
        to_bspline_points(&curves.lum_lum),
        to_bspline_points(&curves.sat_lum),
        to_bspline_points(&curves.hue_fx),
    ];
    let empty: [f32; 0] = [];
    let point_refs: [&[GradingControlPoint]; HUE_CURVE_COUNT] =
        std::array::from_fn(|i| points[i].as_slice());
    let slope_refs: [&[f32]; HUE_CURVE_COUNT] = std::array::from_fn(|_| &empty[..]);
    build_hue_uniforms(point_refs, slope_refs, false)
}

#[inline]
fn eval_fwd(data: &HueCurveData, curve_idx: usize, x: f32, identity_x: f32) -> f32 {
    eval_curve_fwd(
        curve_idx,
        x,
        identity_x,
        &data.knots_offsets,
        &data.coefs_offsets,
        &data.knots,
        &data.coefs,
    )
}

#[inline]
fn eval_rev(data: &HueCurveData, curve_idx: usize, y: f32) -> f32 {
    eval_curve_rev(
        curve_idx,
        y,
        &data.knots_offsets,
        &data.coefs_offsets,
        &data.knots,
        &data.coefs,
    )
}

#[inline]
fn eval_rev_hue(data: &HueCurveData, curve_idx: usize, y: f32) -> f32 {
    eval_curve_rev_hue(
        curve_idx,
        y,
        &data.knots_offsets,
        &data.coefs_offsets,
        &data.knots,
        &data.coefs,
    )
}

/// Draw-curve-only: evaluate HUE_SAT on each channel (ref `GradingHueCurveOpCPU.cpp:190-192`).
/// Does **not** consult `local_bypass` — the reference still graphs an identity set.
#[inline]
pub fn eval_draw_curve_only(data: &HueCurveData, x: f32) -> f32 {
    eval_fwd(data, HUE_SAT, x, 1.0)
}

/// Apply grading hue curves forward against pre-fitted uniforms.
///
/// `do_rgb_to_hsy`: when false (`HsyTransformStyle::None`), the pixel is treated as already-HSY
/// and the RGB↔HSY pair is skipped (ref `:108-114`, matching the four GPU emitters).
///
/// Luminance gain keys on `is_linear` (additive for Log/Video) — deliberate EMIT-14 decision;
/// do not flip Video to multiplicative while moving this code.
pub fn apply_hue_curves_fwd_data(
    style: GradingStyle,
    do_rgb_to_hsy: bool,
    data: &HueCurveData,
    rgb: &mut [f32; 3],
) {
    if data.local_bypass {
        return;
    }

    let is_linear = style == GradingStyle::Linear;
    let variant = style.hsy_variant();

    let mut hsy = if do_rgb_to_hsy {
        rgb_to_hsy(*rgb, variant)
    } else {
        *rgb
    };

    if is_linear {
        hsy[2] = lin_to_log(hsy[2]);
    }

    let hue_sat_gain = eval_fwd(data, HUE_SAT, hsy[0], 1.0).max(0.0);
    let hue_lum_gain_raw = eval_fwd(data, HUE_LUM, hsy[0], 1.0).max(0.0);
    hsy[0] = eval_fwd(data, HUE_HUE, hsy[0], hsy[0]);
    hsy[1] = eval_fwd(data, SAT_SAT, hsy[1], hsy[1]).max(0.0);
    let lum_sat_gain = eval_fwd(data, LUM_SAT, hsy[2], 1.0).max(0.0);
    hsy[1] *= lum_sat_gain * hue_sat_gain;
    let sat_lum_gain = eval_fwd(data, SAT_LUM, hsy[1], 1.0).max(0.0);
    hsy[2] = eval_fwd(data, LUM_LUM, hsy[2], hsy[2]);

    if is_linear {
        hsy[2] = log_to_lin(hsy[2]);
    }

    let hue_lum_gain = 1.0 - (1.0 - hue_lum_gain_raw) * hsy[1].min(1.0);

    if is_linear {
        hsy[2] *= hue_lum_gain * sat_lum_gain;
    } else {
        hsy[2] += (hue_lum_gain + sat_lum_gain - 2.0) * 0.1;
    }

    hsy[0] = hsy[0] - hsy[0].floor();
    hsy[0] += eval_fwd(data, HUE_FX, hsy[0], 0.0);

    *rgb = if do_rgb_to_hsy {
        hsy_to_rgb(hsy, variant)
    } else {
        hsy
    };
}

/// Apply grading hue curves reverse against pre-fitted uniforms.
pub fn apply_hue_curves_rev_data(
    style: GradingStyle,
    do_rgb_to_hsy: bool,
    data: &HueCurveData,
    rgb: &mut [f32; 3],
) {
    if data.local_bypass {
        return;
    }

    let is_linear = style == GradingStyle::Linear;
    let variant = style.hsy_variant();

    let mut hsy = if do_rgb_to_hsy {
        rgb_to_hsy(*rgb, variant)
    } else {
        *rgb
    };

    hsy[0] = eval_rev_hue(data, HUE_FX, hsy[0]);
    hsy[0] = eval_rev_hue(data, HUE_HUE, hsy[0]);

    hsy[0] = hsy[0] - hsy[0].floor();
    let hue_sat_gain = eval_fwd(data, HUE_SAT, hsy[0], 1.0).max(0.0);
    let hue_lum_gain_raw = eval_fwd(data, HUE_LUM, hsy[0], 1.0).max(0.0);

    hsy[1] = hsy[1].max(0.0);
    let sat_lum_gain = eval_fwd(data, SAT_LUM, hsy[1], 1.0).max(0.0);
    let hue_lum_gain = 1.0 - (1.0 - hue_lum_gain_raw) * hsy[1].min(1.0);

    let lum_gain = hue_lum_gain * sat_lum_gain;
    if is_linear {
        hsy[2] /= lum_gain.max(0.01);
    } else {
        hsy[2] -= (hue_lum_gain + sat_lum_gain - 2.0) * 0.1;
    }

    if is_linear {
        hsy[2] = lin_to_log(hsy[2]);
    }
    hsy[2] = eval_rev(data, LUM_LUM, hsy[2]);

    let lum_sat_gain = eval_fwd(data, LUM_SAT, hsy[2], 1.0).max(0.0);
    if is_linear {
        hsy[2] = log_to_lin(hsy[2]);
    }

    hsy[1] /= (lum_sat_gain * hue_sat_gain).max(0.01);
    hsy[1] = eval_rev(data, SAT_SAT, hsy[1]).max(0.0);

    *rgb = if do_rgb_to_hsy {
        hsy_to_rgb(hsy, variant)
    } else {
        hsy
    };
}

/// Convenience: fit then forward. Prefer [`apply_hue_curves_fwd_data`] with compile-time
/// `curve_data` so user slopes are honoured.
#[cfg(test)]
pub fn apply_hue_curves_fwd(curves: &GradingHueCurves, rgb: &mut [f32; 3]) {
    let data = fit_curves(curves);
    apply_hue_curves_fwd_data(curves.style, true, &data, rgb);
}

/// Convenience: fit then reverse. Prefer [`apply_hue_curves_rev_data`] in production.
#[cfg(test)]
pub fn apply_hue_curves_rev(curves: &GradingHueCurves, rgb: &mut [f32; 3]) {
    let data = fit_curves(curves);
    apply_hue_curves_rev_data(curves.style, true, &data, rgb);
}

#[cfg(test)]
mod tests {
    use super::*;
    use vfx_core::consts::luma::{REC709_LUMA_B, REC709_LUMA_G, REC709_LUMA_R};

    fn luma(rgb: [f32; 3]) -> f32 {
        REC709_LUMA_R * rgb[0] + REC709_LUMA_G * rgb[1] + REC709_LUMA_B * rgb[2]
    }

    const V: HsyVariant = HsyVariant::Log;

    fn identity_diagonal() -> HueCurve {
        HueCurve::new(vec![
            HueControlPoint::new(0.0, 0.0),
            HueControlPoint::new(1.0, 1.0),
        ])
    }

    fn identity_flat(v: f32) -> HueCurve {
        HueCurve::new(vec![
            HueControlPoint::new(0.0, v),
            HueControlPoint::new(1.0, v),
        ])
    }

    fn identity_curves() -> GradingHueCurves {
        GradingHueCurves {
            style: GradingStyle::Log,
            hue_hue: identity_diagonal(),
            hue_sat: identity_flat(1.0),
            hue_lum: identity_flat(1.0),
            lum_sat: identity_flat(1.0),
            sat_sat: identity_diagonal(),
            lum_lum: identity_diagonal(),
            sat_lum: identity_flat(1.0),
            hue_fx: identity_flat(0.0),
        }
    }

    #[test]
    fn grading_hue_curves_identity_check() {
        let curves = identity_curves();
        assert!(curves.is_identity());
        assert!(fit_curves(&curves).local_bypass);
    }

    #[test]
    fn grading_hue_curves_non_identity() {
        let mut curves = identity_curves();
        curves.hue_sat = HueCurve::new(vec![
            HueControlPoint::new(0.0, 1.5),
            HueControlPoint::new(1.0, 1.0),
        ]);
        assert!(!curves.is_identity());
        assert!(!fit_curves(&curves).local_bypass);
    }

    #[test]
    fn apply_identity_no_change() {
        let curves = identity_curves();
        let mut rgb = [0.5_f32, 0.3, 0.7];
        let original = rgb;
        apply_hue_curves_fwd(&curves, &mut rgb);
        assert!((rgb[0] - original[0]).abs() < 0.01);
        assert!((rgb[1] - original[1]).abs() < 0.01);
        assert!((rgb[2] - original[2]).abs() < 0.01);
    }

    #[test]
    fn apply_hue_sat_boost() {
        let mut curves = identity_curves();
        curves.hue_sat = HueCurve::new(vec![
            HueControlPoint::new(0.0, 1.5),
            HueControlPoint::new(0.1, 1.5),
            HueControlPoint::new(0.2, 1.0),
            HueControlPoint::new(1.0, 1.0),
        ]);

        let mut rgb = [0.8_f32, 0.2, 0.2];
        let hsy_before = rgb_to_hsy(rgb, V);
        apply_hue_curves_fwd(&curves, &mut rgb);
        let hsy_after = rgb_to_hsy(rgb, V);

        assert!(hsy_after[1] > hsy_before[1], "Red saturation should boost");
    }

    #[test]
    fn apply_hue_hue_shift() {
        let mut curves = identity_curves();
        curves.hue_hue = HueCurve::new(vec![
            HueControlPoint::new(0.0, 0.05),
            HueControlPoint::new(0.1, 0.15),
            HueControlPoint::new(0.5, 0.5),
            HueControlPoint::new(1.0, 1.0),
        ]);

        let mut rgb = [0.9_f32, 0.1, 0.1];
        let hsy_before = rgb_to_hsy(rgb, V);
        apply_hue_curves_fwd(&curves, &mut rgb);
        let hsy_after = rgb_to_hsy(rgb, V);

        assert!(
            (hsy_after[0] - hsy_before[0]).abs() > 0.01,
            "Hue should shift"
        );
    }

    #[test]
    fn apply_lum_lum_gamma() {
        let mut curves = identity_curves();
        curves.lum_lum = HueCurve::new(vec![
            HueControlPoint::new(0.0, 0.0),
            HueControlPoint::new(0.5, 0.7),
            HueControlPoint::new(1.0, 1.0),
        ]);

        let mut rgb = [0.5_f32, 0.5, 0.5];
        let lum_before = luma(rgb);
        apply_hue_curves_fwd(&curves, &mut rgb);
        let lum_after = luma(rgb);

        assert!(lum_after > lum_before, "Midtones should lift");
    }

    #[test]
    fn apply_all_8_curves() {
        let curves = GradingHueCurves {
            style: GradingStyle::Log,
            hue_hue: HueCurve::new(vec![
                HueControlPoint::new(0.0, 0.1),
                HueControlPoint::new(1.0 / 6.0, 1.0 / 6.0 + 0.1),
                HueControlPoint::new(2.0 / 6.0, 2.0 / 6.0),
                HueControlPoint::new(0.5, 0.5),
                HueControlPoint::new(4.0 / 6.0, 4.0 / 6.0),
                HueControlPoint::new(5.0 / 6.0, 5.0 / 6.0),
            ]),
            hue_sat: HueCurve::new(vec![
                HueControlPoint::new(0.0, 1.8),
                HueControlPoint::new(1.0 / 6.0, 1.8),
                HueControlPoint::new(2.0 / 6.0, 1.0),
                HueControlPoint::new(0.5, 1.0),
                HueControlPoint::new(4.0 / 6.0, 1.0),
                HueControlPoint::new(5.0 / 6.0, 1.0),
            ]),
            hue_lum: HueCurve::new(vec![
                HueControlPoint::new(0.0, 1.3),
                HueControlPoint::new(1.0 / 6.0, 1.3),
                HueControlPoint::new(2.0 / 6.0, 1.0),
                HueControlPoint::new(0.5, 1.0),
                HueControlPoint::new(4.0 / 6.0, 1.0),
                HueControlPoint::new(5.0 / 6.0, 1.0),
            ]),
            lum_sat: HueCurve::new(vec![
                HueControlPoint::new(0.0, 1.4),
                HueControlPoint::new(0.5, 1.4),
                HueControlPoint::new(1.0, 1.4),
            ]),
            sat_sat: HueCurve::new(vec![
                HueControlPoint::new(0.0, 0.0),
                HueControlPoint::new(0.5, 0.8),
                HueControlPoint::new(1.0, 1.0),
            ]),
            lum_lum: HueCurve::new(vec![
                HueControlPoint::new(0.0, 0.0),
                HueControlPoint::new(0.5, 0.7),
                HueControlPoint::new(1.0, 1.0),
            ]),
            sat_lum: HueCurve::new(vec![
                HueControlPoint::new(0.0, 1.2),
                HueControlPoint::new(0.5, 1.2),
                HueControlPoint::new(1.0, 1.2),
            ]),
            hue_fx: HueCurve::new(vec![
                HueControlPoint::new(0.0, 0.05),
                HueControlPoint::new(1.0 / 6.0, 0.05),
                HueControlPoint::new(2.0 / 6.0, 0.0),
                HueControlPoint::new(0.5, 0.0),
                HueControlPoint::new(4.0 / 6.0, 0.0),
                HueControlPoint::new(5.0 / 6.0, 0.0),
            ]),
        };

        assert!(!curves.is_identity());
        assert!(!fit_curves(&curves).local_bypass);

        let mut rgb = [0.6_f32, 0.3, 0.4];
        let original = rgb;
        apply_hue_curves_fwd(&curves, &mut rgb);

        let diff = (rgb[0] - original[0]).abs()
            + (rgb[1] - original[1]).abs()
            + (rgb[2] - original[2]).abs();
        assert!(
            diff > 0.01,
            "All 8 curves should affect the result, got diff={diff}"
        );

        // Reverse convenience path must compile and move the pixel back toward the original.
        let mut back = rgb;
        apply_hue_curves_rev(&curves, &mut back);
        let back_err = (back[0] - original[0]).abs()
            + (back[1] - original[1]).abs()
            + (back[2] - original[2]).abs();
        assert!(back_err < diff, "inverse should reduce the forward delta");
    }

    #[test]
    fn linear_style_lin_log_conversion() {
        let mut curves = identity_curves();
        curves.style = GradingStyle::Linear;
        curves.lum_lum = HueCurve::new(vec![
            HueControlPoint::new(-6.0, -6.0),
            HueControlPoint::new(0.0, 0.5),
            HueControlPoint::new(6.0, 6.0),
        ]);

        let mut rgb = [0.18_f32, 0.18, 0.18];
        apply_hue_curves_fwd(&curves, &mut rgb);

        let lum = luma(rgb);
        assert!(lum > 0.18, "Linear style should apply lin-log conversion");
    }

    #[test]
    fn rgb_hsy_roundtrip() {
        let colors = [
            [1.0_f32, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.5, 0.5, 0.5],
            [0.8, 0.3, 0.6],
        ];

        for rgb in &colors {
            let hsy = rgb_to_hsy(*rgb, V);
            let back = hsy_to_rgb(hsy, V);
            assert!((back[0] - rgb[0]).abs() < 0.1, "RGB->HSY->RGB R");
            assert!((back[1] - rgb[1]).abs() < 0.1, "RGB->HSY->RGB G");
            assert!((back[2] - rgb[2]).abs() < 0.1, "RGB->HSY->RGB B");
        }
    }

    #[test]
    fn hsy_none_skips_rgb_hsy_pair() {
        // Mild sat boost so we are not testing local_bypass.
        let mut curves = identity_curves();
        curves.hue_sat = HueCurve::new(vec![
            HueControlPoint::new(0.0, 1.2),
            HueControlPoint::new(0.5, 1.2),
            HueControlPoint::new(1.0, 1.2),
        ]);
        let data = fit_curves(&curves);

        // Synthetic HSY: hue=0, sat=0.5, luma=0.4 — not a valid RGB triple.
        let mut hsy_pix = [0.0_f32, 0.5, 0.4];
        let before = hsy_pix;
        apply_hue_curves_fwd_data(GradingStyle::Log, false, &data, &mut hsy_pix);
        assert!((hsy_pix[0] - before[0]).abs() < 1e-4);
        assert!(hsy_pix[1] > before[1]);

        // Same input with do_hsy=true interprets [0,0.5,0.4] as RGB and disagrees.
        let mut as_rgb = before;
        apply_hue_curves_fwd_data(GradingStyle::Log, true, &data, &mut as_rgb);
        let channel_diff = (hsy_pix[0] - as_rgb[0]).abs()
            + (hsy_pix[1] - as_rgb[1]).abs()
            + (hsy_pix[2] - as_rgb[2]).abs();
        assert!(
            channel_diff > 1e-3,
            "None vs Default must disagree on non-RGB HSY input"
        );
    }
}
