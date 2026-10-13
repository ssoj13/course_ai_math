#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// AVX2 impl of [`crate::mc_avg_u8`].  `_mm256_avg_epu8` — 32 bytes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_avg_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    dst: *mut u8,
    dst_stride: isize,
    w: usize,
    h: usize,
) {
    unsafe {
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 32 <= w {
                let av = _mm256_loadu_si256(a_row.add(col) as *const __m256i);
                let bv = _mm256_loadu_si256(b_row.add(col) as *const __m256i);
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, _mm256_avg_epu8(av, bv));
                col += 32;
            }
            if col + 16 <= w {
                let av = _mm_loadu_si128(a_row.add(col) as *const __m128i);
                let bv = _mm_loadu_si128(b_row.add(col) as *const __m128i);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, _mm_avg_epu8(av, bv));
                col += 16;
            }
            if col + 8 <= w {
                let av = _mm_loadl_epi64(a_row.add(col) as *const __m128i);
                let bv = _mm_loadl_epi64(b_row.add(col) as *const __m128i);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_avg_epu8(av, bv));
                col += 8;
            }
            while col < w {
                let av = *a_row.add(col) as u16;
                let bv = *b_row.add(col) as u16;
                *d_row.add(col) = ((av + bv + 1) >> 1) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_8tap_h_u8`].  16 output columns per iteration.
///
/// `src` bytes widen u8→i16→i32; the exact i32 product with the broadcast
/// tap uses `_mm256_mullo_epi32` (u8 × i16 never overflows i32),
/// accumulated in two i32x8 lanes (cols 0-7 / 8-15).  A remainder of 8
/// falls through to an SSE2-style 128-bit path (`mullo`+`mulhi`+`unpack`);
/// the final tail is scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_8tap_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let round = _mm256_set1_epi32(64);
        let round128 = _mm_set1_epi32(64);
        let mut ft = [_mm256_setzero_si256(); 8]; // i32 broadcast per tap
        let mut ft16 = [zero128; 8]; // i16 broadcast (8-col path)
        let mut f = [0i32; 8];
        for i in 0..8 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256(); // cols 0-7
                let mut acc1 = _mm256_setzero_si256(); // cols 8-15
                for i in 0..8 {
                    let s = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm256_cvtepu8_epi16(s);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s16));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s16));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_srai_epi32(_mm256_add_epi32(acc0, round), 7);
                let r1 = _mm256_srai_epi32(_mm256_add_epi32(acc1, round), 7);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero128);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_srai_epi32(_mm_add_epi32(acc_lo, round128), 7);
                let r_hi = _mm_srai_epi32(_mm_add_epi32(acc_hi, round128), 7);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + 64) >> 7).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_8tap_v_u8`].  Vertical 8-tap, 16 output
/// columns per iteration.
///
/// Each tap loads a whole source row (16 bytes), widens u8→i16→i32, and
/// multiplies by the broadcast tap with `_mm256_mullo_epi32` (exact),
/// accumulating cols 0-7 / 8-15.  Remainder of 8 uses the SSE2 path;
/// final tail scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_8tap_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let round = _mm256_set1_epi32(64);
        let round128 = _mm_set1_epi32(64);
        let mut ft = [_mm256_setzero_si256(); 8];
        let mut ft16 = [zero128; 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..8 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s = _mm_loadu_si128(s_ptr as *const __m128i);
                    let s16 = _mm256_cvtepu8_epi16(s);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s16));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s16));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_srai_epi32(_mm256_add_epi32(acc0, round), 7);
                let r1 = _mm256_srai_epi32(_mm256_add_epi32(acc1, round), 7);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s8 = _mm_loadl_epi64(s_ptr as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero128);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_srai_epi32(_mm_add_epi32(acc_lo, round128), 7);
                let r_hi = _mm_srai_epi32(_mm_add_epi32(acc_hi, round128), 7);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + 64) >> 7).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_4tap_h_u8`].  16 cols/iter (`_mm256_mullo_epi32`),
/// 8-col + scalar tails, runtime `shift` via `_mm256_sra_epi32`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_4tap_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let round = _mm256_set1_epi32(1 << (shift - 1));
        let round128 = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 4];
        let mut ft16 = [zero128; 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..4 {
                    let s = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm256_cvtepu8_epi16(s);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s16));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s16));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero128);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_4tap_v_u8`].  Vertical 4-tap, 16 cols/iter,
/// runtime `shift`.  Each tap loads a whole source row.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_4tap_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let round = _mm256_set1_epi32(1 << (shift - 1));
        let round128 = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 4];
        let mut ft16 = [zero128; 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..4 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s = _mm_loadu_si128(s_ptr as *const __m128i);
                    let s16 = _mm256_cvtepu8_epi16(s);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s16));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s16));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s8 = _mm_loadl_epi64(s_ptr as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero128);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_6tap_h_u8`].  16 cols/iter, 8-col + scalar
/// tails, runtime `shift`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_6tap_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let round = _mm256_set1_epi32(1 << (shift - 1));
        let round128 = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 6];
        let mut ft16 = [zero128; 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..6 {
                    let s = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm256_cvtepu8_epi16(s);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s16));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s16));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..6 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero128);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..6 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_6tap_v_u8`].  Vertical 6-tap, 16 cols/iter,
/// runtime `shift`.  Each tap loads a whole source row.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_6tap_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let round = _mm256_set1_epi32(1 << (shift - 1));
        let round128 = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 6];
        let mut ft16 = [zero128; 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..6 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s = _mm_loadu_si128(s_ptr as *const __m128i);
                    let s16 = _mm256_cvtepu8_epi16(s);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s16));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s16));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..6 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s8 = _mm_loadl_epi64(s_ptr as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero128);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..6 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_bilinear_h_u8`].  16 cols/iter, 8-col +
/// scalar tails, runtime `shift`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_bilinear_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let round = _mm256_set1_epi32(1 << (shift - 1));
        let round128 = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let ft = [
            _mm256_set1_epi32(*filter as i32),
            _mm256_set1_epi32(*filter.add(1) as i32),
        ];
        let ft16 = [_mm_set1_epi16(*filter), _mm_set1_epi16(*filter.add(1))];
        let f = [*filter as i32, *filter.add(1) as i32];
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..2 {
                    let s = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm256_cvtepu8_epi16(s);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s16));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s16));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..2 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero128);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let acc = *s_row.add(col) as i32 * f[0] + *s_row.add(col + 1) as i32 * f[1];
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_bilinear_v_u8`].  Vertical 2-tap, 16
/// cols/iter, runtime `shift`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_bilinear_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let round = _mm256_set1_epi32(1 << (shift - 1));
        let round128 = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let ft = [
            _mm256_set1_epi32(*filter as i32),
            _mm256_set1_epi32(*filter.add(1) as i32),
        ];
        let ft16 = [_mm_set1_epi16(*filter), _mm_set1_epi16(*filter.add(1))];
        let f = [*filter as i32, *filter.add(1) as i32];
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..2 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s = _mm_loadu_si128(s_ptr as *const __m128i);
                    let s16 = _mm256_cvtepu8_epi16(s);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s16));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s16));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..2 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s8 = _mm_loadl_epi64(s_ptr as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero128);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let a = *src.offset(row as isize * src_stride).add(col) as i32;
                let b = *src.offset((row + 1) as isize * src_stride).add(col) as i32;
                *d_row.add(col) = ((a * f[0] + b * f[1] + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_8tap_h_u8_to_i16`].  i16-output horizontal
/// 8-tap, 16 cols/iter (`mullo_epi32`), 8-col + scalar tails, runtime
/// `shift`.  Saturating `packs_epi32` to i16, no pixel clip.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_8tap_h_u8_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let round = _mm256_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let round128 = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 8];
        let mut ft16 = [zero128; 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..8 {
                    let s = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm256_cvtepu8_epi16(s);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s16));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s16));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, p0);
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storeu_si128(d_row.add(col + 8) as *mut __m128i, p1);
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero128);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, _mm_packs_epi32(r_lo, r_hi));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) =
                    ((acc + rscalar) >> shift).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_8tap_v_i16_to_u8`].  Vertical 8-tap over an
/// i16 intermediate, 16 cols/iter, runtime `shift`, clipped to u8.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_8tap_v_i16_to_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let round = _mm256_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let round128 = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 8];
        let mut ft16 = [_mm_setzero_si128(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..8 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s = _mm256_loadu_si256(s_ptr as *const __m256i);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(s_ptr as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_4tap_h_u8_to_i16`].  i16-output horizontal
/// 4-tap, 16 cols/iter, runtime `shift`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_4tap_h_u8_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let round = _mm256_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let round128 = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 4];
        let mut ft16 = [zero128; 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..4 {
                    let s = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm256_cvtepu8_epi16(s);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s16));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s16));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                _mm_storeu_si128(
                    d_row.add(col) as *mut __m128i,
                    _mm_packs_epi32(
                        _mm256_castsi256_si128(r0),
                        _mm256_extracti128_si256::<1>(r0),
                    ),
                );
                _mm_storeu_si128(
                    d_row.add(col + 8) as *mut __m128i,
                    _mm_packs_epi32(
                        _mm256_castsi256_si128(r1),
                        _mm256_extracti128_si256::<1>(r1),
                    ),
                );
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero128);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, _mm_packs_epi32(r_lo, r_hi));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) =
                    ((acc + rscalar) >> shift).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_4tap_v_i16_to_u8`].  i16-input vertical
/// 4-tap, 16 cols/iter, runtime `shift`, clipped to u8.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_4tap_v_i16_to_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let round = _mm256_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let round128 = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 4];
        let mut ft16 = [_mm_setzero_si128(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..4 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s = _mm256_loadu_si256(s_ptr as *const __m256i);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(s_ptr as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_6tap_h_u8_to_i16`].  16 cols/iter, runtime shift.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_6tap_h_u8_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let round = _mm256_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let round128 = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 6];
        let mut ft16 = [zero128; 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..6 {
                    let s = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm256_cvtepu8_epi16(s);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s16));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s16));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                _mm_storeu_si128(
                    d_row.add(col) as *mut __m128i,
                    _mm_packs_epi32(
                        _mm256_castsi256_si128(r0),
                        _mm256_extracti128_si256::<1>(r0),
                    ),
                );
                _mm_storeu_si128(
                    d_row.add(col + 8) as *mut __m128i,
                    _mm_packs_epi32(
                        _mm256_castsi256_si128(r1),
                        _mm256_extracti128_si256::<1>(r1),
                    ),
                );
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..6 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero128);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, _mm_packs_epi32(r_lo, r_hi));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..6 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) =
                    ((acc + rscalar) >> shift).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_6tap_v_i16_to_u8`].  16 cols/iter, runtime shift.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_6tap_v_i16_to_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let round = _mm256_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let round128 = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 6];
        let mut ft16 = [_mm_setzero_si128(); 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
            ft16[i] = _mm_set1_epi16(*filter.add(i));
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let mut acc0 = _mm256_setzero_si256();
                let mut acc1 = _mm256_setzero_si256();
                for i in 0..6 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s = _mm256_loadu_si256(s_ptr as *const __m256i);
                    let lo32 = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s));
                    let hi32 = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s));
                    acc0 = _mm256_add_epi32(acc0, _mm256_mullo_epi32(lo32, ft[i]));
                    acc1 = _mm256_add_epi32(acc1, _mm256_mullo_epi32(hi32, ft[i]));
                }
                let r0 = _mm256_sra_epi32(_mm256_add_epi32(acc0, round), cnt);
                let r1 = _mm256_sra_epi32(_mm256_add_epi32(acc1, round), cnt);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..6 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(s_ptr as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft16[i]);
                    let phi = _mm_mulhi_epi16(s16, ft16[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round128), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round128), cnt);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..6 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::weighted_avg_u8`].  16 cols/iter, 8-col +
/// scalar tails.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn weighted_avg_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    dst: *mut u8,
    dst_stride: isize,
    w: usize,
    h: usize,
    w0: i16,
    w1: i16,
    offset: i32,
    shift: u32,
) {
    unsafe {
        let zero128 = _mm_setzero_si128();
        let w0v = _mm256_set1_epi32(w0 as i32);
        let w1v = _mm256_set1_epi32(w1 as i32);
        let offv = _mm256_set1_epi32(offset);
        let w0v16 = _mm_set1_epi16(w0);
        let w1v16 = _mm_set1_epi16(w1);
        let off128 = _mm_set1_epi32(offset);
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let (w0s, w1s) = (w0 as i32, w1 as i32);
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let a16 = _mm256_cvtepu8_epi16(_mm_loadu_si128(a_row.add(col) as *const __m128i));
                let b16 = _mm256_cvtepu8_epi16(_mm_loadu_si128(b_row.add(col) as *const __m128i));
                let a_lo = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(a16));
                let a_hi = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(a16));
                let b_lo = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(b16));
                let b_hi = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(b16));
                let acc0 = _mm256_add_epi32(
                    _mm256_add_epi32(_mm256_mullo_epi32(a_lo, w0v), _mm256_mullo_epi32(b_lo, w1v)),
                    offv,
                );
                let acc1 = _mm256_add_epi32(
                    _mm256_add_epi32(_mm256_mullo_epi32(a_hi, w0v), _mm256_mullo_epi32(b_hi, w1v)),
                    offv,
                );
                let r0 = _mm256_sra_epi32(acc0, cnt);
                let r1 = _mm256_sra_epi32(acc1, cnt);
                let p0 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r0),
                    _mm256_extracti128_si256::<1>(r0),
                );
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(p0, p0));
                let p1 = _mm_packs_epi32(
                    _mm256_castsi256_si128(r1),
                    _mm256_extracti128_si256::<1>(r1),
                );
                _mm_storel_epi64(d_row.add(col + 8) as *mut __m128i, _mm_packus_epi16(p1, p1));
                col += 16;
            }
            while col + 8 <= w {
                let a16 =
                    _mm_unpacklo_epi8(_mm_loadl_epi64(a_row.add(col) as *const __m128i), zero128);
                let b16 =
                    _mm_unpacklo_epi8(_mm_loadl_epi64(b_row.add(col) as *const __m128i), zero128);
                let pa_lo = _mm_mullo_epi16(a16, w0v16);
                let pa_hi = _mm_mulhi_epi16(a16, w0v16);
                let pb_lo = _mm_mullo_epi16(b16, w1v16);
                let pb_hi = _mm_mulhi_epi16(b16, w1v16);
                let lo = _mm_add_epi32(
                    _mm_add_epi32(
                        _mm_unpacklo_epi16(pa_lo, pa_hi),
                        _mm_unpacklo_epi16(pb_lo, pb_hi),
                    ),
                    off128,
                );
                let hi = _mm_add_epi32(
                    _mm_add_epi32(
                        _mm_unpackhi_epi16(pa_lo, pa_hi),
                        _mm_unpackhi_epi16(pb_lo, pb_hi),
                    ),
                    off128,
                );
                let i16s = _mm_packs_epi32(_mm_sra_epi32(lo, cnt), _mm_sra_epi32(hi, cnt));
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
                col += 8;
            }
            while col < w {
                let av = *a_row.add(col) as i32;
                let bv = *b_row.add(col) as i32;
                *d_row.add(col) = ((w0s * av + w1s * bv + offset) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_avg_u16`].  `_mm256_avg_epu16` — 16 u16/iter,
/// falling through to an SSE2 8-u16 chunk and scalar tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_avg_u16(
    a: *const u16,
    a_stride: isize,
    b: *const u16,
    b_stride: isize,
    dst: *mut u16,
    dst_stride: isize,
    w: usize,
    h: usize,
) {
    unsafe {
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let av = _mm256_loadu_si256(a_row.add(col) as *const __m256i);
                let bv = _mm256_loadu_si256(b_row.add(col) as *const __m256i);
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, _mm256_avg_epu16(av, bv));
                col += 16;
            }
            if col + 8 <= w {
                let av = _mm_loadu_si128(a_row.add(col) as *const __m128i);
                let bv = _mm_loadu_si128(b_row.add(col) as *const __m128i);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, _mm_avg_epu16(av, bv));
                col += 8;
            }
            while col < w {
                let av = *a_row.add(col) as u32;
                let bv = *b_row.add(col) as u32;
                *d_row.add(col) = ((av + bv + 1) >> 1) as u16;
                col += 1;
            }
        }
    }
}

// =========================================================================
// HEVC luma 8-tap MC, i32-source → raw i32 intermediate (v0.3.22).
//
// AVX2 has a native 32-bit multiply (`_mm256_mullo_epi32`), so the i32 source
// is multiplied directly (no source narrow): the low 32 bits are the exact
// product because every 8-tap accumulation provably fits i32.  There is NO
// rounding offset (`_mm256_sra_epi32` straight on the accumulator) and the
// result is stored as the RAW i32 lanes — there is NO output narrow and NO
// saturation (the `packs_epi32` that used to clamp the output is gone): the hv
// second pass can exceed i16 and the wrapping-`put` store / uni/bi `clip_pixel`
// are separate downstream steps.  8 cols/iter + scalar tail.
// =========================================================================

/// AVX2 impl of [`crate::mc_8tap_h_i32_to_i32`].  Horizontal 8-tap over i32
/// pixels, native `mullo_epi32`, no rounding offset, RAW i32 output.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_8tap_h_i32_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i32,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    unsafe {
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc = _mm256_setzero_si256();
                for i in 0..8 {
                    let s = _mm256_loadu_si256(s_row.add(col + i) as *const __m256i);
                    acc = _mm256_add_epi32(acc, _mm256_mullo_epi32(s, ft[i]));
                }
                let r = _mm256_sra_epi32(acc, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, r);
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *s_row.add(col + i) * f[i];
                }
                *d_row.add(col) = acc >> shift;
                col += 1;
            }
        }
    }
}

// =========================================================================
// HEVC CHROMA 4-tap MC, i32-source -> raw i32 intermediate (v0.3.23).
//
// The 4-tap sibling of the luma 8-tap i32 family above; identical AVX2 shape
// (native `mullo_epi32`, arithmetic `sra_epi32`, RAW i32 store — NO output
// narrow / saturation) with the tap loop reduced from 8 to 4.  8 cols/iter +
// scalar tail.
// =========================================================================

/// AVX2 impl of [`crate::mc_4tap_h_i32_to_i32`].  Horizontal 4-tap over i32
/// pixels, native `mullo_epi32`, no rounding offset, RAW i32 output.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_4tap_h_i32_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i32,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    unsafe {
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc = _mm256_setzero_si256();
                for i in 0..4 {
                    let s = _mm256_loadu_si256(s_row.add(col + i) as *const __m256i);
                    acc = _mm256_add_epi32(acc, _mm256_mullo_epi32(s, ft[i]));
                }
                let r = _mm256_sra_epi32(acc, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, r);
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *s_row.add(col + i) * f[i];
                }
                *d_row.add(col) = acc >> shift;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_4tap_v_i32_to_i32`].  Vertical 4-tap over i32
/// pixels; native `mullo_epi32`, no rounding offset, RAW i32 output.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_4tap_v_i32_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i32,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    unsafe {
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc = _mm256_setzero_si256();
                for i in 0..4 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let s = _mm256_loadu_si256(sp as *const __m256i);
                    acc = _mm256_add_epi32(acc, _mm256_mullo_epi32(s, ft[i]));
                }
                let r = _mm256_sra_epi32(acc, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, r);
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) * f[i];
                }
                *d_row.add(col) = acc >> shift;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_4tap_v_i16_to_i32`].  Vertical 4-tap over the i16
/// hv intermediate: widen i16→i32 (`cvtepi16_epi32`), native `mullo_epi32`,
/// no rounding offset, RAW i32 output (the second pass can exceed i16, so NO
/// output narrow / saturation).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_4tap_v_i16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    unsafe {
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc = _mm256_setzero_si256();
                for i in 0..4 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(sp as *const __m128i);
                    let s = _mm256_cvtepi16_epi32(s16);
                    acc = _mm256_add_epi32(acc, _mm256_mullo_epi32(s, ft[i]));
                }
                let r = _mm256_sra_epi32(acc, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, r);
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = acc >> shift;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_8tap_v_i32_to_i32`].  Vertical 8-tap over i32
/// pixels; native `mullo_epi32`, no rounding offset, RAW i32 output.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_8tap_v_i32_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i32,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    unsafe {
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc = _mm256_setzero_si256();
                for i in 0..8 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let s = _mm256_loadu_si256(sp as *const __m256i);
                    acc = _mm256_add_epi32(acc, _mm256_mullo_epi32(s, ft[i]));
                }
                let r = _mm256_sra_epi32(acc, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, r);
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) * f[i];
                }
                *d_row.add(col) = acc >> shift;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_8tap_v_i16_to_i32`].  Vertical 8-tap over the i16
/// hv intermediate: widen i16→i32 (`cvtepi16_epi32`), native `mullo_epi32`,
/// no rounding offset, RAW i32 output (the second pass can exceed i16, so NO
/// output narrow / saturation).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_8tap_v_i16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    unsafe {
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc = _mm256_setzero_si256();
                for i in 0..8 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(sp as *const __m128i);
                    let s = _mm256_cvtepi16_epi32(s16);
                    acc = _mm256_add_epi32(acc, _mm256_mullo_epi32(s, ft[i]));
                }
                let r = _mm256_sra_epi32(acc, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, r);
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = acc >> shift;
                col += 1;
            }
        }
    }
}

// =========================================================================
// HEVC MC, u16-SOURCE -> raw i32 intermediate (v0.3.27).
//
// The u16 siblings of the i32-source tap filters above.  A u16 pixel
// (`0..(1<<bit_depth)` ⊆ i16) is loaded 8-at-a-time in a single 128-bit load
// (half the bytes of the i32 source = the 2x-lane win) and zero-extended to i32
// with `_mm256_cvtepu16_epi32` — the exact mirror of the i16-htmp v-pass's
// `_mm256_cvtepi16_epi32`, differing only in zero- vs sign-extend (identical
// for the always-positive pixel values) — then fed to the SAME native
// `mullo_epi32` MAC and arithmetic `sra_epi32`.  Byte-identical to the
// i32-source twin given the same pixel values.  8 cols/iter + scalar tail.
// =========================================================================

/// AVX2 impl of [`crate::mc_8tap_h_u16_to_i32`].  Horizontal 8-tap over u16
/// pixels: widen u16→i32 (`cvtepu16_epi32`), native `mullo_epi32`, no rounding
/// offset, RAW i32 output.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_8tap_h_u16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const u16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    unsafe {
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc = _mm256_setzero_si256();
                for i in 0..8 {
                    let s16 = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let s = _mm256_cvtepu16_epi32(s16);
                    acc = _mm256_add_epi32(acc, _mm256_mullo_epi32(s, ft[i]));
                }
                let r = _mm256_sra_epi32(acc, cnt);
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, r);
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) = acc >> shift;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_8tap_v_u16_to_i32`].  Vertical 8-tap over u16 pixels:
/// widen u16→i32 (`cvtepu16_epi32`), native `mullo_epi32`, RAW i32 output.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_8tap_v_u16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const u16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    unsafe {
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc = _mm256_setzero_si256();
                for i in 0..8 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(sp as *const __m128i);
                    let s = _mm256_cvtepu16_epi32(s16);
                    acc = _mm256_add_epi32(acc, _mm256_mullo_epi32(s, ft[i]));
                }
                let r = _mm256_sra_epi32(acc, cnt);
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, r);
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = acc >> shift;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_4tap_h_u16_to_i32`].  Horizontal 4-tap chroma over
/// u16 pixels: widen u16→i32 (`cvtepu16_epi32`), native `mullo_epi32`, RAW i32.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_4tap_h_u16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const u16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    unsafe {
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc = _mm256_setzero_si256();
                for i in 0..4 {
                    let s16 = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let s = _mm256_cvtepu16_epi32(s16);
                    acc = _mm256_add_epi32(acc, _mm256_mullo_epi32(s, ft[i]));
                }
                let r = _mm256_sra_epi32(acc, cnt);
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, r);
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) = acc >> shift;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_4tap_v_u16_to_i32`].  Vertical 4-tap chroma over u16
/// pixels: widen u16→i32 (`cvtepu16_epi32`), native `mullo_epi32`, RAW i32.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_4tap_v_u16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const u16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    unsafe {
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm256_setzero_si256(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            f[i] = *filter.add(i) as i32;
            ft[i] = _mm256_set1_epi32(f[i]);
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc = _mm256_setzero_si256();
                for i in 0..4 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(sp as *const __m128i);
                    let s = _mm256_cvtepu16_epi32(s16);
                    acc = _mm256_add_epi32(acc, _mm256_mullo_epi32(s, ft[i]));
                }
                let r = _mm256_sra_epi32(acc, cnt);
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, r);
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = acc >> shift;
                col += 1;
            }
        }
    }
}

// =========================================================================
// HEVC MC "final" clip/combine kernels (v0.3.26): i32 intermediate -> u16
// pixel / wrapping i16.  Same 16-lane i32->u16 clamp+narrow idiom as
// `prims_avx2::add_clip_to_pixel_u16`: `max_epi32(0)` + `min_epi32(max)` clamp
// in i32, then `packus_epi32` + `permute4x64::<0b1101_1000>` to narrow to 16
// sequential u16.  `v` is loaded as two 256-bit halves (cols 0..7 / 8..15) so
// the lo/hi ordering matches the `src2`/pack layout.  Weights use
// `mullo_epi32`, `src2` is sign-widened with `cvtepi16_epi32`.  16 lanes/iter +
// scalar tail (byte-identical to `prims_scalar`).
// =========================================================================

/// AVX2 impl of [`crate::mc_uni_clip_i32_to_u16`].
/// `dst = clip((v + offset) >> shift)`.  16 lanes/iter + scalar tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_uni_clip_i32_to_u16(
    dst: *mut u16,
    dst_stride: isize,
    v: *const i32,
    v_stride: isize,
    w: usize,
    h: usize,
    offset: i32,
    shift: u32,
    max: u16,
) {
    unsafe {
        let offv = _mm256_set1_epi32(offset);
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let zero = _mm256_setzero_si256();
        let maxv = _mm256_set1_epi32(max as i32);
        let maxi = max as i32;
        for row in 0..h {
            let v_row = v.offset(row as isize * v_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let lo = _mm256_loadu_si256(v_row.add(col) as *const __m256i);
                let hi = _mm256_loadu_si256(v_row.add(col + 8) as *const __m256i);
                let s_lo = _mm256_sra_epi32(_mm256_add_epi32(lo, offv), cnt);
                let s_hi = _mm256_sra_epi32(_mm256_add_epi32(hi, offv), cnt);
                let s_lo = _mm256_min_epi32(_mm256_max_epi32(s_lo, zero), maxv);
                let s_hi = _mm256_min_epi32(_mm256_max_epi32(s_hi, zero), maxv);
                let packed = _mm256_packus_epi32(s_lo, s_hi);
                let fixed = _mm256_permute4x64_epi64::<0b1101_1000>(packed);
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, fixed);
                col += 16;
            }
            while col < w {
                let x = *v_row.add(col);
                *d_row.add(col) = ((x + offset) >> shift).clamp(0, maxi) as u16;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_uni_w_clip_i32_to_u16`].
/// `dst = clip(((v * wx + offset) >> shift) + ox)`.  16 lanes/iter + tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn mc_uni_w_clip_i32_to_u16(
    dst: *mut u16,
    dst_stride: isize,
    v: *const i32,
    v_stride: isize,
    w: usize,
    h: usize,
    wx: i32,
    offset: i32,
    shift: u32,
    ox: i32,
    max: u16,
) {
    unsafe {
        let wxv = _mm256_set1_epi32(wx);
        let offv = _mm256_set1_epi32(offset);
        let oxv = _mm256_set1_epi32(ox);
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let zero = _mm256_setzero_si256();
        let maxv = _mm256_set1_epi32(max as i32);
        let maxi = max as i32;
        for row in 0..h {
            let v_row = v.offset(row as isize * v_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let lo = _mm256_loadu_si256(v_row.add(col) as *const __m256i);
                let hi = _mm256_loadu_si256(v_row.add(col + 8) as *const __m256i);
                let s_lo = _mm256_add_epi32(
                    _mm256_sra_epi32(_mm256_add_epi32(_mm256_mullo_epi32(lo, wxv), offv), cnt),
                    oxv,
                );
                let s_hi = _mm256_add_epi32(
                    _mm256_sra_epi32(_mm256_add_epi32(_mm256_mullo_epi32(hi, wxv), offv), cnt),
                    oxv,
                );
                let s_lo = _mm256_min_epi32(_mm256_max_epi32(s_lo, zero), maxv);
                let s_hi = _mm256_min_epi32(_mm256_max_epi32(s_hi, zero), maxv);
                let packed = _mm256_packus_epi32(s_lo, s_hi);
                let fixed = _mm256_permute4x64_epi64::<0b1101_1000>(packed);
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, fixed);
                col += 16;
            }
            while col < w {
                let x = *v_row.add(col);
                *d_row.add(col) = (((x * wx + offset) >> shift) + ox).clamp(0, maxi) as u16;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_bi_clip_i32_to_u16`].
/// `dst = clip((v + src2 + offset) >> shift)`, `src2` sign-widened i16 -> i32.
/// 16 lanes/iter + scalar tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn mc_bi_clip_i32_to_u16(
    dst: *mut u16,
    dst_stride: isize,
    v: *const i32,
    v_stride: isize,
    src2: *const i16,
    src2_stride: isize,
    w: usize,
    h: usize,
    offset: i32,
    shift: u32,
    max: u16,
) {
    unsafe {
        let offv = _mm256_set1_epi32(offset);
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let zero = _mm256_setzero_si256();
        let maxv = _mm256_set1_epi32(max as i32);
        let maxi = max as i32;
        for row in 0..h {
            let v_row = v.offset(row as isize * v_stride);
            let s2_row = src2.offset(row as isize * src2_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let lo = _mm256_loadu_si256(v_row.add(col) as *const __m256i);
                let hi = _mm256_loadu_si256(v_row.add(col + 8) as *const __m256i);
                let s2v = _mm256_loadu_si256(s2_row.add(col) as *const __m256i);
                let s2_lo = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s2v));
                let s2_hi = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s2v));
                let s_lo =
                    _mm256_sra_epi32(_mm256_add_epi32(_mm256_add_epi32(lo, s2_lo), offv), cnt);
                let s_hi =
                    _mm256_sra_epi32(_mm256_add_epi32(_mm256_add_epi32(hi, s2_hi), offv), cnt);
                let s_lo = _mm256_min_epi32(_mm256_max_epi32(s_lo, zero), maxv);
                let s_hi = _mm256_min_epi32(_mm256_max_epi32(s_hi, zero), maxv);
                let packed = _mm256_packus_epi32(s_lo, s_hi);
                let fixed = _mm256_permute4x64_epi64::<0b1101_1000>(packed);
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, fixed);
                col += 16;
            }
            while col < w {
                let x = *v_row.add(col);
                let s2 = *s2_row.add(col) as i32;
                *d_row.add(col) = ((x + s2 + offset) >> shift).clamp(0, maxi) as u16;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_bi_w_clip_i32_to_u16`].
/// `dst = clip((v * wx1 + src2 * wx0 + bias) >> rshift)`.  16 lanes/iter + tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn mc_bi_w_clip_i32_to_u16(
    dst: *mut u16,
    dst_stride: isize,
    v: *const i32,
    v_stride: isize,
    src2: *const i16,
    src2_stride: isize,
    w: usize,
    h: usize,
    wx0: i32,
    wx1: i32,
    bias: i32,
    rshift: u32,
    max: u16,
) {
    unsafe {
        let wx0v = _mm256_set1_epi32(wx0);
        let wx1v = _mm256_set1_epi32(wx1);
        let biasv = _mm256_set1_epi32(bias);
        let cnt = _mm_cvtsi32_si128(rshift as i32);
        let zero = _mm256_setzero_si256();
        let maxv = _mm256_set1_epi32(max as i32);
        let maxi = max as i32;
        for row in 0..h {
            let v_row = v.offset(row as isize * v_stride);
            let s2_row = src2.offset(row as isize * src2_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let lo = _mm256_loadu_si256(v_row.add(col) as *const __m256i);
                let hi = _mm256_loadu_si256(v_row.add(col + 8) as *const __m256i);
                let s2v = _mm256_loadu_si256(s2_row.add(col) as *const __m256i);
                let s2_lo = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(s2v));
                let s2_hi = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(s2v));
                let s_lo = _mm256_sra_epi32(
                    _mm256_add_epi32(
                        _mm256_add_epi32(
                            _mm256_mullo_epi32(lo, wx1v),
                            _mm256_mullo_epi32(s2_lo, wx0v),
                        ),
                        biasv,
                    ),
                    cnt,
                );
                let s_hi = _mm256_sra_epi32(
                    _mm256_add_epi32(
                        _mm256_add_epi32(
                            _mm256_mullo_epi32(hi, wx1v),
                            _mm256_mullo_epi32(s2_hi, wx0v),
                        ),
                        biasv,
                    ),
                    cnt,
                );
                let s_lo = _mm256_min_epi32(_mm256_max_epi32(s_lo, zero), maxv);
                let s_hi = _mm256_min_epi32(_mm256_max_epi32(s_hi, zero), maxv);
                let packed = _mm256_packus_epi32(s_lo, s_hi);
                let fixed = _mm256_permute4x64_epi64::<0b1101_1000>(packed);
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, fixed);
                col += 16;
            }
            while col < w {
                let x = *v_row.add(col);
                let s2 = *s2_row.add(col) as i32;
                *d_row.add(col) = ((x * wx1 + s2 * wx0 + bias) >> rshift).clamp(0, maxi) as u16;
                col += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::mc_store_i32_to_i16`].  Wrapping i32 -> i16 narrow
/// (`x as i16`): mask to low 16 bits then `packus_epi32` +
/// `permute4x64::<0b1101_1000>`.  16 lanes/iter + scalar tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn mc_store_i32_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    v: *const i32,
    v_stride: isize,
    w: usize,
    h: usize,
) {
    unsafe {
        let m16 = _mm256_set1_epi32(0xFFFF);
        for row in 0..h {
            let v_row = v.offset(row as isize * v_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let lo =
                    _mm256_and_si256(_mm256_loadu_si256(v_row.add(col) as *const __m256i), m16);
                let hi = _mm256_and_si256(
                    _mm256_loadu_si256(v_row.add(col + 8) as *const __m256i),
                    m16,
                );
                let packed = _mm256_packus_epi32(lo, hi);
                let fixed = _mm256_permute4x64_epi64::<0b1101_1000>(packed);
                _mm256_storeu_si256(d_row.add(col) as *mut __m256i, fixed);
                col += 16;
            }
            while col < w {
                *d_row.add(col) = *v_row.add(col) as i16;
                col += 1;
            }
        }
    }
}
