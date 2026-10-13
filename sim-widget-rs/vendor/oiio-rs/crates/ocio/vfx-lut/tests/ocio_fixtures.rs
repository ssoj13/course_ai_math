//! Fixture-based parity tests against OpenColorIO's own reference test suite.
//!
//! `docs/audit/tq-ocio.md` §1 item 11: the previous unit tests for these parsers asserted only
//! the first/last entry of a cube (black/white are identical under R-fastest and B-fastest
//! order, so this cannot catch a transposed axis) or used a 0.1 tolerance loose enough to hide a
//! wrong scale. These tests instead parse real fixtures copied from
//! `OpenColorIO/tests/data/files/` (into `tests/data/ocio/`, kept small: everything here is under
//! 1 MB; `spi_ocio_srgb_test.spi3d` and `lut3d_1.spi3d` were skipped for being ~1.1 MB each, see
//! the crate's ROADMAP/report) and assert interior entries against the exact values OCIO's own
//! `tests/cpu/fileformats/*_tests.cpp` assert for them, at 1e-6 (or the reference's own
//! tolerance where it is looser than that, cited per test).
//!
//! Every expected value below is either copied verbatim from an OCIO C++ test assertion (cited
//! by file:line) or read directly out of the fixture's own text by inspection (never computed by
//! calling the vfx-lut code under test).

use std::path::Path;

use vfx_lut::{cube, hdl, iridas_look, nuke_vf, pandora, spi, spi_mtx, threedl};

/// Flat index of lattice point `(r, g, b)` in a parsed 3D LUT's `data`: blue varies fastest.
fn mem_idx(size: usize, r: usize, g: usize, b: usize) -> usize {
    b + size * (g + size * r)
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/ocio")
        .join(name)
}

/// `FileFormatSpiMtx_tests.cpp:35-66` (`FileFormatSpiMtx, Test`): `camera_to_aces.spimtx`.
#[test]
fn spimtx_camera_to_aces_matches_ocio_test() {
    let mtx = spi_mtx::read_spimtx(fixture("camera_to_aces.spimtx")).expect("parse failed");

    assert!((mtx.matrix[0][0] - 0.754338638).abs() < 1e-6);
    assert!((mtx.matrix[0][1] - 0.133697046).abs() < 1e-6);
    assert!((mtx.matrix[0][2] - 0.111968437).abs() < 1e-6);

    assert!((mtx.matrix[1][0] - 0.021198141).abs() < 1e-6);
    assert!((mtx.matrix[1][1] - 1.005410934).abs() < 1e-6);
    assert!((mtx.matrix[1][2] - (-0.026610548)).abs() < 1e-6);

    assert!((mtx.matrix[2][0] - (-0.009756991)).abs() < 1e-6);
    assert!((mtx.matrix[2][1] - 0.004508563).abs() < 1e-6);
    assert!((mtx.matrix[2][2] - 1.005253201).abs() < 1e-6);

    assert_eq!(mtx.offset, [0.0, 0.0, 0.0]);
}

/// `FileFormat3DL_tests.cpp:157-182` (`FileFormat3DL, load`): `discreet-3d-lut.3dl`, a 17^3
/// 12-bit LUT. File and memory order coincide (blue-fastest), per
/// `FileFormat3DL.cpp:457-459`, so no reordering is exercised here -- this pins the bit-depth
/// inference (12-bit, scale 4095) against two INTERIOR entries, not the corners.
#[test]
fn threedl_discreet_3d_lut_matches_ocio_test() {
    let lut = threedl::read_3dl(fixture("discreet-3d-lut.3dl")).expect("parse failed");
    assert_eq!(lut.size, 17);

    let scale = 4095.0f32;
    // OCIO_CHECK_EQUAL( 54.f, scale * lutArray[41 * 3]); etc.
    let e41 = lut.data[41];
    assert!((e41[0] * scale - 54.0).abs() < 1e-3, "e41={e41:?}");
    assert!((e41[1] * scale - 323.0).abs() < 1e-3, "e41={e41:?}");
    assert!((e41[2] * scale - 597.0).abs() < 1e-3, "e41={e41:?}");

    let e4591 = lut.data[4591];
    assert!((e4591[0] * scale - 4025.0).abs() < 1e-3, "e4591={e4591:?}");
    assert!((e4591[1] * scale - 3426.0).abs() < 1e-3, "e4591={e4591:?}");
    assert!((e4591[2] * scale - 0.0).abs() < 1e-3, "e4591={e4591:?}");
}

/// `FileFormatSpi1D_tests.cpp:32-55` (`FileFormatSpi1D, test`): `cpf.spi1d`, a mono, 2048-entry,
/// F32 LUT.
#[test]
fn spi1d_cpf_matches_ocio_test() {
    let lut = spi::read_spi1d(&fixture("cpf.spi1d")).expect("parse failed");
    assert_eq!(lut.size(), 2048);
    assert!(lut.is_mono());

    assert_eq!(lut.r[0], 0.0f32);
    // Interior entry (not black/white): OCIO_CHECK_EQUAL(4.511920005404118f, lutArray[1970*3]);
    assert!((lut.r[1970] - 4.511_92_f32).abs() < 1e-6);
}

/// `FileFormatSpi3D_tests.cpp` doesn't assert `lut3d_bizarre.spi3d` by name (it isn't loaded by
/// any OCIO CPU unit test); the expected values below are copied verbatim from the fixture's own
/// text (`tests/data/ocio/lut3d_bizarre.spi3d`), not computed by this crate's parser -- this
/// pins the SPI3D index-addressed (not file-order) placement (ROADMAP COLOR-01) on a real,
/// non-synthetic file with a non-trivial index order.
#[test]
fn spi3d_bizarre_interior_entries_match_file_text() {
    let lut = spi::read_spi3d(&fixture("lut3d_bizarre.spi3d")).expect("parse failed");
    assert_eq!(lut.size, 3);

    // File line "0 0 1  -0.009775171  0.048875855  0.391006843" -> index (r=0,g=0,b=1).
    let e001 = lut.data[1]; // blue-fastest: b + size*(g + size*r) = 1 + 3*(0+3*0) = 1
    assert!((e001[0] - (-0.009775171)).abs() < 1e-6, "e001={e001:?}");
    assert!((e001[1] - 0.048875855).abs() < 1e-6, "e001={e001:?}");
    assert!((e001[2] - 0.391_006_86).abs() < 1e-6, "e001={e001:?}");

    // File line "1 1 1  0.3714565  0.391006843  0.391006843" -> index (r=1,g=1,b=1).
    let e111 = lut.data[1 + 3 * (1 + 3)]; // = 13
    assert!((e111[0] - 0.3714565).abs() < 1e-6, "e111={e111:?}");
    assert!((e111[1] - 0.391_006_86).abs() < 1e-6, "e111={e111:?}");
    assert!((e111[2] - 0.391_006_86).abs() < 1e-6, "e111={e111:?}");
}

/// `FileFormatIridasCube_tests.cpp:258-323` (`FileFormatIridasCube, load_1d_op`): `iridas_1d.cube`.
/// OCIO's `Lut1DOpData` array holds the raw file values (a separate `MatrixOffsetOp` maps the
/// domain); this crate's `Lut1D` keeps its own `domain_min`/`domain_max` the same way, so the
/// per-channel data is the raw file rows, unnormalized.
#[test]
fn cube_iridas_1d_matches_ocio_test() {
    let lut = cube::read_1d(fixture("iridas_1d.cube")).expect("parse failed");
    assert_eq!(lut.size(), 5);
    assert_eq!(lut.domain_min, [-2.0, 1.0, 0.0]);
    assert_eq!(lut.domain_max, [2.0, 2.0, 1.0]);

    let g = lut.g.as_ref().expect("RGB lut has g channel");
    let b = lut.b.as_ref().expect("RGB lut has b channel");

    // Row 0: -1.0 -2.0 -3.0
    assert_eq!((lut.r[0], g[0], b[0]), (-1.0, -2.0, -3.0));
    // Row 2 (interior): 0.4 0.5 0.6
    assert_eq!((lut.r[2], g[2], b[2]), (0.4, 0.5, 0.6));
    // Row 4: 1.0 2.1 3.2
    assert_eq!((lut.r[4], g[4], b[4]), (1.0, 2.1, 3.2));
}

/// `FileFormatIridasCube_tests.cpp:325-397` (`FileFormatIridasCube, load_3d_op`): `iridas_3d.cube`,
/// a 2^3 cube with an asymmetric domain (catches a transposed axis; corners alone would not,
/// per `cube.rs`'s `roundtrip_3d_asymmetric_detects_axis_swap`).
#[test]
fn cube_iridas_3d_matches_ocio_test() {
    let lut = cube::read_3d(fixture("iridas_3d.cube")).expect("parse failed");
    assert_eq!(lut.size, 2);
    assert_eq!(lut.domain_min, [0.0, 1.0, 0.0]);
    assert_eq!(lut.domain_max, [2.0, 2.0, 1.0]);

    // lutArray[3..6] == (0,0,2) -> (r=0,g=0,b=1); lutArray[6..9] == (0,2,0) -> (r=0,g=1,b=0).
    let size = lut.size;
    let idx001 = mem_idx(size, 0, 0, 1);
    assert_eq!(lut.data[idx001], [0.0, 0.0, 2.0]);
    let idx010 = mem_idx(size, 0, 1, 0);
    assert_eq!(lut.data[idx010], [0.0, 2.0, 0.0]);
    // lutArray[21..24] == (2,2,2) -> (r=1,g=1,b=1), the white corner.
    let idx111 = mem_idx(size, 1, 1, 1);
    assert_eq!(lut.data[idx111], [2.0, 2.0, 2.0]);
}

/// `resolve_1d3d.cube` isn't pinned to explicit values by any OCIO unit test (`FileFormatResolveCube,
/// read_1d_3d` only checks that a similarly-shaped inline sample parses without throwing); this
/// exercises the Resolve `LUT_1D_INPUT_RANGE`/`LUT_3D_INPUT_RANGE` extension on the real fixture
/// and cross-checks an interior 3D entry against the file's own text via the documented
/// R-fastest -> B-fastest reorder (the same formula `cube.rs`'s `roundtrip_3d_asymmetric_detects_axis_swap`
/// pins), read independently from the fixture, not from the parser under test.
#[test]
fn cube_resolve_1d3d_parses_and_reorders_correctly() {
    let file = cube::read_cube(fixture("resolve_1d3d.cube")).expect("parse failed");

    let lut1d = file.lut1d.expect("file has LUT_1D_SIZE");
    assert_eq!(lut1d.size(), 6);
    assert_eq!(lut1d.domain_min, [-1.0, -1.0, -1.0]);
    assert_eq!(lut1d.domain_max, [3.0, 3.0, 3.0]);
    // First 1D row: "3.3 3.4 3.5".
    let g1 = lut1d.g.as_ref().unwrap();
    let b1 = lut1d.b.as_ref().unwrap();
    assert_eq!((lut1d.r[0], g1[0], b1[0]), (3.3, 3.4, 3.5));

    let lut3d = file.lut3d.expect("file has LUT_3D_SIZE");
    assert_eq!(lut3d.size, 3);
    assert_eq!(lut3d.domain_min, [0.0, 0.0, 0.0]);
    assert_eq!(lut3d.domain_max, [4.0, 4.0, 4.0]);

    // File's 3D block (after the 6 shaper rows), R-fastest: 0-based row 1 is "0.5 1.0 1.0" ->
    // file_idx = 1 = r + g*3 + b*9 => (r=1,g=0,b=0).
    let size = lut3d.size;
    let mem_idx = size * size; // (r=1,g=0,b=0), blue-fastest
    assert_eq!(lut3d.data[mem_idx], [0.5, 1.0, 1.0]);

    // Last 3D data row is the file's last line "0.0 0.0 0.0" (r=2,g=2,b=2), the identity corner.
    let mem_last = 2 + size * (2 + size * 2);
    assert_eq!(lut3d.data[mem_last], [0.0, 0.0, 0.0]);
}

/// `FileFormatHDL_tests.cpp:349-426` (`FileFormatHDL, read_3d`): the exact inline fixture from
/// that test (no standalone `.hdl` file ships in `tests/data/files`), asserted the same way OCIO
/// does -- element-wise over every grid point via the file's Red-fastest / memory's Blue-fastest
/// index formula, not just the corners.
#[test]
fn hdl_read_3d_matches_ocio_test() {
    let content = "Version         2\n\
Format      any\n\
Type        3D\n\
From        0.2 0.9\n\
To      0.001 0.999\n\
Black       0.002\n\
White       0.98\n\
Length      2\n\
LUT:\n\
 {\n\
 0 0 0\n\
 0 0 0\n\
 0 0.390735 2.68116e-28\n\
 0 0.390735 0\n\
 0 0 0\n\
 0 0 0.599397\n\
 0 0.601016 0\n\
 0 0.601016 0.917034\n\
 }\n";

    let file = hdl::parse_hdl(content.as_bytes()).expect("parse failed");
    let lut = file.lut3d.expect("Type 3D has a lut3d");
    assert_eq!(lut.size, 2);

    // OCIO's inline `cube[]`, Houdini order (Red changes fastest), same order as the file.
    let cube: [[f32; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.390735, 2.68116e-28],
        [0.0, 0.390735, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 0.599397],
        [0.0, 0.601016, 0.0],
        [0.0, 0.601016, 0.917034],
    ];

    let size = lut.size;
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                // Memory (OpData::Lut3D): blue changes fastest.
                let array_idx = b + size * (g + size * r);
                // File / Houdini order: red changes fastest.
                let ocio_idx = r + size * (g + size * b);
                let got = lut.data[array_idx];
                let want = cube[ocio_idx];
                assert!(
                    (got[0] - want[0]).abs() < 1e-6
                        && (got[1] - want[1]).abs() < 1e-6
                        && (got[2] - want[2]).abs() < 1e-6,
                    "(r={r},g={g},b={b}): got {got:?}, want {want:?}"
                );
            }
        }
    }
}

/// `FileFormatPandora_tests.cpp:149-193` (`FileFormatPandora, load_op`): `pandora_3d.m3d`. This
/// fixture is also what exposed the COLOR-08 colon-parsing bug fixed in `pandora.rs` (the real
/// format has no colon after the keyword; the pre-fix parser silently dropped every line).
#[test]
fn pandora_3d_m3d_matches_ocio_test() {
    let lut = pandora::read_mga(fixture("pandora_3d.m3d")).expect("parse failed");
    assert_eq!(lut.size, 2);
    assert_eq!(lut.data.len(), 8);

    let err = 1e-6f32;
    // lutArray[3,4,5] == (0, 0, 0.8) -- file row 1 "1 0 0 204", scale = 1/255.
    let e1 = lut.data[1];
    assert!((e1[0] - 0.0).abs() < err);
    assert!((e1[1] - 0.0).abs() < err);
    assert!((e1[2] - 0.8).abs() < err);

    // lutArray[21,22,23] == (1.2, 1.0, 1.2) -- file row 7 "7 306 255 306".
    let e7 = lut.data[7];
    assert!((e7[0] - 1.2).abs() < err);
    assert!((e7[1] - 1.0).abs() < err);
    assert!((e7[2] - 1.2).abs() < err);
}

/// `FileFormatVF_tests.cpp:86-153` (`FileFormatVF, load_ops`): `nuke_3d.vf`. Also pins the
/// `global_transform` pre-scale-by-grid-size undo (`FileFormatVF.cpp:236-251`).
#[test]
fn nuke_vf_matches_ocio_test() {
    let file = nuke_vf::read_vf(fixture("nuke_3d.vf")).expect("parse failed");
    assert_eq!(file.lut.size, 2);

    let m = file.matrix.expect("file has a global_transform");
    // File's global_transform is identity; OCIO multiplies the linear 3x3 part by grid_size (2).
    assert!((m[0] - 2.0).abs() < 1e-6);
    assert!((m[5] - 2.0).abs() < 1e-6);
    assert!((m[10] - 2.0).abs() < 1e-6);
    assert!((m[15] - 1.0).abs() < 1e-6);
    for &off in &[m[3], m[7], m[11], m[12], m[13], m[14]] {
        assert!(off.abs() < 1e-6);
    }

    // lutArray is raw file data, unnormalized, blue-fastest (no reorder: OCIO's own comment
    // "LUT in file is blue fastest."). Interior entry 1 = file row 1 "0 0 2".
    assert_eq!(file.lut.data[1], [0.0, 0.0, 2.0]);
    // Entry 4 = file row 4 "2 0 0".
    assert_eq!(file.lut.data[4], [2.0, 0.0, 0.0]);
    // Last entry = file row 7 "2 2 2".
    assert_eq!(file.lut.data[7], [2.0, 2.0, 2.0]);
}

/// `FileFormatIridasLook_tests.cpp:108-891` (`FileFormatIridasLook, simple3d`): the exact inline
/// XML/hex fixture from that test, copied byte-for-byte into `tests/data/ocio/
/// iridas_look_simple3d.look` (no standalone `.look` file ships in `tests/data/files`). The
/// expected values are the reference's own decimal `cube[]` oracle array
/// (`FileFormatIridasLook_tests.cpp:347-859`), picked at three axis-only interior probes plus the
/// white corner so a transposed R/B axis or a wrong `lut_size` scale would fail.
#[test]
fn iridas_look_simple3d_matches_ocio_test() {
    let lut = iridas_look::read_look(fixture("iridas_look_simple3d.look")).expect("parse failed");
    assert_eq!(lut.size, 8);

    let size = lut.size;
    let tol = 1e-4f32; // OCIO_CHECK_CLOSE(..., 1e-4) at FileFormatIridasLook_tests.cpp:876-878.

    // cube[] flat index 1 (r=1,g=0,b=0), Red-fastest file order -> 0.04271.
    let idx_r1 = mem_idx(size, 1, 0, 0);
    let got = lut.data[idx_r1];
    assert!(got.iter().all(|&c| (c - 0.04271).abs() < tol), "{got:?}");

    // cube[] flat index 8 (r=0,g=1,b=0) -> 0.08386.
    let idx_g1 = mem_idx(size, 0, 1, 0);
    let got = lut.data[idx_g1];
    assert!(got.iter().all(|&c| (c - 0.08386).abs() < tol), "{got:?}");

    // cube[] flat index 64 (r=0,g=0,b=1) -> 0.01629.
    let idx_b1 = mem_idx(size, 0, 0, 1);
    let got = lut.data[idx_b1];
    assert!(got.iter().all(|&c| (c - 0.01629).abs() < tol), "{got:?}");

    // cube[] flat index 511 (r=7,g=7,b=7), the white corner -> 1.0.
    let idx_last = mem_idx(size, 7, 7, 7);
    let got = lut.data[idx_last];
    assert!(got.iter().all(|&c| (c - 1.0).abs() < tol), "{got:?}");
}
