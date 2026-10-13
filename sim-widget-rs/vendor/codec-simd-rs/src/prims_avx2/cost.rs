#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// AVX2 impl of [`crate::sad_block_u8`].
///
/// `_mm256_sad_epu8` — 32 bytes per row per instruction, producing four
/// u16 partial sums in the low halves of each of four 64-bit lanes.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn sad_block_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    w: usize,
    h: usize,
) -> u32 {
    unsafe {
        let mut acc = _mm256_setzero_si256();
        let mut scalar_acc: u32 = 0;
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let mut col = 0usize;
            while col + 32 <= w {
                let av = _mm256_loadu_si256(a_row.add(col) as *const __m256i);
                let bv = _mm256_loadu_si256(b_row.add(col) as *const __m256i);
                acc = _mm256_add_epi64(acc, _mm256_sad_epu8(av, bv));
                col += 32;
            }
            // 16-byte SSE2 chunk
            if col + 16 <= w {
                let av = _mm_loadu_si128(a_row.add(col) as *const __m128i);
                let bv = _mm_loadu_si128(b_row.add(col) as *const __m128i);
                let s = _mm_sad_epu8(av, bv);
                // Add to low 128 bits of acc; widen to 256-bit ZMM not needed.
                let s256 = _mm256_zextsi128_si256(s);
                acc = _mm256_add_epi64(acc, s256);
                col += 16;
            }
            if col + 8 <= w {
                let av = _mm_loadl_epi64(a_row.add(col) as *const __m128i);
                let bv = _mm_loadl_epi64(b_row.add(col) as *const __m128i);
                let s = _mm_sad_epu8(av, bv);
                let s256 = _mm256_zextsi128_si256(s);
                acc = _mm256_add_epi64(acc, s256);
                col += 8;
            }
            while col < w {
                scalar_acc += (*a_row.add(col)).abs_diff(*b_row.add(col)) as u32;
                col += 1;
            }
        }
        // Horizontal sum of acc (4 × u64).
        let lo128 = _mm256_castsi256_si128(acc);
        let hi128 = _mm256_extracti128_si256::<1>(acc);
        let s128 = _mm_add_epi64(lo128, hi128);
        let s = _mm_add_epi64(s128, _mm_shuffle_epi32::<0b00_00_11_10>(s128));
        scalar_acc + _mm_cvtsi128_si32(s) as u32
    }
}

/// AVX2 impl of [`crate::sse_block_u8`].  32 bytes per chunk via
/// `_mm256_madd_epi16` (i16² pair-sums into i32 lanes).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn sse_block_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    w: usize,
    h: usize,
) -> u64 {
    unsafe {
        let zero = _mm256_setzero_si256();
        let mut sum: u64 = 0;
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let mut acc = _mm256_setzero_si256();
            let mut col = 0usize;
            while col + 32 <= w {
                let av = _mm256_loadu_si256(a_row.add(col) as *const __m256i);
                let bv = _mm256_loadu_si256(b_row.add(col) as *const __m256i);
                let a_lo = _mm256_unpacklo_epi8(av, zero);
                let a_hi = _mm256_unpackhi_epi8(av, zero);
                let b_lo = _mm256_unpacklo_epi8(bv, zero);
                let b_hi = _mm256_unpackhi_epi8(bv, zero);
                let d_lo = _mm256_sub_epi16(a_lo, b_lo);
                let d_hi = _mm256_sub_epi16(a_hi, b_hi);
                acc = _mm256_add_epi32(acc, _mm256_madd_epi16(d_lo, d_lo));
                acc = _mm256_add_epi32(acc, _mm256_madd_epi16(d_hi, d_hi));
                col += 32;
            }
            // 16-byte SSE2 chunk for remainder
            let zero128 = _mm_setzero_si128();
            let mut acc128 = _mm_setzero_si128();
            if col + 16 <= w {
                let av = _mm_loadu_si128(a_row.add(col) as *const __m128i);
                let bv = _mm_loadu_si128(b_row.add(col) as *const __m128i);
                let a_lo = _mm_unpacklo_epi8(av, zero128);
                let a_hi = _mm_unpackhi_epi8(av, zero128);
                let b_lo = _mm_unpacklo_epi8(bv, zero128);
                let b_hi = _mm_unpackhi_epi8(bv, zero128);
                let d_lo = _mm_sub_epi16(a_lo, b_lo);
                let d_hi = _mm_sub_epi16(a_hi, b_hi);
                acc128 = _mm_add_epi32(acc128, _mm_madd_epi16(d_lo, d_lo));
                acc128 = _mm_add_epi32(acc128, _mm_madd_epi16(d_hi, d_hi));
                col += 16;
            }
            if col + 8 <= w {
                let av = _mm_loadl_epi64(a_row.add(col) as *const __m128i);
                let bv = _mm_loadl_epi64(b_row.add(col) as *const __m128i);
                let a16 = _mm_unpacklo_epi8(av, zero128);
                let b16 = _mm_unpacklo_epi8(bv, zero128);
                let d = _mm_sub_epi16(a16, b16);
                acc128 = _mm_add_epi32(acc128, _mm_madd_epi16(d, d));
                col += 8;
            }
            // Drain acc + acc128 to u64.
            // Sum 8 i32 lanes of acc:
            let acc_lo = _mm256_castsi256_si128(acc);
            let acc_hi = _mm256_extracti128_si256::<1>(acc);
            let s128 = _mm_add_epi32(_mm_add_epi32(acc_lo, acc_hi), acc128);
            let hi = _mm_shuffle_epi32::<0b00_00_11_10>(s128);
            let s = _mm_add_epi32(s128, hi);
            let hi2 = _mm_shuffle_epi32::<0b00_00_00_01>(s);
            let total = _mm_add_epi32(s, hi2);
            sum += _mm_cvtsi128_si32(total) as u32 as u64;
            while col < w {
                let av = *a_row.add(col) as i32;
                let bv = *b_row.add(col) as i32;
                let d = av - bv;
                sum += (d * d) as u64;
                col += 1;
            }
        }
        sum
    }
}

/// AVX2 impl of [`crate::variance_block_u8`].  32 bytes/iter via
/// `_mm256_sad_epu8` (byte sum) and `_mm256_madd_epi16` (Σv²), falling
/// through to SSE2 16/8-byte paths for remainders.  `sum_sq` i32
/// accumulator drained per row to u64.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn variance_block_u8(
    src: *const u8,
    stride: isize,
    w: usize,
    h: usize,
) -> (u32, u64) {
    unsafe {
        let zero = _mm256_setzero_si256();
        let zero128 = _mm_setzero_si128();
        let mut sum_acc = _mm256_setzero_si256(); // 4 × u64 byte-sum partials
        let mut sum_sq: u64 = 0;
        let mut sum_scalar: u32 = 0;
        for row in 0..h {
            let s_row = src.offset(row as isize * stride);
            let mut sq_acc = _mm256_setzero_si256(); // 8 × i32
            let mut sq_acc128 = _mm_setzero_si128(); // 4 × i32 (remainder)
            let mut col = 0usize;
            while col + 32 <= w {
                let v = _mm256_loadu_si256(s_row.add(col) as *const __m256i);
                sum_acc = _mm256_add_epi64(sum_acc, _mm256_sad_epu8(v, zero));
                let lo = _mm256_unpacklo_epi8(v, zero);
                let hi = _mm256_unpackhi_epi8(v, zero);
                sq_acc = _mm256_add_epi32(sq_acc, _mm256_madd_epi16(lo, lo));
                sq_acc = _mm256_add_epi32(sq_acc, _mm256_madd_epi16(hi, hi));
                col += 32;
            }
            if col + 16 <= w {
                let v = _mm_loadu_si128(s_row.add(col) as *const __m128i);
                sum_acc =
                    _mm256_add_epi64(sum_acc, _mm256_zextsi128_si256(_mm_sad_epu8(v, zero128)));
                let lo = _mm_unpacklo_epi8(v, zero128);
                let hi = _mm_unpackhi_epi8(v, zero128);
                sq_acc128 = _mm_add_epi32(sq_acc128, _mm_madd_epi16(lo, lo));
                sq_acc128 = _mm_add_epi32(sq_acc128, _mm_madd_epi16(hi, hi));
                col += 16;
            }
            if col + 8 <= w {
                let v = _mm_loadl_epi64(s_row.add(col) as *const __m128i);
                sum_acc =
                    _mm256_add_epi64(sum_acc, _mm256_zextsi128_si256(_mm_sad_epu8(v, zero128)));
                let lo = _mm_unpacklo_epi8(v, zero128);
                sq_acc128 = _mm_add_epi32(sq_acc128, _mm_madd_epi16(lo, lo));
                col += 8;
            }
            // Drain sq_acc (8 × i32) + sq_acc128 (4 × i32) → u64.
            let sq_lo = _mm256_castsi256_si128(sq_acc);
            let sq_hi = _mm256_extracti128_si256::<1>(sq_acc);
            let s128 = _mm_add_epi32(_mm_add_epi32(sq_lo, sq_hi), sq_acc128);
            let hi = _mm_shuffle_epi32::<0b00_00_11_10>(s128);
            let s = _mm_add_epi32(s128, hi);
            let hi2 = _mm_shuffle_epi32::<0b00_00_00_01>(s);
            let total = _mm_add_epi32(s, hi2);
            sum_sq += _mm_cvtsi128_si32(total) as u32 as u64;
            while col < w {
                let vv = *s_row.add(col) as u32;
                sum_scalar += vv;
                sum_sq += (vv * vv) as u64;
                col += 1;
            }
        }
        // Horizontal sum of sum_acc (4 × u64).
        let lo128 = _mm256_castsi256_si128(sum_acc);
        let hi128 = _mm256_extracti128_si256::<1>(sum_acc);
        let s128 = _mm_add_epi64(lo128, hi128);
        let s = _mm_add_epi64(s128, _mm_shuffle_epi32::<0b00_00_11_10>(s128));
        let sum = sum_scalar + _mm_cvtsi128_si32(s) as u32;
        (sum, sum_sq)
    }
}

/// AVX2 impl of [`crate::sad_x4_block_u8`].  32 bytes/iter via
/// `_mm256_sad_epu8`; `a` loaded once per chunk, reused for 4 candidates.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn sad_x4_block_u8(
    a: *const u8,
    a_stride: isize,
    b0: *const u8,
    b1: *const u8,
    b2: *const u8,
    b3: *const u8,
    b_stride: isize,
    w: usize,
    h: usize,
) -> [u32; 4] {
    unsafe {
        let bs = [b0, b1, b2, b3];
        let mut acc = [_mm256_setzero_si256(); 4];
        let mut scalar = [0u32; 4];
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let off = row as isize * b_stride;
            let br = [
                bs[0].offset(off),
                bs[1].offset(off),
                bs[2].offset(off),
                bs[3].offset(off),
            ];
            let mut col = 0usize;
            while col + 32 <= w {
                let av = _mm256_loadu_si256(a_row.add(col) as *const __m256i);
                for k in 0..4 {
                    let bv = _mm256_loadu_si256(br[k].add(col) as *const __m256i);
                    acc[k] = _mm256_add_epi64(acc[k], _mm256_sad_epu8(av, bv));
                }
                col += 32;
            }
            if col + 16 <= w {
                let av = _mm_loadu_si128(a_row.add(col) as *const __m128i);
                for k in 0..4 {
                    let bv = _mm_loadu_si128(br[k].add(col) as *const __m128i);
                    acc[k] = _mm256_add_epi64(acc[k], _mm256_zextsi128_si256(_mm_sad_epu8(av, bv)));
                }
                col += 16;
            }
            if col + 8 <= w {
                let av = _mm_loadl_epi64(a_row.add(col) as *const __m128i);
                for k in 0..4 {
                    let bv = _mm_loadl_epi64(br[k].add(col) as *const __m128i);
                    acc[k] = _mm256_add_epi64(acc[k], _mm256_zextsi128_si256(_mm_sad_epu8(av, bv)));
                }
                col += 8;
            }
            while col < w {
                let av = *a_row.add(col);
                for k in 0..4 {
                    scalar[k] += av.abs_diff(*br[k].add(col)) as u32;
                }
                col += 1;
            }
        }
        let mut out = [0u32; 4];
        for k in 0..4 {
            let lo = _mm256_castsi256_si128(acc[k]);
            let hi = _mm256_extracti128_si256::<1>(acc[k]);
            let s = _mm_add_epi64(lo, hi);
            let s = _mm_add_epi64(s, _mm_shuffle_epi32::<0b00_00_11_10>(s));
            out[k] = scalar[k] + _mm_cvtsi128_si32(s) as u32;
        }
        out
    }
}
