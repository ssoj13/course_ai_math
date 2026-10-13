#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// AVX2 impl of [`crate::lincomb3_clip_u8`].  16/iter via `mullo_epi32`,
/// 8-element SSE2 remainder, scalar tail.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
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
        let c0v = _mm256_set1_epi32(c0 as i32);
        let c1v = _mm256_set1_epi32(c1 as i32);
        let c2v = _mm256_set1_epi32(c2 as i32);
        let offv = _mm256_set1_epi32(offset);
        let c0v16 = _mm_set1_epi16(c0);
        let c1v16 = _mm_set1_epi16(c1);
        let c2v16 = _mm_set1_epi16(c2);
        let off128 = _mm_set1_epi32(offset);
        let cnt = _mm_cvtsi32_si128(shift as i32);
        let (c0s, c1s, c2s) = (c0 as i32, c1 as i32, c2 as i32);
        let mut i = 0usize;
        while i + 16 <= n {
            let a16 = _mm256_cvtepu8_epi16(_mm_loadu_si128(p0.add(i) as *const __m128i));
            let b16 = _mm256_cvtepu8_epi16(_mm_loadu_si128(p1.add(i) as *const __m128i));
            let c16 = _mm256_cvtepu8_epi16(_mm_loadu_si128(p2.add(i) as *const __m128i));
            let a_lo = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(a16));
            let a_hi = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(a16));
            let b_lo = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(b16));
            let b_hi = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(b16));
            let c_lo = _mm256_cvtepi16_epi32(_mm256_castsi256_si128(c16));
            let c_hi = _mm256_cvtepi16_epi32(_mm256_extracti128_si256::<1>(c16));
            let acc0 = _mm256_add_epi32(
                _mm256_add_epi32(
                    _mm256_add_epi32(_mm256_mullo_epi32(a_lo, c0v), _mm256_mullo_epi32(b_lo, c1v)),
                    _mm256_mullo_epi32(c_lo, c2v),
                ),
                offv,
            );
            let acc1 = _mm256_add_epi32(
                _mm256_add_epi32(
                    _mm256_add_epi32(_mm256_mullo_epi32(a_hi, c0v), _mm256_mullo_epi32(b_hi, c1v)),
                    _mm256_mullo_epi32(c_hi, c2v),
                ),
                offv,
            );
            let r0 = _mm256_sra_epi32(acc0, cnt);
            let r1 = _mm256_sra_epi32(acc1, cnt);
            let p0_ = _mm_packs_epi32(
                _mm256_castsi256_si128(r0),
                _mm256_extracti128_si256::<1>(r0),
            );
            _mm_storel_epi64(dst.add(i) as *mut __m128i, _mm_packus_epi16(p0_, p0_));
            let p1_ = _mm_packs_epi32(
                _mm256_castsi256_si128(r1),
                _mm256_extracti128_si256::<1>(r1),
            );
            _mm_storel_epi64(dst.add(i + 8) as *mut __m128i, _mm_packus_epi16(p1_, p1_));
            i += 16;
        }
        while i + 8 <= n {
            let a = _mm_unpacklo_epi8(_mm_loadl_epi64(p0.add(i) as *const __m128i), zero);
            let b = _mm_unpacklo_epi8(_mm_loadl_epi64(p1.add(i) as *const __m128i), zero);
            let c = _mm_unpacklo_epi8(_mm_loadl_epi64(p2.add(i) as *const __m128i), zero);
            let pa0 = _mm_mullo_epi16(a, c0v16);
            let pa1 = _mm_mulhi_epi16(a, c0v16);
            let pb0 = _mm_mullo_epi16(b, c1v16);
            let pb1 = _mm_mulhi_epi16(b, c1v16);
            let pc0 = _mm_mullo_epi16(c, c2v16);
            let pc1 = _mm_mulhi_epi16(c, c2v16);
            let lo = _mm_add_epi32(
                _mm_add_epi32(
                    _mm_add_epi32(_mm_unpacklo_epi16(pa0, pa1), _mm_unpacklo_epi16(pb0, pb1)),
                    _mm_unpacklo_epi16(pc0, pc1),
                ),
                off128,
            );
            let hi = _mm_add_epi32(
                _mm_add_epi32(
                    _mm_add_epi32(_mm_unpackhi_epi16(pa0, pa1), _mm_unpackhi_epi16(pb0, pb1)),
                    _mm_unpackhi_epi16(pc0, pc1),
                ),
                off128,
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
            *dst.add(i) = (acc >> shift).clamp(0, 255) as u8;
            i += 1;
        }
    }
}

/// AVX2 impl of [`crate::rct_fwd_i32`].  8 i32/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
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
        while i + 8 <= n {
            let rr = _mm256_loadu_si256(r.add(i) as *const __m256i);
            let gg = _mm256_loadu_si256(g.add(i) as *const __m256i);
            let bb = _mm256_loadu_si256(b.add(i) as *const __m256i);
            let yv = _mm256_srai_epi32(
                _mm256_add_epi32(_mm256_add_epi32(rr, _mm256_slli_epi32(gg, 1)), bb),
                2,
            );
            _mm256_storeu_si256(y.add(i) as *mut __m256i, yv);
            _mm256_storeu_si256(cb.add(i) as *mut __m256i, _mm256_sub_epi32(bb, gg));
            _mm256_storeu_si256(cr.add(i) as *mut __m256i, _mm256_sub_epi32(rr, gg));
            i += 8;
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

/// AVX2 impl of [`crate::rct_bwd_i32`].  8 i32/iter.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "avx2")]
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
        while i + 8 <= n {
            let yv = _mm256_loadu_si256(y.add(i) as *const __m256i);
            let cbv = _mm256_loadu_si256(cb.add(i) as *const __m256i);
            let crv = _mm256_loadu_si256(cr.add(i) as *const __m256i);
            let gg = _mm256_sub_epi32(yv, _mm256_srai_epi32(_mm256_add_epi32(cbv, crv), 2));
            _mm256_storeu_si256(g.add(i) as *mut __m256i, gg);
            _mm256_storeu_si256(r.add(i) as *mut __m256i, _mm256_add_epi32(crv, gg));
            _mm256_storeu_si256(b.add(i) as *mut __m256i, _mm256_add_epi32(cbv, gg));
            i += 8;
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
