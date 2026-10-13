//! SSE2 implementation of the HEVC **SAO edge** filter primitive on the `u16`
//! picture plane.  (The SAO **band** filter is a 32-entry per-pixel LUT gather
//! that scalar handles at least as well as a SIMD scalar-gather, so it is NOT
//! installed here — the dispatch table keeps the scalar band impl; see
//! `dispatch.rs`.)
//!
//! # SIMD strategy — classify 8 pixels per step in `i16` lanes
//! HEVC picture samples are `≤ 12-bit` (`≤ 4095`), so every sample fits the
//! non-negative half of `i16` and the whole edge kernel runs in `i16` lanes
//! (8 per 128-bit register): the sign `CMP(p,n) = (p>n) - (p<n)` is two signed
//! `cmpgt_epi16` (valid because both operands are in `[0, 4095]`), the
//! category `2 + sign0 + sign1 ∈ [0,4]` is two adds, the category→offset LUT
//! (`offsets[edge_idx[cat]]`) is 4 `cmpeq`+select blends (SSE2 has no
//! `blendv`), and `clip(p + off, 0, max)` is `add`/`max_epi16(0)`/
//! `min_epi16(max)`.  Byte-exact vs the scalar reference.
//!
//! # Boundary handling
//! For an `eo_class` with `dx`/`dy` ∈ {−1,0,+1}, the pixels that receive an
//! offset form the interior rectangle `[has_dx, w-has_dx) × [has_dy, h-has_dy)`
//! (a pixel on the first/last row when `dy≠0`, or first/last column when
//! `dx≠0`, has an out-of-region neighbour and keeps its value).  The interior
//! rows run SIMD over full 8-wide column chunks + a scalar tail; boundary rows
//! and boundary/tail columns are copied/handled scalar — net-identical to the
//! per-pixel reference.

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

use crate::prims_scalar::{EDGE_IDX, POS, cmp};

/// SSE2 backend for [`crate::sao_edge_u16`].  See the module header.
///
/// # Safety
/// `src`/`dst` valid for the `w × h` region at the given strides; `offsets`
/// valid for 5 `i16` reads.  Byte-exactness requires `sample + offset ∈ i16`
/// (`max + max|offset| ≤ i16::MAX`); HEVC (`bit_depth ≤ 12`, small scaled
/// offsets) always satisfies this.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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

        // Category→offset LUT: lut[cat] = offsets[edge_idx[cat]], cat ∈ [0,4].
        let lut = [
            *offsets.add(EDGE_IDX[0]),
            *offsets.add(EDGE_IDX[1]),
            *offsets.add(EDGE_IDX[2]),
            *offsets.add(EDGE_IDX[3]),
            *offsets.add(EDGE_IDX[4]),
        ];

        let two = _mm_set1_epi16(2);
        let zero = _mm_setzero_si128();
        let vmax = _mm_set1_epi16(maxi as i16);
        let vlut1 = _mm_set1_epi16(lut[1]);
        let vlut2 = _mm_set1_epi16(lut[2]);
        let vlut3 = _mm_set1_epi16(lut[3]);
        let vlut4 = _mm_set1_epi16(lut[4]);

        let cx0 = has_dx;
        let cx_end = w - has_dx; // exclusive; == w when has_dx == 0

        for y in 0..h {
            let s_row = src.offset(y as isize * src_stride);
            let d_row = dst.offset(y as isize * dst_stride);

            // Boundary rows: keep src.
            if y < has_dy || y >= h - has_dy {
                for x in 0..w {
                    *d_row.add(x) = *s_row.add(x);
                }
                continue;
            }

            // Boundary columns on the left: keep src.
            for x in 0..cx0 {
                *d_row.add(x) = *s_row.add(x);
            }

            let n0_row = src.offset((y as isize + dy0 as isize) * src_stride);
            let n1_row = src.offset((y as isize + dy1 as isize) * src_stride);

            // SIMD interior: 8 pixels per step.
            let mut x = cx0;
            while x + 8 <= cx_end {
                let s = _mm_loadu_si128(s_row.add(x) as *const __m128i);
                let n0 =
                    _mm_loadu_si128(n0_row.offset(x as isize + dx0 as isize) as *const __m128i);
                let n1 =
                    _mm_loadu_si128(n1_row.offset(x as isize + dx1 as isize) as *const __m128i);

                // sign(p - n) = (p>n) - (p<n) via signed 16-bit compares.
                let sign0 = _mm_sub_epi16(_mm_cmpgt_epi16(n0, s), _mm_cmpgt_epi16(s, n0));
                let sign1 = _mm_sub_epi16(_mm_cmpgt_epi16(n1, s), _mm_cmpgt_epi16(s, n1));
                let cat = _mm_add_epi16(_mm_add_epi16(two, sign0), sign1); // 0..=4

                // offv = lut[cat] via cmpeq+select (default lut[0]).
                let mut offv = _mm_set1_epi16(lut[0]);
                let m1 = _mm_cmpeq_epi16(cat, _mm_set1_epi16(1));
                offv = _mm_or_si128(_mm_andnot_si128(m1, offv), _mm_and_si128(m1, vlut1));
                let m2 = _mm_cmpeq_epi16(cat, two);
                offv = _mm_or_si128(_mm_andnot_si128(m2, offv), _mm_and_si128(m2, vlut2));
                let m3 = _mm_cmpeq_epi16(cat, _mm_set1_epi16(3));
                offv = _mm_or_si128(_mm_andnot_si128(m3, offv), _mm_and_si128(m3, vlut3));
                let m4 = _mm_cmpeq_epi16(cat, _mm_set1_epi16(4));
                offv = _mm_or_si128(_mm_andnot_si128(m4, offv), _mm_and_si128(m4, vlut4));

                // clip(p + offv, 0, max).
                let v = _mm_min_epi16(_mm_max_epi16(_mm_add_epi16(s, offv), zero), vmax);
                _mm_storeu_si128(d_row.add(x) as *mut __m128i, v);
                x += 8;
            }

            // Scalar interior tail.
            while x < cx_end {
                let s = *s_row.add(x) as i32;
                let s0 = *n0_row.offset(x as isize + dx0 as isize) as i32;
                let s1 = *n1_row.offset(x as isize + dx1 as isize) as i32;
                let cat = (2 + cmp(s, s0) + cmp(s, s1)) as usize;
                let off = lut[cat] as i32;
                *d_row.add(x) = (s + off).clamp(0, maxi) as u16;
                x += 1;
            }

            // Boundary columns on the right: keep src.
            for xx in cx_end..w {
                *d_row.add(xx) = *s_row.add(xx);
            }
        }
    }
}
