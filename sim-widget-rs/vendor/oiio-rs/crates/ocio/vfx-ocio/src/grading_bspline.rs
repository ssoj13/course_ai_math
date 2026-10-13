//! OCIO grading B-spline fitting and evaluation, shared by the CPU ops and the GPU emitters.
//!
//! This is the workspace's single implementation of the reference's quadratic B-spline
//! machinery — `PrepHueCurveData`, `EstimateHueSlopes`, `CalcKsi`, `FitHueSpline`,
//! `EstimateRGBSlopes`, `FitRGBSpline`, `AdjustRGBSlopes` and the `KnotsCoefs` evaluators —
//! from `ops/gradingrgbcurve/GradingBSplineCurve.cpp`.
//!
//! It is public because `vfx-ops` consumes it for the CPU grading curves. Keeping one copy
//! matters here specifically: the hue curves previously carried their own piecewise-linear
//! evaluation and an iterative inverse, and diverged from both the reference and the GPU
//! path that was already using this module.

/// One user-facing control point on a grading curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradingControlPoint {
    /// Input position.
    pub x: f32,
    /// Output value at `x`.
    pub y: f32,
}

/// Which spline kind and boundary condition a curve slot is fitted with.
///
/// Reference: OCIO's per-curve-type defaults; see [`hue_curve_spline_type`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BSplineType {
    /// Plain B-spline with no special end handling.
    BSpline,
    /// Identity is the diagonal `y = x`.
    Diagonal,
    /// Periodic hue-to-hue mapping; identity is the diagonal and wraps at the seam.
    HueHue,
    /// Periodic, identity value 1.0 (a gain curve).
    Periodic1,
    /// Periodic, identity value 0.0 (a delta curve, e.g. HUE_FX).
    Periodic0,
    /// Non-periodic, identity value 1.0.
    Horizontal1,
}

/// Number of curve slots in an RGB-curve set (Red, Green, Blue, Master).
pub const RGB_CURVE_COUNT: usize = 4;
/// Number of curve slots in a hue-curve set (HUE_HUE .. HUE_FX).
pub const HUE_CURVE_COUNT: usize = 8;
/// Reference's cap on fitted knots per curve set (`GradingBSplineCurve.cpp:839-844`).
pub const MAX_NUM_KNOTS: usize = 120;
/// Reference's cap on fitted coefficients per curve set (`GradingBSplineCurve.cpp:839-844`).
pub const MAX_NUM_COEFS: usize = 360;

/// Fitted knots and quadratic coefficients for `N/2` curves, in the reference's flat layout.
///
/// `knots_offsets` and `coefs_offsets` hold `(offset, count)` pairs per curve, so curve `i`
/// occupies `knots_offsets[i*2] .. +knots_offsets[i*2+1]`. An offset of `-1` marks a curve
/// that fitted to identity and is bypassed. Coefficients are stored as three consecutive
/// runs (A, then B, then C) of `count/3` entries each, matching `KnotsCoefs`.
#[derive(Debug, Clone)]
pub struct CurveUniformData<const N: usize> {
    /// `(offset, count)` pairs into [`Self::knots`], two entries per curve.
    pub knots_offsets: [i32; N],
    /// Flat knot positions for all curves.
    pub knots: Vec<f32>,
    /// `(offset, count)` pairs into [`Self::coefs`], two entries per curve.
    pub coefs_offsets: [i32; N],
    /// Flat A/B/C coefficient runs for all curves.
    pub coefs: Vec<f32>,
    /// True when every curve in the set fitted to identity.
    pub local_bypass: bool,
}

impl<const N: usize> Default for CurveUniformData<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> CurveUniformData<N> {
    /// An empty set with all offsets zeroed.
    pub fn new() -> Self {
        Self {
            knots_offsets: [0; N],
            knots: Vec::new(),
            coefs_offsets: [0; N],
            coefs: Vec::new(),
            local_bypass: false,
        }
    }

    /// True when every curve slot bypassed (fitted to identity).
    ///
    /// `knots_offsets`/`coefs_offsets` store `(offset, count)` PAIRS per curve — a bypassed
    /// curve sets `offset = -1` but `count = 0`, not `-1` (see the bypass-marking code in
    /// `build_hue_uniforms`/`build_rgb_uniforms`). Checking every element for `-1`
    /// (`.iter().all(...)`) therefore can never be true whenever at least one curve exists,
    /// because the interleaved `count = 0` entries always fail it — this used to be computed
    /// inline, identically wrong, in both builder functions. Only the offset slot (every other
    /// entry, starting at index 0) actually marks bypass.
    fn all_bypassed(&self) -> bool {
        self.knots_offsets.iter().step_by(2).all(|v| *v == -1)
            && self.coefs_offsets.iter().step_by(2).all(|v| *v == -1)
    }
}

/// Spline kind for hue-curve slot `index`, in HUE_HUE..HUE_FX order.
///
/// Slots out of range fall back to a plain [`BSplineType::BSpline`].
pub fn hue_curve_spline_type(index: usize) -> BSplineType {
    match index {
        0 => BSplineType::HueHue,
        1 => BSplineType::Periodic1,
        2 => BSplineType::Periodic1,
        3 => BSplineType::Horizontal1,
        4 => BSplineType::Diagonal,
        5 => BSplineType::Diagonal,
        6 => BSplineType::Horizontal1,
        7 => BSplineType::Periodic0,
        _ => BSplineType::BSpline,
    }
}

fn slopes_are_default(slopes: &[f32]) -> bool {
    if slopes.is_empty() {
        return true;
    }
    slopes.iter().all(|v| *v == 0.0)
}

fn is_identity(points: &[GradingControlPoint], spline: BSplineType, slopes: &[f32]) -> bool {
    let mut identity = match spline {
        BSplineType::BSpline | BSplineType::Diagonal | BSplineType::HueHue => {
            points.iter().all(|p| p.x == p.y)
        }
        BSplineType::Periodic0 => points.iter().all(|p| p.y == 0.0),
        BSplineType::Horizontal1 | BSplineType::Periodic1 => points.iter().all(|p| p.y == 1.0),
    };

    if !identity {
        return false;
    }
    if !slopes_are_default(slopes) {
        identity = false;
    }
    identity
}

fn prep_hue_curve_data(
    ctrl: &[GradingControlPoint],
    is_periodic: bool,
    is_horizontal: bool,
) -> Vec<GradingControlPoint> {
    let mut out = Vec::with_capacity(ctrl.len() + 2);
    for p in ctrl {
        let mut x = p.x;
        let mut y = p.y;
        if is_periodic && x < 0.0 {
            x += 1.0;
            if !is_horizontal {
                y += 1.0;
            }
        } else if is_periodic && x >= 1.0 {
            x -= 1.0;
            if !is_horizontal {
                y -= 1.0;
            }
        }
        out.push(GradingControlPoint { x, y });
    }

    // sort by x
    out.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());

    // minimum spacing
    let tol = 2e-3_f32;
    let x_span = out[out.len() - 1].x - out[0].x;
    for i in 1..out.len() {
        if (out[i].x - out[i - 1].x) < x_span * tol {
            out[i].x = out[i - 1].x + x_span * tol;
        }
    }
    if !is_horizontal {
        let y_span = out[out.len() - 1].y - out[0].y;
        for i in 1..out.len() {
            if (out[i].y - out[i - 1].y) < y_span * tol {
                out[i].y = out[i - 1].y + y_span * tol;
            }
        }
    }

    if is_periodic {
        let mut first = out[out.len() - 1];
        first.x -= 1.0;
        if !is_horizontal {
            first.y -= 1.0;
        }
        out.insert(0, first);

        let mut last = out[1];
        last.x += 1.0;
        if !is_horizontal {
            last.y += 1.0;
        }
        out.push(last);
    }

    out
}

fn estimate_hue_slopes(
    ctrl: &[GradingControlPoint],
    is_periodic: bool,
    is_horizontal: bool,
) -> Vec<f32> {
    let mut secant_slope = Vec::new();
    let mut secant_len = Vec::new();
    for i in 0..ctrl.len() - 1 {
        let dx = ctrl[i + 1].x - ctrl[i].x;
        let dy = ctrl[i + 1].y - ctrl[i].y;
        secant_slope.push(dy / dx);
        secant_len.push((dx * dx + dy * dy).sqrt());
    }

    let num = ctrl.len();
    if num == 2 {
        return vec![secant_slope[0], secant_slope[0]];
    }

    let mut slopes = Vec::with_capacity(num);
    slopes.push(0.0);

    if is_horizontal {
        for i in 1..num - 1 {
            let denom = secant_slope[i] + secant_slope[i - 1];
            let mut s = if denom.abs() < 1e-3 {
                let minval = if denom < 0.0 { -1e-3 } else { 1e-3 };
                2.0 * secant_slope[i] * secant_slope[i - 1] / minval
            } else {
                2.0 * secant_slope[i] * secant_slope[i - 1] / denom
            };
            if secant_slope[i] * secant_slope[i - 1] <= 0.0 {
                s = 0.0;
            }
            slopes.push(s);
        }
        slopes.push(0.5 * (3.0 * secant_slope[num - 2] - slopes[num - 2]));
        slopes[0] = 0.5 * (3.0 * secant_slope[0] - slopes[1]);
    } else {
        let mut i = 0usize;
        while i < num - 1 {
            let mut j = i;
            let mut dl = secant_len[i];
            while j < num - 2 && (secant_slope[j + 1] - secant_slope[j]).abs() < 1e-6 {
                dl += secant_len[j + 1];
                j += 1;
            }
            secant_len[i..=j].fill(dl);
            if j >= num - 3 {
                break;
            }
            i = j + 1;
        }
        for k in 1..num - 1 {
            let s = (secant_len[k] * secant_slope[k] + secant_len[k - 1] * secant_slope[k - 1])
                / (secant_len[k] + secant_len[k - 1]);
            slopes.push(s);
        }
        let min_slope = 0.01;
        slopes.push((0.5 * (3.0 * secant_slope[num - 2] - slopes[num - 2])).max(min_slope));
        slopes[0] = (0.5 * (3.0 * secant_slope[0] - slopes[1])).max(min_slope);
    }

    for i in 0..num - 1 {
        let mut k = 0.2;
        if slopes[i].abs() > slopes[i + 1].abs() {
            k = 1.0 - k;
        }
        let m_near_min = slopes[i] + k * (slopes[i + 1] - slopes[i]);
        let mut scale = 1.0;
        if m_near_min != 0.0 {
            scale = 0.75 * 2.0 * secant_slope[i] / m_near_min;
        }
        if scale < 1.0 {
            slopes[i] *= scale;
            slopes[i + 1] *= scale;
        }
    }

    if is_periodic {
        slopes[0] = slopes[num - 2];
        slopes[num - 1] = slopes[1];
    }

    slopes
}

fn calc_ksi(i: usize, ctrl: &[GradingControlPoint], slopes: &[f32]) -> f32 {
    let p0 = ctrl[i];
    let p1 = ctrl[i + 1];
    let k = 0.2_f32;
    let dx = p1.x - p0.x;
    let secant_slope = (p1.y - p0.y) / dx;
    let mut secant = secant_slope;
    let mut m0 = slopes[i];
    let mut m1 = slopes[i + 1];
    if secant < 0.0 {
        m0 = -m0;
        m1 = -m1;
        secant = -secant;
    }

    let x_mid = p0.x + 0.5 * dx;
    let left_bnd = p0.x + dx * k;
    let right_bnd = p1.x - dx * k;
    let mut top_bnd = left_bnd;
    let mut bottom_bnd = right_bnd;
    let mut m_min = m0;
    let mut m_max = m1;
    if m0 > m1 {
        m_max = m0;
        m_min = m1;
        top_bnd = right_bnd;
        bottom_bnd = left_bnd;
    }

    let dm = m_max - m_min;
    let b = 1.0 - 0.5 * k;
    let b_high = m_min + b * dm;
    let b_low = m_min + (1.0 - b) * dm;
    let bbb = m_max * 4.0;
    let bb = m_max * 1.1;
    let m_rel_diff = dm / 0.01_f32.max(m_max);
    let alpha = 0.0_f32.max(((m_rel_diff - 0.05) / (0.75 - 0.05)).min(1.0));
    top_bnd = x_mid + alpha * (top_bnd - x_mid);
    bottom_bnd = x_mid + alpha * (bottom_bnd - x_mid);

    if secant >= bbb {
        x_mid
    } else if secant > bb {
        let blend = (secant - bb) / (bbb - bb);
        top_bnd + blend * (x_mid - top_bnd)
    } else if secant >= b_high {
        top_bnd
    } else if secant > b_low && (b_high - b_low).abs() > 1e-10 {
        let blend = (secant - b_low) / (b_high - b_low);
        bottom_bnd + blend * (top_bnd - bottom_bnd)
    } else {
        bottom_bnd
    }
}

fn fit_hue_spline(
    ctrl: &[GradingControlPoint],
    slopes: &[f32],
    knots: &mut Vec<f32>,
    coefs_a: &mut Vec<f32>,
    coefs_b: &mut Vec<f32>,
    coefs_c: &mut Vec<f32>,
) {
    knots.push(ctrl[0].x);
    for i in 0..ctrl.len() - 1 {
        let p0 = ctrl[i];
        let p1 = ctrl[i + 1];
        let dx = p1.x - p0.x;
        let secant = (p1.y - p0.y) / dx;
        if ((slopes[i] + slopes[i + 1]) - 2.0 * secant).abs() <= 1e-5 {
            coefs_c.push(p0.y);
            coefs_b.push(slopes[i]);
            coefs_a.push(0.5 * (slopes[i + 1] - slopes[i]) / dx);
        } else {
            let ksi = calc_ksi(i, ctrl, slopes);
            let m_bar = (2.0 * secant - slopes[i + 1])
                + (slopes[i + 1] - slopes[i]) * (ksi - p0.x) / (p1.x - p0.x);
            let eta = (m_bar - slopes[i]) / (ksi - p0.x);
            coefs_c.push(p0.y);
            coefs_b.push(slopes[i]);
            coefs_a.push(0.5 * eta);
            let dk = ksi - p0.x;
            coefs_c.push(p0.y + slopes[i] * dk + 0.5 * eta * dk * dk);
            coefs_b.push(m_bar);
            coefs_a.push(0.5 * (slopes[i + 1] - m_bar) / (p1.x - ksi));
            knots.push(ksi);
        }
        knots.push(p1.x);
    }
}

fn estimate_rgb_slopes(ctrl: &[GradingControlPoint]) -> Vec<f32> {
    let mut secant_slope = Vec::new();
    let mut secant_len = Vec::new();
    for i in 0..ctrl.len() - 1 {
        let dx = ctrl[i + 1].x - ctrl[i].x;
        let dy = ctrl[i + 1].y - ctrl[i].y;
        secant_slope.push(dy / dx);
        secant_len.push((dx * dx + dy * dy).sqrt());
    }
    let num = ctrl.len();
    if num == 2 {
        return vec![secant_slope[0], secant_slope[0]];
    }
    let mut i = 0usize;
    while i < num - 1 {
        let mut j = i;
        let mut dl = secant_len[i];
        while j < num - 2 && (secant_slope[j + 1] - secant_slope[j]).abs() < 1e-6 {
            dl += secant_len[j + 1];
            j += 1;
        }
        secant_len[i..=j].fill(dl);
        if j >= num - 3 {
            break;
        }
        i = j + 1;
    }
    let mut slopes = Vec::with_capacity(num);
    slopes.push(0.0);
    for k in 1..num - 1 {
        let s = (secant_len[k] * secant_slope[k] + secant_len[k - 1] * secant_slope[k - 1])
            / (secant_len[k] + secant_len[k - 1]);
        slopes.push(s);
    }
    slopes.push((0.5 * (3.0 * secant_slope[num - 2] - slopes[num - 2])).max(0.01));
    slopes[0] = (0.5 * (3.0 * secant_slope[0] - slopes[1])).max(0.01);
    slopes
}

fn fit_rgb_spline(
    ctrl: &[GradingControlPoint],
    slopes: &[f32],
    knots: &mut Vec<f32>,
    coefs_a: &mut Vec<f32>,
    coefs_b: &mut Vec<f32>,
    coefs_c: &mut Vec<f32>,
) {
    knots.push(ctrl[0].x);
    for i in 0..ctrl.len() - 1 {
        let xi = ctrl[i].x;
        let xi1 = ctrl[i + 1].x;
        let yi = ctrl[i].y;
        let yi1 = ctrl[i + 1].y;
        let del_x = xi1 - xi;
        let del_y = yi1 - yi;
        let secant = del_y / del_x;
        if ((slopes[i] + slopes[i + 1]) - 2.0 * secant).abs() < 1e-6 {
            coefs_c.push(yi);
            coefs_b.push(slopes[i]);
            coefs_a.push(0.5 * (slopes[i + 1] - slopes[i]) / del_x);
        } else {
            let aa = slopes[i] - secant;
            let bb = slopes[i + 1] - secant;
            let ksi = if aa * bb >= 0.0 {
                (xi + xi1) * 0.5
            } else if aa.abs() > bb.abs() {
                xi1 + aa * del_x / (slopes[i + 1] - slopes[i])
            } else {
                xi + bb * del_x / (slopes[i + 1] - slopes[i])
            };
            let s_bar =
                (2.0 * secant - slopes[i + 1]) + (slopes[i + 1] - slopes[i]) * (ksi - xi) / del_x;
            let eta = (s_bar - slopes[i]) / (ksi - xi);
            coefs_c.push(yi);
            coefs_b.push(slopes[i]);
            coefs_a.push(0.5 * eta);
            coefs_c.push(yi + slopes[i] * (ksi - xi) + 0.5 * eta * (ksi - xi) * (ksi - xi));
            coefs_b.push(s_bar);
            coefs_a.push(0.5 * (slopes[i + 1] - s_bar) / (xi1 - ksi));
            knots.push(ksi);
        }
        knots.push(xi1);
    }
}

fn adjust_rgb_slopes(ctrl: &[GradingControlPoint], slopes: &mut [f32], knots: &[f32]) -> bool {
    let mut adjusted = false;
    let mut i = 0usize;
    let mut j = 0usize;
    while j < knots.len() {
        if (ctrl[i].x - knots[j]).abs() > 1e-9 {
            let ksi = knots[j];
            let xi = ctrl[i].x;
            let xi1 = ctrl[i + 1].x;
            let yi = ctrl[i].y;
            let yi1 = ctrl[i + 1].y;
            let s_bar = (2.0 * (yi1 - yi) - (ksi - xi) * slopes[i] - (xi1 - ksi) * slopes[i + 1])
                / (xi1 - xi);
            if s_bar < 0.0 {
                adjusted = true;
                let secant = (yi1 - yi) / (xi1 - xi);
                let blend = ((ksi - xi) * slopes[i] + (xi1 - ksi) * slopes[i + 1]) / (xi1 - xi);
                let mut aim = 0.01 * 0.5 * (slopes[i] + slopes[i + 1]);
                if aim > secant {
                    aim = secant;
                }
                let adjust = (2.0 * secant - aim) / blend;
                slopes[i] *= adjust;
                slopes[i + 1] *= adjust;
            }
            i += 1;
        }
        j += 1;
    }
    adjusted
}

/// Fit the four RGB curves (Red, Green, Blue, Master) into the flat knots/coefs layout.
///
/// Widened from `pub(crate)` to `pub` (PLAN9.md C10): `vfx-ops`'s own `grading_rgb_curve` used
/// to carry a second, independent port of this exact fitting logic, which had drifted from it
/// in two confirmed places (an exact- vs epsilon-tolerant knot comparison in the slope
/// adjustment pass, and a different segment-search fallthrough in reverse evaluation). Routed
/// through this one copy instead, the same fix `grading_hue_curve` already got via
/// `build_hue_uniforms`.
pub fn build_rgb_uniforms(
    curves: [&[GradingControlPoint]; RGB_CURVE_COUNT],
    slopes: [&[f32]; RGB_CURVE_COUNT],
) -> CurveUniformData<{ RGB_CURVE_COUNT * 2 }> {
    let mut data = CurveUniformData::<{ RGB_CURVE_COUNT * 2 }>::new();
    let mut num_knots = 0usize;
    let mut num_coefs = 0usize;

    for (curve_idx, (points, slope_vals)) in curves.iter().zip(slopes.iter()).enumerate() {
        if points.len() < 2 || is_identity(points, BSplineType::BSpline, slope_vals) {
            data.knots_offsets[curve_idx * 2] = -1;
            data.knots_offsets[curve_idx * 2 + 1] = 0;
            data.coefs_offsets[curve_idx * 2] = -1;
            data.coefs_offsets[curve_idx * 2 + 1] = 0;
            continue;
        }

        let mut knots = Vec::new();
        let mut ca = Vec::new();
        let mut cb = Vec::new();
        let mut cc = Vec::new();

        let mut slopes_vec = if !slopes_are_default(slope_vals) && slope_vals.len() == points.len()
        {
            slope_vals.to_vec()
        } else {
            estimate_rgb_slopes(points)
        };

        fit_rgb_spline(points, &slopes_vec, &mut knots, &mut ca, &mut cb, &mut cc);
        if adjust_rgb_slopes(points, &mut slopes_vec, &knots) {
            knots.clear();
            ca.clear();
            cb.clear();
            cc.clear();
            fit_rgb_spline(points, &slopes_vec, &mut knots, &mut ca, &mut cb, &mut cc);
        }

        let new_knots = knots.len();
        let new_coefs = ca.len() * 3;
        if num_knots + new_knots > MAX_NUM_KNOTS || num_coefs + new_coefs > MAX_NUM_COEFS {
            // Clamp by dropping the curve (OCIO would throw; we keep safe default).
            data.knots_offsets[curve_idx * 2] = -1;
            data.knots_offsets[curve_idx * 2 + 1] = 0;
            data.coefs_offsets[curve_idx * 2] = -1;
            data.coefs_offsets[curve_idx * 2 + 1] = 0;
            continue;
        }

        data.knots_offsets[curve_idx * 2] = num_knots as i32;
        data.knots_offsets[curve_idx * 2 + 1] = new_knots as i32;
        data.coefs_offsets[curve_idx * 2] = num_coefs as i32;
        data.coefs_offsets[curve_idx * 2 + 1] = new_coefs as i32;

        data.knots.extend_from_slice(&knots);
        data.coefs.extend_from_slice(&ca);
        data.coefs.extend_from_slice(&cb);
        data.coefs.extend_from_slice(&cc);

        num_knots += new_knots;
        num_coefs += new_coefs;
    }

    data.local_bypass = data.all_bypassed();
    data
}

/// Fit all eight hue curves into the flat knots/coefs layout.
///
/// Pass an empty slope slice for a curve to have its slopes estimated the way the
/// reference does (`EstimateHueSlopes`). `draw_curve_only` mirrors OCIO's
/// `GradingHueCurveDrawOpCPU` mode, which materialises identity curves instead of
/// bypassing them.
pub fn build_hue_uniforms(
    curves: [&[GradingControlPoint]; HUE_CURVE_COUNT],
    slopes: [&[f32]; HUE_CURVE_COUNT],
    draw_curve_only: bool,
) -> CurveUniformData<{ HUE_CURVE_COUNT * 2 }> {
    let mut data = CurveUniformData::<{ HUE_CURVE_COUNT * 2 }>::new();
    let mut num_knots = 0usize;
    let mut num_coefs = 0usize;

    for curve_idx in 0..HUE_CURVE_COUNT {
        let points = curves[curve_idx];
        let slope_vals = slopes[curve_idx];
        let spline = hue_curve_spline_type(curve_idx);

        if points.len() < 2 || is_identity(points, spline, slope_vals) {
            if !draw_curve_only {
                data.knots_offsets[curve_idx * 2] = -1;
                data.knots_offsets[curve_idx * 2 + 1] = 0;
                data.coefs_offsets[curve_idx * 2] = -1;
                data.coefs_offsets[curve_idx * 2 + 1] = 0;
                continue;
            }

            // drawCurveOnly identity curve
            let num_knots_i = num_knots;
            let num_coefs_i = num_coefs;
            const N_ID_KNOTS: usize = 2;
            const N_ID_COEFS: usize = 3;

            data.knots_offsets[curve_idx * 2] = num_knots_i as i32;
            data.knots_offsets[curve_idx * 2 + 1] = N_ID_KNOTS as i32;
            data.coefs_offsets[curve_idx * 2] = num_coefs_i as i32;
            data.coefs_offsets[curve_idx * 2 + 1] = N_ID_COEFS as i32;

            data.knots.push(0.0);
            data.knots.push(1.0);

            data.coefs.push(0.0); // A
            let linear = match spline {
                BSplineType::Diagonal | BSplineType::HueHue => 1.0,
                _ => 0.0,
            };
            data.coefs.push(linear); // B
            let constant = match spline {
                BSplineType::Periodic1 | BSplineType::Horizontal1 => 1.0,
                _ => 0.0,
            };
            data.coefs.push(constant); // C

            num_knots += N_ID_KNOTS;
            num_coefs += N_ID_COEFS;
            continue;
        }

        let is_periodic = matches!(
            spline,
            BSplineType::Periodic0 | BSplineType::Periodic1 | BSplineType::HueHue
        );
        // OCIO `computeKnotsAndCoefs`: every hue spline is horizontal (its y values are not
        // shifted when wrapped) except the diagonal ones. The periodic HUE_SAT / HUE_LUM /
        // HUE_FX curves are horizontal; treating them as diagonal shifted a wrapped point's y by
        // one and bent every non-identity hue-sat/hue-lum/hue-fx curve.
        let mut is_horizontal = !matches!(spline, BSplineType::Diagonal | BSplineType::HueHue);

        let ctrl = prep_hue_curve_data(points, is_periodic, is_horizontal);

        if matches!(spline, BSplineType::HueHue) {
            is_horizontal = true; // for slope estimation
        }

        let slopes_vec = if !slopes_are_default(slope_vals) && slope_vals.len() == points.len() {
            let mut v = slope_vals.to_vec();
            if is_periodic {
                let first = *v.last().unwrap();
                v.insert(0, first);
                let last = v[1];
                v.push(last);
            }
            v
        } else {
            estimate_hue_slopes(&ctrl, is_periodic, is_horizontal)
        };

        let mut knots = Vec::new();
        let mut ca = Vec::new();
        let mut cb = Vec::new();
        let mut cc = Vec::new();
        fit_hue_spline(&ctrl, &slopes_vec, &mut knots, &mut ca, &mut cb, &mut cc);

        let new_knots = knots.len();
        let new_coefs = ca.len() * 3;
        if num_knots + new_knots > MAX_NUM_KNOTS || num_coefs + new_coefs > MAX_NUM_COEFS {
            data.knots_offsets[curve_idx * 2] = -1;
            data.knots_offsets[curve_idx * 2 + 1] = 0;
            data.coefs_offsets[curve_idx * 2] = -1;
            data.coefs_offsets[curve_idx * 2 + 1] = 0;
            continue;
        }

        data.knots_offsets[curve_idx * 2] = num_knots as i32;
        data.knots_offsets[curve_idx * 2 + 1] = new_knots as i32;
        data.coefs_offsets[curve_idx * 2] = num_coefs as i32;
        data.coefs_offsets[curve_idx * 2 + 1] = new_coefs as i32;

        data.knots.extend_from_slice(&knots);
        data.coefs.extend_from_slice(&ca);
        data.coefs.extend_from_slice(&cb);
        data.coefs.extend_from_slice(&cc);

        num_knots += new_knots;
        num_coefs += new_coefs;
    }

    data.local_bypass = data.all_bypassed();
    data
}

/// Evaluate a fitted curve forward as `(A*t + B)*t + C`, with linear extrapolation
/// beyond the end knots.
///
/// Returns `identity_x` when the curve is bypassed.
/// Reference: `KnotsCoefs::evalCurve` (`GradingBSplineCurve.cpp:1258`).
pub fn eval_curve_fwd(
    curve_idx: usize,
    x: f32,
    identity_x: f32,
    knots_offsets: &[i32],
    coefs_offsets: &[i32],
    knots: &[f32],
    coefs: &[f32],
) -> f32 {
    let knots_offs = knots_offsets[curve_idx * 2];
    let knots_cnt = knots_offsets[curve_idx * 2 + 1];
    let coefs_offs = coefs_offsets[curve_idx * 2];
    let coefs_cnt = coefs_offsets[curve_idx * 2 + 1];
    if knots_offs < 0 || coefs_offs < 0 || knots_cnt <= 0 || coefs_cnt <= 0 {
        return identity_x;
    }
    let knots_offs = knots_offs as usize;
    let knots_cnt = knots_cnt as usize;
    let coefs_offs = coefs_offs as usize;
    let coefs_cnt = coefs_cnt as usize;
    let coefs_sets = coefs_cnt / 3;
    if coefs_sets == 0 {
        return identity_x;
    }

    let kn_start = knots[knots_offs];
    let kn_end = knots[knots_offs + knots_cnt - 1];

    if x <= kn_start {
        let b = coefs[coefs_offs + coefs_sets];
        let c = coefs[coefs_offs + coefs_sets * 2];
        return (x - kn_start) * b + c;
    } else if x >= kn_end {
        let a = coefs[coefs_offs + coefs_sets - 1];
        let b = coefs[coefs_offs + coefs_sets * 2 - 1];
        let c = coefs[coefs_offs + coefs_sets * 3 - 1];
        let kn = knots[knots_offs + knots_cnt - 2];
        let t = kn_end - kn;
        let slope = 2.0 * a * t + b;
        let offs = (a * t + b) * t + c;
        return (x - kn_end) * slope + offs;
    }

    // OCIO `for (i = 0; i < knotsCnt - 2; ++i) if (..) break;`: no match leaves i on the
    // LAST segment (knotsCnt - 2), not the one before it.
    let mut i = knots_cnt.saturating_sub(2);
    for idx in 0..knots_cnt - 2 {
        if x < knots[knots_offs + idx + 1] {
            i = idx;
            break;
        }
    }

    let a = coefs[coefs_offs + i];
    let b = coefs[coefs_offs + coefs_sets + i];
    let c = coefs[coefs_offs + coefs_sets * 2 + i];
    let kn = knots[knots_offs + i];
    let t = x - kn;
    (a * t + b) * t + c
}

/// Invert a fitted monotonic curve in closed form, per quadratic segment.
///
/// Reference: `KnotsCoefs::evalCurveRev` (`GradingBSplineCurve.cpp:1311`).
pub fn eval_curve_rev(
    curve_idx: usize,
    x: f32,
    knots_offsets: &[i32],
    coefs_offsets: &[i32],
    knots: &[f32],
    coefs: &[f32],
) -> f32 {
    let knots_offs = knots_offsets[curve_idx * 2];
    let knots_cnt = knots_offsets[curve_idx * 2 + 1];
    let coefs_offs = coefs_offsets[curve_idx * 2];
    let coefs_cnt = coefs_offsets[curve_idx * 2 + 1];
    if knots_offs < 0 || coefs_offs < 0 || knots_cnt <= 0 || coefs_cnt <= 0 {
        return x;
    }
    let knots_offs = knots_offs as usize;
    let knots_cnt = knots_cnt as usize;
    let coefs_offs = coefs_offs as usize;
    let coefs_cnt = coefs_cnt as usize;
    let coefs_sets = coefs_cnt / 3;
    if coefs_sets == 0 {
        return x;
    }

    let kn_start = knots[knots_offs];
    let kn_end = knots[knots_offs + knots_cnt - 1];
    let kn_start_y = coefs[coefs_offs + coefs_sets * 2];

    let kn_end_y = {
        let a = coefs[coefs_offs + coefs_sets - 1];
        let b = coefs[coefs_offs + coefs_sets * 2 - 1];
        let c = coefs[coefs_offs + coefs_sets * 3 - 1];
        let kn = knots[knots_offs + knots_cnt - 2];
        let t = kn_end - kn;
        (a * t + b) * t + c
    };

    if x <= kn_start_y {
        let b = coefs[coefs_offs + coefs_sets];
        let c = coefs[coefs_offs + coefs_sets * 2];
        return if b.abs() < 1e-5 {
            kn_start
        } else {
            (x - c) / b + kn_start
        };
    } else if x >= kn_end_y {
        let a = coefs[coefs_offs + coefs_sets - 1];
        let b = coefs[coefs_offs + coefs_sets * 2 - 1];
        let c = coefs[coefs_offs + coefs_sets * 3 - 1];
        let kn = knots[knots_offs + knots_cnt - 2];
        let t = kn_end - kn;
        let slope = 2.0 * a * t + b;
        let offs = (a * t + b) * t + c;
        return if slope.abs() < 1e-5 {
            kn_end
        } else {
            (x - offs) / slope + kn_end
        };
    }

    // OCIO `for (i = 0; i < knotsCnt - 2; ++i) if (..) break;`: no match leaves i on the
    // LAST segment (knotsCnt - 2), not the one before it.
    let mut i = knots_cnt.saturating_sub(2);
    for idx in 0..knots_cnt - 2 {
        if x < coefs[coefs_offs + coefs_sets * 2 + idx + 1] {
            i = idx;
            break;
        }
    }

    let a = coefs[coefs_offs + i];
    let b = coefs[coefs_offs + coefs_sets + i];
    let c = coefs[coefs_offs + coefs_sets * 2 + i];
    let kn = knots[knots_offs + i];
    let c0 = c - x;
    // `denom` vanishes exactly for a linear segment with negative slope (A~=0, B<0), where
    // `discrim = sqrt(B*B) = -B`. Without this branch the result is +/-inf (NaN when
    // C0 == 0) straight into the output pixel. Reference: `GradingBSplineCurve.cpp:1370-1379`.
    let discrim = (b * b - 4.0 * a * c0).sqrt();
    let denom = discrim + b;
    if denom.abs() < 1e-5 {
        return if b.abs() < 1e-5 { kn } else { kn + (-c0 / b) };
    }
    kn + (-2.0 * c0) / denom
}

/// Invert a periodic hue curve in closed form.
///
/// These curves define `HueOut = HueIn + Curve(HueIn)` (HUE_FX is a pure delta; HUE_HUE is
/// fitted so the same relation holds). This takes `HueOut` and returns `HueIn`.
///
/// Reference: `KnotsCoefs::evalCurveRevHue` (`GradingBSplineCurve.cpp:1385-1459`).
///
/// # Why this is not a copy of [`eval_curve_rev`]
///
/// An earlier port reused the monotonic reverse's out-of-range *extrapolation* branches and
/// omitted the HUE_FX diagonal (`C += kn; B += 1`). That inverts a different function for
/// HUE_FX and sends out-of-range hues off the periodic domain instead of wrapping them back
/// with `ceil` — both disagree with the reference and with the GPU emitters in
/// `gpu/grading.rs` (`evalBSplineCurveRevHue`). Found while planning C10 HueCurve
/// unification (`.recover/c10/B_hue_plan.md` B1).
///
/// One deliberate CPU/GPU split matches the reference's own: ref CPU shifts `knStartY` by
/// `knStart` when `isHfx` (`:1408`); ref GPU and our emitters do not. We follow ref CPU here.
pub fn eval_curve_rev_hue(
    curve_idx: usize,
    mut x: f32,
    knots_offsets: &[i32],
    coefs_offsets: &[i32],
    knots: &[f32],
    coefs: &[f32],
) -> f32 {
    let knots_offs = knots_offsets[curve_idx * 2];
    let knots_cnt = knots_offsets[curve_idx * 2 + 1];
    let coefs_offs = coefs_offsets[curve_idx * 2];
    let coefs_cnt = coefs_offsets[curve_idx * 2 + 1];
    if knots_offs < 0 || coefs_offs < 0 || knots_cnt <= 0 || coefs_cnt <= 0 {
        return x;
    }
    let knots_offs = knots_offs as usize;
    let knots_cnt = knots_cnt as usize;
    let coefs_offs = coefs_offs as usize;
    let coefs_cnt = coefs_cnt as usize;
    let coefs_sets = coefs_cnt / 3;
    if coefs_sets == 0 {
        return x;
    }

    // Slot 7 is HUE_FX (Periodic0 delta). Same index the GPU emitters use (`curveIdx == 7`).
    let is_hfx = curve_idx == 7;

    let kn_start = knots[knots_offs];
    let kn_end = knots[knots_offs + knots_cnt - 1];
    let mut kn_start_y = coefs[coefs_offs + coefs_sets * 2];
    // Ref CPU only (`GradingBSplineCurve.cpp:1408`); ref GPU / our emitters omit this shift.
    if is_hfx {
        kn_start_y += kn_start;
    }

    let mut kn_end_y = {
        let a = coefs[coefs_offs + coefs_sets - 1];
        let b = coefs[coefs_offs + coefs_sets * 2 - 1];
        let c = coefs[coefs_offs + coefs_sets * 3 - 1];
        let kn = knots[knots_offs + knots_cnt - 2];
        let t = kn_end - kn;
        (a * t + b) * t + c
    };
    if is_hfx {
        kn_end_y += kn_end;
    }

    // Periodic wrap into the fitted hue range — NOT the monotonic reverse's extrapolation.
    if x < kn_start_y {
        x += (kn_start_y - x).ceil();
    } else if x > kn_end_y {
        x -= (x - kn_end_y).ceil();
    }

    // OCIO `for (i = 0; i < knotsCnt - 2; ++i) if (..) break;`: no match leaves i on the
    // LAST segment (knotsCnt - 2), not the one before it.
    let mut i = knots_cnt.saturating_sub(2);
    for idx in 0..knots_cnt - 2 {
        let mut curve_y = coefs[coefs_offs + coefs_sets * 2 + idx + 1];
        if is_hfx {
            curve_y += knots[knots_offs + idx + 1];
        }
        if x < curve_y {
            i = idx;
            break;
        }
    }

    let a = coefs[coefs_offs + i];
    let mut b = coefs[coefs_offs + coefs_sets + i];
    let mut c = coefs[coefs_offs + coefs_sets * 2 + i];
    let kn = knots[knots_offs + i];
    if is_hfx {
        // Shift the delta curve onto the main diagonal so we invert HueIn+Curve(HueIn).
        c += kn;
        b += 1.0;
    }
    let c0 = c - x;
    let discrim = (b * b - 4.0 * a * c0).max(0.0).sqrt();
    let denom = discrim + b;
    // Same guard as [`eval_curve_rev`] / ref `:1452-1456` — flat/near-flat segments otherwise /
    // ~0.
    if denom.abs() < 1e-5 {
        return if b.abs() < 1e-5 { kn } else { kn + (-c0 / b) };
    }
    kn + (-2.0 * c0) / denom
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PLAN9.md C10: `local_bypass` used to be computed as
    /// `knots_offsets.iter().all(|v| *v == -1) && coefs_offsets.iter().all(|v| *v == -1)`, but
    /// a bypassed curve's `(offset, count)` pair is `(-1, 0)`, not `(-1, -1)` — the interleaved
    /// `count = 0` entries made that check impossible to ever satisfy. Went unnoticed because
    /// no caller ever fed `build_rgb_uniforms`/`build_hue_uniforms` a real (non-empty-slice)
    /// all-identity curve set AND checked `local_bypass`'s truth — `HueCurvePreRender` never
    /// even exposes it. Found while wiring `vfx-ops`'s `grading_rgb_curve` onto this module.
    #[test]
    fn all_bypassed_true_when_every_rgb_curve_is_identity() {
        let identity = [
            GradingControlPoint { x: 0.0, y: 0.0 },
            GradingControlPoint { x: 1.0, y: 1.0 },
        ];
        let e: [f32; 0] = [];
        let data = build_rgb_uniforms(
            [&identity, &identity, &identity, &identity],
            [&e, &e, &e, &e],
        );
        assert!(
            data.local_bypass,
            "all four identity curves must report local_bypass=true"
        );
    }

    #[test]
    fn all_bypassed_false_when_one_rgb_curve_is_not_identity() {
        let identity = [
            GradingControlPoint { x: 0.0, y: 0.0 },
            GradingControlPoint { x: 1.0, y: 1.0 },
        ];
        let lifted = [
            GradingControlPoint { x: 0.0, y: 0.1 },
            GradingControlPoint { x: 0.5, y: 0.6 },
            GradingControlPoint { x: 1.0, y: 1.0 },
        ];
        let e: [f32; 0] = [];
        let data = build_rgb_uniforms([&lifted, &identity, &identity, &identity], [&e, &e, &e, &e]);
        assert!(
            !data.local_bypass,
            "one non-identity curve must make local_bypass=false"
        );
    }

    #[test]
    fn all_bypassed_true_when_every_hue_curve_is_identity() {
        // Each hue-curve slot's own identity shape (per `hue_curve_spline_type`): HueHue/
        // Diagonal identity is the diagonal, Periodic1/Horizontal1 identity is y=1,
        // Periodic0 identity is y=0.
        let diag = [
            GradingControlPoint { x: 0.0, y: 0.0 },
            GradingControlPoint { x: 1.0, y: 1.0 },
        ];
        let y1 = [
            GradingControlPoint { x: 0.0, y: 1.0 },
            GradingControlPoint { x: 1.0, y: 1.0 },
        ];
        let y0 = [
            GradingControlPoint { x: 0.0, y: 0.0 },
            GradingControlPoint { x: 1.0, y: 0.0 },
        ];
        // HueHue, Periodic1, Periodic1, Horizontal1, Diagonal, Diagonal, Horizontal1, Periodic0
        let curves: [&[GradingControlPoint]; HUE_CURVE_COUNT] =
            [&diag, &y1, &y1, &y1, &diag, &diag, &y1, &y0];
        let e: [f32; 0] = [];
        let slopes: [&[f32]; HUE_CURVE_COUNT] = [&e, &e, &e, &e, &e, &e, &e, &e];
        let data = build_hue_uniforms(curves, slopes, false);
        assert!(
            data.local_bypass,
            "all eight identity hue curves must report local_bypass=true"
        );
    }

    /// C10 B1: HUE_FX inverse needs `C += kn; B += 1` so it inverts `HueIn + Curve(HueIn)`,
    /// not the bare delta curve. Without the diagonal, round-trip error stays large.
    ///
    /// The periodic `ceil` wrap (same commit) is the other half of the ref port; it is harder to
    /// isolate on a synthetic knot set without re-deriving the fitted `knStartY`, so it rides on
    /// this round-trip and on the GPU emitter which already had the wrap.
    #[test]
    fn rev_hue_fx_roundtrips_hue_in_plus_delta() {
        let diag = [
            GradingControlPoint { x: 0.0, y: 0.0 },
            GradingControlPoint { x: 1.0, y: 1.0 },
        ];
        let y1 = [
            GradingControlPoint { x: 0.0, y: 1.0 },
            GradingControlPoint { x: 1.0, y: 1.0 },
        ];
        // Non-trivial HUE_FX delta: bumps mid-hues by ~0.1.
        let hue_fx = [
            GradingControlPoint { x: 0.0, y: 0.0 },
            GradingControlPoint { x: 0.5, y: 0.1 },
            GradingControlPoint { x: 1.0, y: 0.0 },
        ];
        let refs: [&[GradingControlPoint]; HUE_CURVE_COUNT] =
            [&diag, &y1, &y1, &y1, &diag, &diag, &y1, &hue_fx];
        let e: [f32; 0] = [];
        let slopes: [&[f32]; HUE_CURVE_COUNT] = [&e, &e, &e, &e, &e, &e, &e, &e];
        let data = build_hue_uniforms(refs, slopes, false);

        for &h in &[0.15_f32, 0.35, 0.5, 0.65, 0.85] {
            let delta = eval_curve_fwd(
                7,
                h,
                0.0,
                &data.knots_offsets,
                &data.coefs_offsets,
                &data.knots,
                &data.coefs,
            );
            let hue_out = h + delta;
            let back = eval_curve_rev_hue(
                7,
                hue_out,
                &data.knots_offsets,
                &data.coefs_offsets,
                &data.knots,
                &data.coefs,
            );
            assert!(
                (back - h).abs() < 1e-3,
                "HUE_FX rev must recover HueIn: in={h} delta={delta} out={hue_out} back={back}"
            );
        }
    }
}
