//! AVX2 implementation of the HEVC **SAO edge** filter primitive on the `u16`
//! picture plane.  Same `i16`-lane strategy as the SSE2 backend (see
//! `prims_sse2/sao.rs`), widened to 16 pixels per 256-bit step.  (The SAO
//! **band** filter's 32-entry per-pixel LUT gather is left to scalar; it is
//! not installed here — see `dispatch.rs`.)
//!
//! HEVC samples are `≤ 12-bit` (`≤ 4095`), so the sign `CMP(p,n) = (p>n) -
//! (p<n)` uses signed `cmpgt_epi16`, the category `2 + sign0 + sign1 ∈ [0,4]`
//! is two adds, the LUT `offsets[edge_idx[cat]]` is 4 `cmpeq`+select blends,
//! and `clip(p+off, 0, max)` is `add`/`max_epi16(0)`/`min_epi16(max)`.
//! Byte-exact vs the scalar reference; interior boundary/tail handled scalar.

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

use crate::prims_scalar::{EDGE_IDX, POS, cmp};

/// AVX2 backend for [`crate::sao_edge_u16`].  See the module header.
///
/// # Safety
/// `src`/`dst` valid for the `w × h` region at the given strides; `offsets`
/// valid for 5 `i16` reads.  Byte-exactness requires `sample + offset ∈ i16`
/// (`max + max|offset| ≤ i16::MAX`); HEVC (`bit_depth ≤ 12`, small scaled
/// offsets) always satisfies this.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn sao_edge_u16(
    dst: *mut u16,
    dst_stride: isize,
    src: *const u16,
    src_stride: isize,
    w: usize,
    h: usize,
    eo_class: u32,
    offsets: *const i16,
    max: u16,
) {
    unsafe {
        let maxi = max as i32;
        let eo = (eo_class as usize).min(3);
        let (dx0, dy0) = POS[eo][0];
        let (dx1, dy1) = POS[eo][1];
        let has_dx = (dx0 != 0) as usize;
        let has_dy = (dy0 != 0) as usize;

        let lut = [
            *offsets.add(EDGE_IDX[0]),
            *offsets.add(EDGE_IDX[1]),
            *offsets.add(EDGE_IDX[2]),
            *offsets.add(EDGE_IDX[3]),
            *offsets.add(EDGE_IDX[4]),
        ];

        let two = _mm256_set1_epi16(2);
        let zero = _mm256_setzero_si256();
        let vmax = _mm256_set1_epi16(maxi as i16);
        let vlut0 = _mm256_set1_epi16(lut[0]);
        let vlut1 = _mm256_set1_epi16(lut[1]);
        let vlut2 = _mm256_set1_epi16(lut[2]);
        let vlut3 = _mm256_set1_epi16(lut[3]);
        let vlut4 = _mm256_set1_epi16(lut[4]);

        let cx0 = has_dx;
        let cx_end = w - has_dx;

        for y in 0..h {
            let s_row = src.offset(y as isize * src_stride);
            let d_row = dst.offset(y as isize * dst_stride);

            if y < has_dy || y >= h - has_dy {
                for x in 0..w {
                    *d_row.add(x) = *s_row.add(x);
                }
                continue;
            }

            for x in 0..cx0 {
                *d_row.add(x) = *s_row.add(x);
            }

            let n0_row = src.offset((y as isize + dy0 as isize) * src_stride);
            let n1_row = src.offset((y as isize + dy1 as isize) * src_stride);

            let mut x = cx0;
            while x + 16 <= cx_end {
                let s = _mm256_loadu_si256(s_row.add(x) as *const __m256i);
                let n0 =
                    _mm256_loadu_si256(n0_row.offset(x as isize + dx0 as isize) as *const __m256i);
                let n1 =
                    _mm256_loadu_si256(n1_row.offset(x as isize + dx1 as isize) as *const __m256i);

                let sign0 = _mm256_sub_epi16(_mm256_cmpgt_epi16(n0, s), _mm256_cmpgt_epi16(s, n0));
                let sign1 = _mm256_sub_epi16(_mm256_cmpgt_epi16(n1, s), _mm256_cmpgt_epi16(s, n1));
                let cat = _mm256_add_epi16(_mm256_add_epi16(two, sign0), sign1);

                let mut offv = vlut0;
                let m1 = _mm256_cmpeq_epi16(cat, _mm256_set1_epi16(1));
                offv = _mm256_or_si256(_mm256_andnot_si256(m1, offv), _mm256_and_si256(m1, vlut1));
                let m2 = _mm256_cmpeq_epi16(cat, two);
                offv = _mm256_or_si256(_mm256_andnot_si256(m2, offv), _mm256_and_si256(m2, vlut2));
                let m3 = _mm256_cmpeq_epi16(cat, _mm256_set1_epi16(3));
                offv = _mm256_or_si256(_mm256_andnot_si256(m3, offv), _mm256_and_si256(m3, vlut3));
                let m4 = _mm256_cmpeq_epi16(cat, _mm256_set1_epi16(4));
                offv = _mm256_or_si256(_mm256_andnot_si256(m4, offv), _mm256_and_si256(m4, vlut4));

                let v = _mm256_min_epi16(_mm256_max_epi16(_mm256_add_epi16(s, offv), zero), vmax);
                _mm256_storeu_si256(d_row.add(x) as *mut __m256i, v);
                x += 16;
            }

            while x < cx_end {
                let s = *s_row.add(x) as i32;
                let s0 = *n0_row.offset(x as isize + dx0 as isize) as i32;
                let s1 = *n1_row.offset(x as isize + dx1 as isize) as i32;
                let cat = (2 + cmp(s, s0) + cmp(s, s1)) as usize;
                let off = lut[cat] as i32;
                *d_row.add(x) = (s + off).clamp(0, maxi) as u16;
                x += 1;
            }

            for xx in cx_end..w {
                *d_row.add(xx) = *s_row.add(xx);
            }
        }
    }
}
