//! Truelight `.cub` and CSP 3D cubes are red-fastest on disk (ROADMAP C2 finding, 2026-09-24).
//!
//! OCIO reads both with red varying fastest (`FileFormatTruelight.cpp`
//! `setArrayFromRedFastestOrder`, `FileFormatCSP.cpp` "CSP stores the LUT in red-fastest order").
//! vfx-lut read them as blue-fastest, which swapped red and blue on every `FileTransform`; identity
//! fixtures and our own write/read round-trips could not see it. The fixtures in
//! `tests/data/lut_order/` are asymmetric, and the expected values are PyOpenColorIO 2.5.1's
//! `FileTransform` (linear interpolation) on the same files.

use std::path::PathBuf;
use vfx_ocio::{CdlStyle, FileTransform, Interpolation, Processor, Transform, TransformDirection};

/// `(input, PyOpenColorIO 2.5.1 output)`, identical for both fixtures.
const ORACLE: [([f32; 3], [f32; 3]); 5] = [
    ([1.0, 0.0, 0.0], [0.7, 0.2, 0.32]),
    ([0.0, 0.0, 1.0], [0.15, 0.2, 0.7]),
    ([0.0, 1.0, 0.0], [0.1, 0.7, 0.3]),
    ([0.25, 0.5, 0.75], [0.2875, 0.45, 0.605]),
    ([0.9, 0.1, 0.4], [0.66, 0.25, 0.478]),
];

fn check(file: &str) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/lut_order")
        .join(file);
    let t = Transform::FileTransform(FileTransform {
        src,
        ccc_id: None,
        cdl_style: CdlStyle::NoClamp,
        interpolation: Interpolation::Linear,
        direction: TransformDirection::Forward,
    });
    let proc = Processor::from_transform(&t, TransformDirection::Forward).expect("processor");
    for (input, want) in ORACLE {
        let mut px = [[input[0], input[1], input[2], 1.0]];
        proc.apply_rgba(&mut px);
        for c in 0..3 {
            assert!(
                (px[0][c] - want[c]).abs() < 1e-5,
                "{file} {input:?}: got {:?}, PyOpenColorIO {want:?}",
                &px[0][..3]
            );
        }
    }
}

#[test]
fn truelight_cub_matches_pyocio() {
    check("asym.cub");
}

#[test]
fn csp_3d_matches_pyocio() {
    check("asym.csp");
}
