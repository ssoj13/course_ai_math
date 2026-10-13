//! Every curve against numbers from outside this crate.
//!
//! - OpenColorIO's own unit-test table (`tests/cpu/transforms/BuiltinTransform_tests.cpp`, the
//!   `CURVE - *` rows), with OCIO's tolerance.
//! - For a curve OCIO has no builtin for, or a point the table does not cover, the value computed
//!   in f64 from the published definition named on the row. The ones at 18% grey agree with
//!   PyOCIO 2.5.1 to 1e-6 (`docs/audit/tq-ocio.md` §3).
//!
//! This replaces a file whose "OCIO oracle" was the curve re-typed with the same constants, so a
//! wrong constant was wrong in both places.

use vfx_transfer::{
    acescct, apple_log, bmd_film, canon_log, d_log, davinci_intermediate, gamma, hlg, log_c,
    log_c4, pq, rec709, red_log, s_log2, s_log3, srgb, v_log,
};

/// One curve, some inputs, the reference outputs and the tolerance (relative above 1.0).
struct Row {
    name: &'static str,
    f: fn(f32) -> f32,
    inputs: &'static [f32],
    want: &'static [f64],
    tol: f64,
}

fn check(rows: &[Row]) {
    let mut failures = Vec::new();
    for row in rows {
        assert_eq!(
            row.inputs.len(),
            row.want.len(),
            "{}: table shape",
            row.name
        );
        for (&x, &want) in row.inputs.iter().zip(row.want) {
            let got = (row.f)(x) as f64;
            if (got - want).abs() > row.tol * want.abs().max(1.0) {
                failures.push(format!("{}({x}) = {got}, want {want}", row.name));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// OCIO's scene-linear HLG (`Displays.cpp` `namespace HLG`, `E_MAX = 3`) is the BT.2100 OETF
/// of `E / 3`.
fn ocio_hlg_oetf(e: f32) -> f32 {
    hlg::oetf(e / 3.0)
}

fn ocio_hlg_oetf_inverse(v: f32) -> f32 {
    hlg::eotf(v) * 3.0
}

#[test]
fn ocio_builtin_curve_rows() {
    check(&[
        Row {
            name: "CURVE - ACEScct-LOG_to_LINEAR",
            f: acescct::decode,
            inputs: &[0.5, 0.4, 0.3],
            want: &[0.514056913328, 0.152618314084, 0.045310838527],
            tol: 1e-6,
        },
        Row {
            name: "CURVE - APPLE_LOG_to_LINEAR",
            f: apple_log::decode,
            inputs: &[0.5, 0.4, 0.3],
            want: &[0.198913991, 0.083076466024, 0.0315782763],
            tol: 1e-6,
        },
        Row {
            name: "CURVE - CANON_CLOG2_to_LINEAR",
            f: canon_log::clog2_decode,
            inputs: &[0.5, 0.4, 0.3],
            want: &[0.492082215086, 0.183195624930, 0.064213555991],
            tol: 1e-6,
        },
        Row {
            name: "CURVE - CANON_CLOG3_to_LINEAR",
            f: canon_log::clog3_decode,
            inputs: &[0.5, 0.4, 0.3],
            want: &[0.580777404788, 0.282284436009, 0.122823721131],
            tol: 1e-6,
        },
        Row {
            name: "CURVE - ST-2084_to_LINEAR",
            f: pq::ocio_pq_to_lin,
            inputs: &[0.5, 0.4, 0.3, -0.1, -0.3, 1.01],
            want: &[
                0.922457089941,
                0.324479178538,
                0.100382263105,
                -0.0032456566,
                -0.10038226,
                110.045776,
            ],
            tol: 4e-5,
        },
        Row {
            name: "CURVE - LINEAR_to_ST-2084",
            f: pq::ocio_lin_to_pq,
            inputs: &[0.5, 0.4, 0.3, -0.1, 101.0, 0.2],
            want: &[
                0.440281573420,
                0.419284117712,
                0.392876186489,
                -0.299699098,
                1.00104129,
                0.357012421,
            ],
            tol: 1e-5,
        },
        Row {
            name: "CURVE - HLG-OETF",
            f: ocio_hlg_oetf,
            inputs: &[0.5, 0.4, 0.3, 0.2],
            want: &[0.656409985167, 0.608926718364, 0.544089493962, 0.4472135955],
            tol: 1e-5,
        },
        Row {
            name: "CURVE - HLG-OETF-INVERSE",
            f: ocio_hlg_oetf_inverse,
            inputs: &[0.5, 0.4, 0.3, 0.9],
            want: &[0.25, 0.16, 0.09, 1.745512772886],
            tol: 1e-5,
        },
    ]);
}

#[test]
fn published_definitions() {
    check(&[
        Row {
            name: "sRGB, IEC 61966-2-1: 1.055 x^(1/2.4) - 0.055, 12.92 x below 0.0031308",
            f: srgb::oetf,
            inputs: &[0.18, 0.003_130_8, 0.001, 1.0, 0.0],
            want: &[0.46135613, 0.04044994, 0.01292, 1.0, 0.0],
            tol: 1e-6,
        },
        Row {
            name: "BT.709: 1.099 x^0.45 - 0.099",
            f: rec709::oetf,
            inputs: &[0.18],
            want: &[0.40900773],
            tol: 1e-6,
        },
        Row {
            name: "gamma 2.2: x^(1/2.2)",
            f: gamma::oetf_22,
            inputs: &[0.18],
            want: &[0.45865645],
            tol: 1e-6,
        },
        Row {
            name: "gamma 2.4: x^(1/2.4)",
            f: gamma::oetf_24,
            inputs: &[0.18],
            want: &[0.48943709],
            tol: 1e-6,
        },
        Row {
            name: "gamma 2.6: x^(1/2.6)",
            f: gamma::oetf_26,
            inputs: &[0.5, 0.18],
            want: &[0.76598318, 0.51709025],
            tol: 1e-6,
        },
        Row {
            name: "ACEScct, S-2016-001: (log2 x + 9.72) / 17.52",
            f: acescct::encode,
            inputs: &[0.18, 1.0],
            want: &[0.41358840, 0.55479452],
            tol: 1e-6,
        },
        Row {
            name: "ARRI LogC3 EI800: 0.24719 log10(5.555556 x + 0.052272) + 0.385537; 0.092809 at 0",
            f: log_c::encode,
            inputs: &[0.18, 0.0],
            want: &[0.39100683, 0.092809],
            tol: 1e-5,
        },
        Row {
            name: "ARRI LogC4: ((log2(a x + 64) - 6) / 14) b + c",
            f: log_c4::encode,
            inputs: &[0.18],
            want: &[0.27839584],
            tol: 1e-6,
        },
        Row {
            name: "Sony S-Log3: (420 + 261.5 log10((x + 0.01) / 0.19)) / 1023",
            f: s_log3::encode,
            inputs: &[0.18],
            want: &[0.41055718],
            tol: 1e-6,
        },
        Row {
            name: "Panasonic V-Log: 0.241514 log10(x + 0.00873) + 0.598206",
            f: v_log::encode,
            inputs: &[0.18],
            want: &[0.42331145],
            tol: 1e-6,
        },
        Row {
            name: "RED Log3G10: 0.224282 log10((x + 0.01) 155.975327 + 1)",
            f: red_log::log3g10_encode,
            inputs: &[0.18],
            want: &[0.33333291],
            tol: 1e-6,
        },
        Row {
            name: "Apple Log (OCIO AppleCameras.cpp): 0.08550479 log2(x + 0.00964052) + 0.69336945",
            f: apple_log::encode,
            inputs: &[0.18],
            want: &[0.48827246],
            tol: 1e-6,
        },
        Row {
            name: "Canon Log 2 (OCIO CanonCameras.cpp): 0.092864125 + 0.24136077 log10(87.099375 x / 0.9 + 1)",
            f: canon_log::clog2_encode,
            inputs: &[0.18],
            want: &[0.39825469],
            tol: 1e-6,
        },
        Row {
            name: "Canon Log 3 (OCIO CanonCameras.cpp): 0.12240537 + 0.36726845 log10(14.98325 x / 0.9 + 1)",
            f: canon_log::clog3_encode,
            inputs: &[0.18],
            want: &[0.34338937],
            tol: 1e-6,
        },
        Row {
            name: "DaVinci Intermediate: (log2(x + 0.0075) + 7) 0.07329248",
            f: davinci_intermediate::encode,
            inputs: &[0.18],
            want: &[0.33604327],
            tol: 1e-6,
        },
        Row {
            name: "Blackmagic Film Gen 5: A ln(x + B) + C",
            f: bmd_film::bmd_film_gen5_encode,
            inputs: &[0.18],
            want: &[0.38356164],
            tol: 1e-6,
        },
        Row {
            name: "DJI D-Log: 0.256663 log10(0.9892 x + 0.0108) + 0.584555",
            f: d_log::encode,
            inputs: &[0.18],
            want: &[0.39876456],
            tol: 1e-6,
        },
        Row {
            name: "ST 2084 of 100 and 1000 cd/m2",
            f: pq::oetf,
            inputs: &[100.0, 1000.0],
            want: &[0.50807842, 0.75182710],
            // f32 through the m2 = 78.8 power: OCIO's own PQ rows allow 1e-5 too.
            tol: 1e-5,
        },
        Row {
            name: "BT.2100 HLG: a ln(12 E - b) + c, sqrt(3 E) up to 1/12",
            f: hlg::oetf,
            inputs: &[0.5, 1.0 / 12.0],
            want: &[0.87164347, 0.5],
            tol: 1e-6,
        },
    ]);
}

/// Every encode/decode pair returns its input across the curve's range, the log segments and
/// the linear toe included.
#[test]
fn encode_then_decode_is_identity() {
    type Pair = (&'static str, fn(f32) -> f32, fn(f32) -> f32);
    let pairs: &[Pair] = &[
        ("srgb", srgb::oetf, srgb::eotf),
        ("rec709", rec709::oetf, rec709::eotf),
        ("acescct", acescct::encode, acescct::decode),
        ("log_c", log_c::encode, log_c::decode),
        ("log_c4", log_c4::encode, log_c4::decode),
        ("s_log2", s_log2::encode, s_log2::decode),
        ("s_log3", s_log3::encode, s_log3::decode),
        ("v_log", v_log::encode, v_log::decode),
        ("log3g10", red_log::log3g10_encode, red_log::log3g10_decode),
        (
            "redlogfilm",
            red_log::redlogfilm_encode,
            red_log::redlogfilm_decode,
        ),
        ("apple_log", apple_log::encode, apple_log::decode),
        ("clog", canon_log::clog_encode, canon_log::clog_decode),
        ("clog2", canon_log::clog2_encode, canon_log::clog2_decode),
        ("clog3", canon_log::clog3_encode, canon_log::clog3_decode),
        (
            "davinci_intermediate",
            davinci_intermediate::encode,
            davinci_intermediate::decode,
        ),
        (
            "bmd_film_gen5",
            bmd_film::bmd_film_gen5_encode,
            bmd_film::bmd_film_gen5_decode,
        ),
        ("d_log", d_log::encode, d_log::decode),
        ("hlg", hlg::oetf, hlg::eotf),
    ];
    let inputs = [0.0f32, 0.001, 0.005, 0.01, 0.05, 0.18, 0.5, 0.9, 1.0];
    let mut failures = Vec::new();
    for &(name, encode, decode) in pairs {
        for x in inputs {
            let back = decode(encode(x));
            if (back - x).abs() > 1e-5 * x.max(1.0) {
                failures.push(format!("{name}: decode(encode({x})) = {back}"));
            }
        }
    }
    let nits = [0.0f32, 0.1, 1.0, 100.0, 1000.0, 10000.0];
    for x in nits {
        let back = pq::eotf(pq::oetf(x));
        if (back - x).abs() > 1e-4 * x.max(1.0) {
            failures.push(format!("pq: eotf(oetf({x})) = {back}"));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
