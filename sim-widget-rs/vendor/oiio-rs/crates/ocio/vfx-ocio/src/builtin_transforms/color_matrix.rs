//! Port of OCIO's `transforms/builtins/ColorMatrixHelpers.cpp`: chromaticities, white points
//! and the RGB <-> XYZ / RGB -> RGB matrix builders every builtin transform uses.
//!
//! This is the single source of the builtin matrices. The previous `builtin_transforms.rs`
//! carried a hand-typed table of camera/display matrices next to three separate NPM builders
//! (`rgb_to_xyz`, `npm_from_xy`, `gamut_matrix`) and two adaptation helpers; the table drifted
//! from what those builders produce (wrong CAT for ALEXA Wide Gamut, Bradford where OCIO uses
//! none, Lindbloom's rounded sRGB matrix instead of the one derived from the primaries, a P3-DCI
//! matrix with the wrong white), which is where most of the builtin parity failures came from.
//! Everything here runs in `f64`, as OCIO's `MatrixOpData::MatrixArray` does, and is narrowed to
//! `f32` only when the processor compiles the matrix op.
//!
//! Matrices are row-major 4x4 (`[f64; 16]`), the layout of `MatrixTransform::matrix`, applied as
//! `M * [r g b a]^T`; `inner(a, b)` is OCIO's `MatrixArray::inner`, i.e. the product `a * b`.
//!
//! Hosts reach this module as `vfx_ocio::color_matrix`: the chromaticities, [`Adaptation`] and the
//! three checked builders [`conversion_matrix`], [`conversion_matrix_to_xyz_d65`] and
//! [`conversion_matrix_from_xyz_d65`]. The unchecked `build_*` builders stay crate-internal: they
//! only ever see the constants below and panic on a singular matrix, which a host's own
//! chromaticities (e.g. an EXR `chromaticities` attribute) can produce.

use crate::{OcioError, OcioResult};

/// Row-major 4x4 matrix.
pub type M44 = [f64; 16];

/// xy chromaticities of the three primaries and the white point (OCIO `Primaries`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Primaries {
    /// Red primary, CIE xy.
    pub red: [f64; 2],
    /// Green primary, CIE xy.
    pub grn: [f64; 2],
    /// Blue primary, CIE xy.
    pub blu: [f64; 2],
    /// White point, CIE xy.
    pub wht: [f64; 2],
}

impl Primaries {
    /// Ok when these chromaticities give a finite, invertible RGB -> XYZ matrix (the reference's
    /// test: an exactly zero determinant, plus non-finite values, which it lets through). Every
    /// checked builder calls it.
    pub fn check(&self) -> OcioResult<()> {
        try_rgb2xyz(self).map(|_| ()).ok_or_else(|| {
            OcioError::Validation(format!(
                "primaries {self:?} do not define an RGB space (singular or non-finite RGB -> \
                 XYZ matrix)"
            ))
        })
    }
}

const fn prims(red: [f64; 2], grn: [f64; 2], blu: [f64; 2], wht: [f64; 2]) -> Primaries {
    Primaries { red, grn, blu, wht }
}

// ColorMatrixHelpers.cpp:13-112.
/// CIE XYZ as an RGB space (illuminant E white).
pub const CIE_XYZ_ILLUM_E: Primaries =
    prims([1.0, 0.0], [0.0, 1.0], [0.0, 0.0], [1.0 / 3.0, 1.0 / 3.0]);
/// ACES AP0 (ACES2065-1), ACES white.
pub const ACES_AP0: Primaries = prims(
    [0.7347, 0.2653],
    [0.0000, 1.0000],
    [0.0001, -0.0770],
    [0.32168, 0.33767],
);
/// ACES AP1 (ACEScg / ACEScct), ACES white.
pub const ACES_AP1: Primaries = prims(
    [0.713, 0.293],
    [0.165, 0.830],
    [0.128, 0.044],
    [0.32168, 0.33767],
);
/// Rec.709 / sRGB, D65.
pub const REC709: Primaries = prims([0.64, 0.33], [0.30, 0.60], [0.15, 0.06], [0.3127, 0.3290]);
/// Rec.709 primaries, ACES (D60) white.
pub const REC709_D60: Primaries =
    prims([0.64, 0.33], [0.30, 0.60], [0.15, 0.06], [0.32168, 0.33767]);
/// Rec.2020 / Rec.2100, D65.
pub const REC2020: Primaries = prims(
    [0.708, 0.292],
    [0.170, 0.797],
    [0.131, 0.046],
    [0.3127, 0.3290],
);
/// Rec.2020 primaries, ACES (D60) white.
pub const REC2020_D60: Primaries = prims(
    [0.708, 0.292],
    [0.170, 0.797],
    [0.131, 0.046],
    [0.32168, 0.33767],
);
/// P3, DCI white.
pub const P3_DCI: Primaries = prims(
    [0.680, 0.320],
    [0.265, 0.690],
    [0.150, 0.060],
    [0.314, 0.351],
);
/// P3 (Display P3), D65.
pub const P3_D65: Primaries = prims(
    [0.680, 0.320],
    [0.265, 0.690],
    [0.150, 0.060],
    [0.3127, 0.3290],
);
/// P3 primaries, ACES (D60) white.
pub const P3_D60: Primaries = prims(
    [0.680, 0.320],
    [0.265, 0.690],
    [0.150, 0.060],
    [0.32168, 0.33767],
);

// Camera gamuts, from the vendor files.
/// ArriCameras.cpp:18-26.
pub const ARRI_ALEXA_WIDE_GAMUT: Primaries = prims(
    [0.68400, 0.31300],
    [0.22100, 0.84800],
    [0.08610, -0.10200],
    [0.31270, 0.32900],
);
/// ArriCameras.cpp:28-36.
pub const ARRI_WIDE_GAMUT_4: Primaries = prims(
    [0.73470, 0.26530],
    [0.14240, 0.85760],
    [0.09910, -0.03080],
    [0.31270, 0.32900],
);
/// CanonCameras.cpp:26-34.
pub const CANON_CGAMUT: Primaries = prims(
    [0.7400, 0.2700],
    [0.1700, 1.1400],
    [0.0800, -0.1000],
    [0.3127, 0.3290],
);
/// PanasonicCameras.cpp:21-29.
pub const PANASONIC_VGAMUT: Primaries = prims(
    [0.730, 0.280],
    [0.165, 0.840],
    [0.100, -0.030],
    [0.3127, 0.3290],
);
/// RedCameras.cpp:20-28.
pub const RED_WIDE_GAMUT_RGB: Primaries = prims(
    [0.780308, 0.304253],
    [0.121595, 1.493994],
    [0.095612, -0.084589],
    [0.3127, 0.3290],
);
/// SonyCameras.cpp:20-28.
pub const SONY_SGAMUT3: Primaries = prims(
    [0.730, 0.280],
    [0.140, 0.855],
    [0.100, -0.050],
    [0.3127, 0.3290],
);
/// SonyCameras.cpp:30-38.
pub const SONY_SGAMUT3_CINE: Primaries = prims(
    [0.766, 0.275],
    [0.225, 0.800],
    [0.089, -0.087],
    [0.3127, 0.3290],
);

/// White point XYZ values (ColorMatrixHelpers.cpp:114-119).
pub mod whitepoint {
    /// ACES (D60) white, Y = 1.
    pub const D60_XYZ: [f64; 3] = [0.95264607456985, 1.0, 1.00882518435159];
    /// D65 white, Y = 1.
    pub const D65_XYZ: [f64; 3] = [0.95045592705167, 1.0, 1.08905775075988];
    /// DCI white, Y = 1.
    pub const DCI_XYZ: [f64; 3] = [0.89458689458689, 1.0, 0.95441595441595];
}

/// Chromatic adaptation method (OCIO `AdaptationMethod`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adaptation {
    /// No adaptation: the whites of the two spaces are not mapped onto each other.
    None,
    /// Von Kries in the Bradford cone space.
    Bradford,
    /// Von Kries in the CAT02 cone space.
    Cat02,
}

pub const IDENTITY: M44 = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// A 4x4 from a row-major 3x3, with identity alpha.
pub const fn m33(m: [f64; 9]) -> M44 {
    [
        m[0], m[1], m[2], 0.0, //
        m[3], m[4], m[5], 0.0, //
        m[6], m[7], m[8], 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]
}

/// `MatrixArray::inner(B)`: the product `a * b`.
pub fn inner(a: &M44, b: &M44) -> M44 {
    let mut r = [0.0; 16];
    for i in 0..4 {
        for j in 0..4 {
            r[i * 4 + j] = (0..4).map(|k| a[i * 4 + k] * b[k * 4 + j]).sum();
        }
    }
    r
}

/// `MatrixArray::inner(Offsets)`: `m * v` on the RGB part.
pub fn apply(m: &M44, v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| m[i * 4] * v[0] + m[i * 4 + 1] * v[1] + m[i * 4 + 2] * v[2])
}

/// `MatrixArray::inverse`, in f64.
///
/// Every matrix inverted here is a primaries / cone matrix, well conditioned by construction; a
/// singular one would mean corrupted constants in this file, hence the panic.
pub fn inverse(m: &M44) -> M44 {
    crate::processor::op::invert_matrix_offset(m, &[0.0; 4])
        .unwrap_or_else(|| panic!("singular builtin matrix {m:?}"))
        .0
}

/// `rgb2xyz_from_xy` (ColorMatrixHelpers.cpp:184-227): the normalised primary matrix, scaled so
/// that white has `Y = 1`.
pub fn rgb2xyz_from_xy(p: &Primaries) -> M44 {
    try_rgb2xyz(p).unwrap_or_else(|| panic!("builtin primaries {p:?} do not define an RGB space"))
}

/// [`rgb2xyz_from_xy`], or `None` when the primaries matrix or the result is singular or not
/// finite ([`Primaries::check`]). A `Some` result is invertible, so every builder that inverts
/// an `rgb2xyz` of checked primaries cannot hit `inverse`'s panic.
fn try_rgb2xyz(p: &Primaries) -> Option<M44> {
    let mut m = IDENTITY;
    for (i, c) in [p.red, p.grn, p.blu].iter().enumerate() {
        m[i] = c[0];
        m[4 + i] = c[1];
        m[8 + i] = 1.0 - c[0] - c[1];
    }
    let inv = crate::processor::op::invert_matrix_offset(&m, &[0.0; 4])?.0;
    let wht_xyz = [
        p.wht[0] / p.wht[1],
        1.0,
        (1.0 - p.wht[0] - p.wht[1]) / p.wht[1],
    ];
    let mut rgb2xyz = IDENTITY;
    for i in 0..3 {
        let gain =
            wht_xyz[0] * inv[i * 4] + wht_xyz[1] * inv[i * 4 + 1] + wht_xyz[2] * inv[i * 4 + 2];
        for j in 0..3 {
            rgb2xyz[j * 4 + i] = gain * m[j * 4 + i];
        }
    }
    crate::processor::op::invert_matrix_offset(&rgb2xyz, &[0.0; 4])?;
    Some(rgb2xyz)
}

/// `build_vonkries_adapt` (ColorMatrixHelpers.cpp:229-272): `inv(cone) * diag(dst/src) * cone`
/// in the cone space of `method` (Bradford unless CAT02 is asked for, as the reference).
pub fn build_vonkries_adapt(src_xyz: [f64; 3], dst_xyz: [f64; 3], method: Adaptation) -> M44 {
    const CONE_RESP_MAT_BRADFORD: [f64; 9] = [
        0.8951, 0.2664, -0.1614, -0.7502, 1.7135, 0.0367, 0.0389, -0.0685, 1.0296,
    ];
    const CONE_RESP_MAT_CAT02: [f64; 9] = [
        0.7328, 0.4296, -0.1624, -0.7036, 1.6975, 0.0061, 0.0030, 0.0136, 0.9834,
    ];
    let xyz2rgb = m33(if method == Adaptation::Cat02 {
        CONE_RESP_MAT_CAT02
    } else {
        CONE_RESP_MAT_BRADFORD
    });
    let rgb2xyz = inverse(&xyz2rgb);
    let src = apply(&xyz2rgb, src_xyz);
    let dst = apply(&xyz2rgb, dst_xyz);
    let mut scale = IDENTITY;
    for i in 0..3 {
        scale[i * 5] = dst[i] / src[i];
    }
    inner(&rgb2xyz, &inner(&scale, &xyz2rgb))
}

/// `build_conversion_matrix` with explicit white overrides (ColorMatrixHelpers.cpp:276-328).
///
/// `src_wht_xyz` / `dst_wht_xyz` replace the white implied by the primaries when given (OCIO's
/// "non-null offsets"); otherwise the white is `rgb2xyz * [1, 1, 1]`.
pub fn build_conversion_matrix_with_whites(
    src: &Primaries,
    dst: &Primaries,
    src_wht_xyz: Option<[f64; 3]>,
    dst_wht_xyz: Option<[f64; 3]>,
    method: Adaptation,
) -> M44 {
    let src_rgb2xyz = rgb2xyz_from_xy(src);
    let dst_rgb2xyz = rgb2xyz_from_xy(dst);
    let dst_xyz2rgb = inverse(&dst_rgb2xyz);

    // Return the composed matrix if no white point adaptation is needed.
    if src_wht_xyz.is_none() && dst_wht_xyz.is_none() && src.wht == dst.wht {
        return inner(&dst_xyz2rgb, &src_rgb2xyz);
    }
    if method == Adaptation::None {
        return inner(&dst_xyz2rgb, &src_rgb2xyz);
    }
    let dst_wht = dst_wht_xyz.unwrap_or_else(|| apply(&dst_rgb2xyz, [1.0; 3]));
    let src_wht = src_wht_xyz.unwrap_or_else(|| apply(&src_rgb2xyz, [1.0; 3]));
    let vkmat = build_vonkries_adapt(src_wht, dst_wht, method);
    inner(&dst_xyz2rgb, &inner(&vkmat, &src_rgb2xyz))
}

/// `build_conversion_matrix(src, dst, method)` (ColorMatrixHelpers.cpp:330-336).
pub fn build_conversion_matrix(src: &Primaries, dst: &Primaries, method: Adaptation) -> M44 {
    build_conversion_matrix_with_whites(src, dst, None, None, method)
}

/// `build_conversion_matrix_to_XYZ_D65` (ColorMatrixHelpers.cpp:338-343): to CIE XYZ with its
/// conventional (illuminant E) normalisation, adapting the source white to D65.
pub fn build_conversion_matrix_to_xyz_d65(src: &Primaries, method: Adaptation) -> M44 {
    build_conversion_matrix_with_whites(
        src,
        &CIE_XYZ_ILLUM_E,
        None,
        Some(whitepoint::D65_XYZ),
        method,
    )
}

/// `build_conversion_matrix_from_XYZ_D65` (ColorMatrixHelpers.cpp:345-350).
pub fn build_conversion_matrix_from_xyz_d65(dst: &Primaries, method: Adaptation) -> M44 {
    build_conversion_matrix_with_whites(
        &CIE_XYZ_ILLUM_E,
        dst,
        Some(whitepoint::D65_XYZ),
        None,
        method,
    )
}

/// A matrix built from checked primaries: an adaptation between whites whose cone responses
/// divide by zero still yields inf/NaN, which is an error here rather than a matrix.
fn finite(m: M44, what: &str) -> OcioResult<M44> {
    if m.iter().all(|v| v.is_finite()) {
        Ok(m)
    } else {
        Err(OcioError::Validation(format!(
            "{what}: the matrix is not finite"
        )))
    }
}

/// The RGB -> RGB matrix from `src` to `dst` (OCIO `build_conversion_matrix`), row-major 4x4 as
/// a [`MatrixTransform`](crate::MatrixTransform) takes it; an error for primaries that fail
/// [`Primaries::check`].
pub fn conversion_matrix(src: &Primaries, dst: &Primaries, method: Adaptation) -> OcioResult<M44> {
    src.check()?;
    dst.check()?;
    finite(
        build_conversion_matrix(src, dst, method),
        "conversion_matrix",
    )
}

/// RGB `src` -> CIE XYZ, `src`'s white adapted to D65 (OCIO `build_conversion_matrix_to_XYZ_D65`:
/// the display reference of OCIO v2 configs).
pub fn conversion_matrix_to_xyz_d65(src: &Primaries, method: Adaptation) -> OcioResult<M44> {
    src.check()?;
    finite(
        build_conversion_matrix_to_xyz_d65(src, method),
        "conversion_matrix_to_xyz_d65",
    )
}

/// CIE XYZ (D65 white) -> RGB `dst` (OCIO `build_conversion_matrix_from_XYZ_D65`).
pub fn conversion_matrix_from_xyz_d65(dst: &Primaries, method: Adaptation) -> OcioResult<M44> {
    dst.check()?;
    finite(
        build_conversion_matrix_from_xyz_d65(dst, method),
        "conversion_matrix_from_xyz_d65",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(got: &M44, want: [f64; 9], tol: f64, what: &str) {
        let want = m33(want);
        for i in 0..16 {
            assert!(
                (got[i] - want[i]).abs() <= tol,
                "{what}[{i}]: {} vs {}",
                got[i],
                want[i]
            );
        }
    }

    /// Reference matrices: the matrix ops of the corresponding PyOpenColorIO 2.5.1 builtins
    /// (`Processor.createGroupTransform()`), i.e. OCIO's own `build_conversion_matrix*` output.
    #[test]
    fn matches_ocio_builtin_matrices() {
        assert_close(
            &build_conversion_matrix(&ACES_AP0, &ACES_AP1, Adaptation::None),
            [
                1.4514393161456651,
                -0.23651074689374021,
                -0.2149285692519253, //
                -0.07655377339602057,
                1.176229699833573,
                -0.09967592643755223, //
                0.008316148425697719,
                -0.006032449791021028,
                0.9977163013653234,
            ],
            1e-12,
            "AP0->AP1 (inverse ACEScg_to_ACES2065-1)",
        );
        assert_close(
            &build_conversion_matrix_from_xyz_d65(&REC709, Adaptation::None),
            [
                3.2409699419045213,
                -1.5373831775700932,
                -0.4986107602930035, //
                -0.9692436362808797,
                1.8759675015077204,
                0.04155505740717564, //
                0.05563007969699365,
                -0.20397695888897652,
                1.0569715142428788,
            ],
            1e-12,
            "XYZ-D65->Rec.709 (DISPLAY - CIE-XYZ-D65_to_sRGB)",
        );
        assert_close(
            &build_conversion_matrix_from_xyz_d65(&P3_DCI, Adaptation::Bradford),
            [
                2.690225911625597,
                -1.0940019373661367,
                -0.4250823476747524, //
                -0.8200821842734911,
                1.7504809082920574,
                0.026601954212205743, //
                0.03624575465400463,
                -0.07858083680558861,
                0.9587469936609855,
            ],
            1e-12,
            "XYZ-D65->P3-DCI BFD (DISPLAY - CIE-XYZ-D65_to_G2.6-P3-DCI-BFD)",
        );
        assert_close(
            &build_conversion_matrix_to_xyz_d65(&ACES_AP0, Adaptation::Bradford),
            [
                0.9382798492772554,
                -0.004451445812361399,
                0.016627523586776168, //
                0.3373688907878367,
                0.7295215666902654,
                -0.06689045747810234, //
                0.001173950849685879,
                -0.0037107064020525625,
                1.0915945063122465,
            ],
            1e-12,
            "AP0->XYZ-D65 BFD (UTILITY - ACES-AP0_to_CIE-XYZ-D65_BFD)",
        );
    }

    #[test]
    fn to_and_from_xyz_d65_map_white_to_d65() {
        let to = build_conversion_matrix_to_xyz_d65(&ACES_AP0, Adaptation::Bradford);
        let w = apply(&to, [1.0; 3]);
        for c in 0..3 {
            assert!((w[c] - whitepoint::D65_XYZ[c]).abs() < 1e-9, "{w:?}");
        }
        let from = build_conversion_matrix_from_xyz_d65(&P3_DCI, Adaptation::Bradford);
        let back = apply(&from, whitepoint::D65_XYZ);
        for c in 0..3 {
            assert!((back[c] - 1.0).abs() < 1e-9, "D65 -> P3-DCI white {back:?}");
        }
    }

    /// The checked builders are the internal ones on valid primaries, bit for bit.
    #[test]
    fn checked_builders_equal_the_builtin_ones() {
        let (b, n) = (Adaptation::Bradford, Adaptation::None);
        assert_eq!(
            conversion_matrix(&ACES_AP0, &REC2020, b).unwrap(),
            build_conversion_matrix(&ACES_AP0, &REC2020, b)
        );
        assert_eq!(
            conversion_matrix_to_xyz_d65(&P3_DCI, b).unwrap(),
            build_conversion_matrix_to_xyz_d65(&P3_DCI, b)
        );
        assert_eq!(
            conversion_matrix_from_xyz_d65(&REC709, n).unwrap(),
            build_conversion_matrix_from_xyz_d65(&REC709, n)
        );
    }

    /// Chromaticities a host may read from a file: each degenerate one is an error from every
    /// checked builder, on either side, never the internal builders' panic. The reference
    /// (`MatrixArray::inverse`, MatrixOpData.cpp:216/257) throws only on an exactly zero pivot:
    /// a repeated primary. A white at y = 0 or a NaN it turns into an inf/NaN matrix without a
    /// word; here those are errors too.
    #[test]
    fn degenerate_primaries_are_errors_not_panics() {
        let mut repeated = REC709;
        repeated.grn = REC709.red;
        let mut white_y0 = REC709;
        white_y0.wht = [0.3127, 0.0];
        let mut nan = REC709;
        nan.red = [f64::NAN, 0.33];
        let n = Adaptation::None;
        for (what, p) in [
            ("repeated primary", repeated),
            ("white y = 0", white_y0),
            ("NaN", nan),
        ] {
            assert!(p.check().is_err(), "{what}");
            assert!(conversion_matrix(&p, &REC709, n).is_err(), "{what} as src");
            assert!(conversion_matrix(&REC709, &p, n).is_err(), "{what} as dst");
            assert!(conversion_matrix_to_xyz_d65(&p, n).is_err(), "{what}");
            assert!(conversion_matrix_from_xyz_d65(&p, n).is_err(), "{what}");
        }
        for p in [
            CIE_XYZ_ILLUM_E,
            ACES_AP0,
            ACES_AP1,
            REC709,
            REC2020,
            P3_DCI,
            P3_D65,
            P3_D60,
        ] {
            assert!(p.check().is_ok(), "{p:?}");
        }
    }
}
