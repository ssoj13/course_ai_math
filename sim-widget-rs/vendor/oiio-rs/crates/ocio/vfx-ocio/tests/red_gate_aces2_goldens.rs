//! Red-gate golden-value tests for `vfx_ocio::aces2`.
//!
//! # What this pins
//!
//! `crates/ocio/vfx-ocio/src/aces2.rs` has property tests and forward/inverse round-trip
//! tests, but **zero reference anchors** against OpenColorIO's own numeric output. Every
//! test here asserts our Rust implementation against literal input/expected arrays lifted
//! verbatim from OpenColorIO's C++ unit tests (`tests/cpu/ops/fixedfunction/FixedFunctionOpCPU_tests.cpp`
//! in the reference tree at `C:/projects/projects.rust.cg.offload/vfx.ref/OpenColorIO`).
//!
//! # Why round-trip tests miss this
//!
//! A round-trip test (`fwd` then `inv`, or vice versa, checking you get the original value
//! back) can only fail if forward and inverse *disagree* with each other. It says nothing
//! about whether either direction agrees with the *correct* answer. If a defect is baked
//! into a helper that both forward and inverse call identically, round-trip stays green
//! while both directions are simultaneously wrong in the same way.
//!
//! That is exactly the shape of **ACES2-01** (`aces2.rs:1553-1562`,
//! `determine_hue_linearity_range`): it builds the `[i32; 2]` binary-search window used by
//! `lookup_hue_interval` (`aces2.rs:428-443`) but omits the `TABLE_FIRST_NOM` (+1) offset
//! that `lookup_hue_interval` itself applies via `hues.nominal_hue_pos(h)`
//! (`aces2.rs:167-169`) before adding the range. The window ends up one table slot narrower
//! on the low side than OCIO's C++ `Transform.cpp` equivalent. `lookup_hue_interval` is
//! called from `init_hue_gamut_params` (`aces2.rs:885`), which both `gamut_compress_fwd`
//! and `gamut_compress_inv` call identically (`aces2.rs:911`, `:923`) — so a wrong binary
//! search selects the same wrong table interval in both directions, producing an
//! interpolation weight (`interp_weight`, `aces2.rs:445-447`) outside `[0, 1]` and hence an
//! *extrapolated* (not interpolated) cusp / `gamma_top_inv` for hues near the edges of the
//! search window. Forward and inverse both consume that same wrong cusp, so they still
//! invert each other — round-trip is blind to it by construction.
//!
//! Every test in this file instead checks ONE direction against an OCIO-derived anchor
//! that does not depend on our own inverse (or forward) agreeing with anything:
//!   - `*_fwd` tests: `our_fwd(golden_input) ≈ golden_expected`
//!   - `*_inv` tests: `our_inv(golden_expected) ≈ golden_input`
//!
//! Both are independently anchored to OCIO ground truth, so a shared forward+inverse
//! defect shows up in *both* tests instead of neither.
//!
//! # Expected outcome (historical: was a red gate; now green)
//!
//! This file was written as a **red gate**: failure was the deliverable, not a mistake, and
//! at the time nothing here weakened an assertion or added `#[ignore]` to force a pass. **All
//! 11 tests are now green** (`docs/testing.md`, red gate table: closed by `c52eb41`) —
//! left in place as a regression anchor per the red-gate rules, not removed now that it
//! passes.
//!
//! ## The failures that used to exist here were NOT ACES2-01
//!
//! ACES2-01 is the *motivation* for this file — it is why anchors were needed at all, since
//! a round-trip cannot see it — but it was **not** the cause of the 3-of-11 failures this file
//! originally exposed. The investigation below (kept for its record of what was ruled out) found
//! the failure signature did not match ACES2-01's mechanism: a full-hue-circle sweep at 0.01°
//! resolution found no discontinuity, and a wrong interval selection is by nature discontinuous.
//! The hypothesis recorded at the time — that `gamut_compress_inv` (`aces2.rs:915-928`)'s
//! two-pass approximate inverse was the cause — was ALSO wrong (see the red gate
//! table in `docs/testing.md`). **The real cause was f32 precision**: the 3x3 matrix
//! inversion ran in f32 where OCIO works in double, and the cusp/reach tables were generated in
//! f32; at hue ~192.2° the row sits at an ill-conditioned point of `solve_j_intersect`, which
//! amplified that error ~10x. `c52eb41` computes both in f64, narrowing only where OCIO does,
//! and all 11 goldens now pass at the UNCHANGED tolerances (no assertion here was weakened).
//!
//! # Numeric comparison
//!
//! OpenColorIO's `ApplyFixedFunction` test helper (`tests/cpu/UnitTestUtils.h:79-100`,
//! `EqualWithSafeRelError`) compares with **absolute** error below `min_expected = 1.0` and
//! **relative** error above it: `|actual - expected| / max(|expected|, 1.0) <= eps`. We
//! reproduce that exact comparison (`ocio_close` below) rather than a plain absolute
//! tolerance, since several golden values (J, M) are well above 1.0 and the OCIO tests were
//! authored against that metric.

use vfx_ocio::aces2::{
    ACES_AP0, ACES_AP1, Chromaticity, Primaries, gamut_compress_fwd_deg, gamut_compress_inv_deg,
    init_chroma_compress_params, init_gamut_compress_params, init_jmh_params,
    init_output_transform, init_shared_compression_params, init_tonescale_params,
    jmh_degrees_to_rgb, output_transform_fwd, output_transform_inv, rgb_to_jmh_degrees,
    tonescale_compress_fwd, tonescale_compress_inv,
};

/// A JMh or RGB triple (alpha channel dropped — every OCIO golden below passthroughs alpha
/// unchanged, so it carries no information about the algorithm under test).
type Row = [f32; 3];

// --- OCIO numeric-comparison semantics (ported verbatim) ---

/// Port of OpenColorIO `EqualWithSafeRelError` (`tests/cpu/UnitTestUtils.h:79-100`), the
/// comparison every `ApplyFixedFunction`-driven OCIO golden test uses, always called with
/// `min_expected = 1.0f`. Absolute error for `|expected| < 1`, relative error above it.
fn ocio_close(actual: f32, expected: f32, eps: f32) -> bool {
    const MIN_EXPECTED: f32 = 1.0;
    if actual == expected {
        return true;
    }
    let div = if expected > 0.0 {
        expected.max(MIN_EXPECTED)
    } else {
        (-expected).max(MIN_EXPECTED)
    };
    ((actual - expected).abs() / div) <= eps
}

/// Compare a batch of rows against OCIO-derived expected rows, collecting every mismatch
/// instead of stopping at the first one, so a single `cargo test` run reports the full
/// diagnostic (which rows/channels disagree, by how much) rather than just one panic.
fn assert_rows_close(label: &str, actual: &[Row], expected: &[Row], eps: f32) {
    assert_eq!(actual.len(), expected.len(), "{label}: row count mismatch");
    let mut failures = Vec::new();
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        for c in 0..3 {
            if !ocio_close(a[c], e[c], eps) {
                failures.push(format!(
                    "  row {i} chan {c}: actual={:.6} expected={:.6} (eps={eps})",
                    a[c], e[c]
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{label}: {}/{} channel(s) diverge from OCIO reference:\n{}",
        failures.len(),
        actual.len() * 3,
        failures.join("\n")
    );
}

// --- Display primaries used by the OCIO goldens ---

/// P3-D65 display primaries, exactly as passed to OCIO's `FixedFunctionOpData::Params` in
/// `aces_output_transform_20`, `aces_tonescale_compress_20` (chroma/tonescale doesn't take
/// gamut params but the chain feeds `aces_gamut_map_20`), and `aces_gamut_map_20`
/// (FixedFunctionOpCPU_tests.cpp:514-515, :963).
fn p3d65_primaries() -> Primaries {
    Primaries {
        red: Chromaticity { x: 0.680, y: 0.320 },
        green: Chromaticity { x: 0.265, y: 0.690 },
        blue: Chromaticity { x: 0.150, y: 0.060 },
        white: Chromaticity {
            x: 0.3127,
            y: 0.3290,
        },
    }
}

/// Rec.709 display primaries, as passed to OCIO's `aces_ot_20_edge_cases`
/// (FixedFunctionOpCPU_tests.cpp:609-610).
fn rec709_primaries() -> Primaries {
    Primaries {
        red: Chromaticity {
            x: 0.6400,
            y: 0.3300,
        },
        green: Chromaticity {
            x: 0.3000,
            y: 0.6000,
        },
        blue: Chromaticity {
            x: 0.1500,
            y: 0.0600,
        },
        white: Chromaticity {
            x: 0.3127,
            y: 0.3290,
        },
    }
}

// ===========================================================================
// Stage 1: RGB_to_JMh / JMh_to_RGB (ACES2065-1 / AP0), no peak-luminance or
// display-gamut dependence.
//
// SOURCE: FixedFunctionOpCPU_tests.cpp:696-791 (`aces_rgb_to_jmh_20`).
// OCIO Params = ACES AP0 primaries verbatim (matches our `ACES_AP0` constant):
//   {0.7347, 0.2653, 0.0000, 1.0000, 0.0001, -0.0770, 0.32168, 0.33767}
// Renderer construction: `Renderer_ACES_RGB_TO_JMh_20` ctor,
// FixedFunctionOpCPU.cpp:1168-1190 — `m_p = ACES2::init_JMhParams(primaries)`, no other
// state. Fwd tolerance 1e-5f (line 780), inv (JMh->RGB, vs original AP0 input) 1e-4f
// (line 789).
// ===========================================================================

/// Input RGB (AP0 / ACES2065-1), FixedFunctionOpCPU_tests.cpp:703-734. Alpha column
/// dropped (passthrough only).
const RGB_TO_JMH_INPUT: [Row; 27] = [
    // ACEScg primaries and secondaries scaled by 4
    [2.781_808_9, 0.179_178_25, -0.022_103_53],
    [3.344_523_7, 3.617_862_7, -0.006002689],
    [0.562_714_8, 3.438_684_5, 0.016_100_84],
    [1.218_191, 3.820_821_8, 4.022_103_3],
    [0.655_476_3, 0.382_137_27, 4.006_003],
    [3.437_285_2, 0.561_315_54, 3.983_899],
    // OCIO test values
    [0.11, 0.02, 0.04],
    [0.71, 0.51, 0.81],
    [0.43, 0.82, 0.71],
    // ColorChecker24 (SMPTE 2065-1 2021)
    [0.118_77, 0.087_09, 0.058_95],
    [0.400_02, 0.319_16, 0.237_36],
    [0.184_76, 0.203_98, 0.313_11],
    [0.109_01, 0.135_11, 0.064_93],
    [0.266_84, 0.246_04, 0.409_32],
    [0.322_83, 0.462_08, 0.406_06],
    [0.386_05, 0.227_43, 0.057_77],
    [0.138_22, 0.130_37, 0.337_03],
    [0.302_02, 0.137_52, 0.127_58],
    [0.093_1, 0.063_47, 0.135_25],
    [0.348_76, 0.436_54, 0.106_13],
    [0.486_55, 0.366_85, 0.080_61],
    [0.087_32, 0.074_43, 0.272_74],
    [0.153_66, 0.256_92, 0.090_71],
    [0.217_42, 0.070_7, 0.051_3],
    [0.589_19, 0.539_43, 0.091_57],
    [0.309_04, 0.148_18, 0.274_26],
    [0.149_01, 0.233_78, 0.359_39],
];

/// Expected JMh (J, M, hue-in-degrees), FixedFunctionOpCPU_tests.cpp:739-770.
const RGB_TO_JMH_EXPECTED: [Row; 27] = [
    [107.480_64, 206.827_3, 25.025_11],
    [173.194_08, 133.330_89, 106.183_45],
    [139.210_22, 191.922_36, 147.056_49],
    [157.905_17, 111.975_31, 192.204_73],
    [79.229_28, 100.424_66, 268.442_1],
    [132.888_14, 173.358_78, 341.715_24],
    [26.112_514, 42.523_605, 4.173_158],
    [79.190_46, 25.002_3, 332.159_76],
    [81.912_56, 39.754_81, 182.925_75],
    [33.924_664, 12.254_567, 38.146_66],
    [61.332_394, 15.169_423, 39.841_843],
    [47.191_544, 11.839_942, 249.107_12],
    [37.328_3, 13.224_151, 128.878_04],
    [53.465_55, 13.121_579, 285.658_97],
    [65.414_51, 19.172_148, 179.324_26],
    [55.711_514, 37.182_04, 50.924_01],
    [40.020_96, 20.762_512, 271.008_33],
    [47.704_77, 35.791_145, 13.975_611],
    [30.385_914, 14.544_74, 317.544_28],
    [64.222_85, 33.487_698, 119.145_134],
    [65.570_36, 35.864_014, 70.842_19],
    [31.800_465, 23.920_212, 273.228_97],
    [47.950_405, 28.027_388, 144.154_16],
    [38.440_968, 42.604_164, 17.892_262],
    [75.117_74, 40.952_045, 90.752_045],
    [49.311_21, 33.812_24, 348.832_1],
    [47.441_757, 22.915_655, 218.454_38],
];

#[test]
fn red_gate_aces2_rgb_to_jmh_fwd() {
    let p = init_jmh_params(&ACES_AP0);
    let actual: Vec<Row> = RGB_TO_JMH_INPUT
        .iter()
        .map(|rgb| rgb_to_jmh_degrees(rgb, &p))
        .collect();
    assert_rows_close(
        "rgb_to_jmh_degrees fwd vs OCIO aces_rgb_to_jmh_20 (FixedFunctionOpCPU_tests.cpp:696)",
        &actual,
        &RGB_TO_JMH_EXPECTED,
        1e-5,
    );
}

#[test]
fn red_gate_aces2_jmh_to_rgb_inv() {
    // Anchor: JMh_to_RGB(golden JMh expected) must recover the golden RGB input, exactly
    // as OCIO checks at FixedFunctionOpCPU_tests.cpp:783-790 (tolerance 1e-4f).
    let p = init_jmh_params(&ACES_AP0);
    let actual: Vec<Row> = RGB_TO_JMH_EXPECTED
        .iter()
        .map(|jmh| jmh_degrees_to_rgb(jmh, &p))
        .collect();
    assert_rows_close(
        "jmh_degrees_to_rgb inv vs OCIO aces_rgb_to_jmh_20 (FixedFunctionOpCPU_tests.cpp:696)",
        &actual,
        &RGB_TO_JMH_INPUT,
        1e-4,
    );
}

// ===========================================================================
// Stage 2: tonescale + chroma compress (peak luminance only, no display gamut).
//
// SOURCE: FixedFunctionOpCPU_tests.cpp:793-884 (`aces_tonescale_compress_20`).
// Input is the *same* JMh values as `RGB_TO_JMH_EXPECTED` above (OCIO chains the stages by
// reusing `expected_32f` from the previous test as `input_32f` here — see comment at
// line 700-701: "processed and carried over to the next FixedFunctionOp test").
// Renderer construction: `Renderer_ACES_TONESCALE_COMPRESS_20` ctor,
// FixedFunctionOpCPU.cpp:1243-1255 — `m_p = init_JMhParams(AP0)`,
// `m_t = init_ToneScaleParams(1000)`, `m_s = init_SharedCompressionParams(1000, m_p,
// init_JMhParams(AP1))`, `m_c = init_ChromaCompressParams(1000, m_t)`. Fwd tolerance 1e-5f
// (line 873), inv tolerance 1e-4f (line 882).
// ===========================================================================

/// Expected JMh after tonescale + chroma compress fwd, peak_lum=1000,
/// FixedFunctionOpCPU_tests.cpp:833-864.
const TONESCALE_COMPRESS_EXPECTED: [Row; 27] = [
    [110.702_45, 211.251_77, 25.025_11],
    [168.016_82, 129.796_25, 106.183_45],
    [140.814_85, 193.459_2, 147.056_49],
    [156.429_5, 110.938_42, 192.204_73],
    [80.456_56, 98.490_53, 268.442_1],
    [135.172_23, 175.559_33, 341.715_24],
    [18.187_317, 33.819_19, 4.173_158],
    [80.413_1, 21.309_33, 332.159_76],
    [83.447_88, 37.852_524, 182.925_75],
    [27.411_968, 13.382_785, 38.146_66],
    [59.987_66, 14.391_894, 39.841_843],
    [43.298_923, 12.199_878, 249.107_12],
    [31.489_655, 14.075_142, 128.878_04],
    [50.749_2, 12.731_807, 285.658_97],
    [64.728_64, 18.593_792, 179.324_26],
    [53.399_445, 37.394_417, 50.924_01],
    [34.719_597, 21.616_766, 271.008_33],
    [43.910_71, 36.788_166, 13.975_611],
    [23.196_53, 15.118_355, 317.544_28],
    [63.348_682, 33.283_52, 119.145_134],
    [64.908_875, 35.371_063, 70.842_19],
    [24.876_913, 23.143_16, 273.228_97],
    [44.203_377, 28.918_33, 144.154_16],
    [32.824_36, 43.447_853, 17.892_262],
    [75.830_87, 39.872_49, 90.752_045],
    [45.823_12, 34.652_058, 348.832_1],
    [43.597_237, 23.079_071, 218.454_38],
];

fn tonescale_1000_params() -> (
    vfx_ocio::aces2::JMhParams,
    vfx_ocio::aces2::ToneScaleParams,
    vfx_ocio::aces2::SharedCompressionParams,
    vfx_ocio::aces2::ChromaCompressParams,
) {
    let p_in = init_jmh_params(&ACES_AP0);
    let ts = init_tonescale_params(1000.0);
    let reach = init_jmh_params(&ACES_AP1);
    let shared = init_shared_compression_params(1000.0, &p_in, &reach);
    let chroma = init_chroma_compress_params(1000.0, &ts);
    (p_in, ts, shared, chroma)
}

#[test]
fn red_gate_aces2_tonescale_compress_fwd() {
    let (p_in, ts, shared, chroma) = tonescale_1000_params();
    let actual: Vec<Row> = RGB_TO_JMH_EXPECTED
        .iter()
        .map(|jmh| tonescale_compress_fwd(jmh, &p_in, &ts, &shared, &chroma))
        .collect();
    assert_rows_close(
        "tonescale_compress_fwd vs OCIO aces_tonescale_compress_20 (FixedFunctionOpCPU_tests.cpp:793)",
        &actual,
        &TONESCALE_COMPRESS_EXPECTED,
        1e-5,
    );
}

#[test]
fn red_gate_aces2_tonescale_compress_inv() {
    // Anchor: tonescale_compress_inv(golden fwd-expected) must recover the pre-tonescale
    // JMh (RGB_TO_JMH_EXPECTED), independent of our own fwd path.
    let (p_in, ts, shared, chroma) = tonescale_1000_params();
    let actual: Vec<Row> = TONESCALE_COMPRESS_EXPECTED
        .iter()
        .map(|jmh| tonescale_compress_inv(jmh, &p_in, &ts, &shared, &chroma))
        .collect();
    assert_rows_close(
        "tonescale_compress_inv vs OCIO aces_tonescale_compress_20 (FixedFunctionOpCPU_tests.cpp:793)",
        &actual,
        &RGB_TO_JMH_EXPECTED,
        1e-4,
    );
}

// ===========================================================================
// Stage 3: gamut compress (peak luminance + P3-D65 display gamut). THIS is the stage
// where ACES2-01 lives: `gamut_compress_fwd_deg`/`gamut_compress_inv_deg` both go through
// `init_hue_gamut_params` -> `lookup_hue_interval`, which consumes the
// `hue_linearity_search_range` built by the buggy `determine_hue_linearity_range`
// (aces2.rs:1553-1562).
//
// SOURCE: FixedFunctionOpCPU_tests.cpp:886-982 (`aces_gamut_map_20`).
// Input is the literal `input_32f` array at lines 890-921 (NOT re-derived from our own
// tonescale-compress output — OCIO re-dumped the actual C++ FWD result as this test's
// literal input, so a handful of last-digit values differ slightly from
// `TONESCALE_COMPRESS_EXPECTED` above; both are lifted verbatim from their own test).
// Renderer construction: `Renderer_ACES_GAMUT_COMPRESS_20` ctor, FixedFunctionOpCPU.cpp:
// 1321-1350 — `pIn = init_JMhParams(AP0)`, `pLim = init_JMhParams(P3D65)`,
// `t = init_ToneScaleParams(1000)`, `m_s = init_SharedCompressionParams(1000, pIn,
// init_JMhParams(AP1))`, `m_g = init_GamutCompressParams(1000, pIn, pLim, t, m_s,
// init_JMhParams(AP1))`. Fwd tolerance 1e-5f (line 972), inv tolerance 1e-5f (line 981).
// ===========================================================================

/// Input JMh (post tonescale-compress), FixedFunctionOpCPU_tests.cpp:890-921.
const GAMUT_COMPRESS_INPUT: [Row; 27] = [
    // ACEScg primaries and secondaries scaled by 4 — these six rows are near-saturated
    // hues derived from the ACEScg RGB-cube corners/secondaries. They sit close to the
    // limiting-gamut cusp region that `build_limiting_cusp_corners` (aces2.rs:1122) uses
    // to seed the densely-sampled portion of the hue table, making them the natural
    // candidates for exposing an off-by-one in the cusp-table binary-search window
    // (ACES2-01) — see `red_gate_gamut_cusp_window_aces2_01_fwd`/`_inv` below.
    [110.702_45, 211.251_77, 25.025_11],
    [168.016_82, 129.796_25, 106.183_45],
    [140.814_85, 193.459_21, 147.056_49],
    [156.429_52, 110.938_515, 192.204_73],
    [80.456_54, 98.490_524, 268.442_1],
    [135.172_2, 175.559_28, 341.715_24],
    // OCIO test values
    [18.187_315, 33.819_176, 4.173_158],
    [80.413_12, 21.309_33, 332.159_76],
    [83.447_89, 37.852_29, 182.925_75],
    // ColorChecker24 (SMPTE 2065-1 2021)
    [27.411_964, 13.382_77, 38.146_66],
    [59.987_67, 14.391_894, 39.841_843],
    [43.298_923, 12.199_878, 249.107_12],
    [31.489_658, 14.075_143, 128.878_04],
    [50.749_2, 12.731_814, 285.658_97],
    [64.728_64, 18.593_796, 179.324_26],
    [53.399_45, 37.394_43, 50.924_01],
    [34.719_597, 21.616_766, 271.008_33],
    [43.910_713, 36.788_166, 13.975_611],
    [23.196_526, 15.118_355, 317.544_28],
    [63.348_675, 33.283_493, 119.145_134],
    [64.908_89, 35.371_044, 70.842_19],
    [24.876_911, 23.143_16, 273.228_97],
    [44.203_377, 28.918_33, 144.154_16],
    [32.824_356, 43.447_876, 17.892_262],
    [75.830_87, 39.872_475, 90.752_045],
    [45.823_116, 34.652_07, 348.832_1],
    [43.597_24, 23.079_079, 218.454_38],
];

/// Expected JMh after gamut compress fwd, peak_lum=1000, P3-D65,
/// FixedFunctionOpCPU_tests.cpp:926-957.
const GAMUT_COMPRESS_EXPECTED: [Row; 27] = [
    [107.829_74, 174.270_16, 25.025_11],
    [168.028_27, 118.227_56, 106.183_45],
    [140.030_17, 127.184_48, 147.056_49],
    [156.512_44, 73.219_185, 192.204_73],
    [79.378_555, 72.608_604, 268.442_1],
    [133.827_94, 149.930_62, 341.715_24],
    [18.193_993, 33.313_07, 4.173_158],
    [80.413_12, 21.309_33, 332.159_76],
    [83.467_445, 37.305_03, 182.925_75],
    [27.411_963, 13.382_77, 38.146_66],
    [59.987_675, 14.391_894, 39.841_843],
    [43.298_92, 12.199_878, 249.107_12],
    [31.489_658, 14.075_143, 128.878_04],
    [50.749_2, 12.731_814, 285.658_97],
    [64.728_64, 18.593_796, 179.324_26],
    [53.399_45, 37.394_43, 50.924_01],
    [34.719_597, 21.616_766, 271.008_33],
    [43.910_713, 36.788_166, 13.975_611],
    [23.196_526, 15.118_355, 317.544_28],
    [63.348_675, 33.283_493, 119.145_134],
    [64.908_88, 35.371_044, 70.842_19],
    [24.876_911, 23.143_16, 273.228_97],
    [44.203_377, 28.918_33, 144.154_16],
    [32.824_356, 43.447_876, 17.892_262],
    [75.830_87, 39.872_475, 90.752_045],
    [45.823_112, 34.652_07, 348.832_1],
    [43.635_548, 21.629_519, 218.454_38],
];

fn gamut_1000_p3d65_params() -> (
    vfx_ocio::aces2::SharedCompressionParams,
    vfx_ocio::aces2::GamutCompressParams,
) {
    let p_in = init_jmh_params(&ACES_AP0);
    let p_lim = init_jmh_params(&p3d65_primaries());
    let ts = init_tonescale_params(1000.0);
    let reach = init_jmh_params(&ACES_AP1);
    let shared = init_shared_compression_params(1000.0, &p_in, &reach);
    let gamut = init_gamut_compress_params(1000.0, &p_in, &p_lim, &ts, &shared, &reach);
    (shared, gamut)
}

#[test]
fn red_gate_aces2_gamut_compress_fwd() {
    let (shared, gamut) = gamut_1000_p3d65_params();
    let actual: Vec<Row> = GAMUT_COMPRESS_INPUT
        .iter()
        .map(|jmh| gamut_compress_fwd_deg(jmh, &shared, &gamut))
        .collect();
    assert_rows_close(
        "gamut_compress_fwd_deg vs OCIO aces_gamut_map_20 (FixedFunctionOpCPU_tests.cpp:886)",
        &actual,
        &GAMUT_COMPRESS_EXPECTED,
        1e-5,
    );
}

#[test]
fn red_gate_aces2_gamut_compress_inv() {
    let (shared, gamut) = gamut_1000_p3d65_params();
    let actual: Vec<Row> = GAMUT_COMPRESS_EXPECTED
        .iter()
        .map(|jmh| gamut_compress_inv_deg(jmh, &shared, &gamut))
        .collect();
    assert_rows_close(
        "gamut_compress_inv_deg vs OCIO aces_gamut_map_20 (FixedFunctionOpCPU_tests.cpp:886)",
        &actual,
        &GAMUT_COMPRESS_INPUT,
        1e-5,
    );
}

// --- Hue-window slice: now green (`c52eb41`, f32-vs-f64 precision — see the module doc
// comment at the top of this file and the red gate table in `docs/testing.md`). Kept as a
// regression anchor, function names no longer say "UNATTRIBUTED" now that the cause is
// known and fixed. ---
//
// HISTORICAL CONTEXT, kept for the record: when these tests were red, the cause had NOT
// been established, and in particular was NOT known to be ACES2-01 (an earlier revision of
// this file had named them after ACES2-01, which was an overclaim, withdrawn before this
// pass). The investigation below ruled out ACES2-01 and a wrong `gamut_compress_inv`
// refinement count correctly, but its own leading hypothesis was ALSO not the true cause —
// see the module doc comment for what actually was (f32 precision in the cusp/reach tables
// and matrix inversion).
//
// TWO HONEST LIMITATIONS OF THIS SLICE (still true, unrelated to the fix):
//
//  1. It does NOT probe near a gamut cusp. It reuses the first six rows of
//     `aces_gamut_map_20` verbatim (hues 25.03, 106.18, 147.06, 192.20, 268.44, 341.72 deg)
//     because those are the only OCIO-derived anchors available. Minting anchors at hues
//     genuinely close to a cusp would require running the OCIO C++ implementation, which was
//     not possible here, and inventing them was declined. So the name says "hue window", not
//     "cusp".
//  2. The failure signature does not match ACES2-01's mechanism. ACES2-01 is a wrong
//     *interval selection*: as hue crosses a table-slot boundary the binary search flips to a
//     different pair of entries, which makes the output DISCONTINUOUS in hue. A sweep of
//     `gamut_compress_inv_deg` across the full hue circle at 0.01 deg resolution, holding
//     row 3's J and M fixed, found no such jump:
//       - around the failing hue 192.2 deg (+/-2 deg): max/median first-difference = 1.2,
//         i.e. perfectly smooth;
//       - over all 360 deg: median step 1.526e-5, largest 3.120e-2 at hue ~221.75 -- but
//         spread smoothly across ~10 consecutive samples (a steep ramp), not a single-sample
//         jump, and at a hue no golden covers.
//
// LEADING HYPOTHESIS for the two inverse-direction failures, which is NOT ACES2-01:
// `gamut_compress_inv` (aces2.rs:915-928) is an APPROXIMATE inverse -- a two-pass fixed-point
// refinement that calls `compress_gamut_core::<true>` once to refine `jx` and then again --
// whereas the forward has no such iteration. A small residual is therefore expected in the
// inverse by construction, and this cleanly explains why the forward PASSES on exactly the
// row where the inverse fails. The open question for the repair is whether OCIO's inverse
// uses the same refinement scheme, threshold, and iteration count as ours; that comparison
// has not been made.
//
// DO NOT respond to a failure here by loosening the tolerance. The tolerance is OCIO's own
// (1e-5, FixedFunctionOpCPU_tests.cpp:972/981) and we genuinely exceed it. Attribute the
// cause first.

#[test]
fn red_gate_gamut_hue_window_slice_fwd() {
    let (shared, gamut) = gamut_1000_p3d65_params();
    let actual: Vec<Row> = GAMUT_COMPRESS_INPUT[0..6]
        .iter()
        .map(|jmh| gamut_compress_fwd_deg(jmh, &shared, &gamut))
        .collect();
    assert_rows_close(
        "ACES2-01 cusp-adjacent window fwd (aces2.rs:1553 determine_hue_linearity_range) \
         vs OCIO aces_gamut_map_20 rows 0-5 (FixedFunctionOpCPU_tests.cpp:890-897, :928-933)",
        &actual,
        &GAMUT_COMPRESS_EXPECTED[0..6],
        1e-5,
    );
}

#[test]
fn red_gate_gamut_hue_window_slice_inv() {
    let (shared, gamut) = gamut_1000_p3d65_params();
    let actual: Vec<Row> = GAMUT_COMPRESS_EXPECTED[0..6]
        .iter()
        .map(|jmh| gamut_compress_inv_deg(jmh, &shared, &gamut))
        .collect();
    assert_rows_close(
        "ACES2-01 cusp-adjacent window inv (aces2.rs:1553 determine_hue_linearity_range) \
         vs OCIO aces_gamut_map_20 rows 0-5 (FixedFunctionOpCPU_tests.cpp:890-897, :928-933)",
        &actual,
        &GAMUT_COMPRESS_INPUT[0..6],
        1e-5,
    );
}

// ===========================================================================
// Full pipeline: ACES_OUTPUT_TRANSFORM_20 (scene-linear AP0 -> P3-D65 display), peak_lum
// 1000 nit. Exercises RGB_to_JMh -> tonescale_compress -> gamut_compress -> JMh_to_RGB in
// one call (`output_transform_fwd`/`output_transform_inv`), so a gamut-compress-only
// defect (ACES2-01) can be masked here if the chroma is low enough that `gamut_m <= 0.0`
// short-circuits (aces2.rs:851-853) — the per-stage tests above are the primary
// discriminator; this is included for OCIO coverage parity.
//
// SOURCE: FixedFunctionOpCPU_tests.cpp:418-524 (`aces_output_transform_20`).
// Params: peak_lum=1000, P3-D65 gamut (line 511-516). Fwd tolerance 1e-5f (line 523), inv
// (vs original AP0 input) tolerance 1e-4f (line 543).
// ===========================================================================

const OUTPUT_TRANSFORM_INPUT: [Row; 35] = [
    // ACEScg primaries and secondaries scaled by 4
    [2.781_808_9, 0.179_178_25, -0.022_103_53],
    [3.344_523_7, 3.617_862_7, -0.006002689],
    [0.562_714_8, 3.438_684_5, 0.016_100_84],
    [1.218_191, 3.820_821_8, 4.022_103_3],
    [0.655_476_3, 0.382_137_27, 4.006_003],
    [3.437_285_2, 0.561_315_54, 3.983_899],
    // OCIO test values
    [0.11, 0.02, 0.04],
    [0.71, 0.51, 0.81],
    [0.43, 0.82, 0.71],
    // ColorChecker24 (SMPTE 2065-1 2021)
    [0.118_77, 0.087_09, 0.058_95],
    [0.400_02, 0.319_16, 0.237_36],
    [0.184_76, 0.203_98, 0.313_11],
    [0.109_01, 0.135_11, 0.064_93],
    [0.266_84, 0.246_04, 0.409_32],
    [0.322_83, 0.462_08, 0.406_06],
    [0.386_05, 0.227_43, 0.057_77],
    [0.138_22, 0.130_37, 0.337_03],
    [0.302_02, 0.137_52, 0.127_58],
    [0.093_1, 0.063_47, 0.135_25],
    [0.348_76, 0.436_54, 0.106_13],
    [0.486_55, 0.366_85, 0.080_61],
    [0.087_32, 0.074_43, 0.272_74],
    [0.153_66, 0.256_92, 0.090_71],
    [0.217_42, 0.070_7, 0.051_3],
    [0.589_19, 0.539_43, 0.091_57],
    [0.309_04, 0.148_18, 0.274_26],
    [0.149_01, 0.233_78, 0.359_39],
    [0.866_53, 0.867_92, 0.858_18],
    [0.573_56, 0.572_56, 0.571_69],
    [0.353_46, 0.353_37, 0.353_91],
    [0.202_53, 0.202_43, 0.202_87],
    [0.094_67, 0.095_2, 0.096_37],
    [0.037_45, 0.037_66, 0.038_95],
    // Spectrally non-selective 18 % reflecting diffuser
    [0.18, 0.18, 0.18],
    // Perfect reflecting diffuser
    [0.977_84, 0.977_84, 0.977_84],
];

const OUTPUT_TRANSFORM_EXPECTED: [Row; 35] = [
    [4.966_013_4, -0.033002287, 0.041583523],
    [3.969_460_7, 3.825_797_6, -0.056160748],
    [-0.075_460_04, 3.689_072_6, 0.270_235_06],
    [-0.095_436_63, 3.650_521_5, 3.459_975_7],
    [-0.028881177, 0.196_473_42, 2.796_123_7],
    [4.900_828_4, -0.064_385_53, 3.838_271],
    [0.096_890_49, -0.001135427, 0.018971475],
    [0.809_613_6, 0.479_857_15, 0.814_24],
    [0.107_417_94, 0.920_530_44, 0.726_379_04],
    [0.115_475_34, 0.050812997, 0.030212998],
    [0.484_880_15, 0.301_042_9, 0.226_769_03],
    [0.098_463_45, 0.160_814_84, 0.277_010_8],
    [0.071130276, 0.107_334_51, 0.035097614],
    [0.207_111_37, 0.198_474_82, 0.375_326_1],
    [0.195_447_12, 0.481_112_54, 0.393_299_1],
    [0.571_913_3, 0.196_873_26, 0.041634843],
    [0.045791976, 0.069_875_41, 0.291_233_57],
    [0.424_848_88, 0.083199054, 0.102_153_93],
    [0.059589352, 0.022219239, 0.091246955],
    [0.360_364_88, 0.478_741_5, 0.086726815],
    [0.695_661_96, 0.371_994_47, 0.068_298_06],
    [0.011_806_24, 0.021665439, 0.199_594_87],
    [0.076526135, 0.256_237_6, 0.060564563],
    [0.300_064_7, 0.023_416_28, 0.030360531],
    [0.805_483_2, 0.596_904_04, 0.082996234],
    [0.388_385_4, 0.079_899_33, 0.245_818_96],
    [0.010951802, 0.196_106_05, 0.307_181_54],
    [0.921_020_27, 0.921_707_63, 0.912_857_53],
    [0.590_191_6, 0.588_424_56, 0.587_825_3],
    [0.337_743_22, 0.337_686, 0.338_155_24],
    [0.169_266_4, 0.169_178_58, 0.169_557_15],
    [0.058_346_01, 0.059387885, 0.060296256],
    [0.012581199, 0.012947144, 0.013654212],
    // Spectrally non-selective 18 % reflecting diffuser
    [0.145_115_08, 0.145_115_7, 0.145_115_48],
    // Perfect reflecting diffuser
    [1.041_565_5, 1.041_566_6, 1.041_566_3],
];

#[test]
fn red_gate_aces2_output_transform_p3d65_1000n_fwd() {
    let st = init_output_transform(1000.0, &p3d65_primaries());
    let actual: Vec<Row> = OUTPUT_TRANSFORM_INPUT
        .iter()
        .map(|rgb| output_transform_fwd(rgb, &st))
        .collect();
    assert_rows_close(
        "output_transform_fwd vs OCIO aces_output_transform_20 (FixedFunctionOpCPU_tests.cpp:418)",
        &actual,
        &OUTPUT_TRANSFORM_EXPECTED,
        1e-5,
    );
}

#[test]
fn red_gate_aces2_output_transform_p3d65_1000n_inv() {
    let st = init_output_transform(1000.0, &p3d65_primaries());
    let actual: Vec<Row> = OUTPUT_TRANSFORM_EXPECTED
        .iter()
        .map(|rgb| output_transform_inv(rgb, &st))
        .collect();
    assert_rows_close(
        "output_transform_inv vs OCIO aces_output_transform_20 (FixedFunctionOpCPU_tests.cpp:418)",
        &actual,
        &OUTPUT_TRANSFORM_INPUT,
        1e-4,
    );
}

// ===========================================================================
// Hue-boundary edge case: bug #2220 ("related to hue angle calculation triggering an out
// of bounds access in the tables at exactly 360 degrees"). Same bug *class* as ACES2-01
// (a hue-table lookup index computed incorrectly near a table boundary), on the full
// output-transform path.
//
// SOURCE: FixedFunctionOpCPU_tests.cpp:589-618 (`aces_ot_20_edge_cases`).
// Params: peak_lum=100, Rec.709 gamut. Fwd tolerance 1e-4f (line 617). OCIO's own comment
// notes the exact numeric expected value is not significant — what matters is that the
// table access does not go out of bounds / diverge; we keep the literal expected array
// since that is what OCIO asserts.
// ===========================================================================

const EDGE_CASE_INPUT: [Row; 2] = [[0.0, 0.0, 0.0], [0.742_242_3, 0.093_193_34, 0.321_542_14]];

const EDGE_CASE_EXPECTED: [Row; 2] = [
    [0.0, 0.0, 0.0],
    [0.747_365_7, -0.001_935_247_3, 0.194_513_57],
];

#[test]
fn red_gate_aces2_edge_case_hue360_bug2220_fwd() {
    let st = init_output_transform(100.0, &rec709_primaries());
    let actual: Vec<Row> = EDGE_CASE_INPUT
        .iter()
        .map(|rgb| output_transform_fwd(rgb, &st))
        .collect();
    assert_rows_close(
        "output_transform_fwd vs OCIO aces_ot_20_edge_cases / bug #2220 (FixedFunctionOpCPU_tests.cpp:589)",
        &actual,
        &EDGE_CASE_EXPECTED,
        1e-4,
    );
}
