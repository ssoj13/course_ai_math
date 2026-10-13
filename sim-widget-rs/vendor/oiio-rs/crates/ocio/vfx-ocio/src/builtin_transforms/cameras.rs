//! Ports of OCIO's camera builtins: `AppleCameras.cpp`, `ArriCameras.cpp`, `CanonCameras.cpp`,
//! `PanasonicCameras.cpp`, `RedCameras.cpp`, `SonyCameras.cpp`.

use super::Registry;
use super::color_matrix::{
    ACES_AP0, ARRI_ALEXA_WIDE_GAMUT, ARRI_WIDE_GAMUT_4, Adaptation, CANON_CGAMUT, PANASONIC_VGAMUT,
    REC2020, RED_WIDE_GAMUT_RGB, SONY_SGAMUT3, SONY_SGAMUT3_CINE, build_conversion_matrix, m33,
};
use super::op_helpers::{LogParams, fixed_function, log_to_linear, lut, matrix, range};
use crate::transform::{FixedFunctionStyle as FF, Transform};

/// `APPLE_LOG::GenerateAppleLogToLinearOps` (AppleCameras.cpp:30-86; LUT branch :39-57,
/// FixedFunction branch :58-85).
///
/// OCIO's default build samples this curve into a half-domain LUT; its `OCIO_LUT_SUPPORT == 0`
/// branch expresses the same function as a Range clamp at 0 plus a `GAMMA_LOG_TO_LIN`
/// FixedFunction. The two agree to the half LUT's interpolation error (~1e-7 relative, it is a
/// sampling of this very function), and only the FixedFunction form exists on the GPU, so it is
/// the one used here.
fn apple_log_to_linear(ops: &mut Vec<Transform>) {
    const R_0: f64 = -0.05641088;
    const R_T: f64 = 0.01;
    const C: f64 = 47.28711236;
    const BETA: f64 = 0.00964052;
    const GAMMA: f64 = 0.08550479;
    const DELTA: f64 = 0.69336945;
    ops.push(range(Some(0.0), None, Some(0.0), None)); // don't clamp the high end
    ops.push(fixed_function(
        FF::GammaLogToLin,
        vec![
            R_0,   // mirror point
            R_T,   // break point
            2.0,   // gamma power
            C,     // post-power scale
            -R_0,  // pre-power offset
            2.0,   // log base
            GAMMA, // log-side slope
            DELTA, // log-side offset
            1.0,   // lin-side slope
            BETA,  // lin-side offset
        ],
    ));
}

/// `CANON_CLOG2::GenerateOpsToLinear` (CanonCameras.cpp:39-82): a 4096-entry LUT over [0, 1].
///
/// This one must stay a LUT: unlike the half-domain LUTs, a [0, 1] LUT clamps its input, so it
/// is NOT the same function as the `DOUBLE_LOG_TO_LIN` fallback outside [0, 1] (Canon Log 2 at
/// 1.5 is 59.23 here and in OCIO, ~6986 through the unclamped curve).
fn canon_clog2_to_linear(ops: &mut Vec<Transform>) {
    ops.push(lut(4096, |x| {
        let out = if x < 0.092864125 {
            -(10.0_f64.powf((0.092864125 - x) / 0.24136077) - 1.0) / 87.099375
        } else {
            (10.0_f64.powf((x - 0.092864125) / 0.24136077) - 1.0) / 87.099375
        };
        (out * 0.9) as f32
    }));
}

/// `CANON_CLOG3::GenerateOpsToLinear` (CanonCameras.cpp:89-136): a 4096-entry LUT over [0, 1].
fn canon_clog3_to_linear(ops: &mut Vec<Transform>) {
    ops.push(lut(4096, |x| {
        let out = if x < 0.097465473 {
            -(10.0_f64.powf((0.12783901 - x) / 0.36726845) - 1.0) / 14.98325
        } else if x <= 0.15277891 {
            (x - 0.12512219) / 1.9754798
        } else {
            (10.0_f64.powf((x - 0.12240537) / 0.36726845) - 1.0) / 14.98325
        };
        (out * 0.9) as f32
    }));
}

/// `SONY_SLOG3_to_LINEAR` (SonyCameras.cpp:41-56).
fn sony_slog3_to_linear() -> Transform {
    log_to_linear(
        10.0,
        &LogParams {
            log_side_slope: 261.5 / 1023.0,
            log_side_offset: 420.0 / 1023.0,
            lin_side_slope: 1.0 / (0.18 + 0.01),
            lin_side_offset: 0.01 / (0.18 + 0.01),
            lin_side_break: Some(0.01125000),
            linear_slope: Some(((171.2102946929 - 95.0) / 0.01125000) / 1023.0),
        },
    )
}

pub(super) fn register_all(r: &mut Registry) {
    // CAMERA::APPLE (AppleCameras.cpp:96-122).
    r.add(
        "APPLE_LOG_to_ACES2065-1",
        "Convert Apple Log to ACES2065-1",
        |ops| {
            apple_log_to_linear(ops);
            ops.push(matrix(build_conversion_matrix(
                &REC2020,
                &ACES_AP0,
                Adaptation::Bradford,
            )));
        },
    );
    r.add(
        "CURVE - APPLE_LOG_to_LINEAR",
        "Convert Apple Log to linear",
        apple_log_to_linear,
    );

    // CAMERA::ARRI (ArriCameras.cpp:75-108).
    r.add(
        "ARRI_ALEXA-LOGC-EI800-AWG_to_ACES2065-1",
        "Convert ARRI ALEXA LogC (EI800) ALEXA Wide Gamut to ACES2065-1",
        |ops| {
            // ARRI_ALEXA_LOGC_EI800_to_LINEAR, ArriCameras.cpp:38-51.
            let lin_side_slope = 1.0 / (0.18 * 0.005 * (800.0 / 400.0) / 0.01);
            let lin_side_offset = 0.0522722750;
            ops.push(log_to_linear(
                10.0,
                &LogParams {
                    log_side_slope: 0.2471896383,
                    log_side_offset: 0.3855369987,
                    lin_side_slope,
                    lin_side_offset,
                    lin_side_break: Some(((1.0 / 9.0) - lin_side_offset) / lin_side_slope),
                    linear_slope: None,
                },
            ));
            ops.push(matrix(build_conversion_matrix(
                &ARRI_ALEXA_WIDE_GAMUT,
                &ACES_AP0,
                Adaptation::Cat02,
            )));
        },
    );
    r.add(
        "ARRI_LOGC4_to_ACES2065-1",
        "Convert ARRI LogC4 to ACES2065-1",
        |ops| {
            // ARRI_LOGC4_to_LINEAR, ArriCameras.cpp:53-66.
            ops.push(log_to_linear(
                2.0,
                &LogParams {
                    log_side_slope: 0.0647954196341293,
                    log_side_offset: -0.295908392682586,
                    lin_side_slope: 2231.82630906769,
                    lin_side_offset: 64.0,
                    lin_side_break: Some(-0.0180569961199113),
                    linear_slope: None,
                },
            ));
            ops.push(matrix(build_conversion_matrix(
                &ARRI_WIDE_GAMUT_4,
                &ACES_AP0,
                Adaptation::Cat02,
            )));
        },
    );

    // CAMERA::CANON (CanonCameras.cpp:146-197).
    r.add(
        "CANON_CLOG2-CGAMUT_to_ACES2065-1",
        "Convert Canon Log 2 Cinema Gamut to ACES2065-1",
        |ops| {
            canon_clog2_to_linear(ops);
            ops.push(matrix(build_conversion_matrix(
                &CANON_CGAMUT,
                &ACES_AP0,
                Adaptation::Cat02,
            )));
        },
    );
    r.add(
        "CURVE - CANON_CLOG2_to_LINEAR",
        "Convert Canon Log 2 to linear",
        canon_clog2_to_linear,
    );
    r.add(
        "CANON_CLOG3-CGAMUT_to_ACES2065-1",
        "Convert Canon Log 3 Cinema Gamut to ACES2065-1",
        |ops| {
            canon_clog3_to_linear(ops);
            ops.push(matrix(build_conversion_matrix(
                &CANON_CGAMUT,
                &ACES_AP0,
                Adaptation::Cat02,
            )));
        },
    );
    r.add(
        "CURVE - CANON_CLOG3_to_LINEAR",
        "Convert Canon Log 3 to linear",
        canon_clog3_to_linear,
    );

    // CAMERA::PANASONIC (PanasonicCameras.cpp:58-75).
    r.add(
        "PANASONIC_VLOG-VGAMUT_to_ACES2065-1",
        "Convert Panasonic Varicam V-Log V-Gamut to ACES2065-1",
        |ops| {
            // PANASONIC_VLOG_VGAMUT_to_LINEAR, PanasonicCameras.cpp:31-50.
            ops.push(log_to_linear(
                10.0,
                &LogParams {
                    log_side_slope: 0.241514,
                    log_side_offset: 0.598206,
                    lin_side_slope: 1.0,
                    lin_side_offset: 0.00873,
                    lin_side_break: Some(0.01),
                    linear_slope: None,
                },
            ));
            ops.push(matrix(build_conversion_matrix(
                &PANASONIC_VGAMUT,
                &ACES_AP0,
                Adaptation::Bradford,
            )));
        },
    );

    // CAMERA::RED (RedCameras.cpp:77-113).
    r.add(
        "RED_REDLOGFILM-RWG_to_ACES2065-1",
        "Convert RED LogFilm RED Wide Gamut to ACES2065-1",
        |ops| {
            // RED_REDLOGFILM_RWG_to_LINEAR, RedCameras.cpp:30-53: a log-affine curve (no break).
            let ref_white = 685.0 / 1023.0;
            let ref_black = 95.0 / 1023.0;
            let range_ = 0.002 * 1023.0;
            let gamma = 0.6;
            let (highlight, shadow) = (1.0, 0.0);
            let multi_factor = range_ / gamma;
            let gain = (highlight - shadow)
                / (1.0 - 10.0_f64.powf(multi_factor * (ref_black - ref_white)));
            let offset = gain - (highlight - shadow);
            ops.push(log_to_linear(
                10.0,
                &LogParams {
                    log_side_slope: 1.0 / multi_factor,
                    log_side_offset: ref_white,
                    lin_side_slope: 1.0 / gain,
                    lin_side_offset: (offset - shadow) / gain,
                    lin_side_break: None,
                    linear_slope: None,
                },
            ));
            ops.push(matrix(build_conversion_matrix(
                &RED_WIDE_GAMUT_RGB,
                &ACES_AP0,
                Adaptation::Bradford,
            )));
        },
    );
    r.add(
        "RED_LOG3G10-RWG_to_ACES2065-1",
        "Convert RED Log3G10 RED Wide Gamut to ACES2065-1",
        |ops| {
            // RED_LOG3G10_RWG_to_LINEAR, RedCameras.cpp:55-69.
            let lin_side_slope = 155.975327;
            ops.push(log_to_linear(
                10.0,
                &LogParams {
                    log_side_slope: 0.224282,
                    log_side_offset: 0.0,
                    lin_side_slope,
                    lin_side_offset: 0.01 * lin_side_slope + 1.0,
                    lin_side_break: Some(-0.01),
                    linear_slope: None,
                },
            ));
            ops.push(matrix(build_conversion_matrix(
                &RED_WIDE_GAMUT_RGB,
                &ACES_AP0,
                Adaptation::Bradford,
            )));
        },
    );

    // CAMERA::SONY (SonyCameras.cpp:64-143).
    r.add(
        "SONY_SLOG3-SGAMUT3_to_ACES2065-1",
        "Convert Sony S-Log3 S-Gamut3 to ACES2065-1",
        |ops| {
            ops.push(sony_slog3_to_linear());
            ops.push(matrix(build_conversion_matrix(
                &SONY_SGAMUT3,
                &ACES_AP0,
                Adaptation::Cat02,
            )));
        },
    );
    r.add(
        "SONY_SLOG3-SGAMUT3.CINE_to_ACES2065-1",
        "Convert Sony S-Log3 S-Gamut3.Cine to ACES2065-1",
        |ops| {
            ops.push(sony_slog3_to_linear());
            ops.push(matrix(build_conversion_matrix(
                &SONY_SGAMUT3_CINE,
                &ACES_AP0,
                Adaptation::Cat02,
            )));
        },
    );
    r.add(
        "SONY_SLOG3-SGAMUT3-VENICE_to_ACES2065-1",
        "Convert Sony S-Log3 S-Gamut3 for the Venice camera to ACES2065-1",
        |ops| {
            ops.push(sony_slog3_to_linear());
            // NB: Primaries were not provided for this case, only the matrix.
            ops.push(matrix(m33([
                0.7933297411,
                0.0890786256,
                0.1175916333, //
                0.0155810585,
                1.0327123069,
                -0.0482933654, //
                -0.0188647478,
                0.0127694121,
                1.0060953358,
            ])));
        },
    );
    r.add(
        "SONY_SLOG3-SGAMUT3.CINE-VENICE_to_ACES2065-1",
        "Convert Sony S-Log3 S-Gamut3.Cine for the Venice camera to ACES2065-1",
        |ops| {
            ops.push(sony_slog3_to_linear());
            ops.push(matrix(m33([
                0.6742570921,
                0.2205717359,
                0.1051711720, //
                -0.0093136061,
                1.1059588614,
                -0.0966452553, //
                -0.0382090673,
                -0.0179383766,
                1.0561474439,
            ])));
        },
    );
}
