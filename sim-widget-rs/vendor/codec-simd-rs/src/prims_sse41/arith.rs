#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// SSE4.1 (incl. SSSE3) impl of [`crate::mul_hi_round_i16`].
///
/// Uses `_mm_mulhrs_epi16` — SSSE3's "multiply high with rounding and
/// scaling".  Semantically: `((a * b) + (1 << 14)) >> 15` — exactly our
/// target.  Single-instruction per 8 lanes.  Saturates `i16::MIN ×
/// i16::MIN` to `i16::MAX` (the only overflow case).
///
/// (CPU must support SSSE3, which is implied by our SSE4.1 dispatch tier.)
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "ssse3")]
pub(crate) unsafe fn mul_hi_round_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    // Fixup needed: _mm_mulhrs_epi16 does NOT saturate. For MIN×MIN the
    // raw result is 0x8000 (-32768); we want 0x7FFF (+32767).  XOR with
    // a mask of `(a==MIN) AND (b==MIN)` flips exactly those affected
    // lanes from 0x8000 to 0x7FFF, no-op elsewhere.
    unsafe {
        let min = _mm_set1_epi16(i16::MIN);
        let mut i = 0usize;
        while i + 8 <= n {
            let av = _mm_loadu_si128(a.add(i) as *const __m128i);
            let bv = _mm_loadu_si128(b.add(i) as *const __m128i);
            let r = _mm_mulhrs_epi16(av, bv);
            let a_is_min = _mm_cmpeq_epi16(av, min);
            let b_is_min = _mm_cmpeq_epi16(bv, min);
            let both_min = _mm_and_si128(a_is_min, b_is_min);
            _mm_storeu_si128(dst.add(i) as *mut __m128i, _mm_xor_si128(r, both_min));
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

/// SSE4.1-tier (only SSSE3 instructions used) impl of
/// [`crate::prefix_sum_bias_u8_inplace`] — port of OpenEXR's own SSE4.1
/// `reconstruct` (`internal_zip.c:49-94`).
///
/// 16 bytes/iteration via a classic SIMD prefix-sum (shift-and-add tree over
/// `_mm_slli_si128` by 1/2/4/8 bytes), carrying the running delta between
/// 16-byte blocks by broadcasting the previous block's LAST lane
/// (`_mm_shuffle_epi8` with an all-15 mask, SSSE3's `pshufb`). `buf[0]` is
/// pre-adjusted by `-128` (matching C's unconditional `buf[0] += -128;`
/// before the loop) so the loop's uniform `+(-128)` per lane leaves it
/// unmodified overall — this holds regardless of `n`, including `n < 16`
/// (the vector loop then never runs and the scalar tail starts at index 0
/// with `prev = 0`, algebraically the same as a "block with carry-in 0").
///
/// C avoids `_mm_extract_epi8` (a true SSE4.1 immediate-lane extract) by
/// using a stack store instead, so this only actually needs SSSE3 — but is
/// still installed at the SSE4.1 dispatch tier, matching C's own
/// `IMF_HAVE_SSE4_1` gate exactly (a hardware SSSE3-only CPU never reaches
/// this function; `mul_hi_round_i16` above makes the identical choice).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "ssse3")]
pub(crate) unsafe fn prefix_sum_bias_u8_inplace(buf: *mut u8, n: usize) {
    unsafe {
        const CHUNK: usize = 16;
        let v_n = n / CHUNK;

        if n > 0 {
            *buf = (*buf).wrapping_sub(128);
        }

        let c = _mm_set1_epi8(-128i8);
        let shuffle_mask = _mm_set1_epi8(15i8);
        let mut v_prev = _mm_setzero_si128();

        let mut i = 0usize;
        while i < v_n {
            let p = buf.add(i * CHUNK) as *mut __m128i;
            let mut d = _mm_add_epi8(_mm_loadu_si128(p as *const __m128i), c);

            d = _mm_add_epi8(d, _mm_slli_si128(d, 1));
            d = _mm_add_epi8(d, _mm_slli_si128(d, 2));
            d = _mm_add_epi8(d, _mm_slli_si128(d, 4));
            d = _mm_add_epi8(d, _mm_slli_si128(d, 8));
            d = _mm_add_epi8(d, v_prev);

            _mm_storeu_si128(p, d);
            v_prev = _mm_shuffle_epi8(d, shuffle_mask);
            i += 1;
        }

        let mut prev_arr = [0u8; 16];
        _mm_storeu_si128(prev_arr.as_mut_ptr() as *mut __m128i, v_prev);
        let mut prev = prev_arr[15];

        let mut j = v_n * CHUNK;
        while j < n {
            let p = buf.add(j);
            let d = prev.wrapping_add(*p).wrapping_sub(128);
            *p = d;
            prev = d;
            j += 1;
        }
    }
}
