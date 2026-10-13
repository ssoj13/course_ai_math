//! AVX2 implementations of the HEVC **intra-prediction** primitive family
//! (planar + angular fill) on the `u16` picture plane. Same i32-accumulate
//! strategy as the SSE2 backend (`prims_sse2/intra.rs`) but with **native**
//! `_mm256_mullo_epi32` (AVX2), so no `mullo`/`mulhi` widening dance.
//!
//! # i32-lane MAC (see the module invariant)
//! Products are formed directly in 8×i32 lanes (`_mm256_mullo_epi32`) and
//! accumulated in i32 — the outputs are provably in `[0, 2^bd) ⊆ [0,4095]`, so
//! the i32→u16 narrow (`packus_epi32` on the two 128-bit halves, exact for
//! `[0,65535]`) needs no clamp. HEVC samples ≤ 4095 and the small weights all
//! fit, but the ACCUMULATION is i32 to avoid the 10/12-bit i16 overflow.
//!
//! # 8-wide interior + scalar tail (bounded loads, G3)
//! The inner index runs in 8-lane chunks (`col + 8 <= size`) then a scalar
//! tail — a 256-bit load never reaches past the last full chunk, so it stays
//! inside the caller-sized buffers (`size = 4` runs entirely scalar).

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// AVX2 impl of [`crate::hevc_intra_planar_u16`]. Exact identity
/// `v = base + x·D + wy·top[x]` (`>> shift`); see the SSE2 backend header.
///
/// # Safety
/// `dst` valid for `size×size` at `dst_stride`; `top`/`left` valid for `size+1`
/// reads.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn hevc_intra_planar_u16(
    dst: *mut u16,
    dst_stride: isize,
    top: *const u16,
    left: *const u16,
    log2_size: u32,
) {
    unsafe {
        let size = 1usize << log2_size;
        let isize_ = size as i32;
        let shift = log2_size + 1;
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let t_s = *top.add(size) as i32;
        let l_s = *left.add(size) as i32;
        let ramp = _mm256_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7);
        for y in 0..size {
            let l_y = *left.add(y) as i32;
            let d = t_s - l_y;
            let base = (isize_ - 1) * l_y + t_s + (y as i32 + 1) * l_s + isize_;
            let wy = isize_ - 1 - y as i32;
            let wd = _mm256_set1_epi32(d);
            let wwy = _mm256_set1_epi32(wy);
            let vbase = _mm256_set1_epi32(base);
            let d_row = dst.offset(y as isize * dst_stride);
            let mut x = 0usize;
            while x + 8 <= size {
                let xr = _mm256_add_epi32(ramp, _mm256_set1_epi32(x as i32));
                let tp = _mm256_cvtepu16_epi32(_mm_loadu_si128(top.add(x) as *const __m128i));
                let acc = _mm256_add_epi32(
                    _mm256_add_epi32(_mm256_mullo_epi32(xr, wd), _mm256_mullo_epi32(tp, wwy)),
                    vbase,
                );
                let acc = _mm256_sra_epi32(acc, cnt);
                let lo = _mm256_castsi256_si128(acc);
                let hi = _mm256_extracti128_si256(acc, 1);
                let u = _mm_packus_epi32(lo, hi); // 8 u16, order j..j+7
                _mm_storeu_si128(d_row.add(x) as *mut __m128i, u);
                x += 8;
            }
            while x < size {
                let v = (base + (x as i32) * d + wy * (*top.add(x) as i32)) >> shift;
                *d_row.add(x) = v as u16;
                x += 1;
            }
        }
    }
}

/// Contiguous-row angular fill (AVX2). 8 samples/step via the native i32 2-tap
/// MAC `((32-fact)·r1 + fact·r2 + 16) >> 5`; `fact == 0` needs no branch (the
/// general formula yields `r1` and the extra `r2` read is in-bounds).
///
/// # Safety
/// `out` valid for `size` rows at `out_stride`; `ref_buf` covers the tap window.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
unsafe fn fill_rows_avx2(
    out: *mut u16,
    out_stride: isize,
    ref_buf: *const i32,
    bias: usize,
    size: usize,
    angle: i32,
) {
    unsafe {
        let round = _mm256_set1_epi32(16);
        let cnt5 = _mm_cvtsi32_si128(5);
        for r in 0..size {
            let ra = r as i32 + 1;
            let idx = (ra * angle) >> 5;
            let fact = (ra * angle) & 31;
            let w1 = _mm256_set1_epi32(32 - fact);
            let w2 = _mm256_set1_epi32(fact);
            let base1 = (idx + 1 + bias as i32) as isize;
            let out_row = out.offset(r as isize * out_stride);
            let mut j = 0usize;
            while j + 8 <= size {
                let p1 = ref_buf.offset(base1 + j as isize);
                let r1 = _mm256_loadu_si256(p1 as *const __m256i);
                let r2 = _mm256_loadu_si256(p1.add(1) as *const __m256i);
                let acc = _mm256_add_epi32(
                    _mm256_add_epi32(_mm256_mullo_epi32(r1, w1), _mm256_mullo_epi32(r2, w2)),
                    round,
                );
                let acc = _mm256_sra_epi32(acc, cnt5);
                let lo = _mm256_castsi256_si128(acc);
                let hi = _mm256_extracti128_si256(acc, 1);
                let u = _mm_packus_epi32(lo, hi);
                _mm_storeu_si128(out_row.add(j) as *mut __m128i, u);
                j += 8;
            }
            while j < size {
                let a = *ref_buf.offset(base1 + j as isize);
                let b = *ref_buf.offset(base1 + j as isize + 1);
                *out_row.add(j) = (((32 - fact) * a + fact * b + 16) >> 5) as u16;
                j += 1;
            }
        }
    }
}

/// AVX2 impl of [`crate::hevc_intra_angular_u16`]. Vertical writes rows;
/// horizontal fills a scratch then transpose-stores (scalar data move).
///
/// # Safety
/// `dst` valid for `size×size` at `dst_stride`; `ref_buf`/`bias`/`size` uphold
/// the tap-window bound.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn hevc_intra_angular_u16(
    dst: *mut u16,
    dst_stride: isize,
    ref_buf: *const i32,
    bias: usize,
    size: usize,
    angle: i32,
    vertical: bool,
) {
    unsafe {
        if vertical {
            fill_rows_avx2(dst, dst_stride, ref_buf, bias, size, angle);
        } else {
            let mut scratch = [0u16; 32 * 32];
            fill_rows_avx2(
                scratch.as_mut_ptr(),
                size as isize,
                ref_buf,
                bias,
                size,
                angle,
            );
            crate::prims_scalar::transpose_to_dst(dst, dst_stride, scratch.as_ptr(), size);
        }
    }
}
