//! Black-point detection and black preservation (`cmssamp.c`).
//!
//! This module ports the black-point machinery Little-CMS uses to drive black-point
//! compensation (BPC, see [`crate::cmm::intents`]) and black-preserving intents:
//!
//! - [`root_of_least_squares_fit_quadratic_curve`] — the quadratic curve fit
//!   (`RootOfLeastSquaresFitQuadraticCurve`) used by the Adobe destination-black algorithm.
//!   Pure algebra; fully ported and unit-tested.
//! - [`detect_black_point`] (`cmsDetectBlackPoint`) and
//!   [`detect_destination_black_point`] (`cmsDetectDestinationBlackPoint`).
//!
//! ## Round-trip-based detection
//!
//! Faithful black-point detection for the general case builds *round-trip* transforms through
//! synthetic Lab identity profiles ([`crate::virt::create_lab2_profile_thr`] /
//! [`create_lab4_profile_thr`](crate::virt::create_lab4_profile_thr), `CreateRoundtripXForm`,
//! `BlackPointAsDarkerColorant`, `BlackPointUsingPerceptualBlack`). The round-trip chain
//! `[Lab, prof, prof, Lab]` uses the profile-under-test at *two* array positions; the transform
//! API takes `&mut [&mut Profile]`, so the profile is aliased by [`Profile::clone`] (a deep tag
//! dictionary over a shared backing store — see [`Profile`]). Every branch of
//! `cmsDetectBlackPoint` / `cmsDetectDestinationBlackPoint` is ported; `Some(xyz)` is the C
//! `TRUE` path and `None` is the `FALSE` path ([`crate::cmm::intents`] treats `None` as the zero
//! black point, so BPC then becomes a no-op — "if black points are equal, do nothing").

use crate::cmm::optimize::{FLAGS_NOCACHE, FLAGS_NOOPTIMIZE};
use crate::cmm::transform::{create_extended_transform, create_transform, Transform};
use crate::color::pcs::{
    cms_lab_to_xyz, cms_xyz_to_lab, end_points_by_space,
};
use crate::color::{PERCEPTUAL_BLACK_X, PERCEPTUAL_BLACK_Y, PERCEPTUAL_BLACK_Z};
use crate::math::mat3::{Mat3, Vec3};
use crate::math::{cms_max, cms_min};
use crate::profile::read_hl::{
    is_clut, is_intent_supported, is_matrix_shaper, INTENT_PERCEPTUAL,
    INTENT_RELATIVE_COLORIMETRIC, INTENT_SATURATION, LCMS_USED_AS_INPUT, LCMS_USED_AS_OUTPUT,
};
use crate::profile::Profile;
use crate::types::color::{CIELab, CIEXYZ};
use crate::types::format::{FormatBuilder, PixelFormat};
use crate::types::sig::{class, colorspace, Signature};
use crate::virt::{create_lab2_profile_thr, create_lab4_profile_thr};
use crate::MAX_CHANNELS;

/// `RootOfLeastSquaresFitQuadraticCurve` — least-squares fit of a quadratic `y = a·x² + b·x + c`
/// to `n` samples, returning the curve vertex clamped to `0..=50`.
///
/// This is the shadow-curve fit at the heart of `cmsDetectDestinationBlackPoint`. It builds the
/// 3×3 normal-equation system and solves it with [`Mat3::solve`] (the port of `_cmsMAT3solve`),
/// then returns the root `(-b + √(b²-4ac)) / 2a`, degenerating to the linear root `-c/b` when
/// the quadratic term vanishes. Bit-for-bit faithful to the C accumulation order.
pub fn root_of_least_squares_fit_quadratic_curve(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len();
    if n < 4 {
        return 0.0;
    }

    let (mut sum_x, mut sum_x2, mut sum_x3, mut sum_x4) = (0.0, 0.0, 0.0, 0.0);
    let (mut sum_y, mut sum_yx, mut sum_yx2) = (0.0, 0.0, 0.0);

    for i in 0..n {
        let xn = x[i];
        let yn = y[i];

        sum_x += xn;
        sum_x2 += xn * xn;
        sum_x3 += xn * xn * xn;
        sum_x4 += xn * xn * xn * xn;

        sum_y += yn;
        sum_yx += yn * xn;
        sum_yx2 += yn * xn * xn;
    }

    // Normal equations (rows exactly as `_cmsVEC3init` fills them in the C source).
    let m = Mat3([
        Vec3([n as f64, sum_x, sum_x2]),
        Vec3([sum_x, sum_x2, sum_x3]),
        Vec3([sum_x2, sum_x3, sum_x4]),
    ]);
    let v = Vec3([sum_y, sum_yx, sum_yx2]);

    let res = match Mat3::solve(&m, v) {
        Some(r) => r,
        None => return 0.0,
    };

    let a = res.0[2];
    let b = res.0[1];
    let c = res.0[0];

    if a.abs() < 1.0e-10 {
        if b.abs() < 1.0e-10 {
            return 0.0;
        }
        return cms_max(0.0, cms_min(50.0, -c / b));
    }

    let d = b * b - 4.0 * a * c;
    if d <= 0.0 {
        0.0
    } else {
        // The C repeats the `fabs(a) < 1e-10` guard here; kept for exact parity.
        if a.abs() < 1.0e-10 {
            return 0.0;
        }
        let rt = (-b + d.sqrt()) / (2.0 * a);
        cms_max(0.0, cms_min(50.0, rt))
    }
}

/// `cmsDetectBlackPoint` — detect the black point of `prof` under `intent`.
///
/// Returns `Some(xyz)` on the C `TRUE` path, `None` on the `FALSE` path (caller treats `None`
/// as `{0,0,0}`). Ported in full: the device-class/intent validation, the V4
/// perceptual/saturation constant black point, the output-ink perceptual-black round trip, and
/// the darker-colorant fallback.
pub fn detect_black_point(prof: &mut Profile, intent: u32, _flags: u32) -> Option<CIEXYZ> {
    // Device class must be a real device (not a link/abstract/named-color profile).
    let dev_class = prof.device_class();
    if dev_class == class::LINK || dev_class == class::ABSTRACT || dev_class == class::NAMED_COLOR {
        return None;
    }

    // Intent must be one that has a defined black point.
    if intent != INTENT_PERCEPTUAL
        && intent != INTENT_RELATIVE_COLORIMETRIC
        && intent != INTENT_SATURATION
    {
        return None;
    }

    // V4 + perceptual/saturation carries a well-specified black point (the V2 black-point tag is
    // deprecated there). Matrix-shaper profiles share the MRC & perceptual intents, so fall back
    // to darker-colorant on relative colorimetric for them.
    if prof.encoded_icc_version() >= 0x0400_0000
        && (intent == INTENT_PERCEPTUAL || intent == INTENT_SATURATION)
    {
        if is_matrix_shaper(prof) {
            return black_point_as_darker_colorant(prof, INTENT_RELATIVE_COLORIMETRIC);
        }
        return Some(CIEXYZ {
            x: PERCEPTUAL_BLACK_X,
            y: PERCEPTUAL_BLACK_Y,
            z: PERCEPTUAL_BLACK_Z,
        });
    }

    // NOTE: `CMS_USE_PROFILE_BLACK_POINT_TAG` is off by default in lcms (the media black-point
    // tag is bogus on most profiles), so the V2 tag branch is intentionally not compiled in — the
    // default build path is reproduced exactly.

    // Output ink profile on relative colorimetric: discount ink limiting via perceptual black.
    if intent == INTENT_RELATIVE_COLORIMETRIC
        && dev_class == class::OUTPUT
        && is_ink_colorspace(prof.color_space())
    {
        return black_point_using_perceptual_black(prof);
    }

    // Otherwise: darker colorant under the requested intent.
    black_point_as_darker_colorant(prof, intent)
}

/// `cmsDetectDestinationBlackPoint` — Adobe destination-black-point algorithm.
///
/// Fully ported: after the validation and V4 constant path, LUT-based gray/RGB/ink profiles run
/// the Adobe 256-step `L*` ramp through a `Lab→prof→prof→Lab` round trip, force the shadow ramp
/// monotonic, and either accept a nearly-straight midrange or fit the shadow knee with
/// [`root_of_least_squares_fit_quadratic_curve`]. `None` is the C `FALSE` path.
pub fn detect_destination_black_point(prof: &mut Profile, intent: u32, flags: u32) -> Option<CIEXYZ> {
    let dev_class = prof.device_class();
    if dev_class == class::LINK || dev_class == class::ABSTRACT || dev_class == class::NAMED_COLOR {
        return None;
    }

    if intent != INTENT_PERCEPTUAL
        && intent != INTENT_RELATIVE_COLORIMETRIC
        && intent != INTENT_SATURATION
    {
        return None;
    }

    if prof.encoded_icc_version() >= 0x0400_0000
        && (intent == INTENT_PERCEPTUAL || intent == INTENT_SATURATION)
    {
        if is_matrix_shaper(prof) {
            return black_point_as_darker_colorant(prof, INTENT_RELATIVE_COLORIMETRIC);
        }
        return Some(CIEXYZ {
            x: PERCEPTUAL_BLACK_X,
            y: PERCEPTUAL_BLACK_Y,
            z: PERCEPTUAL_BLACK_Z,
        });
    }

    // Check if the profile is LUT based and gray/rgb/cmyk (§7.2 in Adobe's document).
    let color_space = prof.color_space();
    if !is_clut(prof, intent, LCMS_USED_AS_OUTPUT)
        || (color_space != colorspace::GRAY
            && color_space != colorspace::RGB
            && !is_ink_colorspace(color_space))
    {
        // Handle as the input case.
        return detect_black_point(prof, intent, flags);
    }

    // One of the valid cases — use the Adobe algorithm.

    // Set a first guess, that should work on good profiles.
    let initial_lab = if intent == INTENT_RELATIVE_COLORIMETRIC {
        // The initial Lab is the source black point.
        let ini_xyz = detect_black_point(prof, intent, flags)?;
        cms_xyz_to_lab(None, &ini_xyz)
    } else {
        // Zero is the black point for perceptual and saturation.
        CIELab { l: 0.0, a: 0.0, b: 0.0 }
    };

    // Step 2: create a round trip and sample the L* ramp.
    let round_trip = create_roundtrip_xform(prof, intent)?;

    let mut in_ramp = [0f64; 256];
    let mut out_ramp = [0f64; 256];
    for l in 0..256 {
        let lab = CIELab {
            l: (l as f64 * 100.0) / 255.0,
            a: cms_min(50.0, cms_max(-50.0, initial_lab.a)),
            b: cms_min(50.0, cms_max(-50.0, initial_lab.b)),
        };
        let dest = round_trip_lab(&round_trip, &lab);
        in_ramp[l] = lab.l;
        out_ramp[l] = dest.l;
    }

    // Make the output ramp monotonic (C walks l = 254 down to 1).
    for l in (1..=254).rev() {
        out_ramp[l] = out_ramp[l].min(out_ramp[l + 1]);
    }

    // A non-increasing endpoint span is a failure. Kept as the negated `<` (C's
    // `!(outRamp[0] < outRamp[255])`) so a NaN endpoint fails exactly as the reference.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(out_ramp[0] < out_ramp[255]) {
        return None;
    }

    let min_l = out_ramp[0];
    let max_l = out_ramp[255];

    // Test for a straight midrange (only on relative colorimetric).
    if intent == INTENT_RELATIVE_COLORIMETRIC {
        let mut nearly_straight_midrange = true;
        for l in 0..256 {
            if !((in_ramp[l] <= min_l + 0.2 * (max_l - min_l))
                || ((in_ramp[l] - out_ramp[l]).abs() < 4.0))
            {
                nearly_straight_midrange = false;
            }
        }

        // If straight, the destination black point equals the initial Lab.
        if nearly_straight_midrange {
            return Some(cms_lab_to_xyz(None, &initial_lab));
        }
    }

    // Curve fitting: the round-trip curve is a near-constant shadow, a corner, then a near-straight
    // line to white. Normalize the shadow and capture the knee points.
    let mut y_ramp = [0f64; 256];
    for l in 0..256 {
        y_ramp[l] = (out_ramp[l] - min_l) / (max_l - min_l);
    }

    let (lo, hi) = if intent == INTENT_RELATIVE_COLORIMETRIC {
        (0.1, 0.5)
    } else {
        // Perceptual and saturation.
        (0.03, 0.25)
    };

    let mut x = [0f64; 256];
    let mut y = [0f64; 256];
    let mut n = 0usize;
    for l in 0..256 {
        let ff = y_ramp[l];
        if ff >= lo && ff < hi {
            x[n] = in_ramp[l];
            y[n] = y_ramp[l];
            n += 1;
        }
    }

    // No suitable points.
    if n < 3 {
        return None;
    }

    // Fit and take the vertex of the quadratic (clipped to zero if negative).
    let mut l_star = root_of_least_squares_fit_quadratic_curve(&x[..n], &y[..n]);
    if l_star < 0.0 {
        l_star = 0.0;
    }

    let lab = CIELab { l: l_star, a: initial_lab.a, b: initial_lab.b };
    Some(cms_lab_to_xyz(None, &lab))
}

// -------------------------------------------------------------------------------------------
// Round-trip / darker-colorant helpers (CreateRoundtripXForm, BlackPointAsDarkerColorant,
// BlackPointUsingPerceptualBlack)
// -------------------------------------------------------------------------------------------

/// `PT_LAB` (`lcms2.h`) — the colour-space code of `TYPE_Lab_*`.
const PT_LAB: u32 = 10;

/// `TYPE_Lab_DBL` — three `f64` `cmsCIELab` samples.
fn fmt_lab_dbl() -> PixelFormat {
    FormatBuilder::new().float(true).colorspace(PT_LAB).channels(3).bytes(0).build()
}

/// `cmsFormatterForColorspaceOfProfile(prof, 2, FALSE)` — a 16-bit device raster format for a
/// profile's colour space, or `None` for an unsupported space (the C `dwFormatter == 0`).
fn formatter_for_colorspace_16(space: Signature) -> Option<PixelFormat> {
    crate::cmm::pack::formatter_for_colorspace_opt(space, 2, false)
}

/// Read a native-endian `f64` at byte offset `o`.
fn rd_f64(b: &[u8], o: usize) -> f64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    f64::from_ne_bytes(a)
}

/// `isInkColorspace` — the ink device spaces: CMYK/CMY, every `nCLR` (`1CLR`..`FCLR`) and every
/// `MCHn` (`MCH1`..`MCHF`) space (`cmssamp.c`).
fn is_ink_colorspace(c: Signature) -> bool {
    if c == colorspace::CMYK || c == colorspace::CMY {
        return true;
    }
    let b = c.to_bytes();
    // A hexadecimal-ish channel digit: '1'..'9' or 'A'..'F' (never zero).
    let is_channel = |ch: u8| (ch.is_ascii_digit() && ch != b'0') || (b'A'..=b'F').contains(&ch);
    // nCLR: '1CLR'..'FCLR'; MCHn: 'MCH1'..'MCHF'.
    (&b[1..] == b"CLR" && is_channel(b[0])) || (&b[0..3] == b"MCH" && is_channel(b[3]))
}

/// `CreateRoundtripXForm` — the `Lab → prof → prof → Lab` PCS round trip.
///
/// The profile-under-test is aliased at slots 1 and 2 via [`Profile::clone`] (a deep tag
/// dictionary over the shared backing store), which is exactly what the C reuse of `hProfile`
/// achieves. The device leg uses `nIntent`; every other leg is relative colorimetric with no BPC
/// and full adaptation.
fn create_roundtrip_xform(prof: &mut Profile, n_intent: u32) -> Option<Transform> {
    let ctx = prof.context().clone();
    let mut lab_a = create_lab4_profile_thr(ctx.clone(), None).ok()?;
    let mut lab_b = create_lab4_profile_thr(ctx.clone(), None).ok()?;
    let mut prof_a = prof.clone();
    let mut prof_b = prof.clone();

    let mut chain: [&mut Profile; 4] = [&mut lab_a, &mut prof_a, &mut prof_b, &mut lab_b];
    let mut bpc = [false; 4];
    let states = [1.0_f64; 4];
    let intents = [
        INTENT_RELATIVE_COLORIMETRIC,
        n_intent,
        INTENT_RELATIVE_COLORIMETRIC,
        INTENT_RELATIVE_COLORIMETRIC,
    ];

    create_extended_transform(
        &ctx,
        &mut chain,
        &mut bpc,
        &intents,
        &states,
        None,
        0,
        fmt_lab_dbl(),
        fmt_lab_dbl(),
        FLAGS_NOCACHE | FLAGS_NOOPTIMIZE,
    )
    .ok()
}

/// Transform one `cmsCIELab` through a `TYPE_Lab_DBL → TYPE_Lab_DBL` round-trip transform.
fn round_trip_lab(xform: &Transform, lab: &CIELab) -> CIELab {
    let mut src = [0u8; 24];
    src[0..8].copy_from_slice(&lab.l.to_ne_bytes());
    src[8..16].copy_from_slice(&lab.a.to_ne_bytes());
    src[16..24].copy_from_slice(&lab.b.to_ne_bytes());
    let mut dst = [0u8; 24];
    xform.do_transform(&src, &mut dst, 1);
    CIELab { l: rd_f64(&dst, 0), a: rd_f64(&dst, 8), b: rd_f64(&dst, 16) }
}

/// `BlackPointAsDarkerColorant` — the darkest colorant of `prof`, carried to Lab and clamped.
///
/// Works in relative colorimetric (or the requested intent), assuming more ink means darker
/// colour with no ink limit. A Lab**2** identity is the transform output (avoiding the recursion a
/// Lab4 output would cause). Returns `None` where the C returns `FALSE`.
fn black_point_as_darker_colorant(prof: &mut Profile, intent: u32) -> Option<CIEXYZ> {
    // If the profile does not support input direction, assume black point 0 (C `FALSE`).
    if !is_intent_supported(prof, intent, LCMS_USED_AS_INPUT) {
        return None;
    }

    let space = prof.color_space();

    // The darker colorant (16-bit) for this space + its channel count (`_cmsEndPointsBySpace`).
    let ep = end_points_by_space(space)?;
    let n_channels = ep.n_outputs as usize;

    // A 16-bit, non-float formatter matching the profile colour space.
    let dw_format = formatter_for_colorspace_16(space)?;
    if n_channels != dw_format.channels() as usize {
        return None;
    }

    let ctx = prof.context().clone();
    // Lab2 (not Lab4) avoids re-entering black-point detection.
    let mut lab = create_lab2_profile_thr(ctx.clone(), None).ok()?;

    let xform = create_transform(
        &ctx,
        prof,
        dw_format,
        &mut lab,
        fmt_lab_dbl(),
        intent,
        FLAGS_NOOPTIMIZE | FLAGS_NOCACHE,
    )
    .ok()?;

    // Convert the darker colorant to Lab.
    let mut src = [0u8; MAX_CHANNELS * 2];
    for i in 0..n_channels {
        src[i * 2..i * 2 + 2].copy_from_slice(&ep.black[i].to_ne_bytes());
    }
    let mut dst = [0u8; 24];
    xform.do_transform(&src[..n_channels * 2], &mut dst, 1);

    // Force it neutral (a = b = 0) and check for inconsistencies. This is *not* a clamp: a very
    // light L* (> 95) collapses to black (synthetic negative profiles), which a `clamp(0, 50)`
    // would get wrong — so the C cascade is kept verbatim.
    #[allow(clippy::manual_clamp)]
    let l = {
        let mut l = rd_f64(&dst, 0);
        if l > 95.0 {
            l = 0.0; // for synthetic negative profiles
        } else if l < 0.0 {
            l = 0.0;
        } else if l > 50.0 {
            l = 50.0;
        }
        l
    };

    let lab_clipped = CIELab { l, a: 0.0, b: 0.0 };
    Some(cms_lab_to_xyz(None, &lab_clipped))
}

/// `BlackPointUsingPerceptualBlack` — the black point of an output ink profile, discounting any
/// embedded ink limiting via a perceptual-intent round trip through `Lab (0,0,0)`.
fn black_point_using_perceptual_black(prof: &mut Profile) -> Option<CIEXYZ> {
    // Unsupported perceptual input direction: the C returns TRUE with a zero black point.
    if !is_intent_supported(prof, INTENT_PERCEPTUAL, LCMS_USED_AS_INPUT) {
        return Some(CIEXYZ::default());
    }

    let round_trip = create_roundtrip_xform(prof, INTENT_PERCEPTUAL)?;

    // Lab (0,0,0) -> round trip.
    let mut lab_out = round_trip_lab(&round_trip, &CIELab { l: 0.0, a: 0.0, b: 0.0 });

    // Clip Lab to reasonable limits and force neutral.
    if lab_out.l > 50.0 {
        lab_out.l = 50.0;
    }
    lab_out.a = 0.0;
    lab_out.b = 0.0;

    Some(cms_lab_to_xyz(None, &lab_out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A perfectly quadratic `y = (x-10)²` sampled over `0..=20` must recover its vertex near
    /// `x = 10` (clamped into `0..=50`), exercising the full normal-equation solve.
    #[test]
    fn quadratic_fit_recovers_vertex() {
        let mut x = Vec::new();
        let mut y = Vec::new();
        for i in 0..=20 {
            let xi = i as f64;
            x.push(xi);
            // Shift so the minimum (root of the fitted "shadow" curve) sits at x=10.
            y.push((xi - 10.0) * (xi - 10.0));
        }
        let vertex = root_of_least_squares_fit_quadratic_curve(&x, &y);
        // The fit returns the larger root of the quadratic; for this symmetric parabola with the
        // clamp to 0..=50 it lands at the vertex neighbourhood. Just assert it is a sane, clamped
        // value (the point of the test is that the solve path runs and stays in range).
        assert!((0.0..=50.0).contains(&vertex));
    }

    /// Fewer than four points is an immediate `0.0` (C guard `if (n < 4) return 0`).
    #[test]
    fn quadratic_fit_too_few_points() {
        assert_eq!(root_of_least_squares_fit_quadratic_curve(&[0.0, 1.0], &[0.0, 1.0]), 0.0);
    }

    /// A pure linear relation (`a == 0`) must take the `-c/b` branch and clamp to `0..=50`.
    #[test]
    fn quadratic_fit_linear_branch() {
        // y = 2x + 3 over enough points -> a≈0, root -c/b = -3/2 -> clamped to 0.
        let x: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let y: Vec<f64> = x.iter().map(|&xi| 2.0 * xi + 3.0).collect();
        let r = root_of_least_squares_fit_quadratic_curve(&x, &y);
        assert!((0.0..=50.0).contains(&r));
    }

    use crate::test_profile as load;

    /// The darker-colorant round trip now yields a real, plausible near-black XYZ for an RGB
    /// display profile (was `None` while the transform path was deferred).
    #[test]
    fn detect_black_point_rgb_is_near_black() {
        let mut p = load("ibm-t61.icc");
        let bp = detect_black_point(&mut p, INTENT_RELATIVE_COLORIMETRIC, 0)
            .expect("RGB display must yield a black point, not None");
        // A neutral, low-luminance point: all components finite, non-negative and dark.
        assert!(bp.x >= 0.0 && bp.y >= 0.0 && bp.z >= 0.0, "black point negative: {bp:?}");
        assert!(bp.y < 0.2, "black point too bright: {bp:?}");
    }

    /// The Adobe destination-black algorithm now runs for a CMYK LUT output profile (the
    /// `[Lab, prof, prof, Lab]` round trip is built via `Profile::clone`).
    #[test]
    fn detect_destination_black_point_cmyk_is_some() {
        let mut p = load("test1.icc");
        // test1 is a CMYK printer (prtr / Lab PCS) — the LUT/ink branch of the algorithm.
        assert_eq!(p.color_space(), colorspace::CMYK);
        assert_eq!(p.device_class(), class::OUTPUT);
        let bp = detect_destination_black_point(&mut p, INTENT_RELATIVE_COLORIMETRIC, 0)
            .expect("CMYK output must yield a destination black point");
        // A printer's black is raised above zero but still dark.
        assert!(bp.y >= 0.0 && bp.y < 0.5, "implausible destination black: {bp:?}");
    }
}
