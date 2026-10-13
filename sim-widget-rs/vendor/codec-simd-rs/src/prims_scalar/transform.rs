//! Scalar reference implementations of the HEVC inverse-transform family
//! (size-4 IDCT / DST-VII + the size-generic DC fast path).
//!
//! Byte-exact port of ffmpeg-rs `av-codec-h265/transform/idct.rs`
//! (`idct_nxn` size-4 path / `transform_4x4_luma` / `idct_dc`), itself a port
//! of `_ref/FFmpeg/libavcodec/hevc/dsp_template.c`.  The transforms operate
//! **in-place on an `i16` coefficient block** (the add-to-plane is a separate
//! `add_residual` step).  Every backend (SSE2/AVX2) is fuzz-compared against
//! this scalar reference AND against an independent naive golden in
//! `tests/cross_tier.rs`.
//!
//! # Two-pass SCALE model
//! Both size-4 transforms run two passes (columns then rows) of a 4-point
//! butterfly, each element `SCALEd` as `clip_int16((r + add) >> shift)`:
//! pass-1 (columns) `shift = 7`, pass-2 (rows) `shift = 20 - bit_depth`.
//! `clip_int16` clamps to the `i16` range `[-32768, 32767]` (spec
//! `coeffMin`/`coeffMax`).  The DC path uses `shift = 14 - bit_depth`.

/// `av_clip_int16` — clamp an `i32` to the signed 16-bit range (the `SCALE`
/// macro's `coeffMin`/`coeffMax`).  Identical to signed-saturating narrowing.
#[inline]
fn clip_int16(x: i32) -> i16 {
    x.clamp(-32768, 32767) as i16
}

/// The DCT-II 4-point core (`tr_4_core`, `dsp_template.c` `TR_4`), collapsed to
/// the direct basis form `out = M · s` (algebraically identical to the
/// even/odd butterfly `e0/e1/o0/o1`, since the arithmetic is exact `i32` with
/// no intermediate rounding).  Returns the four un-scaled `i32` results.
#[inline]
fn tr4(s0: i32, s1: i32, s2: i32, s3: i32) -> [i32; 4] {
    // Even/odd butterfly, verbatim from tr_4_core (constants 64/83/36).
    let e0 = 64 * s0 + 64 * s2;
    let e1 = 64 * s0 - 64 * s2;
    let o0 = 83 * s1 + 36 * s3;
    let o1 = 36 * s1 - 83 * s3;
    [e0 + o0, e1 + o1, e1 - o1, e0 - o0]
}

/// The DST-VII 4-point core (`tr_4x4_luma_core`, constants 29/55/74).
#[inline]
fn dst4(s0: i32, s1: i32, s2: i32, s3: i32) -> [i32; 4] {
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

/// Scalar reference for [`crate::hevc_idct4_i16_inplace`].
///
/// The `idct_nxn` size-4 path: pass 1 over the 4 columns (`tr_4` then
/// `SCALE >> 7`), pass 2 over the 4 rows (`tr_4` then `SCALE >> (20-bd)`).
/// `TR_4` ignores the `col_limit`/`end` pruning (it has no odd loop), so no
/// pruning argument is threaded.  In-place on `coeffs[0..16]` (row-major).
///
/// # Safety
/// `coeffs` must be valid for 16 `i16` reads+writes; `bit_depth ∈ {8,9,10,12}`.
pub(crate) unsafe fn hevc_idct4_i16_inplace(coeffs: *mut i16, bit_depth: u8) {
    unsafe {
        // Pass 1: columns (step = 4), shift = 7.
        let shift = 7i32;
        let add = 1i32 << (shift - 1);
        for i in 0..4 {
            let s0 = *coeffs.add(i) as i32;
            let s1 = *coeffs.add(i + 4) as i32;
            let s2 = *coeffs.add(i + 8) as i32;
            let s3 = *coeffs.add(i + 12) as i32;
            let r = tr4(s0, s1, s2, s3);
            for k in 0..4 {
                *coeffs.add(i + k * 4) = clip_int16((r[k] + add) >> shift);
            }
        }
        // Pass 2: rows (step = 1, base += 4), shift = 20 - bit_depth.
        let shift = 20 - bit_depth as i32;
        let add = 1i32 << (shift - 1);
        for i in 0..4 {
            let base = i * 4;
            let s0 = *coeffs.add(base) as i32;
            let s1 = *coeffs.add(base + 1) as i32;
            let s2 = *coeffs.add(base + 2) as i32;
            let s3 = *coeffs.add(base + 3) as i32;
            let r = tr4(s0, s1, s2, s3);
            for k in 0..4 {
                *coeffs.add(base + k) = clip_int16((r[k] + add) >> shift);
            }
        }
    }
}

/// Scalar reference for [`crate::hevc_dst4_i16_inplace`].
///
/// `transform_4x4_luma` (DST-VII): same two-pass SCALE model as
/// [`hevc_idct4_i16_inplace`] but the [`dst4`] core.  In-place on
/// `coeffs[0..16]`.
///
/// # Safety
/// `coeffs` must be valid for 16 `i16` reads+writes; `bit_depth ∈ {8,9,10,12}`.
pub(crate) unsafe fn hevc_dst4_i16_inplace(coeffs: *mut i16, bit_depth: u8) {
    unsafe {
        let shift = 7i32;
        let add = 1i32 << (shift - 1);
        for i in 0..4 {
            let s0 = *coeffs.add(i) as i32;
            let s1 = *coeffs.add(i + 4) as i32;
            let s2 = *coeffs.add(i + 8) as i32;
            let s3 = *coeffs.add(i + 12) as i32;
            let r = dst4(s0, s1, s2, s3);
            for k in 0..4 {
                *coeffs.add(i + k * 4) = clip_int16((r[k] + add) >> shift);
            }
        }
        let shift = 20 - bit_depth as i32;
        let add = 1i32 << (shift - 1);
        for i in 0..4 {
            let base = i * 4;
            let s0 = *coeffs.add(base) as i32;
            let s1 = *coeffs.add(base + 1) as i32;
            let s2 = *coeffs.add(base + 2) as i32;
            let s3 = *coeffs.add(base + 3) as i32;
            let r = dst4(s0, s1, s2, s3);
            for k in 0..4 {
                *coeffs.add(base + k) = clip_int16((r[k] + add) >> shift);
            }
        }
    }
}

/// Scalar reference for [`crate::hevc_idct_dc_i16_inplace`].
///
/// The DC-only fast path `idct_NxN_dc`: every output sample equals
/// `clip_int16((((coeffs[0] + 1) >> 1) + add) >> shift)`, `shift = 14 - bd`,
/// filling the whole `size×size` block (`size = 1 << log2_size`, `∈ {4,8,16,32}`).
///
/// # Safety
/// `coeffs` valid for `size*size` `i16` writes; `bit_depth ∈ {8,9,10,12}`.
pub(crate) unsafe fn hevc_idct_dc_i16_inplace(coeffs: *mut i16, log2_size: u32, bit_depth: u8) {
    unsafe {
        let size = 1usize << log2_size;
        let shift = 14 - bit_depth as i32;
        let add = 1i32 << (shift - 1);
        let coeff = clip_int16(((((*coeffs) as i32 + 1) >> 1) + add) >> shift);
        for k in 0..size * size {
            *coeffs.add(k) = coeff;
        }
    }
}

// =========================================================================
// HEVC inverse DCT-II — sizes 8 / 16 / 32 (`idct_nxn` 8/16/32 paths)
//
// Byte-exact port of ffmpeg-rs `av-codec-h265/transform/idct.rs`
// (`tr_8_core` / `tr_16_core` / `tr_32_core` + `idct_nxn` col/row passes),
// itself a port of `dsp_template.c` TR_8/TR_16/TR_32 + IDCT(H).  Each
// `tr_N_core` = the even part `tr_{N/2}` on the even-index inputs + the odd
// part `o[i] = Σ_{j odd, j<end} TRANSFORM[N-row][i]·src[j]`, combined as
// `e[i]±o[i]`.  Two passes (columns `>>7`, rows `>>(20-bd)`) with per-element
// `clip_int16` SCALE.  `end` is the odd-coefficient pruning: the column pass
// uses `limit2 = min(col_limit+4, H)` (shrinking by 4 every 4th column), the
// row pass uses `limit = min(col_limit, H)`.  This scalar reference replicates
// the pruning EXACTLY (option (a) — a faithful standalone primitive for any
// input); the SSE2/AVX2 backends achieve the same via source pre-zeroing (see
// `hevc_prune_col` / `hevc_prune_row`).
// =========================================================================

/// The 32×32 HEVC DCT-II basis matrix `transform[i][j]` — ported VERBATIM from
/// `dsp.c:27-92`.  Shared by this scalar reference AND both SIMD backends
/// (SSE2/AVX2 index it for the odd-part constant multiplies), so it lives here
/// once rather than being duplicated per backend.  Indexed as the C does:
/// `TRANSFORM[4*j][i]` (8-pt odd), `TRANSFORM[2*j][i]` (16-pt odd),
/// `TRANSFORM[j][i]` (32-pt odd).
#[rustfmt::skip]
pub(crate) const TRANSFORM: [[i8; 32]; 32] = [
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

/// `tr_8_core` (`dsp_template.c` TR_8): even part `tr_4` on the even inputs +
/// odd part `o[i] = Σ_{j odd, j<end} TRANSFORM[4j][i]·src[j]`.  Returns the 8
/// full-precision `i32` results `e[i]±o[i]`.  `end` is the C `int` bound; a
/// non-positive `end` means the odd loop runs 0 times (native `(j) < end`).
#[inline]
fn tr8_core(src: &[i32; 8], end: i32) -> [i32; 8] {
    let mut o = [0i32; 4];
    for i in 0..4 {
        let mut j = 1i32;
        while j < end {
            o[i] += TRANSFORM[4 * j as usize][i] as i32 * src[j as usize];
            j += 2;
        }
    }
    let e = tr4(src[0], src[2], src[4], src[6]);
    let mut out = [0i32; 8];
    for i in 0..4 {
        out[i] = e[i] + o[i];
        out[7 - i] = e[i] - o[i];
    }
    out
}

/// `tr_16_core` (`dsp_template.c` TR_16).  The even sub-transform hard-codes
/// `end = 8` (the even part is never pruned).
#[inline]
fn tr16_core(src: &[i32; 16], end: i32) -> [i32; 16] {
    let mut o = [0i32; 8];
    for i in 0..8 {
        let mut j = 1i32;
        while j < end {
            o[i] += TRANSFORM[2 * j as usize][i] as i32 * src[j as usize];
            j += 2;
        }
    }
    let mut even = [0i32; 8];
    for k in 0..8 {
        even[k] = src[2 * k];
    }
    let e = tr8_core(&even, 8);
    let mut out = [0i32; 16];
    for i in 0..8 {
        out[i] = e[i] + o[i];
        out[15 - i] = e[i] - o[i];
    }
    out
}

/// `tr_32_core` (`dsp_template.c` TR_32).  The ONLY level whose even
/// sub-transform end is `end/2` (i32 divide; a negative end stays non-positive).
#[inline]
fn tr32_core(src: &[i32; 32], end: i32) -> [i32; 32] {
    let mut o = [0i32; 16];
    for i in 0..16 {
        let mut j = 1i32;
        while j < end {
            o[i] += TRANSFORM[j as usize][i] as i32 * src[j as usize];
            j += 2;
        }
    }
    let mut even = [0i32; 16];
    for k in 0..16 {
        even[k] = src[2 * k];
    }
    let e = tr16_core(&even, end / 2);
    let mut out = [0i32; 32];
    for i in 0..16 {
        out[i] = e[i] + o[i];
        out[31 - i] = e[i] - o[i];
    }
    out
}

/// One column-pass butterfly for `idct_nxn`: gather `coeffs[base + k*H]`, run
/// `TR_H`, write SCALEd back.  (`H ∈ {8,16,32}`.)
#[inline]
unsafe fn idct_col(coeffs: *mut i16, h: usize, base: usize, end: i32, shift: i32, add: i32) {
    unsafe {
        match h {
            8 => {
                let mut s = [0i32; 8];
                for k in 0..8 {
                    s[k] = *coeffs.add(base + k * 8) as i32;
                }
                let r = tr8_core(&s, end);
                for k in 0..8 {
                    *coeffs.add(base + k * 8) = clip_int16((r[k] + add) >> shift);
                }
            }
            16 => {
                let mut s = [0i32; 16];
                for k in 0..16 {
                    s[k] = *coeffs.add(base + k * 16) as i32;
                }
                let r = tr16_core(&s, end);
                for k in 0..16 {
                    *coeffs.add(base + k * 16) = clip_int16((r[k] + add) >> shift);
                }
            }
            _ => {
                let mut s = [0i32; 32];
                for k in 0..32 {
                    s[k] = *coeffs.add(base + k * 32) as i32;
                }
                let r = tr32_core(&s, end);
                for k in 0..32 {
                    *coeffs.add(base + k * 32) = clip_int16((r[k] + add) >> shift);
                }
            }
        }
    }
}

/// One row-pass butterfly for `idct_nxn`: gather `coeffs[base + k]`, run
/// `TR_H`, write SCALEd back.
#[inline]
unsafe fn idct_row(coeffs: *mut i16, h: usize, base: usize, end: i32, shift: i32, add: i32) {
    unsafe {
        match h {
            8 => {
                let mut s = [0i32; 8];
                for k in 0..8 {
                    s[k] = *coeffs.add(base + k) as i32;
                }
                let r = tr8_core(&s, end);
                for k in 0..8 {
                    *coeffs.add(base + k) = clip_int16((r[k] + add) >> shift);
                }
            }
            16 => {
                let mut s = [0i32; 16];
                for k in 0..16 {
                    s[k] = *coeffs.add(base + k) as i32;
                }
                let r = tr16_core(&s, end);
                for k in 0..16 {
                    *coeffs.add(base + k) = clip_int16((r[k] + add) >> shift);
                }
            }
            _ => {
                let mut s = [0i32; 32];
                for k in 0..32 {
                    s[k] = *coeffs.add(base + k) as i32;
                }
                let r = tr32_core(&s, end);
                for k in 0..32 {
                    *coeffs.add(base + k) = clip_int16((r[k] + add) >> shift);
                }
            }
        }
    }
}

/// The generic `idct_nxn` (`dsp_template.c` IDCT(H)) for `H ∈ {8,16,32}`.
/// Pass 1 = columns (`>>7`, `end = limit2`, shrinking every 4th column),
/// pass 2 = rows (`>>(20-bd)`, `end = limit`).  In-place on `coeffs[0..H*H]`.
#[inline]
unsafe fn idct_nxn(coeffs: *mut i16, h: usize, col_limit: i32, bit_depth: u8) {
    unsafe {
        let limit = col_limit.min(h as i32);
        let mut limit2 = (col_limit + 4).min(h as i32);

        let shift = 7i32;
        let add = 1i32 << (shift - 1);
        for i in 0..h {
            idct_col(coeffs, h, i, limit2, shift, add);
            if limit2 < h as i32 && i % 4 == 0 && i != 0 {
                limit2 -= 4;
            }
        }

        let shift = 20 - bit_depth as i32;
        let add = 1i32 << (shift - 1);
        for i in 0..h {
            idct_row(coeffs, h, i * h, limit, shift, add);
        }
    }
}

/// Whether source coefficient index `m` survives the odd-coefficient pruning of
/// `TR_H` at bound `end` (used by the SIMD backends' source pre-zeroing).
///
/// The DCT butterfly routes each source index to exactly one leaf of the
/// even/odd recursion, so a source is pruned iff its routed odd-index exceeds
/// that level's `end`:
/// - odd `m` → top-level odd sum, kept iff `m < end`.
/// - `m ≡ 2 (mod 4)` at `H == 32` → the `TR_16(.., end/2)` odd sum, kept iff
///   `m/2 < end/2` (the ONLY nested prune; `TR_16`'s and `TR_8`'s even parts
///   are always full).
/// - every other even `m` → an always-full sub-transform, always kept.
///
/// Zeroing the pruned sources then running the FULL butterfly is byte-identical
/// to the reference's pruned loops (linearity + the one-leaf routing), which is
/// how the vectorized backends stay byte-exact for ANY input (option (a)).
///
/// Only the x86 SIMD backends consume this (the scalar `idct_nxn` prunes inline
/// via `tr_N_core`'s `end`), so it is gated to avoid a dead-code warning on the
/// NEON/wasm scalar-fallthrough targets.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub(crate) fn hevc_kept(h: usize, end: i32, m: usize) -> bool {
    if m % 2 == 1 {
        return (m as i32) < end;
    }
    if h == 32 && m % 4 == 2 {
        return (m as i32 / 2) < (end / 2);
    }
    true
}

/// Column-pass source pre-zero: zero every `coeffs[m*H + c]` pruned for column
/// `c` under the `limit2 = min(col_limit+4, H)` sequence (shrinking by 4 every
/// 4th column) — the exact `idct_nxn` pass-1 pruning, applied to the buffer so
/// the SIMD butterfly can run the FULL odd range uniformly across columns.
///
/// # Safety
/// `coeffs` valid for `H*H` `i16` reads+writes; `H ∈ {8,16,32}`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub(crate) unsafe fn hevc_prune_col(coeffs: *mut i16, h: usize, col_limit: i32) {
    unsafe {
        let mut limit2 = (col_limit + 4).min(h as i32);
        for c in 0..h {
            for m in 0..h {
                if !hevc_kept(h, limit2, m) {
                    *coeffs.add(m * h + c) = 0;
                }
            }
            if limit2 < h as i32 && c % 4 == 0 && c != 0 {
                limit2 -= 4;
            }
        }
    }
}

/// Row-pass source pre-zero: zero every source row `m` pruned under the uniform
/// `limit = min(col_limit, H)` — the exact `idct_nxn` pass-2 pruning.  The row
/// pass sees the pass-1 result transposed, so source index `m` is the whole
/// buffer row `m` (all columns) here.
///
/// # Safety
/// `coeffs` valid for `H*H` `i16` reads+writes; `H ∈ {8,16,32}`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub(crate) unsafe fn hevc_prune_row(coeffs: *mut i16, h: usize, col_limit: i32) {
    unsafe {
        let limit = col_limit.min(h as i32);
        for m in 0..h {
            if !hevc_kept(h, limit, m) {
                for b in 0..h {
                    *coeffs.add(m * h + b) = 0;
                }
            }
        }
    }
}

/// Scalar reference for [`crate::hevc_idct8_i16_inplace`] — `idct_nxn` H=8.
///
/// # Safety
/// `coeffs` valid for 64 `i16` reads+writes; `bit_depth ∈ {8,9,10,12}`.
pub(crate) unsafe fn hevc_idct8_i16_inplace(coeffs: *mut i16, col_limit: i32, bit_depth: u8) {
    unsafe { idct_nxn(coeffs, 8, col_limit, bit_depth) }
}

/// Scalar reference for [`crate::hevc_idct16_i16_inplace`] — `idct_nxn` H=16.
///
/// # Safety
/// `coeffs` valid for 256 `i16` reads+writes; `bit_depth ∈ {8,9,10,12}`.
pub(crate) unsafe fn hevc_idct16_i16_inplace(coeffs: *mut i16, col_limit: i32, bit_depth: u8) {
    unsafe { idct_nxn(coeffs, 16, col_limit, bit_depth) }
}

/// Scalar reference for [`crate::hevc_idct32_i16_inplace`] — `idct_nxn` H=32.
///
/// # Safety
/// `coeffs` valid for 1024 `i16` reads+writes; `bit_depth ∈ {8,9,10,12}`.
pub(crate) unsafe fn hevc_idct32_i16_inplace(coeffs: *mut i16, col_limit: i32, bit_depth: u8) {
    unsafe { idct_nxn(coeffs, 32, col_limit, bit_depth) }
}
