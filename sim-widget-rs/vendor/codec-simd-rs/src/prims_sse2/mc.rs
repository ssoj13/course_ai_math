#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// SSE2 impl of [`crate::mc_avg_u8`].  `_mm_avg_epu8` — single
/// instruction does `(a + b + 1) >> 1` lane-wise.  16 bytes/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
            while col + 16 <= w {
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

/// SSE2 impl of [`crate::mc_8tap_h_u8`].  Horizontal 8-tap convolution,
/// 8 output columns per iteration.
///
/// For each tap the 8 source bytes widen u8→i16; the full i32 product
/// with the broadcast tap is formed by the signed widening multiply
/// (`mullo` + `mulhi` + `unpack`) and accumulated into two i32x4 lanes
/// (cols 0-3 / 4-7).  After 8 taps: `(acc + 64) >> 7`, signed-narrow to
/// i16, then unsigned-narrow to u8 (= clamp to [0,255]).  Tail scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let round = _mm_set1_epi32(64);
        let mut ft = [zero; 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_srai_epi32(_mm_add_epi32(acc_lo, round), 7);
                let r_hi = _mm_srai_epi32(_mm_add_epi32(acc_hi, round), 7);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                let u8s = _mm_packus_epi16(i16s, i16s);
                _mm_storel_epi64(d_row.add(col) as *mut __m128i, u8s);
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

/// SSE2 impl of [`crate::mc_8tap_v_u8`].  Vertical 8-tap convolution,
/// 8 output columns per iteration.
///
/// Identical math to the horizontal kernel, but each tap loads a whole
/// source row (`src[(row+i)*src_stride + col ..]`) rather than a shifted
/// slice — so no per-row shuffles, just 8 row loads multiplied by the
/// broadcast taps.  Same full-width signed multiply + round/pack/clamp.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let round = _mm_set1_epi32(64);
        let mut ft = [zero; 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s8 = _mm_loadl_epi64(s_ptr as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_srai_epi32(_mm_add_epi32(acc_lo, round), 7);
                let r_hi = _mm_srai_epi32(_mm_add_epi32(acc_hi, round), 7);
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

/// SSE2 impl of [`crate::mc_4tap_h_u8`].  Horizontal 4-tap, 8 cols/iter,
/// runtime `shift` (manual round bias + `_mm_sra_epi32` with a register
/// shift count).  Same full-width signed multiply as the 8-tap kernel.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let round = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [zero; 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
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

/// SSE2 impl of [`crate::mc_4tap_v_u8`].  Vertical 4-tap, 8 cols/iter,
/// runtime `shift`.  Each tap loads a whole source row.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let round = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [zero; 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s8 = _mm_loadl_epi64(s_ptr as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
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

/// SSE2 impl of [`crate::mc_6tap_h_u8`].  Horizontal 6-tap, 8 cols/iter,
/// runtime `shift`.  Same shape as the 4-tap kernel with a 6-tap loop.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let round = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [zero; 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..6 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
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

/// SSE2 impl of [`crate::mc_6tap_v_u8`].  Vertical 6-tap, 8 cols/iter,
/// runtime `shift`.  Each tap loads a whole source row.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let round = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [zero; 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..6 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s8 = _mm_loadl_epi64(s_ptr as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
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

/// SSE2 impl of [`crate::mc_bilinear_h_u8`].  Horizontal 2-tap, 8
/// cols/iter, runtime `shift`.  Same shape as the 4/6-tap kernels.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let round = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let ft = [_mm_set1_epi16(*filter), _mm_set1_epi16(*filter.add(1))];
        let f = [*filter as i32, *filter.add(1) as i32];
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..2 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
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

/// SSE2 impl of [`crate::mc_bilinear_v_u8`].  Vertical 2-tap, 8
/// cols/iter, runtime `shift`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let round = _mm_set1_epi32(1 << (shift - 1));
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let ft = [_mm_set1_epi16(*filter), _mm_set1_epi16(*filter.add(1))];
        let f = [*filter as i32, *filter.add(1) as i32];
        let rscalar = 1i32 << (shift - 1);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..2 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s8 = _mm_loadl_epi64(s_ptr as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
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

/// SSE2 impl of [`crate::mc_8tap_h_u8_to_i16`].  i16-output horizontal
/// 8-tap, 8 cols/iter, runtime `shift` (0..=14).  Full-width signed
/// multiply, round bias + `_mm_sra_epi32`, then `packs_epi32`
/// (saturating i32→i16) — NO clip to pixel range.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let round = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [zero; 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, i16s);
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

/// SSE2 impl of [`crate::mc_8tap_v_i16_to_u8`].  Vertical 8-tap over an
/// i16 intermediate, 8 cols/iter, runtime `shift`, clipped to u8.  Loads
/// i16 rows directly (no u8 widen); same signed widening multiply.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let round = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm_setzero_si128(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(s_ptr as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
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

/// SSE2 impl of [`crate::mc_4tap_h_u8_to_i16`].  i16-output horizontal
/// 4-tap, 8 cols/iter, runtime `shift`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let round = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [zero; 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
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

/// SSE2 impl of [`crate::mc_4tap_v_i16_to_u8`].  i16-input vertical
/// 4-tap, 8 cols/iter, runtime `shift`, clipped to u8.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let round = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm_setzero_si128(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(s_ptr as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
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

/// SSE2 impl of [`crate::mc_6tap_h_u8_to_i16`].  8 cols/iter, runtime shift.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let round = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [zero; 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..6 {
                    let s8 = _mm_loadl_epi64(s_row.add(col + i) as *const __m128i);
                    let s16 = _mm_unpacklo_epi8(s8, zero);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
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

/// SSE2 impl of [`crate::mc_6tap_v_i16_to_u8`].  8 cols/iter, runtime shift.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let round = _mm_set1_epi32(if shift == 0 { 0 } else { 1 << (shift - 1) });
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let mut ft = [_mm_setzero_si128(); 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = _mm_set1_epi16(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..6 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(s_ptr as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt);
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

/// SSE2 impl of [`crate::weighted_avg_u8`].  8 cols/iter; widen u8→i16,
/// signed widening multiply by each weight, sum + offset, round-shift,
/// clip to u8.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let zero = _mm_setzero_si128();
        let w0v = _mm_set1_epi16(w0);
        let w1v = _mm_set1_epi16(w1);
        let offv = _mm_set1_epi32(offset);
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let (w0s, w1s) = (w0 as i32, w1 as i32);
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let a16 =
                    _mm_unpacklo_epi8(_mm_loadl_epi64(a_row.add(col) as *const __m128i), zero);
                let b16 =
                    _mm_unpacklo_epi8(_mm_loadl_epi64(b_row.add(col) as *const __m128i), zero);
                let pa_lo = _mm_mullo_epi16(a16, w0v);
                let pa_hi = _mm_mulhi_epi16(a16, w0v);
                let pb_lo = _mm_mullo_epi16(b16, w1v);
                let pb_hi = _mm_mulhi_epi16(b16, w1v);
                let lo = _mm_add_epi32(
                    _mm_add_epi32(
                        _mm_unpacklo_epi16(pa_lo, pa_hi),
                        _mm_unpacklo_epi16(pb_lo, pb_hi),
                    ),
                    offv,
                );
                let hi = _mm_add_epi32(
                    _mm_add_epi32(
                        _mm_unpackhi_epi16(pa_lo, pa_hi),
                        _mm_unpackhi_epi16(pb_lo, pb_hi),
                    ),
                    offv,
                );
                let r_lo = _mm_sra_epi32(lo, cnt);
                let r_hi = _mm_sra_epi32(hi, cnt);
                let i16s = _mm_packs_epi32(r_lo, r_hi);
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

/// SSE2 impl of [`crate::mc_avg_u16`].  `_mm_avg_epu16` — single
/// instruction `(a + b + 1) >> 1` lane-wise.  8 u16/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
            while col + 8 <= w {
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
// SOURCE-NARROW CONTRACT: the `_i32_to_i32` inputs are pixels in
// `0..(1<<bit_depth)` (≤ 4095 for 12-bit), which fit i16, so each tap window is
// narrowed i32→i16 with `_mm_packs_epi32` and fed through the same proven
// signed widening multiply (`mullo`+`mulhi`+`unpack`) used by the u8→i16
// kernels.  This source narrow is LOSSLESS ONLY because the pixel source is in
// i16 range — callers must uphold `src ∈ 0..(1<<bit_depth) ⊆ i16`.
//
// OUTPUT: the accumulator is arithmetic-shifted (`_mm_sra_epi32`, NO rounding
// offset) and stored as the RAW i32 result — there is NO output narrow and NO
// saturation (the `packs_epi32` that used to clamp the output is gone): the hv
// second pass can exceed i16 and both the wrapping-`put` store and the uni/bi
// `clip_pixel` are separate downstream steps.  8 cols/iter + scalar tail.
// =========================================================================

/// SSE2 impl of [`crate::mc_8tap_h_i32_to_i32`].  Horizontal 8-tap over i32
/// pixels, no rounding offset, RAW i32 output (no narrow/saturation).
/// Source-narrow contract: `src ∈ 0..(1<<bit_depth) ⊆ i16` (see module header).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let mut ft = [_mm_setzero_si128(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            let c = *filter.add(i) as i16;
            ft[i] = _mm_set1_epi16(c);
            f[i] = c as i32;
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let a = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let b = _mm_loadu_si128(s_row.add(col + i + 4) as *const __m128i);
                    let s16 = _mm_packs_epi32(a, b);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(acc_lo, cnt);
                let r_hi = _mm_sra_epi32(acc_hi, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, r_lo);
                _mm_storeu_si128(d_row.add(col + 4) as *mut __m128i, r_hi);
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
// The 4-tap sibling of the luma 8-tap i32 family above; identical SSE2 shape
// (source narrow i32->i16 via `packs_epi32`, signed widening multiply
// `mullo`+`mulhi`+`unpack`, arithmetic `sra_epi32`, RAW i32 store — NO output
// narrow / saturation) with the tap loop reduced from 8 to 4.  Source-narrow
// contract: `src ∈ 0..(1<<bit_depth) ⊆ i16`.  8 cols/iter + scalar tail.
// =========================================================================

/// SSE2 impl of [`crate::mc_4tap_h_i32_to_i32`].  Horizontal 4-tap over i32
/// pixels, no rounding offset, RAW i32 output (no narrow/saturation).
/// Source-narrow contract: `src ∈ 0..(1<<bit_depth) ⊆ i16` (see module header).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let mut ft = [_mm_setzero_si128(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            let c = *filter.add(i) as i16;
            ft[i] = _mm_set1_epi16(c);
            f[i] = c as i32;
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let a = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let b = _mm_loadu_si128(s_row.add(col + i + 4) as *const __m128i);
                    let s16 = _mm_packs_epi32(a, b);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(acc_lo, cnt);
                let r_hi = _mm_sra_epi32(acc_hi, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, r_lo);
                _mm_storeu_si128(d_row.add(col + 4) as *mut __m128i, r_hi);
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

/// SSE2 impl of [`crate::mc_4tap_v_i32_to_i32`].  Vertical 4-tap over i32
/// pixels; same source narrow + signed multiply, no rounding offset, RAW i32
/// output.  Source-narrow contract: `src ∈ 0..(1<<bit_depth) ⊆ i16`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let mut ft = [_mm_setzero_si128(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            let c = *filter.add(i) as i16;
            ft[i] = _mm_set1_epi16(c);
            f[i] = c as i32;
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let a = _mm_loadu_si128(sp as *const __m128i);
                    let b = _mm_loadu_si128(sp.add(4) as *const __m128i);
                    let s16 = _mm_packs_epi32(a, b);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(acc_lo, cnt);
                let r_hi = _mm_sra_epi32(acc_hi, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, r_lo);
                _mm_storeu_si128(d_row.add(col + 4) as *mut __m128i, r_hi);
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

/// SSE2 impl of [`crate::mc_4tap_v_i16_to_i32`].  Vertical 4-tap over the i16
/// hv horizontal-pass intermediate; i16 loaded directly (no source narrow —
/// the htmp is already i16), no rounding offset, RAW i32 output (the second
/// pass can exceed i16, so NO output narrow / saturation).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let mut ft = [_mm_setzero_si128(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            let c = *filter.add(i) as i16;
            ft[i] = _mm_set1_epi16(c);
            f[i] = c as i32;
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(sp as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(acc_lo, cnt);
                let r_hi = _mm_sra_epi32(acc_hi, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, r_lo);
                _mm_storeu_si128(d_row.add(col + 4) as *mut __m128i, r_hi);
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

/// SSE2 impl of [`crate::mc_8tap_v_i32_to_i32`].  Vertical 8-tap over i32
/// pixels; same source narrow + signed multiply, no rounding offset, RAW i32
/// output.  Source-narrow contract: `src ∈ 0..(1<<bit_depth) ⊆ i16`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let mut ft = [_mm_setzero_si128(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            let c = *filter.add(i) as i16;
            ft[i] = _mm_set1_epi16(c);
            f[i] = c as i32;
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let a = _mm_loadu_si128(sp as *const __m128i);
                    let b = _mm_loadu_si128(sp.add(4) as *const __m128i);
                    let s16 = _mm_packs_epi32(a, b);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(acc_lo, cnt);
                let r_hi = _mm_sra_epi32(acc_hi, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, r_lo);
                _mm_storeu_si128(d_row.add(col + 4) as *mut __m128i, r_hi);
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

/// SSE2 impl of [`crate::mc_8tap_v_i16_to_i32`].  Vertical 8-tap over the i16
/// hv horizontal-pass intermediate; i16 loaded directly (no source narrow —
/// the htmp is already i16), no rounding offset, RAW i32 output (the second
/// pass can exceed i16, so NO output narrow / saturation).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let mut ft = [_mm_setzero_si128(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            let c = *filter.add(i) as i16;
            ft[i] = _mm_set1_epi16(c);
            f[i] = c as i32;
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(sp as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(acc_lo, cnt);
                let r_hi = _mm_sra_epi32(acc_hi, cnt);
                // Store the raw i32 lanes directly (no output narrow / pack).
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, r_lo);
                _mm_storeu_si128(d_row.add(col + 4) as *mut __m128i, r_hi);
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
// The u16 siblings of the i32-source tap filters above.  The u16 pixel
// (`0..(1<<bit_depth)` ⊆ i16) is already 16-bit, so — unlike the i32 source,
// which loads two 128-bit vectors and narrows via `packs_epi32` — the u16 source
// is loaded DIRECTLY as 8 i16 lanes in a single `_mm_loadu_si128` (half the
// load bandwidth = the 2x-lane win) and fed to the SAME signed widening
// multiply (`mullo_epi16`+`mulhi_epi16`+`unpack`) and arithmetic `sra_epi32`
// used by the i32/i16 kernels.  Reinterpreting a u16 pixel `≤ 4095` as i16 is
// bit-identical (it is positive), so this is byte-identical to the i32-source
// twin.  The vertical passes are the exact shape of the i16-htmp v-passes (same
// direct load) with a u16 pointer.  8 cols/iter + scalar tail.  Source contract:
// `src ∈ 0..(1<<bit_depth) ⊆ i16`.
// =========================================================================

/// SSE2 impl of [`crate::mc_8tap_h_u16_to_i32`].  Horizontal 8-tap over u16
/// pixels loaded directly as i16 (no narrow), no rounding offset, RAW i32 output.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let mut ft = [_mm_setzero_si128(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            let c = *filter.add(i) as i16;
            ft[i] = _mm_set1_epi16(c);
            f[i] = c as i32;
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    // 8 consecutive u16 samples [col+i, col+i+8) as i16 lanes.
                    let s16 = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(acc_lo, cnt);
                let r_hi = _mm_sra_epi32(acc_hi, cnt);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, r_lo);
                _mm_storeu_si128(d_row.add(col + 4) as *mut __m128i, r_hi);
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

/// SSE2 impl of [`crate::mc_8tap_v_u16_to_i32`].  Vertical 8-tap over u16 pixels
/// loaded directly as i16; no rounding offset, RAW i32 output.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let mut ft = [_mm_setzero_si128(); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            let c = *filter.add(i) as i16;
            ft[i] = _mm_set1_epi16(c);
            f[i] = c as i32;
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..8 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(sp as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(acc_lo, cnt);
                let r_hi = _mm_sra_epi32(acc_hi, cnt);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, r_lo);
                _mm_storeu_si128(d_row.add(col + 4) as *mut __m128i, r_hi);
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

/// SSE2 impl of [`crate::mc_4tap_h_u16_to_i32`].  Horizontal 4-tap chroma over
/// u16 pixels loaded directly as i16; no rounding offset, RAW i32 output.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let mut ft = [_mm_setzero_si128(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            let c = *filter.add(i) as i16;
            ft[i] = _mm_set1_epi16(c);
            f[i] = c as i32;
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let s16 = _mm_loadu_si128(s_row.add(col + i) as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(acc_lo, cnt);
                let r_hi = _mm_sra_epi32(acc_hi, cnt);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, r_lo);
                _mm_storeu_si128(d_row.add(col + 4) as *mut __m128i, r_hi);
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

/// SSE2 impl of [`crate::mc_4tap_v_u16_to_i32`].  Vertical 4-tap chroma over u16
/// pixels loaded directly as i16; no rounding offset, RAW i32 output.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let mut ft = [_mm_setzero_si128(); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            let c = *filter.add(i) as i16;
            ft[i] = _mm_set1_epi16(c);
            f[i] = c as i32;
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = _mm_setzero_si128();
                let mut acc_hi = _mm_setzero_si128();
                for i in 0..4 {
                    let sp = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = _mm_loadu_si128(sp as *const __m128i);
                    let plo = _mm_mullo_epi16(s16, ft[i]);
                    let phi = _mm_mulhi_epi16(s16, ft[i]);
                    acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo, phi));
                    acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo, phi));
                }
                let r_lo = _mm_sra_epi32(acc_lo, cnt);
                let r_hi = _mm_sra_epi32(acc_hi, cnt);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, r_lo);
                _mm_storeu_si128(d_row.add(col + 4) as *mut __m128i, r_hi);
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
