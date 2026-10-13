//! White point, correlated colour temperature, and chromatic adaptation — a direct
//! port of `cmswtpnt.c`.
//!
//! This module covers the illuminant-side colour science: deriving a white point from a
//! correlated colour temperature (CCT) and back (Robertson's method), building Bradford
//! chromatic-adaptation matrices, and the RGB→XYZ transfer matrix used when assembling a
//! profile from primaries + white point.
//!
//! Bit-exactness note: every formula, constant, and floating-point operation order is
//! transcribed verbatim from the C so results match bit for bit. In particular the
//! `-xs + 6*ys + 1.5` denominator is recomputed (not factored) exactly as C does, and the
//! `cmsXYZ2xyY` / `cmsxyY2XYZ` conversions are inlined here (the `pcs` module is not yet
//! available) with the same `1./sum` reciprocal and division layout as `cmspcs.c`.

use crate::color::d50_xyz;
use crate::error::{ErrorCode, Result};
use crate::math::mat3::{MATRIX_DET_TOLERANCE, Mat3, Vec3};
use crate::types::color::{CIEXYZ, CIExyY, CIExyYTriple};

/// `cmsxyY2XYZ` (from `cmspcs.c`) — inlined here until the `pcs` module lands.
///
/// Preserves the C operation layout exactly: `X = (x/y)*Y`, `Z = ((1-x-y)/y)*Y`.
#[inline]
fn xyy_to_xyz(s: &CIExyY) -> CIEXYZ {
    CIEXYZ {
        x: (s.x / s.y) * s.yy,
        y: s.yy,
        z: ((1.0 - s.x - s.y) / s.y) * s.yy,
    }
}

/// `cmsXYZ2xyY` (from `cmspcs.c`) — inlined here until the `pcs` module lands.
///
/// Uses the reciprocal-then-multiply order (`ISum = 1./sum`) of the C so the low bits
/// match.
#[inline]
fn xyz_to_xyy(s: &CIEXYZ) -> CIExyY {
    let isum = 1.0 / (s.x + s.y + s.z);
    CIExyY {
        x: s.x * isum,
        y: s.y * isum,
        yy: s.y,
    }
}

/// The D50 reference white as a chromaticity (`cmsD50_xyY`).
///
/// Derived from [`d50_xyz`] via [`xyz_to_xyy`], matching the C which computes it lazily
/// from `cmsD50_XYZ()` on first use.
pub fn d50_xyy() -> CIExyY {
    xyz_to_xyy(&d50_xyz())
}

/// Obtain a white point (as CIE xyY) from a correlated colour temperature
/// (`cmsWhitePointFromTemp`).
///
/// Uses the standard cubic-polynomial approximation of the Planckian locus, split into
/// two temperature ranges (4000–7000 K and 7000–25000 K), then a `y(x)` parabola. `Y` is
/// fixed at `1.0`. Temperatures outside `[4000, 25000]` K yield
/// [`ErrorCode::Range`] (the C returns `FALSE` after `cmsSignalError(cmsERROR_RANGE)`).
#[allow(clippy::manual_range_contains)] // keep C's explicit comparison form verbatim
pub fn white_point_from_temp(temp_k: f64) -> Result<CIExyY> {
    let t = temp_k;
    let t2 = t * t; // Square
    let t3 = t2 * t; // Cube

    // For correlated color temperature (T) between 4000K and 7000K, or between 7000K and
    // 25000K; otherwise out of range.
    let x = if t >= 4000. && t <= 7000. {
        -4.6070 * (1E9 / t3) + 2.9678 * (1E6 / t2) + 0.09911 * (1E3 / t) + 0.244063
    } else if t > 7000.0 && t <= 25000.0 {
        -2.0064 * (1E9 / t3) + 1.9018 * (1E6 / t2) + 0.24748 * (1E3 / t) + 0.237040
    } else {
        return Err(crate::cms_err!(
            ErrorCode::Range,
            "cmsWhitePointFromTemp: invalid temp"
        ));
    };

    // Obtain y(x)
    let y = -3.000 * (x * x) + 2.870 * x - 0.275;

    Ok(CIExyY { x, y, yy: 1.0 })
}

/// One row of Robertson's isotemperature table (`ISOTEMPERATURE` in `cmswtpnt.c`).
struct IsoTemperature {
    /// temp (in microreciprocal kelvin)
    mirek: f64,
    /// u coord of intersection w/ blackbody locus
    ut: f64,
    /// v coord of intersection w/ blackbody locus
    vt: f64,
    /// slope of ISOTEMPERATURE line
    tt: f64,
}

/// Robertson's isotemperature line table (`isotempdata[]`), transcribed verbatim.
#[rustfmt::skip]
const ISOTEMPDATA: [IsoTemperature; 31] = [
    //  {Mirek, Ut,       Vt,      Tt      }
    IsoTemperature { mirek: 0.0,   ut: 0.18006, vt: 0.26352, tt: -0.24341 },
    IsoTemperature { mirek: 10.0,  ut: 0.18066, vt: 0.26589, tt: -0.25479 },
    IsoTemperature { mirek: 20.0,  ut: 0.18133, vt: 0.26846, tt: -0.26876 },
    IsoTemperature { mirek: 30.0,  ut: 0.18208, vt: 0.27119, tt: -0.28539 },
    IsoTemperature { mirek: 40.0,  ut: 0.18293, vt: 0.27407, tt: -0.30470 },
    IsoTemperature { mirek: 50.0,  ut: 0.18388, vt: 0.27709, tt: -0.32675 },
    IsoTemperature { mirek: 60.0,  ut: 0.18494, vt: 0.28021, tt: -0.35156 },
    IsoTemperature { mirek: 70.0,  ut: 0.18611, vt: 0.28342, tt: -0.37915 },
    IsoTemperature { mirek: 80.0,  ut: 0.18740, vt: 0.28668, tt: -0.40955 },
    IsoTemperature { mirek: 90.0,  ut: 0.18880, vt: 0.28997, tt: -0.44278 },
    IsoTemperature { mirek: 100.0, ut: 0.19032, vt: 0.29326, tt: -0.47888 },
    IsoTemperature { mirek: 125.0, ut: 0.19462, vt: 0.30141, tt: -0.58204 },
    IsoTemperature { mirek: 150.0, ut: 0.19962, vt: 0.30921, tt: -0.70471 },
    IsoTemperature { mirek: 175.0, ut: 0.20525, vt: 0.31647, tt: -0.84901 },
    IsoTemperature { mirek: 200.0, ut: 0.21142, vt: 0.32312, tt: -1.0182  },
    IsoTemperature { mirek: 225.0, ut: 0.21807, vt: 0.32909, tt: -1.2168  },
    IsoTemperature { mirek: 250.0, ut: 0.22511, vt: 0.33439, tt: -1.4512  },
    IsoTemperature { mirek: 275.0, ut: 0.23247, vt: 0.33904, tt: -1.7298  },
    IsoTemperature { mirek: 300.0, ut: 0.24010, vt: 0.34308, tt: -2.0637  },
    IsoTemperature { mirek: 325.0, ut: 0.24702, vt: 0.34655, tt: -2.4681  },
    IsoTemperature { mirek: 350.0, ut: 0.25591, vt: 0.34951, tt: -2.9641  },
    IsoTemperature { mirek: 375.0, ut: 0.26400, vt: 0.35200, tt: -3.5814  },
    IsoTemperature { mirek: 400.0, ut: 0.27218, vt: 0.35407, tt: -4.3633  },
    IsoTemperature { mirek: 425.0, ut: 0.28039, vt: 0.35577, tt: -5.3762  },
    IsoTemperature { mirek: 450.0, ut: 0.28863, vt: 0.35714, tt: -6.7262  },
    IsoTemperature { mirek: 475.0, ut: 0.29685, vt: 0.35823, tt: -8.5955  },
    IsoTemperature { mirek: 500.0, ut: 0.30505, vt: 0.35907, tt: -11.324  },
    IsoTemperature { mirek: 525.0, ut: 0.31320, vt: 0.35968, tt: -15.628  },
    IsoTemperature { mirek: 550.0, ut: 0.32129, vt: 0.36011, tt: -23.325  },
    IsoTemperature { mirek: 575.0, ut: 0.32931, vt: 0.36038, tt: -40.770  },
    IsoTemperature { mirek: 600.0, ut: 0.33724, vt: 0.36051, tt: -116.45  },
];

/// Correlated colour temperature from a white point (`cmsTempFromWhitePoint`),
/// Robertson's method.
///
/// Converts `(x, y)` to CIE 1960 `(u, v)`, then walks the isotemperature table looking
/// for the sign change in the perpendicular distance `di/dj < 0` that brackets the white
/// point, interpolating the mirek value. Returns [`ErrorCode::Range`] when no bracketing
/// interval is found (the C returns `FALSE`).
pub fn temp_from_white_point(white_point: &CIExyY) -> Result<f64> {
    let mut di: f64 = 0.0;
    let mut mi: f64 = 0.0;
    let xs = white_point.x;
    let ys = white_point.y;

    // convert (x,y) to CIE 1960 (u,v)
    let us = (2.0 * xs) / (-xs + 6.0 * ys + 1.5);
    let vs = (3.0 * ys) / (-xs + 6.0 * ys + 1.5);

    for (j, e) in ISOTEMPDATA.iter().enumerate() {
        let uj = e.ut;
        let vj = e.vt;
        let tj = e.tt;
        let mj = e.mirek;

        let dj = ((vs - vj) - tj * (us - uj)) / (1.0 + tj * tj).sqrt();

        if (j != 0) && (di / dj < 0.0) {
            // Found a match
            return Ok(1000000.0 / (mi + (di / (di - dj)) * (mj - mi)));
        }

        di = dj;
        mi = mj;
    }

    // Not found
    Err(crate::cms_err!(
        ErrorCode::Range,
        "cmsTempFromWhitePoint: temperature not found"
    ))
}

/// Compute a chromatic-adaptation matrix using `Chad` as the cone-response matrix
/// (`ComputeChromaticAdaptation`).
///
/// Builds `M = Chad^-1 · diag(destRGB / srcRGB) · Chad`, where `srcRGB`/`destRGB` are the
/// source/destination white points projected into cone space. Returns `None` if `Chad` is
/// singular or if any source cone response is below [`MATRIX_DET_TOLERANCE`].
fn compute_chromatic_adaptation(
    source_white_point: &CIEXYZ,
    dest_white_point: &CIEXYZ,
    chad: &Mat3,
) -> Option<Mat3> {
    let chad_inv = chad.inverse()?;

    let cone_source_xyz = Vec3::new(
        source_white_point.x,
        source_white_point.y,
        source_white_point.z,
    );
    let cone_dest_xyz = Vec3::new(dest_white_point.x, dest_white_point.y, dest_white_point.z);

    let cone_source_rgb = chad.eval(cone_source_xyz);
    let cone_dest_rgb = chad.eval(cone_dest_xyz);

    if (cone_source_rgb.0[0].abs() < MATRIX_DET_TOLERANCE)
        || (cone_source_rgb.0[1].abs() < MATRIX_DET_TOLERANCE)
        || (cone_source_rgb.0[2].abs() < MATRIX_DET_TOLERANCE)
    {
        return None;
    }

    // Build matrix
    let cone = Mat3::from_rows(
        Vec3::new(cone_dest_rgb.0[0] / cone_source_rgb.0[0], 0.0, 0.0),
        Vec3::new(0.0, cone_dest_rgb.0[1] / cone_source_rgb.0[1], 0.0),
        Vec3::new(0.0, 0.0, cone_dest_rgb.0[2] / cone_source_rgb.0[2]),
    );

    // Normalize
    let tmp = Mat3::mul(&cone, chad);
    Some(Mat3::mul(&chad_inv, &tmp))
}

/// Final chromatic adaptation from illuminant `from_ill` to illuminant `to_ill`
/// (`_cmsAdaptationMatrix`).
///
/// The cone-response matrix may be supplied via `cone_matrix`; when `None`, the Bradford
/// (`LamRigg`) matrix is used. Returns `None` on a singular/degenerate configuration.
pub fn adaptation_matrix(
    cone_matrix: Option<&Mat3>,
    from_ill: &CIEXYZ,
    to_ill: &CIEXYZ,
) -> Option<Mat3> {
    // Bradford matrix
    let lam_rigg = Mat3::from_rows(
        Vec3::new(0.8951, 0.2664, -0.1614),
        Vec3::new(-0.7502, 1.7135, 0.0367),
        Vec3::new(0.0389, -0.0685, 1.0296),
    );

    let cone = cone_matrix.unwrap_or(&lam_rigg);

    compute_chromatic_adaptation(from_ill, to_ill, cone)
}

/// Adaptation matrix to a D50 destination, source white given in xyY
/// (`_cmsAdaptMatrixToD50`).
///
/// Post-multiplies `r` by the Bradford adaptation `source_wp → D50`, i.e. returns
/// `Bradford · r`. Returns `None` if the adaptation matrix cannot be built.
fn adapt_matrix_to_d50(r: &Mat3, source_white_pt: &CIExyY) -> Option<Mat3> {
    let dn = xyy_to_xyz(source_white_pt);

    let bradford = adaptation_matrix(None, &dn, &d50_xyz())?;

    Some(Mat3::mul(&bradford, r))
}

/// Build the RGB→CIE XYZ transfer matrix from a white point and primary chromaticities
/// (`_cmsBuildRGB2XYZtransferMatrix`).
///
/// Algorithm (verbatim from the C): build the absolute primaries matrix in XYZ, invert it,
/// evaluate the source white point across the inverse to obtain per-primary coefficients,
/// scale the primaries by those coefficients, then adapt the result to D50. Returns `None`
/// if the primaries matrix is singular or D50 adaptation fails.
pub fn build_rgb2xyz_transfer_matrix(
    white_pt: &CIExyY,
    primaries: &CIExyYTriple,
) -> Option<Mat3> {
    let xn = white_pt.x;
    let yn = white_pt.y;
    let xr = primaries.red.x;
    let yr = primaries.red.y;
    let xg = primaries.green.x;
    let yg = primaries.green.y;
    let xb = primaries.blue.x;
    let yb = primaries.blue.y;

    // Build Primaries matrix
    let primaries_mat = Mat3::from_rows(
        Vec3::new(xr, xg, xb),
        Vec3::new(yr, yg, yb),
        Vec3::new(1.0 - xr - yr, 1.0 - xg - yg, 1.0 - xb - yb),
    );

    // Result = Primaries ^ (-1) inverse matrix
    let result = primaries_mat.inverse()?;

    let white_point = Vec3::new(xn / yn, 1.0, (1.0 - xn - yn) / yn);

    // Across inverse primaries ...
    let coef = result.eval(white_point);

    // Give us the Coefs, then build transformation matrix (VX=0, VY=1, VZ=2)
    let r = Mat3::from_rows(
        Vec3::new(coef.0[0] * xr, coef.0[1] * xg, coef.0[2] * xb),
        Vec3::new(coef.0[0] * yr, coef.0[1] * yg, coef.0[2] * yb),
        Vec3::new(
            coef.0[0] * (1.0 - xr - yr),
            coef.0[1] * (1.0 - xg - yg),
            coef.0[2] * (1.0 - xb - yb),
        ),
    );

    adapt_matrix_to_d50(&r, white_pt)
}

/// Adapt a single XYZ colour to a given illuminant (`cmsAdaptToIlluminant`).
///
/// The input colour is assumed to have `source_white_pt` as its white point; the Bradford
/// adaptation `source_white_pt → illuminant` is applied to `value` and written to
/// `result`. Returns `false` (matching the C `FALSE`) if the adaptation matrix cannot be
/// built.
pub fn adapt_to_illuminant(
    result: &mut CIEXYZ,
    source_white_pt: &CIEXYZ,
    illuminant: &CIEXYZ,
    value: &CIEXYZ,
) -> bool {
    let bradford = match adaptation_matrix(None, source_white_pt, illuminant) {
        Some(m) => m,
        None => return false,
    };

    let in_v = Vec3::new(value.x, value.y, value.z);
    let out = bradford.eval(in_v);

    result.x = out.0[0];
    result.y = out.0[1];
    result.z = out.0[2];

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{D50X, D50Y, D50Z};

    /// D50 chromaticity matches the inline XYZ→xyY conversion of the D50 white.
    #[test]
    fn d50_xyy_matches_conversion() {
        let d = d50_xyy();
        // x = X/(X+Y+Z), y = Y/(X+Y+Z), Y = Y  (D50X=0.9642, D50Y=1.0, D50Z=0.8249)
        let sum = 0.9642 + 1.0 + 0.8249;
        assert!((d.x - 0.9642 / sum).abs() < 1e-12);
        assert!((d.y - 1.0 / sum).abs() < 1e-12);
        assert!((d.yy - 1.0).abs() < 1e-12);
    }

    /// D50's correlated colour temperature is ~5000 K (Robertson yields ≈5000.7 K for
    /// the lcms D50 white; its nominal defining CCT is ≈5003 K).
    #[test]
    fn d50_temperature_is_5000k() {
        let temp = temp_from_white_point(&d50_xyy()).expect("D50 is in-range");
        assert!((temp - 5000.0).abs() < 10.0, "D50 CCT = {temp}, expected ~5000K");
    }

    /// White-point ↔ temperature round-trips across the valid range with max error
    /// under 100 K — a direct port of the C testbed's `CheckTemp2CHRM`. (The daylight
    /// locus of `white_point_from_temp` and the Planckian locus of Robertson's method
    /// differ, so the recovered CCT is only ~100 K-resolution close, not exact.)
    #[test]
    fn temp_roundtrip_matches_c_testbed() {
        let mut max = 0.0_f64;
        for j in 4000..25000 {
            let t = j as f64;
            let wp = white_point_from_temp(t).expect("in range");
            let v = temp_from_white_point(&wp).expect("recoverable");
            let d = (v - t).abs();
            if d > max {
                max = d;
            }
        }
        // 100 degree is the actual resolution (comment from the C testbed).
        assert!(max < 100.0, "max CCT round-trip error {max} >= 100K");
    }

    /// Out-of-range temperatures are rejected.
    #[test]
    fn temp_out_of_range_errors() {
        assert!(white_point_from_temp(3000.0).is_err());
        assert!(white_point_from_temp(30000.0).is_err());
        assert_eq!(
            white_point_from_temp(3000.0).unwrap_err().code,
            ErrorCode::Range
        );
    }

    /// Adapting D50 to D50 is the identity transform.
    #[test]
    fn adapt_d50_to_d50_is_identity() {
        let m = adaptation_matrix(None, &d50_xyz(), &d50_xyz()).expect("non-singular");
        assert!(m.is_identity(), "D50->D50 adaptation should be identity, got {m:?}");
    }

    /// Adapting a value from D50 to D50 leaves it unchanged.
    #[test]
    fn adapt_to_illuminant_d50_identity() {
        let value = CIEXYZ { x: 0.5, y: 0.4, z: 0.3 };
        let mut result = CIEXYZ::default();
        let ok = adapt_to_illuminant(&mut result, &d50_xyz(), &d50_xyz(), &value);
        assert!(ok);
        assert!((result.x - value.x).abs() < 1e-12);
        assert!((result.y - value.y).abs() < 1e-12);
        assert!((result.z - value.z).abs() < 1e-12);
    }

    /// The RGB→XYZ transfer matrix maps neutral RGB (1,1,1) to the D50 white,
    /// because the coefficients reproduce the white point and the result is D50-adapted.
    #[test]
    fn rgb2xyz_maps_white_to_d50() {
        // sRGB primaries + D65 white point.
        let primaries = CIExyYTriple {
            red: CIExyY { x: 0.64, y: 0.33, yy: 1.0 },
            green: CIExyY { x: 0.30, y: 0.60, yy: 1.0 },
            blue: CIExyY { x: 0.15, y: 0.06, yy: 1.0 },
        };
        let d65 = CIExyY { x: 0.3127, y: 0.3290, yy: 1.0 };

        let m = build_rgb2xyz_transfer_matrix(&d65, &primaries).expect("non-singular");
        let white = m.eval(Vec3::new(1.0, 1.0, 1.0));

        assert!((white.0[0] - D50X).abs() < 1e-9, "X = {}", white.0[0]);
        assert!((white.0[1] - D50Y).abs() < 1e-9, "Y = {}", white.0[1]);
        assert!((white.0[2] - D50Z).abs() < 1e-9, "Z = {}", white.0[2]);
    }
}
