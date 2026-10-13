//! Scalar reference implementations of the HEVC **in-loop deblocking-filter**
//! primitive family on the reconstructed `u16` picture plane.
//!
//! Byte-exact port of ffmpeg-rs `av-codec-h265/filter/deblock.rs`
//! (`luma_strong` ~752, `luma_weak` ~794, `chroma_weak` ~877), itself a port of
//! `_ref/FFmpeg/libavcodec/h26x/h2656_deblock_template.c`
//! (`loop_filter_luma_strong` / `loop_filter_luma_weak` / `loop_filter_chroma_weak`).
//!
//! # Scope — the DSP inner kernels only
//! These primitives are the 4-line (luma) / `size`-line (chroma) filter loops.
//! The CALLER (`filter_luma_edge` / `filter_chroma_edge`) owns everything
//! around them: the boundary-strength grid, the `d0+d3 < beta` decision, the
//! strong-vs-weak selection, and the derivation of `tc`/`nd_p`/`nd_q`/`size`.
//! The primitive receives the segment origin `base` and the two orientation
//! strides and simply runs the arithmetic.
//!
//! # Orientation via `xstride` / `ystride` (ONE kernel, both edges)
//! A HEVC deblock edge is filtered along `xstride` (the P/Q axis) over `ystride`
//! (the segment-step / line axis), both in **samples** (vertical edge:
//! `xstride = 1`, `ystride = stride`; horizontal edge: `xstride = stride`,
//! `ystride = 1`).
//!
//! The scalar kernel is orientation-agnostic — it only ever indexes
//! `base + d*ystride + k*xstride`, so the SAME code serves both edges (the
//! SIMD backends fold the vertical/horizontal difference into their gather).
//!
//! # i32 accumulation (see the SIMD backends)
//! Every tap sum accumulates in **i32**. The strong-filter intermediate
//! `2*p3 + 3*p2 + p1 + p0 + q0 + 4` reaches ~32764 at 12-bit — within 3 of the
//! i16 maximum — so a packed-i16 MAC would overflow; the SIMD twins accumulate
//! in i32 lanes to match this reference exactly.

/// `av_clip(x, lo, hi)` (`deblock.rs:59`).
#[inline]
fn clip(x: i32, lo: i32, hi: i32) -> i32 {
    x.clamp(lo, hi)
}

/// `av_clip_pixel` for the current bit depth (`deblock.rs:387`): clip to
/// `[0, max]` and narrow to `u16`.
#[inline]
fn clip_pixel(x: i32, max: i32) -> u16 {
    x.clamp(0, max) as u16
}

/// Scalar reference for [`crate::hevc_deblock_luma_strong_u16`].
///
/// The 4-tap clipped strong filter over 4 sample lines. Per line reads
/// `p3..q3` (8 samples) and writes 6 outputs, each `x + clip(tap - x, -tcN,
/// tcN)`. The strong filter clips the *delta* only (NO `clip_pixel`): the
/// result is provably in `[0, max]` (a clipped move toward an in-range
/// weighted average), so the `u16` store needs no clamp. `_max` is accepted for
/// signature symmetry with the weak/chroma primitives and is unused here.
///
/// # Safety
/// `plane` valid for every index `base + d*ystride + k*xstride`, `d in 0..4`,
/// `k in -4..=3` (the safe wrapper asserts this).
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn hevc_deblock_luma_strong_u16(
    plane: *mut u16,
    base: usize,
    xstride: isize,
    ystride: isize,
    tc: i32,
    tc2: i32,
    tc3: i32,
    _max: i32,
) {
    unsafe {
        let base = base as isize;
        for d in 0..4isize {
            let off = base + d * ystride;
            let p3 = *plane.offset(off - 4 * xstride) as i32;
            let p2 = *plane.offset(off - 3 * xstride) as i32;
            let p1 = *plane.offset(off - 2 * xstride) as i32;
            let p0 = *plane.offset(off - xstride) as i32;
            let q0 = *plane.offset(off) as i32;
            let q1 = *plane.offset(off + xstride) as i32;
            let q2 = *plane.offset(off + 2 * xstride) as i32;
            let q3 = *plane.offset(off + 3 * xstride) as i32;
            let np0 = p0
                + clip(
                    ((p2 + 2 * p1 + 2 * p0 + 2 * q0 + q1 + 4) >> 3) - p0,
                    -tc3,
                    tc3,
                );
            let np1 = p1 + clip(((p2 + p1 + p0 + q0 + 2) >> 2) - p1, -tc2, tc2);
            let np2 = p2 + clip(((2 * p3 + 3 * p2 + p1 + p0 + q0 + 4) >> 3) - p2, -tc, tc);
            let nq0 = q0
                + clip(
                    ((p1 + 2 * p0 + 2 * q0 + 2 * q1 + q2 + 4) >> 3) - q0,
                    -tc3,
                    tc3,
                );
            let nq1 = q1 + clip(((p0 + q0 + q1 + q2 + 2) >> 2) - q1, -tc2, tc2);
            let nq2 = q2 + clip(((2 * q3 + 3 * q2 + q1 + q0 + p0 + 4) >> 3) - q2, -tc, tc);
            *plane.offset(off - 3 * xstride) = np2 as u16;
            *plane.offset(off - 2 * xstride) = np1 as u16;
            *plane.offset(off - xstride) = np0 as u16;
            *plane.offset(off) = nq0 as u16;
            *plane.offset(off + xstride) = nq1 as u16;
            *plane.offset(off + 2 * xstride) = nq2 as u16;
        }
    }
}

/// Scalar reference for [`crate::hevc_deblock_luma_weak_u16`].
///
/// The weak filter over 4 lines: `delta0 = (9*(q0-p0) - 3*(q1-p1) + 8) >> 4`
/// with the **per-line** gate `|delta0| < 10*tc`; a line whose gate fails keeps
/// its input bytes. On a passing line: `p0`/`q0` are always written
/// (`clip_pixel`), and `p1`/`q1` only when the (uniform, per-segment) `nd_p`/
/// `nd_q > 1`.
///
/// # Safety
/// `plane` valid for every index `base + d*ystride + k*xstride`, `d in 0..4`,
/// `k in -3..=2` (the safe wrapper asserts this).
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn hevc_deblock_luma_weak_u16(
    plane: *mut u16,
    base: usize,
    xstride: isize,
    ystride: isize,
    tc: i32,
    nd_p: i32,
    nd_q: i32,
    max: i32,
) {
    unsafe {
        let base = base as isize;
        let tc_2 = tc >> 1;
        for d in 0..4isize {
            let off = base + d * ystride;
            let p2 = *plane.offset(off - 3 * xstride) as i32;
            let p1 = *plane.offset(off - 2 * xstride) as i32;
            let p0 = *plane.offset(off - xstride) as i32;
            let q0 = *plane.offset(off) as i32;
            let q1 = *plane.offset(off + xstride) as i32;
            let q2 = *plane.offset(off + 2 * xstride) as i32;
            let mut delta0 = (9 * (q0 - p0) - 3 * (q1 - p1) + 8) >> 4;
            if delta0.abs() < 10 * tc {
                delta0 = clip(delta0, -tc, tc);
                *plane.offset(off - xstride) = clip_pixel(p0 + delta0, max);
                *plane.offset(off) = clip_pixel(q0 - delta0, max);
                if nd_p > 1 {
                    let deltap1 = clip((((p2 + p0 + 1) >> 1) - p1 + delta0) >> 1, -tc_2, tc_2);
                    *plane.offset(off - 2 * xstride) = clip_pixel(p1 + deltap1, max);
                }
                if nd_q > 1 {
                    let deltaq1 = clip((((q2 + q0 + 1) >> 1) - q1 - delta0) >> 1, -tc_2, tc_2);
                    *plane.offset(off + xstride) = clip_pixel(q1 + deltaq1, max);
                }
            }
        }
    }
}

/// Scalar reference for [`crate::hevc_deblock_chroma_weak_u16`].
///
/// The single-tap chroma filter over `size` lines:
/// `delta0 = clip(((q0-p0)*4 + p1 - q1 + 4) >> 3, -tc, tc)`, then `p0`/`q0`
/// written via `clip_pixel`. Fully branchless.
///
/// # Safety
/// `plane` valid for every index `base + d*ystride + k*xstride`,
/// `d in 0..size`, `k in -2..=1` (the safe wrapper asserts this).
pub(crate) unsafe fn hevc_deblock_chroma_weak_u16(
    plane: *mut u16,
    base: usize,
    xstride: isize,
    ystride: isize,
    size: usize,
    tc: i32,
    max: i32,
) {
    unsafe {
        let base = base as isize;
        for d in 0..size as isize {
            let off = base + d * ystride;
            let p1 = *plane.offset(off - 2 * xstride) as i32;
            let p0 = *plane.offset(off - xstride) as i32;
            let q0 = *plane.offset(off) as i32;
            let q1 = *plane.offset(off + xstride) as i32;
            let delta0 = clip((((q0 - p0) * 4) + p1 - q1 + 4) >> 3, -tc, tc);
            *plane.offset(off - xstride) = clip_pixel(p0 + delta0, max);
            *plane.offset(off) = clip_pixel(q0 - delta0, max);
        }
    }
}
