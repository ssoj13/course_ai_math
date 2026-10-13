#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// SSE4.1 impl of [`crate::weighted_avg_u16`].  8 u16/iter; widen u16→i32
/// (`cvtepu16_epi32`), `mullo_epi32` per weight, sum + offset,
/// round-shift, then `packus_epi32` (clamp ≥ 0) + `min_epu16` (clamp
/// ≤ max).  SSE2 / NEON / wasm use the scalar fallback.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse4.1")]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn weighted_avg_u16(
    a: *const u16,
    a_stride: isize,
    b: *const u16,
    b_stride: isize,
    dst: *mut u16,
    dst_stride: isize,
    w: usize,
    h: usize,
    w0: i16,
    w1: i16,
    offset: i32,
    shift: u32,
    max: u16,
) {
    unsafe {
        let w0v = _mm_set1_epi32(w0 as i32);
        let w1v = _mm_set1_epi32(w1 as i32);
        let offv = _mm_set1_epi32(offset);
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let maxv = _mm_set1_epi16(max as i16);
        let (w0s, w1s, maxi) = (w0 as i32, w1 as i32, max as i32);
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let av = _mm_loadu_si128(a_row.add(col) as *const __m128i);
                let bv = _mm_loadu_si128(b_row.add(col) as *const __m128i);
                let a_lo = _mm_cvtepu16_epi32(av);
                let a_hi = _mm_cvtepu16_epi32(_mm_srli_si128::<8>(av));
                let b_lo = _mm_cvtepu16_epi32(bv);
                let b_hi = _mm_cvtepu16_epi32(_mm_srli_si128::<8>(bv));
                let acc_lo = _mm_add_epi32(
                    _mm_add_epi32(_mm_mullo_epi32(a_lo, w0v), _mm_mullo_epi32(b_lo, w1v)),
                    offv,
                );
                let acc_hi = _mm_add_epi32(
                    _mm_add_epi32(_mm_mullo_epi32(a_hi, w0v), _mm_mullo_epi32(b_hi, w1v)),
                    offv,
                );
                let r_lo = _mm_sra_epi32(acc_lo, cnt);
                let r_hi = _mm_sra_epi32(acc_hi, cnt);
                let packed = _mm_packus_epi32(r_lo, r_hi);
                let clamped = _mm_min_epu16(packed, maxv);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, clamped);
                col += 8;
            }
            while col < w {
                let av = *a_row.add(col) as i32;
                let bv = *b_row.add(col) as i32;
                *d_row.add(col) = ((w0s * av + w1s * bv + offset) >> shift).clamp(0, maxi) as u16;
                col += 1;
            }
        }
    }
}

// =========================================================================
// HEVC MC "final" clip/combine kernels (v0.3.26): i32 intermediate -> u16
// pixel / wrapping i16.  Same i32->u16 clamp+narrow idiom proven by
// `weighted_avg_u16` above: `packus_epi32` folds the `>= 0` lower clamp into
// the narrow, `min_epu16` applies the `<= max` upper clamp.  Weights use
// `mullo_epi32`, `src2` is sign-widened with `cvtepi16_epi32`.  8 lanes/iter
// + scalar tail (byte-identical to `prims_scalar`).  SSE2 / NEON / wasm fall
// through to the scalar reference.
// =========================================================================

/// SSE4.1 impl of [`crate::mc_uni_clip_i32_to_u16`].
/// `dst = clip((v + offset) >> shift)`.  8 lanes/iter + scalar tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse4.1")]
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
        let offv = _mm_set1_epi32(offset);
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let maxv = _mm_set1_epi16(max as i16);
        let maxi = max as i32;
        for row in 0..h {
            let v_row = v.offset(row as isize * v_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let lo = _mm_loadu_si128(v_row.add(col) as *const __m128i);
                let hi = _mm_loadu_si128(v_row.add(col + 4) as *const __m128i);
                let r_lo = _mm_sra_epi32(_mm_add_epi32(lo, offv), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(hi, offv), cnt);
                let packed = _mm_packus_epi32(r_lo, r_hi);
                let clamped = _mm_min_epu16(packed, maxv);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, clamped);
                col += 8;
            }
            while col < w {
                let x = *v_row.add(col);
                *d_row.add(col) = ((x + offset) >> shift).clamp(0, maxi) as u16;
                col += 1;
            }
        }
    }
}

/// SSE4.1 impl of [`crate::mc_uni_w_clip_i32_to_u16`].
/// `dst = clip(((v * wx + offset) >> shift) + ox)`.  8 lanes/iter + scalar tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse4.1")]
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
        let wxv = _mm_set1_epi32(wx);
        let offv = _mm_set1_epi32(offset);
        let oxv = _mm_set1_epi32(ox);
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let maxv = _mm_set1_epi16(max as i16);
        let maxi = max as i32;
        for row in 0..h {
            let v_row = v.offset(row as isize * v_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let lo = _mm_loadu_si128(v_row.add(col) as *const __m128i);
                let hi = _mm_loadu_si128(v_row.add(col + 4) as *const __m128i);
                let r_lo = _mm_add_epi32(
                    _mm_sra_epi32(_mm_add_epi32(_mm_mullo_epi32(lo, wxv), offv), cnt),
                    oxv,
                );
                let r_hi = _mm_add_epi32(
                    _mm_sra_epi32(_mm_add_epi32(_mm_mullo_epi32(hi, wxv), offv), cnt),
                    oxv,
                );
                let packed = _mm_packus_epi32(r_lo, r_hi);
                let clamped = _mm_min_epu16(packed, maxv);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, clamped);
                col += 8;
            }
            while col < w {
                let x = *v_row.add(col);
                *d_row.add(col) = (((x * wx + offset) >> shift) + ox).clamp(0, maxi) as u16;
                col += 1;
            }
        }
    }
}

/// SSE4.1 impl of [`crate::mc_bi_clip_i32_to_u16`].
/// `dst = clip((v + src2 + offset) >> shift)`, `src2` sign-widened i16 -> i32.
/// 8 lanes/iter + scalar tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse4.1")]
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
        let offv = _mm_set1_epi32(offset);
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let maxv = _mm_set1_epi16(max as i16);
        let maxi = max as i32;
        for row in 0..h {
            let v_row = v.offset(row as isize * v_stride);
            let s2_row = src2.offset(row as isize * src2_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let lo = _mm_loadu_si128(v_row.add(col) as *const __m128i);
                let hi = _mm_loadu_si128(v_row.add(col + 4) as *const __m128i);
                let s2 = _mm_loadu_si128(s2_row.add(col) as *const __m128i);
                let s2_lo = _mm_cvtepi16_epi32(s2);
                let s2_hi = _mm_cvtepi16_epi32(_mm_srli_si128::<8>(s2));
                let r_lo = _mm_sra_epi32(_mm_add_epi32(_mm_add_epi32(lo, s2_lo), offv), cnt);
                let r_hi = _mm_sra_epi32(_mm_add_epi32(_mm_add_epi32(hi, s2_hi), offv), cnt);
                let packed = _mm_packus_epi32(r_lo, r_hi);
                let clamped = _mm_min_epu16(packed, maxv);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, clamped);
                col += 8;
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

/// SSE4.1 impl of [`crate::mc_bi_w_clip_i32_to_u16`].
/// `dst = clip((v * wx1 + src2 * wx0 + bias) >> rshift)`.  8 lanes/iter + tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse4.1")]
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
        let wx0v = _mm_set1_epi32(wx0);
        let wx1v = _mm_set1_epi32(wx1);
        let biasv = _mm_set1_epi32(bias);
        let cnt = _mm_cvtsi32_si128(rshift as i32);
        let maxv = _mm_set1_epi16(max as i16);
        let maxi = max as i32;
        for row in 0..h {
            let v_row = v.offset(row as isize * v_stride);
            let s2_row = src2.offset(row as isize * src2_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let lo = _mm_loadu_si128(v_row.add(col) as *const __m128i);
                let hi = _mm_loadu_si128(v_row.add(col + 4) as *const __m128i);
                let s2 = _mm_loadu_si128(s2_row.add(col) as *const __m128i);
                let s2_lo = _mm_cvtepi16_epi32(s2);
                let s2_hi = _mm_cvtepi16_epi32(_mm_srli_si128::<8>(s2));
                let r_lo = _mm_sra_epi32(
                    _mm_add_epi32(
                        _mm_add_epi32(_mm_mullo_epi32(lo, wx1v), _mm_mullo_epi32(s2_lo, wx0v)),
                        biasv,
                    ),
                    cnt,
                );
                let r_hi = _mm_sra_epi32(
                    _mm_add_epi32(
                        _mm_add_epi32(_mm_mullo_epi32(hi, wx1v), _mm_mullo_epi32(s2_hi, wx0v)),
                        biasv,
                    ),
                    cnt,
                );
                let packed = _mm_packus_epi32(r_lo, r_hi);
                let clamped = _mm_min_epu16(packed, maxv);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, clamped);
                col += 8;
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

/// SSE4.1 impl of [`crate::mc_store_i32_to_i16`].  Wrapping i32 -> i16 narrow
/// (`x as i16`, low 16 bits): mask each i32 lane to `[0, 65535]` then
/// `packus_epi32` packs the exact low words (no saturation loss).  8 lanes/iter
/// + scalar tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse4.1")]
pub(crate) unsafe fn mc_store_i32_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    v: *const i32,
    v_stride: isize,
    w: usize,
    h: usize,
) {
    unsafe {
        let m16 = _mm_set1_epi32(0xFFFF);
        for row in 0..h {
            let v_row = v.offset(row as isize * v_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let lo = _mm_and_si128(_mm_loadu_si128(v_row.add(col) as *const __m128i), m16);
                let hi = _mm_and_si128(_mm_loadu_si128(v_row.add(col + 4) as *const __m128i), m16);
                let packed = _mm_packus_epi32(lo, hi);
                _mm_storeu_si128(d_row.add(col) as *mut __m128i, packed);
                col += 8;
            }
            while col < w {
                *d_row.add(col) = *v_row.add(col) as i16;
                col += 1;
            }
        }
    }
}
