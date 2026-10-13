//! SSE2 implementations of the HEVC inverse-transform family (size-4 IDCT /
//! DST-VII + the size-generic DC fast path).
//!
//! # SIMD strategy — vectorize ACROSS the 4 columns
//! All 4 columns of a size-4 transform run the IDENTICAL 4-point butterfly, so
//! one SSE register (4 × `i32` lanes) processes all 4 columns at once.  The
//! per-lane products use the `mullo`/`mulhi` widen (`i16 × i16 → i32`, exactly
//! the reference's `int` width) because SSE2 has no `mullo_epi32`; the
//! multiplicands are therefore the ORIGINAL `i16` coefficients (each ⊆ `i16`),
//! which is why the DST core is expressed in the collapsed direct-basis matrix
//! form (`out = M · s`) rather than the `c0 = s0+s2 …` intermediates (those
//! sums can exceed `i16`).  The matrices are algebraically identical to the
//! reference cores (exact `i32`, no intermediate rounding) and the differential
//! golden in `tests/cross_tier.rs` guards every entry.
//!
//! The row (2nd) pass is the column pass applied to the TRANSPOSE of the
//! 1st-pass result (separable-transform identity; the per-element `SCALE`
//! clip commutes with transposition), so the whole kernel is
//! `pass → transpose → pass → transpose → store`, reusing one butterfly.

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// Multiply-widen: `i16` lane vector (4 columns in the low 64 bits) × constant
/// `c` → four `i32` products.  `mullo`(low 16) ∪ `mulhi`(signed high 16) is the
/// exact signed 32-bit product, matching the reference `int` multiply.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn mw(s: __m128i, c: i16) -> __m128i {
    let cv = _mm_set1_epi16(c);
    let lo = _mm_mullo_epi16(s, cv);
    let hi = _mm_mulhi_epi16(s, cv);
    _mm_unpacklo_epi16(lo, hi)
}

/// `SCALE`: `(v + add) >> shift` (arithmetic) then signed-saturate narrow to
/// `i16` (= `clip_int16`).  Returns the 4 clipped `i16` in the low 64 bits.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn scale_pack(v: __m128i, add: __m128i, cnt: __m128i) -> __m128i {
    let t = _mm_sra_epi32(_mm_add_epi32(v, add), cnt);
    _mm_packs_epi32(t, t)
}

/// 4×4 `i16` transpose (each input holds one row's 4 values in the low 64
/// bits; each output holds one column in the low 64 bits).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn transpose4(
    a: __m128i,
    b: __m128i,
    c: __m128i,
    d: __m128i,
) -> (__m128i, __m128i, __m128i, __m128i) {
    let t0 = _mm_unpacklo_epi16(a, b);
    let t1 = _mm_unpacklo_epi16(c, d);
    let m0 = _mm_unpacklo_epi32(t0, t1); // low64 = col0
    let m2 = _mm_unpackhi_epi32(t0, t1); // low64 = col2
    let m1 = _mm_unpackhi_epi64(m0, m0); // low64 = col1
    let m3 = _mm_unpackhi_epi64(m2, m2); // low64 = col3
    (m0, m1, m2, m3)
}

/// One DCT-4 pass across 4 lanes (`tr_4` even/odd butterfly + `SCALE`).
/// `s0..s3` are the 4 transform elements (each a 4-column `i16` lane vector).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn idct4_pass(
    s0: __m128i,
    s1: __m128i,
    s2: __m128i,
    s3: __m128i,
    add: __m128i,
    cnt: __m128i,
) -> (__m128i, __m128i, __m128i, __m128i) {
    unsafe {
        let m0 = mw(s0, 64);
        let m2 = mw(s2, 64);
        let e0 = _mm_add_epi32(m0, m2);
        let e1 = _mm_sub_epi32(m0, m2);
        let o0 = _mm_add_epi32(mw(s1, 83), mw(s3, 36));
        let o1 = _mm_sub_epi32(mw(s1, 36), mw(s3, 83));
        (
            scale_pack(_mm_add_epi32(e0, o0), add, cnt),
            scale_pack(_mm_add_epi32(e1, o1), add, cnt),
            scale_pack(_mm_sub_epi32(e1, o1), add, cnt),
            scale_pack(_mm_sub_epi32(e0, o0), add, cnt),
        )
    }
}

/// One DST-VII pass across 4 lanes (`tr_4x4_luma` collapsed matrix + `SCALE`).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn dst4_pass(
    s0: __m128i,
    s1: __m128i,
    s2: __m128i,
    s3: __m128i,
    add: __m128i,
    cnt: __m128i,
) -> (__m128i, __m128i, __m128i, __m128i) {
    unsafe {
        // d0=[29,74,84,55] d1=[55,74,-29,-84] d2=[74,0,-74,74] d3=[84,-74,55,-29]
        let d0 = _mm_add_epi32(
            _mm_add_epi32(mw(s0, 29), mw(s1, 74)),
            _mm_add_epi32(mw(s2, 84), mw(s3, 55)),
        );
        let d1 = _mm_add_epi32(
            _mm_add_epi32(mw(s0, 55), mw(s1, 74)),
            _mm_add_epi32(mw(s2, -29), mw(s3, -84)),
        );
        let d2 = _mm_add_epi32(mw(s0, 74), _mm_add_epi32(mw(s2, -74), mw(s3, 74)));
        let d3 = _mm_add_epi32(
            _mm_add_epi32(mw(s0, 84), mw(s1, -74)),
            _mm_add_epi32(mw(s2, 55), mw(s3, -29)),
        );
        (
            scale_pack(d0, add, cnt),
            scale_pack(d1, add, cnt),
            scale_pack(d2, add, cnt),
            scale_pack(d3, add, cnt),
        )
    }
}

/// SSE2 impl of [`crate::hevc_idct4_i16_inplace`].  Byte-exact vs scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn hevc_idct4_i16_inplace(coeffs: *mut i16, bit_depth: u8) {
    unsafe {
        let r0 = _mm_loadl_epi64(coeffs as *const __m128i);
        let r1 = _mm_loadl_epi64(coeffs.add(4) as *const __m128i);
        let r2 = _mm_loadl_epi64(coeffs.add(8) as *const __m128i);
        let r3 = _mm_loadl_epi64(coeffs.add(12) as *const __m128i);

        let cnt1 = _mm_cvtsi32_si128(7);
        let add1 = _mm_set1_epi32(1 << 6);
        let (o0, o1, o2, o3) = idct4_pass(r0, r1, r2, r3, add1, cnt1);

        let (c0, c1, c2, c3) = transpose4(o0, o1, o2, o3);

        let shift2 = 20 - bit_depth as i32;
        let cnt2 = _mm_cvtsi32_si128(shift2);
        let add2 = _mm_set1_epi32(1 << (shift2 - 1));
        let (p0, p1, p2, p3) = idct4_pass(c0, c1, c2, c3, add2, cnt2);

        let (f0, f1, f2, f3) = transpose4(p0, p1, p2, p3);
        _mm_storel_epi64(coeffs as *mut __m128i, f0);
        _mm_storel_epi64(coeffs.add(4) as *mut __m128i, f1);
        _mm_storel_epi64(coeffs.add(8) as *mut __m128i, f2);
        _mm_storel_epi64(coeffs.add(12) as *mut __m128i, f3);
    }
}

/// SSE2 impl of [`crate::hevc_dst4_i16_inplace`].  Byte-exact vs scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn hevc_dst4_i16_inplace(coeffs: *mut i16, bit_depth: u8) {
    unsafe {
        let r0 = _mm_loadl_epi64(coeffs as *const __m128i);
        let r1 = _mm_loadl_epi64(coeffs.add(4) as *const __m128i);
        let r2 = _mm_loadl_epi64(coeffs.add(8) as *const __m128i);
        let r3 = _mm_loadl_epi64(coeffs.add(12) as *const __m128i);

        let cnt1 = _mm_cvtsi32_si128(7);
        let add1 = _mm_set1_epi32(1 << 6);
        let (o0, o1, o2, o3) = dst4_pass(r0, r1, r2, r3, add1, cnt1);

        let (c0, c1, c2, c3) = transpose4(o0, o1, o2, o3);

        let shift2 = 20 - bit_depth as i32;
        let cnt2 = _mm_cvtsi32_si128(shift2);
        let add2 = _mm_set1_epi32(1 << (shift2 - 1));
        let (p0, p1, p2, p3) = dst4_pass(c0, c1, c2, c3, add2, cnt2);

        let (f0, f1, f2, f3) = transpose4(p0, p1, p2, p3);
        _mm_storel_epi64(coeffs as *mut __m128i, f0);
        _mm_storel_epi64(coeffs.add(4) as *mut __m128i, f1);
        _mm_storel_epi64(coeffs.add(8) as *mut __m128i, f2);
        _mm_storel_epi64(coeffs.add(12) as *mut __m128i, f3);
    }
}

/// SSE2 impl of [`crate::hevc_idct_dc_i16_inplace`].  Computes the single DC
/// output value (scalar, byte-exact) then splat-stores it across the block.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn hevc_idct_dc_i16_inplace(coeffs: *mut i16, log2_size: u32, bit_depth: u8) {
    unsafe {
        let size = 1usize << log2_size;
        let n = size * size;
        let shift = 14 - bit_depth as i32;
        let add = 1i32 << (shift - 1);
        let coeff = (((((*coeffs) as i32 + 1) >> 1) + add) >> shift).clamp(-32768, 32767) as i16;
        let sv = _mm_set1_epi16(coeff);
        let mut k = 0usize;
        while k + 8 <= n {
            _mm_storeu_si128(coeffs.add(k) as *mut __m128i, sv);
            k += 8;
        }
        while k < n {
            *coeffs.add(k) = coeff;
            k += 1;
        }
    }
}

// =========================================================================
// HEVC inverse DCT-II — sizes 8 / 16 / 32 (SSE2)
//
// # SIMD strategy — vectorize the partial butterfly ACROSS 4 columns
// Each pass is a 1-D `TR_H` applied down every column of the H×H block.  Four
// ADJACENT columns are 4 CONTIGUOUS `i16` per source row, so `loadl_epi64`
// gathers them and the whole even/odd butterfly runs on `i32` lanes (4 columns
// at once) via the size-4 `mw` widen (`i16×const → i32`; every multiplicand is
// a SOURCE coefficient ⊆ i16 — the reference never multiplies an intermediate,
// so `mulhi/mullo` suffices).  The row (2nd) pass is the SAME column butterfly
// applied to the TRANSPOSE of the pass-1 result (separable-transform identity;
// the SCALE clip commutes with transposition).
//
// # Odd-coefficient pruning (option (a) — byte-exact for ANY input)
// The reference prunes the odd sums via `col_limit`.  Rather than mask per
// column in-register, we PRE-ZERO the pruned source coefficients in the buffer
// (`hevc_prune_col` / `hevc_prune_row`) and run the FULL odd range uniformly
// across all columns — byte-identical to the pruned reference (see `hevc_kept`).
//
// # Overflow: i32 accumulators (matches the reference `int`).  A `TR_32` sum is
// at most `32 · 90 · 32767 ≈ 94M ≪ i32::MAX`, so no widening is needed.
// =========================================================================

/// `TRANSFORM[row][i]` as an `i16` multiplier (the shared 32×32 basis matrix).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
fn tcoef(row: usize, i: usize) -> i16 {
    crate::prims_scalar::TRANSFORM[row][i] as i16
}

/// Scalar square transpose `dst = srcᵀ` (light glue between the two SIMD passes;
/// the compute-heavy butterfly is vectorized, the transpose is O(H²) copy).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
unsafe fn transpose_sq_i16(src: *const i16, dst: *mut i16, h: usize) {
    unsafe {
        for a in 0..h {
            for b in 0..h {
                *dst.add(a * h + b) = *src.add(b * h + a);
            }
        }
    }
}

/// `TR_4` even/odd butterfly across 4 columns (`i32` lanes).  Inputs are 4
/// source rows (each 4 columns in the low 64 bits); outputs are 4 `i32` vecs.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn vtr4(s0: __m128i, s1: __m128i, s2: __m128i, s3: __m128i) -> [__m128i; 4] {
    unsafe {
        let m0 = mw(s0, 64);
        let m2 = mw(s2, 64);
        let e0 = _mm_add_epi32(m0, m2);
        let e1 = _mm_sub_epi32(m0, m2);
        let o0 = _mm_add_epi32(mw(s1, 83), mw(s3, 36));
        let o1 = _mm_sub_epi32(mw(s1, 36), mw(s3, 83));
        [
            _mm_add_epi32(e0, o0),
            _mm_add_epi32(e1, o1),
            _mm_sub_epi32(e1, o1),
            _mm_sub_epi32(e0, o0),
        ]
    }
}

/// `TR_8` across 4 columns.  `o[i] = Σ_{j∈{1,3,5,7}} TRANSFORM[4j][i]·s[j]`
/// (full range — pruning is pre-applied via source zeroing); even part = `TR_4`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn vtr8(s: &[__m128i; 8]) -> [__m128i; 8] {
    unsafe {
        let mut o = [_mm_setzero_si128(); 4];
        for i in 0..4 {
            let mut j = 1usize;
            while j < 8 {
                o[i] = _mm_add_epi32(o[i], mw(s[j], tcoef(4 * j, i)));
                j += 2;
            }
        }
        let e = vtr4(s[0], s[2], s[4], s[6]);
        let mut out = [_mm_setzero_si128(); 8];
        for i in 0..4 {
            out[i] = _mm_add_epi32(e[i], o[i]);
            out[7 - i] = _mm_sub_epi32(e[i], o[i]);
        }
        out
    }
}

/// `TR_16` across 4 columns.  `o[i] = Σ_{j odd<16} TRANSFORM[2j][i]·s[j]`;
/// even part = `TR_8` on the even-index sources.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn vtr16(s: &[__m128i; 16]) -> [__m128i; 16] {
    unsafe {
        let mut o = [_mm_setzero_si128(); 8];
        for i in 0..8 {
            let mut j = 1usize;
            while j < 16 {
                o[i] = _mm_add_epi32(o[i], mw(s[j], tcoef(2 * j, i)));
                j += 2;
            }
        }
        let even = [s[0], s[2], s[4], s[6], s[8], s[10], s[12], s[14]];
        let e = vtr8(&even);
        let mut out = [_mm_setzero_si128(); 16];
        for i in 0..8 {
            out[i] = _mm_add_epi32(e[i], o[i]);
            out[15 - i] = _mm_sub_epi32(e[i], o[i]);
        }
        out
    }
}

/// `TR_32` across 4 columns.  `o[i] = Σ_{j odd<32} TRANSFORM[j][i]·s[j]`;
/// even part = `TR_16` on the even-index sources.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn vtr32(s: &[__m128i; 32]) -> [__m128i; 32] {
    unsafe {
        let mut o = [_mm_setzero_si128(); 16];
        for i in 0..16 {
            let mut j = 1usize;
            while j < 32 {
                o[i] = _mm_add_epi32(o[i], mw(s[j], tcoef(j, i)));
                j += 2;
            }
        }
        let even = [
            s[0], s[2], s[4], s[6], s[8], s[10], s[12], s[14], s[16], s[18], s[20], s[22], s[24],
            s[26], s[28], s[30],
        ];
        let e = vtr16(&even);
        let mut out = [_mm_setzero_si128(); 32];
        for i in 0..16 {
            out[i] = _mm_add_epi32(e[i], o[i]);
            out[31 - i] = _mm_sub_epi32(e[i], o[i]);
        }
        out
    }
}

/// One separable pass (all columns of `inp` → `outp`), 4 columns at a time,
/// SCALEd with `(v + add) >> cnt` + `clip_int16`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn idct_pass_sse2(inp: *const i16, outp: *mut i16, h: usize, cnt: __m128i, add: __m128i) {
    unsafe {
        let mut c0 = 0usize;
        while c0 < h {
            match h {
                8 => {
                    let mut s = [_mm_setzero_si128(); 8];
                    for m in 0..8 {
                        s[m] = _mm_loadl_epi64(inp.add(m * 8 + c0) as *const __m128i);
                    }
                    let r = vtr8(&s);
                    for i in 0..8 {
                        _mm_storel_epi64(
                            outp.add(i * 8 + c0) as *mut __m128i,
                            scale_pack(r[i], add, cnt),
                        );
                    }
                }
                16 => {
                    let mut s = [_mm_setzero_si128(); 16];
                    for m in 0..16 {
                        s[m] = _mm_loadl_epi64(inp.add(m * 16 + c0) as *const __m128i);
                    }
                    let r = vtr16(&s);
                    for i in 0..16 {
                        _mm_storel_epi64(
                            outp.add(i * 16 + c0) as *mut __m128i,
                            scale_pack(r[i], add, cnt),
                        );
                    }
                }
                _ => {
                    let mut s = [_mm_setzero_si128(); 32];
                    for m in 0..32 {
                        s[m] = _mm_loadl_epi64(inp.add(m * 32 + c0) as *const __m128i);
                    }
                    let r = vtr32(&s);
                    for i in 0..32 {
                        _mm_storel_epi64(
                            outp.add(i * 32 + c0) as *mut __m128i,
                            scale_pack(r[i], add, cnt),
                        );
                    }
                }
            }
            c0 += 4;
        }
    }
}

/// Shared SSE2 driver: pre-zero → column pass → transpose → pre-zero → row pass
/// → transpose, in-place on the `H×H` block.  `tmp` is an `H*H` scratch.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn idct_run_sse2(coeffs: *mut i16, tmp: *mut i16, h: usize, col_limit: i32, bit_depth: u8) {
    unsafe {
        // Pass 1: columns, shift 7.
        crate::prims_scalar::hevc_prune_col(coeffs, h, col_limit);
        let cnt1 = _mm_cvtsi32_si128(7);
        let add1 = _mm_set1_epi32(1 << 6);
        idct_pass_sse2(coeffs, tmp, h, cnt1, add1);
        transpose_sq_i16(tmp, coeffs, h);
        // Pass 2: rows (columns of the transpose), shift 20 - bit_depth.
        crate::prims_scalar::hevc_prune_row(coeffs, h, col_limit);
        let sh2 = 20 - bit_depth as i32;
        let cnt2 = _mm_cvtsi32_si128(sh2);
        let add2 = _mm_set1_epi32(1 << (sh2 - 1));
        idct_pass_sse2(coeffs, tmp, h, cnt2, add2);
        transpose_sq_i16(tmp, coeffs, h);
    }
}

/// SSE2 impl of [`crate::hevc_idct8_i16_inplace`].  Byte-exact vs scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn hevc_idct8_i16_inplace(coeffs: *mut i16, col_limit: i32, bit_depth: u8) {
    unsafe {
        let mut tmp = [0i16; 64];
        idct_run_sse2(coeffs, tmp.as_mut_ptr(), 8, col_limit, bit_depth);
    }
}

/// SSE2 impl of [`crate::hevc_idct16_i16_inplace`].  Byte-exact vs scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn hevc_idct16_i16_inplace(coeffs: *mut i16, col_limit: i32, bit_depth: u8) {
    unsafe {
        let mut tmp = [0i16; 256];
        idct_run_sse2(coeffs, tmp.as_mut_ptr(), 16, col_limit, bit_depth);
    }
}

/// SSE2 impl of [`crate::hevc_idct32_i16_inplace`].  Byte-exact vs scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn hevc_idct32_i16_inplace(coeffs: *mut i16, col_limit: i32, bit_depth: u8) {
    unsafe {
        let mut tmp = [0i16; 1024];
        idct_run_sse2(coeffs, tmp.as_mut_ptr(), 32, col_limit, bit_depth);
    }
}
