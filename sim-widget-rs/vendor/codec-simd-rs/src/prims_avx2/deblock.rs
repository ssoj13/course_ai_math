//! AVX2 implementations of the HEVC **deblocking-filter** primitive family on
//! the `u16` picture plane. Same i32-lane strategy as the SSE2 backend
//! (`prims_sse2/deblock.rs`) but with **native** `_mm_min_epi32` /
//! `_mm_max_epi32` (SSE4.1-level, available under `avx2`) instead of the
//! `cmpgt`+select emulation.
//!
//! The deblock kernel filters exactly 4 luma lines (`size` chroma lines), so
//! its natural SIMD width is 4 i32 lanes = one `__m128i`; AVX2's 256-bit lanes
//! offer no extra parallelism here (there is no 8th line), so these kernels use
//! 128-bit vectors — the AVX2 win over SSE2 is only the native signed
//! min/max/`packus` availability. Byte-exact vs the scalar reference at every
//! forced tier (the cross-tier sweep is the guard against any SSE2/AVX2 typo
//! divergence). See the SSE2 header for the i32-accumulation, gather-transpose,
//! and bounded-load contracts (identical here).

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// `av_clip` per lane: `min(max(x, lo), hi)` via native `min/max_epi32`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn clip_v(x: __m128i, lo: __m128i, hi: __m128i) -> __m128i {
    _mm_min_epi32(_mm_max_epi32(x, lo), hi)
}

/// `av_clip_pixel` per lane: `min(max(x, 0), max)` (i32 lanes; narrowed at the
/// scatter store).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn clip_pixel_v(x: __m128i, zero: __m128i, vmax: __m128i) -> __m128i {
    _mm_min_epi32(_mm_max_epi32(x, zero), vmax)
}

/// Per-lane blend: pick `a` where `mask` is all-ones, else `b`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn blend_i32(mask: __m128i, a: __m128i, b: __m128i) -> __m128i {
    _mm_or_si128(_mm_and_si128(mask, a), _mm_andnot_si128(mask, b))
}

/// Gather the 4 lines' sample at P/Q position `k` into i32 lanes
/// (`lane d = plane[base + d*ystride + k*xstride]`).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn gather4(
    plane: *const u16,
    base: isize,
    xstride: isize,
    ystride: isize,
    k: isize,
) -> __m128i {
    unsafe {
        let c = base + k * xstride;
        _mm_setr_epi32(
            *plane.offset(c) as i32,
            *plane.offset(c + ystride) as i32,
            *plane.offset(c + 2 * ystride) as i32,
            *plane.offset(c + 3 * ystride) as i32,
        )
    }
}

/// Scatter i32 lanes back to the 4 lines' sample at P/Q position `k`
/// (narrowing each lane `as u16`; values proven in range by the kernel).
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn scatter4(
    plane: *mut u16,
    base: isize,
    xstride: isize,
    ystride: isize,
    k: isize,
    v: __m128i,
) {
    unsafe {
        let mut t = [0i32; 4];
        _mm_storeu_si128(t.as_mut_ptr() as *mut __m128i, v);
        let c = base + k * xstride;
        *plane.offset(c) = t[0] as u16;
        *plane.offset(c + ystride) = t[1] as u16;
        *plane.offset(c + 2 * ystride) = t[2] as u16;
        *plane.offset(c + 3 * ystride) = t[3] as u16;
    }
}

/// AVX2 impl of [`crate::hevc_deblock_luma_strong_u16`].
///
/// # Safety
/// `plane` valid for `base + d*ystride + k*xstride`, `d in 0..4`, `k in -4..=3`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[allow(clippy::too_many_arguments)]
#[target_feature(enable = "avx2")]
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
        let p3 = gather4(plane, base, xstride, ystride, -4);
        let p2 = gather4(plane, base, xstride, ystride, -3);
        let p1 = gather4(plane, base, xstride, ystride, -2);
        let p0 = gather4(plane, base, xstride, ystride, -1);
        let q0 = gather4(plane, base, xstride, ystride, 0);
        let q1 = gather4(plane, base, xstride, ystride, 1);
        let q2 = gather4(plane, base, xstride, ystride, 2);
        let q3 = gather4(plane, base, xstride, ystride, 3);
        let four = _mm_set1_epi32(4);
        let two = _mm_set1_epi32(2);
        let tc_p = _mm_set1_epi32(tc);
        let tc_n = _mm_set1_epi32(-tc);
        let tc2_p = _mm_set1_epi32(tc2);
        let tc2_n = _mm_set1_epi32(-tc2);
        let tc3_p = _mm_set1_epi32(tc3);
        let tc3_n = _mm_set1_epi32(-tc3);

        // np0 = p0 + clip(((p2 + 2p1 + 2p0 + 2q0 + q1 + 4) >> 3) - p0, -tc3, tc3)
        let s0 = _mm_add_epi32(
            _mm_add_epi32(
                _mm_add_epi32(
                    _mm_add_epi32(
                        _mm_add_epi32(p2, _mm_slli_epi32(p1, 1)),
                        _mm_slli_epi32(p0, 1),
                    ),
                    _mm_slli_epi32(q0, 1),
                ),
                q1,
            ),
            four,
        );
        let np0 = _mm_add_epi32(
            p0,
            clip_v(_mm_sub_epi32(_mm_srai_epi32(s0, 3), p0), tc3_n, tc3_p),
        );
        // np1 = p1 + clip(((p2 + p1 + p0 + q0 + 2) >> 2) - p1, -tc2, tc2)
        let s1 = _mm_add_epi32(
            _mm_add_epi32(_mm_add_epi32(_mm_add_epi32(p2, p1), p0), q0),
            two,
        );
        let np1 = _mm_add_epi32(
            p1,
            clip_v(_mm_sub_epi32(_mm_srai_epi32(s1, 2), p1), tc2_n, tc2_p),
        );
        // np2 = p2 + clip(((2p3 + 3p2 + p1 + p0 + q0 + 4) >> 3) - p2, -tc, tc)
        let three_p2 = _mm_add_epi32(_mm_slli_epi32(p2, 1), p2);
        let s2 = _mm_add_epi32(
            _mm_add_epi32(
                _mm_add_epi32(
                    _mm_add_epi32(_mm_add_epi32(_mm_slli_epi32(p3, 1), three_p2), p1),
                    p0,
                ),
                q0,
            ),
            four,
        );
        let np2 = _mm_add_epi32(
            p2,
            clip_v(_mm_sub_epi32(_mm_srai_epi32(s2, 3), p2), tc_n, tc_p),
        );
        // nq0 = q0 + clip(((p1 + 2p0 + 2q0 + 2q1 + q2 + 4) >> 3) - q0, -tc3, tc3)
        let sq0 = _mm_add_epi32(
            _mm_add_epi32(
                _mm_add_epi32(
                    _mm_add_epi32(
                        _mm_add_epi32(p1, _mm_slli_epi32(p0, 1)),
                        _mm_slli_epi32(q0, 1),
                    ),
                    _mm_slli_epi32(q1, 1),
                ),
                q2,
            ),
            four,
        );
        let nq0 = _mm_add_epi32(
            q0,
            clip_v(_mm_sub_epi32(_mm_srai_epi32(sq0, 3), q0), tc3_n, tc3_p),
        );
        // nq1 = q1 + clip(((p0 + q0 + q1 + q2 + 2) >> 2) - q1, -tc2, tc2)
        let sq1 = _mm_add_epi32(
            _mm_add_epi32(_mm_add_epi32(_mm_add_epi32(p0, q0), q1), q2),
            two,
        );
        let nq1 = _mm_add_epi32(
            q1,
            clip_v(_mm_sub_epi32(_mm_srai_epi32(sq1, 2), q1), tc2_n, tc2_p),
        );
        // nq2 = q2 + clip(((2q3 + 3q2 + q1 + q0 + p0 + 4) >> 3) - q2, -tc, tc)
        let three_q2 = _mm_add_epi32(_mm_slli_epi32(q2, 1), q2);
        let sq2 = _mm_add_epi32(
            _mm_add_epi32(
                _mm_add_epi32(
                    _mm_add_epi32(_mm_add_epi32(_mm_slli_epi32(q3, 1), three_q2), q1),
                    q0,
                ),
                p0,
            ),
            four,
        );
        let nq2 = _mm_add_epi32(
            q2,
            clip_v(_mm_sub_epi32(_mm_srai_epi32(sq2, 3), q2), tc_n, tc_p),
        );

        scatter4(plane, base, xstride, ystride, -3, np2);
        scatter4(plane, base, xstride, ystride, -2, np1);
        scatter4(plane, base, xstride, ystride, -1, np0);
        scatter4(plane, base, xstride, ystride, 0, nq0);
        scatter4(plane, base, xstride, ystride, 1, nq1);
        scatter4(plane, base, xstride, ystride, 2, nq2);
    }
}

/// AVX2 impl of [`crate::hevc_deblock_luma_weak_u16`].
///
/// # Safety
/// `plane` valid for `base + d*ystride + k*xstride`, `d in 0..4`, `k in -3..=2`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[allow(clippy::too_many_arguments)]
#[target_feature(enable = "avx2")]
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
        let p2 = gather4(plane, base, xstride, ystride, -3);
        let p1 = gather4(plane, base, xstride, ystride, -2);
        let p0 = gather4(plane, base, xstride, ystride, -1);
        let q0 = gather4(plane, base, xstride, ystride, 0);
        let q1 = gather4(plane, base, xstride, ystride, 1);
        let q2 = gather4(plane, base, xstride, ystride, 2);
        let zero = _mm_setzero_si128();
        let eight = _mm_set1_epi32(8);
        let one = _mm_set1_epi32(1);
        let vmax = _mm_set1_epi32(max);
        let tc_p = _mm_set1_epi32(tc);
        let tc_n = _mm_set1_epi32(-tc);
        let tc2_p = _mm_set1_epi32(tc_2);
        let tc2_n = _mm_set1_epi32(-tc_2);
        let ten_tc = _mm_set1_epi32(10 * tc);

        // delta0 = (9*(q0-p0) - 3*(q1-p1) + 8) >> 4     (9x = (x<<3)+x, 3x = (x<<1)+x)
        let qp0 = _mm_sub_epi32(q0, p0);
        let nine_qp0 = _mm_add_epi32(_mm_slli_epi32(qp0, 3), qp0);
        let qp1 = _mm_sub_epi32(q1, p1);
        let three_qp1 = _mm_add_epi32(_mm_slli_epi32(qp1, 1), qp1);
        let delta0 = _mm_srai_epi32(_mm_add_epi32(_mm_sub_epi32(nine_qp0, three_qp1), eight), 4);
        // gate = |delta0| < 10*tc
        let absd = _mm_abs_epi32(delta0);
        let gate = _mm_cmpgt_epi32(ten_tc, absd);
        // d0c = clip(delta0, -tc, tc)
        let d0c = clip_v(delta0, tc_n, tc_p);
        let p0n = clip_pixel_v(_mm_add_epi32(p0, d0c), zero, vmax);
        let q0n = clip_pixel_v(_mm_sub_epi32(q0, d0c), zero, vmax);
        scatter4(plane, base, xstride, ystride, -1, blend_i32(gate, p0n, p0));
        scatter4(plane, base, xstride, ystride, 0, blend_i32(gate, q0n, q0));
        if nd_p > 1 {
            let a = _mm_srai_epi32(_mm_add_epi32(_mm_add_epi32(p2, p0), one), 1);
            let num = _mm_add_epi32(_mm_sub_epi32(a, p1), d0c);
            let deltap1 = clip_v(_mm_srai_epi32(num, 1), tc2_n, tc2_p);
            let p1n = clip_pixel_v(_mm_add_epi32(p1, deltap1), zero, vmax);
            scatter4(plane, base, xstride, ystride, -2, blend_i32(gate, p1n, p1));
        }
        if nd_q > 1 {
            let a = _mm_srai_epi32(_mm_add_epi32(_mm_add_epi32(q2, q0), one), 1);
            let num = _mm_sub_epi32(_mm_sub_epi32(a, q1), d0c);
            let deltaq1 = clip_v(_mm_srai_epi32(num, 1), tc2_n, tc2_p);
            let q1n = clip_pixel_v(_mm_add_epi32(q1, deltaq1), zero, vmax);
            scatter4(plane, base, xstride, ystride, 1, blend_i32(gate, q1n, q1));
        }
    }
}

/// AVX2 impl of [`crate::hevc_deblock_chroma_weak_u16`]. 4-lane chunks + scalar
/// tail (HEVC chroma `size` is 4 → one vector iteration in the common path).
///
/// # Safety
/// `plane` valid for `base + d*ystride + k*xstride`, `d in 0..size`,
/// `k in -2..=1`.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
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
        let base0 = base as isize;
        let four = _mm_set1_epi32(4);
        let zero = _mm_setzero_si128();
        let vmax = _mm_set1_epi32(max);
        let tc_p = _mm_set1_epi32(tc);
        let tc_n = _mm_set1_epi32(-tc);
        let mut d0 = 0usize;
        while d0 + 4 <= size {
            let b = base0 + d0 as isize * ystride;
            let p1 = gather4(plane, b, xstride, ystride, -2);
            let p0 = gather4(plane, b, xstride, ystride, -1);
            let q0 = gather4(plane, b, xstride, ystride, 0);
            let q1 = gather4(plane, b, xstride, ystride, 1);
            // delta0 = clip(((q0-p0)*4 + p1 - q1 + 4) >> 3, -tc, tc)
            let num = _mm_add_epi32(
                _mm_sub_epi32(
                    _mm_add_epi32(_mm_slli_epi32(_mm_sub_epi32(q0, p0), 2), p1),
                    q1,
                ),
                four,
            );
            let delta0 = clip_v(_mm_srai_epi32(num, 3), tc_n, tc_p);
            let p0n = clip_pixel_v(_mm_add_epi32(p0, delta0), zero, vmax);
            let q0n = clip_pixel_v(_mm_sub_epi32(q0, delta0), zero, vmax);
            scatter4(plane, b, xstride, ystride, -1, p0n);
            scatter4(plane, b, xstride, ystride, 0, q0n);
            d0 += 4;
        }
        while d0 < size {
            let off = base0 + d0 as isize * ystride;
            let p1 = *plane.offset(off - 2 * xstride) as i32;
            let p0 = *plane.offset(off - xstride) as i32;
            let q0 = *plane.offset(off) as i32;
            let q1 = *plane.offset(off + xstride) as i32;
            let delta0 = ((((q0 - p0) * 4) + p1 - q1 + 4) >> 3).clamp(-tc, tc);
            *plane.offset(off - xstride) = (p0 + delta0).clamp(0, max) as u16;
            *plane.offset(off) = (q0 - delta0).clamp(0, max) as u16;
            d0 += 1;
        }
    }
}
