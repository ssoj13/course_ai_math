//! What the built-in config does to known values, pinned to PyOpenColorIO 2.5.1 on the same
//! config file (`builtin/data/studio-config-all-views-*.ocio`).
//!
//! The first test is the table a viewer's display/view menu rests on: every display, every view,
//! ACEScg 0, 0.18 and 1.0, as OCIO computes them. The others anchor the HDR and sRGB paths to
//! values that hold by definition. `--nocapture` prints the HDR nits table.
//!
//! The table was made with (`DisplayViewTransform(src="ACEScg", display, view)`, CPU processor):
//!
//! ```text
//! for d in config.getDisplays():
//!     for v in config.getViews(d):
//!         cpu = config.getProcessor(DisplayViewTransform(src="ACEScg", display=d, view=v))
//!                     .getDefaultCPUProcessor()
//!         row(d, v, [cpu.applyRGB([x] * 3) for x in (0.0, 0.18, 1.0)])
//! ```

use vfx_ocio::builtin;

type Rgb = [f32; 3];

/// Largest difference allowed from PyOCIO. The measured worst is 6.7e-6: OCIO's sRGB moncurve,
/// evaluated with f32 constants, encodes 1.0 as 1.0000067 where ours gives 1.0.
const TOLERANCE: f32 = 1e-5;

/// `(display, view, ACEScg 0, ACEScg 0.18, ACEScg 1.0)` from PyOCIO 2.5.1.
#[rustfmt::skip]
const PYOCIO_VIEWS: &[(&str, &str, Rgb, Rgb, Rgb)] = &[
    ("sRGB - Display", "ACES 2.0 - SDR 100 nits (Rec.709)", [0.0, 0.0, 0.0], [0.3491874, 0.34918812, 0.349_187_8], [0.706_683, 0.70668364, 0.70668334]),
    ("sRGB - Display", "ACES 2.0 - SDR 100 nits (Rec.709 D60 in Rec.709 D65)", [0.0, 0.0, 0.0], [0.349_187_3, 0.340_606_4, 0.32715747], [0.70668304, 0.69051623, 0.665169]),
    ("sRGB - Display", "Un-tone-mapped", [0.0, 0.0, 0.0], [0.46135455, 0.461_354_6, 0.461_354_6], [1.0000067, 1.0000067, 1.0000067]),
    ("sRGB - Display", "Video (colorimetric)", [0.0, 0.0, 0.0], [0.46135455, 0.461_354_6, 0.461_354_6], [1.0000067, 1.0000067, 1.0000067]),
    ("sRGB - Display", "Raw", [0.0, 0.0, 0.0], [0.18, 0.18, 0.18], [1.0, 1.0, 1.0]),
    ("Display P3 - Display", "ACES 2.0 - SDR 100 nits (P3 D65)", [0.0, 0.0, 0.0], [0.34918746, 0.34918818, 0.34918785], [0.706_682_9, 0.7066837, 0.70668334]),
    ("Display P3 - Display", "ACES 2.0 - SDR 100 nits (P3 D60 in P3 D65)", [0.0, 0.0, 0.0], [0.34918755, 0.34237492, 0.329_976_3], [0.7066831, 0.693_848_5, 0.670_482_2]),
    ("Display P3 - Display", "Un-tone-mapped", [0.0, 0.0, 0.0], [0.461_354_6, 0.461_354_6, 0.461_354_6], [1.0000067, 1.0000067, 1.0000067]),
    ("Display P3 - Display", "Video (colorimetric)", [0.0, 0.0, 0.0], [0.461_354_6, 0.461_354_6, 0.461_354_6], [1.0000067, 1.0000067, 1.0000067]),
    ("Display P3 - Display", "Raw", [0.0, 0.0, 0.0], [0.18, 0.18, 0.18], [1.0, 1.0, 1.0]),
    ("Display P3 HDR - Display", "ACES 2.0 - HDR 1000 nits (P3 D65)", [0.0, 0.0, 0.0], [0.41702783, 0.41702864, 0.4170284], [1.0283132, 1.028_315, 1.0283142]),
    ("Display P3 HDR - Display", "ACES 2.0 - HDR 1000 nits (P3 D60 in P3 D65)", [0.0, 0.0, 0.0], [0.41702795, 0.40907124, 0.39459026], [1.0283136, 1.0100611, 0.97682697]),
    ("Display P3 HDR - Display", "ACES 2.0 - SDR 100 nits (Rec.709)", [0.0, 0.0, 0.0], [0.34918755, 0.349_188_1, 0.34918785], [0.706_683_3, 0.706_683_6, 0.7066834]),
    ("Display P3 HDR - Display", "ACES 2.0 - SDR 100 nits (Rec.709 D60 in P3 D65)", [0.0, 0.0, 0.0], [0.34918746, 0.34237495, 0.329_976_3], [0.70668316, 0.69384843, 0.670_482_2]),
    ("Display P3 HDR - Display", "Un-tone-mapped", [0.0, 0.0, 0.0], [0.461_354_6, 0.461_354_6, 0.461_354_6], [1.0000067, 1.0000067, 1.0000067]),
    ("Display P3 HDR - Display", "Video (colorimetric)", [0.0, 0.0, 0.0], [0.461_354_6, 0.461_354_6, 0.461_354_6], [1.0000067, 1.0000067, 1.0000067]),
    ("Display P3 HDR - Display", "Raw", [0.0, 0.0, 0.0], [0.18, 0.18, 0.18], [1.0, 1.0, 1.0]),
    ("Gamma 2.2 Rec.709 - Display", "ACES 2.0 - SDR 100 nits (Rec.709)", [0.0, 0.0, 0.0], [0.351_117_3, 0.351118, 0.35111773], [0.700_907_4, 0.70090806, 0.700_907_8]),
    ("Gamma 2.2 Rec.709 - Display", "ACES 2.0 - SDR 100 nits (Rec.709 D60 in Rec.709 D65)", [0.0, 0.0, 0.0], [0.35111725, 0.34299347, 0.33029312], [0.70090747, 0.684_694, 0.659338]),
    ("Gamma 2.2 Rec.709 - Display", "Un-tone-mapped", [0.0, 0.0, 0.0], [0.45865595, 0.458_656, 0.45865598], [1.0000067, 1.0000067, 1.0000067]),
    ("Gamma 2.2 Rec.709 - Display", "Video (colorimetric)", [0.0, 0.0, 0.0], [0.45865595, 0.458_656, 0.45865598], [1.0000067, 1.0000067, 1.0000067]),
    ("Gamma 2.2 Rec.709 - Display", "Raw", [0.0, 0.0, 0.0], [0.18, 0.18, 0.18], [1.0, 1.0, 1.0]),
    ("P3-D65 - Display", "ACES 2.0 - SDR 100 nits (P3 D65)", [0.0, 0.0, 0.0], [0.41246045, 0.4124611, 0.41246083], [0.740_294_2, 0.740_294_9, 0.740_294_7]),
    ("P3-D65 - Display", "ACES 2.0 - SDR 100 nits (P3 D60 in P3 D65)", [0.0, 0.0, 0.0], [0.41246057, 0.40603876, 0.39432973], [0.74029446, 0.72877216, 0.707_756]),
    ("P3-D65 - Display", "ACES 2.0 - SDR 100 nits (Rec.709)", [0.0, 0.0, 0.0], [0.41246057, 0.41246104, 0.41246083], [0.740_294_6, 0.740_294_8, 0.740_294_7]),
    ("P3-D65 - Display", "ACES 2.0 - SDR 100 nits (Rec.709 D60 in P3 D65)", [0.0, 0.0, 0.0], [0.41246045, 0.40603882, 0.39432973], [0.740_294_5, 0.72877216, 0.707_756]),
    ("P3-D65 - Display", "Un-tone-mapped", [0.0, 0.0, 0.0], [0.517_087_8, 0.517_087_8, 0.517_087_8], [1.0000061, 1.0000061, 1.0000061]),
    ("P3-D65 - Display", "Video (colorimetric)", [0.0, 0.0, 0.0], [0.517_087_8, 0.517_087_8, 0.517_087_8], [1.0000061, 1.0000061, 1.0000061]),
    ("P3-D65 - Display", "Raw", [0.0, 0.0, 0.0], [0.18, 0.18, 0.18], [1.0, 1.0, 1.0]),
    ("Rec.1886 Rec.709 - Display", "ACES 2.0 - SDR 100 nits (Rec.709)", [0.0, 0.0, 0.0], [0.38311604, 0.38311672, 0.38311642], [0.72197443, 0.721_975, 0.721_974_8]),
    ("Rec.1886 Rec.709 - Display", "ACES 2.0 - SDR 100 nits (Rec.709 D60 in Rec.709 D65)", [0.0, 0.0, 0.0], [0.38311598, 0.374_982_4, 0.362_234_6], [0.721_974_5, 0.7066505, 0.6826247]),
    ("Rec.1886 Rec.709 - Display", "Un-tone-mapped", [0.0, 0.0, 0.0], [0.489_435_6, 0.48943567, 0.48943567], [1.0000063, 1.0000063, 1.0000063]),
    ("Rec.1886 Rec.709 - Display", "Video (colorimetric)", [0.0, 0.0, 0.0], [0.489_435_6, 0.48943567, 0.48943567], [1.0000063, 1.0000063, 1.0000063]),
    ("Rec.1886 Rec.709 - Display", "Raw", [0.0, 0.0, 0.0], [0.18, 0.18, 0.18], [1.0, 1.0, 1.0]),
    ("Rec.2100-HLG - Display", "ACES 2.0 - HDR 1000 nits (P3 D65)", [0.0, 0.0, 0.0], [0.29689825, 0.296_898_7, 0.296_898_5], [0.64086425, 0.64086485, 0.640_864_6]),
    ("Rec.2100-HLG - Display", "ACES 2.0 - HDR 1000 nits (P3 D60 in Rec.2020 D65)", [0.0, 0.0, 0.0], [0.29748777, 0.293_567_8, 0.282646], [0.64170206, 0.63608927, 0.619_890_3]),
    ("Rec.2100-HLG - Display", "Un-tone-mapped", [0.0, 0.0, 0.0], [0.32478118, 0.32478118, 0.32478118], [0.629_620_3, 0.629_620_3, 0.629_620_3]),
    ("Rec.2100-HLG - Display", "Video (colorimetric)", [0.0, 0.0, 0.0], [0.32478118, 0.32478118, 0.32478118], [0.629_620_3, 0.629_620_3, 0.629_620_3]),
    ("Rec.2100-HLG - Display", "Raw", [0.0, 0.0, 0.0], [0.18, 0.18, 0.18], [1.0, 1.0, 1.0]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 1000 nits (P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.32983688, 0.32983717, 0.32983705], [0.51447237, 0.51447266, 0.51447254]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 1000 nits (Rec.2020)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.32983693, 0.329_837_1, 0.32983705], [0.514_472_4, 0.5144726, 0.51447254]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 1000 nits (P3 D60 in Rec.2020 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.32983693, 0.32763892, 0.32139894], [0.514_472_4, 0.511_800_8, 0.5041911]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 1000 nits (Rec.2020 D60 in Rec.2020 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.32983702, 0.32763892, 0.321399], [0.51447254, 0.511_800_8, 0.50419116]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 2000 nits (P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.3366529, 0.33665314, 0.33665305], [0.52788615, 0.527_886_4, 0.527_886_3]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 2000 nits (Rec.2020)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.33665296, 0.336_653_1, 0.33665305], [0.527_886_2, 0.527_886_4, 0.527_886_3]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 2000 nits (P3 D60 in Rec.2020 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.33665293, 0.33443144, 0.32812387], [0.527_886_2, 0.525_191_7, 0.517_515_5]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 2000 nits (Rec.2020 D60 in Rec.2020 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.336_653, 0.33443144, 0.3281239], [0.52788633, 0.525_191_7, 0.51751554]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 4000 nits (P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.342_228_1, 0.34222835, 0.34222826], [0.5376423, 0.537_642_5, 0.537_642_4]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 4000 nits (Rec.2020)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.34222817, 0.34222835, 0.34222826], [0.53764236, 0.537_642_5, 0.537_642_4]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 4000 nits (P3 D60 in Rec.2020 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.34222814, 0.33998784, 0.33362606], [0.53764236, 0.53493214, 0.527_209_9]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 4000 nits (Rec.2020 D60 in Rec.2020 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.34222823, 0.33998784, 0.33362612], [0.537_642_5, 0.5349322, 0.527_209_9]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 500 nits (P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.32197955, 0.32197973, 0.32197964], [0.49653894, 0.496_539_1, 0.496539]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 500 nits (Rec.2020)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.321_979_6, 0.3219797, 0.32197964], [0.49653903, 0.49653906, 0.49653903]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 500 nits (P3 D60 in Rec.2020 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.321_979_6, 0.31980923, 0.313_649_1], [0.496539, 0.49390003, 0.48638558]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - HDR 500 nits (Rec.2020 D60 in Rec.2020 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.32197964, 0.31980923, 0.31364912], [0.49653906, 0.49390006, 0.48638564]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - SDR 100 nits (Rec.709)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.299_698_4, 0.299_698_6, 0.29969853], [0.43188426, 0.43188435, 0.43188432]),
    ("Rec.2100-PQ - Display", "ACES 2.0 - SDR 100 nits (Rec.709 D60 in Rec.2020 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.299_698_4, 0.29761073, 0.291688], [0.43188426, 0.42938542, 0.42227763]),
    ("Rec.2100-PQ - Display", "Un-tone-mapped", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.34796667, 0.34796667, 0.34796667], [0.5080784, 0.5080784, 0.5080784]),
    ("Rec.2100-PQ - Display", "Video (colorimetric)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.34796667, 0.34796667, 0.34796667], [0.5080784, 0.5080784, 0.5080784]),
    ("Rec.2100-PQ - Display", "Raw", [0.0, 0.0, 0.0], [0.18, 0.18, 0.18], [1.0, 1.0, 1.0]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - HDR 1000 nits (P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.32983682, 0.32983717, 0.32983705], [0.514_472_3, 0.51447266, 0.51447254]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - HDR 1000 nits (P3 D60 in P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.32983688, 0.32646036, 0.32021177], [0.51447237, 0.51036644, 0.502_739]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - HDR 2000 nits (P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.33665285, 0.33665314, 0.33665305], [0.527_886_1, 0.52788645, 0.527_886_3]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - HDR 2000 nits (P3 D60 in P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.33665287, 0.333_240_2, 0.32692367], [0.52788615, 0.523745, 0.5160504]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - HDR 4000 nits (P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.34222803, 0.34222835, 0.34222826], [0.5376423, 0.537_642_5, 0.537_642_4]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - HDR 4000 nits (P3 D60 in P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.34222808, 0.33878645, 0.33241543], [0.5376423, 0.53347677, 0.52573586]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - HDR 500 nits (P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.32197952, 0.32197973, 0.32197964], [0.49653888, 0.496_539_1, 0.496539]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - HDR 500 nits (P3 D60 in P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.32197958, 0.3186456, 0.31247726], [0.49653897, 0.49248338, 0.48495206]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - HDR 108 nits (P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.25566033, 0.25566062, 0.25566053], [0.39955226, 0.39955243, 0.39955235]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - HDR 108 nits (P3 D60 in P3 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.25566038, 0.25273317, 0.24732728], [0.39955232, 0.39584202, 0.38896504]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - SDR 100 nits (Rec.709)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.29969838, 0.29969862, 0.29969853], [0.43188426, 0.43188435, 0.43188432]),
    ("ST2084-P3-D65 - Display", "ACES 2.0 - SDR 100 nits (Rec.709 D60 in Rec.2020 D65)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.3007459, 0.297_533, 0.29159164], [0.43313667, 0.42929232, 0.422_161_8]),
    ("ST2084-P3-D65 - Display", "Un-tone-mapped", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.34796667, 0.34796667, 0.34796667], [0.5080784, 0.5080784, 0.5080784]),
    ("ST2084-P3-D65 - Display", "Video (colorimetric)", [7.309427e-07, 7.309427e-07, 7.309427e-07], [0.34796667, 0.34796667, 0.34796667], [0.5080784, 0.5080784, 0.5080784]),
    ("ST2084-P3-D65 - Display", "Raw", [0.0, 0.0, 0.0], [0.18, 0.18, 0.18], [1.0, 1.0, 1.0]),
];

/// Every display and view of the config, in OCIO's order, applied to ACEScg 0, 0.18 and 1.0,
/// equals PyOCIO. This is the menu a viewer shows and what each entry does to the anchors.
/// (The "D60 in D65" views are meant to tint a neutral: they simulate a D60 white.)
#[test]
fn every_display_view_matches_pyocio() {
    let config = builtin::aces_2_0_studio_all_views();
    let ours: Vec<(String, String)> = config
        .display_names()
        .iter()
        .flat_map(|d| {
            config
                .get_views(d)
                .into_iter()
                .map(move |v| (d.to_string(), v.name().to_string()))
        })
        .collect();
    let theirs: Vec<(String, String)> = PYOCIO_VIEWS
        .iter()
        .map(|(d, v, ..)| (d.to_string(), v.to_string()))
        .collect();
    assert_eq!(ours, theirs);

    let mut failures = Vec::new();
    let mut worst = 0.0f32;
    for &(display, view, at0, at18, at1) in PYOCIO_VIEWS {
        let proc = config
            .display_processor("ACEScg", display, view)
            .unwrap_or_else(|e| panic!("{display} / {view}: {e}"));
        for (x, want) in [(0.0f32, at0), (0.18, at18), (1.0, at1)] {
            let mut px = [[x, x, x]];
            proc.apply_rgb(&mut px);
            let err = (0..3)
                .map(|c| (px[0][c] - want[c]).abs())
                .fold(0.0f32, f32::max);
            worst = worst.max(err);
            if err > TOLERANCE {
                failures.push(format!("{display} / {view} @ {x}: {:?} vs {want:?}", px[0]));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "worst {worst:e}\n{}",
        failures.join("\n")
    );
}

/// PQ is an ABSOLUTE encoding, so an HDR view can be checked against nits.
///
/// ACES 2.0 anchors scene mid-grey at 10 nits regardless of the peak the
/// transform targets, so every HDR view here should put 0.18 at ~10 nits and
/// scene 1.0 well above it.
#[test]
fn hdr_views_land_where_the_nits_say() {
    /// SMPTE ST 2084 inverse EOTF: PQ code value to cd/m2.
    fn pq_to_nits(e: f64) -> f64 {
        const M1: f64 = 0.1593017578125;
        const M2: f64 = 78.84375;
        const C1: f64 = 0.8359375;
        const C2: f64 = 18.8515625;
        const C3: f64 = 18.6875;
        let ep = e.powf(1.0 / M2);
        let num = (ep - C1).max(0.0);
        let den = C2 - C3 * ep;
        if den <= 0.0 {
            return f64::INFINITY;
        }
        (num / den).powf(1.0 / M1) * 10_000.0
    }

    let config = builtin::aces_2_0_studio_all_views();
    let display = "Rec.2100-PQ - Display";
    println!("\n=== {display}: what the PQ code values mean in nits ===");
    println!(
        "{:<52} {:>10} {:>10} {:>12}",
        "view", "0.18 code", "0.18 nits", "1.0 nits"
    );

    let views: Vec<String> = config
        .get_views(display)
        .into_iter()
        .map(|v| v.name().to_string())
        .collect();
    for v in views {
        let Ok(proc) = config.display_processor("ACEScg", display, &v) else {
            continue;
        };
        let sample = |x: f32| {
            let mut px = [[x, x, x]];
            proc.apply_rgb(&mut px);
            px[0][0] as f64
        };
        let (mid, white) = (sample(0.18), sample(1.0));
        println!(
            "{v:<52} {mid:>10.4} {:>10.2} {:>12.2}",
            pq_to_nits(mid),
            pq_to_nits(white)
        );
    }

    // Two anchors, both exact by definition rather than by measurement.
    //
    // `Un-tone-mapped` is a plain encode with no rendering, so it must land on
    // the convention itself: display-referred 1.0 IS 100 cd/m2. If the PQ chain
    // ever loses its scaling again, this is 1.0 nit and the whole HDR path is
    // 100x dark while still looking like a perfectly well-formed picture.
    let plain = config
        .display_processor("ACEScg", display, "Un-tone-mapped")
        .expect("Un-tone-mapped exists on the PQ display");
    let nits_at = |x: f32| {
        let mut px = [[x, x, x]];
        plain.apply_rgb(&mut px);
        pq_to_nits(px[0][0] as f64)
    };
    assert!(
        (nits_at(1.0) - 100.0).abs() < 0.5,
        "display-referred 1.0 must encode as 100 cd/m2, got {:.2}",
        nits_at(1.0)
    );
    assert!(
        (nits_at(0.18) - 18.0).abs() < 0.2,
        "display-referred 0.18 must encode as 18 cd/m2, got {:.2}",
        nits_at(0.18)
    );

    // And the rendering anchor: ACES 2.0 puts scene mid-grey at 10 nits, the
    // same value the SDR display reaches as 0.10 display-linear.
    let aces = config
        .display_processor("ACEScg", display, "ACES 2.0 - SDR 100 nits (Rec.709)")
        .expect("the SDR view exists on the PQ display");
    let mut px = [[0.18_f32; 3]];
    aces.apply_rgb(&mut px);
    let mid = pq_to_nits(px[0][0] as f64);
    assert!(
        (mid - 10.0).abs() < 0.3,
        "ACES 2.0 renders scene mid-grey to 10 cd/m2, got {mid:.2}"
    );
}

/// The plain encode path, with no rendering: what a JPG/PNG viewer needs.
///
/// A display-referred file is already encoded, so showing it is a decode
/// followed by an encode for the monitor. When both are sRGB that round trip
/// must be the identity, or every PNG in the app is wrong by a gamma.
#[test]
fn srgb_encode_is_exact_and_round_trips() {
    let config = builtin::aces_2_0_studio_all_views();
    // `TestSchema` is `#[cfg(test)]`-only, so name the spaces directly.
    let (linear, srgb) = ("Linear Rec.709 (sRGB)", "sRGB - Display");

    let enc = config
        .processor(linear, srgb)
        .expect("linear -> sRGB display");
    let dec = config
        .processor(srgb, linear)
        .expect("sRGB display -> linear");

    // PyOCIO 2.5.1: `getProcessor("Linear Rec.709 (sRGB)", "sRGB - Display")`. OCIO's sRGB
    // display is a moncurve with f32 constants, so 1.0 encodes to 1.0000067 (ours: 1.0).
    for (x, want) in [
        (0.0_f32, 0.0_f32),
        (0.003_130_8, 0.040_449_76),
        (0.18, 0.461_354_6),
        (0.5, 0.735_358_06),
        (1.0, 1.000_006_7),
    ] {
        let mut px = [[x, x, x]];
        enc.apply_rgb(&mut px);
        let encoded = px[0][0];
        assert!(
            (encoded - want).abs() < TOLERANCE,
            "encode({x}) = {encoded}, OCIO {want}"
        );
        let mut back = [[encoded, encoded, encoded]];
        dec.apply_rgb(&mut back);
        assert!(
            (back[0][0] - x).abs() < 1e-6,
            "decode(encode({x})) = {}",
            back[0][0]
        );
    }
}
