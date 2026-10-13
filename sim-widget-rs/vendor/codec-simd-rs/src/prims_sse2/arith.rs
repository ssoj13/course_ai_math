#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// SSE2 impl of [`crate::abs_diff_u8`].  16 lanes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn abs_diff_u8(a: *const u8, b: *const u8, dst: *mut u8, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 16 <= n {
            let av = _mm_loadu_si128(a.add(i) as *const __m128i);
            let bv = _mm_loadu_si128(b.add(i) as *const __m128i);
            let mx = _mm_max_epu8(av, bv);
            let mn = _mm_min_epu8(av, bv);
            let d = _mm_sub_epi8(mx, mn);
            _mm_storeu_si128(dst.add(i) as *mut __m128i, d);
            i += 16;
        }
        while i < n {
            let av = *a.add(i);
            let bv = *b.add(i);
            *dst.add(i) = av.abs_diff(bv);
            i += 1;
        }
    }
}

/// SSE2 impl of [`crate::transpose_4x4_i16`].
///
/// Classic 4×4 SSE transpose using `unpacklo_epi16` + `unpacklo_epi32`.
/// Each row is 4 i16 = 64 bits — load 4 rows into 4 xmm low halves,
/// transpose, store.
///
/// Sequence: load 4 rows (8 bytes each), 2× unpacklo_epi16 to interleave
/// pairs, 2× unpacklo_epi32 to interleave quads.  4 stores.
///
/// # Safety
/// `src` valid for 16 `i16` reads; `dst` valid for 16 `i16` writes.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn transpose_4x4_i16(src: *const i16, dst: *mut i16) {
    unsafe {
        // Load 4 rows of 4 i16 each into the low 64 bits of xmm.
        let r0 = _mm_loadl_epi64(src.add(0) as *const __m128i);
        let r1 = _mm_loadl_epi64(src.add(4) as *const __m128i);
        let r2 = _mm_loadl_epi64(src.add(8) as *const __m128i);
        let r3 = _mm_loadl_epi64(src.add(12) as *const __m128i);

        // Interleave 16-bit lanes pairwise: rows 0+1, 2+3.
        // After unpacklo_epi16: [r0[0], r1[0], r0[1], r1[1], r0[2], r1[2], r0[3], r1[3]]
        let r01 = _mm_unpacklo_epi16(r0, r1);
        let r23 = _mm_unpacklo_epi16(r2, r3);

        // Interleave 32-bit lanes: produces full 4×4 transposed pairs.
        // r0123_lo holds rows 0&1 of transposed; r0123_hi holds rows 2&3.
        let r0123_lo = _mm_unpacklo_epi32(r01, r23);
        let r0123_hi = _mm_unpackhi_epi32(r01, r23);

        // Store each transposed row (4 i16 = 64 bits).
        _mm_storel_epi64(dst.add(0) as *mut __m128i, r0123_lo);
        _mm_storel_epi64(
            dst.add(4) as *mut __m128i,
            _mm_unpackhi_epi64(r0123_lo, r0123_lo),
        );
        _mm_storel_epi64(dst.add(8) as *mut __m128i, r0123_hi);
        _mm_storel_epi64(
            dst.add(12) as *mut __m128i,
            _mm_unpackhi_epi64(r0123_hi, r0123_hi),
        );
    }
}

/// SSE2 impl of [`crate::transpose_8x8_i16`].
///
/// Classic 8×8 SSE in-register transpose.  16 unpack instructions in
/// 3 rounds:
///   1. unpacklo/hi_epi16 — interleave 16-bit pairs across row pairs.
///   2. unpacklo/hi_epi32 — interleave 32-bit quads.
///   3. unpacklo/hi_epi64 — final 64-bit interleave between halves.
///
/// 8 loads, 8 stores.  See e.g. ffmpeg `transpose_8x8_h` macro.
///
/// # Safety
/// `src` valid for 64 `i16` reads; `dst` valid for 64 `i16` writes.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn transpose_8x8_i16(src: *const i16, dst: *mut i16) {
    unsafe {
        // Load 8 rows × 8 i16 = 16 bytes each.
        let r0 = _mm_loadu_si128(src.add(0) as *const __m128i);
        let r1 = _mm_loadu_si128(src.add(8) as *const __m128i);
        let r2 = _mm_loadu_si128(src.add(16) as *const __m128i);
        let r3 = _mm_loadu_si128(src.add(24) as *const __m128i);
        let r4 = _mm_loadu_si128(src.add(32) as *const __m128i);
        let r5 = _mm_loadu_si128(src.add(40) as *const __m128i);
        let r6 = _mm_loadu_si128(src.add(48) as *const __m128i);
        let r7 = _mm_loadu_si128(src.add(56) as *const __m128i);

        // Round 1: 16-bit interleave.
        let a0 = _mm_unpacklo_epi16(r0, r1);
        let a1 = _mm_unpackhi_epi16(r0, r1);
        let a2 = _mm_unpacklo_epi16(r2, r3);
        let a3 = _mm_unpackhi_epi16(r2, r3);
        let a4 = _mm_unpacklo_epi16(r4, r5);
        let a5 = _mm_unpackhi_epi16(r4, r5);
        let a6 = _mm_unpacklo_epi16(r6, r7);
        let a7 = _mm_unpackhi_epi16(r6, r7);

        // Round 2: 32-bit interleave.
        let b0 = _mm_unpacklo_epi32(a0, a2);
        let b1 = _mm_unpackhi_epi32(a0, a2);
        let b2 = _mm_unpacklo_epi32(a1, a3);
        let b3 = _mm_unpackhi_epi32(a1, a3);
        let b4 = _mm_unpacklo_epi32(a4, a6);
        let b5 = _mm_unpackhi_epi32(a4, a6);
        let b6 = _mm_unpacklo_epi32(a5, a7);
        let b7 = _mm_unpackhi_epi32(a5, a7);

        // Round 3: 64-bit interleave → final transposed rows.
        let t0 = _mm_unpacklo_epi64(b0, b4);
        let t1 = _mm_unpackhi_epi64(b0, b4);
        let t2 = _mm_unpacklo_epi64(b1, b5);
        let t3 = _mm_unpackhi_epi64(b1, b5);
        let t4 = _mm_unpacklo_epi64(b2, b6);
        let t5 = _mm_unpackhi_epi64(b2, b6);
        let t6 = _mm_unpacklo_epi64(b3, b7);
        let t7 = _mm_unpackhi_epi64(b3, b7);

        _mm_storeu_si128(dst.add(0) as *mut __m128i, t0);
        _mm_storeu_si128(dst.add(8) as *mut __m128i, t1);
        _mm_storeu_si128(dst.add(16) as *mut __m128i, t2);
        _mm_storeu_si128(dst.add(24) as *mut __m128i, t3);
        _mm_storeu_si128(dst.add(32) as *mut __m128i, t4);
        _mm_storeu_si128(dst.add(40) as *mut __m128i, t5);
        _mm_storeu_si128(dst.add(48) as *mut __m128i, t6);
        _mm_storeu_si128(dst.add(56) as *mut __m128i, t7);
    }
}

/// SSE2 impl of [`crate::sat_add_i16`].  8 lanes/iter via `paddsw`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn sat_add_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            let av = _mm_loadu_si128(a.add(i) as *const __m128i);
            let bv = _mm_loadu_si128(b.add(i) as *const __m128i);
            _mm_storeu_si128(dst.add(i) as *mut __m128i, _mm_adds_epi16(av, bv));
            i += 8;
        }
        while i < n {
            let av = *a.add(i) as i32;
            let bv = *b.add(i) as i32;
            *dst.add(i) = (av + bv).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}

/// SSE2 impl of [`crate::sat_sub_i16`].  8 lanes/iter via `psubsw`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn sat_sub_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            let av = _mm_loadu_si128(a.add(i) as *const __m128i);
            let bv = _mm_loadu_si128(b.add(i) as *const __m128i);
            _mm_storeu_si128(dst.add(i) as *mut __m128i, _mm_subs_epi16(av, bv));
            i += 8;
        }
        while i < n {
            let av = *a.add(i) as i32;
            let bv = *b.add(i) as i32;
            *dst.add(i) = (av - bv).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}

/// SSE2 impl of [`crate::abs_i16`].
///
/// SSE2 has no `pabsw` (that's SSSE3+).  Emulates via `max(x, -x)`:
/// `_mm_max_epi16` is SSE2.  Note `-i16::MIN` overflows; we use the
/// scalar-equivalent saturating semantic where `abs(i16::MIN) = i16::MAX`.
/// `_mm_subs_epi16(zero, x)` saturating-negates, so
/// `max(x, subs_epi16(0, x))` is bit-exact with the scalar reference.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn abs_i16(src: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let zero = _mm_setzero_si128();
        let mut i = 0usize;
        while i + 8 <= n {
            let v = _mm_loadu_si128(src.add(i) as *const __m128i);
            // Saturating negate: -i16::MIN -> i16::MAX (matches scalar).
            let neg = _mm_subs_epi16(zero, v);
            _mm_storeu_si128(dst.add(i) as *mut __m128i, _mm_max_epi16(v, neg));
            i += 8;
        }
        while i < n {
            let v = *src.add(i);
            *dst.add(i) = if v == i16::MIN { i16::MAX } else { v.abs() };
            i += 1;
        }
    }
}

/// SSE2 impl of [`crate::mul_hi_round_i16`].
///
/// SSE2 has no `pmulhrsw` (that's SSSE3+).  Emulates:
///   widen i16×i16 → 2× i32 via unpack+srai sign-extend trick + pmaddwd
///   isn't suitable (pairs); instead use pmullw + pmulhw to get lo/hi
///   parts of products, combine into i32, +round, >>15, pack.
///
/// 8 lanes per iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn mul_hi_round_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let round = _mm_set1_epi32(1 << 14);
        let mut i = 0usize;
        while i + 8 <= n {
            let av = _mm_loadu_si128(a.add(i) as *const __m128i);
            let bv = _mm_loadu_si128(b.add(i) as *const __m128i);
            // Get full i32 products by computing low+high i16 halves
            // and reassembling. pmullw gives low 16 bits, pmulhw gives high.
            let p_lo = _mm_mullo_epi16(av, bv);
            let p_hi = _mm_mulhi_epi16(av, bv);
            // Interleave to form 4 × i32 lanes (low half) and another 4
            // (high half).
            let p32_lo = _mm_unpacklo_epi16(p_lo, p_hi); // 4 × i32 (lanes 0..3)
            let p32_hi = _mm_unpackhi_epi16(p_lo, p_hi); // 4 × i32 (lanes 4..7)
            // + (1 << 14), then >> 15.
            let r_lo = _mm_srai_epi32(_mm_add_epi32(p32_lo, round), 15);
            let r_hi = _mm_srai_epi32(_mm_add_epi32(p32_hi, round), 15);
            // Pack i32 -> i16 with signed saturation.
            let packed = _mm_packs_epi32(r_lo, r_hi);
            _mm_storeu_si128(dst.add(i) as *mut __m128i, packed);
            i += 8;
        }
        while i < n {
            let av = *a.add(i) as i32;
            let bv = *b.add(i) as i32;
            let p = av.wrapping_mul(bv).wrapping_add(1 << 14) >> 15;
            *dst.add(i) = p.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}

/// SSE2 impl of [`crate::dequant_block_i16`].  8 i16/iter; signed
/// widening multiply (`mullo`+`mulhi`+`unpack`), round-shift, `packs`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn dequant_block_i16(buf: *mut i16, n: usize, scale: i16, shift: u32) {
    unsafe {
        let scv = _mm_set1_epi16(scale);
        let round = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let sc = scale as i32;
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let mut i = 0usize;
        while i + 8 <= n {
            let v = _mm_loadu_si128(buf.add(i) as *const __m128i);
            let plo = _mm_mullo_epi16(v, scv);
            let phi = _mm_mulhi_epi16(v, scv);
            let lo = _mm_sra_epi32(_mm_add_epi32(_mm_unpacklo_epi16(plo, phi), round), cnt);
            let hi = _mm_sra_epi32(_mm_add_epi32(_mm_unpackhi_epi16(plo, phi), round), cnt);
            _mm_storeu_si128(buf.add(i) as *mut __m128i, _mm_packs_epi32(lo, hi));
            i += 8;
        }
        while i < n {
            let p = buf.add(i);
            let v = ((*p as i32) * sc + rscalar) >> shift;
            *p = v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}

/// SSE2 impl of [`crate::interleave_halves_u8`] — port of OpenEXR's own
/// SSE2 `interleave` (`internal_zip.c:161-191`).
///
/// 32 output bytes/iteration: two 16-byte loads (one from each half)
/// unpacked low/high (`_mm_unpacklo_epi8`/`_mm_unpackhi_epi8`) into two
/// interleaved 16-byte stores. The tail is handled scalar, one byte at a
/// time, exactly like C's own scalar tail loop (the bulk SIMD loop always
/// consumes an equal number of bytes from each half, so the tail's own
/// even/odd parity lines up with the whole buffer's).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn interleave_halves_u8(t1: *const u8, t2: *const u8, out: *mut u8, n: usize) {
    unsafe {
        const HALF_CHUNK: usize = 16;
        let v_n = n / (2 * HALF_CHUNK);

        let mut v1 = t1;
        let mut v2 = t2;
        let mut o = out;
        let mut i = 0usize;
        while i < v_n {
            let a = _mm_loadu_si128(v1 as *const __m128i);
            let b = _mm_loadu_si128(v2 as *const __m128i);
            let lo = _mm_unpacklo_epi8(a, b);
            let hi = _mm_unpackhi_epi8(a, b);
            _mm_storeu_si128(o as *mut __m128i, lo);
            _mm_storeu_si128(o.add(HALF_CHUNK) as *mut __m128i, hi);
            v1 = v1.add(HALF_CHUNK);
            v2 = v2.add(HALF_CHUNK);
            o = o.add(2 * HALF_CHUNK);
            i += 1;
        }

        let mut k = v_n * 2 * HALF_CHUNK;
        let mut i1 = v_n * HALF_CHUNK;
        let mut i2 = v_n * HALF_CHUNK;
        while k < n {
            *out.add(k) = *t1.add(i1);
            i1 += 1;
            k += 1;
            if k < n {
                *out.add(k) = *t2.add(i2);
                i2 += 1;
                k += 1;
            }
        }
    }
}
