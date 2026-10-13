#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// SSE2 impl of [`crate::sat_pack_i16_to_u8`].  16 lanes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn sat_pack_i16_to_u8(src: *const i16, dst: *mut u8, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 16 <= n {
            let lo = _mm_loadu_si128(src.add(i) as *const __m128i);
            let hi = _mm_loadu_si128(src.add(i + 8) as *const __m128i);
            let packed = _mm_packus_epi16(lo, hi);
            _mm_storeu_si128(dst.add(i) as *mut __m128i, packed);
            i += 16;
        }
        while i < n {
            let v = *src.add(i);
            *dst.add(i) = v.clamp(0, 255) as u8;
            i += 1;
        }
    }
}

/// SSE2 impl of [`crate::round_shift_narrow_i32_to_i16`].
///
/// `(src + round) >> shift`, then `_mm_packs_epi32` saturating-narrows
/// i32 → i16.  8 lanes/iter.  Shift is variable but small enough to be
/// passed as a register (uses `_mm_srai_epi32`'s `count` arg).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn round_shift_narrow_i32_to_i16(
    src: *const i32,
    dst: *mut i16,
    n: usize,
    shift: u32,
) {
    unsafe {
        let round = _mm_set1_epi32(1i32 << (shift - 1));
        let count = _mm_cvtsi32_si128(shift as i32);
        let mut i = 0usize;
        while i + 8 <= n {
            let lo = _mm_loadu_si128(src.add(i) as *const __m128i);
            let lo = _mm_sra_epi32(_mm_add_epi32(lo, round), count);
            let hi = _mm_loadu_si128(src.add(i + 4) as *const __m128i);
            let hi = _mm_sra_epi32(_mm_add_epi32(hi, round), count);
            let packed = _mm_packs_epi32(lo, hi);
            _mm_storeu_si128(dst.add(i) as *mut __m128i, packed);
            i += 8;
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
