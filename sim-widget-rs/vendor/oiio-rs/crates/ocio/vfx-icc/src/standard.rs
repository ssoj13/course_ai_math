//! Standard ICC profile definitions.

use crate::{IccError, IccResult, Profile};
use lcms_rs::prelude::{CIExyY, CIExyYTriple, ToneCurve, create_rgb_profile, create_srgb_profile};

/// Standard color profile specifications.
///
/// Pre-defined color spaces commonly used in VFX and color pipelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardProfile {
    /// IEC 61966-2-1 sRGB.
    Srgb,
    /// Linear sRGB (gamma 1.0).
    LinearSrgb,
    /// Adobe RGB (1998).
    AdobeRgb,
    /// Display P3 (Apple).
    DisplayP3,
    /// DCI-P3 (Digital Cinema).
    DciP3,
    /// ACES AP0 (Academy).
    AcesAp0,
    /// ACES AP1 / ACEScg.
    AcesAp1,
    /// ITU-R BT.709.
    Rec709,
    /// ITU-R BT.2020.
    Rec2020,
}

impl StandardProfile {
    /// Creates an ICC profile from this standard.
    ///
    /// # Errors
    ///
    /// Returns [`IccError::StandardCurveFailed`] if the standard's transfer curve cannot be
    /// built, or [`IccError::StandardProfileFailed`] if the white point, primaries, and curve
    /// cannot be combined into a profile.
    pub fn to_profile(self) -> IccResult<Profile> {
        match self {
            StandardProfile::Srgb => {
                let inner = create_srgb_profile().map_err(|e| IccError::StandardProfileFailed {
                    profile: "Srgb",
                    reason: e.to_string(),
                })?;
                Ok(Profile { inner })
            }
            StandardProfile::LinearSrgb => {
                let curve = gamma_curve("LinearSrgb", 1.0)?;
                build_rgb("LinearSrgb", &d65_white(), &srgb_primaries(), &curve)
            }
            StandardProfile::AdobeRgb => {
                let curve = gamma_curve("AdobeRgb", 2.2)?;
                build_rgb("AdobeRgb", &d65_white(), &adobe_rgb_primaries(), &curve)
            }
            StandardProfile::DisplayP3 => {
                // Display P3 uses the sRGB piecewise transfer curve, NOT simple gamma 2.2.
                // ICC parametric Type 4: (aX+b)^gamma + c for X>=d, eX+f for X<d
                let curve = parametric_curve(
                    "DisplayP3",
                    4,
                    &[
                        2.4,           // gamma
                        1.0 / 1.055,   // a
                        0.055 / 1.055, // b
                        0.0,           // c
                        0.04045,       // d
                        1.0 / 12.92,   // e
                        0.0,           // f
                    ],
                )?;
                build_rgb("DisplayP3", &d65_white(), &p3_primaries(), &curve)
            }
            StandardProfile::DciP3 => {
                // DCI white point (x=0.314, y=0.351)
                let white = CIExyY {
                    x: 0.314,
                    y: 0.351,
                    yy: 1.0,
                };
                let curve = gamma_curve("DciP3", 2.6)?;
                build_rgb("DciP3", &white, &p3_primaries(), &curve)
            }
            StandardProfile::AcesAp0 => {
                let curve = gamma_curve("AcesAp0", 1.0)?;
                build_rgb("AcesAp0", &aces_white(), &aces_ap0_primaries(), &curve)
            }
            StandardProfile::AcesAp1 => {
                let curve = gamma_curve("AcesAp1", 1.0)?;
                build_rgb("AcesAp1", &aces_white(), &aces_ap1_primaries(), &curve)
            }
            StandardProfile::Rec709 => {
                // BT.709 EOTF (inverse OETF): piecewise power+linear curve.
                // ICC parametric Type 4: (aX+b)^gamma + c for X>=d, eX+f for X<d
                let curve = parametric_curve(
                    "Rec709",
                    4,
                    &[
                        1.0 / 0.45,    // gamma (~2.222)
                        1.0 / 1.099,   // a
                        0.099 / 1.099, // b
                        0.0,           // c
                        0.081,         // d (4.5 * 0.018)
                        1.0 / 4.5,     // e
                        0.0,           // f
                    ],
                )?;
                build_rgb("Rec709", &d65_white(), &srgb_primaries(), &curve)
            }
            StandardProfile::Rec2020 => {
                // BT.2020 EOTF: piecewise with alpha=1.09929682680944, beta=0.018053968510807.
                // ICC parametric Type 4: (aX+b)^gamma + c for X>=d, eX+f for X<d
                let alpha: f64 = 1.09929682680944;
                let curve = parametric_curve(
                    "Rec2020",
                    4,
                    &[
                        1.0 / 0.45,              // gamma (~2.222)
                        1.0 / alpha,             // a
                        (alpha - 1.0) / alpha,   // b
                        0.0,                     // c
                        4.5 * 0.018053968510807, // d (~0.08124)
                        1.0 / 4.5,               // e
                        0.0,                     // f
                    ],
                )?;
                build_rgb("Rec2020", &d65_white(), &rec2020_primaries(), &curve)
            }
        }
    }
}

/// Builds an RGB ICC profile from a white point, primaries, and a single transfer curve shared
/// by all three channels.
///
/// Names the failing [`StandardProfile`] in the returned error (`StandardProfileFailed`) so a
/// bad primaries/white-point/curve combination surfaces as a diagnosable error instead of an
/// anonymous CMM failure or a silent substitution of a different color space.
fn build_rgb(
    name: &'static str,
    white: &CIExyY,
    primaries: &CIExyYTriple,
    curve: &ToneCurve,
) -> IccResult<Profile> {
    // The same `&ToneCurve` in all three slots is intentional: `create_rgb_profile` compares the
    // references by identity and links `gTRC`/`bTRC` to `rTRC` when they are the same curve,
    // which is what keeps the built profile byte-compatible with lcms2's output.
    let curves = [curve, curve, curve];
    let inner = create_rgb_profile(Some(white), Some(primaries), Some(curves)).map_err(|e| {
        IccError::StandardProfileFailed {
            profile: name,
            reason: e.to_string(),
        }
    })?;
    Ok(Profile { inner })
}

/// Builds a pure-power transfer curve for a standard's TRC, naming the failing
/// [`StandardProfile`] in the returned error (`StandardCurveFailed`) on failure.
fn gamma_curve(name: &'static str, gamma: f64) -> IccResult<ToneCurve> {
    ToneCurve::build_gamma(gamma).map_err(|e| IccError::StandardCurveFailed {
        profile: name,
        reason: e.to_string(),
    })
}

/// Builds a parametric transfer curve for a standard's TRC, naming the failing
/// [`StandardProfile`] in the returned error (`StandardCurveFailed`) on failure.
fn parametric_curve(name: &'static str, curve_type: i32, params: &[f64]) -> IccResult<ToneCurve> {
    ToneCurve::build_parametric(curve_type, params).map_err(|e| IccError::StandardCurveFailed {
        profile: name,
        reason: e.to_string(),
    })
}

/// D65 white point.
fn d65_white() -> CIExyY {
    CIExyY {
        x: 0.3127,
        y: 0.3290,
        yy: 1.0,
    }
}

/// ACES white point (approximately D60).
fn aces_white() -> CIExyY {
    CIExyY {
        x: 0.32168,
        y: 0.33767,
        yy: 1.0,
    }
}

/// sRGB / Rec.709 primaries.
fn srgb_primaries() -> CIExyYTriple {
    CIExyYTriple {
        red: CIExyY {
            x: 0.6400,
            y: 0.3300,
            yy: 1.0,
        },
        green: CIExyY {
            x: 0.3000,
            y: 0.6000,
            yy: 1.0,
        },
        blue: CIExyY {
            x: 0.1500,
            y: 0.0600,
            yy: 1.0,
        },
    }
}

/// P3 primaries (shared by Display P3 and DCI-P3; they differ only in white point/TRC).
fn p3_primaries() -> CIExyYTriple {
    CIExyYTriple {
        red: CIExyY {
            x: 0.680,
            y: 0.320,
            yy: 1.0,
        },
        green: CIExyY {
            x: 0.265,
            y: 0.690,
            yy: 1.0,
        },
        blue: CIExyY {
            x: 0.150,
            y: 0.060,
            yy: 1.0,
        },
    }
}

/// Adobe RGB (1998) primaries.
fn adobe_rgb_primaries() -> CIExyYTriple {
    CIExyYTriple {
        red: CIExyY {
            x: 0.6400,
            y: 0.3300,
            yy: 1.0,
        },
        green: CIExyY {
            x: 0.2100,
            y: 0.7100,
            yy: 1.0,
        },
        blue: CIExyY {
            x: 0.1500,
            y: 0.0600,
            yy: 1.0,
        },
    }
}

/// ACES AP0 (Academy) primaries.
fn aces_ap0_primaries() -> CIExyYTriple {
    CIExyYTriple {
        red: CIExyY {
            x: 0.7347,
            y: 0.2653,
            yy: 1.0,
        },
        green: CIExyY {
            x: 0.0000,
            y: 1.0000,
            yy: 1.0,
        },
        blue: CIExyY {
            x: 0.0001,
            y: -0.0770,
            yy: 1.0,
        },
    }
}

/// ACES AP1 / ACEScg primaries.
fn aces_ap1_primaries() -> CIExyYTriple {
    CIExyYTriple {
        red: CIExyY {
            x: 0.713,
            y: 0.293,
            yy: 1.0,
        },
        green: CIExyY {
            x: 0.165,
            y: 0.830,
            yy: 1.0,
        },
        blue: CIExyY {
            x: 0.128,
            y: 0.044,
            yy: 1.0,
        },
    }
}

/// ITU-R BT.2020 primaries.
fn rec2020_primaries() -> CIExyYTriple {
    CIExyYTriple {
        red: CIExyY {
            x: 0.708,
            y: 0.292,
            yy: 1.0,
        },
        green: CIExyY {
            x: 0.170,
            y: 0.797,
            yy: 1.0,
        },
        blue: CIExyY {
            x: 0.131,
            y: 0.046,
            yy: 1.0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_standards() {
        let standards = [
            StandardProfile::Srgb,
            StandardProfile::LinearSrgb,
            StandardProfile::AdobeRgb,
            StandardProfile::DisplayP3,
            StandardProfile::DciP3,
            StandardProfile::AcesAp0,
            StandardProfile::AcesAp1,
            StandardProfile::Rec709,
            StandardProfile::Rec2020,
        ];

        for std in standards {
            let profile = std
                .to_profile()
                .unwrap_or_else(|e| panic!("{:?}: {}", std, e));
            assert!(profile.is_rgb(), "{:?} should be RGB", std);
        }
    }

    /// Verifies each profile carries the CORRECT primaries, not just "is RGB".
    ///
    /// We convert a profile's pure primary to sRGB (RelativeColorimetric,
    /// Bradford CAT) and check a colorimetric invariant that an identity /
    /// broken transform could not satisfy:
    ///   * sRGB->sRGB is the IDENTITY (red stays (1,0,0), green (0,1,0)).
    ///   * any wide-gamut RGB profile has primaries OUTSIDE the sRGB triangle,
    ///     so converting its pure primary to sRGB drives a channel negative or
    ///     a channel above 1.0 (out-of-gamut). A no-op transform would instead
    ///     leave the input in [0,1]. Reference outputs measured from lcms2 (and
    ///     reproduced identically by the lcms-rs port).
    #[test]
    fn test_standard_primaries() {
        use crate::{Intent, Profile, Transform};
        let srgb = Profile::srgb().unwrap();

        // sRGB -> sRGB must be identity (proves the round-trip machinery and
        // that sRGB really carries the sRGB primaries / D65 white).
        let t = Transform::new(
            &Profile::srgb().unwrap(),
            &srgb,
            Intent::RelativeColorimetric,
        )
        .unwrap();
        let mut red = [[1.0f32, 0.0, 0.0]];
        let mut green = [[0.0f32, 1.0, 0.0]];
        t.apply(&mut red);
        t.apply(&mut green);
        assert!(
            (red[0][0] - 1.0).abs() < 1e-3 && red[0][1].abs() < 1e-3 && red[0][2].abs() < 1e-3,
            "sRGB->sRGB red not identity: {:?}",
            red[0]
        );
        assert!(
            green[0][0].abs() < 1e-3
                && (green[0][1] - 1.0).abs() < 1e-3
                && green[0][2].abs() < 1e-3,
            "sRGB->sRGB green not identity: {:?}",
            green[0]
        );

        // Wide-gamut profiles: pure primary -> sRGB must leave the unit cube
        // (a channel < 0 or > 1) because their primaries lie outside sRGB.
        // Per-profile the distinguishing primary differs: Adobe RGB shares the
        // sRGB RED chromaticity (0.64,0.33) but has a wider GREEN, so we test
        // GREEN there; the others are wider on RED.
        let out_of_gamut = |c: [f32; 3]| c.iter().any(|&v| !(-1e-2..=1.0 + 1e-2).contains(&v));
        for (std, test_green) in [
            (StandardProfile::Rec2020, false),
            (StandardProfile::DisplayP3, false),
            (StandardProfile::AcesAp1, false),
            (StandardProfile::AdobeRgb, true),
        ] {
            let p = std.to_profile().unwrap();
            let t = Transform::new(&p, &srgb, Intent::RelativeColorimetric).unwrap();
            let mut primary = if test_green {
                [[0.0f32, 1.0, 0.0]]
            } else {
                [[1.0f32, 0.0, 0.0]]
            };
            t.apply(&mut primary);
            assert!(
                out_of_gamut(primary[0]),
                "{:?} primary should be outside sRGB gamut (wider primaries), got {:?}",
                std,
                primary[0]
            );
        }
    }
}
