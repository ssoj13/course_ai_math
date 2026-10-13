#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// AVX2 impl of [`crate::abs_diff_u8`].  32 lanes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn abs_diff_u8(a: *const u8, b: *const u8, dst: *mut u8, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 32 <= n {
            let av = _mm256_loadu_si256(a.add(i) as *const __m256i);
            let bv = _mm256_loadu_si256(b.add(i) as *const __m256i);
            let mx = _mm256_max_epu8(av, bv);
            let mn = _mm256_min_epu8(av, bv);
            let d = _mm256_sub_epi8(mx, mn);
            _mm256_storeu_si256(dst.add(i) as *mut __m256i, d);
            i += 32;
        }
        while i < n {
            let av = *a.add(i);
            let bv = *b.add(i);
            *dst.add(i) = av.abs_diff(bv);
            i += 1;
        }
    }
}

/// AVX2 impl of [`crate::sat_add_i16`].  16 lanes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn sat_add_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 16 <= n {
            let av = _mm256_loadu_si256(a.add(i) as *const __m256i);
            let bv = _mm256_loadu_si256(b.add(i) as *const __m256i);
            _mm256_storeu_si256(dst.add(i) as *mut __m256i, _mm256_adds_epi16(av, bv));
            i += 16;
        }
        while i < n {
            let av = *a.add(i) as i32;
            let bv = *b.add(i) as i32;
            *dst.add(i) = (av + bv).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}

/// AVX2 impl of [`crate::sat_sub_i16`].  16 lanes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn sat_sub_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 16 <= n {
            let av = _mm256_loadu_si256(a.add(i) as *const __m256i);
            let bv = _mm256_loadu_si256(b.add(i) as *const __m256i);
            _mm256_storeu_si256(dst.add(i) as *mut __m256i, _mm256_subs_epi16(av, bv));
            i += 16;
        }
        while i < n {
            let av = *a.add(i) as i32;
            let bv = *b.add(i) as i32;
            *dst.add(i) = (av - bv).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}

/// AVX2 impl of [`crate::abs_i16`].
///
/// `_mm256_abs_epi16` would NOT saturate `i16::MIN` (it returns
/// `0x8000 = i16::MIN` for that input, since `-(-32768)` overflows).
/// Use `max(v, subs_epi16(0, v))` instead — saturating negate produces
/// `i16::MAX` for `i16::MIN` input, and signed-max picks the positive
/// (or saturated) version.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn abs_i16(src: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let zero = _mm256_setzero_si256();
        let mut i = 0usize;
        while i + 16 <= n {
            let v = _mm256_loadu_si256(src.add(i) as *const __m256i);
            let neg = _mm256_subs_epi16(zero, v);
            _mm256_storeu_si256(dst.add(i) as *mut __m256i, _mm256_max_epi16(v, neg));
            i += 16;
        }
        while i < n {
            let v = *src.add(i);
            *dst.add(i) = if v == i16::MIN { i16::MAX } else { v.abs() };
            i += 1;
        }
    }
}

/// AVX2 impl of [`crate::mul_hi_round_i16`].
///
/// `_mm256_mulhrs_epi16` — same semantics as SSSE3 version at 256-bit.
/// 16 lanes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mul_hi_round_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    // Same MIN×MIN fixup as SSSE3 variant (see prims_sse41.rs).
    unsafe {
        let min = _mm256_set1_epi16(i16::MIN);
        let mut i = 0usize;
        while i + 16 <= n {
            let av = _mm256_loadu_si256(a.add(i) as *const __m256i);
            let bv = _mm256_loadu_si256(b.add(i) as *const __m256i);
            let r = _mm256_mulhrs_epi16(av, bv);
            let a_is_min = _mm256_cmpeq_epi16(av, min);
            let b_is_min = _mm256_cmpeq_epi16(bv, min);
            let both_min = _mm256_and_si256(a_is_min, b_is_min);
            _mm256_storeu_si256(dst.add(i) as *mut __m256i, _mm256_xor_si256(r, both_min));
            i += 16;
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

/// AVX2 impl of [`crate::dequant_block_i16`].  16 i16/iter.  The per-128
/// `unpack`/`packs` scrambles cancel, so element order is preserved.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn dequant_block_i16(buf: *mut i16, n: usize, scale: i16, shift: u32) {
    unsafe {
        let scv = _mm256_set1_epi16(scale);
        let round = _mm256_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let sc = scale as i32;
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let mut i = 0usize;
        while i + 16 <= n {
            let v = _mm256_loadu_si256(buf.add(i) as *const __m256i);
            let plo = _mm256_mullo_epi16(v, scv);
            let phi = _mm256_mulhi_epi16(v, scv);
            let lo = _mm256_sra_epi32(
                _mm256_add_epi32(_mm256_unpacklo_epi16(plo, phi), round),
                cnt,
            );
            let hi = _mm256_sra_epi32(
                _mm256_add_epi32(_mm256_unpackhi_epi16(plo, phi), round),
                cnt,
            );
            _mm256_storeu_si256(buf.add(i) as *mut __m256i, _mm256_packs_epi32(lo, hi));
            i += 16;
        }
        while i < n {
            let p = buf.add(i);
            let v = ((*p as i32) * sc + rscalar) >> shift;
            *p = v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}
