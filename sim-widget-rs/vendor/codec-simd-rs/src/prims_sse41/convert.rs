#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// SSE4.1 impl of [`crate::sat_pack_i32_to_u16`].
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse4.1")]
pub(crate) unsafe fn sat_pack_i32_to_u16(src: *const i32, dst: *mut u16, n: usize, max: u16) {
    unsafe {
        let zero = _mm_setzero_si128();
        let maxv = _mm_set1_epi32(max as i32);
        let mut i = 0usize;
        while i + 8 <= n {
            let lo = _mm_loadu_si128(src.add(i) as *const __m128i);
            let lo = _mm_max_epi32(lo, zero);
            let lo = _mm_min_epi32(lo, maxv);
            let hi = _mm_loadu_si128(src.add(i + 4) as *const __m128i);
            let hi = _mm_max_epi32(hi, zero);
            let hi = _mm_min_epi32(hi, maxv);
            let packed = _mm_packus_epi32(lo, hi);
            _mm_storeu_si128(dst.add(i) as *mut __m128i, packed);
            i += 8;
        }
        if i + 4 <= n {
            let v = _mm_loadu_si128(src.add(i) as *const __m128i);
            let v = _mm_max_epi32(v, zero);
            let v = _mm_min_epi32(v, maxv);
            let packed = _mm_packus_epi32(v, v);
            _mm_storel_epi64(dst.add(i) as *mut __m128i, packed);
            i += 4;
        }
        let max_i32 = max as i32;
        while i < n {
            let v = *src.add(i);
            *dst.add(i) = v.max(0).min(max_i32) as u16;
            i += 1;
        }
    }
}

/// SSE4.1 impl of [`crate::add_clip_to_pixel_u16`].
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse4.1")]
pub(crate) unsafe fn add_clip_to_pixel_u16(
    pred: *const u16,
    res: *const i16,
    dst: *mut u16,
    n: usize,
    max: u16,
) {
    unsafe {
        let zero = _mm_setzero_si128();
        let maxv = _mm_set1_epi32(max as i32);
        let mut i = 0usize;
        while i + 8 <= n {
            let p = _mm_loadu_si128(pred.add(i) as *const __m128i);
            let r = _mm_loadu_si128(res.add(i) as *const __m128i);

            let p_lo = _mm_cvtepu16_epi32(p);
            let r_lo = _mm_cvtepi16_epi32(r);
            let s_lo = _mm_add_epi32(p_lo, r_lo);
            let s_lo = _mm_max_epi32(s_lo, zero);
            let s_lo = _mm_min_epi32(s_lo, maxv);

            let p_hi_raw = _mm_srli_si128::<8>(p);
            let r_hi_raw = _mm_srli_si128::<8>(r);
            let p_hi = _mm_cvtepu16_epi32(p_hi_raw);
            let r_hi = _mm_cvtepi16_epi32(r_hi_raw);
            let s_hi = _mm_add_epi32(p_hi, r_hi);
            let s_hi = _mm_max_epi32(s_hi, zero);
            let s_hi = _mm_min_epi32(s_hi, maxv);

            let packed = _mm_packus_epi32(s_lo, s_hi);
            _mm_storeu_si128(dst.add(i) as *mut __m128i, packed);
            i += 8;
        }
        let max_i32 = max as i32;
        while i < n {
            let p = *pred.add(i) as i32;
            let r = *res.add(i) as i32;
            *dst.add(i) = (p + r).max(0).min(max_i32) as u16;
            i += 1;
        }
    }
}

/// SSE4.1 impl of [`crate::add_clip_to_pixel_u8`].  16 lanes/iter.
///
/// Widens `u8` pred and `i16` res to four `i32x4` vectors each
/// (`cvtepu8_epi32` / `cvtepi16_epi32`), adds, clamps to `[0, 255]` in i32
/// (`max_epi32`/`min_epi32`), then narrows i32 → u16 (`packus_epi32`) → u8
/// (`packus_epi16`).  All packs are within-lane on 128-bit so no permute is
/// needed.  Accumulating in i32 is mandatory: `255 + i16::MAX` overflows i16.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse4.1")]
pub(crate) unsafe fn add_clip_to_pixel_u8(
    pred: *const u8,
    res: *const i16,
    dst: *mut u8,
    n: usize,
) {
    unsafe {
        let zero = _mm_setzero_si128();
        let maxv = _mm_set1_epi32(255);
        let mut i = 0usize;
        while i + 16 <= n {
            let p = _mm_loadu_si128(pred.add(i) as *const __m128i);
            let r0 = _mm_loadu_si128(res.add(i) as *const __m128i);
            let r1 = _mm_loadu_si128(res.add(i + 8) as *const __m128i);

            // Widen 16 u8 -> four i32x4 (bytes 0-3, 4-7, 8-11, 12-15).
            let p0 = _mm_cvtepu8_epi32(p);
            let p1 = _mm_cvtepu8_epi32(_mm_srli_si128::<4>(p));
            let p2 = _mm_cvtepu8_epi32(_mm_srli_si128::<8>(p));
            let p3 = _mm_cvtepu8_epi32(_mm_srli_si128::<12>(p));

            // Widen 16 i16 -> four i32x4 (sign-extend).
            let r_0 = _mm_cvtepi16_epi32(r0);
            let r_1 = _mm_cvtepi16_epi32(_mm_srli_si128::<8>(r0));
            let r_2 = _mm_cvtepi16_epi32(r1);
            let r_3 = _mm_cvtepi16_epi32(_mm_srli_si128::<8>(r1));

            let s0 = _mm_min_epi32(_mm_max_epi32(_mm_add_epi32(p0, r_0), zero), maxv);
            let s1 = _mm_min_epi32(_mm_max_epi32(_mm_add_epi32(p1, r_1), zero), maxv);
            let s2 = _mm_min_epi32(_mm_max_epi32(_mm_add_epi32(p2, r_2), zero), maxv);
            let s3 = _mm_min_epi32(_mm_max_epi32(_mm_add_epi32(p3, r_3), zero), maxv);

            let pk_lo = _mm_packus_epi32(s0, s1); // 8 u16
            let pk_hi = _mm_packus_epi32(s2, s3); // 8 u16
            let out = _mm_packus_epi16(pk_lo, pk_hi); // 16 u8
            _mm_storeu_si128(dst.add(i) as *mut __m128i, out);
            i += 16;
        }
        while i < n {
            let p = *pred.add(i) as i32;
            let r = *res.add(i) as i32;
            *dst.add(i) = (p + r).clamp(0, 255) as u8;
            i += 1;
        }
    }
}

/// SSE4.1 impl of [`crate::clamp_i32_to_i16`].
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse4.1")]
pub(crate) unsafe fn clamp_i32_to_i16(buf: *mut i32, n: usize) {
    unsafe {
        const MIN: i32 = i16::MIN as i32;
        const MAX: i32 = i16::MAX as i32;
        let minv = _mm_set1_epi32(MIN);
        let maxv = _mm_set1_epi32(MAX);
        let mut i = 0usize;
        while i + 4 <= n {
            let v = _mm_loadu_si128(buf.add(i) as *const __m128i);
            let v = _mm_max_epi32(v, minv);
            let v = _mm_min_epi32(v, maxv);
            _mm_storeu_si128(buf.add(i) as *mut __m128i, v);
            i += 4;
        }
        while i < n {
            let p = buf.add(i);
            let v = *p;
            *p = v.clamp(MIN, MAX);
            i += 1;
        }
    }
}

/// SSE4.1 impl of [`crate::mac_widen_i16_to_i32`].
///
/// 8 lanes/iter.  Widens 8 `i16` from each input to two 4-lane `i32`
/// vectors via `cvtepi16_epi32` (sign-extend), multiplies with
/// `pmulld` (SSE4.1), adds into the accumulator (also 8 × i32).
///
/// SSE2 does not have `pmulld` (i32 × i32 → i32); pre-SSE4.1 CPUs run
/// the scalar fallback.
///
/// # Safety
/// `a`/`b` valid for `n` `i16` reads; `acc` valid for `n` `i32`
/// reads-and-writes.  CPU must support SSE4.1.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse4.1")]
pub(crate) unsafe fn mac_widen_i16_to_i32(a: *const i16, b: *const i16, acc: *mut i32, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            let av = _mm_loadu_si128(a.add(i) as *const __m128i);
            let bv = _mm_loadu_si128(b.add(i) as *const __m128i);

            // Widen low 4 lanes i16 -> i32 sign-extend, multiply, accumulate.
            let a_lo = _mm_cvtepi16_epi32(av);
            let b_lo = _mm_cvtepi16_epi32(bv);
            let m_lo = _mm_mullo_epi32(a_lo, b_lo);
            let acc_lo = _mm_loadu_si128(acc.add(i) as *const __m128i);
            let s_lo = _mm_add_epi32(acc_lo, m_lo);
            _mm_storeu_si128(acc.add(i) as *mut __m128i, s_lo);

            // High 4 lanes.
            let a_hi = _mm_cvtepi16_epi32(_mm_srli_si128::<8>(av));
            let b_hi = _mm_cvtepi16_epi32(_mm_srli_si128::<8>(bv));
            let m_hi = _mm_mullo_epi32(a_hi, b_hi);
            let acc_hi = _mm_loadu_si128(acc.add(i + 4) as *const __m128i);
            let s_hi = _mm_add_epi32(acc_hi, m_hi);
            _mm_storeu_si128(acc.add(i + 4) as *mut __m128i, s_hi);
            i += 8;
        }
        while i < n {
            let av = *a.add(i) as i32;
            let bv = *b.add(i) as i32;
            let p = acc.add(i);
            *p = (*p).wrapping_add(av.wrapping_mul(bv));
            i += 1;
        }
    }
}
