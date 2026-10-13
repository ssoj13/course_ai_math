//! Safe public wrappers for the HEVC **in-loop deblocking-filter** primitive
//! family on the `u16` picture plane.
//!
//! Byte-exact port of ffmpeg-rs `av-codec-h265/filter/deblock.rs` (`luma_strong`
//! / `luma_weak` / `chroma_weak`). These are the DSP inner kernels: the caller
//! owns the boundary-strength grid, the `d0+d3 < beta` decision, the strong-vs-
//! weak selection, and the derivation of `tc`/`nd_p`/`nd_q`/`size`.
//!
//! # Plane + stride ownership (ONE kernel, both orientations)
//! Each primitive takes the plane slice, the segment origin `base` (a flat
//! index in `u16` elements), and the two orientation strides in **samples**
//! (vertical edge: `xstride = 1`, `ystride = stride`; horizontal edge:
//! `xstride = stride`, `ystride = 1`).
//!
//! The kernel indexes `base + d*ystride + k*xstride` (`d` = line, `k` = P/Q
//! position), so the SAME primitive serves both edges; the SIMD backends fold
//! the vertical/horizontal difference into their internal gather. Both strides
//! must be `>= 1` (real HEVC deblock edges are never at the picture boundary,
//! so `base` is always far enough from the plane start for the negative-`k`
//! reads to stay in-bounds — the asserts below enforce it).

use crate::dispatch;

/// HEVC **strong luma** deblock filter over 4 sample lines: per line reads
/// `p3..q3` and writes the 6 outputs `x + clip(tap - x, -tcN, tcN)` (the strong
/// filter clips the delta only — the result is provably in `[0, max]`, so the
/// `u16` store needs no clamp). `tc`/`tc2`/`tc3` are the per-band clip bounds
/// (the caller passes `tc<<1` for all three in the reference path; kept
/// separate for faithfulness). `max = (1<<bd)-1` is accepted for signature
/// symmetry and is unused by the strong filter.
///
/// # Panics
/// If `xstride < 1` or `ystride < 1`, or `base` / `plane` are too small for the
/// 4×(`p3..q3`) access window (`base - 4*xstride .. base + 3*ystride + 3*xstride`).
#[allow(clippy::too_many_arguments)]
pub fn hevc_deblock_luma_strong_u16(
    plane: &mut [u16],
    base: usize,
    xstride: isize,
    ystride: isize,
    tc: i32,
    tc2: i32,
    tc3: i32,
    max: i32,
) {
    assert!(
        xstride >= 1 && ystride >= 1,
        "codec-simd: hevc_deblock_luma_strong_u16 strides must be >= 1 (xstride={xstride}, ystride={ystride})"
    );
    let (xn, yn) = (xstride as usize, ystride as usize);
    assert!(
        base >= 4 * xn,
        "codec-simd: hevc_deblock_luma_strong_u16 base too small ({base} < {})",
        4 * xn
    );
    let hi = base + 3 * yn + 3 * xn;
    assert!(
        plane.len() > hi,
        "codec-simd: hevc_deblock_luma_strong_u16 plane too short ({} <= {hi})",
        plane.len()
    );
    // SAFETY: the access window `base - 4*xstride ..= base + 3*ystride + 3*xstride`
    // is verified in-bounds above; strides are >= 1.
    unsafe {
        (dispatch::get().hevc_deblock_luma_strong_u16)(
            plane.as_mut_ptr(),
            base,
            xstride,
            ystride,
            tc,
            tc2,
            tc3,
            max,
        );
    }
}

/// HEVC **weak luma** deblock filter over 4 sample lines:
/// `delta0 = (9*(q0-p0) - 3*(q1-p1) + 8) >> 4` with the per-line gate
/// `|delta0| < 10*tc` (a failing line keeps its input bytes). On a passing
/// line `p0`/`q0` are written (`clip_pixel(0, max)`), plus `p1` iff `nd_p > 1`
/// and `q1` iff `nd_q > 1` (`nd_p`/`nd_q` are uniform across the 4 lines).
///
/// # Panics
/// If `xstride < 1` or `ystride < 1`, or `base` / `plane` are too small for the
/// 4×(`p2..q2`) access window (`base - 3*xstride .. base + 3*ystride + 2*xstride`).
#[allow(clippy::too_many_arguments)]
pub fn hevc_deblock_luma_weak_u16(
    plane: &mut [u16],
    base: usize,
    xstride: isize,
    ystride: isize,
    tc: i32,
    nd_p: i32,
    nd_q: i32,
    max: i32,
) {
    assert!(
        xstride >= 1 && ystride >= 1,
        "codec-simd: hevc_deblock_luma_weak_u16 strides must be >= 1 (xstride={xstride}, ystride={ystride})"
    );
    let (xn, yn) = (xstride as usize, ystride as usize);
    assert!(
        base >= 3 * xn,
        "codec-simd: hevc_deblock_luma_weak_u16 base too small ({base} < {})",
        3 * xn
    );
    let hi = base + 3 * yn + 2 * xn;
    assert!(
        plane.len() > hi,
        "codec-simd: hevc_deblock_luma_weak_u16 plane too short ({} <= {hi})",
        plane.len()
    );
    // SAFETY: window `base - 3*xstride ..= base + 3*ystride + 2*xstride` in-bounds.
    unsafe {
        (dispatch::get().hevc_deblock_luma_weak_u16)(
            plane.as_mut_ptr(),
            base,
            xstride,
            ystride,
            tc,
            nd_p,
            nd_q,
            max,
        );
    }
}

/// HEVC **weak chroma** deblock filter over `size` sample lines: single-tap
/// `delta0 = clip(((q0-p0)*4 + p1 - q1 + 4) >> 3, -tc, tc)`, then `p0`/`q0`
/// written via `clip_pixel(0, max)`. Fully branchless. `size` is the HEVC
/// chroma segment length (always 4 in the shipping path).
///
/// # Panics
/// If `xstride < 1` or `ystride < 1`, `size == 0`, or `base` / `plane` are too
/// small for the `size`×(`p1..q1`) access window
/// (`base - 2*xstride .. base + (size-1)*ystride + xstride`).
pub fn hevc_deblock_chroma_weak_u16(
    plane: &mut [u16],
    base: usize,
    xstride: isize,
    ystride: isize,
    size: usize,
    tc: i32,
    max: i32,
) {
    assert!(
        xstride >= 1 && ystride >= 1,
        "codec-simd: hevc_deblock_chroma_weak_u16 strides must be >= 1 (xstride={xstride}, ystride={ystride})"
    );
    assert!(
        size >= 1,
        "codec-simd: hevc_deblock_chroma_weak_u16 size must be >= 1"
    );
    let (xn, yn) = (xstride as usize, ystride as usize);
    assert!(
        base >= 2 * xn,
        "codec-simd: hevc_deblock_chroma_weak_u16 base too small ({base} < {})",
        2 * xn
    );
    let hi = base + (size - 1) * yn + xn;
    assert!(
        plane.len() > hi,
        "codec-simd: hevc_deblock_chroma_weak_u16 plane too short ({} <= {hi})",
        plane.len()
    );
    // SAFETY: window `base - 2*xstride ..= base + (size-1)*ystride + xstride` in-bounds.
    unsafe {
        (dispatch::get().hevc_deblock_chroma_weak_u16)(
            plane.as_mut_ptr(),
            base,
            xstride,
            ystride,
            size,
            tc,
            max,
        );
    }
}
