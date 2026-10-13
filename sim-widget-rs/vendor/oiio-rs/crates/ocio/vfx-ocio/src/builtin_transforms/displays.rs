//! Port of OCIO's `transforms/builtins/Displays.cpp`: CIE XYZ (D65) to display encodings, and
//! the PQ / HLG curves.
//!
//! OCIO's default build (`OCIO_LUT_SUPPORT`) samples the PQ and HLG curves into half-domain
//! LUTs; its `OCIO_LUT_SUPPORT == 0` branch expresses the same functions as the `LIN_TO_PQ` /
//! `PQ_TO_LIN` / `LIN_TO_GAMMA_LOG` / `GAMMA_LOG_TO_LIN` FixedFunctions. A half LUT is a
//! sampling of exactly that function, so the two agree to its interpolation error (~1e-7
//! relative) over the whole half range, and only the FixedFunction form runs on the GPU — so
//! that is the form used here.
//!
//! Note the linear convention both forms share: **1.0 = 100 cd/m2**, mirrored about 0.

use super::Registry;
use super::color_matrix::{
    Adaptation, P3_D60, P3_D65, P3_DCI, REC709, REC2020, build_conversion_matrix_from_xyz_d65,
};
use super::op_helpers::{GammaRevStyle, fixed_function, gamma, matrix, scale};
use crate::transform::{FixedFunctionStyle as FF, Transform};

/// `ST_2084::GeneratePQToLinearOps` (Displays.cpp:40-54 LUT / 74-77 FixedFunction): nits/100 out.
fn pq_to_linear(ops: &mut Vec<Transform>) {
    ops.push(fixed_function(FF::PqToLin, vec![]));
}

/// `ST_2084::GenerateLinearToPQOps` (Displays.cpp:56-70 LUT / 79-82 FixedFunction): nits/100 in.
fn linear_to_pq(ops: &mut Vec<Transform>) {
    ops.push(fixed_function(FF::LinToPq, vec![]));
}

/// HLG constants (Displays.cpp:91-99).
mod hlg {
    pub const LW: f64 = 1000.0;
    pub const E_MAX: f64 = 3.0;
    pub const A: f64 = 0.17883277;
    pub const B: f64 = (1.0 - 4.0 * A) * E_MAX / 12.0;
    pub const E_SCALE: f64 = 3.0 / E_MAX;
    pub const E_BREAK: f64 = E_MAX / 12.0;

    /// `c = log(12 / E_MAX) * a + c0`, `c0 = 0.5 - a * log(4a)`.
    pub fn c() -> f64 {
        let c0 = 0.5 - A * (4.0 * A).ln();
        (12.0 / E_MAX).ln() * A + c0
    }

    /// The `GAMMA_LOG` parameters (Displays.cpp:145-161).
    pub fn params() -> Vec<f64> {
        vec![
            0.0,                 // mirror point
            E_BREAK,             // break point
            0.5,                 // gamma power
            E_SCALE.sqrt(),      // post-power scale
            0.0,                 // pre-power offset
            std::f64::consts::E, // log base
            A,                   // log-side slope
            c(),                 // log-side offset
            1.0,                 // lin-side slope
            -B,                  // lin-side offset
        ]
    }
}

/// `HLG::GenerateHLGToLinearOps` (Displays.cpp:103-121 LUT / 163-166 FixedFunction).
fn hlg_to_linear(ops: &mut Vec<Transform>) {
    ops.push(fixed_function(FF::GammaLogToLin, hlg::params()));
}

/// `HLG::GenerateLinearToHLGOps` (Displays.cpp:123-141 LUT / 168-171 FixedFunction).
fn linear_to_hlg(ops: &mut Vec<Transform>) {
    ops.push(fixed_function(FF::LinToGammaLog, hlg::params()));
}

/// `CIE_XYZ_D65_to_<display>`: the from-XYZ matrix, then the display gamma.
fn xyz_to_display(
    ops: &mut Vec<Transform>,
    prims: &super::color_matrix::Primaries,
    method: Adaptation,
    style: GammaRevStyle,
    g: f64,
    offset: f64,
) {
    ops.push(matrix(build_conversion_matrix_from_xyz_d65(prims, method)));
    ops.push(gamma(style, g, offset));
}

/// `DISPLAY::RegisterAll` (Displays.cpp:177-556).
pub(super) fn register_all(r: &mut Registry) {
    use GammaRevStyle::{Basic, BasicMirror, Moncurve, MoncurveMirror};
    const NONE: Adaptation = Adaptation::None;
    const BFD: Adaptation = Adaptation::Bradford;

    r.add(
        "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.709",
        "Convert CIE XYZ (D65 white) to Rec.1886/Rec.709, clamp neg. values",
        |ops| {
            xyz_to_display(ops, &REC709, NONE, Basic, 2.4, 0.0);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.709 - MIRROR NEGS",
        "Convert CIE XYZ (D65 white) to Rec.1886/Rec.709, mirror neg. values",
        |ops| {
            xyz_to_display(ops, &REC709, NONE, BasicMirror, 2.4, 0.0);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.2020",
        "Convert CIE XYZ (D65 white) to Rec.1886/Rec.2020, clamp neg. values",
        |ops| {
            xyz_to_display(ops, &REC2020, NONE, Basic, 2.4, 0.0);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.2020 - MIRROR NEGS",
        "Convert CIE XYZ (D65 white) to Rec.1886/Rec.2020, mirror neg. values",
        |ops| {
            xyz_to_display(ops, &REC2020, NONE, BasicMirror, 2.4, 0.0);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_G2.2-REC.709",
        "Convert CIE XYZ (D65 white) to Gamma2.2, Rec.709, clamp neg. values",
        |ops| {
            xyz_to_display(ops, &REC709, NONE, Basic, 2.2, 0.0);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_G2.2-REC.709 - MIRROR NEGS",
        "Convert CIE XYZ (D65 white) to Gamma2.2, Rec.709, mirror neg. values",
        |ops| {
            xyz_to_display(ops, &REC709, NONE, BasicMirror, 2.2, 0.0);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_sRGB",
        "Convert CIE XYZ (D65 white) to sRGB (piecewise EOTF)",
        |ops| {
            xyz_to_display(ops, &REC709, NONE, Moncurve, 2.4, 0.055);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_sRGB - MIRROR NEGS",
        "Convert CIE XYZ (D65 white) to sRGB (piecewise EOTF), mirror neg. values",
        |ops| {
            xyz_to_display(ops, &REC709, NONE, MoncurveMirror, 2.4, 0.055);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-DCI-BFD",
        "Convert CIE XYZ (D65 white) to Gamma 2.6, P3-DCI (DCI white with Bradford adaptation)",
        |ops| {
            xyz_to_display(ops, &P3_DCI, BFD, Basic, 2.6, 0.0);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-D65",
        "Convert CIE XYZ (D65 white) to Gamma 2.6, P3-D65, clamp neg. values",
        |ops| {
            xyz_to_display(ops, &P3_D65, NONE, Basic, 2.6, 0.0);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-D65 - MIRROR NEGS",
        "Convert CIE XYZ (D65 white) to Gamma 2.6, P3-D65, mirror neg. values",
        |ops| {
            xyz_to_display(ops, &P3_D65, NONE, BasicMirror, 2.6, 0.0);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_G2.6-P3-D60-BFD",
        "Convert CIE XYZ (D65 white) to Gamma 2.6, P3-D60 (Bradford adaptation)",
        |ops| {
            xyz_to_display(ops, &P3_D60, BFD, Basic, 2.6, 0.0);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_DCDM-D65",
        "Convert CIE XYZ (D65 white) to Gamma 2.6 (D65 white in XYZ-E encoding)",
        |ops| {
            ops.push(scale(48.0 / 52.37));
            ops.push(gamma(Basic, 2.6, 0.0));
        },
    );
    // Displays.cpp:405-440: for macOS kCGColorSpaceDisplayP3 / ExtendedDisplayP3, which extend
    // the sRGB curve below 0 by reflection, hence MONCURVE_MIRROR_REV. The -HDR name exists only
    // to partition SDR and HDR views under two displays.
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_DisplayP3",
        "Convert CIE XYZ (D65 white) to Apple Display P3, mirror neg. values",
        |ops| {
            xyz_to_display(ops, &P3_D65, NONE, MoncurveMirror, 2.4, 0.055);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_DisplayP3-HDR",
        "Convert CIE XYZ (D65 white) to Apple Display P3 (HDR), mirror neg. values",
        |ops| {
            xyz_to_display(ops, &P3_D65, NONE, MoncurveMirror, 2.4, 0.055);
        },
    );
    r.add(
        "CURVE - ST-2084_to_LINEAR",
        "Convert SMPTE ST-2084 (PQ) full-range to linear nits/100",
        pq_to_linear,
    );
    r.add(
        "CURVE - LINEAR_to_ST-2084",
        "Convert linear nits/100 to SMPTE ST-2084 (PQ) full-range",
        linear_to_pq,
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_REC.2100-PQ",
        "Convert CIE XYZ (D65 white) to Rec.2100-PQ",
        |ops| {
            ops.push(matrix(build_conversion_matrix_from_xyz_d65(&REC2020, NONE)));
            linear_to_pq(ops);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_ST2084-P3-D65",
        "Convert CIE XYZ (D65 white) to ST-2084 (PQ), P3-D65 primaries",
        |ops| {
            ops.push(matrix(build_conversion_matrix_from_xyz_d65(&P3_D65, NONE)));
            linear_to_pq(ops);
        },
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_ST2084-DCDM-D65",
        "Convert CIE XYZ (D65 white) to ST-2084 (PQ) (D65 white in XYZ-E encoding)",
        linear_to_pq,
    );
    r.add(
        "CURVE - HLG-OETF-INVERSE",
        "Apply ITU-R BT.2100 (HLG) OETF inverse, scaled with HLG 0.42 at 18% grey",
        hlg_to_linear,
    );
    r.add(
        "CURVE - HLG-OETF",
        "Apply ITU-R BT.2100 (HLG) OETF, scaled with 18% grey at HLG 0.42",
        linear_to_hlg,
    );
    r.add(
        "DISPLAY - CIE-XYZ-D65_to_REC.2100-HLG-1000nit",
        "Convert CIE XYZ (D65 white) to Rec.2100-HLG, 1000 nit",
        |ops| {
            ops.push(matrix(build_conversion_matrix_from_xyz_d65(&REC2020, NONE)));
            let gamma = 1.2 + 0.42 * (hlg::LW / 1000.0).log10();
            ops.push(scale(100.0));
            ops.push(scale(hlg::E_MAX.powf(gamma) / hlg::LW));
            ops.push(fixed_function(FF::Rec2100Surround, vec![1.0 / gamma]));
            linear_to_hlg(ops);
        },
    );
}
