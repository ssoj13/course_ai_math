//! Red-gate tests pinning two matrix-convention bugs:
//!
//! - **OCIO-M6** (`crates/ocio/vfx-ocio/src/gpu/glsl.rs:93`,
//!   `crates/ocio/vfx-ocio/src/gpu/hlsl.rs:129`): the GLSL and HLSL shader
//!   emitters transpose the colour matrix. Metal, WGSL, and the CPU path
//!   agree and are correct.
//! - **OCIO-M13** (`crates/ocio/vfx-ocio/src/builtin_transforms.rs:2158-2160`):
//!   `bradford_adapt` feeds the Bradford cone-response matrix's ROWS to
//!   `glam::DMat3::from_cols_array` (a COLUMNS reader), so it silently uses
//!   `M^T` instead of `M`.
//!
//! Both are fixed and these are their regression tests: do not weaken the
//! assertions and do not `#[ignore]` anything in this file. OCIO-M6 went away
//! with the matrix written as one dot product per row for every language
//! (`gpu/ops.rs::matrix`, ROADMAP G6); the derivation below describes the old
//! emitters.
//!
//! # Why the existing test coverage missed both bugs
//!
//! `crates/ocio/vfx-ocio/src/gpu/tests.rs::test_hlsl_matrix_op` /
//! `test_metal_matrix_op` only assert that the emitted shader TEXT contains
//! certain substrings (e.g. `"float3x3 m = float3x3("`,
//! `"mul(color.rgb, m)"`) and they feed an IDENTITY matrix
//! (`gpu/tests.rs:130-133`). An identity matrix is its own transpose, so no
//! substring check and no identity-matrix numeric probe can ever notice a
//! row/column-major swap.
//!
//! OCIO-M13 has the analogous blind spot: every legitimate chromatic
//! adaptation matrix maps white to white by construction (the CAT
//! algorithm's diagonal-scaling step is defined to send `src_white ->
//! dst_white`), and the buggy transposed Bradford matrix does too -- see
//! `red_gate_bradford_neutral_control_OCIO_M13` below, a CONTROL that is
//! expected to pass both before and after the fix. A neutral-grey test can
//! never catch this bug.
//!
//! Every non-control test in this file therefore uses a matrix that is
//! neither symmetric nor identity, and a colour that is not neutral.
//!
//! # OCIO-M6 derivation
//!
//! `gpu/glsl.rs:91-118` builds `mat3(row0, row1, row2)` where `row0..row2`
//! are read straight off the row-major `matrix: [f32;16]` operand
//! (`matrix[0..3]`, `matrix[4..7]`, `matrix[8..11]` -- see the "4x4 matrix
//! stored row-major" comment at `glsl.rs:92`, and the CPU reference at
//! `processor/apply.rs:395-401`, `out[i] = sum_j in[j]*matrix[i*4+j]`, which
//! is unambiguously `M*v` with `matrix[0..3]` as row 0). GLSL's
//! `mat3(c0,c1,c2)` constructor reads its three `vec3` arguments as
//! COLUMNS, so passing the matrix's rows there builds `M^T`, not `M`.
//!
//! `gpu/hlsl.rs:125-151` builds `float3x3(row0,row1,row2)` from the same
//! row extraction, then applies it with `mul(color.rgb, m)`
//! (`hlsl.rs:146`). HLSL's `float3x3(v0,v1,v2)` constructor genuinely does
//! read its arguments as ROWS (so `m` there does equal `M`), but
//! `mul(vector, matrix)` is HLSL's row-vector-times-matrix operator
//! (`v^T * M`), which as a result vector equals `M^T * v`. Two different
//! mistakes, same wrong answer -- see `apply_triples_as_columns` below for
//! the algebra showing why both land on the identical formula.
//!
//! `gpu/metal.rs:196-221` and `gpu/wgsl.rs:3026-3051` instead extract the
//! matrix's COLUMNS (`matrix[0], matrix[4], matrix[8]`, ...) and hand them
//! to `float3x3(c0,c1,c2)` / `mat3x3<f32>(c0,c1,c2)`, both genuinely
//! column-major constructors used with `m * v`, so they compute the
//! correct `M*v`, matching CPU.
//!
//! # OCIO-M13 derivation
//!
//! `builtin_transforms.rs:2156-2189` (`bradford_adapt`) calls
//! `glam::DMat3::from_cols_array(&[0.8951, 0.2664, -0.1614, -0.7502,
//! 1.7135, 0.0367, 0.0389, -0.0685, 1.0296])`. Those 9 numbers are the
//! textbook Bradford cone-response matrix's ROWS (Lam 1985 / OCIO
//! `ColorMatrixHelpers.cpp:233-237`):
//! ```text
//!  0.8951   0.2664  -0.1614
//! -0.7502   1.7135   0.0367
//!  0.0389  -0.0685   1.0296
//! ```
//! `from_cols_array` reads a flat 9-element slice as three COLUMNS, so the
//! `Ma` actually built inside `bradford_adapt` is the TRANSPOSE of the
//! matrix above. The sibling `cat02_adapt` (`builtin_transforms.rs:2247-`
//! `2255`) runs the identical CAT algorithm but supplies its cone matrix's
//! columns explicitly (with a comment: "glam from_cols_array wants
//! COLUMN-major, so supply the columns (OCIO applies M*XYZ, not
//! M^T*XYZ)") -- proof the correct pattern was known elsewhere in the same
//! file and simply not carried over to `bradford_adapt`.
//!
//! (File/line references in this section describe the code as it was when OCIO-M13 was found;
//! `builtin_transforms.rs` has since been replaced by the `builtin_transforms/` port of OCIO's
//! registry, whose one Bradford/CAT02 builder is `color_matrix::build_vonkries_adapt`.)
//!
//! The builder is private, so this test drives it through the public door that reaches it:
//! `Transform::Builtin(BuiltinTransform { style: "UTILITY - ACES-AP1_to_CIE-XYZ-D65_BFD", .. })`,
//! OCIO's AP1 -> CIE-XYZ-D65 (Bradford) builtin, which is a single matrix op (`ACES.cpp`
//! `AP1_to_CIE_XYZ_D65::GenerateOps`). Nothing else touches the pixel. This used to be spelled
//! `"ACEScg_to_XYZ-D65"`, a name OCIO does not have that the old separator-stripping lookup
//! happened to map onto the same matrix; OCIO rejects it, and so does this crate now.
//!
//! The "expected correct" golden below is derived independently of the
//! buggy function, in f64, via:
//! 1. The standard Bruce-Lindbloom chromaticity -> RGB2XYZ ("NPM") algorithm
//!    -- the same algorithm `rgb_to_xyz` (`builtin_transforms.rs:2112-`
//!    `2153`) implements correctly (not part of this bug) -- applied to the
//!    AP1 (ACEScg) primaries vfx-ocio itself hard-codes at
//!    `builtin_transforms.rs:2309-2314`: R(0.713,0.293), G(0.165,0.830),
//!    B(0.128,0.044), white D60(0.32168,0.33767).
//! 2. A sanity cross-check: this reimplementation's white-point row-sum
//!    reproduces the crate's own `D60_XYZ` constant
//!    (`builtin_transforms.rs:2337`, `[0.95264607456985, 1.0,`
//!    `1.00882518435159]`) to 8 significant figures, confirming step 1 is
//!    faithful to what the real (non-buggy) code path also computes.
//! 3. The classic Bradford CAT sandwich, `Ma^-1 * diag(dst_LMS/src_LMS) *`
//!    `Ma`, run with the 9 Bradford numbers above kept as ROWS (correct)
//!    from AP1's white to `D65_XYZ` (`builtin_transforms.rs:2338`,
//!    `[0.95045592705167, 1.0, 1.08905775075988]`).
//! 4. `AP1_TO_XYZ_D65_correct = bfd_correct * ap1_xyz`, matching the
//!    composition order at `builtin_transforms.rs:2023-2024`
//!    (`mat4_mul(&bfd, &ap1_xyz)`).
//!
//! **This derivation is performed in-repo, at test time** — see
//! `ap1_to_xyz_d65_correct` below. It was originally computed by an offline
//! NumPy script and transcribed as literals; that was replaced because a
//! golden whose generator lives outside the repository cannot be re-derived
//! or re-checked by the next reader, which is the same rot this red gate
//! exists to stop. `control_bradford_derivation_reproduces_d60_whitepoint`
//! asserts step 2 rather than merely asserting it in prose.
//!
//! The values the derivation produces, recorded here for orientation only —
//! the test does not read them from this comment:
//! ```text
//! ap1_xyz (AP1->XYZ D60, correct) =
//!   [ 0.66245418  0.13400421  0.15618769]
//!   [ 0.27222872  0.67408177  0.05368952]
//!   [-0.00557465  0.00406073  1.0103391 ]
//! src_wht = [0.95264607, 1.0, 1.00882518]   (== D60_XYZ to 8 s.f., sanity OK)
//!
//! bfd_correct (D60->D65 Bradford CAT, rows kept as rows) =
//!   [ 0.98722401 -0.00611323  0.01595329]
//!   [-0.00759837  1.00186148  0.00533004]
//!   [ 0.00307258 -0.00509596  1.0816806 ]
//!
//! AP1_TO_XYZ_D65_correct = bfd_correct @ ap1_xyz =
//!   [ 0.65223754  0.12823614  0.16998225]
//!   [ 0.26767218  0.67433999  0.05798783]
//!   [-0.00538182  0.00136906  1.09307051]
//!
//! v = [0.9, 0.05, 0.05] (saturated, non-neutral AP1 colour)
//! out_correct = AP1_TO_XYZ_D65_correct @ v = [0.60192471, 0.27752135, 0.04987834]
//!
//! vn = [0.5, 0.5, 0.5] (neutral CONTROL)
//! outn_correct = AP1_TO_XYZ_D65_correct @ vn = [0.47522796, 0.5, 0.54452888]
//! ```

#![allow(non_snake_case)]

use vfx_ocio::{
    BuiltinTransform, GpuLanguage, GpuProcessor, Processor, Transform, TransformDirection,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ============================================================================
// OCIO-M6: GLSL/HLSL vs Metal/WGSL/CPU matrix convention
// ============================================================================

/// Row-major 4x4 test matrix. The RGB 3x3 submatrix is
/// `[[1,2,3],[4,5,6],[7,8,10]]` -- neither symmetric nor identity, so
/// swapping its rows and columns changes the result (unlike the identity
/// matrix `gpu/tests.rs` uses).
const TEST_MATRIX: [f64; 16] = [
    1.0, 2.0, 3.0, 0.0, //
    4.0, 5.0, 6.0, 0.0, //
    7.0, 8.0, 10.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// Non-neutral RGB probe colour (components all distinct).
const TEST_V: [f32; 3] = [1.0, 2.0, 4.0];

/// `M*v` by hand: row0=(1,2,3)*v=1+4+12=17; row1=(4,5,6)*v=4+10+24=38;
/// row2=(7,8,10)*v=7+16+40=63. This is what CPU, Metal, and WGSL must
/// compute.
const EXPECTED_CORRECT: [f64; 3] = [17.0, 38.0, 63.0];

/// `M^T*v` by hand: row0=(1,4,7)*v=1+8+28=37; row1=(2,5,8)*v=2+10+32=44;
/// row2=(3,6,10)*v=3+12+40=55. This is what OCIO-M6 predicts GLSL and HLSL
/// actually compute.
const EXPECTED_TRANSPOSED_BUG: [f64; 3] = [37.0, 44.0, 55.0];

const EPS: f64 = 1e-3;

fn assert_close(actual: [f64; 3], expected: [f64; 3], label: &str) {
    for i in 0..3 {
        let diff = (actual[i] - expected[i]).abs();
        assert!(
            diff < EPS,
            "{label}: component {i} mismatch -- actual={actual:?} expected={expected:?} diff={diff:.6} (eps={EPS})"
        );
    }
}

/// Builds a one-op `Processor` for `TEST_MATRIX` and returns the CPU result
/// of applying it to `TEST_V`. This is the trusted reference: `M*v` per
/// `processor/apply.rs:395-401`.
fn cpu_reference() -> Result<[f64; 3], Box<dyn std::error::Error>> {
    let transform = Transform::matrix(TEST_MATRIX);
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)?;
    let mut pixels = [TEST_V];
    processor.apply_rgb(&mut pixels);
    Ok([
        pixels[0][0] as f64,
        pixels[0][1] as f64,
        pixels[0][2] as f64,
    ])
}

/// Generates shader source for `TEST_MATRIX` in the given language.
fn gpu_shader_for(language: GpuLanguage) -> Result<String, Box<dyn std::error::Error>> {
    let transform = Transform::matrix(TEST_MATRIX);
    let processor = Processor::from_transform(&transform, TransformDirection::Forward)?;
    let gpu = GpuProcessor::from_processor(&processor)?;
    Ok(gpu
        .generate_shader(language)
        .unwrap()
        .fragment_code()
        .to_string())
}

/// Extracts every floating-point literal appearing between the first
/// occurrence of `start` and the following occurrence of `end` in `text`.
/// Numbers are emitted with a fixed 8-decimal format (`{:.8}`) and no
/// exponent, so a simple digit/`.`/leading-`-` scanner is sufficient --
/// no need to pull in a regex for this.
fn floats_between(text: &str, start: &str, end: &str) -> Vec<f64> {
    let start_idx = text
        .find(start)
        .unwrap_or_else(|| panic!("marker {start:?} not found in shader:\n{text}"));
    let after_start = &text[start_idx + start.len()..];
    let end_idx = after_start
        .find(end)
        .unwrap_or_else(|| panic!("marker {end:?} not found after {start:?} in shader:\n{text}"));
    let block = &after_start[..end_idx];

    // Small state machine over 3 states, rather than a per-character regex,
    // because a numeric literal like the trailing "3" in `float3(` or the
    // "32" in `vec3<f32>(` (Metal/WGSL wrap each matrix row in one of those
    // constructors) must NOT be mistaken for a standalone number: once any
    // letter is seen, every alphanumeric character glued to it (including
    // digits) belongs to that identifier until the next non-alnum
    // delimiter, full stop -- checking only the single previous character
    // (an earlier, buggier version of this parser) is not enough, because
    // an identifier can contain a run of digits following its own digits
    // (`f32`'s '3' then '2').
    #[derive(Clone, Copy, PartialEq)]
    enum St {
        Idle,
        Ident,
        Num,
    }

    let mut out = Vec::new();
    let mut cur = String::new();
    let mut state = St::Idle;
    for c in block.chars() {
        let is_alnum = c.is_ascii_alphanumeric() || c == '_';
        match state {
            St::Idle => {
                if c.is_ascii_digit() || c == '-' {
                    cur.push(c);
                    state = St::Num;
                } else if c.is_ascii_alphabetic() || c == '_' {
                    state = St::Ident;
                }
            }
            St::Num => {
                if c.is_ascii_digit() || c == '.' {
                    cur.push(c);
                } else if c.is_ascii_alphabetic() || c == '_' {
                    // Digits glued to a following letter -- not a matrix
                    // literal (shouldn't happen for this shader's fixed
                    // `-?\d+\.\d{8}` formatting, but don't misparse if it
                    // ever does): discard and treat as an identifier.
                    cur.clear();
                    state = St::Ident;
                } else {
                    if let Ok(v) = cur.parse::<f64>() {
                        out.push(v);
                    }
                    cur.clear();
                    state = St::Idle;
                }
            }
            St::Ident => {
                if !is_alnum {
                    state = St::Idle;
                }
            }
        }
    }
    if state == St::Num
        && !cur.is_empty()
        && let Ok(v) = cur.parse::<f64>()
    {
        out.push(v);
    }
    out
}

/// The four rows of the emitted matrix: `gpu/ops.rs::matrix` writes the product as one
/// `dot(<row literal>, px)` per output channel, so the literals ARE the rows, read in emission
/// order, in every language. Panics (a harness bug) unless there are exactly four rows of four.
fn extract_rows(text: &str) -> [[f64; 4]; 4] {
    let block_start = text
        .find("{ // Matrix")
        .unwrap_or_else(|| panic!("no Matrix block in shader:\n{text}"));
    let block = &text[block_start..];
    let block = &block[..block.find("\n    }").or_else(|| block.find("\n}")).unwrap()];
    let mut rows = Vec::new();
    let mut rest = block;
    while let Some(i) = rest.find("dot(") {
        let after = &rest[i + 4..];
        let end = after
            .find(", px)")
            .unwrap_or_else(|| panic!("unterminated dot in:\n{block}"));
        let nums = floats_between(&after[..end], "(", ")");
        rows.push(nums);
        rest = &after[end..];
    }
    assert_eq!(rows.len(), 4, "expected 4 dot rows in:\n{block}");
    let mut out = [[0.0; 4]; 4];
    for (r, nums) in rows.iter().enumerate() {
        assert_eq!(
            nums.len(),
            4,
            "row {r} is not 4 literals: {nums:?} in:\n{block}"
        );
        out[r].copy_from_slice(nums);
    }
    out
}

/// `rows * (v, 1)`, the RGB part: what the emitted shader computes for `TEST_V`.
fn apply_rows(rows: [[f64; 4]; 4], v: [f32; 3]) -> [f64; 3] {
    let v4 = [v[0] as f64, v[1] as f64, v[2] as f64, 1.0];
    std::array::from_fn(|r| (0..4).map(|c| rows[r][c] * v4[c]).sum())
}

fn check_language(language: GpuLanguage, label: &str) -> TestResult {
    let code = gpu_shader_for(language)?;
    let rows = extract_rows(&code);
    let got = apply_rows(rows, TEST_V);
    let cpu = cpu_reference()?;
    assert_close(
        got,
        cpu,
        &format!(
            "{label} vs CPU (OCIO-M6); rows={rows:?}; the transposed answer would be {EXPECTED_TRANSPOSED_BUG:?}"
        ),
    );
    Ok(())
}

/// OCIO-M6 (fixed): GLSL computes `M*v`. It used to pass the matrix's rows to the column-major
/// `mat3(...)`, computing `M^T*v`; the product is now one dot product per row
/// (`gpu/ops.rs::matrix`, as OCIO `MatrixOpGPU.cpp` computes it), which has no convention to get
/// wrong.
#[test]
fn glsl_matrix_is_m_times_v_OCIO_M6() -> TestResult {
    check_language(GpuLanguage::Glsl330, "GLSL")
}

/// OCIO-M6 (fixed): HLSL computes `M*v`. It used to apply its row-built `float3x3` with the
/// row-vector product `mul(v, m)`, `M^T*v`.
#[test]
fn hlsl_matrix_is_m_times_v_OCIO_M6() -> TestResult {
    check_language(GpuLanguage::Hlsl50, "HLSL")
}

/// Metal computes `M*v`.
#[test]
fn metal_matrix_is_m_times_v_OCIO_M6() -> TestResult {
    check_language(GpuLanguage::Metal, "Metal")
}

/// WGSL computes `M*v`.
#[test]
fn wgsl_matrix_is_m_times_v_OCIO_M6() -> TestResult {
    check_language(GpuLanguage::Wgsl, "WGSL")
}

/// All four languages agree with the CPU (`EXPECTED_CORRECT` by hand).
#[test]
fn all_backends_agree_with_cpu_OCIO_M6() -> TestResult {
    let cpu = cpu_reference()?;
    assert_close(cpu, EXPECTED_CORRECT, "CPU vs hand-derived M*v");
    for (language, label) in [
        (GpuLanguage::Glsl330, "GLSL"),
        (GpuLanguage::Hlsl50, "HLSL"),
        (GpuLanguage::Metal, "Metal"),
        (GpuLanguage::Wgsl, "WGSL"),
    ] {
        check_language(language, label)?;
    }
    Ok(())
}

// ============================================================================
// OCIO-M13: bradford_adapt transposes the Bradford cone matrix
// ============================================================================

/// Saturated (non-neutral) AP1 (ACEScg) test colour.
const BRADFORD_SATURATED_V: [f32; 3] = [0.9, 0.05, 0.05];

/// Neutral CONTROL colour -- see module doc comment for why this cannot
/// detect OCIO-M13.
const BRADFORD_NEUTRAL_V: [f32; 3] = [0.5, 0.5, 0.5];

// ---------------------------------------------------------------------------
// In-repo derivation of the correct AP1->XYZ(D65) matrix.
//
// The expected values below are NOT literals. They are recomputed at test time
// from two first-party inputs, so no number in this file depends on a script
// that lives outside the repository. A golden whose origin is not in the tree
// is a literal waiting to rot -- the exact defect class this red gate exists
// to repair -- so the derivation is code, not a comment.
//
// Inputs:
//   * AP1 (ACEScg) chromaticities, transcribed from PRIMS_AP1 at
//     `builtin_transforms.rs:2308-2313`.
//   * The Bradford cone-response matrix, transcribed ROW BY ROW from
//     `CONE_RESP_MAT_BRADFORD`, OCIO
//     `src/OpenColorIO/transforms/builtins/ColorMatrixHelpers.cpp:232-236`.
//     That array is row-major in the reference. `bradford_adapt`
//     (`builtin_transforms.rs:2156-2159`) hands exactly these nine numbers to
//     `glam::DMat3::from_cols_array`, which reads them as COLUMNS -- so the
//     shipped code builds Ma-transpose. That single misreading is OCIO-M13.
//
// Everything here is plain f64 array arithmetic: no glam, and no call into the
// code under test. Using the same matrix type as the defect would risk
// inheriting the very convention being checked.
// ---------------------------------------------------------------------------

type M3 = [[f64; 3]; 3];

/// AP1 (ACEScg) chromaticities: R, G, B, white (D60).
/// Source: `PRIMS_AP1`, `builtin_transforms.rs:2308-2313`.
const AP1_CHROMS: [[f64; 2]; 4] = [
    [0.713, 0.293],
    [0.165, 0.830],
    [0.128, 0.044],
    [0.32168, 0.33767],
];

/// Bradford cone-response matrix, kept as ROWS -- which is what it is in the
/// reference. Source: `ColorMatrixHelpers.cpp:233-235`.
const BRADFORD_MA: M3 = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];

/// D65 white in XYZ. Source: `D65_XYZ`, `builtin_transforms.rs:2338`.
const D65_XYZ: [f64; 3] = [0.95045592705167, 1.0, 1.08905775075988];

/// D60 white in XYZ, used only as an independent cross-check of the NPM
/// derivation below. Source: `D60_XYZ`, `builtin_transforms.rs:2337`.
const D60_XYZ: [f64; 3] = [0.95264607456985, 1.0, 1.00882518435159];

fn mat_mul(a: &M3, b: &M3) -> M3 {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

fn mat_vec(m: &M3, v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// Cofactor inverse. Returns `None` on a singular matrix rather than producing
/// infinities that would surface later as a confusing assertion failure.
fn mat_inv(m: &M3) -> Option<M3> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let mut inv = [[0.0; 3]; 3];
    for (i, inv_row) in inv.iter_mut().enumerate() {
        for (j, inv_ij) in inv_row.iter_mut().enumerate() {
            // Cofactor of (j,i) gives the adjugate entry (i,j) -- the transpose
            // is what turns the cofactor matrix into the inverse.
            let (r0, r1) = ((j + 1) % 3, (j + 2) % 3);
            let (c0, c1) = ((i + 1) % 3, (i + 2) % 3);
            *inv_ij = (m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]) / det;
        }
    }
    Some(inv)
}

fn diag(v: [f64; 3]) -> M3 {
    [[v[0], 0.0, 0.0], [0.0, v[1], 0.0], [0.0, 0.0, v[2]]]
}

/// Chromaticity (x,y) to XYZ at unit luminance.
fn xy_to_xyz(xy: [f64; 2]) -> [f64; 3] {
    [xy[0] / xy[1], 1.0, (1.0 - xy[0] - xy[1]) / xy[1]]
}

/// Builds the RGB->XYZ "normalised primary matrix" from chromaticities, via the
/// standard Bruce-Lindbloom construction: scale each primary's unit-luminance
/// XYZ so that (1,1,1) maps exactly onto the white point.
///
/// This is the same algorithm `rgb_to_xyz` (`builtin_transforms.rs:2112-2153`)
/// implements, and that function is *not* part of OCIO-M13 -- it is
/// reimplemented here so the expected value owes nothing to the code under test.
fn rgb_to_xyz_npm(chroms: [[f64; 2]; 4]) -> Option<M3> {
    let r = xy_to_xyz(chroms[0]);
    let g = xy_to_xyz(chroms[1]);
    let b = xy_to_xyz(chroms[2]);
    let w = xy_to_xyz(chroms[3]);

    // Columns are the primaries' unit-luminance XYZ.
    let prim = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let scale = mat_vec(&mat_inv(&prim)?, w);
    Some(mat_mul(&prim, &diag(scale)))
}

/// Bradford CAT, `Ma^-1 * diag(dst_LMS / src_LMS) * Ma`, with `Ma` kept in the
/// row orientation the reference uses. This is the correct construction; the
/// shipped `bradford_adapt` performs the same algebra on `Ma^T`.
fn bradford_cat(src_xyz: [f64; 3], dst_xyz: [f64; 3]) -> Option<M3> {
    let src_lms = mat_vec(&BRADFORD_MA, src_xyz);
    let dst_lms = mat_vec(&BRADFORD_MA, dst_xyz);
    let ratio = [
        dst_lms[0] / src_lms[0],
        dst_lms[1] / src_lms[1],
        dst_lms[2] / src_lms[2],
    ];
    let ma_inv = mat_inv(&BRADFORD_MA)?;
    Some(mat_mul(&ma_inv, &mat_mul(&diag(ratio), &BRADFORD_MA)))
}

/// The correct AP1->XYZ(D65) matrix, composed in the same order as
/// `builtin_transforms.rs:2023-2024` (`mat4_mul(&bfd, &ap1_xyz)`).
fn ap1_to_xyz_d65_correct() -> Option<M3> {
    let ap1_xyz = rgb_to_xyz_npm(AP1_CHROMS)?;
    let src_white = mat_vec(&ap1_xyz, [1.0, 1.0, 1.0]);
    let cat = bradford_cat(src_white, D65_XYZ)?;
    Some(mat_mul(&cat, &ap1_xyz))
}

fn expected_correct(v: [f32; 3]) -> Result<[f64; 3], Box<dyn std::error::Error>> {
    let m = ap1_to_xyz_d65_correct()
        .ok_or("singular matrix while deriving the correct AP1->XYZ(D65) transform")?;
    Ok(mat_vec(&m, [v[0] as f64, v[1] as f64, v[2] as f64]))
}

fn bradford_ap1_to_xyz_d65_processor() -> Result<Processor, Box<dyn std::error::Error>> {
    let transform = Transform::Builtin(BuiltinTransform {
        style: "UTILITY - ACES-AP1_to_CIE-XYZ-D65_BFD".to_string(),
        direction: TransformDirection::Forward,
    });
    Ok(Processor::from_transform(
        &transform,
        TransformDirection::Forward,
    )?)
}

/// THE RED GATE for OCIO-M13. Expected to FAIL today: the real
/// `AP1_TO_XYZ_D65` builtin (which calls the buggy `bradford_adapt`
/// internally) diverges from the hand-derived correct matrix by roughly
/// 1-15% per channel on this saturated colour (measured offline: R ~1.8%,
/// G ~0.8%, B ~14.9% -- see module doc comment for the full derivation).
#[test]
fn red_gate_bradford_saturated_d60_to_d65_OCIO_M13() -> TestResult {
    let expected = expected_correct(BRADFORD_SATURATED_V)?;

    let processor = bradford_ap1_to_xyz_d65_processor()?;
    let mut pixels = [BRADFORD_SATURATED_V];
    processor.apply_rgb(&mut pixels);
    let actual = [
        pixels[0][0] as f64,
        pixels[0][1] as f64,
        pixels[0][2] as f64,
    ];

    let diff_pct: Vec<f64> = (0..3)
        .map(|i| (actual[i] - expected[i]).abs() / expected[i].abs() * 100.0)
        .collect();

    assert_close(
        actual,
        expected,
        &format!(
            "Bradford D60->D65 on saturated AP1 colour (OCIO-M13); per-channel %diff={diff_pct:?}"
        ),
    );
    Ok(())
}

/// Guards the derivation above, so a mistake in the test's own arithmetic cannot
/// masquerade as OCIO-M13.
///
/// The NPM construction is fed only AP1's chromaticities; it is never told what
/// D60 is. Mapping (1,1,1) through the result must therefore land on the white
/// point implied by those chromaticities, and that must agree with the crate's
/// independently-stated `D60_XYZ` constant. Agreement to 8 significant figures
/// between two values computed from unrelated inputs is what makes the rest of
/// the chain trustworthy; this was a comment before, and a comment cannot fail.
///
/// Expected to PASS, before and after OCIO-M13 is fixed. If it ever fails, the
/// test is wrong, not the code under test -- fix this first and ignore whatever
/// the red gates are saying.
#[test]
fn control_bradford_derivation_reproduces_d60_whitepoint() -> TestResult {
    let ap1_xyz =
        rgb_to_xyz_npm(AP1_CHROMS).ok_or("singular AP1 primary matrix in the test derivation")?;
    let white = mat_vec(&ap1_xyz, [1.0, 1.0, 1.0]);

    for i in 0..3 {
        let rel = (white[i] - D60_XYZ[i]).abs() / D60_XYZ[i].abs();
        assert!(
            rel < 1e-8,
            "test derivation is unsound: NPM white-point component {i} is {:.14}, but the crate's \
             D60_XYZ (builtin_transforms.rs:2337) says {:.14} (rel={rel:.3e}). The expected values \
             in this file are computed from AP1 chromaticities alone, so this disagreement means \
             the reimplementation is wrong -- not that OCIO-M13 is present.",
            white[i],
            D60_XYZ[i]
        );
    }
    Ok(())
}

/// CONTROL for OCIO-M13 -- documents why neutral-colour tests are blind to
/// this bug. Any chromatic-adaptation matrix (the buggy transpose
/// included) maps the achromatic axis onto itself, because the CAT's
/// diagonal-scaling step is defined precisely to send `src_white ->`
/// `dst_white`; the bug only changes how the *cone-response basis* is
/// read, not whether the composed map is a similarity along that axis.
/// This assertion is expected to PASS both before and after OCIO-M13 is
/// fixed -- a pass here is not evidence of correctness, only a
/// demonstration of the blind spot that let the bug ship.
#[test]
fn red_gate_bradford_neutral_control_OCIO_M13() -> TestResult {
    let expected = expected_correct(BRADFORD_NEUTRAL_V)?;

    let processor = bradford_ap1_to_xyz_d65_processor()?;
    let mut pixels = [BRADFORD_NEUTRAL_V];
    processor.apply_rgb(&mut pixels);
    let actual = [
        pixels[0][0] as f64,
        pixels[0][1] as f64,
        pixels[0][2] as f64,
    ];
    assert_close(
        actual,
        expected,
        "Bradford neutral CONTROL (OCIO-M13) -- expected to pass regardless of the bug",
    );
    Ok(())
}
