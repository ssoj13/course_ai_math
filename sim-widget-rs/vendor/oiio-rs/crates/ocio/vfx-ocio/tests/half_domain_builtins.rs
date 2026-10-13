//! The four OCIO builtins built on a half-domain Lut1D (ADX10/16 density, the SDR-CINEMA DCI
//! simulations) in both directions, against PyOpenColorIO 2.5.1 at `OPTIMIZATION_LOSSLESS` (the
//! exact inverse renderer `InvLut1DRendererHalfCode`; this port's CPU always inverts exactly).
//! Red before: every inverse was refused (B31 remainder), so the studio config's
//! `ACES2065-1 -> ADX10/ADX16` conversions did not build.

use vfx_ocio::{BuiltinTransform, Processor, Transform, TransformDirection};

const PX: [[f32; 3]; 7] = [
    [0.18, 0.35, 0.6],
    [0.8, 0.5, 0.2],
    [0.02, 0.9, 0.45],
    [1.2, 0.05, 0.7],
    [-0.1, 0.3, 0.1],
    [0.0, 0.0, 0.0],
    [4.0, 10.0, 0.001],
];

type Row = (&'static str, bool, [[f32; 3]; 7]);

#[rustfmt::skip]
const PYOCIO: [Row; 8] = [
    ("ADX10_to_ACES2065-1", true, [[0.081_242_28, 0.099_712_69, 0.325_561_26], [1.554_288_9, 0.554_423_5, 0.119_303_03], [0.913_308_6, 5.298_533_4, 0.703_430_2], [9.660_276, 1.806_046_7, 2.264_082_4], [0.005_762_379, 0.034_395_94, 0.004_219_293_6], [1.000_000_3e-6, 1.000_000_3e-6, 9.999_903e-7], [55_717.605, 57_939.723, 6_879.102_5]]),
    ("ADX10_to_ACES2065-1", false, [[0.226_525_57, 0.535_414_9, 0.660_795], [0.671_062_8, 0.535_323_74, 0.359_873_03], [-0.389_208_9, 0.717_115_16, 0.702_314_85], [0.937_835_34, -0.162_368_24, 0.6100595], [-0.342_927, 0.584_039_9, 0.458_462_1], [-0.119_297_18, -0.119_297_19, -0.119_297_18], [0.784_013_3, 0.901_922, -0.408_823_16]]),
    ("ADX16_to_ACES2065-1", true, [[9_937.93, 7_939.341, 59221.25], [55_660.285, 57895.418, 6_535.73], [9_237.635, 50_798.055, 11_115.555], [57_230.85, 15447.252, 60_123.508], [8.710_228, 52.644_92, 5.721_174_2], [1.000_000_3e-6, 1.000_000_3e-6, 9.999_903e-7], [65_503.99, 65_503.992, 65_503.34]]),
    ("ADX16_to_ACES2065-1", false, [[0.056_576_956, 0.133_725_05, 0.165_039_94], [0.167_604_4, 0.133_702_26, 0.089_881_78], [-0.097_208_68, 0.179_106_44, 0.175_409_94], [0.234_233_45, -0.040_553_03, 0.152_368_26], [-0.085_649_33, 0.145_869_61, 0.114505336], [-0.029_795_62, -0.029_795_624, -0.029_795_62], [0.195_814_9, 0.225_263_71, -0.10210754]]),
    ("ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-CINEMA-D60sim-DCI_1.0", true, [[0.119_173_44, 0.179_083_11, 0.478_177_46], [0.46676147, 0.408_965_14, 0.145_041_3], [0.148_274_8, 0.409_431_82, 0.370_291_95], [0.550_738_2, 0.222_144_2, 0.549_488_66], [0.041_632_44, 0.153_689_8, 0.046_208_4], [2.824_725_5e-11, 2.798_839_3e-11, 3.218_335_3e-11], [0.720_811_84, 0.815_559_45, 0.266_346_42]]),
    ("ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-CINEMA-D60sim-DCI_1.0", false, [[0.267_553_78, 0.677_114_25, 0.801_520_6], [14.20858, 0.959_748_27, 0.000_575_065_6], [2.341_439_7, 14.205_383, 0.180_208_62], [14.230566, 0.841_411_23, 0.846_730_2], [0.158_380_09, 0.833_296_95, 0.152_587_32], [0.001_988_737_6, 0.001_988_737_6, 0.001_988_737_8], [13.652_918, 14.767_923, 0.0]]),
    ("ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-CINEMA-D65sim-DCI_1.1", true, [[0.123_861_21, 0.180_588_14, 0.515_343_25], [0.459_166_47, 0.405_791_76, 0.155_833_51], [0.149_318_4, 0.409_615_1, 0.397_871_1], [0.548_372_86, 0.220_188_86, 0.593_392_3], [0.040_791_675, 0.153_505_04, 0.049_198_99], [2.815_518_7e-11, 2.792_448_9e-11, 3.466_576e-11], [0.703_863_14, 0.803_366_5, 0.285_498_77]]),
    ("ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-CINEMA-D65sim-DCI_1.1", false, [[0.259_207_73, 0.669_000_74, 0.717_792], [14.205_813, 0.958_656_9, 0.0], [2.335_407, 14.201_929, 0.143_269_1], [14.231_306, 0.824_655_5, 0.670_897_5], [0.15704605, 0.830_872_1, 0.146_246_76], [0.001_988_737_6, 0.001_988_737_6, 0.001_988_737_8], [13.652_836, 14.767_836, 0.0]]),
];

fn builtin(style: &str, forward: bool) -> Transform {
    Transform::Builtin(BuiltinTransform {
        style: style.to_string(),
        direction: if forward {
            TransformDirection::Forward
        } else {
            TransformDirection::Inverse
        },
    })
}

#[test]
fn half_domain_builtins_match_pyocio_both_ways() {
    let mut failures = Vec::new();
    for (style, forward, want) in PYOCIO {
        let p = match Processor::from_transform(
            &builtin(style, forward),
            TransformDirection::Forward,
        ) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("{style} forward={forward}: {e}"));
                continue;
            }
        };
        let mut px = PX;
        p.apply_rgb(&mut px);
        for k in 0..PX.len() {
            for c in 0..3 {
                let (got, w) = (px[k][c], want[k][c]);
                // The forward DCI sims output ~3e-11 for black; compare those absolutely.
                if (got - w).abs() > 1e-4 * w.abs().max(1e-3) {
                    failures.push(format!(
                        "{style} forward={forward} px{k}[{c}]: {got} vs PyOCIO {w}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The studio config's ADX spaces convert from ACES2065-1 (the inverse direction) and match the
/// builtin's inverse above.
#[test]
fn studio_config_aces_to_adx_builds() {
    let config = vfx_ocio::builtin::aces_2_0_studio_all_views();
    for (space, style) in [
        ("ADX10", "ADX10_to_ACES2065-1"),
        ("ADX16", "ADX16_to_ACES2065-1"),
    ] {
        let p = config
            .processor("ACES2065-1", space)
            .unwrap_or_else(|e| panic!("{space}: {e}"));
        let mut px = PX;
        p.apply_rgb(&mut px);
        let (_, _, want) = PYOCIO.iter().find(|(s, f, _)| *s == style && !*f).unwrap();
        for k in 0..PX.len() {
            for c in 0..3 {
                assert!(
                    (px[k][c] - want[k][c]).abs() <= 1e-4 * want[k][c].abs().max(1e-3),
                    "{space} px{k}[{c}]: {} vs {}",
                    px[k][c],
                    want[k][c]
                );
            }
        }
    }
}
