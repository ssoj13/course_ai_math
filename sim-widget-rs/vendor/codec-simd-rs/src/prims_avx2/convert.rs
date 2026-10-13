#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// AVX2 impl of [`crate::sat_pack_i16_to_u8`].  32 lanes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn sat_pack_i16_to_u8(src: *const i16, dst: *mut u8, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 32 <= n {
            let lo = _mm256_loadu_si256(src.add(i) as *const __m256i);
            let hi = _mm256_loadu_si256(src.add(i + 16) as *const __m256i);
            let packed = _mm256_packus_epi16(lo, hi);
            let fixed = _mm256_permute4x64_epi64::<0b1101_1000>(packed);
            _mm256_storeu_si256(dst.add(i) as *mut __m256i, fixed);
            i += 32;
        }
        while i < n {
            let v = *src.add(i);
            *dst.add(i) = v.clamp(0, 255) as u8;
            i += 1;
        }
    }
}

/// AVX2 impl of [`crate::sat_pack_i32_to_u16`].  16 lanes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn sat_pack_i32_to_u16(src: *const i32, dst: *mut u16, n: usize, max: u16) {
    unsafe {
        let zero = _mm256_setzero_si256();
        let maxv = _mm256_set1_epi32(max as i32);
        let mut i = 0usize;
        while i + 16 <= n {
            let lo = _mm256_loadu_si256(src.add(i) as *const __m256i);
            let lo = _mm256_max_epi32(lo, zero);
            let lo = _mm256_min_epi32(lo, maxv);
            let hi = _mm256_loadu_si256(src.add(i + 8) as *const __m256i);
            let hi = _mm256_max_epi32(hi, zero);
            let hi = _mm256_min_epi32(hi, maxv);
            let packed = _mm256_packus_epi32(lo, hi);
            let fixed = _mm256_permute4x64_epi64::<0b1101_1000>(packed);
            _mm256_storeu_si256(dst.add(i) as *mut __m256i, fixed);
            i += 16;
        }
        let max_i32 = max as i32;
        while i < n {
            let v = *src.add(i);
            *dst.add(i) = v.max(0).min(max_i32) as u16;
            i += 1;
        }
    }
}

/// AVX2 impl of [`crate::add_clip_to_pixel_u16`].  16 lanes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn add_clip_to_pixel_u16(
    pred: *const u16,
    res: *const i16,
    dst: *mut u16,
    n: usize,
    max: u16,
) {
    unsafe {
        let zero = _mm256_setzero_si256();
        let maxv = _mm256_set1_epi32(max as i32);
        let mut i = 0usize;
        while i + 16 <= n {
            let p = _mm256_loadu_si256(pred.add(i) as *const __m256i);
            let r = _mm256_loadu_si256(res.add(i) as *const __m256i);

            let p_lo = _mm256_cvtepu16_epi32(_mm256_castsi256_si128(p));
            let p_hi = _mm256_cvtepu16_epi32(_mm256_extracti128_si256::<1>(p));
            let r_lo = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(r));
            let r_hi = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(r));

            let s_lo = _mm256_add_epi32(p_lo, r_lo);
            let s_lo = _mm256_max_epi32(s_lo, zero);
            let s_lo = _mm256_min_epi32(s_lo, maxv);
            let s_hi = _mm256_add_epi32(p_hi, r_hi);
            let s_hi = _mm256_max_epi32(s_hi, zero);
            let s_hi = _mm256_min_epi32(s_hi, maxv);

            let packed = _mm256_packus_epi32(s_lo, s_hi);
            let fixed = _mm256_permute4x64_epi64::<0b1101_1000>(packed);
            _mm256_storeu_si256(dst.add(i) as *mut __m256i, fixed);
            i += 16;
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

/// AVX2 impl of [`crate::add_clip_to_pixel_u8`].  16 lanes/iter.
///
/// Widens 16 `u8` pred and 16 `i16` res to two `i32x8` vectors each
/// (`_mm256_cvtepu8_epi32` / `_mm256_cvtepi16_epi32`), adds, clamps to
/// `[0, 255]` in i32, then packs i32 → u16 (`packus_epi32`, fixed with the
/// `0b1101_1000` `permute4x64` for the AVX2 intra-lane pack) → u8
/// (`packus_epi16`, gathered with `0b0000_1000`).  Stores 16 bytes.
/// Accumulating in i32 is mandatory: `255 + i16::MAX` overflows i16.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn add_clip_to_pixel_u8(
    pred: *const u8,
    res: *const i16,
    dst: *mut u8,
    n: usize,
) {
    unsafe {
        let zero = _mm256_setzero_si256();
        let maxv = _mm256_set1_epi32(255);
        let mut i = 0usize;
        while i + 16 <= n {
            let p = _mm_loadu_si128(pred.add(i) as *const __m128i);
            let r = _mm256_loadu_si256(res.add(i) as *const __m256i);

            let p_lo = _mm256_cvtepu8_epi32(p); // bytes 0-7
            let p_hi = _mm256_cvtepu8_epi32(_mm_srli_si128::<8>(p)); // bytes 8-15
            let r_lo = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(r));
            let r_hi = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(r));

            let s_lo = _mm256_min_epi32(_mm256_max_epi32(_mm256_add_epi32(p_lo, r_lo), zero), maxv);
            let s_hi = _mm256_min_epi32(_mm256_max_epi32(_mm256_add_epi32(p_hi, r_hi), zero), maxv);

            // Pack 16 i32 -> 16 u16, fixing the AVX2 intra-lane interleave.
            let pk = _mm256_packus_epi32(s_lo, s_hi);
            let pk = _mm256_permute4x64_epi64::<0b1101_1000>(pk);
            // Narrow u16 -> u8; both 128-bit halves hold the wanted quad low.
            let u8v = _mm256_packus_epi16(pk, pk);
            let u8v = _mm256_permute4x64_epi64::<0b0000_1000>(u8v);
            let out = _mm256_castsi256_si128(u8v); // 16 u8
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

/// AVX2 impl of [`crate::clamp_i32_to_i16`].  8 lanes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn clamp_i32_to_i16(buf: *mut i32, n: usize) {
    unsafe {
        const MIN: i32 = i16::MIN as i32;
        const MAX: i32 = i16::MAX as i32;
        let minv = _mm256_set1_epi32(MIN);
        let maxv = _mm256_set1_epi32(MAX);
        let mut i = 0usize;
        while i + 8 <= n {
            let v = _mm256_loadu_si256(buf.add(i) as *const __m256i);
            let v = _mm256_max_epi32(v, minv);
            let v = _mm256_min_epi32(v, maxv);
            _mm256_storeu_si256(buf.add(i) as *mut __m256i, v);
            i += 8;
        }
        while i < n {
            let p = buf.add(i);
            let v = *p;
            *p = v.clamp(MIN, MAX);
            i += 1;
        }
    }
}

/// AVX2 impl of [`crate::mac_widen_i16_to_i32`].
///
/// 16 lanes/iter.  Splits 256-bit i16 loads into two 128-bit halves,
/// widens each to 256-bit i32 with `_mm256_cvtepi16_epi32`, multiplies
/// with `_mm256_mullo_epi32`, accumulates.
///
/// # Safety
/// `a`/`b` valid for `n` `i16` reads; `acc` valid for `n` `i32` r/w.
/// CPU must support AVX2.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mac_widen_i16_to_i32(a: *const i16, b: *const i16, acc: *mut i32, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 16 <= n {
            let av = _mm256_loadu_si256(a.add(i) as *const __m256i);
            let bv = _mm256_loadu_si256(b.add(i) as *const __m256i);

            let a_lo = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(av));
            let b_lo = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(bv));
            let m_lo = _mm256_mullo_epi32(a_lo, b_lo);
            let acc_lo = _mm256_loadu_si256(acc.add(i) as *const __m256i);
            let s_lo = _mm256_add_epi32(acc_lo, m_lo);
            _mm256_storeu_si256(acc.add(i) as *mut __m256i, s_lo);

            let a_hi = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(av));
            let b_hi = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(bv));
            let m_hi = _mm256_mullo_epi32(a_hi, b_hi);
            let acc_hi = _mm256_loadu_si256(acc.add(i + 8) as *const __m256i);
            let s_hi = _mm256_add_epi32(acc_hi, m_hi);
            _mm256_storeu_si256(acc.add(i + 8) as *mut __m256i, s_hi);
            i += 16;
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

/// AVX2 impl of [`crate::round_shift_narrow_i32_to_i16`].
///
/// Same shape as SSE2 but 256-bit.  16 lanes/iter.  The
/// `_mm256_packs_epi32` has the same intra-lane gotcha as packus —
/// fix with `_mm256_permute4x64_epi64::<0b1101_1000>`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn round_shift_narrow_i32_to_i16(
    src: *const i32,
    dst: *mut i16,
    n: usize,
    shift: u32,
) {
    unsafe {
        let round = _mm256_set1_epi32(1i32 << (shift - 1));
        let count = _mm_cvtsi32_si128(shift as i32);
        let mut i = 0usize;
        while i + 16 <= n {
            let lo = _mm256_loadu_si256(src.add(i) as *const __m256i);
            let lo = _mm256_sra_epi32(_mm256_add_epi32(lo, round), count);
            let hi = _mm256_loadu_si256(src.add(i + 8) as *const __m256i);
            let hi = _mm256_sra_epi32(_mm256_add_epi32(hi, round), count);
            let packed = _mm256_packs_epi32(lo, hi);
            let fixed = _mm256_permute4x64_epi64::<0b1101_1000>(packed);
            _mm256_storeu_si256(dst.add(i) as *mut __m256i, fixed);
            i += 16;
        }
        let shift_i32 = shift;
        let round_i = 1i32 << (shift - 1);
        while i < n {
            let v = *src.add(i);
            let r = v.wrapping_add(round_i) >> shift_i32;
            *dst.add(i) = r.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}
