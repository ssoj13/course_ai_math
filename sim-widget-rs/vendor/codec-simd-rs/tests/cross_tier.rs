//! Cross-tier conformance tests.
//!
//! Every SIMD primitive must produce byte-identical output to the scalar
//! reference for every input.  Add a test set here for every new primitive.

#![allow(clippy::too_many_arguments)]

use codec_simd::Dispatch;

// =========================================================================
// sat_pack_i16_to_u8 (v0.1.0)
// =========================================================================

fn make_input_i16(n: usize) -> Vec<i16> {
    (0..n)
        .map(|i| ((i as i32 * 37 + 11) % 1024 - 384) as i16)
        .collect()
}

fn scalar_ref_sat_pack_i16(src: &[i16]) -> Vec<u8> {
    src.iter().map(|&v| v.clamp(0, 255) as u8).collect()
}

fn run_i16_to_u8(disp: &Dispatch, src: &[i16]) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];
    // SAFETY: Dispatch only installs fns whose target_feature reqs hold here.
    unsafe {
        (disp.sat_pack_i16_to_u8)(src.as_ptr(), out.as_mut_ptr(), src.len());
    }
    out
}

#[test]
fn sat_pack_i16_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 7, 15, 16, 17, 31, 32, 33, 63, 64, 127, 128, 1024];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        let src = make_input_i16(n);
        assert_eq!(
            run_i16_to_u8(&disp, &src),
            scalar_ref_sat_pack_i16(&src),
            "n={n}"
        );
    }
}

#[test]
fn sat_pack_i16_detected_dispatch_matches_scalar() {
    let n_cases = [
        0usize, 1, 7, 15, 16, 17, 31, 32, 33, 63, 64, 127, 128, 1024, 10_001,
    ];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        let src = make_input_i16(n);
        assert_eq!(
            run_i16_to_u8(&detected, &src),
            run_i16_to_u8(&scalar, &src),
            "tier {} diverged at n={n}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

// =========================================================================
// sat_pack_i32_to_u16 (v0.1.1)
// =========================================================================

fn make_input_i32(n: usize) -> Vec<i32> {
    (0..n)
        .map(|i| ((i as i32 * 37 + 11) % 8192) - 2048)
        .collect()
}

fn scalar_ref_sat_pack_i32(src: &[i32], max: u16) -> Vec<u16> {
    let m = max as i32;
    src.iter().map(|&v| v.max(0).min(m) as u16).collect()
}

fn run_i32_to_u16(disp: &Dispatch, src: &[i32], max: u16) -> Vec<u16> {
    let mut out = vec![0u16; src.len()];
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.sat_pack_i32_to_u16)(src.as_ptr(), out.as_mut_ptr(), src.len(), max);
    }
    out
}

#[test]
fn sat_pack_i32_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 3, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 1024];
    let maxes = [255u16, 1023, 4095, 16383, 65535];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        let src = make_input_i32(n);
        for &m in &maxes {
            assert_eq!(
                run_i32_to_u16(&disp, &src, m),
                scalar_ref_sat_pack_i32(&src, m),
                "n={n} max={m}"
            );
        }
    }
}

#[test]
fn sat_pack_i32_detected_dispatch_matches_scalar() {
    let n_cases = [
        0usize, 1, 3, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 1024, 10_001,
    ];
    let maxes = [255u16, 1023, 4095, 16383, 65535];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        let src = make_input_i32(n);
        for &m in &maxes {
            assert_eq!(
                run_i32_to_u16(&detected, &src, m),
                run_i32_to_u16(&scalar, &src, m),
                "tier {} diverged at n={n} max={m}",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
            );
        }
    }
}

#[test]
fn sat_pack_i32_boundary_values_match_scalar() {
    let max = 1023u16;
    let m = max as i32;
    let edge: Vec<i32> = vec![
        i32::MIN,
        -1_000_000,
        -100,
        -1,
        0,
        1,
        100,
        m - 1,
        m,
        m + 1,
        m * 2,
        1_000_000,
        i32::MAX,
    ];
    let mut src = edge.clone();
    while src.len() < 64 {
        src.extend_from_slice(&edge);
    }
    src.truncate(64);
    let want = scalar_ref_sat_pack_i32(&src, max);
    assert_eq!(run_i32_to_u16(&Dispatch::scalar(), &src, max), want);
    assert_eq!(run_i32_to_u16(&Dispatch::detect(), &src, max), want);
}

// =========================================================================
// add_clip_to_pixel_u16 (v0.1.2)
// =========================================================================

fn make_pred_u16(n: usize, max: u16) -> Vec<u16> {
    (0..n)
        .map(|i| ((i as u32 * 53 + 7) % (max as u32 + 1)) as u16)
        .collect()
}

fn make_res_i16(n: usize) -> Vec<i16> {
    (0..n)
        .map(|i| ((i as i32 * 71 + 3) % 4096 - 2048) as i16)
        .collect()
}

fn scalar_ref_add_clip(pred: &[u16], res: &[i16], max: u16) -> Vec<u16> {
    let m = max as i32;
    pred.iter()
        .zip(res.iter())
        .map(|(&p, &r)| ((p as i32) + (r as i32)).max(0).min(m) as u16)
        .collect()
}

fn run_add_clip(disp: &Dispatch, pred: &[u16], res: &[i16], max: u16) -> Vec<u16> {
    let mut out = vec![0u16; pred.len()];
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.add_clip_to_pixel_u16)(
            pred.as_ptr(),
            res.as_ptr(),
            out.as_mut_ptr(),
            pred.len(),
            max,
        );
    }
    out
}

#[test]
fn add_clip_u16_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 3, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 256];
    let maxes = [255u16, 1023, 4095, 16383, 65535];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        for &m in &maxes {
            let pred = make_pred_u16(n, m);
            let res = make_res_i16(n);
            assert_eq!(
                run_add_clip(&disp, &pred, &res, m),
                scalar_ref_add_clip(&pred, &res, m),
                "n={n} max={m}"
            );
        }
    }
}

#[test]
fn add_clip_u16_detected_dispatch_matches_scalar() {
    let n_cases = [
        0usize, 1, 3, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 1024, 10_001,
    ];
    let maxes = [255u16, 1023, 4095, 16383, 65535];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        for &m in &maxes {
            let pred = make_pred_u16(n, m);
            let res = make_res_i16(n);
            assert_eq!(
                run_add_clip(&detected, &pred, &res, m),
                run_add_clip(&scalar, &pred, &res, m),
                "tier {} diverged at n={n} max={m}",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
            );
        }
    }
}

#[test]
fn add_clip_u16_boundary_values_match_scalar() {
    let max = 1023u16;
    let pred: Vec<u16> = vec![0, 0, 1, 100, max - 1, max, max, max, max, max];
    let res: Vec<i16> = vec![-1, 100, -100, 50, 5, 1, 100, -100, i16::MIN, i16::MAX];
    let mut p = pred.clone();
    let mut r = res.clone();
    while p.len() < 32 {
        p.extend_from_slice(&pred);
        r.extend_from_slice(&res);
    }
    p.truncate(32);
    r.truncate(32);
    let want = scalar_ref_add_clip(&p, &r, max);
    assert_eq!(run_add_clip(&Dispatch::scalar(), &p, &r, max), want);
    assert_eq!(run_add_clip(&Dispatch::detect(), &p, &r, max), want);
}

// =========================================================================
// add_clip_to_pixel_u8 (v0.3.20)
// =========================================================================

fn make_pred_u8(n: usize) -> Vec<u8> {
    (0..n).map(|i| ((i as u32 * 53 + 7) % 256) as u8).collect()
}

/// Residuals spanning negative and large-positive values so the sum with a
/// 0..=255 pred clips at both 0 and 255.
fn make_res_i16_u8(n: usize) -> Vec<i16> {
    (0..n)
        .map(|i| ((i as i32 * 71 + 3) % 801 - 400) as i16)
        .collect()
}

fn scalar_ref_add_clip_u8(pred: &[u8], res: &[i16]) -> Vec<u8> {
    pred.iter()
        .zip(res.iter())
        .map(|(&p, &r)| ((p as i32) + (r as i32)).clamp(0, 255) as u8)
        .collect()
}

fn run_add_clip_u8(disp: &Dispatch, pred: &[u8], res: &[i16]) -> Vec<u8> {
    let mut out = vec![0u8; pred.len()];
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.add_clip_to_pixel_u8)(pred.as_ptr(), res.as_ptr(), out.as_mut_ptr(), pred.len());
    }
    out
}

#[test]
fn add_clip_u8_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 3, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 256];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        let pred = make_pred_u8(n);
        let res = make_res_i16_u8(n);
        assert_eq!(
            run_add_clip_u8(&disp, &pred, &res),
            scalar_ref_add_clip_u8(&pred, &res),
            "n={n}"
        );
    }
}

#[test]
fn add_clip_u8_detected_dispatch_matches_scalar() {
    let n_cases = [
        0usize, 1, 3, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 1024, 10_001,
    ];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        let pred = make_pred_u8(n);
        let res = make_res_i16_u8(n);
        assert_eq!(
            run_add_clip_u8(&detected, &pred, &res),
            run_add_clip_u8(&scalar, &pred, &res),
            "tier {} diverged at n={n}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

#[test]
fn add_clip_u8_boundary_values_match_scalar() {
    // Exercise the i32 accumulation requirement: 255 + i16::MAX = 33022
    // overflows i16, and 0 + i16::MIN underflows — both must clip cleanly.
    let pred: Vec<u8> = vec![0, 0, 1, 128, 254, 255, 255, 255, 200, 100];
    let res: Vec<i16> = vec![-1, 100, -100, 50, 5, 1, 100, -100, i16::MIN, i16::MAX];
    let mut p = pred.clone();
    let mut r = res.clone();
    while p.len() < 48 {
        p.extend_from_slice(&pred);
        r.extend_from_slice(&res);
    }
    p.truncate(48);
    r.truncate(48);
    let want = scalar_ref_add_clip_u8(&p, &r);
    assert_eq!(run_add_clip_u8(&Dispatch::scalar(), &p, &r), want);
    assert_eq!(run_add_clip_u8(&Dispatch::detect(), &p, &r), want);
}

// =========================================================================
// add_clip_to_pixel_u16_inplace / _u8_inplace (v0.3.21)
// =========================================================================
//
// These prove the safe in-place wrappers (`pix` aliased as both prediction
// and destination) are byte-identical to the out-of-place path.  Both the
// golden and the in-place call go through the crate-root safe fns, i.e. the
// process-global `dispatch::get()` tier, so running the whole test binary
// under each `CODEC_SIMD_FORCE_LEVEL` proves `pred == dst` aliasing is sound
// at every SIMD tier (scalar/SSE4.1/AVX2/NEON/wasm).

/// Deterministic LCG (reproducible "random" inputs, no rng dependency).
fn lcg_next(state: &mut u64) -> u32 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (*state >> 33) as u32
}

#[test]
fn add_clip_u16_inplace_matches_out_of_place() {
    // (len, max) covering 8/10/12/14/16-bit pixel ranges and non-multiple-of-
    // vector-width lengths so both the SIMD body and the scalar tail run.
    let cases = [
        (0usize, 255u16),
        (1, 255),
        (7, 255),
        (8, 1023),
        (15, 1023),
        (16, 4095),
        (17, 4095),
        (31, 16383),
        (32, 65535),
        (33, 1023),
        (64, 4095),
        (255, 16383),
        (256, 65535),
        (1000, 1023),
    ];
    let mut state = 0x1234_5678_9abc_def0u64;
    for &(n, max) in &cases {
        // Random prediction in 0..=max; full-range i16 residual so the sum
        // clips at both 0 and max.
        let pix: Vec<u16> = (0..n)
            .map(|_| (lcg_next(&mut state) % (max as u32 + 1)) as u16)
            .collect();
        let res: Vec<i16> = (0..n).map(|_| lcg_next(&mut state) as i16).collect();

        // Out-of-place golden via the existing safe wrapper.
        let mut dst = vec![0u16; n];
        codec_simd::add_clip_to_pixel_u16(&pix, &res, &mut dst, max);

        // In-place path on a clone of the same prediction buffer.
        let mut p = pix.clone();
        codec_simd::add_clip_to_pixel_u16_inplace(&mut p, &res, max);

        assert_eq!(
            p,
            dst,
            "u16 in-place != out-of-place at tier {} (n={n} max={max})",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

#[test]
fn add_clip_u8_inplace_matches_out_of_place() {
    let lens = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 64, 255, 256, 1000];
    let mut state = 0x0fed_cba9_8765_4321u64;
    for &n in &lens {
        let pix: Vec<u8> = (0..n).map(|_| (lcg_next(&mut state) % 256) as u8).collect();
        let res: Vec<i16> = (0..n).map(|_| lcg_next(&mut state) as i16).collect();

        let mut dst = vec![0u8; n];
        codec_simd::add_clip_to_pixel_u8(&pix, &res, &mut dst);

        let mut p = pix.clone();
        codec_simd::add_clip_to_pixel_u8_inplace(&mut p, &res);

        assert_eq!(
            p,
            dst,
            "u8 in-place != out-of-place at tier {} (n={n})",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

// =========================================================================
// clamp_i32_to_i16 (v0.1.3)
// =========================================================================

fn scalar_ref_clamp_i32_to_i16(buf: &[i32]) -> Vec<i32> {
    const MIN: i32 = i16::MIN as i32;
    const MAX: i32 = i16::MAX as i32;
    buf.iter().map(|&v| v.clamp(MIN, MAX)).collect()
}

fn run_clamp_i32_to_i16(disp: &Dispatch, src: &[i32]) -> Vec<i32> {
    let mut out = src.to_vec();
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.clamp_i32_to_i16)(out.as_mut_ptr(), out.len());
    }
    out
}

#[test]
fn clamp_i32_to_i16_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 3, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 256];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        // Exercise both in-range and out-of-range values.
        let src: Vec<i32> = (0..n).map(|i| (i as i32) * 4096 - 100_000).collect();
        assert_eq!(
            run_clamp_i32_to_i16(&disp, &src),
            scalar_ref_clamp_i32_to_i16(&src),
            "n={n}"
        );
    }
}

#[test]
fn clamp_i32_to_i16_detected_dispatch_matches_scalar() {
    let n_cases = [
        0usize, 1, 3, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 1024, 10_001,
    ];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        let src: Vec<i32> = (0..n).map(|i| ((i as i32) * 4096) - 100_000).collect();
        assert_eq!(
            run_clamp_i32_to_i16(&detected, &src),
            run_clamp_i32_to_i16(&scalar, &src),
            "tier {} diverged at n={n}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

#[test]
fn clamp_i32_to_i16_boundary_values_match_scalar() {
    let edge: Vec<i32> = vec![
        i32::MIN,
        -100_000,
        i16::MIN as i32 - 1,
        i16::MIN as i32,
        i16::MIN as i32 + 1,
        -1,
        0,
        1,
        i16::MAX as i32 - 1,
        i16::MAX as i32,
        i16::MAX as i32 + 1,
        100_000,
        i32::MAX,
    ];
    let mut src = edge.clone();
    while src.len() < 64 {
        src.extend_from_slice(&edge);
    }
    src.truncate(64);
    let want = scalar_ref_clamp_i32_to_i16(&src);
    assert_eq!(run_clamp_i32_to_i16(&Dispatch::scalar(), &src), want);
    assert_eq!(run_clamp_i32_to_i16(&Dispatch::detect(), &src), want);
}

// =========================================================================
// abs_diff_u8 (v0.1.4)
// =========================================================================

fn make_pair_u8(n: usize) -> (Vec<u8>, Vec<u8>) {
    let a: Vec<u8> = (0..n).map(|i| ((i as u32 * 37 + 11) % 256) as u8).collect();
    let b: Vec<u8> = (0..n).map(|i| ((i as u32 * 53 + 7) % 256) as u8).collect();
    (a, b)
}

fn scalar_ref_abs_diff(a: &[u8], b: &[u8]) -> Vec<u8> {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| x.abs_diff(y))
        .collect()
}

fn run_abs_diff(disp: &Dispatch, a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; a.len()];
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.abs_diff_u8)(a.as_ptr(), b.as_ptr(), out.as_mut_ptr(), a.len());
    }
    out
}

#[test]
fn abs_diff_u8_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 7, 15, 16, 17, 31, 32, 33, 63, 64, 127, 128, 1024];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        let (a, b) = make_pair_u8(n);
        assert_eq!(
            run_abs_diff(&disp, &a, &b),
            scalar_ref_abs_diff(&a, &b),
            "n={n}"
        );
    }
}

#[test]
fn abs_diff_u8_detected_dispatch_matches_scalar() {
    let n_cases = [
        0usize, 1, 7, 15, 16, 17, 31, 32, 33, 63, 64, 127, 128, 1024, 10_001,
    ];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        let (a, b) = make_pair_u8(n);
        assert_eq!(
            run_abs_diff(&detected, &a, &b),
            run_abs_diff(&scalar, &a, &b),
            "tier {} diverged at n={n}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

#[test]
fn abs_diff_u8_boundary_values_match_scalar() {
    // Hammer the extremes: 0 vs 0, 0 vs 255, 255 vs 0, equal lanes, max diff lanes.
    let a: Vec<u8> = vec![0, 0, 255, 255, 128, 1, 254, 100, 100, 0, 255];
    let b: Vec<u8> = vec![0, 255, 0, 255, 128, 254, 1, 100, 50, 100, 100];
    let mut a_buf = a.clone();
    let mut b_buf = b.clone();
    while a_buf.len() < 64 {
        a_buf.extend_from_slice(&a);
        b_buf.extend_from_slice(&b);
    }
    a_buf.truncate(64);
    b_buf.truncate(64);
    let want = scalar_ref_abs_diff(&a_buf, &b_buf);
    assert_eq!(run_abs_diff(&Dispatch::scalar(), &a_buf, &b_buf), want);
    assert_eq!(run_abs_diff(&Dispatch::detect(), &a_buf, &b_buf), want);
}

// =========================================================================
// mac_widen_i16_to_i32 (v0.1.5)
// =========================================================================

fn make_pair_i16(n: usize) -> (Vec<i16>, Vec<i16>) {
    let a: Vec<i16> = (0..n)
        .map(|i| (((i as i32 * 37 + 11) % 4096) - 2048) as i16)
        .collect();
    let b: Vec<i16> = (0..n)
        .map(|i| (((i as i32 * 53 + 7) % 4096) - 2048) as i16)
        .collect();
    (a, b)
}

fn make_acc_i32(n: usize) -> Vec<i32> {
    (0..n)
        .map(|i| ((i as i32 * 13) % 1_000_000) - 500_000)
        .collect()
}

fn scalar_ref_mac(a: &[i16], b: &[i16], acc: &[i32]) -> Vec<i32> {
    a.iter()
        .zip(b.iter())
        .zip(acc.iter())
        .map(|((&x, &y), &a0)| a0.wrapping_add((x as i32).wrapping_mul(y as i32)))
        .collect()
}

fn run_mac(disp: &Dispatch, a: &[i16], b: &[i16], acc_in: &[i32]) -> Vec<i32> {
    let mut acc = acc_in.to_vec();
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.mac_widen_i16_to_i32)(a.as_ptr(), b.as_ptr(), acc.as_mut_ptr(), a.len());
    }
    acc
}

#[test]
fn mac_widen_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 256];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        let (a, b) = make_pair_i16(n);
        let acc = make_acc_i32(n);
        assert_eq!(
            run_mac(&disp, &a, &b, &acc),
            scalar_ref_mac(&a, &b, &acc),
            "n={n}"
        );
    }
}

#[test]
fn mac_widen_detected_dispatch_matches_scalar() {
    let n_cases = [
        0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 1024, 10_001,
    ];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        let (a, b) = make_pair_i16(n);
        let acc = make_acc_i32(n);
        assert_eq!(
            run_mac(&detected, &a, &b, &acc),
            run_mac(&scalar, &a, &b, &acc),
            "tier {} diverged at n={n}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

#[test]
fn mac_widen_boundary_values_match_scalar() {
    // i16::MIN × i16::MIN = +0x40000000 — large but fits i32.
    // i16::MIN × i16::MAX = -0x3FFF8000 — negative large.
    let a: Vec<i16> = vec![
        i16::MIN,
        i16::MIN,
        i16::MAX,
        i16::MAX,
        0,
        1,
        -1,
        100,
        -100,
        i16::MIN,
        i16::MAX,
    ];
    let b: Vec<i16> = vec![
        i16::MIN,
        i16::MAX,
        i16::MIN,
        i16::MAX,
        0,
        -1,
        1,
        -100,
        100,
        1,
        -1,
    ];
    let mut a_buf = a.clone();
    let mut b_buf = b.clone();
    while a_buf.len() < 64 {
        a_buf.extend_from_slice(&a);
        b_buf.extend_from_slice(&b);
    }
    a_buf.truncate(64);
    b_buf.truncate(64);
    let acc = vec![0i32; a_buf.len()];
    let want = scalar_ref_mac(&a_buf, &b_buf, &acc);
    assert_eq!(run_mac(&Dispatch::scalar(), &a_buf, &b_buf, &acc), want);
    assert_eq!(run_mac(&Dispatch::detect(), &a_buf, &b_buf, &acc), want);
}

// =========================================================================
// transpose_4x4_i16 / transpose_8x8_i16 (v0.1.6)
// =========================================================================

fn make_block_i16(n: usize) -> Vec<i16> {
    (0..n)
        .map(|i| (i as i16).wrapping_mul(17).wrapping_add(3))
        .collect()
}

fn scalar_ref_transpose(src: &[i16], side: usize) -> Vec<i16> {
    let mut dst = vec![0i16; side * side];
    for i in 0..side {
        for j in 0..side {
            dst[i * side + j] = src[j * side + i];
        }
    }
    dst
}

fn run_transpose_4x4(disp: &Dispatch, src: &[i16]) -> Vec<i16> {
    let mut dst = vec![0i16; 16];
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.transpose_4x4_i16)(src.as_ptr(), dst.as_mut_ptr());
    }
    dst
}

fn run_transpose_8x8(disp: &Dispatch, src: &[i16]) -> Vec<i16> {
    let mut dst = vec![0i16; 64];
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.transpose_8x8_i16)(src.as_ptr(), dst.as_mut_ptr());
    }
    dst
}

#[test]
fn transpose_4x4_scalar_matches_naive_reference() {
    let src = make_block_i16(16);
    assert_eq!(
        run_transpose_4x4(&Dispatch::scalar(), &src),
        scalar_ref_transpose(&src, 4)
    );
}

#[test]
fn transpose_4x4_detected_dispatch_matches_scalar() {
    let src = make_block_i16(16);
    let want = run_transpose_4x4(&Dispatch::scalar(), &src);
    assert_eq!(
        run_transpose_4x4(&Dispatch::detect(), &src),
        want,
        "tier {} diverged",
        codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
    );
}

#[test]
fn transpose_4x4_double_yields_identity() {
    // Two transposes return the original.
    let src = make_block_i16(16);
    let once = run_transpose_4x4(&Dispatch::detect(), &src);
    let twice = run_transpose_4x4(&Dispatch::detect(), &once);
    assert_eq!(twice, src);
}

#[test]
fn transpose_8x8_scalar_matches_naive_reference() {
    let src = make_block_i16(64);
    assert_eq!(
        run_transpose_8x8(&Dispatch::scalar(), &src),
        scalar_ref_transpose(&src, 8)
    );
}

#[test]
fn transpose_8x8_detected_dispatch_matches_scalar() {
    let src = make_block_i16(64);
    let want = run_transpose_8x8(&Dispatch::scalar(), &src);
    assert_eq!(
        run_transpose_8x8(&Dispatch::detect(), &src),
        want,
        "tier {} diverged",
        codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
    );
}

#[test]
fn transpose_8x8_double_yields_identity() {
    let src = make_block_i16(64);
    let once = run_transpose_8x8(&Dispatch::detect(), &src);
    let twice = run_transpose_8x8(&Dispatch::detect(), &once);
    assert_eq!(twice, src);
}

// =========================================================================
// sat_add_i16 / sat_sub_i16 / abs_i16 (v0.1.7)
// =========================================================================

fn make_pair_i16_full(n: usize) -> (Vec<i16>, Vec<i16>) {
    // Mix in extreme values so saturation paths get hit.
    let a: Vec<i16> = (0..n)
        .map(|i| match i % 7 {
            0 => i16::MAX,
            1 => i16::MIN,
            2 => -1,
            _ => (((i as i32 * 37 + 11) % 65536) - 32768) as i16,
        })
        .collect();
    let b: Vec<i16> = (0..n)
        .map(|i| match i % 5 {
            0 => i16::MIN,
            1 => i16::MAX,
            2 => 1,
            _ => (((i as i32 * 53 + 7) % 65536) - 32768) as i16,
        })
        .collect();
    (a, b)
}

fn scalar_ref_sat_add(a: &[i16], b: &[i16]) -> Vec<i16> {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| ((x as i32) + (y as i32)).clamp(i16::MIN as i32, i16::MAX as i32) as i16)
        .collect()
}

fn scalar_ref_sat_sub(a: &[i16], b: &[i16]) -> Vec<i16> {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| ((x as i32) - (y as i32)).clamp(i16::MIN as i32, i16::MAX as i32) as i16)
        .collect()
}

fn scalar_ref_abs_i16(src: &[i16]) -> Vec<i16> {
    src.iter()
        .map(|&v| if v == i16::MIN { i16::MAX } else { v.abs() })
        .collect()
}

fn run_sat_add(disp: &Dispatch, a: &[i16], b: &[i16]) -> Vec<i16> {
    let mut out = vec![0i16; a.len()];
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.sat_add_i16)(a.as_ptr(), b.as_ptr(), out.as_mut_ptr(), a.len());
    }
    out
}

fn run_sat_sub(disp: &Dispatch, a: &[i16], b: &[i16]) -> Vec<i16> {
    let mut out = vec![0i16; a.len()];
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.sat_sub_i16)(a.as_ptr(), b.as_ptr(), out.as_mut_ptr(), a.len());
    }
    out
}

fn run_abs_i16(disp: &Dispatch, src: &[i16]) -> Vec<i16> {
    let mut out = vec![0i16; src.len()];
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.abs_i16)(src.as_ptr(), out.as_mut_ptr(), src.len());
    }
    out
}

#[test]
fn sat_add_i16_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 64, 256];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        let (a, b) = make_pair_i16_full(n);
        assert_eq!(
            run_sat_add(&disp, &a, &b),
            scalar_ref_sat_add(&a, &b),
            "n={n}"
        );
    }
}

#[test]
fn sat_add_i16_detected_dispatch_matches_scalar() {
    let n_cases = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 1024];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        let (a, b) = make_pair_i16_full(n);
        assert_eq!(
            run_sat_add(&detected, &a, &b),
            run_sat_add(&scalar, &a, &b),
            "tier {} diverged at n={n}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

#[test]
fn sat_sub_i16_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 64, 256];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        let (a, b) = make_pair_i16_full(n);
        assert_eq!(
            run_sat_sub(&disp, &a, &b),
            scalar_ref_sat_sub(&a, &b),
            "n={n}"
        );
    }
}

#[test]
fn sat_sub_i16_detected_dispatch_matches_scalar() {
    let n_cases = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 1024];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        let (a, b) = make_pair_i16_full(n);
        assert_eq!(
            run_sat_sub(&detected, &a, &b),
            run_sat_sub(&scalar, &a, &b),
            "tier {} diverged at n={n}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

#[test]
fn abs_i16_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 64, 256];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        let (a, _b) = make_pair_i16_full(n);
        assert_eq!(run_abs_i16(&disp, &a), scalar_ref_abs_i16(&a), "n={n}");
    }
}

#[test]
fn abs_i16_detected_dispatch_matches_scalar() {
    let n_cases = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 1024];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        let (a, _b) = make_pair_i16_full(n);
        assert_eq!(
            run_abs_i16(&detected, &a),
            run_abs_i16(&scalar, &a),
            "tier {} diverged at n={n}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

// =========================================================================
// mul_hi_round_i16 (v0.1.8)
// =========================================================================

fn scalar_ref_mul_hi_round(a: &[i16], b: &[i16]) -> Vec<i16> {
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| {
            let p = (x as i32).wrapping_mul(y as i32).wrapping_add(1 << 14) >> 15;
            p.clamp(i16::MIN as i32, i16::MAX as i32) as i16
        })
        .collect()
}

fn run_mul_hi_round(disp: &Dispatch, a: &[i16], b: &[i16]) -> Vec<i16> {
    let mut out = vec![0i16; a.len()];
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.mul_hi_round_i16)(a.as_ptr(), b.as_ptr(), out.as_mut_ptr(), a.len());
    }
    out
}

#[test]
fn mul_hi_round_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 64, 256];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        let (a, b) = make_pair_i16_full(n);
        assert_eq!(
            run_mul_hi_round(&disp, &a, &b),
            scalar_ref_mul_hi_round(&a, &b),
            "n={n}"
        );
    }
}

#[test]
fn mul_hi_round_detected_dispatch_matches_scalar() {
    let n_cases = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 1024];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        let (a, b) = make_pair_i16_full(n);
        assert_eq!(
            run_mul_hi_round(&detected, &a, &b),
            run_mul_hi_round(&scalar, &a, &b),
            "tier {} diverged at n={n}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

#[test]
fn mul_hi_round_boundary_min_squared_saturates() {
    // i16::MIN × i16::MIN = +2^30 + 1<<14 = 2^30 + 16384.
    // Shifted right by 15: 32768 + 0 = 32768 -> saturate to 32767.
    let a = vec![i16::MIN; 16];
    let b = vec![i16::MIN; 16];
    let want = vec![i16::MAX; 16];
    assert_eq!(run_mul_hi_round(&Dispatch::scalar(), &a, &b), want);
    assert_eq!(run_mul_hi_round(&Dispatch::detect(), &a, &b), want);
}

// =========================================================================
// round_shift_narrow_i32_to_i16 (v0.1.9)
// =========================================================================

fn make_input_i32_wide(n: usize) -> Vec<i32> {
    (0..n)
        .map(|i| match i % 7 {
            0 => 1_000_000,
            1 => -1_000_000,
            2 => i32::MAX / 2,
            3 => i32::MIN / 2,
            _ => ((i as i32 * 12345) % 200_000) - 100_000,
        })
        .collect()
}

fn scalar_ref_round_shift_narrow(src: &[i32], shift: u32) -> Vec<i16> {
    let round = 1i32 << (shift - 1);
    src.iter()
        .map(|&v| {
            let r = v.wrapping_add(round) >> shift;
            r.clamp(i16::MIN as i32, i16::MAX as i32) as i16
        })
        .collect()
}

fn run_round_shift_narrow(disp: &Dispatch, src: &[i32], shift: u32) -> Vec<i16> {
    let mut out = vec![0i16; src.len()];
    // SAFETY: see run_i16_to_u8.
    unsafe {
        (disp.round_shift_narrow_i32_to_i16)(src.as_ptr(), out.as_mut_ptr(), src.len(), shift);
    }
    out
}

#[test]
fn round_shift_narrow_scalar_matches_naive_reference() {
    let n_cases = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 64, 256];
    let shifts = [1u32, 4, 6, 12, 15, 20];
    let disp = Dispatch::scalar();
    for &n in &n_cases {
        for &shift in &shifts {
            let src = make_input_i32_wide(n);
            assert_eq!(
                run_round_shift_narrow(&disp, &src, shift),
                scalar_ref_round_shift_narrow(&src, shift),
                "n={n} shift={shift}",
            );
        }
    }
}

#[test]
fn round_shift_narrow_detected_dispatch_matches_scalar() {
    let n_cases = [0usize, 1, 7, 8, 15, 16, 17, 31, 32, 33, 1024];
    let shifts = [1u32, 4, 6, 12, 15];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &n_cases {
        for &shift in &shifts {
            let src = make_input_i32_wide(n);
            assert_eq!(
                run_round_shift_narrow(&detected, &src, shift),
                run_round_shift_narrow(&scalar, &src, shift),
                "tier {} diverged at n={n} shift={shift}",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
            );
        }
    }
}

#[test]
fn round_shift_narrow_saturates_large_values() {
    // Large positive should saturate to i16::MAX, large negative to i16::MIN
    // after the shift.
    let src: Vec<i32> = vec![i32::MAX, i32::MIN, 100_000_000, -100_000_000];
    let mut src_buf = src.clone();
    while src_buf.len() < 32 {
        src_buf.extend_from_slice(&src);
    }
    src_buf.truncate(32);
    let want = scalar_ref_round_shift_narrow(&src_buf, 6);
    assert_eq!(
        run_round_shift_narrow(&Dispatch::scalar(), &src_buf, 6),
        want
    );
    assert_eq!(
        run_round_shift_narrow(&Dispatch::detect(), &src_buf, 6),
        want
    );
}

// =========================================================================
// sad_block_u8 (v0.2.0) — first Tier-2 block kernel
// =========================================================================

fn make_plane_u8(rows: usize, stride: usize, seed: u32) -> Vec<u8> {
    let n = rows * stride;
    (0..n)
        .map(|i| ((i as u32 * 37 + seed).wrapping_mul(13) % 256) as u8)
        .collect()
}

fn scalar_ref_sad(a: &[u8], a_stride: usize, b: &[u8], b_stride: usize, w: usize, h: usize) -> u32 {
    let mut sum: u32 = 0;
    for row in 0..h {
        for col in 0..w {
            sum += a[row * a_stride + col].abs_diff(b[row * b_stride + col]) as u32;
        }
    }
    sum
}

fn run_sad(
    disp: &Dispatch,
    a: &[u8],
    a_stride: isize,
    b: &[u8],
    b_stride: isize,
    w: usize,
    h: usize,
) -> u32 {
    // SAFETY: caller passes valid slices with sufficient capacity.
    unsafe { (disp.sad_block_u8)(a.as_ptr(), a_stride, b.as_ptr(), b_stride, w, h) }
}

#[test]
fn sad_block_u8_scalar_matches_naive_reference() {
    // Test all the HEVC block size pairs.
    let sizes = [
        (4, 4),
        (4, 8),
        (8, 4),
        (8, 8),
        (8, 16),
        (16, 8),
        (16, 16),
        (16, 32),
        (32, 16),
        (32, 32),
        (32, 64),
        (64, 32),
        (64, 64),
    ];
    let stride = 128usize; // wider than any block — exercises stride mismatch
    let disp = Dispatch::scalar();
    for &(w, h) in &sizes {
        let a = make_plane_u8(64, stride, 1);
        let b = make_plane_u8(64, stride, 2);
        let want = scalar_ref_sad(&a, stride, &b, stride, w, h);
        let got = run_sad(&disp, &a, stride as isize, &b, stride as isize, w, h);
        assert_eq!(got, want, "scalar diverged at w={w} h={h}");
    }
}

#[test]
fn sad_block_u8_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (4, 8),
        (8, 4),
        (8, 8),
        (8, 16),
        (16, 8),
        (16, 16),
        (16, 32),
        (32, 16),
        (32, 32),
        (32, 64),
        (64, 32),
        (64, 64),
        // Non-power-of-2 widths to exercise tail paths.
        (12, 8),
        (24, 16),
        (40, 32),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(w, h) in &sizes {
        let a = make_plane_u8(72, stride, 3);
        let b = make_plane_u8(72, stride, 7);
        let want = run_sad(&scalar, &a, stride as isize, &b, stride as isize, w, h);
        let got = run_sad(&detected, &a, stride as isize, &b, stride as isize, w, h);
        assert_eq!(
            got,
            want,
            "tier {} diverged at w={w} h={h}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
        );
    }
}

#[test]
fn sad_block_u8_zero_diff_is_zero() {
    let a = make_plane_u8(64, 64, 99);
    let want = 0u32;
    let got = run_sad(&Dispatch::detect(), &a, 64, &a, 64, 64, 64);
    assert_eq!(got, want);
}

#[test]
fn sad_block_u8_max_diff_is_full() {
    // All a=0 vs all b=255 over 16×16 = 16*16*255 = 65280
    let a = vec![0u8; 256];
    let b = vec![255u8; 256];
    let want = (16 * 16 * 255) as u32;
    assert_eq!(run_sad(&Dispatch::scalar(), &a, 16, &b, 16, 16, 16), want);
    assert_eq!(run_sad(&Dispatch::detect(), &a, 16, &b, 16, 16, 16), want);
}

// =========================================================================
// satd_block_4x4_u8 (v0.2.1) — Hadamard-transformed SAD on 4×4 block
// =========================================================================

fn scalar_ref_satd_4x4(a: &[u8], a_stride: usize, b: &[u8], b_stride: usize) -> u32 {
    // Step 1: diff
    let mut d = [0i16; 16];
    for r in 0..4 {
        for c in 0..4 {
            d[r * 4 + c] = a[r * a_stride + c] as i16 - b[r * b_stride + c] as i16;
        }
    }
    // Step 2: horizontal Hadamard
    let mut h = [0i16; 16];
    for r in 0..4 {
        let r0 = d[r * 4];
        let r1 = d[r * 4 + 1];
        let r2 = d[r * 4 + 2];
        let r3 = d[r * 4 + 3];
        let t0 = r0 + r1;
        let t1 = r0 - r1;
        let t2 = r2 + r3;
        let t3 = r2 - r3;
        h[r * 4] = t0 + t2;
        h[r * 4 + 1] = t1 + t3;
        h[r * 4 + 2] = t0 - t2;
        h[r * 4 + 3] = t1 - t3;
    }
    // Step 3: vertical Hadamard
    let mut v = [0i16; 16];
    for c in 0..4 {
        let r0 = h[c];
        let r1 = h[4 + c];
        let r2 = h[8 + c];
        let r3 = h[12 + c];
        let t0 = r0 + r1;
        let t1 = r0 - r1;
        let t2 = r2 + r3;
        let t3 = r2 - r3;
        v[c] = t0 + t2;
        v[4 + c] = t1 + t3;
        v[8 + c] = t0 - t2;
        v[12 + c] = t1 - t3;
    }
    let mut sum: u32 = 0;
    for &val in v.iter() {
        sum += (val as i32).unsigned_abs();
    }
    (sum + 1) >> 1
}

fn run_satd_4x4(disp: &Dispatch, a: &[u8], a_stride: isize, b: &[u8], b_stride: isize) -> u32 {
    // SAFETY: caller provides slices with sufficient capacity.
    unsafe { (disp.satd_block_4x4_u8)(a.as_ptr(), a_stride, b.as_ptr(), b_stride) }
}

#[test]
fn satd_4x4_scalar_matches_naive_reference() {
    let stride = 16usize;
    let disp = Dispatch::scalar();
    for seed_a in 0..5u32 {
        for seed_b in 0..5u32 {
            let a = make_plane_u8(8, stride, seed_a + 11);
            let b = make_plane_u8(8, stride, seed_b + 37);
            let want = scalar_ref_satd_4x4(&a, stride, &b, stride);
            let got = run_satd_4x4(&disp, &a, stride as isize, &b, stride as isize);
            assert_eq!(got, want, "scalar diverged at seeds=({seed_a}, {seed_b})");
        }
    }
}

#[test]
fn satd_4x4_detected_dispatch_matches_scalar() {
    let stride = 16usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for seed_a in 0..7u32 {
        for seed_b in 0..7u32 {
            let a = make_plane_u8(8, stride, seed_a + 5);
            let b = make_plane_u8(8, stride, seed_b + 29);
            assert_eq!(
                run_satd_4x4(&detected, &a, stride as isize, &b, stride as isize),
                run_satd_4x4(&scalar, &a, stride as isize, &b, stride as isize),
                "tier {} diverged at seeds=({seed_a}, {seed_b})",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
            );
        }
    }
}

#[test]
fn satd_4x4_zero_diff_is_zero() {
    let a = make_plane_u8(8, 16, 42);
    assert_eq!(run_satd_4x4(&Dispatch::detect(), &a, 16, &a, 16), 0);
}

#[test]
fn satd_4x4_unaligned_stride_matches_scalar() {
    // Odd strides + an odd base offset give row pointers with no 4-byte alignment;
    // the SIMD 4-byte row loads must use unaligned reads (no UB / debug abort).
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for stride in [7usize, 13, 15, 17] {
        let a = make_plane_u8(8, stride, 3);
        let b = make_plane_u8(8, stride, 91);
        for off in [0usize, 1, 3] {
            assert_eq!(
                run_satd_4x4(
                    &detected,
                    &a[off..],
                    stride as isize,
                    &b[off..],
                    stride as isize
                ),
                run_satd_4x4(
                    &scalar,
                    &a[off..],
                    stride as isize,
                    &b[off..],
                    stride as isize
                ),
                "stride={stride} off={off}",
            );
        }
    }
}

// =========================================================================
// satd_block_8x8_u8 — Hadamard-transformed SAD on 8x8 block, (sum+2)>>2
// =========================================================================

/// Independent naive reference: full 8x8 Sylvester Hadamard matrix multiply
/// H*D*H (NOT a butterfly), cross-checking the implementation's fast WHT.
/// H[i][j] = (-1)^popcount(i&j).
fn scalar_ref_satd_8x8(a: &[u8], a_stride: usize, b: &[u8], b_stride: usize) -> u32 {
    let h = |i: usize, j: usize| -> i32 { if (i & j).count_ones() & 1 == 1 { -1 } else { 1 } };
    let mut d = [[0i32; 8]; 8];
    for r in 0..8 {
        for c in 0..8 {
            d[r][c] = a[r * a_stride + c] as i32 - b[r * b_stride + c] as i32;
        }
    }
    let mut t = [[0i32; 8]; 8];
    for (i, ti) in t.iter_mut().enumerate() {
        for (c, tic) in ti.iter_mut().enumerate() {
            let mut s = 0i32;
            for (k, dk) in d.iter().enumerate() {
                s += h(i, k) * dk[c];
            }
            *tic = s;
        }
    }
    let mut sum: u32 = 0;
    for ti in t.iter() {
        for j in 0..8 {
            let mut s = 0i32;
            for (k, &tik) in ti.iter().enumerate() {
                s += tik * h(k, j);
            }
            sum += s.unsigned_abs();
        }
    }
    (sum + 2) >> 2
}

fn run_satd_8x8(disp: &Dispatch, a: &[u8], a_stride: isize, b: &[u8], b_stride: isize) -> u32 {
    // SAFETY: caller provides slices with sufficient capacity.
    unsafe { (disp.satd_block_8x8_u8)(a.as_ptr(), a_stride, b.as_ptr(), b_stride) }
}

#[test]
fn satd_8x8_scalar_matches_naive_reference() {
    let stride = 16usize;
    let disp = Dispatch::scalar();
    for seed_a in 0..5u32 {
        for seed_b in 0..5u32 {
            let a = make_plane_u8(8, stride, seed_a + 11);
            let b = make_plane_u8(8, stride, seed_b + 37);
            let want = scalar_ref_satd_8x8(&a, stride, &b, stride);
            let got = run_satd_8x8(&disp, &a, stride as isize, &b, stride as isize);
            assert_eq!(got, want, "scalar diverged at seeds=({seed_a}, {seed_b})");
        }
    }
}

#[test]
fn satd_8x8_detected_dispatch_matches_scalar() {
    let stride = 16usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for seed_a in 0..7u32 {
        for seed_b in 0..7u32 {
            let a = make_plane_u8(8, stride, seed_a + 5);
            let b = make_plane_u8(8, stride, seed_b + 29);
            assert_eq!(
                run_satd_8x8(&detected, &a, stride as isize, &b, stride as isize),
                run_satd_8x8(&scalar, &a, stride as isize, &b, stride as isize),
                "tier {} diverged at seeds=({seed_a}, {seed_b})",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
            );
        }
    }
}

#[test]
fn satd_8x8_zero_diff_is_zero() {
    let a = make_plane_u8(8, 16, 42);
    assert_eq!(run_satd_8x8(&Dispatch::detect(), &a, 16, &a, 16), 0);
}

// =========================================================================
// sse_block_u8 (v0.2.3) — sum of squared errors
// =========================================================================

fn scalar_ref_sse(a: &[u8], a_stride: usize, b: &[u8], b_stride: usize, w: usize, h: usize) -> u64 {
    let mut sum: u64 = 0;
    for row in 0..h {
        for col in 0..w {
            let av = a[row * a_stride + col] as i32;
            let bv = b[row * b_stride + col] as i32;
            let d = av - bv;
            sum += (d * d) as u64;
        }
    }
    sum
}

fn run_sse(
    disp: &Dispatch,
    a: &[u8],
    a_stride: isize,
    b: &[u8],
    b_stride: isize,
    w: usize,
    h: usize,
) -> u64 {
    // SAFETY: caller provides slices with sufficient capacity.
    unsafe { (disp.sse_block_u8)(a.as_ptr(), a_stride, b.as_ptr(), b_stride, w, h) }
}

#[test]
fn sse_block_u8_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (4, 8),
        (8, 4),
        (8, 8),
        (8, 16),
        (16, 8),
        (16, 16),
        (16, 32),
        (32, 16),
        (32, 32),
        (32, 64),
        (64, 32),
        (64, 64),
        (12, 8),
        (24, 16),
        (40, 32),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for &(w, h) in &sizes {
        let a = make_plane_u8(72, stride, 17);
        let b = make_plane_u8(72, stride, 23);
        let want = scalar_ref_sse(&a, stride, &b, stride, w, h);
        let got = run_sse(&disp, &a, stride as isize, &b, stride as isize, w, h);
        assert_eq!(got, want, "scalar diverged at w={w} h={h}");
    }
}

#[test]
fn sse_block_u8_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (16, 16),
        (32, 32),
        (64, 64),
        (12, 8),
        (24, 16),
        (40, 32),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(w, h) in &sizes {
        let a = make_plane_u8(72, stride, 41);
        let b = make_plane_u8(72, stride, 67);
        let want = run_sse(&scalar, &a, stride as isize, &b, stride as isize, w, h);
        let got = run_sse(&detected, &a, stride as isize, &b, stride as isize, w, h);
        assert_eq!(
            got,
            want,
            "tier {} diverged at w={w} h={h}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
        );
    }
}

#[test]
fn sse_block_u8_zero_diff_is_zero() {
    let a = make_plane_u8(64, 64, 99);
    assert_eq!(run_sse(&Dispatch::detect(), &a, 64, &a, 64, 64, 64), 0);
}

#[test]
fn sse_block_u8_max_diff_squared() {
    // All a=0, all b=255 → 16×16 of 255² = 16*16*65025 = 16646400
    let a = vec![0u8; 256];
    let b = vec![255u8; 256];
    let want = 16u64 * 16 * 65025;
    assert_eq!(run_sse(&Dispatch::scalar(), &a, 16, &b, 16, 16, 16), want);
    assert_eq!(run_sse(&Dispatch::detect(), &a, 16, &b, 16, 16, 16), want);
}

// =========================================================================
// mc_avg_u8 (v0.2.4) — rounding halving add (bipred)
// =========================================================================

fn scalar_ref_mc_avg(
    a: &[u8],
    a_stride: usize,
    b: &[u8],
    b_stride: usize,
    w: usize,
    h: usize,
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    for row in 0..h {
        for col in 0..w {
            let av = a[row * a_stride + col] as u16;
            let bv = b[row * b_stride + col] as u16;
            dst[row * w + col] = ((av + bv + 1) >> 1) as u8;
        }
    }
    dst
}

fn run_mc_avg(
    disp: &Dispatch,
    a: &[u8],
    a_stride: isize,
    b: &[u8],
    b_stride: isize,
    w: usize,
    h: usize,
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    // SAFETY: caller provides valid slices.
    unsafe {
        (disp.mc_avg_u8)(
            a.as_ptr(),
            a_stride,
            b.as_ptr(),
            b_stride,
            dst.as_mut_ptr(),
            w as isize,
            w,
            h,
        );
    }
    dst
}

#[test]
fn mc_avg_u8_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (8, 8),
        (16, 16),
        (32, 32),
        (64, 64),
        (8, 16),
        (16, 8),
        (12, 8),
        (24, 16),
        (40, 32),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for &(w, h) in &sizes {
        let a = make_plane_u8(72, stride, 11);
        let b = make_plane_u8(72, stride, 29);
        let want = scalar_ref_mc_avg(&a, stride, &b, stride, w, h);
        let got = run_mc_avg(&disp, &a, stride as isize, &b, stride as isize, w, h);
        assert_eq!(got, want, "scalar diverged at w={w} h={h}");
    }
}

#[test]
fn mc_avg_u8_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (16, 16),
        (32, 32),
        (64, 64),
        (12, 8),
        (24, 16),
        (40, 32),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(w, h) in &sizes {
        let a = make_plane_u8(72, stride, 43);
        let b = make_plane_u8(72, stride, 71);
        let want = run_mc_avg(&scalar, &a, stride as isize, &b, stride as isize, w, h);
        let got = run_mc_avg(&detected, &a, stride as isize, &b, stride as isize, w, h);
        assert_eq!(
            got,
            want,
            "tier {} diverged at w={w} h={h}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
        );
    }
}

#[test]
fn mc_avg_u8_rounding_semantics() {
    // avg(254, 1) should round to 128 (not 127 — ties away from zero).
    // avg(255, 0) = ((255 + 0 + 1) >> 1) = 128.
    let a: Vec<u8> = vec![
        0, 1, 255, 254, 128, 129, 100, 101, 1, 2, 200, 201, 50, 51, 0, 255,
    ];
    let b: Vec<u8> = vec![
        1, 0, 0, 1, 129, 128, 101, 100, 2, 1, 201, 200, 51, 50, 255, 0,
    ];
    let want = scalar_ref_mc_avg(&a, 16, &b, 16, 16, 1);
    assert_eq!(run_mc_avg(&Dispatch::scalar(), &a, 16, &b, 16, 16, 1), want);
    assert_eq!(run_mc_avg(&Dispatch::detect(), &a, 16, &b, 16, 16, 1), want);
    // Sanity: avg(255, 0) = 128.
    assert_eq!(want[14], 128);
}

// =========================================================================
// variance_block_u8 (v0.2.5) — sum + sum-of-squares
// =========================================================================

fn scalar_ref_variance(src: &[u8], stride: usize, w: usize, h: usize) -> (u32, u64) {
    let mut sum: u32 = 0;
    let mut sum_sq: u64 = 0;
    for row in 0..h {
        for col in 0..w {
            let v = src[row * stride + col] as u32;
            sum += v;
            sum_sq += (v * v) as u64;
        }
    }
    (sum, sum_sq)
}

fn run_variance(disp: &Dispatch, src: &[u8], stride: isize, w: usize, h: usize) -> (u32, u64) {
    // SAFETY: caller provides a slice with sufficient capacity.
    unsafe { (disp.variance_block_u8)(src.as_ptr(), stride, w, h) }
}

#[test]
fn variance_block_u8_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (4, 8),
        (8, 4),
        (8, 8),
        (8, 16),
        (16, 8),
        (16, 16),
        (16, 32),
        (32, 16),
        (32, 32),
        (32, 64),
        (64, 32),
        (64, 64),
        (12, 8),
        (24, 16),
        (40, 32),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for &(w, h) in &sizes {
        let src = make_plane_u8(72, stride, 19);
        let want = scalar_ref_variance(&src, stride, w, h);
        let got = run_variance(&disp, &src, stride as isize, w, h);
        assert_eq!(got, want, "scalar diverged at w={w} h={h}");
    }
}

#[test]
fn variance_block_u8_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (16, 16),
        (32, 32),
        (64, 64),
        (12, 8),
        (24, 16),
        (40, 32),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(w, h) in &sizes {
        let src = make_plane_u8(72, stride, 53);
        let want = run_variance(&scalar, &src, stride as isize, w, h);
        let got = run_variance(&detected, &src, stride as isize, w, h);
        assert_eq!(
            got,
            want,
            "tier {} diverged at w={w} h={h}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
        );
    }
}

#[test]
fn variance_block_u8_boundary_values() {
    // All-zero block → (0, 0).
    let zeros = vec![0u8; 256];
    assert_eq!(
        run_variance(&Dispatch::scalar(), &zeros, 16, 16, 16),
        (0, 0)
    );
    assert_eq!(
        run_variance(&Dispatch::detect(), &zeros, 16, 16, 16),
        (0, 0)
    );

    // All-255 16×16 → sum = 256×255 = 65280, sum_sq = 256×255² = 16646400.
    let ff = vec![255u8; 256];
    let want = (256u32 * 255, 256u64 * 65025);
    assert_eq!(run_variance(&Dispatch::scalar(), &ff, 16, 16, 16), want);
    assert_eq!(run_variance(&Dispatch::detect(), &ff, 16, 16, 16), want);

    // A constant block has zero variance: sum_sq == sum² / (w*h).
    let (sum, sum_sq) = run_variance(&Dispatch::detect(), &ff, 16, 16, 16);
    assert_eq!(sum_sq, (sum as u64 * sum as u64) / 256);

    // Mixed 0/255 lanes — exercises the widen/madd square path with
    // extreme per-lane contributions, plus the scalar tail at odd widths.
    let mut mixed = vec![0u8; 256];
    for (i, m) in mixed.iter_mut().enumerate() {
        *m = if i % 2 == 0 { 0 } else { 255 };
    }
    for &(w, h) in &[(16usize, 16usize), (15, 16), (13, 11), (7, 7)] {
        let want_m = scalar_ref_variance(&mixed, 16, w, h);
        assert_eq!(
            run_variance(&Dispatch::detect(), &mixed, 16, w, h),
            want_m,
            "mixed diverged at w={w} h={h}"
        );
    }
}

// =========================================================================
// mc_8tap_h_u8 (v0.3.0) — horizontal 8-tap motion compensation
// =========================================================================

/// Realistic + adversarial 8-tap kernels.  The first four sum to 128
/// (the >>7 normalization); the rest stress saturation and the full
/// widening-multiply path.
const MC_FILTERS: &[[i16; 8]] = &[
    [0, 0, 0, 128, 0, 0, 0, 0],               // identity (tap at position 3)
    [-1, 3, -10, 122, 18, -6, 2, 0],          // VP9-like EIGHTTAP, sums to 128
    [-2, 6, -18, 76, 84, -20, 4, -2],         // half-pel, sums to 128
    [16, 16, 16, 16, 16, 16, 16, 16],         // box blur, sums to 128
    [255, -255, 127, -128, 64, -64, 32, -32], // extreme taps (|tap*src| huge)
];

fn scalar_ref_mc_8tap(
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    for row in 0..h {
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                acc += src[row * src_stride + col + i] as i32 * filter[i] as i32;
            }
            dst[row * w + col] = ((acc + 64) >> 7).clamp(0, 255) as u8;
        }
    }
    dst
}

fn run_mc_8tap(
    disp: &Dispatch,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    // SAFETY: src has >= (h-1)*stride + w + 7 bytes; dst is exactly h*w.
    unsafe {
        (disp.mc_8tap_h_u8)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
        );
    }
    dst
}

#[test]
fn mc_8tap_h_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (8, 4),
        (8, 8),
        (12, 8),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 4),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for f in MC_FILTERS {
        for &(w, h) in &sizes {
            let src = make_plane_u8(72, stride, 31);
            let want = scalar_ref_mc_8tap(&src, stride, w, h, f);
            let got = run_mc_8tap(&disp, &src, stride as isize, w, h, f);
            assert_eq!(got, want, "scalar diverged at w={w} h={h} filter={f:?}");
        }
    }
}

#[test]
fn mc_8tap_h_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (24, 16),
        (32, 32),
        (40, 8),
        (48, 4),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC_FILTERS {
        for &(w, h) in &sizes {
            let src = make_plane_u8(72, stride, 97);
            let want = run_mc_8tap(&scalar, &src, stride as isize, w, h, f);
            let got = run_mc_8tap(&detected, &src, stride as isize, w, h, f);
            assert_eq!(
                got,
                want,
                "tier {} diverged at w={w} h={h} filter={f:?}",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
            );
        }
    }
}

#[test]
fn mc_8tap_h_identity_and_saturation() {
    let stride = 128usize;
    let src = make_plane_u8(8, stride, 5);
    // Identity tap at position 3 → dst[c] == src[c+3].
    let id = [0i16, 0, 0, 128, 0, 0, 0, 0];
    let out = run_mc_8tap(&Dispatch::detect(), &src, stride as isize, 16, 4, &id);
    for row in 0..4 {
        for col in 0..16 {
            assert_eq!(
                out[row * 16 + col],
                src[row * stride + col + 3],
                "identity mismatch at r={row} c={col}"
            );
        }
    }
    // Saturate high: taps sum to 256 over a bright plane → clamps to 255.
    let bright = vec![200u8; 8 * stride];
    let hi = [128i16, 128, 0, 0, 0, 0, 0, 0];
    let out_hi = run_mc_8tap(&Dispatch::detect(), &bright, stride as isize, 16, 2, &hi);
    assert!(
        out_hi.iter().all(|&v| v == 255),
        "expected clamp-high to 255"
    );
    // Saturate low: negative taps → negative acc → clamps to 0.
    let lo = [-128i16, -128, 0, 0, 0, 0, 0, 0];
    let out_lo = run_mc_8tap(&Dispatch::detect(), &bright, stride as isize, 16, 2, &lo);
    assert!(out_lo.iter().all(|&v| v == 0), "expected clamp-low to 0");
    // Extreme taps + odd widths: detected must match scalar bit-for-bit.
    let extreme = [255i16, -255, 127, -128, 64, -64, 32, -32];
    let s2 = make_plane_u8(8, stride, 211);
    for &(w, h) in &[(16usize, 4usize), (13, 3), (8, 5), (4, 2)] {
        let a = run_mc_8tap(&Dispatch::scalar(), &s2, stride as isize, w, h, &extreme);
        let b = run_mc_8tap(&Dispatch::detect(), &s2, stride as isize, w, h, &extreme);
        assert_eq!(a, b, "extreme-tap divergence at w={w} h={h}");
    }
}

// =========================================================================
// mc_8tap_v_u8 (v0.3.1) — vertical 8-tap motion compensation
// =========================================================================

fn scalar_ref_mc_8tap_v(
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    for row in 0..h {
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                acc += src[(row + i) * src_stride + col] as i32 * filter[i] as i32;
            }
            dst[row * w + col] = ((acc + 64) >> 7).clamp(0, 255) as u8;
        }
    }
    dst
}

fn run_mc_8tap_v(
    disp: &Dispatch,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    // SAFETY: src has >= (h+6)*stride + w bytes; dst is exactly h*w.
    unsafe {
        (disp.mc_8tap_v_u8)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
        );
    }
    dst
}

#[test]
fn mc_8tap_v_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (8, 4),
        (8, 8),
        (12, 8),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 4),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for f in MC_FILTERS {
        for &(w, h) in &sizes {
            let src = make_plane_u8(72, stride, 37);
            let want = scalar_ref_mc_8tap_v(&src, stride, w, h, f);
            let got = run_mc_8tap_v(&disp, &src, stride as isize, w, h, f);
            assert_eq!(got, want, "scalar diverged at w={w} h={h} filter={f:?}");
        }
    }
}

#[test]
fn mc_8tap_v_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (24, 16),
        (32, 32),
        (40, 8),
        (48, 4),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC_FILTERS {
        for &(w, h) in &sizes {
            let src = make_plane_u8(72, stride, 89);
            let want = run_mc_8tap_v(&scalar, &src, stride as isize, w, h, f);
            let got = run_mc_8tap_v(&detected, &src, stride as isize, w, h, f);
            assert_eq!(
                got,
                want,
                "tier {} diverged at w={w} h={h} filter={f:?}",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
            );
        }
    }
}

#[test]
fn mc_8tap_v_identity_and_saturation() {
    let stride = 128usize;
    let src = make_plane_u8(72, stride, 7);
    // Identity tap at position 3 → dst[r][c] == src[(r+3)*stride + c].
    let id = [0i16, 0, 0, 128, 0, 0, 0, 0];
    let out = run_mc_8tap_v(&Dispatch::detect(), &src, stride as isize, 16, 8, &id);
    for row in 0..8 {
        for col in 0..16 {
            assert_eq!(
                out[row * 16 + col],
                src[(row + 3) * stride + col],
                "identity mismatch at r={row} c={col}"
            );
        }
    }
    // Saturate high / low on a bright plane.
    let bright = vec![200u8; 72 * stride];
    let hi = [128i16, 128, 0, 0, 0, 0, 0, 0];
    let out_hi = run_mc_8tap_v(&Dispatch::detect(), &bright, stride as isize, 16, 4, &hi);
    assert!(
        out_hi.iter().all(|&v| v == 255),
        "expected clamp-high to 255"
    );
    let lo = [-128i16, -128, 0, 0, 0, 0, 0, 0];
    let out_lo = run_mc_8tap_v(&Dispatch::detect(), &bright, stride as isize, 16, 4, &lo);
    assert!(out_lo.iter().all(|&v| v == 0), "expected clamp-low to 0");
    // Extreme taps + odd widths: detected must match scalar bit-for-bit.
    let extreme = [255i16, -255, 127, -128, 64, -64, 32, -32];
    let s2 = make_plane_u8(72, stride, 223);
    for &(w, h) in &[(16usize, 4usize), (13, 3), (8, 5), (4, 2)] {
        let a = run_mc_8tap_v(&Dispatch::scalar(), &s2, stride as isize, w, h, &extreme);
        let b = run_mc_8tap_v(&Dispatch::detect(), &s2, stride as isize, w, h, &extreme);
        assert_eq!(a, b, "extreme-tap divergence at w={w} h={h}");
    }
}

// =========================================================================
// mc_8tap_2d_u8 (v0.3.2) — separable 2D MC (u8 intermediate, libvpx)
// =========================================================================

/// Independent two-pass reference with a u8 intermediate (libvpx
/// `convolve8` convention): horizontal-filter `h+7` rows, then
/// vertical-filter into the `w×h` output.
fn scalar_ref_mc_8tap_2d(
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
    fh: &[i16; 8],
    fv: &[i16; 8],
) -> Vec<u8> {
    let temp_rows = h + 7;
    let mut temp = vec![0u8; temp_rows * w];
    for rr in 0..temp_rows {
        for c in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                acc += src[rr * src_stride + c + i] as i32 * fh[i] as i32;
            }
            temp[rr * w + c] = ((acc + 64) >> 7).clamp(0, 255) as u8;
        }
    }
    let mut dst = vec![0u8; h * w];
    for r in 0..h {
        for c in 0..w {
            let mut acc: i32 = 0;
            for j in 0..8 {
                acc += temp[(r + j) * w + c] as i32 * fv[j] as i32;
            }
            dst[r * w + c] = ((acc + 64) >> 7).clamp(0, 255) as u8;
        }
    }
    dst
}

#[test]
fn mc_8tap_2d_matches_naive_two_pass() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (24, 16),
        (32, 32),
        (40, 8),
        (48, 4),
        (64, 8),
    ];
    let stride = 128usize;
    for fh in MC_FILTERS {
        for fv in MC_FILTERS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(96, stride, 131);
                let want = scalar_ref_mc_8tap_2d(&src, stride, w, h, fh, fv);
                let mut got = vec![0u8; h * w];
                codec_simd::mc_8tap_2d_u8(
                    &mut got,
                    w as isize,
                    &src,
                    stride as isize,
                    w,
                    h,
                    fh,
                    fv,
                );
                assert_eq!(
                    got,
                    want,
                    "tier {} 2d diverged at w={w} h={h} fh={fh:?} fv={fv:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_8tap_2d_separable_identity() {
    // Identity in both axes → out[r][c] == src[(r+3)*stride + (c+3)].
    let stride = 128usize;
    let src = make_plane_u8(96, stride, 17);
    let id = [0i16, 0, 0, 128, 0, 0, 0, 0];
    let mut got = vec![0u8; 8 * 16];
    codec_simd::mc_8tap_2d_u8(&mut got, 16, &src, stride as isize, 16, 8, &id, &id);
    for r in 0..8 {
        for c in 0..16 {
            assert_eq!(
                got[r * 16 + c],
                src[(r + 3) * stride + (c + 3)],
                "2d identity mismatch at r={r} c={c}"
            );
        }
    }
}

// =========================================================================
// mc_4tap_h_u8 / mc_4tap_v_u8 (v0.3.3) — chroma 4-tap, runtime shift
// =========================================================================

/// 4-tap kernels: identity, two HEVC chroma phases (sum 64), box, and an
/// adversarial kernel.  Conformance is filter-agnostic, so these double
/// as stress for the full-width multiply and the runtime-shift path.
const MC4_FILTERS: &[[i16; 4]] = &[
    [0, 64, 0, 0],          // identity (tap at position 1)
    [-2, 58, 10, -2],       // HEVC chroma, sums to 64
    [-4, 54, 16, -2],       // HEVC chroma, sums to 64
    [16, 16, 16, 16],       // box, sums to 64
    [255, -255, 127, -128], // adversarial
];
const MC4_SHIFTS: &[u32] = &[1, 6, 7, 11];

fn scalar_ref_mc_4tap_h(
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
    filter: &[i16; 4],
    shift: u32,
) -> Vec<u8> {
    let round = 1i32 << (shift - 1);
    let mut dst = vec![0u8; h * w];
    for row in 0..h {
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..4 {
                acc += src[row * src_stride + col + i] as i32 * filter[i] as i32;
            }
            dst[row * w + col] = ((acc + round) >> shift).clamp(0, 255) as u8;
        }
    }
    dst
}

fn scalar_ref_mc_4tap_v(
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
    filter: &[i16; 4],
    shift: u32,
) -> Vec<u8> {
    let round = 1i32 << (shift - 1);
    let mut dst = vec![0u8; h * w];
    for row in 0..h {
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..4 {
                acc += src[(row + i) * src_stride + col] as i32 * filter[i] as i32;
            }
            dst[row * w + col] = ((acc + round) >> shift).clamp(0, 255) as u8;
        }
    }
    dst
}

fn run_mc_4tap_h(
    disp: &Dispatch,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 4],
    shift: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    // SAFETY: src has >= (h-1)*stride + w + 3 bytes; dst is exactly h*w.
    unsafe {
        (disp.mc_4tap_h_u8)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_mc_4tap_v(
    disp: &Dispatch,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 4],
    shift: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    // SAFETY: src has >= (h+2)*stride + w bytes; dst is exactly h*w.
    unsafe {
        (disp.mc_4tap_v_u8)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

#[test]
fn mc_4tap_h_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 4),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for f in MC4_FILTERS {
        for &s in MC4_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 47);
                let want = scalar_ref_mc_4tap_h(&src, stride, w, h, f, s);
                let got = run_mc_4tap_h(&disp, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got, want,
                    "scalar diverged at w={w} h={h} shift={s} f={f:?}"
                );
            }
        }
    }
}

#[test]
fn mc_4tap_h_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (24, 16),
        (32, 32),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC4_FILTERS {
        for &s in MC4_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 113);
                let want = run_mc_4tap_h(&scalar, &src, stride as isize, w, h, f, s);
                let got = run_mc_4tap_h(&detected, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got,
                    want,
                    "tier {} diverged at w={w} h={h} shift={s} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_4tap_v_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 4),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for f in MC4_FILTERS {
        for &s in MC4_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 59);
                let want = scalar_ref_mc_4tap_v(&src, stride, w, h, f, s);
                let got = run_mc_4tap_v(&disp, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got, want,
                    "scalar diverged at w={w} h={h} shift={s} f={f:?}"
                );
            }
        }
    }
}

#[test]
fn mc_4tap_v_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (24, 16),
        (32, 32),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC4_FILTERS {
        for &s in MC4_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 151);
                let want = run_mc_4tap_v(&scalar, &src, stride as isize, w, h, f, s);
                let got = run_mc_4tap_v(&detected, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got,
                    want,
                    "tier {} diverged at w={w} h={h} shift={s} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_4tap_identity_and_saturation() {
    let stride = 128usize;
    let src = make_plane_u8(72, stride, 13);
    // Identity tap at position 1, shift 6 → (src*64 + 32) >> 6 == src.
    let id = [0i16, 64, 0, 0];
    let out_h = run_mc_4tap_h(&Dispatch::detect(), &src, stride as isize, 16, 8, &id, 6);
    for r in 0..8 {
        for c in 0..16 {
            assert_eq!(
                out_h[r * 16 + c],
                src[r * stride + c + 1],
                "h identity mismatch at r={r} c={c}"
            );
        }
    }
    let out_v = run_mc_4tap_v(&Dispatch::detect(), &src, stride as isize, 16, 8, &id, 6);
    for r in 0..8 {
        for c in 0..16 {
            assert_eq!(
                out_v[r * 16 + c],
                src[(r + 1) * stride + c],
                "v identity mismatch at r={r} c={c}"
            );
        }
    }
    // Saturation on a bright plane: sum-128 box at shift 6 → ~2× → clamp 255.
    let bright = vec![200u8; 72 * stride];
    let hi = [32i16, 32, 32, 32]; // sum 128, /64 → ~2x src
    let out_hi = run_mc_4tap_h(&Dispatch::detect(), &bright, stride as isize, 16, 4, &hi, 6);
    assert!(
        out_hi.iter().all(|&v| v == 255),
        "expected clamp-high to 255"
    );
    let lo = [-32i16, -32, -32, -32];
    let out_lo = run_mc_4tap_v(&Dispatch::detect(), &bright, stride as isize, 16, 4, &lo, 6);
    assert!(out_lo.iter().all(|&v| v == 0), "expected clamp-low to 0");
}

// =========================================================================
// mc_6tap_h_u8 / mc_6tap_v_u8 (v0.3.4) — H.264 luma 6-tap, runtime shift
// =========================================================================

const MC6_FILTERS: &[[i16; 6]] = &[
    [0, 0, 64, 0, 0, 0],             // identity (tap at position 2)
    [1, -5, 20, 20, -5, 1],          // H.264 luma half-pel, sums to 32
    [2, -10, 40, 40, -10, 2],        // 2x H.264, sums to 64
    [10, 10, 10, 10, 10, 10],        // box-ish, sums to 60
    [255, -255, 127, -128, 64, -64], // adversarial
];
const MC6_SHIFTS: &[u32] = &[1, 5, 6, 7];

fn scalar_ref_mc_6tap_h(
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
    filter: &[i16; 6],
    shift: u32,
) -> Vec<u8> {
    let round = 1i32 << (shift - 1);
    let mut dst = vec![0u8; h * w];
    for row in 0..h {
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..6 {
                acc += src[row * src_stride + col + i] as i32 * filter[i] as i32;
            }
            dst[row * w + col] = ((acc + round) >> shift).clamp(0, 255) as u8;
        }
    }
    dst
}

fn scalar_ref_mc_6tap_v(
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
    filter: &[i16; 6],
    shift: u32,
) -> Vec<u8> {
    let round = 1i32 << (shift - 1);
    let mut dst = vec![0u8; h * w];
    for row in 0..h {
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..6 {
                acc += src[(row + i) * src_stride + col] as i32 * filter[i] as i32;
            }
            dst[row * w + col] = ((acc + round) >> shift).clamp(0, 255) as u8;
        }
    }
    dst
}

fn run_mc_6tap_h(
    disp: &Dispatch,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 6],
    shift: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    // SAFETY: src has >= (h-1)*stride + w + 5 bytes; dst is exactly h*w.
    unsafe {
        (disp.mc_6tap_h_u8)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_mc_6tap_v(
    disp: &Dispatch,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 6],
    shift: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    // SAFETY: src has >= (h+4)*stride + w bytes; dst is exactly h*w.
    unsafe {
        (disp.mc_6tap_v_u8)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

#[test]
fn mc_6tap_h_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 4),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for f in MC6_FILTERS {
        for &s in MC6_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 61);
                let want = scalar_ref_mc_6tap_h(&src, stride, w, h, f, s);
                let got = run_mc_6tap_h(&disp, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got, want,
                    "scalar diverged at w={w} h={h} shift={s} f={f:?}"
                );
            }
        }
    }
}

#[test]
fn mc_6tap_h_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (24, 16),
        (32, 32),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC6_FILTERS {
        for &s in MC6_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 167);
                let want = run_mc_6tap_h(&scalar, &src, stride as isize, w, h, f, s);
                let got = run_mc_6tap_h(&detected, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got,
                    want,
                    "tier {} diverged at w={w} h={h} shift={s} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_6tap_v_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 4),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for f in MC6_FILTERS {
        for &s in MC6_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 71);
                let want = scalar_ref_mc_6tap_v(&src, stride, w, h, f, s);
                let got = run_mc_6tap_v(&disp, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got, want,
                    "scalar diverged at w={w} h={h} shift={s} f={f:?}"
                );
            }
        }
    }
}

#[test]
fn mc_6tap_v_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (24, 16),
        (32, 32),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC6_FILTERS {
        for &s in MC6_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 191);
                let want = run_mc_6tap_v(&scalar, &src, stride as isize, w, h, f, s);
                let got = run_mc_6tap_v(&detected, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got,
                    want,
                    "tier {} diverged at w={w} h={h} shift={s} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_6tap_identity_and_h264_halfpel() {
    let stride = 128usize;
    let src = make_plane_u8(72, stride, 23);
    // Identity tap at position 2, shift 6 → (src*64 + 32) >> 6 == src.
    let id = [0i16, 0, 64, 0, 0, 0];
    let out_h = run_mc_6tap_h(&Dispatch::detect(), &src, stride as isize, 16, 8, &id, 6);
    for r in 0..8 {
        for c in 0..16 {
            assert_eq!(
                out_h[r * 16 + c],
                src[r * stride + c + 2],
                "h identity mismatch at r={r} c={c}"
            );
        }
    }
    let out_v = run_mc_6tap_v(&Dispatch::detect(), &src, stride as isize, 16, 8, &id, 6);
    for r in 0..8 {
        for c in 0..16 {
            assert_eq!(
                out_v[r * 16 + c],
                src[(r + 2) * stride + c],
                "v identity mismatch at r={r} c={c}"
            );
        }
    }
    // H.264 half-pel filter on a constant plane reproduces the constant
    // (taps sum to 32 = 2^5, shift 5).
    let flat = vec![123u8; 72 * stride];
    let h264 = [1i16, -5, 20, 20, -5, 1];
    let out = run_mc_6tap_h(&Dispatch::detect(), &flat, stride as isize, 16, 4, &h264, 5);
    assert!(
        out.iter().all(|&v| v == 123),
        "H.264 half-pel of constant must be constant"
    );
}

// =========================================================================
// mc_bilinear_h_u8 / mc_bilinear_v_u8 (v0.3.5) — 2-tap, runtime shift
// =========================================================================

const MC2_FILTERS: &[[i16; 2]] = &[
    [8, 0],      // H.264 chroma frac 0 (identity at position 0)
    [6, 2],      // H.264 chroma frac 2
    [4, 4],      // half
    [1, 7],      // H.264 chroma frac 7
    [255, -255], // adversarial
];
const MC2_SHIFTS: &[u32] = &[1, 3, 6];

fn scalar_ref_mc_bilinear_h(
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
    filter: &[i16; 2],
    shift: u32,
) -> Vec<u8> {
    let round = 1i32 << (shift - 1);
    let mut dst = vec![0u8; h * w];
    for row in 0..h {
        for col in 0..w {
            let a = src[row * src_stride + col] as i32;
            let b = src[row * src_stride + col + 1] as i32;
            dst[row * w + col] = ((a * filter[0] as i32 + b * filter[1] as i32 + round) >> shift)
                .clamp(0, 255) as u8;
        }
    }
    dst
}

fn scalar_ref_mc_bilinear_v(
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
    filter: &[i16; 2],
    shift: u32,
) -> Vec<u8> {
    let round = 1i32 << (shift - 1);
    let mut dst = vec![0u8; h * w];
    for row in 0..h {
        for col in 0..w {
            let a = src[row * src_stride + col] as i32;
            let b = src[(row + 1) * src_stride + col] as i32;
            dst[row * w + col] = ((a * filter[0] as i32 + b * filter[1] as i32 + round) >> shift)
                .clamp(0, 255) as u8;
        }
    }
    dst
}

fn run_mc_bilinear_h(
    disp: &Dispatch,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 2],
    shift: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    // SAFETY: src has >= (h-1)*stride + w + 1 bytes; dst is exactly h*w.
    unsafe {
        (disp.mc_bilinear_h_u8)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_mc_bilinear_v(
    disp: &Dispatch,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 2],
    shift: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    // SAFETY: src has >= h*stride + w bytes; dst is exactly h*w.
    unsafe {
        (disp.mc_bilinear_v_u8)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

#[test]
fn mc_bilinear_h_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 4),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for f in MC2_FILTERS {
        for &s in MC2_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 81);
                let want = scalar_ref_mc_bilinear_h(&src, stride, w, h, f, s);
                let got = run_mc_bilinear_h(&disp, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got, want,
                    "scalar diverged at w={w} h={h} shift={s} f={f:?}"
                );
            }
        }
    }
}

#[test]
fn mc_bilinear_h_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (24, 16),
        (32, 32),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC2_FILTERS {
        for &s in MC2_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 173);
                let want = run_mc_bilinear_h(&scalar, &src, stride as isize, w, h, f, s);
                let got = run_mc_bilinear_h(&detected, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got,
                    want,
                    "tier {} diverged at w={w} h={h} shift={s} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_bilinear_v_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 4),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for f in MC2_FILTERS {
        for &s in MC2_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 91);
                let want = scalar_ref_mc_bilinear_v(&src, stride, w, h, f, s);
                let got = run_mc_bilinear_v(&disp, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got, want,
                    "scalar diverged at w={w} h={h} shift={s} f={f:?}"
                );
            }
        }
    }
}

#[test]
fn mc_bilinear_v_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (24, 16),
        (32, 32),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC2_FILTERS {
        for &s in MC2_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 199);
                let want = run_mc_bilinear_v(&scalar, &src, stride as isize, w, h, f, s);
                let got = run_mc_bilinear_v(&detected, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got,
                    want,
                    "tier {} diverged at w={w} h={h} shift={s} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_bilinear_identity_frac0() {
    // H.264 chroma frac 0: [8,0], shift 3 → (8*src + 4) >> 3 == src.
    let stride = 128usize;
    let src = make_plane_u8(72, stride, 33);
    let id = [8i16, 0];
    let out_h = run_mc_bilinear_h(&Dispatch::detect(), &src, stride as isize, 16, 8, &id, 3);
    for r in 0..8 {
        for c in 0..16 {
            assert_eq!(
                out_h[r * 16 + c],
                src[r * stride + c],
                "h frac-0 mismatch at r={r} c={c}"
            );
        }
    }
    let out_v = run_mc_bilinear_v(&Dispatch::detect(), &src, stride as isize, 16, 8, &id, 3);
    for r in 0..8 {
        for c in 0..16 {
            assert_eq!(
                out_v[r * 16 + c],
                src[r * stride + c],
                "v frac-0 mismatch at r={r} c={c}"
            );
        }
    }
}

// =========================================================================
// mc_8tap_h_u8_to_i16 (v0.3.6) — i16-output horizontal (2D first pass)
// =========================================================================

const MC_I16_SHIFTS: &[u32] = &[0, 4, 6, 7];

fn scalar_ref_mc_8tap_h_i16(
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
    shift: u32,
) -> Vec<i16> {
    let round = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
    let mut dst = vec![0i16; h * w];
    for row in 0..h {
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                acc += src[row * src_stride + col + i] as i32 * filter[i] as i32;
            }
            dst[row * w + col] =
                ((acc + round) >> shift).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
    }
    dst
}

fn run_mc_8tap_h_i16(
    disp: &Dispatch,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
    shift: u32,
) -> Vec<i16> {
    let mut dst = vec![0i16; h * w];
    // SAFETY: src has >= (h-1)*stride + w + 7 bytes; dst is exactly h*w i16.
    unsafe {
        (disp.mc_8tap_h_u8_to_i16)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

#[test]
fn mc_8tap_h_i16_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 4),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for f in MC_FILTERS {
        for &s in MC_I16_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 101);
                let want = scalar_ref_mc_8tap_h_i16(&src, stride, w, h, f, s);
                let got = run_mc_8tap_h_i16(&disp, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got, want,
                    "scalar diverged at w={w} h={h} shift={s} f={f:?}"
                );
            }
        }
    }
}

#[test]
fn mc_8tap_h_i16_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (24, 16),
        (32, 32),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC_FILTERS {
        for &s in MC_I16_SHIFTS {
            for &(w, h) in &sizes {
                let src = make_plane_u8(72, stride, 211);
                let want = run_mc_8tap_h_i16(&scalar, &src, stride as isize, w, h, f, s);
                let got = run_mc_8tap_h_i16(&detected, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got,
                    want,
                    "tier {} diverged at w={w} h={h} shift={s} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_8tap_h_i16_identity_and_no_clip() {
    let stride = 128usize;
    let src = make_plane_u8(72, stride, 27);
    // Identity tap at position 3, shift 7 → i16 output == src[col+3] (0..255).
    let id = [0i16, 0, 0, 128, 0, 0, 0, 0];
    let out = run_mc_8tap_h_i16(&Dispatch::detect(), &src, stride as isize, 16, 8, &id, 7);
    for r in 0..8 {
        for c in 0..16 {
            assert_eq!(
                out[r * 16 + c],
                src[r * stride + c + 3] as i16,
                "i16 identity mismatch at r={r} c={c}"
            );
        }
    }
    // shift 0 keeps the raw weighted sum: it can be negative and is NOT
    // clipped to [0,255].  A high-pass-ish filter over a ramp must produce
    // values outside the pixel range; scalar and detected must still agree.
    let hp = [1i16, -5, 0, 0, 0, 0, -5, 1];
    let a = run_mc_8tap_h_i16(&Dispatch::scalar(), &src, stride as isize, 16, 8, &hp, 0);
    let b = run_mc_8tap_h_i16(&Dispatch::detect(), &src, stride as isize, 16, 8, &hp, 0);
    assert_eq!(a, b, "shift-0 i16 detected vs scalar diverged");
    assert!(
        a.iter().any(|&v| v < 0),
        "expected some negative i16 (no pixel clip)"
    );
}

// =========================================================================
// mc_8tap_v_i16_to_u8 (v0.3.7) — i16-input vertical (2D second pass)
// =========================================================================

/// Pseudo-random i16 plane (range -8192..8191) standing in for a
/// horizontally-filtered intermediate — includes negatives and
/// above-pixel-range magnitudes.
fn make_plane_i16(rows: usize, stride: usize, seed: u32) -> Vec<i16> {
    let n = rows * stride;
    (0..n)
        .map(|i| {
            let v = (i as u32).wrapping_mul(2_654_435_761).wrapping_add(seed);
            ((v % 16384) as i32 - 8192) as i16
        })
        .collect()
}

fn scalar_ref_mc_8tap_v_i16(
    src: &[i16],
    src_stride: usize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
    shift: u32,
) -> Vec<u8> {
    let round = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
    let mut dst = vec![0u8; h * w];
    for row in 0..h {
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                acc += src[(row + i) * src_stride + col] as i32 * filter[i] as i32;
            }
            dst[row * w + col] = ((acc + round) >> shift).clamp(0, 255) as u8;
        }
    }
    dst
}

fn run_mc_8tap_v_i16(
    disp: &Dispatch,
    src: &[i16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
    shift: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    // SAFETY: src has >= (h+6)*stride + w i16; dst is exactly h*w u8.
    unsafe {
        (disp.mc_8tap_v_i16_to_u8)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

#[test]
fn mc_8tap_v_i16_scalar_matches_naive_reference() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 4),
    ];
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for f in MC_FILTERS {
        for &s in &[0u32, 4, 7, 12] {
            for &(w, h) in &sizes {
                let src = make_plane_i16(72, stride, 303);
                let want = scalar_ref_mc_8tap_v_i16(&src, stride, w, h, f, s);
                let got = run_mc_8tap_v_i16(&disp, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got, want,
                    "scalar diverged at w={w} h={h} shift={s} f={f:?}"
                );
            }
        }
    }
}

#[test]
fn mc_8tap_v_i16_detected_dispatch_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (24, 16),
        (32, 32),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC_FILTERS {
        for &s in &[0u32, 4, 7, 12] {
            for &(w, h) in &sizes {
                let src = make_plane_i16(72, stride, 409);
                let want = run_mc_8tap_v_i16(&scalar, &src, stride as isize, w, h, f, s);
                let got = run_mc_8tap_v_i16(&detected, &src, stride as isize, w, h, f, s);
                assert_eq!(
                    got,
                    want,
                    "tier {} diverged at w={w} h={h} shift={s} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_8tap_v_i16_identity_clamps_intermediate() {
    // Identity tap at position 3, shift 7 → clip_u8(tmp[(r+3)][c]).
    let stride = 128usize;
    let src = make_plane_i16(72, stride, 55);
    let id = [0i16, 0, 0, 128, 0, 0, 0, 0];
    let out = run_mc_8tap_v_i16(&Dispatch::detect(), &src, stride as isize, 16, 8, &id, 7);
    for r in 0..8 {
        for c in 0..16 {
            let want = (src[(r + 3) * stride + c] as i32).clamp(0, 255) as u8;
            assert_eq!(out[r * 16 + c], want, "v-i16 identity clamp at r={r} c={c}");
        }
    }
}

#[test]
fn mc_8tap_2d_high_precision_via_bricks() {
    // Compose the two i16 bricks into an HEVC-8bit-style high-precision 2D
    // (shift1 = BD-8 = 0, shift2 = 20-BD = 12) and check it against an
    // independent two-pass i16-intermediate reference.
    let stride = 128usize;
    let w = 16usize;
    let h = 8usize;
    let fh = [-1i16, 4, -10, 58, 17, -5, 1, 0]; // HEVC-like luma, sums to 64
    let fv = [0i16, 1, -5, 17, 58, -10, 4, -1];
    let src = make_plane_u8(72, stride, 64);
    let (shift1, shift2) = (0u32, 12u32);
    // Brick composition: h-pass into i16 scratch (h+7 rows), then v-pass.
    let tmp_rows = h + 7;
    let mut tmp = vec![0i16; tmp_rows * w];
    codec_simd::mc_8tap_h_u8_to_i16(
        &mut tmp,
        w as isize,
        &src,
        stride as isize,
        w,
        tmp_rows,
        &fh,
        shift1,
    );
    let mut got = vec![0u8; h * w];
    codec_simd::mc_8tap_v_i16_to_u8(&mut got, w as isize, &tmp, w as isize, w, h, &fv, shift2);
    // Independent reference.
    let r1 = if shift1 == 0 { 0 } else { 1i32 << (shift1 - 1) };
    let r2 = if shift2 == 0 { 0 } else { 1i32 << (shift2 - 1) };
    let mut ref_tmp = vec![0i16; tmp_rows * w];
    for rr in 0..tmp_rows {
        for c in 0..w {
            let mut acc = 0i32;
            for i in 0..8 {
                acc += src[rr * stride + c + i] as i32 * fh[i] as i32;
            }
            ref_tmp[rr * w + c] =
                ((acc + r1) >> shift1).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
    }
    let mut want = vec![0u8; h * w];
    for r in 0..h {
        for c in 0..w {
            let mut acc = 0i32;
            for i in 0..8 {
                acc += ref_tmp[(r + i) * w + c] as i32 * fv[i] as i32;
            }
            want[r * w + c] = ((acc + r2) >> shift2).clamp(0, 255) as u8;
        }
    }
    assert_eq!(got, want, "high-precision 2D brick composition mismatch");
}

#[test]
fn mc_8tap_2d_hp_matches_two_pass() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (64, 8),
    ];
    let stride = 128usize;
    let fh = [-1i16, 4, -10, 58, 17, -5, 1, 0];
    let fv = [0i16, 1, -5, 17, 58, -10, 4, -1];
    let shifts = [(0u32, 12u32), (2, 10), (6, 6)];
    for &(s1, s2) in &shifts {
        for &(w, h) in &sizes {
            let src = make_plane_u8(96, stride, 144);
            let r1 = if s1 == 0 { 0 } else { 1i32 << (s1 - 1) };
            let r2 = if s2 == 0 { 0 } else { 1i32 << (s2 - 1) };
            let tmp_rows = h + 7;
            let mut tmp = vec![0i16; tmp_rows * w];
            for rr in 0..tmp_rows {
                for c in 0..w {
                    let mut acc = 0i32;
                    for i in 0..8 {
                        acc += src[rr * stride + c + i] as i32 * fh[i] as i32;
                    }
                    tmp[rr * w + c] =
                        ((acc + r1) >> s1).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                }
            }
            let mut want = vec![0u8; h * w];
            for r in 0..h {
                for c in 0..w {
                    let mut acc = 0i32;
                    for i in 0..8 {
                        acc += tmp[(r + i) * w + c] as i32 * fv[i] as i32;
                    }
                    want[r * w + c] = ((acc + r2) >> s2).clamp(0, 255) as u8;
                }
            }
            let mut got = vec![0u8; h * w];
            codec_simd::mc_8tap_2d_hp_u8(
                &mut got,
                w as isize,
                &src,
                stride as isize,
                w,
                h,
                &fh,
                &fv,
                s1,
                s2,
            );
            assert_eq!(got, want, "2d_hp diverged at w={w} h={h} s1={s1} s2={s2}");
        }
    }
}

// =========================================================================
// 4-tap i16 bricks + 2D HP (v0.3.9)
// =========================================================================

fn run_mc_4tap_h_i16(
    disp: &Dispatch,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 4],
    shift: u32,
) -> Vec<i16> {
    let mut dst = vec![0i16; h * w];
    unsafe {
        (disp.mc_4tap_h_u8_to_i16)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_mc_4tap_v_i16(
    disp: &Dispatch,
    src: &[i16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 4],
    shift: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    unsafe {
        (disp.mc_4tap_v_i16_to_u8)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

#[test]
fn mc_4tap_i16_bricks_detected_match_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (32, 16),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC4_FILTERS {
        for &s in &[0u32, 6, 12] {
            for &(w, h) in &sizes {
                // horizontal u8 -> i16
                let su = make_plane_u8(72, stride, 233);
                assert_eq!(
                    run_mc_4tap_h_i16(&detected, &su, stride as isize, w, h, f, s),
                    run_mc_4tap_h_i16(&scalar, &su, stride as isize, w, h, f, s),
                    "4tap h_i16 tier {} w={w} h={h} s={s}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
                // vertical i16 -> u8
                let si = make_plane_i16(72, stride, 251);
                assert_eq!(
                    run_mc_4tap_v_i16(&detected, &si, stride as isize, w, h, f, s),
                    run_mc_4tap_v_i16(&scalar, &si, stride as isize, w, h, f, s),
                    "4tap v_i16 tier {} w={w} h={h} s={s}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_4tap_2d_hp_matches_two_pass() {
    let sizes = [(4, 4), (8, 8), (12, 4), (16, 16), (24, 8), (32, 16)];
    let stride = 128usize;
    let fh = [-2i16, 58, 10, -2];
    let fv = [-2i16, 10, 58, -2];
    let shifts = [(0u32, 12u32), (2, 10), (6, 6)];
    for &(s1, s2) in &shifts {
        for &(w, h) in &sizes {
            let src = make_plane_u8(96, stride, 145);
            let r1 = if s1 == 0 { 0 } else { 1i32 << (s1 - 1) };
            let r2 = if s2 == 0 { 0 } else { 1i32 << (s2 - 1) };
            let tmp_rows = h + 3;
            let mut tmp = vec![0i16; tmp_rows * w];
            for rr in 0..tmp_rows {
                for c in 0..w {
                    let mut acc = 0i32;
                    for i in 0..4 {
                        acc += src[rr * stride + c + i] as i32 * fh[i] as i32;
                    }
                    tmp[rr * w + c] =
                        ((acc + r1) >> s1).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                }
            }
            let mut want = vec![0u8; h * w];
            for r in 0..h {
                for c in 0..w {
                    let mut acc = 0i32;
                    for i in 0..4 {
                        acc += tmp[(r + i) * w + c] as i32 * fv[i] as i32;
                    }
                    want[r * w + c] = ((acc + r2) >> s2).clamp(0, 255) as u8;
                }
            }
            let mut got = vec![0u8; h * w];
            codec_simd::mc_4tap_2d_hp_u8(
                &mut got,
                w as isize,
                &src,
                stride as isize,
                w,
                h,
                &fh,
                &fv,
                s1,
                s2,
            );
            assert_eq!(
                got, want,
                "4tap 2d_hp diverged at w={w} h={h} s1={s1} s2={s2}"
            );
        }
    }
}

// =========================================================================
// 6-tap i16 bricks + 2D HP / H.264 centre j (v0.3.10)
// =========================================================================

fn run_mc_6tap_h_i16(
    disp: &Dispatch,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 6],
    shift: u32,
) -> Vec<i16> {
    let mut dst = vec![0i16; h * w];
    unsafe {
        (disp.mc_6tap_h_u8_to_i16)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_mc_6tap_v_i16(
    disp: &Dispatch,
    src: &[i16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 6],
    shift: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; h * w];
    unsafe {
        (disp.mc_6tap_v_i16_to_u8)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
    dst
}

#[test]
fn mc_6tap_i16_bricks_detected_match_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (20, 8),
        (32, 16),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for f in MC6_FILTERS {
        for &s in &[0u32, 5, 10] {
            for &(w, h) in &sizes {
                let su = make_plane_u8(72, stride, 234);
                assert_eq!(
                    run_mc_6tap_h_i16(&detected, &su, stride as isize, w, h, f, s),
                    run_mc_6tap_h_i16(&scalar, &su, stride as isize, w, h, f, s),
                    "6tap h_i16 tier {} w={w} h={h} s={s}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
                let si = make_plane_i16(72, stride, 252);
                assert_eq!(
                    run_mc_6tap_v_i16(&detected, &si, stride as isize, w, h, f, s),
                    run_mc_6tap_v_i16(&scalar, &si, stride as isize, w, h, f, s),
                    "6tap v_i16 tier {} w={w} h={h} s={s}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_6tap_2d_hp_h264_centre_j() {
    // The real H.264 centre half-pel `j`: 6-tap [1,-5,20,20,-5,1] both
    // axes, unrounded i16 intermediate (shift1=0), then (.. + 512) >> 10.
    let sizes = [(4, 4), (8, 8), (12, 4), (16, 16), (24, 8), (32, 16)];
    let stride = 128usize;
    let f = [1i16, -5, 20, 20, -5, 1];
    let (s1, s2) = (0u32, 10u32);
    for &(w, h) in &sizes {
        let src = make_plane_u8(96, stride, 146);
        let r2 = 1i32 << (s2 - 1);
        let tmp_rows = h + 5;
        let mut tmp = vec![0i16; tmp_rows * w];
        for rr in 0..tmp_rows {
            for c in 0..w {
                let mut acc = 0i32;
                for i in 0..6 {
                    acc += src[rr * stride + c + i] as i32 * f[i] as i32;
                }
                tmp[rr * w + c] = acc.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            }
        }
        let mut want = vec![0u8; h * w];
        for r in 0..h {
            for c in 0..w {
                let mut acc = 0i32;
                for i in 0..6 {
                    acc += tmp[(r + i) * w + c] as i32 * f[i] as i32;
                }
                want[r * w + c] = ((acc + r2) >> s2).clamp(0, 255) as u8;
            }
        }
        let mut got = vec![0u8; h * w];
        codec_simd::mc_6tap_2d_hp_u8(
            &mut got,
            w as isize,
            &src,
            stride as isize,
            w,
            h,
            &f,
            &f,
            s1,
            s2,
        );
        assert_eq!(got, want, "H.264 centre j mismatch at w={w} h={h}");
    }
}

// =========================================================================
// sad_x4_block_u8 (v0.3.11) — 4 MV candidates in parallel
// =========================================================================

#[test]
fn sad_x4_matches_individual_and_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    let a = make_plane_u8(72, stride, 301);
    let plane = make_plane_u8(72, stride, 401);
    let detected = Dispatch::detect();
    let scalar = Dispatch::scalar();
    for &(w, h) in &sizes {
        let r0 = &plane[0..];
        let r1 = &plane[3..];
        let r2 = &plane[stride..];
        let r3 = &plane[stride + 5..];
        let got = unsafe {
            (detected.sad_x4_block_u8)(
                a.as_ptr(),
                stride as isize,
                r0.as_ptr(),
                r1.as_ptr(),
                r2.as_ptr(),
                r3.as_ptr(),
                stride as isize,
                w,
                h,
            )
        };
        let got_s = unsafe {
            (scalar.sad_x4_block_u8)(
                a.as_ptr(),
                stride as isize,
                r0.as_ptr(),
                r1.as_ptr(),
                r2.as_ptr(),
                r3.as_ptr(),
                stride as isize,
                w,
                h,
            )
        };
        let want = [
            codec_simd::sad_block_u8(&a, stride as isize, r0, stride as isize, w, h),
            codec_simd::sad_block_u8(&a, stride as isize, r1, stride as isize, w, h),
            codec_simd::sad_block_u8(&a, stride as isize, r2, stride as isize, w, h),
            codec_simd::sad_block_u8(&a, stride as isize, r3, stride as isize, w, h),
        ];
        assert_eq!(
            got,
            want,
            "sad_x4 tier {} vs 4x sad_block at w={w} h={h}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
        );
        assert_eq!(got_s, want, "sad_x4 scalar vs 4x sad_block at w={w} h={h}");
    }
}

#[test]
fn sad_x4_public_api_matches_dispatch() {
    let stride = 128usize;
    let a = make_plane_u8(72, stride, 17);
    let plane = make_plane_u8(72, stride, 71);
    let (w, h) = (16usize, 16usize);
    let refs: [&[u8]; 4] = [
        &plane[0..],
        &plane[2..],
        &plane[stride..],
        &plane[stride + 3..],
    ];
    let got = codec_simd::sad_x4_block_u8(&a, stride as isize, &refs, stride as isize, w, h);
    for k in 0..4 {
        let want = codec_simd::sad_block_u8(&a, stride as isize, refs[k], stride as isize, w, h);
        assert_eq!(got[k], want, "public sad_x4 lane {k}");
    }
}

// =========================================================================
// dequant_block_i16 (v0.3.12)
// =========================================================================

#[test]
fn dequant_block_i16_detected_matches_scalar() {
    let ns = [0usize, 1, 7, 8, 15, 16, 17, 64, 1000];
    let scales = [1i16, 6, 64, -3, 300, -1000];
    let shifts = [0u32, 1, 4, 6, 11];
    let plane = make_plane_i16(1, 1024, 314);
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &sc in &scales {
        for &sh in &shifts {
            for &n in &ns {
                let mut a = plane[..n].to_vec();
                let mut b = plane[..n].to_vec();
                unsafe {
                    (scalar.dequant_block_i16)(a.as_mut_ptr(), n, sc, sh);
                }
                unsafe {
                    (detected.dequant_block_i16)(b.as_mut_ptr(), n, sc, sh);
                }
                assert_eq!(
                    a,
                    b,
                    "dequant tier {} n={n} scale={sc} shift={sh}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn dequant_block_i16_known_values() {
    // scale 64, shift 6 → (c*64 + 32) >> 6 == c (identity).
    let mut buf = vec![0i16, 1, -1, 100, -100, 200, -200, 32767, -32768];
    let orig = buf.clone();
    codec_simd::dequant_block_i16(&mut buf, 64, 6);
    assert_eq!(buf, orig, "scale=64 shift=6 should be identity");
    // scale 4, shift 0 → c*4, saturating to i16.
    let mut b2 = vec![10000i16, -10000, 100, -100];
    codec_simd::dequant_block_i16(&mut b2, 4, 0);
    assert_eq!(b2, vec![32767i16, -32768, 400, -400]);
}

// =========================================================================
// weighted_avg_u8 (v0.3.13)
// =========================================================================

#[test]
fn weighted_avg_u8_detected_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    let params = [
        (1i16, 1i16, 1i32, 1u32),
        (5, 3, 4, 3),
        (-2, 10, 8, 4),
        (64, 64, 64, 7),
        (32, 96, 0, 7),
    ];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(w0, w1, off, sh) in &params {
        for &(w, h) in &sizes {
            let a = make_plane_u8(72, stride, 51);
            let b = make_plane_u8(72, stride, 83);
            let mut da = vec![0u8; h * w];
            let mut dd = vec![0u8; h * w];
            unsafe {
                (scalar.weighted_avg_u8)(
                    a.as_ptr(),
                    stride as isize,
                    b.as_ptr(),
                    stride as isize,
                    da.as_mut_ptr(),
                    w as isize,
                    w,
                    h,
                    w0,
                    w1,
                    off,
                    sh,
                );
                (detected.weighted_avg_u8)(
                    a.as_ptr(),
                    stride as isize,
                    b.as_ptr(),
                    stride as isize,
                    dd.as_mut_ptr(),
                    w as isize,
                    w,
                    h,
                    w0,
                    w1,
                    off,
                    sh,
                );
            }
            assert_eq!(
                da,
                dd,
                "weighted tier {} w={w} h={h} params=({w0},{w1},{off},{sh})",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
            );
        }
    }
}

#[test]
fn weighted_avg_u8_equals_mc_avg_for_unit_weights() {
    // w0=w1=1, offset=1, shift=1 → (a+b+1)>>1 == mc_avg_u8.
    let stride = 128usize;
    let a = make_plane_u8(72, stride, 11);
    let b = make_plane_u8(72, stride, 29);
    let (w, h) = (16usize, 8usize);
    let mut wa = vec![0u8; h * w];
    codec_simd::weighted_avg_u8(
        &a,
        stride as isize,
        &b,
        stride as isize,
        &mut wa,
        w as isize,
        w,
        h,
        1,
        1,
        1,
        1,
    );
    let mut mc = vec![0u8; h * w];
    codec_simd::mc_avg_u8(
        &a,
        stride as isize,
        &b,
        stride as isize,
        &mut mc,
        w as isize,
        w,
        h,
    );
    assert_eq!(wa, mc, "weighted unit weights must equal mc_avg");
}

// =========================================================================
// lincomb3_clip_u8 (v0.3.14) — YUV→RGB plane building block
// =========================================================================

#[test]
fn lincomb3_clip_u8_detected_matches_scalar() {
    let ns = [0usize, 1, 7, 8, 15, 16, 17, 31, 64, 1000];
    // (c0, c1, c2, offset, shift) — incl. real BT.601 red & green rows.
    let coeffs = [
        (298i16, 0i16, 409i16, 128 - 298 * 16 - 409 * 128, 8u32),
        (298, -100, -208, 128 - 298 * 16 + 100 * 128 + 208 * 128, 8),
        (1, 1, 1, 1, 2),
        (64, -32, 16, 0, 6),
        (-5, 300, -200, 1000, 7),
    ];
    let p0 = make_plane_u8(1, 1024, 7);
    let p1 = make_plane_u8(1, 1024, 19);
    let p2 = make_plane_u8(1, 1024, 41);
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(c0, c1, c2, off, sh) in &coeffs {
        for &n in &ns {
            let mut da = vec![0u8; n];
            let mut dd = vec![0u8; n];
            unsafe {
                (scalar.lincomb3_clip_u8)(
                    p0.as_ptr(),
                    p1.as_ptr(),
                    p2.as_ptr(),
                    da.as_mut_ptr(),
                    n,
                    c0,
                    c1,
                    c2,
                    off,
                    sh,
                );
                (detected.lincomb3_clip_u8)(
                    p0.as_ptr(),
                    p1.as_ptr(),
                    p2.as_ptr(),
                    dd.as_mut_ptr(),
                    n,
                    c0,
                    c1,
                    c2,
                    off,
                    sh,
                );
            }
            assert_eq!(
                da,
                dd,
                "lincomb3 tier {} n={n} c=({c0},{c1},{c2},{off},{sh})",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
            );
        }
    }
}

#[test]
fn lincomb3_clip_u8_passthrough_and_bt601() {
    // (1,0,0), offset 0, shift 0 → dst == p0 (passthrough).
    let p0 = make_plane_u8(1, 256, 5);
    let p1 = make_plane_u8(1, 256, 6);
    let p2 = make_plane_u8(1, 256, 7);
    let mut dst = vec![0u8; 256];
    codec_simd::lincomb3_clip_u8(&p0, &p1, &p2, &mut dst, 1, 0, 0, 0, 0);
    assert_eq!(dst, p0, "passthrough");
    // BT.601 limited-range red on a uniform white-ish pixel run:
    // Y=235, U=128, V=128 → R = clip((298*235 + 409*128 + (128-298*16-409*128)) >> 8).
    let y = vec![235u8; 16];
    let u = vec![128u8; 16];
    let v = vec![128u8; 16];
    let mut r = vec![0u8; 16];
    let off = 128 - 298 * 16 - 409 * 128;
    codec_simd::lincomb3_clip_u8(&y, &u, &v, &mut r, 298, 0, 409, off, 8);
    // (298*235 + 409*128 + off) >> 8 = (70030 + 52352 + (128-4768-52352))>>8
    //   = (70030 - 4640) >> 8 = 65390 >> 8 = 255 (clipped).
    let want = ((298i32 * 235 + 409 * 128 + off) >> 8).clamp(0, 255) as u8;
    assert!(r.iter().all(|&x| x == want), "BT.601 red uniform");
}

// =========================================================================
// hadamard_4x4_i16 (v0.3.15)
// =========================================================================

#[test]
fn hadamard_4x4_detected_matches_scalar() {
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for seed in 0..40u32 {
        let src: Vec<i16> = (0..16)
            .map(|i| (((i as u32 * 37 + seed) * 13 % 512) as i32 - 256) as i16)
            .collect();
        let mut a = vec![0i16; 16];
        let mut b = vec![0i16; 16];
        unsafe {
            (scalar.hadamard_4x4_i16)(src.as_ptr(), a.as_mut_ptr());
            (detected.hadamard_4x4_i16)(src.as_ptr(), b.as_mut_ptr());
        }
        assert_eq!(
            a,
            b,
            "hadamard tier {} seed={seed}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
        );
    }
}

#[test]
fn hadamard_4x4_double_is_16x() {
    // H is the unnormalized ±1 matrix, so H·H = 4I and the 2D transform
    // applied twice yields 16·X.
    let src: Vec<i16> = (0..16).map(|i| (i as i16 - 8) * 3).collect();
    let mut t1 = vec![0i16; 16];
    codec_simd::hadamard_4x4_i16(&src, &mut t1);
    let mut t2 = vec![0i16; 16];
    codec_simd::hadamard_4x4_i16(&t1, &mut t2);
    for i in 0..16 {
        assert_eq!(t2[i], src[i] * 16, "double hadamard at {i}");
    }
}

// =========================================================================
// mc_avg_u16 (v0.3.17) — high-bit-depth bi-prediction average
// =========================================================================

fn make_plane_u16(rows: usize, stride: usize, seed: u32) -> Vec<u16> {
    let n = rows * stride;
    (0..n)
        .map(|i| ((i as u32).wrapping_mul(2_654_435_761).wrapping_add(seed) % 65536) as u16)
        .collect()
}

#[test]
fn mc_avg_u16_detected_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(w, h) in &sizes {
        let a = make_plane_u16(72, stride, 13);
        let b = make_plane_u16(72, stride, 91);
        let mut da = vec![0u16; h * w];
        let mut dd = vec![0u16; h * w];
        unsafe {
            (scalar.mc_avg_u16)(
                a.as_ptr(),
                stride as isize,
                b.as_ptr(),
                stride as isize,
                da.as_mut_ptr(),
                w as isize,
                w,
                h,
            );
            (detected.mc_avg_u16)(
                a.as_ptr(),
                stride as isize,
                b.as_ptr(),
                stride as isize,
                dd.as_mut_ptr(),
                w as isize,
                w,
                h,
            );
        }
        assert_eq!(
            da,
            dd,
            "mc_avg_u16 tier {} w={w} h={h}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
        );
    }
}

#[test]
fn mc_avg_u16_rounding_full_range() {
    // Overflow-free rounding: (65535+0+1)>>1 = 32768, etc.
    let a = vec![65535u16, 1000, 0, 4095, 65535];
    let b = vec![0u16, 1001, 65535, 4095, 65535];
    let mut d = vec![0u16; 5];
    codec_simd::mc_avg_u16(&a, 5, &b, 5, &mut d, 5, 5, 1);
    assert_eq!(d, vec![32768u16, 1001, 32768, 4095, 65535]);
}

// =========================================================================
// weighted_avg_u16 (v0.3.18)
// =========================================================================

#[test]
fn weighted_avg_u16_detected_matches_scalar() {
    let sizes = [
        (4, 4),
        (8, 8),
        (12, 4),
        (16, 16),
        (24, 8),
        (32, 16),
        (40, 8),
        (64, 8),
    ];
    let stride = 128usize;
    // (w0, w1, offset, shift, max) — 10-bit (1023) and 12-bit (4095) ceilings.
    let params = [
        (1i16, 1i16, 1i32, 1u32, 1023u16),
        (5, 3, 4, 3, 1023),
        (-2, 10, 8, 4, 4095),
        (32, 96, 64, 7, 4095),
        (1, 1, 0, 1, 255),
    ];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(w0, w1, off, sh, max) in &params {
        let m = max as u32 + 1;
        let a: Vec<u16> = (0..72 * stride)
            .map(|i| ((i as u32 * 37 + 7) % m) as u16)
            .collect();
        let b: Vec<u16> = (0..72 * stride)
            .map(|i| ((i as u32 * 53 + 19) % m) as u16)
            .collect();
        for &(w, h) in &sizes {
            let mut da = vec![0u16; h * w];
            let mut dd = vec![0u16; h * w];
            unsafe {
                (scalar.weighted_avg_u16)(
                    a.as_ptr(),
                    stride as isize,
                    b.as_ptr(),
                    stride as isize,
                    da.as_mut_ptr(),
                    w as isize,
                    w,
                    h,
                    w0,
                    w1,
                    off,
                    sh,
                    max,
                );
                (detected.weighted_avg_u16)(
                    a.as_ptr(),
                    stride as isize,
                    b.as_ptr(),
                    stride as isize,
                    dd.as_mut_ptr(),
                    w as isize,
                    w,
                    h,
                    w0,
                    w1,
                    off,
                    sh,
                    max,
                );
            }
            assert_eq!(
                da,
                dd,
                "weighted_avg_u16 tier {} w={w} h={h} params=({w0},{w1},{off},{sh},{max})",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
            );
        }
    }
}

#[test]
fn weighted_avg_u16_clamps_to_max() {
    // w0=w1=1, offset=0, shift=0, max=1023 → a+b clamped to [0,1023].
    let a = vec![1000u16, 1000, 0, 500];
    let b = vec![1000u16, 0, 0, 500];
    let mut d = vec![0u16; 4];
    codec_simd::weighted_avg_u16(&a, 4, &b, 4, &mut d, 4, 4, 1, 1, 1, 0, 0, 1023);
    assert_eq!(d, vec![1023u16, 1000, 0, 1000]);
}

// =========================================================================
// rct_fwd_i32 / rct_bwd_i32 — HTJ2K reversible colour transform.
// =========================================================================

#[test]
fn rct_fwd_detected_matches_scalar() {
    // Cover SIMD body (multiples of 4) plus every scalar-tail residue.
    let sizes = [1usize, 2, 3, 4, 5, 7, 8, 15, 16, 17, 31, 64, 100];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &sizes {
        let r: Vec<i32> = (0..n).map(|i| (i as i32 * 37 + 7) % 4096).collect();
        let g: Vec<i32> = (0..n).map(|i| (i as i32 * 53 + 19) % 4096).collect();
        let b: Vec<i32> = (0..n).map(|i| (i as i32 * 71 + 3) % 4096).collect();
        let (mut ys, mut cbs, mut crs) = (vec![0i32; n], vec![0i32; n], vec![0i32; n]);
        let (mut yd, mut cbd, mut crd) = (vec![0i32; n], vec![0i32; n], vec![0i32; n]);
        unsafe {
            (scalar.rct_fwd_i32)(
                r.as_ptr(),
                g.as_ptr(),
                b.as_ptr(),
                ys.as_mut_ptr(),
                cbs.as_mut_ptr(),
                crs.as_mut_ptr(),
                n,
            );
            (detected.rct_fwd_i32)(
                r.as_ptr(),
                g.as_ptr(),
                b.as_ptr(),
                yd.as_mut_ptr(),
                cbd.as_mut_ptr(),
                crd.as_mut_ptr(),
                n,
            );
        }
        let tier = codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level());
        assert_eq!((ys, cbs, crs), (yd, cbd, crd), "rct_fwd tier {tier} n={n}");
    }
}

#[test]
fn rct_bwd_detected_matches_scalar() {
    let sizes = [1usize, 2, 3, 4, 5, 7, 8, 15, 16, 17, 31, 64, 100];
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &sizes {
        let y: Vec<i32> = (0..n).map(|i| (i as i32 * 29 + 11) % 4096).collect();
        let cb: Vec<i32> = (0..n)
            .map(|i| ((i as i32 * 17 + 5) % 2048) - 1024)
            .collect();
        let cr: Vec<i32> = (0..n)
            .map(|i| ((i as i32 * 41 + 13) % 2048) - 1024)
            .collect();
        let (mut rs, mut gs, mut bs) = (vec![0i32; n], vec![0i32; n], vec![0i32; n]);
        let (mut rd, mut gd, mut bd) = (vec![0i32; n], vec![0i32; n], vec![0i32; n]);
        unsafe {
            (scalar.rct_bwd_i32)(
                y.as_ptr(),
                cb.as_ptr(),
                cr.as_ptr(),
                rs.as_mut_ptr(),
                gs.as_mut_ptr(),
                bs.as_mut_ptr(),
                n,
            );
            (detected.rct_bwd_i32)(
                y.as_ptr(),
                cb.as_ptr(),
                cr.as_ptr(),
                rd.as_mut_ptr(),
                gd.as_mut_ptr(),
                bd.as_mut_ptr(),
                n,
            );
        }
        let tier = codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level());
        assert_eq!((rs, gs, bs), (rd, gd, bd), "rct_bwd tier {tier} n={n}");
    }
}

#[test]
fn rct_roundtrip_is_lossless() {
    // The reversible colour transform must recover RGB exactly: bwd(fwd(x))==x.
    let n = 257usize;
    let r: Vec<i32> = (0..n).map(|i| (i as i32 * 37 + 7) % 4096).collect();
    let g: Vec<i32> = (0..n).map(|i| (i as i32 * 53 + 19) % 4096).collect();
    let b: Vec<i32> = (0..n).map(|i| (i as i32 * 71 + 3) % 4096).collect();
    let (mut y, mut cb, mut cr) = (vec![0i32; n], vec![0i32; n], vec![0i32; n]);
    codec_simd::rct_fwd_i32(&r, &g, &b, &mut y, &mut cb, &mut cr, n);
    let (mut r2, mut g2, mut b2) = (vec![0i32; n], vec![0i32; n], vec![0i32; n]);
    codec_simd::rct_bwd_i32(&y, &cb, &cr, &mut r2, &mut g2, &mut b2, n);
    assert_eq!((r, g, b), (r2, g2, b2), "RCT round-trip must be lossless");
}

// =========================================================================
// HEVC luma 8-tap MC, i32-source -> raw i32 intermediate (v0.3.22).
//
// Byte-exact port of ffmpeg-rs `av-codec-h265/mc.rs`.  The goldens below are
// INDEPENDENT re-implementations of that exact math (pure arithmetic
// `>> shift`, NO rounding offset, NO narrow, NO saturation) copied from the
// reference formulae (`luma_h_sample` / `luma_filter_tmp` / `fill_luma_htmp`);
// they never call the primitive under test.  The tap filters emit the RAW i32
// intermediate: `put_luma` truncates it to `int16_t` (wrapping) itself, the
// uni/bi producers `clip_pixel` from the full i32.  The hv second pass CAN
// exceed i16 (see `hevc_mc_v_i16_i32_exceeds_i16_no_saturate`), which is why a
// saturating output would diverge from ffmpeg-rs — the bug this fixes.  The
// `htmp` first-pass values still lie in `[-6142, 22522] ⊆ i16`, so the 2D
// wrapper's i16 `htmp` store is byte-exact.
// =========================================================================

/// `ff_hevc_qpel_filters` rows 0..3 (i8), copied verbatim from ffmpeg-rs
/// `QPEL_FILTERS`.  Row 0 is the integer (all-zero) position.
const HEVC_QPEL: [[i8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 0],
    [-1, 4, -10, 58, 17, -5, 1, 0],
    [-1, 4, -11, 40, 40, -11, 4, -1],
    [0, 1, -5, 17, 58, -10, 4, -1],
];

/// `(bit_depth, shift = bit_depth - 8)` pairs to sweep.
const HEVC_BD_SHIFTS: &[(u32, u32)] = &[(8, 0), (10, 2), (12, 4)];

/// Pseudo-random pixel-range i32 plane: values in `0..(1<<bit_depth)`.
fn make_plane_i32_px(rows: usize, stride: usize, seed: u32, bit_depth: u32) -> Vec<i32> {
    let modulo = 1u32 << bit_depth;
    (0..rows * stride)
        .map(|i| {
            let v = (i as u32).wrapping_mul(2_654_435_761).wrapping_add(seed);
            (v % modulo) as i32
        })
        .collect()
}

/// Pseudo-random i16 plane standing in for the hv horizontal-pass `htmp`
/// (range roughly `[-6142, 22522]`, the proven HEVC intermediate range).
fn make_plane_htmp_i16(rows: usize, stride: usize, seed: u32) -> Vec<i16> {
    (0..rows * stride)
        .map(|i| {
            let v = (i as u32).wrapping_mul(2_246_822_519).wrapping_add(seed);
            ((v % 28664) as i32 - 6142) as i16
        })
        .collect()
}

fn golden_h_i32_i32(
    src: &[i32],
    stride: usize,
    w: usize,
    h: usize,
    f: &[i8; 8],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    for r in 0..h {
        for c in 0..w {
            let mut acc: i32 = 0;
            for k in 0..8 {
                acc += f[k] as i32 * src[r * stride + c + k];
            }
            dst[r * w + c] = acc >> shift;
        }
    }
    dst
}

fn golden_v_i32_i32(
    src: &[i32],
    stride: usize,
    w: usize,
    h: usize,
    f: &[i8; 8],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    for r in 0..h {
        for c in 0..w {
            let mut acc: i32 = 0;
            for k in 0..8 {
                acc += f[k] as i32 * src[(r + k) * stride + c];
            }
            dst[r * w + c] = acc >> shift;
        }
    }
    dst
}

fn golden_v_i16_i32(
    src: &[i16],
    stride: usize,
    w: usize,
    h: usize,
    f: &[i8; 8],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    for r in 0..h {
        for c in 0..w {
            let mut acc: i32 = 0;
            for k in 0..8 {
                acc += f[k] as i32 * src[(r + k) * stride + c] as i32;
            }
            dst[r * w + c] = acc >> shift;
        }
    }
    dst
}

fn run_h_i32_i32(
    disp: &Dispatch,
    src: &[i32],
    stride: isize,
    w: usize,
    h: usize,
    f: &[i8; 8],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    // SAFETY: src exposes (h-1)*stride + w + 7 i32; dst is exactly w*h i32.
    unsafe {
        (disp.mc_8tap_h_i32_to_i32)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            stride,
            w,
            h,
            f.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_v_i32_i32(
    disp: &Dispatch,
    src: &[i32],
    stride: isize,
    w: usize,
    h: usize,
    f: &[i8; 8],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    // SAFETY: src exposes (h+6)*stride + w i32; dst is exactly w*h i32.
    unsafe {
        (disp.mc_8tap_v_i32_to_i32)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            stride,
            w,
            h,
            f.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_v_i16_i32(
    disp: &Dispatch,
    src: &[i16],
    stride: isize,
    w: usize,
    h: usize,
    f: &[i8; 8],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    // SAFETY: src exposes (h+6)*stride + w i16; dst is exactly w*h i32.
    unsafe {
        (disp.mc_8tap_v_i16_to_i32)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            stride,
            w,
            h,
            f.as_ptr(),
            shift,
        );
    }
    dst
}

const HEVC_SIZES: &[(usize, usize)] = &[
    (4, 4),
    (8, 8),
    (12, 4),
    (16, 16),
    (24, 8),
    (32, 16),
    (64, 4),
    (8, 64),
    (20, 12),
    (40, 8),
];

#[test]
fn hevc_mc_h_i32_i16_scalar_matches_golden() {
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for &(bd, shift) in HEVC_BD_SHIFTS {
        for f in &HEVC_QPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_i32_px(72, stride, 0x1234 ^ bd, bd);
                let want = golden_h_i32_i32(&src, stride, w, h, f, shift);
                let got = run_h_i32_i32(&disp, &src, stride as isize, w, h, f, shift);
                assert_eq!(got, want, "h scalar!=golden bd={bd} w={w} h={h} f={f:?}");
            }
        }
    }
}

#[test]
fn hevc_mc_h_i32_i16_detected_matches_scalar() {
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(bd, shift) in HEVC_BD_SHIFTS {
        for f in &HEVC_QPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_i32_px(72, stride, 0x55aa ^ bd, bd);
                let want = run_h_i32_i32(&scalar, &src, stride as isize, w, h, f, shift);
                let got = run_h_i32_i32(&detected, &src, stride as isize, w, h, f, shift);
                assert_eq!(
                    got,
                    want,
                    "h tier {} != scalar bd={bd} w={w} h={h} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn hevc_mc_v_i32_i16_scalar_matches_golden() {
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for &(bd, shift) in HEVC_BD_SHIFTS {
        for f in &HEVC_QPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_i32_px(80, stride, 0x2468 ^ bd, bd);
                let want = golden_v_i32_i32(&src, stride, w, h, f, shift);
                let got = run_v_i32_i32(&disp, &src, stride as isize, w, h, f, shift);
                assert_eq!(got, want, "v scalar!=golden bd={bd} w={w} h={h} f={f:?}");
            }
        }
    }
}

#[test]
fn hevc_mc_v_i32_i16_detected_matches_scalar() {
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(bd, shift) in HEVC_BD_SHIFTS {
        for f in &HEVC_QPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_i32_px(80, stride, 0x7f7f ^ bd, bd);
                let want = run_v_i32_i32(&scalar, &src, stride as isize, w, h, f, shift);
                let got = run_v_i32_i32(&detected, &src, stride as isize, w, h, f, shift);
                assert_eq!(
                    got,
                    want,
                    "v tier {} != scalar bd={bd} w={w} h={h} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn hevc_mc_v_i16_i16_scalar_matches_golden() {
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for &shift in &[0u32, 2, 4, 6] {
        for f in &HEVC_QPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_htmp_i16(80, stride, 0x9e37 ^ shift);
                let want = golden_v_i16_i32(&src, stride, w, h, f, shift);
                let got = run_v_i16_i32(&disp, &src, stride as isize, w, h, f, shift);
                assert_eq!(
                    got, want,
                    "v-i16 scalar!=golden shift={shift} w={w} h={h} f={f:?}"
                );
            }
        }
    }
}

#[test]
fn hevc_mc_v_i16_i16_detected_matches_scalar() {
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &shift in &[0u32, 2, 4, 6] {
        for f in &HEVC_QPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_htmp_i16(80, stride, 0xc0de ^ shift);
                let want = run_v_i16_i32(&scalar, &src, stride as isize, w, h, f, shift);
                let got = run_v_i16_i32(&detected, &src, stride as isize, w, h, f, shift);
                assert_eq!(
                    got,
                    want,
                    "v-i16 tier {} != scalar shift={shift} w={w} h={h} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn hevc_mc_h_i32_i16_tap_alignment() {
    // Identity tap at position 3 (the HEVC `luma_filter` centre `src[base]`),
    // shift 0 -> dst[r][c] == src[r][c+3].  Verifies the forward-tap /
    // pre-offset convention (tap 0 == src[base-3]).
    let stride = 96usize;
    let src = make_plane_i32_px(40, stride, 0x0777, 10);
    let id = [0i8, 0, 0, 1, 0, 0, 0, 0];
    let out = run_h_i32_i32(&Dispatch::detect(), &src, stride as isize, 20, 8, &id, 0);
    for r in 0..8 {
        for c in 0..20 {
            assert_eq!(
                out[r * 20 + c],
                src[r * stride + c + 3],
                "h tap-alignment r={r} c={c}"
            );
        }
    }
}

/// Independent 2-pass reference for the hv wrapper: horizontal fill of `h+7`
/// rows (`>> shift_h`) truncated to the in-range i16 `htmp` (matching the
/// wrapper's `as i16` store), then vertical qpel (`>> 6`) emitting the RAW i32
/// output — NO saturation, exactly as ffmpeg-rs's `put_luma` hv path (which
/// then truncates to `int16_t` / `clip_pixel`s downstream, not here).
fn golden_2d_hp(
    src: &[i32],
    stride: usize,
    w: usize,
    h: usize,
    hf: &[i8; 8],
    vf: &[i8; 8],
    shift_h: u32,
) -> Vec<i32> {
    let rows = h + 7;
    let mut tmp = vec![0i16; rows * w];
    for yt in 0..rows {
        for x in 0..w {
            let mut acc: i32 = 0;
            for k in 0..8 {
                acc += hf[k] as i32 * src[yt * stride + x + k];
            }
            // htmp is i16 (values ⊆ i16) stored by truncation, as the wrapper does.
            tmp[yt * w + x] = (acc >> shift_h) as i16;
        }
    }
    let mut out = vec![0i32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc: i32 = 0;
            for k in 0..8 {
                acc += vf[k] as i32 * tmp[(y + k) * w + x] as i32;
            }
            out[y * w + x] = acc >> 6; // raw i32, no saturation
        }
    }
    out
}

#[test]
fn hevc_mc_2d_hp_matches_golden() {
    // `src` is pre-offset by -3*stride-3: a self-contained (h+7)x(w+7) window.
    for &(bd, shift_h) in HEVC_BD_SHIFTS {
        for (mi, hf) in HEVC_QPEL.iter().enumerate().skip(1) {
            for (mj, vf) in HEVC_QPEL.iter().enumerate().skip(1) {
                for &(w, h) in HEVC_SIZES {
                    let stride = w + 7;
                    let seed = 0xa1b2u32 ^ bd ^ ((mi as u32) << 4) ^ ((mj as u32) << 8);
                    let plane = make_plane_i32_px(h + 7, stride, seed, bd);
                    let want = golden_2d_hp(&plane, stride, w, h, hf, vf, shift_h);
                    let mut got = vec![0i32; w * h];
                    let mut htmp =
                        vec![0i16; (h + codec_simd::MC_LUMA_EXTRA) * codec_simd::MC_MAX_PB_SIZE];
                    codec_simd::mc_8tap_2d_hp_i32_to_i32(
                        &mut got,
                        w as isize,
                        &plane,
                        stride as isize,
                        w,
                        h,
                        hf,
                        vf,
                        shift_h,
                        &mut htmp,
                    );
                    assert_eq!(
                        got,
                        want,
                        "2d_hp tier {} bd={bd} mi={mi} mj={mj} w={w} h={h}",
                        codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                    );
                }
            }
        }
    }
}

/// Directed ADVERSARIAL test for the hv second pass
/// ([`crate::mc_8tap_v_i16_to_i32`], via `run_v_i16_i32`): construct the i16
/// `htmp` at the proven extremes — `+22522` on the rows a POSITIVE tap lands on
/// and `-6142` on the rows a NEGATIVE tap lands on — under the HEVC half-pel
/// filter, so the `>> 6` accumulator EXCEEDS `i16::MAX`.
///
/// Hand-computed: `Σ vf[k]·htmp[k] = 2_129_344`, `>> 6 = 33_271 > 32_767`.
/// The RAW-i32 primitive must return the exact `33_271`; the OLD saturating
/// (`_mm_packs_epi32` / `.clamp(i16)`) code returned `32_767` here — this is the
/// test that would have caught the bug.  Named `mc_8tap_*` so the per-tier
/// `--test cross_tier mc_8tap` sweep runs it at SSE2 (tier 3) and AVX2 (tier 9).
#[test]
fn mc_8tap_v_i16_i32_exceeds_i16_no_saturate() {
    let vf: [i8; 8] = HEVC_QPEL[2]; // half-pel: [-1, 4, -11, 40, 40, -11, 4, -1]
    let w = 16usize;
    let h = 1usize;
    let stride = w;
    // 8 htmp rows: +22522 where the tap is positive, -6142 where negative.
    let mut src = vec![0i16; 8 * stride];
    for (k, &c) in vf.iter().enumerate() {
        let v: i16 = if c > 0 { 22_522 } else { -6_142 };
        for col in 0..w {
            src[k * stride + col] = v;
        }
    }
    let expected: i32 = 33_271; // = 2_129_344 >> 6
    assert!(
        expected > i16::MAX as i32,
        "adversarial value must exceed i16::MAX to exercise the saturating bug"
    );
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        let got = run_v_i16_i32(&disp, &src, stride as isize, w, h, &vf, 6);
        for (c, &g) in got.iter().enumerate() {
            assert_eq!(
                g,
                expected,
                "hv 2nd pass must return raw i32 {expected} (NOT saturated 32767) \
                 at tier {} col={c}",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
            );
        }
    }
}

// =========================================================================
// HEVC CHROMA 4-tap MC, i32-source -> raw i32 intermediate (v0.3.23).
//
// The 4-tap sibling of the luma i32 tests above.  Byte-exact port of ffmpeg-rs
// `av-codec-h265/mc.rs` chroma path (`chroma_h_sample` / `chroma_filter_tmp` /
// `fill_chroma_htmp`, `EPEL_FILTERS`).  The goldens below are INDEPENDENT
// re-implementations of that exact math (pure `>> shift`, NO rounding offset,
// NO narrow, NO saturation); they never call the primitive under test.  All
// test fns are named `mc_4tap_*` so the `--test cross_tier mc_4tap` sweep runs
// them at every forced tier.  The chroma `htmp` first-pass values lie in
// `[-2560, 18939] ⊆ i16`, so the 2D wrapper's i16 `htmp` store is byte-exact;
// the hv second pass is RAW i32 (a directed adversarial test drives it past
// `i16::MAX`).
// =========================================================================

/// `ff_hevc_epel_filters` rows 0..7 (i8), copied verbatim from ffmpeg-rs
/// `EPEL_FILTERS`.  Row 0 is the integer (all-zero) position.
const HEVC_EPEL: [[i8; 4]; 8] = [
    [0, 0, 0, 0],
    [-2, 58, 10, -2],
    [-4, 54, 16, -2],
    [-6, 46, 28, -4],
    [-4, 36, 36, -4],
    [-4, 28, 46, -6],
    [-2, 16, 54, -4],
    [-2, 10, 58, -2],
];

/// Pseudo-random i16 plane standing in for the chroma hv horizontal-pass
/// `htmp` (range `[-2560, 18939]`, the proven HEVC chroma intermediate range).
fn make_plane_chtmp_i16(rows: usize, stride: usize, seed: u32) -> Vec<i16> {
    (0..rows * stride)
        .map(|i| {
            let v = (i as u32).wrapping_mul(2_246_822_519).wrapping_add(seed);
            ((v % 21500) as i32 - 2560) as i16
        })
        .collect()
}

fn golden_ch_h_i32(
    src: &[i32],
    stride: usize,
    w: usize,
    h: usize,
    f: &[i8; 4],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    for r in 0..h {
        for c in 0..w {
            let mut acc: i32 = 0;
            for k in 0..4 {
                acc += f[k] as i32 * src[r * stride + c + k];
            }
            dst[r * w + c] = acc >> shift;
        }
    }
    dst
}

fn golden_ch_v_i32(
    src: &[i32],
    stride: usize,
    w: usize,
    h: usize,
    f: &[i8; 4],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    for r in 0..h {
        for c in 0..w {
            let mut acc: i32 = 0;
            for k in 0..4 {
                acc += f[k] as i32 * src[(r + k) * stride + c];
            }
            dst[r * w + c] = acc >> shift;
        }
    }
    dst
}

fn golden_ch_v_i16(
    src: &[i16],
    stride: usize,
    w: usize,
    h: usize,
    f: &[i8; 4],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    for r in 0..h {
        for c in 0..w {
            let mut acc: i32 = 0;
            for k in 0..4 {
                acc += f[k] as i32 * src[(r + k) * stride + c] as i32;
            }
            dst[r * w + c] = acc >> shift;
        }
    }
    dst
}

fn run_ch_h_i32(
    disp: &Dispatch,
    src: &[i32],
    stride: isize,
    w: usize,
    h: usize,
    f: &[i8; 4],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    // SAFETY: src exposes (h-1)*stride + w + 3 i32; dst is exactly w*h i32.
    unsafe {
        (disp.mc_4tap_h_i32_to_i32)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            stride,
            w,
            h,
            f.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_ch_v_i32(
    disp: &Dispatch,
    src: &[i32],
    stride: isize,
    w: usize,
    h: usize,
    f: &[i8; 4],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    // SAFETY: src exposes (h+2)*stride + w i32; dst is exactly w*h i32.
    unsafe {
        (disp.mc_4tap_v_i32_to_i32)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            stride,
            w,
            h,
            f.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_ch_v_i16(
    disp: &Dispatch,
    src: &[i16],
    stride: isize,
    w: usize,
    h: usize,
    f: &[i8; 4],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    // SAFETY: src exposes (h+2)*stride + w i16; dst is exactly w*h i32.
    unsafe {
        (disp.mc_4tap_v_i16_to_i32)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            stride,
            w,
            h,
            f.as_ptr(),
            shift,
        );
    }
    dst
}

#[test]
fn mc_4tap_h_i32_scalar_matches_golden() {
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for &(bd, shift) in HEVC_BD_SHIFTS {
        for f in &HEVC_EPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_i32_px(72, stride, 0x1234 ^ bd, bd);
                let want = golden_ch_h_i32(&src, stride, w, h, f, shift);
                let got = run_ch_h_i32(&disp, &src, stride as isize, w, h, f, shift);
                assert_eq!(got, want, "ch h scalar!=golden bd={bd} w={w} h={h} f={f:?}");
            }
        }
    }
}

#[test]
fn mc_4tap_h_i32_detected_matches_scalar() {
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(bd, shift) in HEVC_BD_SHIFTS {
        for f in &HEVC_EPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_i32_px(72, stride, 0x55aa ^ bd, bd);
                let want = run_ch_h_i32(&scalar, &src, stride as isize, w, h, f, shift);
                let got = run_ch_h_i32(&detected, &src, stride as isize, w, h, f, shift);
                assert_eq!(
                    got,
                    want,
                    "ch h tier {} != scalar bd={bd} w={w} h={h} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_4tap_v_i32_scalar_matches_golden() {
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for &(bd, shift) in HEVC_BD_SHIFTS {
        for f in &HEVC_EPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_i32_px(80, stride, 0x2468 ^ bd, bd);
                let want = golden_ch_v_i32(&src, stride, w, h, f, shift);
                let got = run_ch_v_i32(&disp, &src, stride as isize, w, h, f, shift);
                assert_eq!(got, want, "ch v scalar!=golden bd={bd} w={w} h={h} f={f:?}");
            }
        }
    }
}

#[test]
fn mc_4tap_v_i32_detected_matches_scalar() {
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(bd, shift) in HEVC_BD_SHIFTS {
        for f in &HEVC_EPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_i32_px(80, stride, 0x7f7f ^ bd, bd);
                let want = run_ch_v_i32(&scalar, &src, stride as isize, w, h, f, shift);
                let got = run_ch_v_i32(&detected, &src, stride as isize, w, h, f, shift);
                assert_eq!(
                    got,
                    want,
                    "ch v tier {} != scalar bd={bd} w={w} h={h} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_4tap_v_i16_scalar_matches_golden() {
    let stride = 128usize;
    let disp = Dispatch::scalar();
    for &shift in &[0u32, 2, 4, 6] {
        for f in &HEVC_EPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_chtmp_i16(80, stride, 0x9e37 ^ shift);
                let want = golden_ch_v_i16(&src, stride, w, h, f, shift);
                let got = run_ch_v_i16(&disp, &src, stride as isize, w, h, f, shift);
                assert_eq!(
                    got, want,
                    "ch v-i16 scalar!=golden shift={shift} w={w} h={h} f={f:?}"
                );
            }
        }
    }
}

#[test]
fn mc_4tap_v_i16_detected_matches_scalar() {
    let stride = 128usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &shift in &[0u32, 2, 4, 6] {
        for f in &HEVC_EPEL {
            for &(w, h) in HEVC_SIZES {
                let src = make_plane_chtmp_i16(80, stride, 0xc0de ^ shift);
                let want = run_ch_v_i16(&scalar, &src, stride as isize, w, h, f, shift);
                let got = run_ch_v_i16(&detected, &src, stride as isize, w, h, f, shift);
                assert_eq!(
                    got,
                    want,
                    "ch v-i16 tier {} != scalar shift={shift} w={w} h={h} f={f:?}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_4tap_h_i32_tap_alignment() {
    // Identity tap at position 1 (the HEVC `chroma_filter` centre `src[base]`),
    // shift 0 -> dst[r][c] == src[r][c+1].  Verifies the forward-tap /
    // pre-offset convention (tap 0 == src[base-1]).
    let stride = 96usize;
    let src = make_plane_i32_px(40, stride, 0x0777, 10);
    let id = [0i8, 1, 0, 0];
    let out = run_ch_h_i32(&Dispatch::detect(), &src, stride as isize, 20, 8, &id, 0);
    for r in 0..8 {
        for c in 0..20 {
            assert_eq!(
                out[r * 20 + c],
                src[r * stride + c + 1],
                "ch h tap-alignment r={r} c={c}"
            );
        }
    }
}

/// Independent 2-pass reference for the chroma hv wrapper: horizontal fill of
/// `h+3` rows (`>> shift_h`) truncated to the in-range i16 `htmp` (matching the
/// wrapper's `as i16` store), then vertical epel (`>> 6`) emitting the RAW i32
/// output — NO saturation, exactly as ffmpeg-rs's `put_chroma` hv path.
fn golden_ch_2d_hp(
    src: &[i32],
    stride: usize,
    w: usize,
    h: usize,
    hf: &[i8; 4],
    vf: &[i8; 4],
    shift_h: u32,
) -> Vec<i32> {
    let rows = h + 3;
    let mut tmp = vec![0i16; rows * w];
    for yt in 0..rows {
        for x in 0..w {
            let mut acc: i32 = 0;
            for k in 0..4 {
                acc += hf[k] as i32 * src[yt * stride + x + k];
            }
            // htmp is i16 (values ⊆ i16) stored by truncation, as the wrapper does.
            tmp[yt * w + x] = (acc >> shift_h) as i16;
        }
    }
    let mut out = vec![0i32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc: i32 = 0;
            for k in 0..4 {
                acc += vf[k] as i32 * tmp[(y + k) * w + x] as i32;
            }
            out[y * w + x] = acc >> 6; // raw i32, no saturation
        }
    }
    out
}

#[test]
fn mc_4tap_2d_hp_matches_golden() {
    // `src` is pre-offset by -1*stride-1: a self-contained (h+3)x(w+3) window.
    for &(bd, shift_h) in HEVC_BD_SHIFTS {
        for (mi, hf) in HEVC_EPEL.iter().enumerate().skip(1) {
            for (mj, vf) in HEVC_EPEL.iter().enumerate().skip(1) {
                for &(w, h) in HEVC_SIZES {
                    let stride = w + 3;
                    let seed = 0xa1b2u32 ^ bd ^ ((mi as u32) << 4) ^ ((mj as u32) << 8);
                    let plane = make_plane_i32_px(h + 3, stride, seed, bd);
                    let want = golden_ch_2d_hp(&plane, stride, w, h, hf, vf, shift_h);
                    let mut got = vec![0i32; w * h];
                    let mut htmp =
                        vec![0i16; (h + codec_simd::MC_CHROMA_EXTRA) * codec_simd::MC_MAX_PB_SIZE];
                    codec_simd::mc_4tap_2d_hp_i32_to_i32(
                        &mut got,
                        w as isize,
                        &plane,
                        stride as isize,
                        w,
                        h,
                        hf,
                        vf,
                        shift_h,
                        &mut htmp,
                    );
                    assert_eq!(
                        got,
                        want,
                        "ch 2d_hp tier {} bd={bd} mi={mi} mj={mj} w={w} h={h}",
                        codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                    );
                }
            }
        }
    }
}

/// Directed ADVERSARIAL test for the chroma hv second pass
/// ([`crate::mc_4tap_v_i16_to_i32`], via `run_ch_v_i16`): construct the i16
/// `htmp` at `i16::MAX` on the rows a POSITIVE tap lands on and `i16::MIN` on
/// the rows a NEGATIVE tap lands on, under the largest-positive-sum EPEL filter
/// (row 3, `[-6, 46, 28, -4]`), so the `>> 6` accumulator EXCEEDS `i16::MAX`.
///
/// Hand-computed: `Σ vf[k]·htmp[k] = -6·(-32768) + 46·32767 + 28·32767 +
/// -4·(-32768) = 2_752_438`, `>> 6 = 43_006 > 32_767`.  The RAW-i32 primitive
/// must return the exact `43_006`; a saturating (`packs_epi32` / `.clamp(i16)`)
/// implementation would return `32_767` here.  (Note: the natural chroma htmp
/// range `[-2560, 18939]` alone cannot exceed i16 through the 2nd pass, so this
/// test deliberately drives the primitive with full-range i16 `htmp`.)
#[test]
fn mc_4tap_v_i16_exceeds_i16_no_saturate() {
    let vf: [i8; 4] = HEVC_EPEL[3]; // [-6, 46, 28, -4], largest positive tap sum
    let w = 16usize;
    let h = 1usize;
    let stride = w;
    // 4 htmp rows: i16::MAX where the tap is positive, i16::MIN where negative.
    let mut src = vec![0i16; 4 * stride];
    for (k, &c) in vf.iter().enumerate() {
        let v: i16 = if c > 0 { i16::MAX } else { i16::MIN };
        for col in 0..w {
            src[k * stride + col] = v;
        }
    }
    let expected: i32 = 43_006; // = 2_752_438 >> 6
    assert!(
        expected > i16::MAX as i32,
        "adversarial value must exceed i16::MAX to exercise the saturating bug"
    );
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        let got = run_ch_v_i16(&disp, &src, stride as isize, w, h, &vf, 6);
        for (c, &g) in got.iter().enumerate() {
            assert_eq!(
                g,
                expected,
                "ch hv 2nd pass must return raw i32 {expected} (NOT saturated 32767) \
                 at tier {} col={c}",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
            );
        }
    }
}

// =========================================================================
// HEVC MC, u16-SOURCE -> raw i32 intermediate (v0.3.27).
//
// The u16 siblings of the i32-source tap-filter tests above.  The STRONGEST
// check is EQUIVALENCE: run both `mc_*tap_*_u16_to_i32(u16 pixels)` and the
// existing `mc_*tap_*_i32_to_i32(the SAME values as i32)` and assert IDENTICAL
// i32 output — byte-identical by construction (a u16 pixel widened to i32 is the
// i32 value).  Each test ALSO checks an INDEPENDENT naive golden (`golden_*`,
// the exact reference math, never calling the primitive).  The 2D hv wrappers
// additionally exercise the SHARED i16 second pass (`mc_*tap_v_i16_to_i32`),
// whose out-of-i16 adversarial coverage is `mc_8tap_v_i16_i32_exceeds_i16_no_saturate`
// / `mc_4tap_v_i16_exceeds_i16_no_saturate` above (the u16 family reuses the
// exact same 2nd-pass function, so that adversarial case already applies).  All
// fns are named `mc_8tap_*` / `mc_4tap_*` so the `--test cross_tier mc_` sweep
// runs them at every forced tier (the u16-vs-i32 equivalence must hold at SSE2 /
// SSE4.1 / AVX2).
// =========================================================================

/// Pseudo-random pixel-range u16 plane, bit-for-bit the same values as
/// [`make_plane_i32_px`] with the same args (so the u16 and i32 primitives are
/// fed IDENTICAL pixel values in the equivalence tests).
fn make_plane_u16_px(rows: usize, stride: usize, seed: u32, bit_depth: u32) -> Vec<u16> {
    make_plane_i32_px(rows, stride, seed, bit_depth)
        .iter()
        .map(|&v| v as u16)
        .collect()
}

fn run_h_u16_i32(
    disp: &Dispatch,
    src: &[u16],
    stride: isize,
    w: usize,
    h: usize,
    f: &[i8; 8],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    // SAFETY: src exposes (h-1)*stride + w + 7 u16; dst is exactly w*h i32.
    unsafe {
        (disp.mc_8tap_h_u16_to_i32)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            stride,
            w,
            h,
            f.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_v_u16_i32(
    disp: &Dispatch,
    src: &[u16],
    stride: isize,
    w: usize,
    h: usize,
    f: &[i8; 8],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    // SAFETY: src exposes (h+6)*stride + w u16; dst is exactly w*h i32.
    unsafe {
        (disp.mc_8tap_v_u16_to_i32)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            stride,
            w,
            h,
            f.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_ch_h_u16(
    disp: &Dispatch,
    src: &[u16],
    stride: isize,
    w: usize,
    h: usize,
    f: &[i8; 4],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    // SAFETY: src exposes (h-1)*stride + w + 3 u16; dst is exactly w*h i32.
    unsafe {
        (disp.mc_4tap_h_u16_to_i32)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            stride,
            w,
            h,
            f.as_ptr(),
            shift,
        );
    }
    dst
}

fn run_ch_v_u16(
    disp: &Dispatch,
    src: &[u16],
    stride: isize,
    w: usize,
    h: usize,
    f: &[i8; 4],
    shift: u32,
) -> Vec<i32> {
    let mut dst = vec![0i32; w * h];
    // SAFETY: src exposes (h+2)*stride + w u16; dst is exactly w*h i32.
    unsafe {
        (disp.mc_4tap_v_u16_to_i32)(
            dst.as_mut_ptr(),
            w as isize,
            src.as_ptr(),
            stride,
            w,
            h,
            f.as_ptr(),
            shift,
        );
    }
    dst
}

#[test]
fn mc_8tap_h_u16_matches_i32_twin_and_golden() {
    let stride = 128usize;
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        for &(bd, shift) in HEVC_BD_SHIFTS {
            for f in &HEVC_QPEL {
                for &(w, h) in HEVC_SIZES {
                    let seed = 0x11c0 ^ bd;
                    let src_u16 = make_plane_u16_px(72, stride, seed, bd);
                    let src_i32 = make_plane_i32_px(72, stride, seed, bd);
                    let golden = golden_h_i32_i32(&src_i32, stride, w, h, f, shift);
                    let twin = run_h_i32_i32(&disp, &src_i32, stride as isize, w, h, f, shift);
                    let got = run_h_u16_i32(&disp, &src_u16, stride as isize, w, h, f, shift);
                    assert_eq!(got, golden, "u16 h != golden bd={bd} w={w} h={h} f={f:?}");
                    assert_eq!(
                        got,
                        twin,
                        "u16 h != i32-twin tier {} bd={bd} w={w} h={h} f={f:?}",
                        codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                    );
                }
            }
        }
    }
}

#[test]
fn mc_8tap_v_u16_matches_i32_twin_and_golden() {
    let stride = 128usize;
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        for &(bd, shift) in HEVC_BD_SHIFTS {
            for f in &HEVC_QPEL {
                for &(w, h) in HEVC_SIZES {
                    let seed = 0x22d1 ^ bd;
                    let src_u16 = make_plane_u16_px(80, stride, seed, bd);
                    let src_i32 = make_plane_i32_px(80, stride, seed, bd);
                    let golden = golden_v_i32_i32(&src_i32, stride, w, h, f, shift);
                    let twin = run_v_i32_i32(&disp, &src_i32, stride as isize, w, h, f, shift);
                    let got = run_v_u16_i32(&disp, &src_u16, stride as isize, w, h, f, shift);
                    assert_eq!(got, golden, "u16 v != golden bd={bd} w={w} h={h} f={f:?}");
                    assert_eq!(
                        got,
                        twin,
                        "u16 v != i32-twin tier {} bd={bd} w={w} h={h} f={f:?}",
                        codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                    );
                }
            }
        }
    }
}

#[test]
fn mc_4tap_h_u16_matches_i32_twin_and_golden() {
    let stride = 128usize;
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        for &(bd, shift) in HEVC_BD_SHIFTS {
            for f in &HEVC_EPEL {
                for &(w, h) in HEVC_SIZES {
                    let seed = 0x33e2 ^ bd;
                    let src_u16 = make_plane_u16_px(72, stride, seed, bd);
                    let src_i32 = make_plane_i32_px(72, stride, seed, bd);
                    let golden = golden_ch_h_i32(&src_i32, stride, w, h, f, shift);
                    let twin = run_ch_h_i32(&disp, &src_i32, stride as isize, w, h, f, shift);
                    let got = run_ch_h_u16(&disp, &src_u16, stride as isize, w, h, f, shift);
                    assert_eq!(
                        got, golden,
                        "u16 ch h != golden bd={bd} w={w} h={h} f={f:?}"
                    );
                    assert_eq!(
                        got,
                        twin,
                        "u16 ch h != i32-twin tier {} bd={bd} w={w} h={h} f={f:?}",
                        codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                    );
                }
            }
        }
    }
}

#[test]
fn mc_4tap_v_u16_matches_i32_twin_and_golden() {
    let stride = 128usize;
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        for &(bd, shift) in HEVC_BD_SHIFTS {
            for f in &HEVC_EPEL {
                for &(w, h) in HEVC_SIZES {
                    let seed = 0x44f3 ^ bd;
                    let src_u16 = make_plane_u16_px(80, stride, seed, bd);
                    let src_i32 = make_plane_i32_px(80, stride, seed, bd);
                    let golden = golden_ch_v_i32(&src_i32, stride, w, h, f, shift);
                    let twin = run_ch_v_i32(&disp, &src_i32, stride as isize, w, h, f, shift);
                    let got = run_ch_v_u16(&disp, &src_u16, stride as isize, w, h, f, shift);
                    assert_eq!(
                        got, golden,
                        "u16 ch v != golden bd={bd} w={w} h={h} f={f:?}"
                    );
                    assert_eq!(
                        got,
                        twin,
                        "u16 ch v != i32-twin tier {} bd={bd} w={w} h={h} f={f:?}",
                        codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                    );
                }
            }
        }
    }
}

#[test]
fn mc_8tap_2d_hp_u16_matches_i32_twin_and_golden() {
    // `src` is pre-offset by -3*stride-3: a self-contained (h+7)x(w+7) window.
    for &(bd, shift_h) in HEVC_BD_SHIFTS {
        for (mi, hf) in HEVC_QPEL.iter().enumerate().skip(1) {
            for (mj, vf) in HEVC_QPEL.iter().enumerate().skip(1) {
                for &(w, h) in HEVC_SIZES {
                    let stride = w + 7;
                    let seed = 0x5a6bu32 ^ bd ^ ((mi as u32) << 4) ^ ((mj as u32) << 8);
                    let plane_u16 = make_plane_u16_px(h + 7, stride, seed, bd);
                    let plane_i32 = make_plane_i32_px(h + 7, stride, seed, bd);
                    let golden = golden_2d_hp(&plane_i32, stride, w, h, hf, vf, shift_h);
                    let mut twin = vec![0i32; w * h];
                    let mut htmp_a =
                        vec![0i16; (h + codec_simd::MC_LUMA_EXTRA) * codec_simd::MC_MAX_PB_SIZE];
                    codec_simd::mc_8tap_2d_hp_i32_to_i32(
                        &mut twin,
                        w as isize,
                        &plane_i32,
                        stride as isize,
                        w,
                        h,
                        hf,
                        vf,
                        shift_h,
                        &mut htmp_a,
                    );
                    let mut got = vec![0i32; w * h];
                    let mut htmp_b =
                        vec![0i16; (h + codec_simd::MC_LUMA_EXTRA) * codec_simd::MC_MAX_PB_SIZE];
                    codec_simd::mc_8tap_2d_hp_u16_to_i32(
                        &mut got,
                        w as isize,
                        &plane_u16,
                        stride as isize,
                        w,
                        h,
                        hf,
                        vf,
                        shift_h,
                        &mut htmp_b,
                    );
                    assert_eq!(
                        got, golden,
                        "u16 2d_hp != golden bd={bd} mi={mi} mj={mj} w={w} h={h}"
                    );
                    assert_eq!(
                        got,
                        twin,
                        "u16 2d_hp != i32-twin tier {} bd={bd} mi={mi} mj={mj} w={w} h={h}",
                        codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                    );
                }
            }
        }
    }
}

#[test]
fn mc_4tap_2d_hp_u16_matches_i32_twin_and_golden() {
    // `src` is pre-offset by -1*stride-1: a self-contained (h+3)x(w+3) window.
    for &(bd, shift_h) in HEVC_BD_SHIFTS {
        for (mi, hf) in HEVC_EPEL.iter().enumerate().skip(1) {
            for (mj, vf) in HEVC_EPEL.iter().enumerate().skip(1) {
                for &(w, h) in HEVC_SIZES {
                    let stride = w + 3;
                    let seed = 0x6b7cu32 ^ bd ^ ((mi as u32) << 4) ^ ((mj as u32) << 8);
                    let plane_u16 = make_plane_u16_px(h + 3, stride, seed, bd);
                    let plane_i32 = make_plane_i32_px(h + 3, stride, seed, bd);
                    let golden = golden_ch_2d_hp(&plane_i32, stride, w, h, hf, vf, shift_h);
                    let mut twin = vec![0i32; w * h];
                    let mut htmp_a =
                        vec![0i16; (h + codec_simd::MC_CHROMA_EXTRA) * codec_simd::MC_MAX_PB_SIZE];
                    codec_simd::mc_4tap_2d_hp_i32_to_i32(
                        &mut twin,
                        w as isize,
                        &plane_i32,
                        stride as isize,
                        w,
                        h,
                        hf,
                        vf,
                        shift_h,
                        &mut htmp_a,
                    );
                    let mut got = vec![0i32; w * h];
                    let mut htmp_b =
                        vec![0i16; (h + codec_simd::MC_CHROMA_EXTRA) * codec_simd::MC_MAX_PB_SIZE];
                    codec_simd::mc_4tap_2d_hp_u16_to_i32(
                        &mut got,
                        w as isize,
                        &plane_u16,
                        stride as isize,
                        w,
                        h,
                        hf,
                        vf,
                        shift_h,
                        &mut htmp_b,
                    );
                    assert_eq!(
                        got, golden,
                        "u16 ch 2d_hp != golden bd={bd} mi={mi} mj={mj} w={w} h={h}"
                    );
                    assert_eq!(
                        got,
                        twin,
                        "u16 ch 2d_hp != i32-twin tier {} bd={bd} mi={mi} mj={mj} w={w} h={h}",
                        codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                    );
                }
            }
        }
    }
}

#[test]
fn mc_8tap_h_u16_tap_alignment() {
    // Identity tap at position 3, shift 0 -> dst[r][c] == src[r][c+3] (verifies
    // the u16 h-pass forward-tap / pre-offset convention).
    let stride = 96usize;
    let src = make_plane_u16_px(40, stride, 0x0778, 10);
    let id = [0i8, 0, 0, 1, 0, 0, 0, 0];
    let out = run_h_u16_i32(&Dispatch::detect(), &src, stride as isize, 20, 8, &id, 0);
    for r in 0..8 {
        for c in 0..20 {
            assert_eq!(
                out[r * 20 + c],
                src[r * stride + c + 3] as i32,
                "u16 h tap-alignment r={r} c={c}"
            );
        }
    }
}

// =========================================================================
// HEVC MC "final" clip/combine: i32 intermediate -> u16 pixel / wrapping i16
// (v0.3.26).
//
// Byte-exact port of ffmpeg-rs `av-codec-h265/mc.rs` `put_luma_uni` /
// `put_luma_uni_w` / `put_luma_bi` / `put_luma_bi_w` / `put_luma` (`as i16`).
// The goldens below are INDEPENDENT re-implementations of that exact math
// (`clip(x) = x.clamp(0, max)`, `max = (1<<bd)-1`); they never call the
// primitive under test.  The `v` planes deliberately span out-of-i16 values
// (the intermediate legitimately reaches ~33k, and the wrapping-store path is
// driven past i16::MAX by a directed test).  All fns are named
// `mc_uni_*` / `mc_bi_*` / `mc_store_*` so the `--test cross_tier mc_` sweep
// runs them at every forced tier.
// =========================================================================

/// `(bit_depth, max, uni_shift = 14-bd, bi_shift = 15-bd)`.
const HEVC_FINAL_BD: &[(u32, i32, u32, u32)] =
    &[(8, 255, 6, 7), (10, 1023, 4, 5), (12, 4095, 2, 3)];

/// Signed i32 intermediate plane spanning `[-span, span]` (includes
/// out-of-i16 magnitudes so the `> i16` uni/bi/store paths are exercised).
fn make_plane_i32_signed(rows: usize, stride: usize, seed: u32, span: i32) -> Vec<i32> {
    let modulo = 2 * span as u32 + 1;
    (0..rows * stride)
        .map(|i| {
            let v = (i as u32).wrapping_mul(2_654_435_761).wrapping_add(seed);
            (v % modulo) as i32 - span
        })
        .collect()
}

/// Full-range i16 `src2` plane (the other-list first prediction; ffmpeg-rs
/// `put_luma`'s wrapping `int16_t` store can produce any i16).
fn make_plane_src2_i16(rows: usize, stride: usize, seed: u32) -> Vec<i16> {
    (0..rows * stride)
        .map(|i| {
            let v = (i as u32).wrapping_mul(2_246_822_519).wrapping_add(seed);
            (v & 0xFFFF) as i16
        })
        .collect()
}

fn golden_uni(
    v: &[i32],
    vstride: usize,
    w: usize,
    h: usize,
    offset: i32,
    shift: u32,
    max: i32,
) -> Vec<u16> {
    let mut dst = vec![0u16; w * h];
    for r in 0..h {
        for c in 0..w {
            let x = v[r * vstride + c];
            dst[r * w + c] = ((x + offset) >> shift).clamp(0, max) as u16;
        }
    }
    dst
}

#[allow(clippy::too_many_arguments)]
fn golden_uni_w(
    v: &[i32],
    vstride: usize,
    w: usize,
    h: usize,
    wx: i32,
    offset: i32,
    shift: u32,
    ox: i32,
    max: i32,
) -> Vec<u16> {
    let mut dst = vec![0u16; w * h];
    for r in 0..h {
        for c in 0..w {
            let x = v[r * vstride + c];
            dst[r * w + c] = (((x * wx + offset) >> shift) + ox).clamp(0, max) as u16;
        }
    }
    dst
}

#[allow(clippy::too_many_arguments)]
fn golden_bi(
    v: &[i32],
    vstride: usize,
    src2: &[i16],
    s2stride: usize,
    w: usize,
    h: usize,
    offset: i32,
    shift: u32,
    max: i32,
) -> Vec<u16> {
    let mut dst = vec![0u16; w * h];
    for r in 0..h {
        for c in 0..w {
            let x = v[r * vstride + c];
            let s2 = src2[r * s2stride + c] as i32;
            dst[r * w + c] = ((x + s2 + offset) >> shift).clamp(0, max) as u16;
        }
    }
    dst
}

#[allow(clippy::too_many_arguments)]
fn golden_bi_w(
    v: &[i32],
    vstride: usize,
    src2: &[i16],
    s2stride: usize,
    w: usize,
    h: usize,
    wx0: i32,
    wx1: i32,
    bias: i32,
    rshift: u32,
    max: i32,
) -> Vec<u16> {
    let mut dst = vec![0u16; w * h];
    for r in 0..h {
        for c in 0..w {
            let x = v[r * vstride + c];
            let s2 = src2[r * s2stride + c] as i32;
            dst[r * w + c] = ((x * wx1 + s2 * wx0 + bias) >> rshift).clamp(0, max) as u16;
        }
    }
    dst
}

fn golden_store(v: &[i32], vstride: usize, w: usize, h: usize) -> Vec<i16> {
    let mut dst = vec![0i16; w * h];
    for r in 0..h {
        for c in 0..w {
            dst[r * w + c] = v[r * vstride + c] as i16;
        }
    }
    dst
}

#[allow(clippy::too_many_arguments)]
fn run_uni(
    disp: &Dispatch,
    v: &[i32],
    vstride: isize,
    w: usize,
    h: usize,
    offset: i32,
    shift: u32,
    max: u16,
) -> Vec<u16> {
    let mut dst = vec![0u16; w * h];
    // SAFETY: v exposes (h-1)*vstride + w i32; dst is exactly w*h u16.
    unsafe {
        (disp.mc_uni_clip_i32_to_u16)(
            dst.as_mut_ptr(),
            w as isize,
            v.as_ptr(),
            vstride,
            w,
            h,
            offset,
            shift,
            max,
        );
    }
    dst
}

#[allow(clippy::too_many_arguments)]
fn run_uni_w(
    disp: &Dispatch,
    v: &[i32],
    vstride: isize,
    w: usize,
    h: usize,
    wx: i32,
    offset: i32,
    shift: u32,
    ox: i32,
    max: u16,
) -> Vec<u16> {
    let mut dst = vec![0u16; w * h];
    // SAFETY: v exposes (h-1)*vstride + w i32; dst is exactly w*h u16.
    unsafe {
        (disp.mc_uni_w_clip_i32_to_u16)(
            dst.as_mut_ptr(),
            w as isize,
            v.as_ptr(),
            vstride,
            w,
            h,
            wx,
            offset,
            shift,
            ox,
            max,
        );
    }
    dst
}

#[allow(clippy::too_many_arguments)]
fn run_bi(
    disp: &Dispatch,
    v: &[i32],
    vstride: isize,
    src2: &[i16],
    s2stride: isize,
    w: usize,
    h: usize,
    offset: i32,
    shift: u32,
    max: u16,
) -> Vec<u16> {
    let mut dst = vec![0u16; w * h];
    // SAFETY: v/src2 expose (h-1)*stride + w; dst is exactly w*h u16.
    unsafe {
        (disp.mc_bi_clip_i32_to_u16)(
            dst.as_mut_ptr(),
            w as isize,
            v.as_ptr(),
            vstride,
            src2.as_ptr(),
            s2stride,
            w,
            h,
            offset,
            shift,
            max,
        );
    }
    dst
}

#[allow(clippy::too_many_arguments)]
fn run_bi_w(
    disp: &Dispatch,
    v: &[i32],
    vstride: isize,
    src2: &[i16],
    s2stride: isize,
    w: usize,
    h: usize,
    wx0: i32,
    wx1: i32,
    bias: i32,
    rshift: u32,
    max: u16,
) -> Vec<u16> {
    let mut dst = vec![0u16; w * h];
    // SAFETY: v/src2 expose (h-1)*stride + w; dst is exactly w*h u16.
    unsafe {
        (disp.mc_bi_w_clip_i32_to_u16)(
            dst.as_mut_ptr(),
            w as isize,
            v.as_ptr(),
            vstride,
            src2.as_ptr(),
            s2stride,
            w,
            h,
            wx0,
            wx1,
            bias,
            rshift,
            max,
        );
    }
    dst
}

fn run_store(disp: &Dispatch, v: &[i32], vstride: isize, w: usize, h: usize) -> Vec<i16> {
    let mut dst = vec![0i16; w * h];
    // SAFETY: v exposes (h-1)*vstride + w i32; dst is exactly w*h i16.
    unsafe {
        (disp.mc_store_i32_to_i16)(dst.as_mut_ptr(), w as isize, v.as_ptr(), vstride, w, h);
    }
    dst
}

/// HEVC-domain weighted-uni params: `(wx, denom, ox_unscaled)`.
const UNIW_PARAMS: &[(i32, u32, i32)] = &[
    (1, 0, 0),
    (64, 6, 0),
    (-32, 5, 10),
    (127, 7, -20),
    (45, 4, 63),
];

/// HEVC-domain weighted-bi params: `(wx0, wx1, denom, ox0_unscaled, ox1_unscaled)`.
const BIW_PARAMS: &[(i32, i32, u32, i32, i32)] = &[
    (1, 1, 0, 0, 0),
    (32, 32, 6, 0, 0),
    (64, -16, 5, 10, -8),
    (127, 80, 7, -20, 15),
];

#[test]
fn mc_uni_clip_scalar_matches_golden() {
    let stride = 96usize;
    let disp = Dispatch::scalar();
    for &(bd, max, ushift, _) in HEVC_FINAL_BD {
        let offset = 1i32 << (ushift - 1);
        for &(w, h) in HEVC_SIZES {
            let v = make_plane_i32_signed(80, stride, 0x1111 ^ bd, 40000);
            let want = golden_uni(&v, stride, w, h, offset, ushift, max);
            let got = run_uni(&disp, &v, stride as isize, w, h, offset, ushift, max as u16);
            assert_eq!(got, want, "uni scalar!=golden bd={bd} w={w} h={h}");
        }
    }
}

#[test]
fn mc_uni_clip_detected_matches_scalar() {
    let stride = 96usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(bd, max, ushift, _) in HEVC_FINAL_BD {
        let offset = 1i32 << (ushift - 1);
        for &(w, h) in HEVC_SIZES {
            let v = make_plane_i32_signed(80, stride, 0x2222 ^ bd, 40000);
            let want = run_uni(
                &scalar,
                &v,
                stride as isize,
                w,
                h,
                offset,
                ushift,
                max as u16,
            );
            let got = run_uni(
                &detected,
                &v,
                stride as isize,
                w,
                h,
                offset,
                ushift,
                max as u16,
            );
            assert_eq!(
                got,
                want,
                "uni tier {} != scalar bd={bd} w={w} h={h}",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
            );
        }
    }
}

#[test]
fn mc_uni_w_clip_scalar_matches_golden() {
    let stride = 96usize;
    let disp = Dispatch::scalar();
    for &(bd, max, _, _) in HEVC_FINAL_BD {
        for &(wx, denom, ox_u) in UNIW_PARAMS {
            let shift = denom + 14 - bd;
            let offset = 1i32 << (shift - 1);
            let ox = ox_u << (bd - 8);
            for &(w, h) in HEVC_SIZES {
                let v = make_plane_i32_signed(80, stride, 0x3333 ^ bd ^ (wx as u32), 40000);
                let want = golden_uni_w(&v, stride, w, h, wx, offset, shift, ox, max);
                let got = run_uni_w(
                    &disp,
                    &v,
                    stride as isize,
                    w,
                    h,
                    wx,
                    offset,
                    shift,
                    ox,
                    max as u16,
                );
                assert_eq!(
                    got, want,
                    "uni_w scalar!=golden bd={bd} wx={wx} denom={denom} w={w} h={h}"
                );
            }
        }
    }
}

#[test]
fn mc_uni_w_clip_detected_matches_scalar() {
    let stride = 96usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(bd, max, _, _) in HEVC_FINAL_BD {
        for &(wx, denom, ox_u) in UNIW_PARAMS {
            let shift = denom + 14 - bd;
            let offset = 1i32 << (shift - 1);
            let ox = ox_u << (bd - 8);
            for &(w, h) in HEVC_SIZES {
                let v = make_plane_i32_signed(80, stride, 0x4444 ^ bd ^ (wx as u32), 40000);
                let want = run_uni_w(
                    &scalar,
                    &v,
                    stride as isize,
                    w,
                    h,
                    wx,
                    offset,
                    shift,
                    ox,
                    max as u16,
                );
                let got = run_uni_w(
                    &detected,
                    &v,
                    stride as isize,
                    w,
                    h,
                    wx,
                    offset,
                    shift,
                    ox,
                    max as u16,
                );
                assert_eq!(
                    got,
                    want,
                    "uni_w tier {} != scalar bd={bd} wx={wx} denom={denom} w={w} h={h}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_bi_clip_scalar_matches_golden() {
    let stride = 96usize;
    let disp = Dispatch::scalar();
    for &(bd, max, _, bshift) in HEVC_FINAL_BD {
        let offset = 1i32 << (bshift - 1);
        for &(w, h) in HEVC_SIZES {
            let v = make_plane_i32_signed(80, stride, 0x5151 ^ bd, 40000);
            let src2 = make_plane_src2_i16(80, stride, 0x6262 ^ bd);
            let want = golden_bi(&v, stride, &src2, stride, w, h, offset, bshift, max);
            let got = run_bi(
                &disp,
                &v,
                stride as isize,
                &src2,
                stride as isize,
                w,
                h,
                offset,
                bshift,
                max as u16,
            );
            assert_eq!(got, want, "bi scalar!=golden bd={bd} w={w} h={h}");
        }
    }
}

#[test]
fn mc_bi_clip_detected_matches_scalar() {
    let stride = 96usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(bd, max, _, bshift) in HEVC_FINAL_BD {
        let offset = 1i32 << (bshift - 1);
        for &(w, h) in HEVC_SIZES {
            let v = make_plane_i32_signed(80, stride, 0x7171 ^ bd, 40000);
            let src2 = make_plane_src2_i16(80, stride, 0x8282 ^ bd);
            let want = run_bi(
                &scalar,
                &v,
                stride as isize,
                &src2,
                stride as isize,
                w,
                h,
                offset,
                bshift,
                max as u16,
            );
            let got = run_bi(
                &detected,
                &v,
                stride as isize,
                &src2,
                stride as isize,
                w,
                h,
                offset,
                bshift,
                max as u16,
            );
            assert_eq!(
                got,
                want,
                "bi tier {} != scalar bd={bd} w={w} h={h}",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
            );
        }
    }
}

#[test]
fn mc_bi_w_clip_scalar_matches_golden() {
    let stride = 96usize;
    let disp = Dispatch::scalar();
    for &(bd, max, _, _) in HEVC_FINAL_BD {
        for &(wx0, wx1, denom, ox0_u, ox1_u) in BIW_PARAMS {
            let log2_wd = denom + 14 - bd;
            let rshift = log2_wd + 1;
            let ox0 = ox0_u << (bd - 8);
            let ox1 = ox1_u << (bd - 8);
            let bias = (ox0 + ox1 + 1) * (1i32 << log2_wd);
            for &(w, h) in HEVC_SIZES {
                let v = make_plane_i32_signed(80, stride, 0x9191 ^ bd ^ (wx1 as u32), 40000);
                let src2 = make_plane_src2_i16(80, stride, 0xa2a2 ^ bd ^ (wx0 as u32));
                let want =
                    golden_bi_w(&v, stride, &src2, stride, w, h, wx0, wx1, bias, rshift, max);
                let got = run_bi_w(
                    &disp,
                    &v,
                    stride as isize,
                    &src2,
                    stride as isize,
                    w,
                    h,
                    wx0,
                    wx1,
                    bias,
                    rshift,
                    max as u16,
                );
                assert_eq!(
                    got, want,
                    "bi_w scalar!=golden bd={bd} wx0={wx0} wx1={wx1} denom={denom} w={w} h={h}"
                );
            }
        }
    }
}

#[test]
fn mc_bi_w_clip_detected_matches_scalar() {
    let stride = 96usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(bd, max, _, _) in HEVC_FINAL_BD {
        for &(wx0, wx1, denom, ox0_u, ox1_u) in BIW_PARAMS {
            let log2_wd = denom + 14 - bd;
            let rshift = log2_wd + 1;
            let ox0 = ox0_u << (bd - 8);
            let ox1 = ox1_u << (bd - 8);
            let bias = (ox0 + ox1 + 1) * (1i32 << log2_wd);
            for &(w, h) in HEVC_SIZES {
                let v = make_plane_i32_signed(80, stride, 0xb3b3 ^ bd ^ (wx1 as u32), 40000);
                let src2 = make_plane_src2_i16(80, stride, 0xc4c4 ^ bd ^ (wx0 as u32));
                let want = run_bi_w(
                    &scalar,
                    &v,
                    stride as isize,
                    &src2,
                    stride as isize,
                    w,
                    h,
                    wx0,
                    wx1,
                    bias,
                    rshift,
                    max as u16,
                );
                let got = run_bi_w(
                    &detected,
                    &v,
                    stride as isize,
                    &src2,
                    stride as isize,
                    w,
                    h,
                    wx0,
                    wx1,
                    bias,
                    rshift,
                    max as u16,
                );
                assert_eq!(
                    got,
                    want,
                    "bi_w tier {} != scalar bd={bd} wx0={wx0} wx1={wx1} denom={denom} w={w} h={h}",
                    codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
                );
            }
        }
    }
}

#[test]
fn mc_store_i16_scalar_matches_golden() {
    let stride = 96usize;
    let disp = Dispatch::scalar();
    for &(w, h) in HEVC_SIZES {
        let v = make_plane_i32_signed(80, stride, 0xd5d5, 40000);
        let want = golden_store(&v, stride, w, h);
        let got = run_store(&disp, &v, stride as isize, w, h);
        assert_eq!(got, want, "store scalar!=golden w={w} h={h}");
    }
}

#[test]
fn mc_store_i16_detected_matches_scalar() {
    let stride = 96usize;
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &(w, h) in HEVC_SIZES {
        let v = make_plane_i32_signed(80, stride, 0xe6e6, 40000);
        let want = run_store(&scalar, &v, stride as isize, w, h);
        let got = run_store(&detected, &v, stride as isize, w, h);
        assert_eq!(
            got,
            want,
            "store tier {} != scalar w={w} h={h}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
        );
    }
}

/// Directed test: the `store` narrow WRAPS (`as i16`), it does NOT saturate.
/// A value `> i16::MAX` must become its low-16-bit reinterpretation, not
/// `i16::MAX` — matching ffmpeg-rs `put_luma`'s `int16_t` store.
#[test]
fn mc_store_i16_wraps_not_saturates() {
    let w = 20usize;
    let h = 3usize;
    let stride = w;
    // Column-varying values that straddle i16 bounds, incl. >i16::MAX and <i16::MIN.
    let vals: [i32; 20] = [
        0,
        1,
        -1,
        32767,
        32768,
        40000,
        65535,
        65536,
        70000,
        100000,
        -32768,
        -32769,
        -40000,
        -70000,
        123456,
        -123456,
        2_129_344,
        -2_129_344,
        i16::MAX as i32 + 1,
        i16::MIN as i32 - 1,
    ];
    let mut v = vec![0i32; h * stride];
    for r in 0..h {
        for c in 0..w {
            v[r * stride + c] = vals[c].wrapping_add(r as i32);
        }
    }
    let expected: Vec<i16> = v.iter().map(|&x| x as i16).collect();
    // Sanity: at least one input exceeds i16::MAX and its wrap != saturated.
    assert!(
        vals.iter()
            .any(|&x| x > i16::MAX as i32 && (x as i16) != i16::MAX),
        "test must include a >i16::MAX value whose wrap differs from saturation"
    );
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        let got = run_store(&disp, &v, stride as isize, w, h);
        assert_eq!(
            got,
            expected,
            "store must WRAP (not saturate) at tier {}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
        );
    }
}

// =========================================================================
// HEVC inverse transforms — size 4 IDCT / DST-VII + size-generic DC (v0.3.28)
//
// Independent naive golden = the EXACT ffmpeg-rs `av-codec-h265/transform/
// idct.rs` math (`tr_4_core` / `tr_4x4_luma_core` / `idct_dc`, using the
// REFERENCE c0/c1/c2/c3 intermediates — NOT the production's collapsed matrix)
// reimplemented here, so a typo in any backend (scalar's direct port, SSE2's
// mullo/mulhi across-columns matrix, or AVX2's mullo_epi32 path) is caught.
// Blocks span the full i16 range incl. extremes to drive `clip_int16`
// saturation on BOTH ends.  Every tier (via CODEC_SIMD_FORCE_LEVEL → detect())
// must match the golden byte-for-byte.
// =========================================================================

fn hevc_clip16(x: i32) -> i16 {
    x.clamp(-32768, 32767) as i16
}

/// Independent `tr_4_core` (dsp_template.c TR_4 even/odd butterfly).
fn hevc_ref_tr4(s0: i32, s1: i32, s2: i32, s3: i32) -> [i32; 4] {
    let e0 = 64 * s0 + 64 * s2;
    let e1 = 64 * s0 - 64 * s2;
    let o0 = 83 * s1 + 36 * s3;
    let o1 = 36 * s1 - 83 * s3;
    [e0 + o0, e1 + o1, e1 - o1, e0 - o0]
}

/// Independent `tr_4x4_luma_core` (dsp_template.c DST-VII, reference intermediates).
fn hevc_ref_dst4(s0: i32, s1: i32, s2: i32, s3: i32) -> [i32; 4] {
    let c0 = s0 + s2;
    let c1 = s2 + s3;
    let c2 = s0 - s3;
    let c3 = 74 * s1;
    let d2 = 74 * (s0 - s2 + s3);
    let d0 = 29 * c0 + 55 * c1 + c3;
    let d1 = 55 * c2 - 29 * c1 + c3;
    let d3 = 55 * c0 + 29 * c2 - c3;
    [d0, d1, d2, d3]
}

/// Independent two-pass 4x4 golden driver for a given 4-point core.
fn hevc_ref_4x4(inp: &[i16; 16], bd: u8, core: fn(i32, i32, i32, i32) -> [i32; 4]) -> [i16; 16] {
    let mut c = *inp;
    // Pass 1: columns (step 4), shift 7.
    let shift = 7i32;
    let add = 1i32 << (shift - 1);
    for i in 0..4 {
        let r = core(
            c[i] as i32,
            c[i + 4] as i32,
            c[i + 8] as i32,
            c[i + 12] as i32,
        );
        for k in 0..4 {
            c[i + k * 4] = hevc_clip16((r[k] + add) >> shift);
        }
    }
    // Pass 2: rows (step 1), shift 20 - bd.
    let shift = 20 - bd as i32;
    let add = 1i32 << (shift - 1);
    for i in 0..4 {
        let b = i * 4;
        let r = core(
            c[b] as i32,
            c[b + 1] as i32,
            c[b + 2] as i32,
            c[b + 3] as i32,
        );
        for k in 0..4 {
            c[b + k] = hevc_clip16((r[k] + add) >> shift);
        }
    }
    c
}

fn hevc_ref_idct4(inp: &[i16; 16], bd: u8) -> [i16; 16] {
    hevc_ref_4x4(inp, bd, hevc_ref_tr4)
}
fn hevc_ref_dst4_block(inp: &[i16; 16], bd: u8) -> [i16; 16] {
    hevc_ref_4x4(inp, bd, hevc_ref_dst4)
}

/// Independent `idct_dc` golden — the single fill value.
fn hevc_ref_dc(c0: i16, bd: u8) -> i16 {
    let shift = 14 - bd as i32;
    let add = 1i32 << (shift - 1);
    hevc_clip16((((c0 as i32 + 1) >> 1) + add) >> shift)
}

fn run_idct4(disp: &Dispatch, inp: &[i16; 16], bd: u8) -> [i16; 16] {
    let mut c = *inp;
    // SAFETY: Dispatch only installs fns whose target_feature reqs hold here.
    unsafe {
        (disp.hevc_idct4_i16_inplace)(c.as_mut_ptr(), bd);
    }
    c
}
fn run_dst4(disp: &Dispatch, inp: &[i16; 16], bd: u8) -> [i16; 16] {
    let mut c = *inp;
    // SAFETY: see run_idct4.
    unsafe {
        (disp.hevc_dst4_i16_inplace)(c.as_mut_ptr(), bd);
    }
    c
}
fn run_dc(disp: &Dispatch, inp: &[i16], log2: u32, bd: u8) -> Vec<i16> {
    let mut c = inp.to_vec();
    // SAFETY: see run_idct4; `c.len()` is size*size for `log2`.
    unsafe {
        (disp.hevc_idct_dc_i16_inplace)(c.as_mut_ptr(), log2, bd);
    }
    c
}

/// Deterministic pseudo-random 4x4 block, full i16 range (LCG low 16 bits).
fn hevc_block16(seed: u64) -> [i16; 16] {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    let mut b = [0i16; 16];
    for v in b.iter_mut() {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *v = (s >> 33) as i16;
    }
    b
}

const HEVC_BDS: [u8; 4] = [8, 9, 10, 12];

#[test]
fn hevc_idct4_scalar_matches_naive_reference() {
    let disp = Dispatch::scalar();
    for &bd in &HEVC_BDS {
        for seed in 0..500u64 {
            let b = hevc_block16(seed);
            assert_eq!(
                run_idct4(&disp, &b, bd),
                hevc_ref_idct4(&b, bd),
                "scalar idct4 diverged bd={bd} seed={seed} in={b:?}"
            );
        }
    }
}

#[test]
fn hevc_idct4_detected_dispatch_matches_reference() {
    let disp = Dispatch::detect();
    for &bd in &HEVC_BDS {
        for seed in 0..2000u64 {
            let b = hevc_block16(seed);
            assert_eq!(
                run_idct4(&disp, &b, bd),
                hevc_ref_idct4(&b, bd),
                "tier {} idct4 diverged bd={bd} seed={seed}",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
            );
        }
    }
}

#[test]
fn hevc_dst4_scalar_matches_naive_reference() {
    let disp = Dispatch::scalar();
    for &bd in &HEVC_BDS {
        for seed in 0..500u64 {
            let b = hevc_block16(seed);
            assert_eq!(
                run_dst4(&disp, &b, bd),
                hevc_ref_dst4_block(&b, bd),
                "scalar dst4 diverged bd={bd} seed={seed} in={b:?}"
            );
        }
    }
}

#[test]
fn hevc_dst4_detected_dispatch_matches_reference() {
    let disp = Dispatch::detect();
    for &bd in &HEVC_BDS {
        for seed in 0..2000u64 {
            let b = hevc_block16(seed);
            assert_eq!(
                run_dst4(&disp, &b, bd),
                hevc_ref_dst4_block(&b, bd),
                "tier {} dst4 diverged bd={bd} seed={seed}",
                codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level())
            );
        }
    }
}

/// Saturation corners: blocks that drive `clip_int16` to both bounds.
#[test]
fn hevc_idct4_dst4_saturation_extremes_match_reference() {
    let corner = |v: i16| [v; 16];
    let mut blocks: Vec<[i16; 16]> = vec![
        corner(i16::MAX),
        corner(i16::MIN),
        corner(0),
        [
            i16::MAX,
            i16::MIN,
            i16::MAX,
            i16::MIN,
            i16::MIN,
            i16::MAX,
            i16::MIN,
            i16::MAX,
            i16::MAX,
            i16::MIN,
            i16::MAX,
            i16::MIN,
            i16::MIN,
            i16::MAX,
            i16::MIN,
            i16::MAX,
        ],
    ];
    // DC-only extremes.
    let mut dc = [0i16; 16];
    dc[0] = i16::MAX;
    blocks.push(dc);
    let mut dc = [0i16; 16];
    dc[0] = i16::MIN;
    blocks.push(dc);
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        for &bd in &HEVC_BDS {
            for b in &blocks {
                assert_eq!(
                    run_idct4(&disp, b, bd),
                    hevc_ref_idct4(b, bd),
                    "idct4 saturation diverged bd={bd} in={b:?}"
                );
                assert_eq!(
                    run_dst4(&disp, b, bd),
                    hevc_ref_dst4_block(b, bd),
                    "dst4 saturation diverged bd={bd} in={b:?}"
                );
            }
        }
    }
}

#[test]
fn hevc_idct_dc_detected_dispatch_matches_reference() {
    // sizes 4/8/16/32 -> log2 2/3/4/5.
    for log2 in 2u32..=5 {
        let size = 1usize << log2;
        let n = size * size;
        for disp in [Dispatch::scalar(), Dispatch::detect()] {
            for &bd in &HEVC_BDS {
                for seed in 0..64u64 {
                    let mut blk = vec![0i16; n];
                    // Only coeffs[0] matters; fill the rest with noise to prove it is ignored.
                    let b16 = hevc_block16(seed);
                    for (i, v) in blk.iter_mut().enumerate() {
                        *v = b16[i % 16];
                    }
                    let want = hevc_ref_dc(blk[0], bd);
                    let got = run_dc(&disp, &blk, log2, bd);
                    assert!(
                        got.iter().all(|&v| v == want),
                        "tier {} idct_dc diverged log2={log2} bd={bd} c0={} want={want}",
                        codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
                        blk[0]
                    );
                    assert_eq!(got.len(), n);
                }
            }
        }
    }
}

// --- Fixed KAT vectors extracted from ffmpeg-rs transform/tests.rs ----------

#[test]
fn hevc_idct_dc_flat_block_kat() {
    // 4x4, 8-bit: c0=256 -> all 2. 8x8, 10-bit: c0=100 -> all 3. (ffmpeg-rs KAT.)
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        let mut c = vec![0i16; 16];
        c[0] = 256;
        let got = run_dc(&disp, &c, 2, 8);
        assert!(got.iter().all(|&v| v == 2), "4x4 8-bit DC KAT: {got:?}");
        let mut c = vec![0i16; 64];
        c[0] = 100;
        let got = run_dc(&disp, &c, 3, 10);
        assert!(got.iter().all(|&v| v == 3), "8x8 10-bit DC KAT: {got:?}");
    }
}

#[test]
fn hevc_idct4_dc_kat() {
    // ffmpeg-rs KAT: c[0]=64, bd=8 through the full size-4 IDCT -> all outputs == 1.
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        let mut c = [0i16; 16];
        c[0] = 64;
        let got = run_idct4(&disp, &c, 8);
        assert!(got.iter().all(|&v| v == 1), "idct4 DC KAT: {got:?}");
    }
}

#[test]
fn hevc_dst4_dc_kat() {
    // ffmpeg-rs KAT: c[0]=128, bd=8 -> row0 col2 (the 74*a term) rounds to 1.
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        let mut c = [0i16; 16];
        c[0] = 128;
        let got = run_dst4(&disp, &c, 8);
        assert_eq!(
            got,
            hevc_ref_dst4_block(
                &{
                    let mut x = [0i16; 16];
                    x[0] = 128;
                    x
                },
                8
            ),
            "dst4 DC KAT must match independent oracle"
        );
        assert_eq!(got[2], 1, "dst4 DC KAT: row0 col2 == 1, got {got:?}");
    }
}

// =========================================================================
// HEVC inverse DCT-II — sizes 8 / 16 / 32 (v0.3.29)
//
// Independent naive golden = the EXACT ffmpeg-rs `tr_8_core` / `tr_16_core` /
// `tr_32_core` + `idct_nxn` col/row-pass math (WITH the odd-coefficient
// pruning: col pass `limit2 = min(col_limit+4,H)` shrinking every 4th column,
// row pass `limit = min(col_limit,H)`), reimplemented here over its own copy of
// the 32×32 basis matrix — NOT calls to the primitive.  The production SIMD
// backends take a structurally DIFFERENT route (source pre-zeroing + a full
// uniform butterfly + transpose), so a divergence in either the scalar port or
// the SSE2/AVX2 kernels is caught byte-for-byte.  Random full-range i16 blocks
// across bd 8/10/12 and many `col_limit` (fully populated AND pruned), plus
// saturation extremes and the ffmpeg-rs `transform/tests.rs` KAT vectors.
// Every forced tier (CODEC_SIMD_FORCE_LEVEL → detect()) must match byte-exactly.
// =========================================================================

/// Independent copy of the 32×32 HEVC DCT-II basis matrix (`dsp.c`).
#[rustfmt::skip]
const IDCT_TRANSFORM: [[i8; 32]; 32] = [
    [ 64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64,
      64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64,  64],
    [ 90,  90,  88,  85,  82,  78,  73,  67,  61,  54,  46,  38,  31,  22,  13,   4,
      -4, -13, -22, -31, -38, -46, -54, -61, -67, -73, -78, -82, -85, -88, -90, -90],
    [ 90,  87,  80,  70,  57,  43,  25,   9,  -9, -25, -43, -57, -70, -80, -87, -90,
     -90, -87, -80, -70, -57, -43, -25,  -9,   9,  25,  43,  57,  70,  80,  87,  90],
    [ 90,  82,  67,  46,  22,  -4, -31, -54, -73, -85, -90, -88, -78, -61, -38, -13,
      13,  38,  61,  78,  88,  90,  85,  73,  54,  31,   4, -22, -46, -67, -82, -90],
    [ 89,  75,  50,  18, -18, -50, -75, -89, -89, -75, -50, -18,  18,  50,  75,  89,
      89,  75,  50,  18, -18, -50, -75, -89, -89, -75, -50, -18,  18,  50,  75,  89],
    [ 88,  67,  31, -13, -54, -82, -90, -78, -46,  -4,  38,  73,  90,  85,  61,  22,
     -22, -61, -85, -90, -73, -38,   4,  46,  78,  90,  82,  54,  13, -31, -67, -88],
    [ 87,  57,   9, -43, -80, -90, -70, -25,  25,  70,  90,  80,  43,  -9, -57, -87,
     -87, -57,  -9,  43,  80,  90,  70,  25, -25, -70, -90, -80, -43,   9,  57,  87],
    [ 85,  46, -13, -67, -90, -73, -22,  38,  82,  88,  54,  -4, -61, -90, -78, -31,
      31,  78,  90,  61,   4, -54, -88, -82, -38,  22,  73,  90,  67,  13, -46, -85],
    [ 83,  36, -36, -83, -83, -36,  36,  83,  83,  36, -36, -83, -83, -36,  36,  83,
      83,  36, -36, -83, -83, -36,  36,  83,  83,  36, -36, -83, -83, -36,  36,  83],
    [ 82,  22, -54, -90, -61,  13,  78,  85,  31, -46, -90, -67,   4,  73,  88,  38,
     -38, -88, -73,  -4,  67,  90,  46, -31, -85, -78, -13,  61,  90,  54, -22, -82],
    [ 80,   9, -70, -87, -25,  57,  90,  43, -43, -90, -57,  25,  87,  70,  -9, -80,
     -80,  -9,  70,  87,  25, -57, -90, -43,  43,  90,  57, -25, -87, -70,   9,  80],
    [ 78,  -4, -82, -73,  13,  85,  67, -22, -88, -61,  31,  90,  54, -38, -90, -46,
      46,  90,  38, -54, -90, -31,  61,  88,  22, -67, -85, -13,  73,  82,   4, -78],
    [ 75, -18, -89, -50,  50,  89,  18, -75, -75,  18,  89,  50, -50, -89, -18,  75,
      75, -18, -89, -50,  50,  89,  18, -75, -75,  18,  89,  50, -50, -89, -18,  75],
    [ 73, -31, -90, -22,  78,  67, -38, -90, -13,  82,  61, -46, -88,  -4,  85,  54,
     -54, -85,   4,  88,  46, -61, -82,  13,  90,  38, -67, -78,  22,  90,  31, -73],
    [ 70, -43, -87,   9,  90,  25, -80, -57,  57,  80, -25, -90,  -9,  87,  43, -70,
     -70,  43,  87,  -9, -90, -25,  80,  57, -57, -80,  25,  90,   9, -87, -43,  70],
    [ 67, -54, -78,  38,  85, -22, -90,   4,  90,  13, -88, -31,  82,  46, -73, -61,
      61,  73, -46, -82,  31,  88, -13, -90,  -4,  90,  22, -85, -38,  78,  54, -67],
    [ 64, -64, -64,  64,  64, -64, -64,  64,  64, -64, -64,  64,  64, -64, -64,  64,
      64, -64, -64,  64,  64, -64, -64,  64,  64, -64, -64,  64,  64, -64, -64,  64],
    [ 61, -73, -46,  82,  31, -88, -13,  90,  -4, -90,  22,  85, -38, -78,  54,  67,
     -67, -54,  78,  38, -85, -22,  90,   4, -90,  13,  88, -31, -82,  46,  73, -61],
    [ 57, -80, -25,  90,  -9, -87,  43,  70, -70, -43,  87,   9, -90,  25,  80, -57,
     -57,  80,  25, -90,   9,  87, -43, -70,  70,  43, -87,  -9,  90, -25, -80,  57],
    [ 54, -85,  -4,  88, -46, -61,  82,  13, -90,  38,  67, -78, -22,  90, -31, -73,
      73,  31, -90,  22,  78, -67, -38,  90, -13, -82,  61,  46, -88,   4,  85, -54],
    [ 50, -89,  18,  75, -75, -18,  89, -50, -50,  89, -18, -75,  75,  18, -89,  50,
      50, -89,  18,  75, -75, -18,  89, -50, -50,  89, -18, -75,  75,  18, -89,  50],
    [ 46, -90,  38,  54, -90,  31,  61, -88,  22,  67, -85,  13,  73, -82,   4,  78,
     -78,  -4,  82, -73, -13,  85, -67, -22,  88, -61, -31,  90, -54, -38,  90, -46],
    [ 43, -90,  57,  25, -87,  70,   9, -80,  80,  -9, -70,  87, -25, -57,  90, -43,
     -43,  90, -57, -25,  87, -70,  -9,  80, -80,   9,  70, -87,  25,  57, -90,  43],
    [ 38, -88,  73,  -4, -67,  90, -46, -31,  85, -78,  13,  61, -90,  54,  22, -82,
      82, -22, -54,  90, -61, -13,  78, -85,  31,  46, -90,  67,   4, -73,  88, -38],
    [ 36, -83,  83, -36, -36,  83, -83,  36,  36, -83,  83, -36, -36,  83, -83,  36,
      36, -83,  83, -36, -36,  83, -83,  36,  36, -83,  83, -36, -36,  83, -83,  36],
    [ 31, -78,  90, -61,   4,  54, -88,  82, -38, -22,  73, -90,  67, -13, -46,  85,
     -85,  46,  13, -67,  90, -73,  22,  38, -82,  88, -54,  -4,  61, -90,  78, -31],
    [ 25, -70,  90, -80,  43,   9, -57,  87, -87,  57,  -9, -43,  80, -90,  70, -25,
     -25,  70, -90,  80, -43,  -9,  57, -87,  87, -57,   9,  43, -80,  90, -70,  25],
    [ 22, -61,  85, -90,  73, -38,  -4,  46, -78,  90, -82,  54, -13, -31,  67, -88,
      88, -67,  31,  13, -54,  82, -90,  78, -46,   4,  38, -73,  90, -85,  61, -22],
    [ 18, -50,  75, -89,  89, -75,  50, -18, -18,  50, -75,  89, -89,  75, -50,  18,
      18, -50,  75, -89,  89, -75,  50, -18, -18,  50, -75,  89, -89,  75, -50,  18],
    [ 13, -38,  61, -78,  88, -90,  85, -73,  54, -31,   4,  22, -46,  67, -82,  90,
     -90,  82, -67,  46, -22,  -4,  31, -54,  73, -85,  90, -88,  78, -61,  38, -13],
    [  9, -25,  43, -57,  70, -80,  87, -90,  90, -87,  80, -70,  57, -43,  25,  -9,
      -9,  25, -43,  57, -70,  80, -87,  90, -90,  87, -80,  70, -57,  43, -25,   9],
    [  4, -13,  22, -31,  38, -46,  54, -61,  67, -73,  78, -82,  85, -88,  90, -90,
      90, -90,  88, -85,  82, -78,  73, -67,  61, -54,  46, -38,  31, -22,  13,  -4],
];

/// Independent `tr_8_core` (dsp_template.c TR_8): even `tr_4` + odd `< end`.
fn hevc_ref_tr8(src: &[i32; 8], end: i32) -> [i32; 8] {
    let mut o = [0i32; 4];
    for i in 0..4 {
        let mut j = 1i32;
        while j < end {
            o[i] += IDCT_TRANSFORM[4 * j as usize][i] as i32 * src[j as usize];
            j += 2;
        }
    }
    let e = hevc_ref_tr4(src[0], src[2], src[4], src[6]);
    let mut out = [0i32; 8];
    for i in 0..4 {
        out[i] = e[i] + o[i];
        out[7 - i] = e[i] - o[i];
    }
    out
}

/// Independent `tr_16_core` (even sub-transform hard-codes end = 8).
fn hevc_ref_tr16(src: &[i32; 16], end: i32) -> [i32; 16] {
    let mut o = [0i32; 8];
    for i in 0..8 {
        let mut j = 1i32;
        while j < end {
            o[i] += IDCT_TRANSFORM[2 * j as usize][i] as i32 * src[j as usize];
            j += 2;
        }
    }
    let mut even = [0i32; 8];
    for k in 0..8 {
        even[k] = src[2 * k];
    }
    let e = hevc_ref_tr8(&even, 8);
    let mut out = [0i32; 16];
    for i in 0..8 {
        out[i] = e[i] + o[i];
        out[15 - i] = e[i] - o[i];
    }
    out
}

/// Independent `tr_32_core` (even sub-transform prunes at end/2).
fn hevc_ref_tr32(src: &[i32; 32], end: i32) -> [i32; 32] {
    let mut o = [0i32; 16];
    for i in 0..16 {
        let mut j = 1i32;
        while j < end {
            o[i] += IDCT_TRANSFORM[j as usize][i] as i32 * src[j as usize];
            j += 2;
        }
    }
    let mut even = [0i32; 16];
    for k in 0..16 {
        even[k] = src[2 * k];
    }
    let e = hevc_ref_tr16(&even, end / 2);
    let mut out = [0i32; 32];
    for i in 0..16 {
        out[i] = e[i] + o[i];
        out[31 - i] = e[i] - o[i];
    }
    out
}

/// Independent `tr_N_core` dispatch by size (fills `out[0..h]`).
fn hevc_ref_core(h: usize, src: &[i32], end: i32) -> Vec<i32> {
    match h {
        8 => {
            let mut a = [0i32; 8];
            a.copy_from_slice(&src[..8]);
            hevc_ref_tr8(&a, end).to_vec()
        }
        16 => {
            let mut a = [0i32; 16];
            a.copy_from_slice(&src[..16]);
            hevc_ref_tr16(&a, end).to_vec()
        }
        _ => {
            let mut a = [0i32; 32];
            a.copy_from_slice(&src[..32]);
            hevc_ref_tr32(&a, end).to_vec()
        }
    }
}

/// Independent `idct_nxn` (H ∈ {8,16,32}) golden with the exact pruning.
fn hevc_ref_idct(inp: &[i16], h: usize, col_limit: i32, bd: u8) -> Vec<i16> {
    let mut c = inp.to_vec();
    let limit = col_limit.min(h as i32);
    let mut limit2 = (col_limit + 4).min(h as i32);
    // Pass 1: columns (stride h), shift 7.
    let shift = 7i32;
    let add = 1i32 << (shift - 1);
    let mut s = vec![0i32; h];
    for i in 0..h {
        for k in 0..h {
            s[k] = c[i + k * h] as i32;
        }
        let r = hevc_ref_core(h, &s, limit2);
        for k in 0..h {
            c[i + k * h] = hevc_clip16((r[k] + add) >> shift);
        }
        if limit2 < h as i32 && i % 4 == 0 && i != 0 {
            limit2 -= 4;
        }
    }
    // Pass 2: rows (stride 1), shift 20 - bd.
    let shift = 20 - bd as i32;
    let add = 1i32 << (shift - 1);
    for i in 0..h {
        let base = i * h;
        for k in 0..h {
            s[k] = c[base + k] as i32;
        }
        let r = hevc_ref_core(h, &s, limit);
        for k in 0..h {
            c[base + k] = hevc_clip16((r[k] + add) >> shift);
        }
    }
    c
}

/// Deterministic pseudo-random block of `n` full-range i16 (LCG low 16 bits).
fn hevc_block(seed: u64, n: usize) -> Vec<i16> {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    (0..n)
        .map(|_| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 33) as i16
        })
        .collect()
}

fn run_idct(disp: &Dispatch, inp: &[i16], h: usize, col_limit: i32, bd: u8) -> Vec<i16> {
    let mut c = inp.to_vec();
    // SAFETY: Dispatch only installs fns whose target_feature reqs hold here;
    // `c.len()` is h*h.
    unsafe {
        match h {
            8 => (disp.hevc_idct8_i16_inplace)(c.as_mut_ptr(), col_limit, bd),
            16 => (disp.hevc_idct16_i16_inplace)(c.as_mut_ptr(), col_limit, bd),
            _ => (disp.hevc_idct32_i16_inplace)(c.as_mut_ptr(), col_limit, bd),
        }
    }
    c
}

/// The `col_limit` values exercised: fully populated (>= H → no pruning) AND
/// several pruned bounds (drive the limit2 decrement + tr_32 end/2 nesting).
fn hevc_col_limits(h: usize) -> Vec<i32> {
    let hi = h as i32;
    vec![0, 2, 5, hi / 2, hi - 1, hi, hi + 4, 2 * hi]
}

#[test]
fn hevc_idct8_16_32_scalar_matches_naive_reference() {
    let disp = Dispatch::scalar();
    for &h in &[8usize, 16, 32] {
        for &bd in &HEVC_BDS {
            for &cl in &hevc_col_limits(h) {
                for seed in 0..40u64 {
                    let b = hevc_block(seed, h * h);
                    assert_eq!(
                        run_idct(&disp, &b, h, cl, bd),
                        hevc_ref_idct(&b, h, cl, bd),
                        "scalar idct{h} diverged bd={bd} col_limit={cl} seed={seed}"
                    );
                }
            }
        }
    }
}

#[test]
fn hevc_idct8_16_32_detected_matches_reference() {
    let disp = Dispatch::detect();
    let tier = codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level());
    for &h in &[8usize, 16, 32] {
        for &bd in &HEVC_BDS {
            for &cl in &hevc_col_limits(h) {
                for seed in 0..40u64 {
                    let b = hevc_block(seed.wrapping_add(1000), h * h);
                    assert_eq!(
                        run_idct(&disp, &b, h, cl, bd),
                        hevc_ref_idct(&b, h, cl, bd),
                        "tier {tier} idct{h} diverged bd={bd} col_limit={cl} seed={seed}"
                    );
                }
            }
        }
    }
}

/// Saturation corners: blocks that drive `clip_int16` to both bounds, at both
/// full and pruned `col_limit`.
#[test]
fn hevc_idct8_16_32_saturation_extremes_match_reference() {
    for &h in &[8usize, 16, 32] {
        let n = h * h;
        let mut blocks: Vec<Vec<i16>> = vec![
            vec![i16::MAX; n],
            vec![i16::MIN; n],
            vec![0; n],
            (0..n)
                .map(|i| if i % 2 == 0 { i16::MAX } else { i16::MIN })
                .collect(),
        ];
        // DC-only extremes.
        let mut dc = vec![0i16; n];
        dc[0] = i16::MAX;
        blocks.push(dc);
        let mut dc = vec![0i16; n];
        dc[0] = i16::MIN;
        blocks.push(dc);
        for disp in [Dispatch::scalar(), Dispatch::detect()] {
            for &bd in &HEVC_BDS {
                for &cl in &[h as i32, 3, h as i32 * 2] {
                    for b in &blocks {
                        assert_eq!(
                            run_idct(&disp, b, h, cl, bd),
                            hevc_ref_idct(b, h, cl, bd),
                            "idct{h} saturation diverged bd={bd} col_limit={cl}"
                        );
                    }
                }
            }
        }
    }
}

// --- Fixed KAT vectors extracted from ffmpeg-rs transform/tests.rs ----------

#[test]
fn hevc_idct8_known_pattern_kat() {
    // DC + one horizontal AC + one vertical AC through idct_nxn 8x8, col_limit=8.
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        let mut c = vec![0i16; 64];
        c[0] = 80;
        c[1] = -40;
        c[8] = 24;
        let want = hevc_ref_idct(&c, 8, 8, 8);
        assert_eq!(run_idct(&disp, &c, 8, 8, 8), want, "idct8 KAT vs oracle");
    }
}

#[test]
fn hevc_idct16_full_butterfly_kat() {
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        // (a) DC anchor: c[0]=64, bd=8, col_limit=16 -> all outputs == 1.
        let mut c = vec![0i16; 256];
        c[0] = 64;
        let got = run_idct(&disp, &c, 16, 16, 8);
        assert!(got.iter().all(|&v| v == 1), "16x16 DC anchor must be all 1");
        // (b) Multi-coeff mid-frequency pattern through the full butterfly.
        let mut c = vec![0i16; 256];
        c[0] = 96;
        c[3] = -50;
        c[16] = 28;
        c[17] = 11;
        let want = hevc_ref_idct(&c, 16, 16, 8);
        assert_eq!(run_idct(&disp, &c, 16, 16, 8), want, "idct16 KAT vs oracle");
    }
}

#[test]
fn hevc_idct32_full_butterfly_kat() {
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        // (a) DC anchor: c[0]=64, bd=8, col_limit=32 -> all outputs == 1.
        let mut c = vec![0i16; 1024];
        c[0] = 64;
        let got = run_idct(&disp, &c, 32, 32, 8);
        assert!(got.iter().all(|&v| v == 1), "32x32 DC anchor must be all 1");
        // (b) Multi-coeff mid-frequency pattern through the full butterfly
        // (the only level whose even sub-transform prunes with end/2).
        let mut c = vec![0i16; 1024];
        c[0] = 120;
        c[5] = -64;
        c[32] = 40;
        c[33] = -17;
        let want = hevc_ref_idct(&c, 32, 32, 8);
        assert_eq!(run_idct(&disp, &c, 32, 32, 8), want, "idct32 KAT vs oracle");
    }
}

// =========================================================================
// HEVC SAO band + edge on the u16 plane (v0.3.30)
// =========================================================================
//
// Independent naive golden = the EXACT ffmpeg-rs `av-codec-h265/filter/sao.rs`
// per-pixel math (`sao_band` / `sao_edge`), REIMPLEMENTED here (not via the
// primitive).  Band: `dst = clip(src + table[(src>>shift)&31], 0, max)`.
// Edge: `cat = 2 + cmp(p,n0) + cmp(p,n1)`, `dst = clip(src +
// offsets[edge_idx[cat]], 0, max)`, with edge_idx = [1,2,0,3,4]; a pixel whose
// neighbour is outside the `w×h` region keeps its value.  Every forced tier
// (CODEC_SIMD_FORCE_LEVEL → detect()) must match byte-exactly.

// Includes 12 (max=4095): the i16-lane safety argument rests on "HEVC samples ≤
// 12-bit fit i16", so the design ceiling is proven, not just implied by 8/10-bit.
const SAO_BDS: [u8; 3] = [8, 10, 12];
const SAO_EDGE_IDX: [usize; 5] = [1, 2, 0, 3, 4];
const SAO_POS: [[(i32, i32); 2]; 4] = [
    [(-1, 0), (1, 0)],
    [(0, -1), (0, 1)],
    [(-1, -1), (1, 1)],
    [(1, -1), (-1, 1)],
];

/// Deterministic pseudo-random `u16` plane, values in `[0, max]`.
fn sao_plane(seed: u64, n: usize, max: u16) -> Vec<u16> {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    (0..n)
        .map(|_| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((s >> 40) as u16) % (max + 1)
        })
        .collect()
}

/// Extract the `w×h` region (drop stride padding) for comparison.
fn sao_region(buf: &[u16], stride: usize, w: usize, h: usize) -> Vec<u16> {
    let mut v = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            v.push(buf[y * stride + x]);
        }
    }
    v
}

/// Independent naive golden for the band filter (returns the `w×h` region).
fn sao_band_golden(
    src: &[u16],
    stride: usize,
    w: usize,
    h: usize,
    table: &[i16; 32],
    shift: u32,
    max: u16,
) -> Vec<u16> {
    let maxi = max as i32;
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let s = src[y * stride + x] as i32;
            let band = ((s >> shift) & 31) as usize;
            out.push((s + table[band] as i32).clamp(0, maxi) as u16);
        }
    }
    out
}

/// Independent naive golden for the edge filter (returns the `w×h` region).
fn sao_edge_golden(
    src: &[u16],
    stride: usize,
    w: usize,
    h: usize,
    eo: usize,
    offsets: &[i16; 5],
    max: u16,
) -> Vec<u16> {
    let (dx0, dy0) = SAO_POS[eo][0];
    let (dx1, dy1) = SAO_POS[eo][1];
    let (maxi, wi, hi) = (max as i32, w as i32, h as i32);
    let cmp = |a: i32, b: i32| (a > b) as i32 - (a < b) as i32;
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let s = src[y * stride + x] as i32;
            let (xi, yi) = (x as i32, y as i32);
            let (n0x, n0y) = (xi + dx0, yi + dy0);
            let (n1x, n1y) = (xi + dx1, yi + dy1);
            let oob = n0x < 0
                || n0y < 0
                || n1x < 0
                || n1y < 0
                || n0x >= wi
                || n0y >= hi
                || n1x >= wi
                || n1y >= hi;
            let v = if oob {
                s as u16
            } else {
                let s0 = src[n0y as usize * stride + n0x as usize] as i32;
                let s1 = src[n1y as usize * stride + n1x as usize] as i32;
                let cat = (2 + cmp(s, s0) + cmp(s, s1)) as usize;
                (s + offsets[SAO_EDGE_IDX[cat]] as i32).clamp(0, maxi) as u16
            };
            out.push(v);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn run_sao_band(
    disp: &Dispatch,
    src: &[u16],
    stride: usize,
    w: usize,
    h: usize,
    table: &[i16; 32],
    shift: u32,
    max: u16,
) -> Vec<u16> {
    let mut dst = vec![0u16; h * stride];
    // SAFETY: dst/src span h*stride ≥ (h-1)*stride + w; table has 32 entries.
    unsafe {
        (disp.sao_band_u16)(
            dst.as_mut_ptr(),
            stride as isize,
            src.as_ptr(),
            stride as isize,
            w,
            h,
            table.as_ptr(),
            shift,
            max,
        );
    }
    sao_region(&dst, stride, w, h)
}

/// Band in-place (`dst == src`): the primitive reads-before-writes per pixel.
#[allow(clippy::too_many_arguments)]
fn run_sao_band_inplace(
    disp: &Dispatch,
    src: &[u16],
    stride: usize,
    w: usize,
    h: usize,
    table: &[i16; 32],
    shift: u32,
    max: u16,
) -> Vec<u16> {
    let mut buf = src.to_vec();
    let p = buf.as_mut_ptr();
    // SAFETY: single buffer aliased as src+dst; band only touches its own pixel.
    unsafe {
        (disp.sao_band_u16)(
            p,
            stride as isize,
            p,
            stride as isize,
            w,
            h,
            table.as_ptr(),
            shift,
            max,
        );
    }
    sao_region(&buf, stride, w, h)
}

#[allow(clippy::too_many_arguments)]
fn run_sao_edge(
    disp: &Dispatch,
    src: &[u16],
    stride: usize,
    w: usize,
    h: usize,
    eo: usize,
    offsets: &[i16; 5],
    max: u16,
) -> Vec<u16> {
    let mut dst = vec![0u16; h * stride];
    // SAFETY: dst/src span h*stride; offsets has 5 entries; src≠dst (snapshot).
    unsafe {
        (disp.sao_edge_u16)(
            dst.as_mut_ptr(),
            stride as isize,
            src.as_ptr(),
            stride as isize,
            w,
            h,
            eo as u32,
            offsets.as_ptr(),
            max,
        );
    }
    sao_region(&dst, stride, w, h)
}

/// Build a band offset table like ffmpeg: 4 non-zero entries in a window at
/// `band_position` (wrapping &31), the rest 0.
fn sao_band_table(band_position: usize, offs: [i16; 4]) -> [i16; 32] {
    let mut t = [0i16; 32];
    for k in 0..4 {
        t[(k + band_position) & 31] = offs[k];
    }
    t
}

#[test]
fn hevc_sao_band_scalar_matches_golden() {
    let disp = Dispatch::scalar();
    for &bd in &SAO_BDS {
        let max = ((1u32 << bd) - 1) as u16;
        let shift = (bd - 5) as u32;
        for &stride_extra in &[0usize, 3] {
            for seed in 0..80u64 {
                let (w, h) = (7 + (seed as usize % 10), 5 + (seed as usize % 7));
                let stride = w + stride_extra;
                let src = sao_plane(seed, h * stride, max);
                let band_pos = (seed as usize * 5) % 32;
                let offs = [
                    ((seed % 15) as i16) - 7,
                    ((seed % 11) as i16) - 5,
                    7 - ((seed % 15) as i16),
                    ((seed % 9) as i16) - 4,
                ];
                let table = sao_band_table(band_pos, offs);
                let got = run_sao_band(&disp, &src, stride, w, h, &table, shift, max);
                let want = sao_band_golden(&src, stride, w, h, &table, shift, max);
                assert_eq!(
                    got, want,
                    "scalar sao_band bd={bd} seed={seed} stride={stride}"
                );
                // In-place must match too.
                let got_ip = run_sao_band_inplace(&disp, &src, stride, w, h, &table, shift, max);
                assert_eq!(got_ip, want, "scalar sao_band in-place bd={bd} seed={seed}");
            }
        }
    }
}

#[test]
fn hevc_sao_band_detected_matches_golden() {
    let disp = Dispatch::detect();
    let tier = codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level());
    for &bd in &SAO_BDS {
        let max = ((1u32 << bd) - 1) as u16;
        let shift = (bd - 5) as u32;
        for &stride_extra in &[0usize, 3] {
            for seed in 0..200u64 {
                let (w, h) = (7 + (seed as usize % 20), 5 + (seed as usize % 11));
                let stride = w + stride_extra;
                let src = sao_plane(seed ^ 0xABCD, h * stride, max);
                let band_pos = (seed as usize * 7) % 32;
                let offs = [
                    ((seed % 15) as i16) - 7,
                    ((seed % 11) as i16) - 5,
                    7 - ((seed % 15) as i16),
                    ((seed % 9) as i16) - 4,
                ];
                let table = sao_band_table(band_pos, offs);
                let got = run_sao_band(&disp, &src, stride, w, h, &table, shift, max);
                let want = sao_band_golden(&src, stride, w, h, &table, shift, max);
                assert_eq!(
                    got, want,
                    "tier {tier} sao_band bd={bd} seed={seed} stride={stride}"
                );
                let got_ip = run_sao_band_inplace(&disp, &src, stride, w, h, &table, shift, max);
                assert_eq!(
                    got_ip, want,
                    "tier {tier} sao_band in-place bd={bd} seed={seed}"
                );
            }
        }
    }
}

#[test]
fn hevc_sao_edge_scalar_matches_golden() {
    let disp = Dispatch::scalar();
    for &bd in &SAO_BDS {
        let max = ((1u32 << bd) - 1) as u16;
        for eo in 0..4usize {
            for &stride_extra in &[0usize, 5] {
                for seed in 0..60u64 {
                    let (w, h) = (9 + (seed as usize % 12), 6 + (seed as usize % 9));
                    let stride = w + stride_extra;
                    let src = sao_plane(seed.wrapping_add(eo as u64 * 101), h * stride, max);
                    let offsets = [
                        ((seed % 13) as i16) - 6,
                        ((seed % 15) as i16) - 7,
                        0,
                        7 - ((seed % 15) as i16),
                        6 - ((seed % 13) as i16),
                    ];
                    let got = run_sao_edge(&disp, &src, stride, w, h, eo, &offsets, max);
                    let want = sao_edge_golden(&src, stride, w, h, eo, &offsets, max);
                    assert_eq!(
                        got, want,
                        "scalar sao_edge bd={bd} eo={eo} seed={seed} stride={stride}"
                    );
                }
            }
        }
    }
}

#[test]
fn hevc_sao_edge_detected_matches_golden() {
    let disp = Dispatch::detect();
    let tier = codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level());
    for &bd in &SAO_BDS {
        let max = ((1u32 << bd) - 1) as u16;
        for eo in 0..4usize {
            for &stride_extra in &[0usize, 5] {
                // Widths that straddle the SIMD chunk boundaries (8 / 16 lanes).
                for &w in &[1usize, 2, 3, 8, 9, 16, 17, 33] {
                    for seed in 0..24u64 {
                        let h = 3 + (seed as usize % 9);
                        let stride = w + stride_extra;
                        let src = sao_plane(
                            seed.wrapping_mul(2654435761)
                                .wrapping_add(eo as u64 * 7 + w as u64),
                            h * stride,
                            max,
                        );
                        let offsets = [
                            ((seed % 13) as i16) - 6,
                            ((seed % 15) as i16) - 7,
                            ((seed % 5) as i16) - 2,
                            7 - ((seed % 15) as i16),
                            6 - ((seed % 13) as i16),
                        ];
                        let got = run_sao_edge(&disp, &src, stride, w, h, eo, &offsets, max);
                        let want = sao_edge_golden(&src, stride, w, h, eo, &offsets, max);
                        assert_eq!(
                            got, want,
                            "tier {tier} sao_edge bd={bd} eo={eo} w={w} h={h} seed={seed} stride={stride}"
                        );
                    }
                }
            }
        }
    }
}

/// Saturation extremes: values pinned at 0 / max with offsets that push both
/// past 0 and past max, for band + all edge classes.
#[test]
fn hevc_sao_saturation_extremes_match_golden() {
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        for &bd in &SAO_BDS {
            let max = ((1u32 << bd) - 1) as u16;
            let shift = (bd - 5) as u32;
            let (w, h, stride) = (11usize, 9usize, 14usize);
            // A plane of just {0, 1, max-1, max} tiles → drives clip on both ends.
            let corners = [0u16, 1, max - 1, max];
            let mut src = vec![0u16; h * stride];
            for (i, v) in src.iter_mut().enumerate() {
                *v = corners[i % 4];
            }
            // Band: table pushes strongly negative and positive.
            let table = sao_band_table(
                (max as usize >> shift) & 31,
                [max as i16, -(max as i16), 31, -31],
            );
            let got = run_sao_band(&disp, &src, stride, w, h, &table, shift, max);
            let want = sao_band_golden(&src, stride, w, h, &table, shift, max);
            assert_eq!(got, want, "sao_band saturation bd={bd}");
            // Edge: big offsets in every category.
            let offsets = [max as i16, -(max as i16), 0, max as i16, -(max as i16)];
            for eo in 0..4usize {
                let got = run_sao_edge(&disp, &src, stride, w, h, eo, &offsets, max);
                let want = sao_edge_golden(&src, stride, w, h, eo, &offsets, max);
                assert_eq!(got, want, "sao_edge saturation bd={bd} eo={eo}");
            }
        }
    }
}

/// Picture-boundary handling: on tiny regions every edge pixel is a boundary
/// pixel for some class → verify skipped pixels keep their value (vs golden).
#[test]
fn hevc_sao_edge_boundary_tiny_regions_match_golden() {
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        let bd = 10u8;
        let max = ((1u32 << bd) - 1) as u16;
        let offsets = [5i16, -5, 0, 5, -5];
        for eo in 0..4usize {
            for w in 1..=4usize {
                for h in 1..=4usize {
                    for &stride_extra in &[0usize, 2] {
                        let stride = w + stride_extra;
                        let src = sao_plane((w * 31 + h * 7 + eo * 3) as u64, h * stride, max);
                        let got = run_sao_edge(&disp, &src, stride, w, h, eo, &offsets, max);
                        let want = sao_edge_golden(&src, stride, w, h, eo, &offsets, max);
                        assert_eq!(got, want, "sao_edge tiny boundary eo={eo} w={w} h={h}");
                    }
                }
            }
        }
    }
}

// =========================================================================
// HEVC intra prediction: planar + angular FILL kernels (v0.3.31)
//
// Byte-exact vs ffmpeg-rs `av-codec-h265/pred.rs` (`pred_planar`,
// `pred_angular`). The golden below is an INDEPENDENT reimplementation of the
// pred.rs math (not a call into the primitive): planar `POS(x,y)` and the
// angular 2-tap fill (both the fact==0 and fact!=0 branches). `ref_buf` is
// built in the test exactly as `pred_angular` does (incl. the negative-angle
// `INV_ANGLE` back-projection) and fed to the primitive.
//
// Every dst uses `dst_stride = size + pad` and is pre-filled with a SENTINEL
// far outside the pixel range; the full buffer is compared, so any write into
// the pad columns (a transpose/stride bug) is caught.
// =========================================================================

/// `INTRA_PRED_ANGLE[mode-2]` (spec Table 8-5), copied for the independent golden.
const INTRA_PRED_ANGLE_G: [i32; 33] = [
    32, 26, 21, 17, 13, 9, 5, 2, 0, -2, -5, -9, -13, -17, -21, -26, -32, -26, -21, -17, -13, -9,
    -5, -2, 0, 2, 5, 9, 13, 17, 21, 26, 32,
];

/// `INV_ANGLE[mode-11]` (spec Table 8-6), copied for the independent golden.
const INV_ANGLE_G: [i32; 15] = [
    -4096, -1638, -910, -630, -482, -390, -315, -256, -315, -390, -482, -630, -910, -1638, -4096,
];

/// Pixel value never produced by a valid prediction (outputs are `<= 4095`),
/// so surviving pad columns / untouched cells stay identifiable.
const INTRA_SENTINEL: u16 = 0xBEEF;

/// Build the `top`/`left` reference rows + corner. `pattern`: 0 = pseudo-random
/// pixels, 1 = saturated (all `max`), 2 = alternating `0`/`max` (exposes any
/// i16 overflow of the i32-accumulate contract). Length `3*size` is a safe
/// superset of every index `pred_planar`/`pred_angular` can read.
fn intra_refs(seed: u64, size: usize, max: u16, pattern: u8) -> (Vec<u16>, Vec<u16>, u16) {
    let n = 3 * size;
    let mut top = vec![0u16; n];
    let mut left = vec![0u16; n];
    let mut st = seed;
    match pattern {
        1 => {
            top.iter_mut().for_each(|v| *v = max);
            left.iter_mut().for_each(|v| *v = max);
        }
        2 => {
            for (i, v) in top.iter_mut().enumerate() {
                *v = if i % 2 == 0 { 0 } else { max };
            }
            for (i, v) in left.iter_mut().enumerate() {
                *v = if i % 2 == 0 { max } else { 0 };
            }
        }
        _ => {
            top.iter_mut()
                .for_each(|v| *v = (lcg_next(&mut st) % (max as u32 + 1)) as u16);
            left.iter_mut()
                .for_each(|v| *v = (lcg_next(&mut st) % (max as u32 + 1)) as u16);
        }
    }
    let corner = match pattern {
        1 | 2 => max,
        _ => (lcg_next(&mut st) % (max as u32 + 1)) as u16,
    };
    (top, left, corner)
}

/// Independent naive golden for [`codec_simd::hevc_intra_planar_u16`].
fn planar_golden(top: &[u16], left: &[u16], log2_size: u32, dst_stride: usize) -> Vec<u16> {
    let size = 1usize << log2_size;
    let mut dst = vec![INTRA_SENTINEL; size * dst_stride];
    let top_size = top[size] as i32;
    let left_size = left[size] as i32;
    for y in 0..size {
        let ty = left[y] as i32;
        for x in 0..size {
            let v = ((size as i32 - 1 - x as i32) * ty
                + (x as i32 + 1) * top_size
                + (size as i32 - 1 - y as i32) * (top[x] as i32)
                + (y as i32 + 1) * left_size
                + size as i32)
                >> (log2_size + 1);
            dst[y * dst_stride + x] = v as u16;
        }
    }
    dst
}

fn run_planar(
    disp: &Dispatch,
    top: &[u16],
    left: &[u16],
    log2_size: u32,
    dst_stride: usize,
) -> Vec<u16> {
    let size = 1usize << log2_size;
    let mut dst = vec![INTRA_SENTINEL; size * dst_stride];
    // SAFETY: Dispatch only installs fns whose target_feature reqs hold here;
    // geometry sized correctly (top/left length 3*size >= size+1).
    unsafe {
        (disp.hevc_intra_planar_u16)(
            dst.as_mut_ptr(),
            dst_stride as isize,
            top.as_ptr(),
            left.as_ptr(),
            log2_size,
        );
    }
    dst
}

/// Build `ref_buf` (the C `ref` array + `INV_ANGLE` back-projection) exactly as
/// `pred_angular` does; returns `(ref_buf, bias)` with `bias = size`.
fn build_ref_buf(
    top: &[u16],
    left: &[u16],
    corner: u16,
    size: usize,
    angle: i32,
    mode: u8,
    vertical: bool,
) -> (Vec<i32>, usize) {
    let bias = size;
    let cap = 3 * size + 4;
    let mut ref_buf = vec![0i32; cap];
    let isize_ = size as i32;
    let last = (isize_ * angle) >> 5;
    if vertical {
        for k in 0..=(2 * size) {
            let idx = k as i32 - 1;
            ref_buf[k + bias] = if idx < 0 {
                corner as i32
            } else {
                top[idx as usize] as i32
            };
        }
        if angle < 0 && last < -1 {
            for x in 0..=isize_ {
                let idx = x - 1;
                ref_buf[(x + bias as i32) as usize] = if idx < 0 {
                    corner as i32
                } else {
                    top[idx as usize] as i32
                };
            }
            let inv = INV_ANGLE_G[mode as usize - 11];
            let mut x = last;
            while x <= -1 {
                let li = -1 + ((x * inv + 128) >> 8);
                ref_buf[(x + bias as i32) as usize] = if li < 0 {
                    corner as i32
                } else {
                    left[li as usize] as i32
                };
                x += 1;
            }
        }
    } else {
        for k in 0..=(2 * size) {
            let idx = k as i32 - 1;
            ref_buf[k + bias] = if idx < 0 {
                corner as i32
            } else {
                left[idx as usize] as i32
            };
        }
        if angle < 0 && last < -1 {
            for x in 0..=isize_ {
                let idx = x - 1;
                ref_buf[(x + bias as i32) as usize] = if idx < 0 {
                    corner as i32
                } else {
                    left[idx as usize] as i32
                };
            }
            let inv = INV_ANGLE_G[mode as usize - 11];
            let mut x = last;
            while x <= -1 {
                let ti = -1 + ((x * inv + 128) >> 8);
                ref_buf[(x + bias as i32) as usize] = if ti < 0 {
                    corner as i32
                } else {
                    top[ti as usize] as i32
                };
                x += 1;
            }
        }
    }
    (ref_buf, bias)
}

/// Independent naive golden for [`codec_simd::hevc_intra_angular_u16`] (both the
/// fact==0 and fact!=0 branches; vertical `(y,x)=(r,j)`, horizontal `(y,x)=(j,r)`).
fn angular_golden(
    ref_buf: &[i32],
    bias: usize,
    size: usize,
    angle: i32,
    vertical: bool,
    dst_stride: usize,
) -> Vec<u16> {
    let mut dst = vec![INTRA_SENTINEL; size * dst_stride];
    for r in 0..size {
        let ra = r as i32 + 1;
        let idx = (ra * angle) >> 5;
        let fact = (ra * angle) & 31;
        for j in 0..size {
            let base = (idx + 1 + j as i32 + bias as i32) as usize;
            let v = if fact != 0 {
                let r1 = ref_buf[base];
                let r2 = ref_buf[base + 1];
                (((32 - fact) * r1 + fact * r2 + 16) >> 5) as u16
            } else {
                ref_buf[base] as u16
            };
            let (y, x) = if vertical { (r, j) } else { (j, r) };
            dst[y * dst_stride + x] = v;
        }
    }
    dst
}

fn run_angular(
    disp: &Dispatch,
    ref_buf: &[i32],
    bias: usize,
    size: usize,
    angle: i32,
    vertical: bool,
    dst_stride: usize,
) -> Vec<u16> {
    let mut dst = vec![INTRA_SENTINEL; size * dst_stride];
    // SAFETY: ref_buf sized 3*size+4 covers the tap window; dst sized correctly.
    unsafe {
        (disp.hevc_intra_angular_u16)(
            dst.as_mut_ptr(),
            dst_stride as isize,
            ref_buf.as_ptr(),
            bias,
            size,
            angle,
            vertical,
        );
    }
    dst
}

const INTRA_BDS: [u8; 3] = [8, 10, 12];
const INTRA_LOG2: [u32; 4] = [2, 3, 4, 5]; // sizes 4/8/16/32

#[test]
fn hevc_intra_planar_scalar_matches_naive_golden() {
    let disp = Dispatch::scalar();
    for &log2 in &INTRA_LOG2 {
        let size = 1usize << log2;
        let stride = size + 8; // pad columns must stay untouched
        for &bd in &INTRA_BDS {
            let max = ((1u32 << bd) - 1) as u16;
            for pat in 0u8..3 {
                let (top, left, _c) =
                    intra_refs(0x1234 + log2 as u64 * 7 + pat as u64, size, max, pat);
                let want = planar_golden(&top, &left, log2, stride);
                let got = run_planar(&disp, &top, &left, log2, stride);
                assert_eq!(got, want, "planar scalar log2={log2} bd={bd} pat={pat}");
            }
        }
    }
}

#[test]
fn hevc_intra_planar_detected_matches_golden() {
    let disp = Dispatch::detect();
    let tier = codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level());
    for &log2 in &INTRA_LOG2 {
        let size = 1usize << log2;
        let stride = size + 8;
        for &bd in &INTRA_BDS {
            let max = ((1u32 << bd) - 1) as u16;
            for pat in 0u8..3 {
                let (top, left, _c) =
                    intra_refs(0x99 + log2 as u64 * 5 + pat as u64, size, max, pat);
                let want = planar_golden(&top, &left, log2, stride);
                let got = run_planar(&disp, &top, &left, log2, stride);
                assert_eq!(
                    got, want,
                    "planar tier={tier} log2={log2} bd={bd} pat={pat}"
                );
            }
        }
    }
}

#[test]
fn hevc_intra_angular_scalar_matches_naive_golden() {
    let disp = Dispatch::scalar();
    for &log2 in &INTRA_LOG2 {
        let size = 1usize << log2;
        let stride = size + 8;
        for &bd in &INTRA_BDS {
            let max = ((1u32 << bd) - 1) as u16;
            for pat in 0u8..3 {
                let (top, left, corner) =
                    intra_refs(0x5a5a + log2 as u64 * 11 + pat as u64, size, max, pat);
                for mode in 2u8..=34 {
                    let angle = INTRA_PRED_ANGLE_G[mode as usize - 2];
                    let vertical = mode >= 18;
                    let (ref_buf, bias) =
                        build_ref_buf(&top, &left, corner, size, angle, mode, vertical);
                    let want = angular_golden(&ref_buf, bias, size, angle, vertical, stride);
                    let got = run_angular(&disp, &ref_buf, bias, size, angle, vertical, stride);
                    assert_eq!(
                        got, want,
                        "angular scalar log2={log2} bd={bd} pat={pat} mode={mode}"
                    );
                }
            }
        }
    }
}

#[test]
fn hevc_intra_angular_detected_matches_golden() {
    let disp = Dispatch::detect();
    let tier = codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level());
    for &log2 in &INTRA_LOG2 {
        let size = 1usize << log2;
        let stride = size + 8;
        for &bd in &INTRA_BDS {
            let max = ((1u32 << bd) - 1) as u16;
            for pat in 0u8..3 {
                let (top, left, corner) =
                    intra_refs(0xC0DE + log2 as u64 * 13 + pat as u64, size, max, pat);
                for mode in 2u8..=34 {
                    let angle = INTRA_PRED_ANGLE_G[mode as usize - 2];
                    let vertical = mode >= 18;
                    let (ref_buf, bias) =
                        build_ref_buf(&top, &left, corner, size, angle, mode, vertical);
                    let want = angular_golden(&ref_buf, bias, size, angle, vertical, stride);
                    let got = run_angular(&disp, &ref_buf, bias, size, angle, vertical, stride);
                    assert_eq!(
                        got, want,
                        "angular tier={tier} log2={log2} bd={bd} pat={pat} mode={mode}"
                    );
                }
            }
        }
    }
}

/// `fact` extremes {1, 31} and a negative-`idx` case via SYNTHETIC angles (the
/// primitive computes `idx`/`fact` internally from `angle`, so these hit
/// `fact==1`/`fact==31`/negative-`idx` deterministically at every row). Also
/// covers both negative-angle sub-cases through the mode sweep above:
/// `last < -1` (back-projected, e.g. mode 19 size 32) and `last >= -1`
/// (non-back-projected, e.g. size 4 angle -2 → last -1).
#[test]
fn hevc_intra_angular_fact_extremes_synthetic() {
    let disp = Dispatch::detect();
    let scalar = Dispatch::scalar();
    for &log2 in &INTRA_LOG2 {
        let size = 1usize << log2;
        let stride = size + 8;
        let max = 4095u16; // 12-bit — the i16-overflow stressor for the 2-tap MAC
        // pattern 2 (alt 0/max) maximises the per-tap magnitude spread.
        let (top, left, corner) = intra_refs(0x7777 + log2 as u64, size, max, 2);
        for &angle in &[1i32, 31, -1] {
            let vertical_cases = [true, false];
            for &vertical in &vertical_cases {
                let (ref_buf, bias) = build_ref_buf(&top, &left, corner, size, angle, 2, vertical);
                let want = angular_golden(&ref_buf, bias, size, angle, vertical, stride);
                let got_s = run_angular(&scalar, &ref_buf, bias, size, angle, vertical, stride);
                let got_d = run_angular(&disp, &ref_buf, bias, size, angle, vertical, stride);
                assert_eq!(
                    got_s, want,
                    "angular fact-extreme scalar log2={log2} angle={angle} vert={vertical}"
                );
                assert_eq!(
                    got_d, want,
                    "angular fact-extreme detect log2={log2} angle={angle} vert={vertical}"
                );
            }
        }
    }
}

/// G3 bounded-load focus: `size == 4` (cap `3*4+4 = 16` i32; a fixed 8-lane load
/// at `idx+2+bias` must NOT over-read). All modes + both saturated patterns;
/// suitable for `cargo +nightly miri test hevc_intra_angular_size4_bounds`.
#[test]
fn hevc_intra_angular_size4_bounds() {
    let size = 4usize;
    let stride = size + 8;
    let max = 4095u16;
    for pat in 1u8..3 {
        let (top, left, corner) = intra_refs(0x4004 + pat as u64, size, max, pat);
        for mode in 2u8..=34 {
            let angle = INTRA_PRED_ANGLE_G[mode as usize - 2];
            let vertical = mode >= 18;
            let (ref_buf, bias) = build_ref_buf(&top, &left, corner, size, angle, mode, vertical);
            let want = angular_golden(&ref_buf, bias, size, angle, vertical, stride);
            for disp in [Dispatch::scalar(), Dispatch::detect()] {
                let got = run_angular(&disp, &ref_buf, bias, size, angle, vertical, stride);
                assert_eq!(got, want, "angular size4 bounds pat={pat} mode={mode}");
            }
        }
    }
}

// =========================================================================
// HEVC deblocking filter — luma strong/weak + chroma weak (v0.3.32)
//
// The goldens below INDEPENDENTLY reimplement the ffmpeg-rs
// `av-codec-h265/filter/deblock.rs` kernel math (they do NOT call the
// primitives). Both edge orientations are exercised by swapping
// `xstride`/`ystride` (vertical = 1/stride, horizontal = stride/1) — the
// fragile transpose path. bd 8/10/12, tc/nd extremes, the `|delta0| < 10*tc`
// gate boundary, a padded plane with pad-untouched assert, and a size=4
// Miri-ready bounds case.
// =========================================================================

const DEBLOCK_BDS: [u8; 3] = [8, 10, 12];

/// Build a padded plane (`stride` cols × `height` rows) of samples in
/// `[0, max]`. `pattern`: 0 random, 1 all-0, 2 all-max, 3 alternating 0/max
/// (clip-saturation stressor), 4 deterministic ramp.
fn deblock_plane(seed: u64, stride: usize, height: usize, max: u16, pattern: u8) -> Vec<u16> {
    let n = stride * height;
    let mut p = vec![0u16; n];
    let mut st = seed;
    for (i, v) in p.iter_mut().enumerate() {
        *v = match pattern {
            1 => 0,
            2 => max,
            3 => {
                if i % 2 == 0 {
                    0
                } else {
                    max
                }
            }
            4 => ((i as u32 * 37 + 5) % (max as u32 + 1)) as u16,
            _ => (lcg_next(&mut st) % (max as u32 + 1)) as u16,
        };
    }
    p
}

#[inline]
fn dclip(x: i32, lo: i32, hi: i32) -> i32 {
    x.clamp(lo, hi)
}
#[inline]
fn dclip_pixel(x: i32, max: i32) -> u16 {
    x.clamp(0, max) as u16
}

/// Independent golden for [`codec_simd::hevc_deblock_luma_strong_u16`].
fn golden_luma_strong(
    plane: &mut [u16],
    base: usize,
    xstride: isize,
    ystride: isize,
    tc: i32,
    tc2: i32,
    tc3: i32,
) {
    for d in 0..4isize {
        let off = base as isize + d * ystride;
        let idx = |k: isize| (off + k * xstride) as usize;
        let p3 = plane[idx(-4)] as i32;
        let p2 = plane[idx(-3)] as i32;
        let p1 = plane[idx(-2)] as i32;
        let p0 = plane[idx(-1)] as i32;
        let q0 = plane[idx(0)] as i32;
        let q1 = plane[idx(1)] as i32;
        let q2 = plane[idx(2)] as i32;
        let q3 = plane[idx(3)] as i32;
        let np0 = p0
            + dclip(
                ((p2 + 2 * p1 + 2 * p0 + 2 * q0 + q1 + 4) >> 3) - p0,
                -tc3,
                tc3,
            );
        let np1 = p1 + dclip(((p2 + p1 + p0 + q0 + 2) >> 2) - p1, -tc2, tc2);
        let np2 = p2 + dclip(((2 * p3 + 3 * p2 + p1 + p0 + q0 + 4) >> 3) - p2, -tc, tc);
        let nq0 = q0
            + dclip(
                ((p1 + 2 * p0 + 2 * q0 + 2 * q1 + q2 + 4) >> 3) - q0,
                -tc3,
                tc3,
            );
        let nq1 = q1 + dclip(((p0 + q0 + q1 + q2 + 2) >> 2) - q1, -tc2, tc2);
        let nq2 = q2 + dclip(((2 * q3 + 3 * q2 + q1 + q0 + p0 + 4) >> 3) - q2, -tc, tc);
        plane[idx(-3)] = np2 as u16;
        plane[idx(-2)] = np1 as u16;
        plane[idx(-1)] = np0 as u16;
        plane[idx(0)] = nq0 as u16;
        plane[idx(1)] = nq1 as u16;
        plane[idx(2)] = nq2 as u16;
    }
}

/// Independent golden for [`codec_simd::hevc_deblock_luma_weak_u16`].
fn golden_luma_weak(
    plane: &mut [u16],
    base: usize,
    xstride: isize,
    ystride: isize,
    tc: i32,
    nd_p: i32,
    nd_q: i32,
    max: i32,
) {
    let tc_2 = tc >> 1;
    for d in 0..4isize {
        let off = base as isize + d * ystride;
        let idx = |k: isize| (off + k * xstride) as usize;
        let p2 = plane[idx(-3)] as i32;
        let p1 = plane[idx(-2)] as i32;
        let p0 = plane[idx(-1)] as i32;
        let q0 = plane[idx(0)] as i32;
        let q1 = plane[idx(1)] as i32;
        let q2 = plane[idx(2)] as i32;
        let mut delta0 = (9 * (q0 - p0) - 3 * (q1 - p1) + 8) >> 4;
        if delta0.abs() < 10 * tc {
            delta0 = dclip(delta0, -tc, tc);
            plane[idx(-1)] = dclip_pixel(p0 + delta0, max);
            plane[idx(0)] = dclip_pixel(q0 - delta0, max);
            if nd_p > 1 {
                let dp1 = dclip((((p2 + p0 + 1) >> 1) - p1 + delta0) >> 1, -tc_2, tc_2);
                plane[idx(-2)] = dclip_pixel(p1 + dp1, max);
            }
            if nd_q > 1 {
                let dq1 = dclip((((q2 + q0 + 1) >> 1) - q1 - delta0) >> 1, -tc_2, tc_2);
                plane[idx(1)] = dclip_pixel(q1 + dq1, max);
            }
        }
    }
}

/// Independent golden for [`codec_simd::hevc_deblock_chroma_weak_u16`].
fn golden_chroma_weak(
    plane: &mut [u16],
    base: usize,
    xstride: isize,
    ystride: isize,
    size: usize,
    tc: i32,
    max: i32,
) {
    for d in 0..size as isize {
        let off = base as isize + d * ystride;
        let idx = |k: isize| (off + k * xstride) as usize;
        let p1 = plane[idx(-2)] as i32;
        let p0 = plane[idx(-1)] as i32;
        let q0 = plane[idx(0)] as i32;
        let q1 = plane[idx(1)] as i32;
        let delta0 = dclip((((q0 - p0) * 4) + p1 - q1 + 4) >> 3, -tc, tc);
        plane[idx(-1)] = dclip_pixel(p0 + delta0, max);
        plane[idx(0)] = dclip_pixel(q0 - delta0, max);
    }
}

fn run_luma_strong(
    disp: &Dispatch,
    plane: &mut [u16],
    base: usize,
    xstride: isize,
    ystride: isize,
    tc: i32,
    tc2: i32,
    tc3: i32,
    max: i32,
) {
    // SAFETY: Dispatch installs only fns whose target_feature reqs hold here;
    // geometry is verified in-bounds by the caller's plane/base choice.
    unsafe {
        (disp.hevc_deblock_luma_strong_u16)(
            plane.as_mut_ptr(),
            base,
            xstride,
            ystride,
            tc,
            tc2,
            tc3,
            max,
        );
    }
}
fn run_luma_weak(
    disp: &Dispatch,
    plane: &mut [u16],
    base: usize,
    xstride: isize,
    ystride: isize,
    tc: i32,
    nd_p: i32,
    nd_q: i32,
    max: i32,
) {
    unsafe {
        (disp.hevc_deblock_luma_weak_u16)(
            plane.as_mut_ptr(),
            base,
            xstride,
            ystride,
            tc,
            nd_p,
            nd_q,
            max,
        );
    }
}
fn run_chroma_weak(
    disp: &Dispatch,
    plane: &mut [u16],
    base: usize,
    xstride: isize,
    ystride: isize,
    size: usize,
    tc: i32,
    max: i32,
) {
    unsafe {
        (disp.hevc_deblock_chroma_weak_u16)(
            plane.as_mut_ptr(),
            base,
            xstride,
            ystride,
            size,
            tc,
            max,
        );
    }
}

/// The two edge orientations over a fixed padded geometry (stride 24 × 24 rows,
/// `base` centred at row 12 col 12): `(xstride, ystride, label)`.
const DEBLOCK_STRIDE: usize = 24;
const DEBLOCK_HEIGHT: usize = 24;
const DEBLOCK_BASE: usize = 12 * DEBLOCK_STRIDE + 12;
fn deblock_orientations() -> [(isize, isize, &'static str); 2] {
    [
        (1, DEBLOCK_STRIDE as isize, "vertical"), // xstride=1, ystride=stride
        (DEBLOCK_STRIDE as isize, 1, "horizontal"), // xstride=stride, ystride=1
    ]
}

/// tc bounds sweep: 0 (identity/no-op corner), small (weak-gate boundary),
/// through large (clips rarely activate).
const DEBLOCK_TCS: [i32; 7] = [0, 1, 2, 5, 13, 48, 400];

fn deblock_check_strong(disp: &Dispatch, tier: &str) {
    for (xs, ys, olbl) in deblock_orientations() {
        for &bd in &DEBLOCK_BDS {
            let max = ((1u32 << bd) - 1) as i32;
            let maxu = max as u16;
            for pat in 0u8..5 {
                for &tc in &DEBLOCK_TCS {
                    let tc2x = tc << 1; // reference passes tc<<1 for all three
                    let seed = 0xD00D + pat as u64 * 131 + (tc as u64) * 7 + xs as u64;
                    let base = deblock_plane(seed, DEBLOCK_STRIDE, DEBLOCK_HEIGHT, maxu, pat);
                    let mut g = base.clone();
                    let mut p = base.clone();
                    golden_luma_strong(&mut g, DEBLOCK_BASE, xs, ys, tc2x, tc2x, tc2x);
                    run_luma_strong(disp, &mut p, DEBLOCK_BASE, xs, ys, tc2x, tc2x, tc2x, max);
                    assert_eq!(g, p, "strong tier={tier} {olbl} bd={bd} pat={pat} tc={tc}");
                    // The strong filter is delta-clip only: every output must be
                    // provably in [0, max] (justifies the no-clip u16 store).
                    assert!(
                        p.iter().all(|&v| v as i32 <= max),
                        "strong out-of-range tier={tier} {olbl} bd={bd} pat={pat} tc={tc}"
                    );
                }
            }
        }
    }
}

#[test]
fn hevc_deblock_luma_strong_scalar_matches_golden() {
    deblock_check_strong(&Dispatch::scalar(), "scalar");
}

#[test]
fn hevc_deblock_luma_strong_detected_matches_golden() {
    let tier = codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level());
    deblock_check_strong(&Dispatch::detect(), tier);
}

fn deblock_check_weak(disp: &Dispatch, tier: &str) {
    for (xs, ys, olbl) in deblock_orientations() {
        for &bd in &DEBLOCK_BDS {
            let max = ((1u32 << bd) - 1) as i32;
            let maxu = max as u16;
            for pat in 0u8..5 {
                for &tc in &DEBLOCK_TCS {
                    for &nd_p in &[1i32, 2] {
                        for &nd_q in &[1i32, 2] {
                            let seed = 0xBEE5
                                + pat as u64 * 97
                                + (tc as u64) * 11
                                + nd_p as u64 * 3
                                + nd_q as u64
                                + xs as u64;
                            let base =
                                deblock_plane(seed, DEBLOCK_STRIDE, DEBLOCK_HEIGHT, maxu, pat);
                            let mut g = base.clone();
                            let mut p = base.clone();
                            golden_luma_weak(&mut g, DEBLOCK_BASE, xs, ys, tc, nd_p, nd_q, max);
                            run_luma_weak(disp, &mut p, DEBLOCK_BASE, xs, ys, tc, nd_p, nd_q, max);
                            assert_eq!(
                                g, p,
                                "weak tier={tier} {olbl} bd={bd} pat={pat} tc={tc} nd_p={nd_p} nd_q={nd_q}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn hevc_deblock_luma_weak_scalar_matches_golden() {
    deblock_check_weak(&Dispatch::scalar(), "scalar");
}

#[test]
fn hevc_deblock_luma_weak_detected_matches_golden() {
    let tier = codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level());
    deblock_check_weak(&Dispatch::detect(), tier);
}

fn deblock_check_chroma(disp: &Dispatch, tier: &str) {
    for (xs, ys, olbl) in deblock_orientations() {
        for &bd in &DEBLOCK_BDS {
            let max = ((1u32 << bd) - 1) as i32;
            let maxu = max as u16;
            for pat in 0u8..5 {
                for &tc in &DEBLOCK_TCS {
                    let seed = 0xC0FF + pat as u64 * 53 + (tc as u64) * 17 + xs as u64;
                    let base = deblock_plane(seed, DEBLOCK_STRIDE, DEBLOCK_HEIGHT, maxu, pat);
                    let mut g = base.clone();
                    let mut p = base.clone();
                    golden_chroma_weak(&mut g, DEBLOCK_BASE, xs, ys, 4, tc, max);
                    run_chroma_weak(disp, &mut p, DEBLOCK_BASE, xs, ys, 4, tc, max);
                    assert_eq!(g, p, "chroma tier={tier} {olbl} bd={bd} pat={pat} tc={tc}");
                }
            }
        }
    }
}

#[test]
fn hevc_deblock_chroma_weak_scalar_matches_golden() {
    deblock_check_chroma(&Dispatch::scalar(), "scalar");
}

#[test]
fn hevc_deblock_chroma_weak_detected_matches_golden() {
    let tier = codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level());
    deblock_check_chroma(&Dispatch::detect(), tier);
}

/// Discriminating weak cases: (a) gate-PASS with `nd_p == 1` must leave the p1
/// column byte-unchanged; (b) gate-FAIL (`tc == 0` → `|delta0| < 0` never) must
/// leave the whole window byte-unchanged. Both checked directly against the
/// pre-call snapshot (independent of the golden).
#[test]
fn hevc_deblock_weak_gate_and_nd_discriminating() {
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        for (xs, ys, olbl) in deblock_orientations() {
            let max = 1023i32; // 10-bit
            // (a) A ramp guarantees a non-zero delta0 and, with a large tc,
            //     the gate passes on every line; nd_p == 1 must NOT touch p1.
            let orig = deblock_plane(0x11, DEBLOCK_STRIDE, DEBLOCK_HEIGHT, max as u16, 4);
            let mut p = orig.clone();
            run_luma_weak(&disp, &mut p, DEBLOCK_BASE, xs, ys, 200, 1, 1, max);
            for d in 0..4isize {
                let off = DEBLOCK_BASE as isize + d * ys;
                let p1i = (off - 2 * xs) as usize;
                let q1i = (off + xs) as usize;
                assert_eq!(
                    p.get(p1i),
                    orig.get(p1i),
                    "gate-pass nd_p=1 touched p1 {olbl}"
                );
                assert_eq!(
                    p.get(q1i),
                    orig.get(q1i),
                    "gate-pass nd_q=1 touched q1 {olbl}"
                );
            }
            // (b) tc == 0 → gate can never pass → whole plane byte-identical.
            let mut p0 = orig.clone();
            run_luma_weak(&disp, &mut p0, DEBLOCK_BASE, xs, ys, 0, 2, 2, max);
            assert_eq!(p0, orig, "gate-fail (tc=0) modified the plane {olbl}");
        }
    }
}

/// Padded-plane guard: the primitive must touch ONLY the filter window — the
/// far pad column/row of the 24×24 plane must survive byte-identical.
#[test]
fn hevc_deblock_pad_untouched() {
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        for (xs, ys, olbl) in deblock_orientations() {
            let max = 4095i32;
            let orig = deblock_plane(0x5151, DEBLOCK_STRIDE, DEBLOCK_HEIGHT, max as u16, 0);
            let mut p = orig.clone();
            run_luma_strong(&disp, &mut p, DEBLOCK_BASE, xs, ys, 24, 24, 24, max);
            // Row 0 and the last column are far outside the centred window.
            for c in 0..DEBLOCK_STRIDE {
                assert_eq!(p[c], orig[c], "strong touched row 0 col {c} {olbl}");
            }
            for r in 0..DEBLOCK_HEIGHT {
                let last = r * DEBLOCK_STRIDE + (DEBLOCK_STRIDE - 1);
                assert_eq!(
                    p[last], orig[last],
                    "strong touched last col row {r} {olbl}"
                );
            }
        }
    }
}

/// G3 bounded-load focus: a TIGHT plane where the size=4 chroma window exactly
/// fits (no slack), both orientations, all tiers — suitable for
/// `cargo +nightly miri test hevc_deblock_chroma_size4_bounds`.
#[test]
fn hevc_deblock_chroma_size4_bounds() {
    // Vertical edge: xstride=1 (needs cols -2..+1 around base col), ystride=stride
    // over 4 lines. A 6-wide × 4-tall plane with base at col 2 row 0 fits exactly:
    // reads cols 0..=3, lines 0..=3.
    let max = 4095i32;
    // vertical
    {
        let stride = 4usize;
        let height = 4usize;
        let base = 2usize; // col 2, row 0
        for disp in [Dispatch::scalar(), Dispatch::detect()] {
            for pat in 0u8..4 {
                let src = deblock_plane(0x4004 + pat as u64, stride, height, max as u16, pat);
                let mut g = src.clone();
                let mut p = src.clone();
                golden_chroma_weak(&mut g, base, 1, stride as isize, 4, 30, max);
                run_chroma_weak(&disp, &mut p, base, 1, stride as isize, 4, 30, max);
                assert_eq!(g, p, "chroma size4 vertical pat={pat}");
            }
        }
    }
    // horizontal: xstride=stride, ystride=1; base row 2 col 0 in a 4-wide × 6-tall
    // plane → reads rows 0..=3 at cols 0..=3.
    {
        let stride = 4usize;
        let height = 6usize;
        let base = 2 * stride; // row 2, col 0
        for disp in [Dispatch::scalar(), Dispatch::detect()] {
            for pat in 0u8..4 {
                let src = deblock_plane(0x4114 + pat as u64, stride, height, max as u16, pat);
                let mut g = src.clone();
                let mut p = src.clone();
                golden_chroma_weak(&mut g, base, stride as isize, 1, 4, 30, max);
                run_chroma_weak(&disp, &mut p, base, stride as isize, 1, 4, 30, max);
                assert_eq!(g, p, "chroma size4 horizontal pat={pat}");
            }
        }
    }
}

/// G3 coverage nit: the SIMD chroma kernel processes 4-lane vector chunks
/// (`while d0+4<=size`) plus a scalar tail (`while d0<size`); the shipping
/// HEVC path always calls with `size=4` (exactly one chunk, empty tail), so
/// the multi-chunk and non-empty-tail branches are never exercised by the
/// other goldens above. Sweep `size` over {2 (tail-only, zero chunks), 6 (one
/// chunk + a 2-line tail), 8 (two chunks, empty tail)} against the
/// size-generalized `golden_chroma_weak`, both orientations, all bit depths,
/// the same input patterns used elsewhere in this block.
#[test]
fn hevc_deblock_chroma_multi_size() {
    for disp in [Dispatch::scalar(), Dispatch::detect()] {
        for (xs, ys, olbl) in deblock_orientations() {
            for &bd in &DEBLOCK_BDS {
                let max = ((1u32 << bd) - 1) as i32;
                let maxu = max as u16;
                for pat in 0u8..5 {
                    for &size in &[2usize, 6, 8] {
                        for &tc in &[1i32, 13, 400] {
                            let seed = 0xC5C5
                                + pat as u64 * 61
                                + (tc as u64) * 19
                                + size as u64 * 5
                                + xs as u64;
                            let base =
                                deblock_plane(seed, DEBLOCK_STRIDE, DEBLOCK_HEIGHT, maxu, pat);
                            let mut g = base.clone();
                            let mut p = base.clone();
                            golden_chroma_weak(&mut g, DEBLOCK_BASE, xs, ys, size, tc, max);
                            run_chroma_weak(&disp, &mut p, DEBLOCK_BASE, xs, ys, size, tc, max);
                            assert_eq!(
                                g, p,
                                "chroma multi-size size={size} {olbl} bd={bd} pat={pat} tc={tc}"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// G3 bounded-load focus (Miri-ready): TIGHT planes where the luma access
/// windows exactly fit — no slack on either side — for both `luma_strong`
/// (8-sample window `p3..q3`, `k=-4..3`, x 4 lines = 32 elements) and
/// `luma_weak` (6-sample window `p2..q2`, `k=-3..2`, x 4 lines = 24
/// elements), both orientations, suitable for
/// `cargo +nightly miri test hevc_deblock_luma_bounds`. A non-uniform
/// (random / deterministic-ramp) input pattern ensures a wrong-index read
/// surfaces as a value mismatch even without Miri. `nd_p=nd_q=2` + a large
/// `tc` on the weak case exercise the `p1`/`q1` writes at the window edges.
#[test]
fn hevc_deblock_luma_bounds() {
    let max = 4095i32;
    // --- strong: 8-sample window (p3..q3, k=-4..3) x 4 lines = 32 elements, tight.
    // vertical: xstride=1 (p/q direction is columns) -> stride=8 cols x height=4
    // rows, base at col 4 row 0: reads cols 0..=7, rows 0..=3.
    {
        let stride = 8usize;
        let height = 4usize;
        let base = 4usize; // col 4, row 0
        for disp in [Dispatch::scalar(), Dispatch::detect()] {
            for pat in [0u8, 4] {
                let src = deblock_plane(0x5005 + pat as u64, stride, height, max as u16, pat);
                let mut g = src.clone();
                let mut p = src.clone();
                golden_luma_strong(&mut g, base, 1, stride as isize, 24, 24, 24);
                run_luma_strong(&disp, &mut p, base, 1, stride as isize, 24, 24, 24, max);
                assert_eq!(g, p, "strong bounds vertical pat={pat}");
            }
        }
    }
    // horizontal: xstride=stride (p/q direction is rows) -> stride=4 cols x
    // height=8 rows, base at row 4 col 0: reads rows 0..=7, cols 0..=3.
    {
        let stride = 4usize;
        let height = 8usize;
        let base = 4 * stride; // row 4, col 0
        for disp in [Dispatch::scalar(), Dispatch::detect()] {
            for pat in [0u8, 4] {
                let src = deblock_plane(0x5115 + pat as u64, stride, height, max as u16, pat);
                let mut g = src.clone();
                let mut p = src.clone();
                golden_luma_strong(&mut g, base, stride as isize, 1, 24, 24, 24);
                run_luma_strong(&disp, &mut p, base, stride as isize, 1, 24, 24, 24, max);
                assert_eq!(g, p, "strong bounds horizontal pat={pat}");
            }
        }
    }
    // --- weak: 6-sample window (p2..q2, k=-3..2) x 4 lines = 24 elements, tight.
    // vertical: stride=6 cols x height=4 rows, base at col 3 row 0: reads
    // cols 0..=5, rows 0..=3.
    {
        let stride = 6usize;
        let height = 4usize;
        let base = 3usize; // col 3, row 0
        for disp in [Dispatch::scalar(), Dispatch::detect()] {
            for pat in [0u8, 4] {
                let src = deblock_plane(0x6006 + pat as u64, stride, height, max as u16, pat);
                let mut g = src.clone();
                let mut p = src.clone();
                golden_luma_weak(&mut g, base, 1, stride as isize, 400, 2, 2, max);
                run_luma_weak(&disp, &mut p, base, 1, stride as isize, 400, 2, 2, max);
                assert_eq!(g, p, "weak bounds vertical pat={pat}");
            }
        }
    }
    // horizontal: stride=4 cols x height=6 rows, base at row 3 col 0: reads
    // rows 0..=5, cols 0..=3.
    {
        let stride = 4usize;
        let height = 6usize;
        let base = 3 * stride; // row 3, col 0
        for disp in [Dispatch::scalar(), Dispatch::detect()] {
            for pat in [0u8, 4] {
                let src = deblock_plane(0x6116 + pat as u64, stride, height, max as u16, pat);
                let mut g = src.clone();
                let mut p = src.clone();
                golden_luma_weak(&mut g, base, stride as isize, 1, 400, 2, 2, max);
                run_luma_weak(&disp, &mut p, base, stride as isize, 1, 400, 2, 2, max);
                assert_eq!(g, p, "weak bounds horizontal pat={pat}");
            }
        }
    }
}

/// Smoke test through the SAFE crate-root wrappers (exercises the geometry
/// asserts + dispatch path end-to-end, not just the raw Dispatch pointers).
#[test]
fn hevc_deblock_safe_wrappers_smoke() {
    let max = 1023i32;
    let mut p = deblock_plane(0x9, DEBLOCK_STRIDE, DEBLOCK_HEIGHT, max as u16, 0);
    codec_simd::hevc_deblock_luma_strong_u16(
        &mut p,
        DEBLOCK_BASE,
        1,
        DEBLOCK_STRIDE as isize,
        20,
        20,
        20,
        max,
    );
    codec_simd::hevc_deblock_luma_weak_u16(
        &mut p,
        DEBLOCK_BASE,
        1,
        DEBLOCK_STRIDE as isize,
        10,
        2,
        2,
        max,
    );
    codec_simd::hevc_deblock_chroma_weak_u16(
        &mut p,
        DEBLOCK_BASE,
        DEBLOCK_STRIDE as isize,
        1,
        4,
        8,
        max,
    );
    assert!(p.iter().all(|&v| v as i32 <= max));
}

// =========================================================================
// OpenEXR ZIP/RLE predictor decode-side primitives (v0.3.35):
// prefix_sum_bias_u8_inplace, interleave_halves_u8.
// =========================================================================

fn make_input_u8(n: usize, seed: u32) -> Vec<u8> {
    let mut state = seed;
    (0..n)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (state >> 16) as u8
        })
        .collect()
}

fn scalar_ref_prefix_sum_bias_u8(src: &[u8]) -> Vec<u8> {
    let mut out = src.to_vec();
    if out.is_empty() {
        return out;
    }
    let mut prev = out[0];
    for v in out.iter_mut().skip(1) {
        let cur = prev.wrapping_add(*v).wrapping_sub(128);
        *v = cur;
        prev = cur;
    }
    out
}

fn run_prefix_sum(disp: &Dispatch, src: &[u8]) -> Vec<u8> {
    let mut buf = src.to_vec();
    // SAFETY: Dispatch only installs fns whose target_feature reqs hold here.
    unsafe {
        (disp.prefix_sum_bias_u8_inplace)(buf.as_mut_ptr(), buf.len());
    }
    buf
}

const PREFIX_SUM_TEST_LENS: [usize; 15] =
    [0, 1, 2, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 200, 1000];

#[test]
fn prefix_sum_bias_u8_scalar_matches_naive_reference() {
    let disp = Dispatch::scalar();
    for &n in &PREFIX_SUM_TEST_LENS {
        let src = make_input_u8(n, 0xC0FF_EE11);
        assert_eq!(
            run_prefix_sum(&disp, &src),
            scalar_ref_prefix_sum_bias_u8(&src),
            "n={n}"
        );
    }
}

#[test]
fn prefix_sum_bias_u8_detected_dispatch_matches_scalar() {
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &PREFIX_SUM_TEST_LENS {
        let src = make_input_u8(n, 0xBEEF_0001);
        assert_eq!(
            run_prefix_sum(&detected, &src),
            run_prefix_sum(&scalar, &src),
            "tier {} diverged at n={n}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

fn scalar_ref_interleave_halves_u8(t1: &[u8], t2: &[u8], n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    let mut i1 = 0usize;
    let mut i2 = 0usize;
    let mut k = 0usize;
    while k < n {
        out[k] = t1[i1];
        i1 += 1;
        k += 1;
        if k < n {
            out[k] = t2[i2];
            i2 += 1;
            k += 1;
        }
    }
    out
}

fn run_interleave(disp: &Dispatch, t1: &[u8], t2: &[u8], n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    // SAFETY: see run_prefix_sum.
    unsafe {
        (disp.interleave_halves_u8)(t1.as_ptr(), t2.as_ptr(), out.as_mut_ptr(), n);
    }
    out
}

#[test]
fn interleave_halves_u8_scalar_matches_naive_reference() {
    let disp = Dispatch::scalar();
    for &n in &PREFIX_SUM_TEST_LENS {
        let t1 = make_input_u8(n.div_ceil(2), 0x1357_2468);
        let t2 = make_input_u8(n / 2, 0x2468_1357);
        assert_eq!(
            run_interleave(&disp, &t1, &t2, n),
            scalar_ref_interleave_halves_u8(&t1, &t2, n),
            "n={n}"
        );
    }
}

#[test]
fn interleave_halves_u8_detected_dispatch_matches_scalar() {
    let scalar = Dispatch::scalar();
    let detected = Dispatch::detect();
    for &n in &PREFIX_SUM_TEST_LENS {
        let t1 = make_input_u8(n.div_ceil(2), 0xAAAA_5555);
        let t2 = make_input_u8(n / 2, 0x5555_AAAA);
        assert_eq!(
            run_interleave(&detected, &t1, &t2, n),
            run_interleave(&scalar, &t1, &t2, n),
            "tier {} diverged at n={n}",
            codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()),
        );
    }
}

/// Round trip through the SAFE crate-root wrappers together, matching the
/// exact call shape a consumer (e.g. exr-rs's ZIP codec) uses: predict+
/// reorder forward (plain scalar reference here, since the encode side has
/// no SIMD counterpart), then `interleave_halves_u8` +
/// `prefix_sum_bias_u8_inplace` to invert it, on both the scalar and the
/// detected dispatch.
#[test]
fn zip_predictor_safe_wrappers_round_trip() {
    for &n in &PREFIX_SUM_TEST_LENS {
        let original = make_input_u8(n, 0xFEED_FACE);

        // Forward (deinterleave + first-difference), plain scalar -- mirrors
        // OpenEXR's own C reference exactly (no SIMD variant exists there
        // either).
        let mut scratch = vec![0u8; n];
        {
            let t2_start = n.div_ceil(2);
            let (t1, t2) = scratch.split_at_mut(t2_start);
            let mut i1 = 0usize;
            let mut i2 = 0usize;
            let mut k = 0usize;
            while k < n {
                t1[i1] = original[k];
                i1 += 1;
                k += 1;
                if k < n {
                    t2[i2] = original[k];
                    i2 += 1;
                    k += 1;
                }
            }
            if n > 0 {
                let mut p = scratch[0] as i32;
                for cell in scratch.iter_mut().skip(1) {
                    let cur = *cell as i32;
                    let d = cur - p + (128 + 256);
                    p = cur;
                    *cell = d as u8;
                }
            }
        }

        // Backward through the SAFE public API -- `Dispatch::scalar()` isn't
        // reachable through it directly (by design, it always uses the
        // detected tier), so this exercises whatever tier THIS machine
        // detects; the raw-Dispatch tests above already cover the
        // scalar-vs-detected differential precisely.
        let mut buf = scratch.clone();
        codec_simd::prefix_sum_bias_u8_inplace(&mut buf);
        let t2_start = n.div_ceil(2);
        let (t1, t2) = buf.split_at(t2_start);
        let mut restored = vec![0u8; n];
        codec_simd::interleave_halves_u8(t1, t2, &mut restored);
        assert_eq!(restored, original, "n={n}");
    }
}
