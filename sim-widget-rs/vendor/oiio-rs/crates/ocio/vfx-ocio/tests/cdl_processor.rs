//! ROADMAP G7: the processor's CDL is `vfx_lut::cdl` (pinned to PyOpenColorIO 2.5.1 in
//! `vfx-lut/tests/cdl_pyocio.rs`), with and without optimisation. LOSSLESS rewrites a power-free
//! CDL into matrices and clamps (`ProcessorOp::simpler_replacement`, OCIO
//! `getSimplerReplacement`); that rewrite must give the same pixels, including the final [0, 1]
//! clamp of the inverse ASC style that OCIO's own rewrite drops.

use vfx_ocio::{
    CdlStyle, CdlTransform, OptimizationLevel, Processor, Transform, TransformDirection,
};

/// One CDL parameter set: slope, offset, power, saturation.
type CdlParams = ([f32; 3], [f32; 3], [f32; 3], f32);

const PARAMS: [CdlParams; 3] = [
    ([1.2, 0.9, 1.1], [0.02, -0.01, 0.05], [1.8, 0.7, 1.3], 0.8),
    ([1.2, 0.9, 1.1], [0.02, -0.01, 0.05], [1.0, 1.0, 1.0], 0.8),
    ([1.2, 0.9, 1.1], [0.02, -0.01, 0.05], [1.8, 1.0, 1.3], 1.0),
];
const INPUTS: [[f32; 3]; 4] = [
    [0.5, 0.3, 0.2],
    [0.9, 0.05, 0.6],
    [-0.2, 1.4, 0.35],
    [2.5, -0.7, 0.0],
];

#[test]
fn processor_cdl_is_vfx_lut_cdl_at_every_optimisation_level() {
    use vfx_lut::cdl::*;
    for (slope, offset, power, sat) in PARAMS {
        for style in [CdlStyle::AscCdl, CdlStyle::NoClamp] {
            for direction in [TransformDirection::Forward, TransformDirection::Inverse] {
                let t = Transform::Cdl(CdlTransform {
                    name: String::new(),
                    slope: slope.map(f64::from),
                    offset: offset.map(f64::from),
                    power: power.map(f64::from),
                    saturation: f64::from(sat),
                    style,
                    direction,
                });
                let base = Processor::from_transform(&t, TransformDirection::Forward).unwrap();
                for level in [OptimizationLevel::None, OptimizationLevel::Lossless] {
                    let proc = base.optimized(level);
                    for x in INPUTS {
                        let mut want = x;
                        match (style, direction) {
                            (CdlStyle::AscCdl, TransformDirection::Forward) => {
                                apply_cdl_clamped(&mut want, slope, offset, power, sat)
                            }
                            (CdlStyle::NoClamp, TransformDirection::Forward) => {
                                apply_cdl_unclamped(&mut want, slope, offset, power, sat)
                            }
                            (CdlStyle::AscCdl, _) => {
                                apply_cdl_inverse_clamped(&mut want, slope, offset, power, sat)
                            }
                            (CdlStyle::NoClamp, _) => {
                                apply_cdl_inverse_unclamped(&mut want, slope, offset, power, sat)
                            }
                        }
                        let mut px = [[x[0], x[1], x[2], 1.0]];
                        proc.apply_rgba(&mut px);
                        for c in 0..3 {
                            // The matrix rewrite rounds differently from the per-channel form.
                            let err = (px[0][c] - want[c]).abs() / want[c].abs().max(1.0);
                            assert!(
                                err <= 1e-6,
                                "{style:?} {direction:?} {level:?} power {power:?} {x:?}: \
                                 processor {:?}, vfx_lut {want:?}",
                                &px[0][..3]
                            );
                        }
                        if style == CdlStyle::AscCdl {
                            assert!(px[0][..3].iter().all(|v| (0.0..=1.0).contains(v)));
                        }
                    }
                }
            }
        }
    }
}

/// `.cc`/`.ccc`/`.cdl` through a `FileTransform`, against PyOpenColorIO 2.5.1 on the fixtures in
/// `tests/data/cdl/` (input `[0.8, 0.8, -0.1]`): the default style does not clamp (a parsed CDL is
/// `CDL_NO_CLAMP` in OCIO), `cdl_style` overrides it, and `ccc_id` picks a correction by id, then
/// by index, or fails.
#[test]
fn file_cdl_style_and_ccc_id_follow_ocio() {
    use std::path::PathBuf;
    use vfx_ocio::{CdlStyle, FileTransform, Interpolation};
    let run = |file: &str, id: &str, style: CdlStyle| {
        let t = Transform::FileTransform(FileTransform {
            src: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/data/cdl")
                .join(file),
            ccc_id: (!id.is_empty()).then(|| id.to_string()),
            cdl_style: style,
            interpolation: Interpolation::Linear,
            direction: TransformDirection::Forward,
        });
        Processor::from_transform(&t, TransformDirection::Forward).map(|p| {
            let mut px = [[0.8, 0.8, -0.1, 1.0]];
            p.apply_rgba(&mut px);
            [px[0][0], px[0][1], px[0][2]]
        })
    };
    let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
    let slope2 = [1.6, 1.6, -0.2];
    let second = [0.5, 0.5, 0.05];
    for (file, id, want) in [
        ("slope2.cc", "", slope2),
        ("slope2.cc", "zz", slope2), // a .cc ignores ccc_id
        ("two.ccc", "", slope2),
        ("two.ccc", "a", slope2),
        ("two.ccc", "b", second),
        ("two.ccc", "1", second),
        ("two.cdl", "", slope2),
        ("two.cdl", "b", second),
        ("two.cdl", "1", second),
    ] {
        let got = run(file, id, CdlStyle::NoClamp).expect(file);
        assert!(
            close(got, want),
            "{file} id {id:?}: got {got:?}, OCIO {want:?}"
        );
    }
    for (file, id) in [
        ("two.ccc", "2"),
        ("two.ccc", "zz"),
        ("two.cdl", "2"),
        ("two.cdl", "zz"),
    ] {
        assert!(
            run(file, id, CdlStyle::NoClamp).is_err(),
            "{file} id {id:?} must fail"
        );
    }
    let asc = run("two.ccc", "a", CdlStyle::AscCdl).unwrap();
    assert!(close(asc, [1.0, 1.0, 0.0]), "ASC style clamps: {asc:?}");
}

#[test]
fn out_of_range_cdl_parameters_are_errors_as_in_ocio() {
    for (slope, power, sat) in [
        (-0.5, 1.0, 1.0),
        (1.0, 0.0, 1.0),
        (1.0, 1.0, -0.1),
        (f64::NAN, 1.0, 1.0),
    ] {
        let t = Transform::Cdl(CdlTransform {
            name: String::new(),
            slope: [slope, 1.0, 1.0],
            offset: [0.0; 3],
            power: [power, 1.0, 1.0],
            saturation: sat,
            style: CdlStyle::NoClamp,
            direction: TransformDirection::Forward,
        });
        assert!(
            Processor::from_transform(&t, TransformDirection::Forward).is_err(),
            "slope {slope} power {power} saturation {sat} must be rejected"
        );
    }
}
