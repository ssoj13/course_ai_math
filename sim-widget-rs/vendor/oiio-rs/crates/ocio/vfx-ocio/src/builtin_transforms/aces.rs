//! Port of OCIO's `transforms/builtins/ACES.cpp`: the ACES utilities, input curves (ACEScct,
//! ACEScc, ACESproxy, ADX), LMTs, and the ACES 1.x and 2.0 output transforms.

use super::Registry;
use super::color_matrix::{
    ACES_AP0, ACES_AP1, Adaptation, CIE_XYZ_ILLUM_E, P3_D60, P3_D65, Primaries, REC709, REC709_D60,
    REC2020, REC2020_D60, build_conversion_matrix, build_conversion_matrix_to_xyz_d65,
    build_vonkries_adapt, m33, rgb2xyz_from_xy, whitepoint,
};
use super::op_helpers::{
    LogParams, fixed_function, half_lut, interpolate_1d, log, log_to_linear, lut, master_curve,
    matrix, matrix_dir, range, scale, scale_offset,
};
use crate::transform::{FixedFunctionStyle as FF, Transform, TransformDirection};

const FWD: TransformDirection = TransformDirection::Forward;
const INV: TransformDirection = TransformDirection::Inverse;

/// `AP1_to_CIE_XYZ_D65::GenerateOps` (ACES.cpp:28-38).
fn ap1_to_cie_xyz_d65(ops: &mut Vec<Transform>) {
    ops.push(matrix(build_conversion_matrix_to_xyz_d65(
        &ACES_AP1,
        Adaptation::Bradford,
    )));
}

/// `ACEScct_to_LINEAR` (ACES.cpp:40-55).
fn acescct_to_linear() -> Transform {
    log_to_linear(
        2.0,
        &LogParams {
            log_side_slope: 1.0 / 17.52,
            log_side_offset: 9.72 / 17.52,
            lin_side_slope: 1.0,
            lin_side_offset: 0.0,
            lin_side_break: Some(0.0078125),
            linear_slope: None,
        },
    )
}

/// `ADX_to_ACES::GenerateOps` (ACES.cpp:57-139).
fn adx_to_aces(ops: &mut Vec<Transform>) {
    const NONUNIFORM_LUT: [[f64; 2]; 11] = [
        [-0.190000000000000, -6.000000000000000],
        [0.010000000000000, -2.721_718_645],
        [0.028000000000000, -2.521_718_645],
        [0.054000000000000, -2.321_718_645],
        [0.095000000000000, -2.121_718_645],
        [0.145000000000000, -1.921_718_645],
        [0.220000000000000, -1.721_718_645],
        [0.300000000000000, -1.521_718_645],
        [0.400000000000000, -1.321_718_645],
        [0.500000000000000, -1.121_718_645],
        [0.600000000000000, -0.926545676714876],
    ];
    // Convert Channel Dependent Density values into Channel Independent Density values.
    ops.push(matrix(m33([
        0.75573, 0.22197, 0.02230, //
        0.05901, 0.96928, -0.02829, //
        0.16134, 0.07406, 0.76460,
    ])));
    // Convert Channel Independent Density values to Relative Log Exposure values.
    ops.push(half_lut(|x| {
        let t = &NONUNIFORM_LUT;
        let out = if x < t[0][0] {
            // Extrapolate to ease conversion to LUT1D.
            let slope = (t[1][1] - t[0][1]) / (t[1][0] - t[0][0]);
            (t[0][1] - slope * (t[0][0] - x)).max(-10.0)
        } else if x <= t[10][0] {
            interpolate_1d(t, x)
        } else {
            let ref_pt = (7120.0 - 1520.0) / 8000.0 * (100.0 / 55.0) - 0.18_f64.log10();
            ((100.0 / 55.0) * x - ref_pt).min(4.8162678) // log10(HALF_MAX)
        };
        out as f32
    }));
    // Convert Relative Log Exposure values to Relative Exposure values.
    ops.push(log(10.0, INV));
    // Convert Relative Exposure values to ACES values.
    ops.push(matrix(m33([
        0.72286, 0.12630, 0.15084, //
        0.11923, 0.76418, 0.11659, //
        0.01427, 0.08213, 0.90359,
    ])));
}

// ---------------------------------------------------------------------------
// ACES 1.x output transforms (ACES.cpp:141-498)
// ---------------------------------------------------------------------------

/// `Generate_RRT_preamble_ops` (ACES.cpp:144-172).
fn rrt_preamble(ops: &mut Vec<Transform>) {
    ops.push(fixed_function(FF::AcesGlow10, vec![]));
    ops.push(fixed_function(FF::AcesRedMod10, vec![]));
    ops.push(range(Some(0.0), None, Some(0.0), None));
    ops.push(matrix(build_conversion_matrix(
        &ACES_AP0,
        &ACES_AP1,
        Adaptation::None,
    )));
    ops.push(range(Some(0.0), None, Some(0.0), None));
    ops.push(matrix(m33([
        0.970889148671,
        0.026963270632,
        0.002147580696, //
        0.010889148671,
        0.986963270632,
        0.002147580696, //
        0.010889148671,
        0.026963270632,
        0.962147580696,
    ])));
}

/// The RRT shaper, the first B-spline of `Generate_tonecurve_ops` (ACES.cpp:179-199).
const RRT_POINTS: [[f32; 2]; 7] = [
    [-5.260_177_6, -4.0],
    [-3.755_027_5, -3.578_688_4],
    [-2.249_877_5, -1.821_313_3],
    [-0.744_727_5, 0.681_241_2],
    [1.061_452_5, 2.874_577_5],
    [2.867_632_4, 3.834_062],
    [4.673_812_4, 4.0],
];
const RRT_SLOPES: [f32; 7] = [
    0.0,
    0.55982688,
    1.775_322_4,
    1.55,
    0.8787017,
    0.18374463,
    0.0,
];

/// `Generate_tonecurve_ops` (ACES.cpp:174-253): the SDR tone scale.
fn tonecurve(ops: &mut Vec<Transform>) {
    ops.push(log(10.0, FWD));
    ops.push(master_curve(&RRT_POINTS, &RRT_SLOPES));
    ops.push(master_curve(
        &[
            [-2.540_623_7, -1.698_97],
            [-2.080_357_3, -1.588_435],
            [-1.620_090_8, -1.353_5],
            [-1.159_824_4, -1.046_95],
            [-0.699_558, -0.656_4],
            [-0.23929158, -0.221_41],
            [0.22097483, 0.22814402],
            [0.681_241_2, 0.681_241_2],
            [1.012_846_4, 0.991_421_9],
            [1.344_451_4, 1.258],
            [1.676_056_5, 1.449_95],
            [2.007_661_6, 1.559_1],
            [2.339_266_5, 1.622_6],
            [2.670_871_7, 1.660_654_5],
            [3.002_476_7, 1.681_241_3],
        ],
        &[
            0.0,
            0.4803088,
            0.5405565,
            0.791_498_1,
            0.9055625,
            0.984_603_7,
            0.96884766,
            1.0,
            0.87078346,
            0.73702127,
            0.42068113,
            0.23763206,
            0.14535362,
            0.08416378,
            0.04,
        ],
    ));
    ops.push(log(10.0, INV));
    // Cinema white/black.
    const CINEMA_WHITE: f64 = 48.0;
    const CINEMA_BLACK: f64 = 0.02;
    let s = 1.0 / (CINEMA_WHITE - CINEMA_BLACK);
    ops.push(scale_offset(s, -CINEMA_BLACK * s));
}

/// `Generate_video_adjustment_ops` (ACES.cpp:255-269).
fn video_adjustment(ops: &mut Vec<Transform>) {
    ops.push(fixed_function(FF::AcesDarkToDim10, vec![]));
    ops.push(matrix(m33([
        0.949056010175,
        0.047185723607,
        0.003758266219, //
        0.019056010175,
        0.977185723607,
        0.003758266219, //
        0.019056010175,
        0.047185723607,
        0.933758266219,
    ])));
}

/// `Generate_hdr_tonecurve_ops` (ACES.cpp:271-383): the HDR tone scale for `y_max` nits.
fn hdr_tonecurve(ops: &mut Vec<Transform>, y_max: f64) {
    ops.push(log(10.0, FWD));
    let (points, slopes): ([[f32; 2]; 7], [f32; 5]) = match y_max as u32 {
        1000 => (
            [
                [-5.600_501_5, -4.00000000],
                [-4.095_351_7, -3.578_688_4],
                [-2.590_201_6, -1.821_313_3],
                [-1.085_051_7, 0.681_241_2],
                [0.22347059, 2.226_735],
                [1.531_992_8, 2.879_062_2],
                [2.840_515, 3.00000000],
            ],
            [0.55982688, 1.775_322_4, 1.55, 0.81219728, 0.1848466],
        ),
        2000 => (
            [
                [-5.597_385, -4.00000000],
                [-4.092_235, -3.578_688_4],
                [-2.587_085, -1.821_313_3],
                [-1.081_934_9, 0.681_241_2],
                [0.376_397_2, 2.421_301_4],
                [1.834_729_3, 3.166_092],
                [3.293_061_5, 3.301_03],
            ],
            [0.55982688, 1.775_322_4, 1.55, 0.836_370_1, 0.18505799],
        ),
        4000 => (
            [
                [-5.595_033, -4.00000000],
                [-4.089_883_3, -3.578_688_4],
                [-2.584_733_2, -1.821_313_3],
                [-1.079_583_3, 0.681_241_2],
                [0.528_558_8, 2.616_258_4],
                [2.136_700_9, 3.453_512_7],
                [3.744_842_8, 3.602_06],
            ],
            [0.55982688, 1.775_322_4, 1.55, 0.856_525_2, 0.18474395],
        ),
        108 => (
            [
                [-5.378_525_3, -4.00000000],
                [-3.873_375_2, -3.578_688_4],
                [-2.368_225, -1.821_313_3],
                [-0.86307513, 0.681_241_2],
                [-0.035_577_1, 1.604_644_8],
                [0.791_920_9, 1.960_080_6],
                [1.619_419, 2.033_423_7],
            ],
            [0.55982688, 1.775_322_4, 1.55, 0.68179646, 0.17726487],
        ),
        // The registry below only asks for these four; anything else is a programming error
        // in this file, not a user input.
        other => unreachable!("no ACES 1.x HDR tone curve for {other} nits"),
    };
    let slopes = [
        0.0, slopes[0], slopes[1], slopes[2], slopes[3], slopes[4], 0.0,
    ];
    ops.push(master_curve(&points, &slopes));
    ops.push(log(10.0, INV));
    // Cinema white/black (here: Y_MIN / Y_MAX), ACES.cpp:372-382.
    const Y_MIN: f64 = 0.0001;
    let s = 1.0 / (y_max - Y_MIN);
    ops.push(scale_offset(s, -Y_MIN * s));
}

/// `Generate_sdr_primary_clamp_ops` (ACES.cpp:385-399).
fn sdr_primary_clamp(ops: &mut Vec<Transform>, limit: &Primaries) {
    ops.push(matrix(build_conversion_matrix(
        &ACES_AP1,
        limit,
        Adaptation::Bradford,
    )));
    ops.push(range(Some(0.0), Some(1.0), Some(0.0), Some(1.0)));
    ops.push(matrix(rgb2xyz_from_xy(limit)));
}

/// `Generate_hdr_primary_clamp_ops` (ACES.cpp:401-419).
fn hdr_primary_clamp(ops: &mut Vec<Transform>, limit: &Primaries) {
    ops.push(matrix(build_conversion_matrix(
        &ACES_AP1,
        limit,
        Adaptation::None,
    )));
    ops.push(range(Some(0.0), Some(1.0), Some(0.0), Some(1.0)));
    ops.push(matrix(rgb2xyz_from_xy(limit)));
    ops.push(matrix(build_vonkries_adapt(
        whitepoint::D60_XYZ,
        whitepoint::D65_XYZ,
        Adaptation::Bradford,
    )));
}

/// `Generate_nit_normalization_ops` (ACES.cpp:421-428): the PQ curve expects nits / 100.
fn nit_normalization(ops: &mut Vec<Transform>, nit_level: f64) {
    ops.push(scale(nit_level * 0.01));
}

/// `Generate_roll_white_d60_ops` / `_d65_ops` (ACES.cpp:430-496), for `new_wht` 0.918 / 0.908.
fn roll_white(ops: &mut Vec<Transform>, new_wht: f64) {
    ops.push(half_lut(move |input| {
        let width = 0.5;
        let x0 = -1.0;
        let x1 = x0 + width;
        let y0 = -new_wht;
        let y1 = x1;
        let m1 = x1 - x0;
        let a = y0 - y1 + m1;
        let b = 2.0 * (y1 - y0) - m1;
        let c = y0;
        let t = (-input - x0) / (x1 - x0);
        let out = if t < 0.0 {
            -(t * b + c)
        } else if t > 1.0 {
            input
        } else {
            -((t * a + b) * t + c)
        };
        out as f32
    }));
}

// ---------------------------------------------------------------------------
// ACES 2.0 output transforms (ACES.cpp:500-566)
// ---------------------------------------------------------------------------

/// `ACES2_OUTPUT::Generate_output_transform` (ACES.cpp:502-565).
fn aces2_output(
    ops: &mut Vec<Transform>,
    peak_luminance: f32,
    limiting: &Primaries,
    encoding: &Primaries,
    linear_scale: f32,
    scale_white: bool,
) {
    // Clamp to AP1.
    let to_ap1 = build_conversion_matrix(&ACES_AP0, &ACES_AP1, Adaptation::None);
    ops.push(matrix(to_ap1));
    // float arithmetic, as the reference.
    let upper_bound =
        8.0_f32 * (128.0 + 768.0 * ((peak_luminance / 100.0).ln() / (10000.0_f32 / 100.0).ln()));
    let ub = upper_bound as f64;
    ops.push(range(Some(0.0), Some(ub), Some(0.0), Some(ub)));
    ops.push(matrix_dir(to_ap1, INV));
    // Display rendering.
    ops.push(fixed_function(
        FF::AcesOutputTransform20,
        vec![
            peak_luminance as f64,
            limiting.red[0],
            limiting.red[1],
            limiting.grn[0],
            limiting.grn[1],
            limiting.blu[0],
            limiting.blu[1],
            limiting.wht[0],
            limiting.wht[1],
        ],
    ));
    // Post transform clamp.
    let norm_peak = (peak_luminance / 100.0) as f64;
    ops.push(range(
        Some(0.0),
        Some(norm_peak),
        Some(0.0),
        Some(norm_peak),
    ));
    // White point simulation.
    if scale_white {
        let lim_to_out = build_conversion_matrix(limiting, encoding, Adaptation::None);
        let w = super::color_matrix::apply(&lim_to_out, [1.0; 3]);
        ops.push(scale(1.0 / w[0].max(w[1]).max(w[2])));
    }
    // Linear scale factor.
    if linear_scale != 1.0 {
        ops.push(scale(linear_scale as f64));
    }
    ops.push(matrix(build_conversion_matrix_to_xyz_d65(
        limiting,
        Adaptation::None,
    )));
}

/// `ACES::RegisterAll` (ACES.cpp:576-1437), in registration order.
pub(super) fn register_all(r: &mut Registry) {
    r.add(
        "UTILITY - ACES-AP0_to_CIE-XYZ-D65_BFD",
        "Convert ACES AP0 primaries to CIE XYZ with a D65 white point with Bradford adaptation",
        |ops| {
            // A neutral value of [1,1,1] in AP0 maps to the XYZ value of D65.
            ops.push(matrix(build_conversion_matrix_to_xyz_d65(
                &ACES_AP0,
                Adaptation::Bradford,
            )));
        },
    );
    r.add(
        "UTILITY - ACES-AP1_to_CIE-XYZ-D65_BFD",
        "Convert ACES AP1 primaries to CIE XYZ with a D65 white point with Bradford adaptation",
        ap1_to_cie_xyz_d65,
    );
    r.add(
        "UTILITY - ACES-AP1_to_LINEAR-REC709_BFD",
        "Convert ACES AP1 primaries to linear Rec.709 primaries with Bradford adaptation",
        |ops| {
            ops.push(matrix(build_conversion_matrix(
                &ACES_AP1,
                &REC709,
                Adaptation::Bradford,
            )));
        },
    );
    r.add(
        "CURVE - ACEScct-LOG_to_LINEAR",
        "Apply the log-to-lin curve used in ACEScct",
        |ops| {
            ops.push(acescct_to_linear());
        },
    );
    r.add(
        "ACEScct_to_ACES2065-1",
        "Convert ACEScct to ACES2065-1",
        |ops| {
            ops.push(acescct_to_linear());
            ops.push(matrix(build_conversion_matrix(
                &ACES_AP1,
                &ACES_AP0,
                Adaptation::None,
            )));
        },
    );
    r.add(
        "ACEScc_to_ACES2065-1",
        "Convert ACEScc to ACES2065-1",
        |ops| {
            // The LUT input is [0, 1]; remap it to a wider domain to capture the ACEScc extent.
            const IN_MIN: f64 = -0.36;
            const IN_MAX: f64 = 1.50;
            ops.push(range(Some(IN_MIN), Some(IN_MAX), Some(0.0), Some(1.0)));
            ops.push(lut(4096, |input| {
                let x = input * (IN_MAX - IN_MIN) + IN_MIN;
                let out = if x < (9.72 - 15.0) / 17.52 {
                    (2.0_f64.powf(x * 17.52 - 9.72) - 2.0_f64.powi(-16)) * 2.0
                } else {
                    2.0_f64.powf(x * 17.52 - 9.72)
                };
                // The CTL clamps at HALF_MAX, but a LUT is better without the slope discontinuity.
                out as f32
            }));
            ops.push(matrix(build_conversion_matrix(
                &ACES_AP1,
                &ACES_AP0,
                Adaptation::None,
            )));
            // Helps when the transform is inverted to match the CTL, which clamps incoming ACES.
            ops.push(range(Some(0.0), None, Some(0.0), None));
        },
    );
    r.add(
        "ACEScg_to_ACES2065-1",
        "Convert ACEScg to ACES2065-1",
        |ops| {
            ops.push(matrix(build_conversion_matrix(
                &ACES_AP1,
                &ACES_AP0,
                Adaptation::None,
            )));
        },
    );
    r.add(
        "ACESproxy10i_to_ACES2065-1",
        "Convert ACESproxy 10i to ACES2065-1",
        |ops| {
            ops.push(range(
                Some(64.0 / 1023.0),
                Some(940.0 / 1023.0),
                Some((64.0 - 425.0) / 50.0 - 2.5),
                Some((940.0 - 425.0) / 50.0 - 2.5),
            ));
            ops.push(log(2.0, INV));
            ops.push(matrix(build_conversion_matrix(
                &ACES_AP1,
                &ACES_AP0,
                Adaptation::None,
            )));
        },
    );
    r.add(
        "ADX10_to_ACES2065-1",
        "Convert ADX10 to ACES2065-1",
        |ops| {
            // Convert ADX10 values to Channel Dependent Density values.
            ops.push(scale_offset(1023.0 / 500.0, -95.0 / 500.0));
            adx_to_aces(ops);
        },
    );
    r.add(
        "ADX16_to_ACES2065-1",
        "Convert ADX16 to ACES2065-1",
        |ops| {
            // Convert ADX16 values to Channel Dependent Density values.
            ops.push(scale_offset(65535.0 / 8000.0, -1520.0 / 8000.0));
            adx_to_aces(ops);
        },
    );
    r.add(
        "ACES-LMT - BLUE_LIGHT_ARTIFACT_FIX",
        "LMT for desaturating blue hues to reduce clipping artifacts",
        |ops| {
            ops.push(matrix(m33([
                0.9404372683,
                -0.0183068787,
                0.0778696104, //
                0.0083786969,
                0.8286599939,
                0.1629613092, //
                0.0005471261,
                -0.0008833746,
                1.0003362486,
            ])));
        },
    );
    r.add("ACES-LMT - ACES 1.3 Reference Gamut Compression", "LMT (applied in ACES2065-1) to compress scene-referred values from common cameras into the AP1 gamut", |ops| {
        let m = build_conversion_matrix(&ACES_AP0, &ACES_AP1, Adaptation::None);
        ops.push(matrix(m));
        ops.push(fixed_function(
            FF::AcesGamutComp13,
            vec![1.147, 1.264, 1.312, 0.815, 0.803, 0.880, 1.2],
        ));
        ops.push(matrix_dir(m, INV));
    });

    // ACES 1.x output transforms.
    const OUT: &str = "ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - ";
    r.add_owned(
        format!("{OUT}SDR-CINEMA_1.0"),
        "Component of ACES Output Transforms for SDR cinema",
        |ops| {
            rrt_preamble(ops);
            tonecurve(ops);
            ap1_to_cie_xyz_d65(ops);
        },
    );
    r.add_owned(
        format!("{OUT}SDR-VIDEO_1.0"),
        "Component of ACES Output Transforms for SDR D65 video",
        |ops| {
            rrt_preamble(ops);
            tonecurve(ops);
            video_adjustment(ops);
            ap1_to_cie_xyz_d65(ops);
        },
    );
    r.add_owned(
        format!("{OUT}SDR-CINEMA-REC709lim_1.1"),
        "Component of ACES Output Transforms for SDR cinema",
        |ops| {
            rrt_preamble(ops);
            tonecurve(ops);
            sdr_primary_clamp(ops, &REC709);
        },
    );
    r.add_owned(
        format!("{OUT}SDR-VIDEO-REC709lim_1.1"),
        "Component of ACES Output Transforms for SDR D65 video",
        |ops| {
            rrt_preamble(ops);
            tonecurve(ops);
            video_adjustment(ops);
            sdr_primary_clamp(ops, &REC709);
        },
    );
    r.add_owned(
        format!("{OUT}SDR-VIDEO-P3lim_1.1"),
        "Component of ACES Output Transforms for SDR D65 video",
        |ops| {
            rrt_preamble(ops);
            tonecurve(ops);
            video_adjustment(ops);
            sdr_primary_clamp(ops, &P3_D65);
        },
    );
    r.add_owned(
        format!("{OUT}SDR-CINEMA-D60sim-D65_1.1"),
        "Component of ACES Output Transforms for SDR D65 cinema simulating D60 white",
        |ops| {
            rrt_preamble(ops);
            tonecurve(ops);
            ops.push(range(None, Some(1.0), None, Some(1.0)));
            ops.push(scale(0.964));
            ops.push(matrix(rgb2xyz_from_xy(&ACES_AP1)));
        },
    );
    r.add_owned(
        format!("{OUT}SDR-VIDEO-D60sim-D65_1.0"),
        "Component of ACES Output Transforms for SDR D65 video simulating D60 white",
        |ops| {
            rrt_preamble(ops);
            tonecurve(ops);
            ops.push(range(None, Some(1.0), None, Some(1.0)));
            ops.push(scale(0.955));
            video_adjustment(ops);
            ops.push(matrix(rgb2xyz_from_xy(&ACES_AP1)));
        },
    );
    r.add_owned(
        format!("{OUT}SDR-CINEMA-D60sim-DCI_1.0"),
        "Component of ACES Output Transforms for SDR DCI cinema simulating D60 white",
        |ops| {
            rrt_preamble(ops);
            tonecurve(ops);
            roll_white(ops, 0.918);
            ops.push(range(None, Some(0.918), None, Some(0.918)));
            ops.push(scale(0.96));
            ops.push(matrix(rgb2xyz_from_xy(&ACES_AP1)));
            ops.push(matrix(build_vonkries_adapt(
                whitepoint::DCI_XYZ,
                whitepoint::D65_XYZ,
                Adaptation::Bradford,
            )));
        },
    );
    r.add_owned(
        format!("{OUT}SDR-CINEMA-D65sim-DCI_1.1"),
        "Component of ACES Output Transforms for SDR DCI cinema simulating D65 white",
        |ops| {
            rrt_preamble(ops);
            tonecurve(ops);
            roll_white(ops, 0.908);
            ops.push(range(None, Some(0.908), None, Some(0.908)));
            ops.push(scale(0.9575));
            ap1_to_cie_xyz_d65(ops);
            ops.push(matrix(build_vonkries_adapt(
                whitepoint::DCI_XYZ,
                whitepoint::D65_XYZ,
                Adaptation::Bradford,
            )));
        },
    );
    let hdr: [(&str, &str, f64, Primaries); 7] = [
        (
            "HDR-VIDEO-1000nit-15nit-REC2020lim_1.1",
            "Component of ACES Output Transforms for 1000 nit HDR D65 video",
            1000.0,
            REC2020,
        ),
        (
            "HDR-VIDEO-1000nit-15nit-P3lim_1.1",
            "Component of ACES Output Transforms for 1000 nit HDR D65 video",
            1000.0,
            P3_D65,
        ),
        (
            "HDR-VIDEO-2000nit-15nit-REC2020lim_1.1",
            "Component of ACES Output Transforms for 2000 nit HDR D65 video",
            2000.0,
            REC2020,
        ),
        (
            "HDR-VIDEO-2000nit-15nit-P3lim_1.1",
            "Component of ACES Output Transforms for 2000 nit HDR D65 video",
            2000.0,
            P3_D65,
        ),
        (
            "HDR-VIDEO-4000nit-15nit-REC2020lim_1.1",
            "Component of ACES Output Transforms for 4000 nit HDR D65 video",
            4000.0,
            REC2020,
        ),
        (
            "HDR-VIDEO-4000nit-15nit-P3lim_1.1",
            "Component of ACES Output Transforms for 4000 nit HDR D65 video",
            4000.0,
            P3_D65,
        ),
        (
            "HDR-CINEMA-108nit-7.2nit-P3lim_1.1",
            "Component of ACES Output Transforms for 108 nit HDR D65 cinema",
            108.0,
            P3_D65,
        ),
    ];
    for (name, desc, y_max, limit) in hdr {
        r.add_owned(format!("{OUT}{name}"), desc, move |ops| {
            rrt_preamble(ops);
            hdr_tonecurve(ops, y_max);
            hdr_primary_clamp(ops, &limit);
            nit_normalization(ops, y_max);
        });
    }

    // ACES 2 output transforms (ACES.cpp:1133-1419): name, description, peak luminance,
    // limiting primaries, encoding primaries, linear scale, scale white.
    #[rustfmt::skip]
    let aces2: [(&str, &str, f32, Primaries, Primaries, f32, bool); 31] = [
        // D65
        ("SDR-100nit-REC709_2.0", "Component of ACES 2 Output Transforms for 100 nit SDR Rec709", 100.0, REC709, REC709, 1.0, false),
        ("SDR-100nit-P3-D65_2.0", "Component of ACES 2 Output Transforms for 100 nit SDR P3-D65", 100.0, P3_D65, P3_D65, 1.0, false),
        ("HDR-108nit-P3-D65_2.0", "Component of ACES 2 Output Transforms for 108 nit HDR P3-D65", 225.0, P3_D65, P3_D65, 0.48, false), // = 108 * (100/48)
        ("HDR-300nit-P3-D65_2.0", "Component of ACES 2 Output Transforms for 300 nit HDR P3-D65", 625.0, P3_D65, P3_D65, 0.48, false), // = 300 * (100/48)
        ("HDR-500nit-P3-D65_2.0", "Component of ACES 2 Output Transforms for 500 nit HDR P3-D65", 500.0, P3_D65, P3_D65, 1.0, false),
        ("HDR-1000nit-P3-D65_2.0", "Component of ACES 2 Output Transforms for 1000 nit HDR P3-D65", 1000.0, P3_D65, P3_D65, 1.0, false),
        ("HDR-2000nit-P3-D65_2.0", "Component of ACES 2 Output Transforms for 2000 nit HDR P3-D65", 2000.0, P3_D65, P3_D65, 1.0, false),
        ("HDR-4000nit-P3-D65_2.0", "Component of ACES 2 Output Transforms for 4000 nit HDR P3-D65", 4000.0, P3_D65, P3_D65, 1.0, false),
        ("HDR-500nit-REC2020_2.0", "Component of ACES 2 Output Transforms for 500 nit HDR Rec2020", 500.0, REC2020, REC2020, 1.0, false),
        ("HDR-1000nit-REC2020_2.0", "Component of ACES 2 Output Transforms for 1000 nit HDR Rec2020", 1000.0, REC2020, REC2020, 1.0, false),
        ("HDR-2000nit-REC2020_2.0", "Component of ACES 2 Output Transforms for 2000 nit HDR Rec2020", 2000.0, REC2020, REC2020, 1.0, false),
        ("HDR-4000nit-REC2020_2.0", "Component of ACES 2 Output Transforms for 4000 nit HDR Rec2020", 4000.0, REC2020, REC2020, 1.0, false),
        // D60
        ("SDR-100nit-REC709-D60-in-REC709-D65_2.0", "Component of ACES 2 Output Transforms for 100 nit SDR Rec709 simulating D60 white in Rec709", 100.0, REC709_D60, REC709, 1.0, true),
        ("SDR-100nit-REC709-D60-in-P3-D65_2.0", "Component of ACES 2 Output Transforms for 100 nit SDR Rec709 simulating D60 white in P3-D65", 100.0, REC709_D60, P3_D65, 1.0, true),
        ("SDR-100nit-REC709-D60-in-REC2020-D65_2.0", "Component of ACES 2 Output Transforms for 100 nit SDR Rec709 simulating D60 white in Rec2020", 100.0, REC709_D60, REC2020, 1.0, true),
        ("SDR-100nit-P3-D60-in-P3-D65_2.0", "Component of ACES 2 Output Transforms for 100 nit SDR P3-D60 simulating D60 white in P3-D65", 100.0, P3_D60, P3_D65, 1.0, true),
        ("SDR-100nit-P3-D60-in-XYZ-E_2.0", "Component of ACES 2 Output Transforms for 100 nit SDR P3-D60 simulating D60 white in XYZ-E", 100.0, P3_D60, CIE_XYZ_ILLUM_E, 1.0, false),
        ("HDR-108nit-P3-D60-in-P3-D65_2.0", "Component of ACES 2 Output Transforms for 108 nit HDR P3-D60 simulating D60 white in P3-D65", 225.0, P3_D60, P3_D65, 0.48, true),
        ("HDR-300nit-P3-D60-in-XYZ-E_2.0", "Component of ACES 2 Output Transforms for 300 nit HDR P3-D60 simulating D60 white in XYZ-E", 625.0, P3_D60, CIE_XYZ_ILLUM_E, 0.48, true),
        ("HDR-500nit-P3-D60-in-P3-D65_2.0", "Component of ACES 2 Output Transforms for 500 nit HDR P3-D60 simulating D60 white in P3-D65", 500.0, P3_D60, P3_D65, 1.0, true),
        ("HDR-1000nit-P3-D60-in-P3-D65_2.0", "Component of ACES 2 Output Transforms for 1000 nit HDR P3-D60 simulating D60 white in P3-D65", 1000.0, P3_D60, P3_D65, 1.0, true),
        ("HDR-2000nit-P3-D60-in-P3-D65_2.0", "Component of ACES 2 Output Transforms for 2000 nit HDR P3-D60 simulating D60 white in P3-D65", 2000.0, P3_D60, P3_D65, 1.0, true),
        ("HDR-4000nit-P3-D60-in-P3-D65_2.0", "Component of ACES 2 Output Transforms for 4000 nit HDR P3-D60 simulating D60 white in P3-D65", 4000.0, P3_D60, P3_D65, 1.0, true),
        ("HDR-500nit-P3-D60-in-REC2020-D65_2.0", "Component of ACES 2 Output Transforms for 500 nit HDR P3-D60 simulating D60 white in Rec2020", 500.0, P3_D60, REC2020, 1.0, true),
        ("HDR-1000nit-P3-D60-in-REC2020-D65_2.0", "Component of ACES 2 Output Transforms for 1000 nit HDR P3-D60 simulating D60 white in Rec2020", 1000.0, P3_D60, REC2020, 1.0, true),
        ("HDR-2000nit-P3-D60-in-REC2020-D65_2.0", "Component of ACES 2 Output Transforms for 2000 nit HDR P3-D60 simulating D60 white in Rec2020", 2000.0, P3_D60, REC2020, 1.0, true),
        ("HDR-4000nit-P3-D60-in-REC2020-D65_2.0", "Component of ACES 2 Output Transforms for 4000 nit HDR P3-D60 simulating D60 white in Rec2020", 4000.0, P3_D60, REC2020, 1.0, true),
        ("HDR-500nit-REC2020-D60-in-REC2020-D65_2.0", "Component of ACES 2 Output Transforms for 500 nit HDR Rec2020 simulating D60 white in Rec2020", 500.0, REC2020_D60, REC2020, 1.0, true),
        ("HDR-1000nit-REC2020-D60-in-REC2020-D65_2.0", "Component of ACES 2 Output Transforms for 1000 nit HDR Rec2020 simulating D60 white in Rec2020", 1000.0, REC2020_D60, REC2020, 1.0, true),
        ("HDR-2000nit-REC2020-D60-in-REC2020-D65_2.0", "Component of ACES 2 Output Transforms for 2000 nit HDR Rec2020 simulating D60 white in Rec2020", 2000.0, REC2020_D60, REC2020, 1.0, true),
        ("HDR-4000nit-REC2020-D60-in-REC2020-D65_2.0", "Component of ACES 2 Output Transforms for 4000 nit HDR Rec2020 simulating D60 white in Rec2020", 4000.0, REC2020_D60, REC2020, 1.0, true),
    ];
    for (name, desc, peak, limiting, encoding, linear_scale, scale_white) in aces2 {
        r.add_owned(format!("{OUT}{name}"), desc, move |ops| {
            aces2_output(ops, peak, &limiting, &encoding, linear_scale, scale_white);
        });
    }
}
