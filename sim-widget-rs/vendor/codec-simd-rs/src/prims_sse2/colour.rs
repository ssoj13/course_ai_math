#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// SSE2 impl of [`crate::lincomb3_clip_u8`].  8/iter; widen each u8→i16,
/// signed widening multiply by each coefficient, sum + offset,
/// round-shift, clip to u8.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn lincomb3_clip_u8(
    p0: *const u8,
    p1: *const u8,
    p2: *const u8,
    dst: *mut u8,
    n: usize,
    c0: i16,
    c1: i16,
    c2: i16,
    offset: i32,
    shift: u32,
) {
    unsafe {
        let zero = _mm_setzero_si128();
        let c0v = _mm_set1_epi16(c0);
        let c1v = _mm_set1_epi16(c1);
        let c2v = _mm_set1_epi16(c2);
        let offv = _mm_set1_epi32(offset);
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let (c0s, c1s, c2s) = (c0 as i32, c1 as i32, c2 as i32);
        let mut i = 0usize;
        while i + 8 <= n {
            let a = _mm_unpacklo_epi8(_mm_loadl_epi64(p0.add(i) as *const __m128i), zero);
            let b = _mm_unpacklo_epi8(_mm_loadl_epi64(p1.add(i) as *const __m128i), zero);
            let c = _mm_unpacklo_epi8(_mm_loadl_epi64(p2.add(i) as *const __m128i), zero);
            let pa0 = _mm_mullo_epi16(a, c0v);
            let pa1 = _mm_mulhi_epi16(a, c0v);
            let pb0 = _mm_mullo_epi16(b, c1v);
            let pb1 = _mm_mulhi_epi16(b, c1v);
            let pc0 = _mm_mullo_epi16(c, c2v);
            let pc1 = _mm_mulhi_epi16(c, c2v);
            let lo = _mm_add_epi32(
                _mm_add_epi32(
                    _mm_add_epi32(_mm_unpacklo_epi16(pa0, pa1), _mm_unpacklo_epi16(pb0, pb1)),
                    _mm_unpacklo_epi16(pc0, pc1),
                ),
                offv,
            );
            let hi = _mm_add_epi32(
                _mm_add_epi32(
                    _mm_add_epi32(_mm_unpackhi_epi16(pa0, pa1), _mm_unpackhi_epi16(pb0, pb1)),
                    _mm_unpackhi_epi16(pc0, pc1),
                ),
                offv,
            );
            let i16s = _mm_packs_epi32(_mm_sra_epi32(lo, cnt), _mm_sra_epi32(hi, cnt));
            _mm_storel_epi64(dst.add(i) as *mut __m128i, _mm_packus_epi16(i16s, i16s));
            i += 8;
        }
        while i < n {
            let acc = c0s * (*p0.add(i) as i32)
                + c1s * (*p1.add(i) as i32)
                + c2s * (*p2.add(i) as i32)
                + offset;
            *dst.add(i) = ((acc) >> shift).clamp(0, 255) as u8;
            i += 1;
        }
    }
}

/// SSE2 impl of [`crate::rct_fwd_i32`].  4 i32/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn rct_fwd_i32(
    r: *const i32,
    g: *const i32,
    b: *const i32,
    y: *mut i32,
    cb: *mut i32,
    cr: *mut i32,
    n: usize,
) {
    unsafe {
        let mut i = 0usize;
        while i + 4 <= n {
            let rr = _mm_loadu_si128(r.add(i) as *const __m128i);
            let gg = _mm_loadu_si128(g.add(i) as *const __m128i);
            let bb = _mm_loadu_si128(b.add(i) as *const __m128i);
            let yv = _mm_srai_epi32(
                _mm_add_epi32(_mm_add_epi32(rr, _mm_slli_epi32(gg, 1)), bb),
                2,
            );
            _mm_storeu_si128(y.add(i) as *mut __m128i, yv);
            _mm_storeu_si128(cb.add(i) as *mut __m128i, _mm_sub_epi32(bb, gg));
            _mm_storeu_si128(cr.add(i) as *mut __m128i, _mm_sub_epi32(rr, gg));
            i += 4;
        }
        while i < n {
            let (rr, gg, bb) = (*r.add(i), *g.add(i), *b.add(i));
            *y.add(i) = (rr + (gg << 1) + bb) >> 2;
            *cb.add(i) = bb - gg;
            *cr.add(i) = rr - gg;
            i += 1;
        }
    }
}

/// SSE2 impl of [`crate::rct_bwd_i32`].  4 i32/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn rct_bwd_i32(
    y: *const i32,
    cb: *const i32,
    cr: *const i32,
    r: *mut i32,
    g: *mut i32,
    b: *mut i32,
    n: usize,
) {
    unsafe {
        let mut i = 0usize;
        while i + 4 <= n {
            let yv = _mm_loadu_si128(y.add(i) as *const __m128i);
            let cbv = _mm_loadu_si128(cb.add(i) as *const __m128i);
            let crv = _mm_loadu_si128(cr.add(i) as *const __m128i);
            let gg = _mm_sub_epi32(yv, _mm_srai_epi32(_mm_add_epi32(cbv, crv), 2));
            _mm_storeu_si128(g.add(i) as *mut __m128i, gg);
            _mm_storeu_si128(r.add(i) as *mut __m128i, _mm_add_epi32(crv, gg));
            _mm_storeu_si128(b.add(i) as *mut __m128i, _mm_add_epi32(cbv, gg));
            i += 4;
        }
        while i < n {
            let (yy, cbb, crr) = (*y.add(i), *cb.add(i), *cr.add(i));
            let gv = yy - ((cbb + crr) >> 2);
            *g.add(i) = gv;
            *r.add(i) = crr + gv;
            *b.add(i) = cbb + gv;
            i += 1;
        }
    }
}
