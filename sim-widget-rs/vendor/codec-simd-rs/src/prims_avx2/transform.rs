//! AVX2 implementations of the HEVC inverse-transform family (size-4 IDCT /
//! DST-VII + the size-generic DC fast path).
//!
//! # SIMD strategy — vectorize ACROSS the 4 columns (native i32 path)
//! Same "4 columns in 4 lanes" model as the SSE2 backend, but using the
//! SSE4.1 32-bit ops available under AVX2: `cvtepi16_epi32` to widen the
//! source rows once, `mullo_epi32` for the constant multiplies (so the DST
//! core keeps the ORIGINAL `i16` coefficients in the collapsed direct-basis
//! matrix, algebraically identical to the reference cores), and
//! `min/max_epi32` for the `clip_int16` clamp — the whole butterfly stays in
//! the `i32` domain, so the inter-pass transpose is a native 4×4 `i32`
//! transpose (no re-narrow between passes).  Byte-exact vs the scalar
//! reference.  A 4×4 block is 4 `i32` lanes wide, so this is a 128-bit kernel
//! (256-bit gives no clean mapping onto a 4-point butterfly); the AVX2 gain
//! here is the native `mullo_epi32`/`cvtepi16_epi32`/`min_epi32` path over the
//! SSE2 `mullo`/`mulhi`/`unpack` widen.

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// Multiply an `i32` lane vector (4 columns) by constant `c`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn mw(s: __m128i, c: i32) -> __m128i {
    _mm_mullo_epi32(s, _mm_set1_epi32(c))
}

/// `SCALE`: `(v + add) >> shift` (arithmetic) then clamp to the `i16` range
/// (= `clip_int16`), staying in the `i32` domain.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn scale(v: __m128i, add: __m128i, cnt: __m128i) -> __m128i {
    let t = _mm_sra_epi32(_mm_add_epi32(v, add), cnt);
    let lo = _mm_set1_epi32(-32768);
    let hi = _mm_set1_epi32(32767);
    _mm_min_epi32(_mm_max_epi32(t, lo), hi)
}

/// Native 4×4 `i32` transpose (rows → columns).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn transpose4(
    a: __m128i,
    b: __m128i,
    c: __m128i,
    d: __m128i,
) -> (__m128i, __m128i, __m128i, __m128i) {
    let t0 = _mm_unpacklo_epi32(a, b);
    let t1 = _mm_unpackhi_epi32(a, b);
    let t2 = _mm_unpacklo_epi32(c, d);
    let t3 = _mm_unpackhi_epi32(c, d);
    (
        _mm_unpacklo_epi64(t0, t2),
        _mm_unpackhi_epi64(t0, t2),
        _mm_unpacklo_epi64(t1, t3),
        _mm_unpackhi_epi64(t1, t3),
    )
}

/// One DCT-4 pass across 4 lanes (`tr_4` even/odd butterfly + `SCALE`), i32.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
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
            scale(_mm_add_epi32(e0, o0), add, cnt),
            scale(_mm_add_epi32(e1, o1), add, cnt),
            scale(_mm_sub_epi32(e1, o1), add, cnt),
            scale(_mm_sub_epi32(e0, o0), add, cnt),
        )
    }
}

/// One DST-VII pass across 4 lanes (`tr_4x4_luma` collapsed matrix + `SCALE`).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
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
            scale(d0, add, cnt),
            scale(d1, add, cnt),
            scale(d2, add, cnt),
            scale(d3, add, cnt),
        )
    }
}

/// Widen a 4-`i16` row (low 64 bits of the load) to four `i32` lanes.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn load_row(coeffs: *const i16, off: usize) -> __m128i {
    unsafe { _mm_cvtepi16_epi32(_mm_loadl_epi64(coeffs.add(off) as *const __m128i)) }
}

/// Narrow four `i32` lanes (already in `i16` range) to 4 `i16` and store.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn store_row(coeffs: *mut i16, off: usize, v: __m128i) {
    unsafe {
        _mm_storel_epi64(coeffs.add(off) as *mut __m128i, _mm_packs_epi32(v, v));
    }
}

/// AVX2 impl of [`crate::hevc_idct4_i16_inplace`].  Byte-exact vs scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn hevc_idct4_i16_inplace(coeffs: *mut i16, bit_depth: u8) {
    unsafe {
        let r0 = load_row(coeffs, 0);
        let r1 = load_row(coeffs, 4);
        let r2 = load_row(coeffs, 8);
        let r3 = load_row(coeffs, 12);

        let cnt1 = _mm_cvtsi32_si128(7);
        let add1 = _mm_set1_epi32(1 << 6);
        let (o0, o1, o2, o3) = idct4_pass(r0, r1, r2, r3, add1, cnt1);

        let (c0, c1, c2, c3) = transpose4(o0, o1, o2, o3);

        let shift2 = 20 - bit_depth as i32;
        let cnt2 = _mm_cvtsi32_si128(shift2);
        let add2 = _mm_set1_epi32(1 << (shift2 - 1));
        let (p0, p1, p2, p3) = idct4_pass(c0, c1, c2, c3, add2, cnt2);

        let (f0, f1, f2, f3) = transpose4(p0, p1, p2, p3);
        store_row(coeffs, 0, f0);
        store_row(coeffs, 4, f1);
        store_row(coeffs, 8, f2);
        store_row(coeffs, 12, f3);
    }
}

/// AVX2 impl of [`crate::hevc_dst4_i16_inplace`].  Byte-exact vs scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn hevc_dst4_i16_inplace(coeffs: *mut i16, bit_depth: u8) {
    unsafe {
        let r0 = load_row(coeffs, 0);
        let r1 = load_row(coeffs, 4);
        let r2 = load_row(coeffs, 8);
        let r3 = load_row(coeffs, 12);

        let cnt1 = _mm_cvtsi32_si128(7);
        let add1 = _mm_set1_epi32(1 << 6);
        let (o0, o1, o2, o3) = dst4_pass(r0, r1, r2, r3, add1, cnt1);

        let (c0, c1, c2, c3) = transpose4(o0, o1, o2, o3);

        let shift2 = 20 - bit_depth as i32;
        let cnt2 = _mm_cvtsi32_si128(shift2);
        let add2 = _mm_set1_epi32(1 << (shift2 - 1));
        let (p0, p1, p2, p3) = dst4_pass(c0, c1, c2, c3, add2, cnt2);

        let (f0, f1, f2, f3) = transpose4(p0, p1, p2, p3);
        store_row(coeffs, 0, f0);
        store_row(coeffs, 4, f1);
        store_row(coeffs, 8, f2);
        store_row(coeffs, 12, f3);
    }
}

/// AVX2 impl of [`crate::hevc_idct_dc_i16_inplace`].  Splat-store the single
/// DC output value (256-bit stores) across the block.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn hevc_idct_dc_i16_inplace(coeffs: *mut i16, log2_size: u32, bit_depth: u8) {
    unsafe {
        let size = 1usize << log2_size;
        let n = size * size;
        let shift = 14 - bit_depth as i32;
        let add = 1i32 << (shift - 1);
        let coeff = (((((*coeffs) as i32 + 1) >> 1) + add) >> shift).clamp(-32768, 32767) as i16;
        let sv = _mm256_set1_epi16(coeff);
        let mut k = 0usize;
        while k + 16 <= n {
            _mm256_storeu_si256(coeffs.add(k) as *mut __m256i, sv);
            k += 16;
        }
        while k < n {
            *coeffs.add(k) = coeff;
            k += 1;
        }
    }
}

// =========================================================================
// HEVC inverse DCT-II — sizes 8 / 16 / 32 (AVX2)
//
// Same across-columns model as the SSE2 backend, but 8 columns per group in
// native `i32` lanes: `cvtepi16_epi32` widens 8 contiguous source `i16`,
// `mullo_epi32` does the constant multiplies, `min/max_epi32` clamps.  Pruning
// is pre-applied via source zeroing (`hevc_prune_col`/`hevc_prune_row`) so the
// odd sums run the FULL range — byte-identical to the pruned reference.  i32
// accumulators (a `TR_32` sum ≤ 32·90·32767 ≈ 94M ≪ i32::MAX).
// =========================================================================

/// `TRANSFORM[row][i]` as an `i32` multiplier (the shared 32×32 basis matrix).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
fn tcoef32(row: usize, i: usize) -> i32 {
    crate::prims_scalar::TRANSFORM[row][i] as i32
}

/// Scalar square transpose `dst = srcᵀ` (light glue between the two SIMD passes).
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

/// Multiply an 8-lane `i32` column vector by constant `c`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn mw8(s: __m256i, c: i32) -> __m256i {
    _mm256_mullo_epi32(s, _mm256_set1_epi32(c))
}

/// Widen 8 contiguous source `i16` (8 columns of one row) to 8 `i32` lanes.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn load8(inp: *const i16, off: usize) -> __m256i {
    unsafe { _mm256_cvtepi16_epi32(_mm_loadu_si128(inp.add(off) as *const __m128i)) }
}

/// SCALE `(v + add) >> cnt` then clamp to the `i16` range, then narrow the 8
/// `i32` lanes to 8 contiguous `i16` and store.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn store8(outp: *mut i16, off: usize, v: __m256i, add: __m256i, cnt: __m128i) {
    unsafe {
        let t = _mm256_sra_epi32(_mm256_add_epi32(v, add), cnt);
        let lo = _mm256_set1_epi32(-32768);
        let hi = _mm256_set1_epi32(32767);
        let clamped = _mm256_min_epi32(_mm256_max_epi32(t, lo), hi);
        // Narrow 8 i32 -> 8 i16 (values already in range): pack per-128-lane then
        // gather the two low halves into one contiguous 128-bit store.
        let p = _mm256_packs_epi32(clamped, clamped);
        let lo128 = _mm256_castsi256_si128(p); // [v0..3 | v0..3]
        let hi128 = _mm256_extracti128_si256(p, 1); // [v4..7 | v4..7]
        let packed = _mm_unpacklo_epi64(lo128, hi128); // [v0..3, v4..7]
        _mm_storeu_si128(outp.add(off) as *mut __m128i, packed);
    }
}

/// `TR_4` even/odd butterfly across 8 columns (`i32` lanes).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn vtr4(s0: __m256i, s1: __m256i, s2: __m256i, s3: __m256i) -> [__m256i; 4] {
    unsafe {
        let m0 = mw8(s0, 64);
        let m2 = mw8(s2, 64);
        let e0 = _mm256_add_epi32(m0, m2);
        let e1 = _mm256_sub_epi32(m0, m2);
        let o0 = _mm256_add_epi32(mw8(s1, 83), mw8(s3, 36));
        let o1 = _mm256_sub_epi32(mw8(s1, 36), mw8(s3, 83));
        [
            _mm256_add_epi32(e0, o0),
            _mm256_add_epi32(e1, o1),
            _mm256_sub_epi32(e1, o1),
            _mm256_sub_epi32(e0, o0),
        ]
    }
}

/// `TR_8` across 8 columns (full odd range; pruning pre-applied).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn vtr8(s: &[__m256i; 8]) -> [__m256i; 8] {
    unsafe {
        let mut o = [_mm256_setzero_si256(); 4];
        for i in 0..4 {
            let mut j = 1usize;
            while j < 8 {
                o[i] = _mm256_add_epi32(o[i], mw8(s[j], tcoef32(4 * j, i)));
                j += 2;
            }
        }
        let e = vtr4(s[0], s[2], s[4], s[6]);
        let mut out = [_mm256_setzero_si256(); 8];
        for i in 0..4 {
            out[i] = _mm256_add_epi32(e[i], o[i]);
            out[7 - i] = _mm256_sub_epi32(e[i], o[i]);
        }
        out
    }
}

/// `TR_16` across 8 columns.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn vtr16(s: &[__m256i; 16]) -> [__m256i; 16] {
    unsafe {
        let mut o = [_mm256_setzero_si256(); 8];
        for i in 0..8 {
            let mut j = 1usize;
            while j < 16 {
                o[i] = _mm256_add_epi32(o[i], mw8(s[j], tcoef32(2 * j, i)));
                j += 2;
            }
        }
        let even = [s[0], s[2], s[4], s[6], s[8], s[10], s[12], s[14]];
        let e = vtr8(&even);
        let mut out = [_mm256_setzero_si256(); 16];
        for i in 0..8 {
            out[i] = _mm256_add_epi32(e[i], o[i]);
            out[15 - i] = _mm256_sub_epi32(e[i], o[i]);
        }
        out
    }
}

/// `TR_32` across 8 columns.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn vtr32(s: &[__m256i; 32]) -> [__m256i; 32] {
    unsafe {
        let mut o = [_mm256_setzero_si256(); 16];
        for i in 0..16 {
            let mut j = 1usize;
            while j < 32 {
                o[i] = _mm256_add_epi32(o[i], mw8(s[j], tcoef32(j, i)));
                j += 2;
            }
        }
        let even = [
            s[0], s[2], s[4], s[6], s[8], s[10], s[12], s[14], s[16], s[18], s[20], s[22], s[24],
            s[26], s[28], s[30],
        ];
        let e = vtr16(&even);
        let mut out = [_mm256_setzero_si256(); 32];
        for i in 0..16 {
            out[i] = _mm256_add_epi32(e[i], o[i]);
            out[31 - i] = _mm256_sub_epi32(e[i], o[i]);
        }
        out
    }
}

/// One separable pass (all columns of `inp` → `outp`), 8 columns at a time.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn idct_pass_avx2(inp: *const i16, outp: *mut i16, h: usize, cnt: __m128i, add: __m256i) {
    unsafe {
        let mut c0 = 0usize;
        while c0 < h {
            match h {
                8 => {
                    let mut s = [_mm256_setzero_si256(); 8];
                    for m in 0..8 {
                        s[m] = load8(inp, m * 8 + c0);
                    }
                    let r = vtr8(&s);
                    for i in 0..8 {
                        store8(outp, i * 8 + c0, r[i], add, cnt);
                    }
                }
                16 => {
                    let mut s = [_mm256_setzero_si256(); 16];
                    for m in 0..16 {
                        s[m] = load8(inp, m * 16 + c0);
                    }
                    let r = vtr16(&s);
                    for i in 0..16 {
                        store8(outp, i * 16 + c0, r[i], add, cnt);
                    }
                }
                _ => {
                    let mut s = [_mm256_setzero_si256(); 32];
                    for m in 0..32 {
                        s[m] = load8(inp, m * 32 + c0);
                    }
                    let r = vtr32(&s);
                    for i in 0..32 {
                        store8(outp, i * 32 + c0, r[i], add, cnt);
                    }
                }
            }
            c0 += 8;
        }
    }
}

/// Shared AVX2 driver (pre-zero → col pass → transpose → pre-zero → row pass
/// → transpose), in-place on the `H×H` block.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn idct_run_avx2(coeffs: *mut i16, tmp: *mut i16, h: usize, col_limit: i32, bit_depth: u8) {
    unsafe {
        crate::prims_scalar::hevc_prune_col(coeffs, h, col_limit);
        let cnt1 = _mm_cvtsi32_si128(7);
        let add1 = _mm256_set1_epi32(1 << 6);
        idct_pass_avx2(coeffs, tmp, h, cnt1, add1);
        transpose_sq_i16(tmp, coeffs, h);
        crate::prims_scalar::hevc_prune_row(coeffs, h, col_limit);
        let sh2 = 20 - bit_depth as i32;
        let cnt2 = _mm_cvtsi32_si128(sh2);
        let add2 = _mm256_set1_epi32(1 << (sh2 - 1));
        idct_pass_avx2(coeffs, tmp, h, cnt2, add2);
        transpose_sq_i16(tmp, coeffs, h);
    }
}

/// AVX2 impl of [`crate::hevc_idct8_i16_inplace`].  Byte-exact vs scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn hevc_idct8_i16_inplace(coeffs: *mut i16, col_limit: i32, bit_depth: u8) {
    unsafe {
        let mut tmp = [0i16; 64];
        idct_run_avx2(coeffs, tmp.as_mut_ptr(), 8, col_limit, bit_depth);
    }
}

/// AVX2 impl of [`crate::hevc_idct16_i16_inplace`].  Byte-exact vs scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn hevc_idct16_i16_inplace(coeffs: *mut i16, col_limit: i32, bit_depth: u8) {
    unsafe {
        let mut tmp = [0i16; 256];
        idct_run_avx2(coeffs, tmp.as_mut_ptr(), 16, col_limit, bit_depth);
    }
}

/// AVX2 impl of [`crate::hevc_idct32_i16_inplace`].  Byte-exact vs scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn hevc_idct32_i16_inplace(coeffs: *mut i16, col_limit: i32, bit_depth: u8) {
    unsafe {
        let mut tmp = [0i16; 1024];
        idct_run_avx2(coeffs, tmp.as_mut_ptr(), 32, col_limit, bit_depth);
    }
}
