//! SSE2 implementations of the HEVC **intra-prediction** primitive family
//! (planar + angular fill) on the `u16` picture plane. Byte-exact vs the scalar
//! reference (`prims_scalar/intra.rs`).
//!
//! # i32-lane MAC — the single most important invariant
//! Both kernels accumulate in **i32** lanes: each product is formed with the
//! proven signed widening multiply `mullo_epi16` + `mulhi_epi16` +
//! `unpack{lo,hi}_epi16` (the exact idiom the `mc_*tap_*_i32_to_i32` kernels
//! use). This is LOSSLESS because every operand fits `i16`: HEVC picture
//! samples are `≤ 12-bit` (`≤ 4095`); the angular weights `(32-fact) ∈ [1,32]`
//! / `fact ∈ [0,31]`; the planar ramps `x ∈ [0,31]` and `D = top[size]-left[y]
//! ∈ [-4095,4095]`, `wy = size-1-y ∈ [0,31]`. A packed **i16** MAC would
//! overflow (planar numerator ~262112, angular 12-bit 126945) — hence i32.
//!
//! # Store narrow (no clip)
//! The results are provably in `[0, 2^bd) ⊆ [0, 4095]`, so `packs_epi32`
//! (signed saturate i32→i16, exact for `[0, 32767]`) reinterpreted as `u16` is
//! byte-exact — no `packus`/clamp needed (and SSE2 lacks `packus_epi32`).
//!
//! # 8-wide interior + scalar tail (bounded loads, G3)
//! Both kernels process the inner index in 8-lane chunks (`col + 8 <= size`)
//! then a scalar tail. A fixed-width SIMD load therefore never reaches beyond
//! the last full chunk, so it stays inside the caller-sized `ref_buf` /
//! `top`/`left` (`size = 4` runs entirely scalar — no 8-chunk exists — which is
//! exactly why the small-block over-read cannot happen).

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// SSE2 impl of [`crate::hevc_intra_planar_u16`].
///
/// Uses the exact integer identity `v = base + x·D + wy·top[x]` (`>> shift`),
/// where `base = (size-1)·left[y] + top[size] + (y+1)·left[size] + size`,
/// `D = top[size]-left[y]`, `wy = size-1-y` — algebraically identical to the
/// scalar `POS(x,y)`, one widening multiply per varying term.
///
/// # Safety
/// `dst` valid for `size×size` at `dst_stride`; `top`/`left` valid for `size+1`
/// reads. See the module header for the i16-range contract.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        // lane ramp [0,1,..,7]; each 8-chunk adds its starting column.
        let base_ramp = _mm_set_epi16(7, 6, 5, 4, 3, 2, 1, 0);
        for y in 0..size {
            let l_y = *left.add(y) as i32;
            let d = t_s - l_y;
            let base = (isize_ - 1) * l_y + t_s + (y as i32 + 1) * l_s + isize_;
            let wy = isize_ - 1 - y as i32;
            let wd = _mm_set1_epi16(d as i16);
            let wwy = _mm_set1_epi16(wy as i16);
            let vbase = _mm_set1_epi32(base);
            let d_row = dst.offset(y as isize * dst_stride);
            let mut x = 0usize;
            while x + 8 <= size {
                let xr = _mm_add_epi16(base_ramp, _mm_set1_epi16(x as i16)); // x lanes (i16)
                let tp = _mm_loadu_si128(top.add(x) as *const __m128i); // top[x..x+8] (u16 ⊆ i16)
                // x·D  →  i32 lanes
                let plo = _mm_mullo_epi16(xr, wd);
                let phi = _mm_mulhi_epi16(xr, wd);
                let mut acc_lo = _mm_unpacklo_epi16(plo, phi);
                let mut acc_hi = _mm_unpackhi_epi16(plo, phi);
                // wy·top[x]  →  i32 lanes
                let qlo = _mm_mullo_epi16(tp, wwy);
                let qhi = _mm_mulhi_epi16(tp, wwy);
                acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(qlo, qhi));
                acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(qlo, qhi));
                // + base, arithmetic >> shift
                acc_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, vbase), cnt);
                acc_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, vbase), cnt);
                // narrow to u16 (values in [0,4095] → packs_epi32 exact)
                let u = _mm_packs_epi32(acc_lo, acc_hi);
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

/// Contiguous-row angular fill (SSE2). 8 samples/step via the i32-lane 2-tap
/// MAC `((32-fact)·r1 + fact·r2 + 16) >> 5`; the two taps are the SAME
/// contiguous window shifted by 1. The `fact == 0` case needs no branch — the
/// general formula yields exactly `r1` (`(32·r1+16)>>5 == r1`) and the extra
/// `r2` read stays inside the caller-sized buffer (see the API assert).
///
/// # Safety
/// `out` valid for `size` rows at `out_stride`; `ref_buf` covers the tap window.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
unsafe fn fill_rows_sse2(
    out: *mut u16,
    out_stride: isize,
    ref_buf: *const i32,
    bias: usize,
    size: usize,
    angle: i32,
) {
    unsafe {
        let round = _mm_set1_epi32(16);
        let cnt5 = _mm_cvtsi32_si128(5);
        for r in 0..size {
            let ra = r as i32 + 1;
            let idx = (ra * angle) >> 5;
            let fact = (ra * angle) & 31;
            let w1 = _mm_set1_epi16((32 - fact) as i16);
            let w2 = _mm_set1_epi16(fact as i16);
            let base1 = (idx + 1 + bias as i32) as isize; // ref index of tap0 at j=0
            let out_row = out.offset(r as isize * out_stride);
            let mut j = 0usize;
            while j + 8 <= size {
                let p1 = ref_buf.offset(base1 + j as isize);
                let s1 = _mm_packs_epi32(
                    _mm_loadu_si128(p1 as *const __m128i),
                    _mm_loadu_si128(p1.add(4) as *const __m128i),
                ); // r1 window (i16 ⊆ [0,4095])
                let p2 = p1.add(1);
                let s2 = _mm_packs_epi32(
                    _mm_loadu_si128(p2 as *const __m128i),
                    _mm_loadu_si128(p2.add(4) as *const __m128i),
                ); // r2 window
                // (32-fact)·r1  →  i32 lanes
                let plo1 = _mm_mullo_epi16(s1, w1);
                let phi1 = _mm_mulhi_epi16(s1, w1);
                let mut acc_lo = _mm_unpacklo_epi16(plo1, phi1);
                let mut acc_hi = _mm_unpackhi_epi16(plo1, phi1);
                // fact·r2  →  i32 lanes
                let plo2 = _mm_mullo_epi16(s2, w2);
                let phi2 = _mm_mulhi_epi16(s2, w2);
                acc_lo = _mm_add_epi32(acc_lo, _mm_unpacklo_epi16(plo2, phi2));
                acc_hi = _mm_add_epi32(acc_hi, _mm_unpackhi_epi16(plo2, phi2));
                // + 16, >> 5, narrow (values in [0,4095] → packs_epi32 exact)
                acc_lo = _mm_sra_epi32(_mm_add_epi32(acc_lo, round), cnt5);
                acc_hi = _mm_sra_epi32(_mm_add_epi32(acc_hi, round), cnt5);
                let u = _mm_packs_epi32(acc_lo, acc_hi);
                _mm_storeu_si128(out_row.add(j) as *mut __m128i, u);
                j += 8;
            }
            while j < size {
                let r1 = *ref_buf.offset(base1 + j as isize);
                let r2 = *ref_buf.offset(base1 + j as isize + 1);
                *out_row.add(j) = (((32 - fact) * r1 + fact * r2 + 16) >> 5) as u16;
                j += 1;
            }
        }
    }
}

/// SSE2 impl of [`crate::hevc_intra_angular_u16`]. Vertical writes rows
/// directly; horizontal computes into a scratch with the same contiguous kernel
/// then transpose-stores (the transpose is a pure data move, kept scalar).
///
/// # Safety
/// `dst` valid for `size×size` at `dst_stride`; `ref_buf`/`bias`/`size` uphold
/// the tap-window bound (see the API assert).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
            fill_rows_sse2(dst, dst_stride, ref_buf, bias, size, angle);
        } else {
            let mut scratch = [0u16; 32 * 32];
            fill_rows_sse2(
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
