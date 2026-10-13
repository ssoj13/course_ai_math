//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_colour_avx2.cpp`

use crate::openjph::ojph_defs::{Si64, Ui32};
use crate::openjph::ojph_mem::{LFT_32BIT, LFT_64BIT, LFT_INTEGER, LineBuf};

#[cfg(target_arch = "x86")]
use core::arch::x86::{
    __m256, __m256i, _CMP_NGE_UQ, _CMP_NLT_UQ, _mm256_add_epi32, _mm256_add_epi64,
    _mm256_and_si256, _mm256_andnot_si256, _mm256_castps_si256, _mm256_cmp_ps, _mm256_cmpgt_epi32,
    _mm256_cmpgt_epi64, _mm256_cvtepi32_epi64, _mm256_cvtepi32_ps, _mm256_cvtps_epi32,
    _mm256_extracti128_si256, _mm256_load_si256, _mm256_loadu_ps, _mm256_loadu_si256,
    _mm256_mul_ps, _mm256_or_si256, _mm256_permute2x128_si256, _mm256_permute4x64_epi64,
    _mm256_set_epi64x, _mm256_set1_epi32, _mm256_set1_epi64x, _mm256_set1_ps, _mm256_setzero_si256,
    _mm256_shuffle_epi32, _mm256_slli_epi32, _mm256_slli_epi64, _mm256_srai_epi32,
    _mm256_srl_epi64, _mm256_store_si256, _mm256_storeu_ps, _mm256_storeu_si256, _mm256_sub_epi32,
    _mm256_sub_epi64, _mm256_unpackhi_epi32, _mm256_unpacklo_epi32, _mm256_xor_si256,
};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{
    __m256, __m256i, _CMP_NGE_UQ, _CMP_NLT_UQ, _mm_cvtsi32_si128, _mm256_add_epi32,
    _mm256_add_epi64, _mm256_and_si256, _mm256_andnot_si256, _mm256_castps_si256, _mm256_cmp_ps,
    _mm256_cmpgt_epi32, _mm256_cmpgt_epi64, _mm256_cvtepi32_epi64, _mm256_cvtepi32_ps,
    _mm256_cvtps_epi32, _mm256_extracti128_si256, _mm256_load_si256, _mm256_loadu_ps,
    _mm256_loadu_si256, _mm256_mul_ps, _mm256_or_si256, _mm256_permute2x128_si256,
    _mm256_permute4x64_epi64, _mm256_set_epi64x, _mm256_set1_epi32, _mm256_set1_epi64x,
    _mm256_set1_ps, _mm256_setzero_si256, _mm256_shuffle_epi32, _mm256_slli_epi32,
    _mm256_slli_epi64, _mm256_srai_epi32, _mm256_srl_epi64, _mm256_store_si256, _mm256_storeu_ps,
    _mm256_storeu_si256, _mm256_sub_epi32, _mm256_sub_epi64, _mm256_unpackhi_epi32,
    _mm256_unpacklo_epi32, _mm256_xor_si256,
};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn avx2_mm256_srai_epi64(a: __m256i, amt: i32, m: __m256i) -> __m256i {
    let mut x = _mm256_srl_epi64(a, _mm_cvtsi32_si128(amt));
    x = _mm256_xor_si256(x, m);
    _mm256_sub_epi64(x, m)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn ojph_mm256_max_ge_epi32(a: __m256i, b: __m256i, x: __m256, y: __m256) -> __m256i {
    let ct = _mm256_cmp_ps(x, y, _CMP_NLT_UQ);
    let c = _mm256_castps_si256(ct);
    let d = _mm256_and_si256(c, a);
    let e = _mm256_andnot_si256(c, b);
    _mm256_or_si256(d, e)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn ojph_mm256_min_lt_epi32(a: __m256i, b: __m256i, x: __m256, y: __m256) -> __m256i {
    let ct = _mm256_cmp_ps(x, y, _CMP_NGE_UQ);
    let c = _mm256_castps_si256(ct);
    let d = _mm256_and_si256(c, a);
    let e = _mm256_andnot_si256(c, b);
    _mm256_or_si256(d, e)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_rev_convert(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    if (*src_line).flags & LFT_32BIT != 0 {
        if (*dst_line).flags & LFT_32BIT != 0 {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
            let sh = _mm256_set1_epi32(shift as i32);
            let mut i = ((width + 7) >> 3) as i32;
            while i > 0 {
                let mut s = _mm256_loadu_si256(sp as *const __m256i);
                s = _mm256_add_epi32(s, sh);
                _mm256_storeu_si256(dp as *mut __m256i, s);
                sp = sp.add(8);
                dp = dp.add(8);
                i -= 1;
            }
        } else {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i64_.add(dst_line_offset as usize);
            let sh = _mm256_set1_epi64x(shift);
            let mut i = ((width + 7) >> 3) as i32;
            while i > 0 {
                let s = _mm256_loadu_si256(sp as *const __m256i);

                let mut t = _mm256_cvtepi32_epi64(_mm256_extracti128_si256(s, 0));
                t = _mm256_add_epi64(t, sh);
                _mm256_storeu_si256(dp as *mut __m256i, t);

                t = _mm256_cvtepi32_epi64(_mm256_extracti128_si256(s, 1));
                t = _mm256_add_epi64(t, sh);
                _mm256_storeu_si256(dp.add(4) as *mut __m256i, t);

                sp = sp.add(8);
                dp = dp.add(8);
                i -= 1;
            }
        }
    } else {
        debug_assert!((*src_line).flags & LFT_64BIT != 0);
        debug_assert!((*dst_line).flags & LFT_32BIT != 0);
        let mut sp = (*src_line).storage.i64_.add(src_line_offset as usize);
        let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
        let low_bits = _mm256_set_epi64x(0, -1, 0, -1);
        let sh = _mm256_set1_epi64x(shift);
        let mut i = ((width + 7) >> 3) as i32;
        while i > 0 {
            let mut s = _mm256_loadu_si256(sp as *const __m256i);
            s = _mm256_add_epi64(s, sh);

            let mut t = _mm256_shuffle_epi32(s, 0b00_00_10_00);
            t = _mm256_and_si256(low_bits, t);

            s = _mm256_loadu_si256(sp.add(4) as *const __m256i);
            s = _mm256_add_epi64(s, sh);
            s = _mm256_shuffle_epi32(s, 0b10_00_00_00);
            s = _mm256_andnot_si256(low_bits, s);

            t = _mm256_or_si256(s, t);
            t = _mm256_permute4x64_epi64(t, 0b11_01_10_00);
            _mm256_storeu_si256(dp as *mut __m256i, t);

            sp = sp.add(8);
            dp = dp.add(8);
            i -= 1;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_rev_convert_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    if (*src_line).flags & LFT_32BIT != 0 {
        if (*dst_line).flags & LFT_32BIT != 0 {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
            let sh = _mm256_set1_epi32((-shift) as i32);
            let zero = _mm256_setzero_si256();
            let mut i = ((width + 7) >> 3) as i32;
            while i > 0 {
                let mut s = _mm256_loadu_si256(sp as *const __m256i);
                let c = _mm256_cmpgt_epi32(zero, s);
                let mut v_m_sh = _mm256_sub_epi32(sh, s);
                v_m_sh = _mm256_and_si256(c, v_m_sh);
                s = _mm256_andnot_si256(c, s);
                s = _mm256_or_si256(s, v_m_sh);
                _mm256_storeu_si256(dp as *mut __m256i, s);
                sp = sp.add(8);
                dp = dp.add(8);
                i -= 1;
            }
        } else {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i64_.add(dst_line_offset as usize);
            let sh = _mm256_set1_epi64x(-shift);
            let zero = _mm256_setzero_si256();
            let mut i = ((width + 7) >> 3) as i32;
            while i > 0 {
                let s = _mm256_loadu_si256(sp as *const __m256i);

                let t = _mm256_cmpgt_epi32(zero, s);
                let mut u0 = _mm256_unpacklo_epi32(s, t);
                let mut c = _mm256_unpacklo_epi32(t, t);
                let mut v_m_sh = _mm256_sub_epi64(sh, u0);
                v_m_sh = _mm256_and_si256(c, v_m_sh);
                u0 = _mm256_andnot_si256(c, u0);
                u0 = _mm256_or_si256(u0, v_m_sh);

                let mut u1 = _mm256_unpackhi_epi32(s, t);
                c = _mm256_unpackhi_epi32(t, t);
                v_m_sh = _mm256_sub_epi64(sh, u1);
                v_m_sh = _mm256_and_si256(c, v_m_sh);
                u1 = _mm256_andnot_si256(c, u1);
                u1 = _mm256_or_si256(u1, v_m_sh);

                let mut t2 = _mm256_permute2x128_si256(u0, u1, (2 << 4) | 0);
                _mm256_storeu_si256(dp as *mut __m256i, t2);
                t2 = _mm256_permute2x128_si256(u0, u1, (3 << 4) | 1);
                _mm256_storeu_si256(dp.add(4) as *mut __m256i, t2);

                sp = sp.add(8);
                dp = dp.add(8);
                i -= 1;
            }
        }
    } else {
        debug_assert!((*src_line).flags & LFT_64BIT != 0);
        debug_assert!((*dst_line).flags & LFT_32BIT != 0);
        let mut sp = (*src_line).storage.i64_.add(src_line_offset as usize);
        let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
        let sh = _mm256_set1_epi64x(-shift);
        let zero = _mm256_setzero_si256();
        let half_mask = _mm256_set_epi64x(0, -1, 0, -1);
        let mut i = ((width + 7) >> 3) as i32;
        while i > 0 {
            let mut s = _mm256_loadu_si256(sp as *const __m256i);
            let mut m = _mm256_cmpgt_epi64(zero, s);
            let mut tm = _mm256_sub_epi64(sh, s);
            let mut n = _mm256_and_si256(m, tm);
            let mut p = _mm256_andnot_si256(m, s);
            tm = _mm256_or_si256(n, p);
            tm = _mm256_shuffle_epi32(tm, 0b00_00_10_00);
            let mut t = _mm256_and_si256(half_mask, tm);

            s = _mm256_loadu_si256(sp.add(4) as *const __m256i);
            m = _mm256_cmpgt_epi64(zero, s);
            tm = _mm256_sub_epi64(sh, s);
            n = _mm256_and_si256(m, tm);
            p = _mm256_andnot_si256(m, s);
            tm = _mm256_or_si256(n, p);
            tm = _mm256_shuffle_epi32(tm, 0b10_00_00_00);
            tm = _mm256_andnot_si256(half_mask, tm);

            t = _mm256_or_si256(t, tm);
            t = _mm256_permute4x64_epi64(t, 0b11_01_10_00);
            _mm256_storeu_si256(dp as *mut __m256i, t);

            sp = sp.add(8);
            dp = dp.add(8);
            i -= 1;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn local_avx2_irv_convert_to_integer<const NLT_TYPE3: bool>(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    debug_assert!((*src_line).flags & LFT_32BIT != 0);
    debug_assert!((*src_line).flags & LFT_INTEGER == 0);
    debug_assert!((*dst_line).flags & LFT_32BIT != 0);
    debug_assert!((*dst_line).flags & LFT_INTEGER != 0);

    let mut sp = (*src_line).storage.f32_;
    let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
    let neg_limit = i32::MIN >> (32 - bit_depth);
    let mul = _mm256_set1_ps((1u64 << bit_depth) as f32);
    let fl_up_lim = _mm256_set1_ps(-(neg_limit as f32));
    let fl_low_lim = _mm256_set1_ps(neg_limit as f32);
    let s32_up_lim = _mm256_set1_epi32(i32::MAX >> (32 - bit_depth));
    let s32_low_lim = _mm256_set1_epi32(i32::MIN >> (32 - bit_depth));

    if is_signed {
        let zero = _mm256_setzero_si256();
        let bias = _mm256_set1_epi32(-(((1u64 << (bit_depth - 1)) + 1) as i32));
        let mut i = width as i32;
        while i > 0 {
            let mut t = _mm256_loadu_ps(sp);
            t = _mm256_mul_ps(t, mul);
            let mut u = _mm256_cvtps_epi32(t);
            u = ojph_mm256_max_ge_epi32(u, s32_low_lim, t, fl_low_lim);
            u = ojph_mm256_min_lt_epi32(u, s32_up_lim, t, fl_up_lim);
            if NLT_TYPE3 {
                let c = _mm256_cmpgt_epi32(zero, u);
                let mut neg = _mm256_sub_epi32(bias, u);
                neg = _mm256_and_si256(c, neg);
                u = _mm256_andnot_si256(c, u);
                u = _mm256_or_si256(neg, u);
            }
            _mm256_storeu_si256(dp as *mut __m256i, u);
            sp = sp.add(8);
            dp = dp.add(8);
            i -= 8;
        }
    } else {
        let half = _mm256_set1_epi32((1u64 << (bit_depth - 1)) as i32);
        let mut i = width as i32;
        while i > 0 {
            let mut t = _mm256_loadu_ps(sp);
            t = _mm256_mul_ps(t, mul);
            let mut u = _mm256_cvtps_epi32(t);
            u = ojph_mm256_max_ge_epi32(u, s32_low_lim, t, fl_low_lim);
            u = ojph_mm256_min_lt_epi32(u, s32_up_lim, t, fl_up_lim);
            u = _mm256_add_epi32(u, half);
            _mm256_storeu_si256(dp as *mut __m256i, u);
            sp = sp.add(8);
            dp = dp.add(8);
            i -= 8;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_irv_convert_to_integer(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_avx2_irv_convert_to_integer::<false>(
        src_line,
        dst_line,
        dst_line_offset,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_irv_convert_to_integer_nlt_type3(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_avx2_irv_convert_to_integer::<true>(
        src_line,
        dst_line,
        dst_line_offset,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn local_avx2_irv_convert_to_float<const NLT_TYPE3: bool>(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    debug_assert!((*src_line).flags & LFT_32BIT != 0);
    debug_assert!((*src_line).flags & LFT_INTEGER != 0);
    debug_assert!((*dst_line).flags & LFT_32BIT != 0);
    debug_assert!((*dst_line).flags & LFT_INTEGER == 0);

    let mul = _mm256_set1_ps((1.0 / (1u64 << bit_depth) as f64) as f32);
    let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
    let mut dp = (*dst_line).storage.f32_;

    if is_signed {
        let zero = _mm256_setzero_si256();
        let bias = _mm256_set1_epi32(-(((1u64 << (bit_depth - 1)) + 1) as i32));
        let mut i = width as i32;
        while i > 0 {
            let mut t = _mm256_loadu_si256(sp as *const __m256i);
            if NLT_TYPE3 {
                let c = _mm256_cmpgt_epi32(zero, t);
                let mut neg = _mm256_sub_epi32(bias, t);
                neg = _mm256_and_si256(c, neg);
                let pos = _mm256_andnot_si256(c, t);
                t = _mm256_or_si256(neg, pos);
            }
            let mut v = _mm256_cvtepi32_ps(t);
            v = _mm256_mul_ps(v, mul);
            _mm256_storeu_ps(dp, v);
            sp = sp.add(8);
            dp = dp.add(8);
            i -= 8;
        }
    } else {
        let half = _mm256_set1_epi32((1u64 << (bit_depth - 1)) as i32);
        let mut i = width as i32;
        while i > 0 {
            let mut t = _mm256_loadu_si256(sp as *const __m256i);
            t = _mm256_sub_epi32(t, half);
            let mut v = _mm256_cvtepi32_ps(t);
            v = _mm256_mul_ps(v, mul);
            _mm256_storeu_ps(dp, v);
            sp = sp.add(8);
            dp = dp.add(8);
            i -= 8;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_irv_convert_to_float(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_avx2_irv_convert_to_float::<false>(
        src_line,
        src_line_offset,
        dst_line,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_irv_convert_to_float_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_avx2_irv_convert_to_float::<true>(
        src_line,
        src_line_offset,
        dst_line,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_rct_forward(
    r: *const LineBuf,
    g: *const LineBuf,
    b: *const LineBuf,
    y: *mut LineBuf,
    cb: *mut LineBuf,
    cr: *mut LineBuf,
    repeat: Ui32,
) {
    debug_assert!((*y).flags & LFT_INTEGER != 0);
    debug_assert!((*cb).flags & LFT_INTEGER != 0);
    debug_assert!((*cr).flags & LFT_INTEGER != 0);
    debug_assert!((*r).flags & LFT_INTEGER != 0);
    debug_assert!((*g).flags & LFT_INTEGER != 0);
    debug_assert!((*b).flags & LFT_INTEGER != 0);

    if (*y).flags & LFT_32BIT != 0 {
        debug_assert!((*cb).flags & LFT_32BIT != 0);
        debug_assert!((*cr).flags & LFT_32BIT != 0);
        debug_assert!((*r).flags & LFT_32BIT != 0);
        debug_assert!((*g).flags & LFT_32BIT != 0);
        debug_assert!((*b).flags & LFT_32BIT != 0);
        let mut rp = (*r).storage.i32_ as *const i32;
        let mut gp = (*g).storage.i32_ as *const i32;
        let mut bp = (*b).storage.i32_ as *const i32;
        let mut yp = (*y).storage.i32_;
        let mut cbp = (*cb).storage.i32_;
        let mut crp = (*cr).storage.i32_;
        let mut i = ((repeat + 7) >> 3) as i32;
        while i > 0 {
            let mr = _mm256_load_si256(rp as *const __m256i);
            let mg = _mm256_load_si256(gp as *const __m256i);
            let mb = _mm256_load_si256(bp as *const __m256i);
            let mut t = _mm256_add_epi32(mr, mb);
            t = _mm256_add_epi32(t, _mm256_slli_epi32(mg, 1));
            _mm256_store_si256(yp as *mut __m256i, _mm256_srai_epi32(t, 2));
            t = _mm256_sub_epi32(mb, mg);
            _mm256_store_si256(cbp as *mut __m256i, t);
            t = _mm256_sub_epi32(mr, mg);
            _mm256_store_si256(crp as *mut __m256i, t);

            rp = rp.add(8);
            gp = gp.add(8);
            bp = bp.add(8);
            yp = yp.add(8);
            cbp = cbp.add(8);
            crp = crp.add(8);
            i -= 1;
        }
    } else {
        debug_assert!((*y).flags & LFT_64BIT != 0);
        debug_assert!((*cb).flags & LFT_64BIT != 0);
        debug_assert!((*cr).flags & LFT_64BIT != 0);
        debug_assert!((*r).flags & LFT_32BIT != 0);
        debug_assert!((*g).flags & LFT_32BIT != 0);
        debug_assert!((*b).flags & LFT_32BIT != 0);
        let v2 = _mm256_set1_epi64x(1u64.wrapping_shl(61) as i64);
        let mut rp = (*r).storage.i32_ as *const i32;
        let mut gp = (*g).storage.i32_ as *const i32;
        let mut bp = (*b).storage.i32_ as *const i32;
        let mut yp = (*y).storage.i64_;
        let mut cbp = (*cb).storage.i64_;
        let mut crp = (*cr).storage.i64_;
        let mut i = ((repeat + 7) >> 3) as i32;
        while i > 0 {
            let mr32 = _mm256_load_si256(rp as *const __m256i);
            let mg32 = _mm256_load_si256(gp as *const __m256i);
            let mb32 = _mm256_load_si256(bp as *const __m256i);

            let mut mr = _mm256_cvtepi32_epi64(_mm256_extracti128_si256(mr32, 0));
            let mut mg = _mm256_cvtepi32_epi64(_mm256_extracti128_si256(mg32, 0));
            let mut mb = _mm256_cvtepi32_epi64(_mm256_extracti128_si256(mb32, 0));

            let mut t = _mm256_add_epi64(mr, mb);
            t = _mm256_add_epi64(t, _mm256_slli_epi64(mg, 1));
            _mm256_store_si256(yp as *mut __m256i, avx2_mm256_srai_epi64(t, 2, v2));
            t = _mm256_sub_epi64(mb, mg);
            _mm256_store_si256(cbp as *mut __m256i, t);
            t = _mm256_sub_epi64(mr, mg);
            _mm256_store_si256(crp as *mut __m256i, t);

            yp = yp.add(4);
            cbp = cbp.add(4);
            crp = crp.add(4);

            mr = _mm256_cvtepi32_epi64(_mm256_extracti128_si256(mr32, 1));
            mg = _mm256_cvtepi32_epi64(_mm256_extracti128_si256(mg32, 1));
            mb = _mm256_cvtepi32_epi64(_mm256_extracti128_si256(mb32, 1));

            t = _mm256_add_epi64(mr, mb);
            t = _mm256_add_epi64(t, _mm256_slli_epi64(mg, 1));
            _mm256_store_si256(yp as *mut __m256i, avx2_mm256_srai_epi64(t, 2, v2));
            t = _mm256_sub_epi64(mb, mg);
            _mm256_store_si256(cbp as *mut __m256i, t);
            t = _mm256_sub_epi64(mr, mg);
            _mm256_store_si256(crp as *mut __m256i, t);

            rp = rp.add(8);
            gp = gp.add(8);
            bp = bp.add(8);
            yp = yp.add(4);
            cbp = cbp.add(4);
            crp = crp.add(4);
            i -= 1;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_rct_backward(
    y: *const LineBuf,
    cb: *const LineBuf,
    cr: *const LineBuf,
    r: *mut LineBuf,
    g: *mut LineBuf,
    b: *mut LineBuf,
    repeat: Ui32,
) {
    debug_assert!((*y).flags & LFT_INTEGER != 0);
    debug_assert!((*cb).flags & LFT_INTEGER != 0);
    debug_assert!((*cr).flags & LFT_INTEGER != 0);
    debug_assert!((*r).flags & LFT_INTEGER != 0);
    debug_assert!((*g).flags & LFT_INTEGER != 0);
    debug_assert!((*b).flags & LFT_INTEGER != 0);

    if (*y).flags & LFT_32BIT != 0 {
        debug_assert!((*cb).flags & LFT_32BIT != 0);
        debug_assert!((*cr).flags & LFT_32BIT != 0);
        debug_assert!((*r).flags & LFT_32BIT != 0);
        debug_assert!((*g).flags & LFT_32BIT != 0);
        debug_assert!((*b).flags & LFT_32BIT != 0);
        let mut yp = (*y).storage.i32_ as *const i32;
        let mut cbp = (*cb).storage.i32_ as *const i32;
        let mut crp = (*cr).storage.i32_ as *const i32;
        let mut rp = (*r).storage.i32_;
        let mut gp = (*g).storage.i32_;
        let mut bp = (*b).storage.i32_;
        let mut i = ((repeat + 7) >> 3) as i32;
        while i > 0 {
            let my = _mm256_load_si256(yp as *const __m256i);
            let mcb = _mm256_load_si256(cbp as *const __m256i);
            let mcr = _mm256_load_si256(crp as *const __m256i);

            let mut t = _mm256_add_epi32(mcb, mcr);
            t = _mm256_sub_epi32(my, _mm256_srai_epi32(t, 2));
            _mm256_store_si256(gp as *mut __m256i, t);
            let mut u = _mm256_add_epi32(mcb, t);
            _mm256_store_si256(bp as *mut __m256i, u);
            u = _mm256_add_epi32(mcr, t);
            _mm256_store_si256(rp as *mut __m256i, u);

            yp = yp.add(8);
            cbp = cbp.add(8);
            crp = crp.add(8);
            rp = rp.add(8);
            gp = gp.add(8);
            bp = bp.add(8);
            i -= 1;
        }
    } else {
        debug_assert!((*y).flags & LFT_64BIT != 0);
        debug_assert!((*cb).flags & LFT_64BIT != 0);
        debug_assert!((*cr).flags & LFT_64BIT != 0);
        debug_assert!((*r).flags & LFT_32BIT != 0);
        debug_assert!((*g).flags & LFT_32BIT != 0);
        debug_assert!((*b).flags & LFT_32BIT != 0);
        let v2 = _mm256_set1_epi64x(1u64.wrapping_shl(61) as i64);
        let low_bits = _mm256_set_epi64x(0, -1, 0, -1);
        let mut yp = (*y).storage.i64_ as *const i64;
        let mut cbp = (*cb).storage.i64_ as *const i64;
        let mut crp = (*cr).storage.i64_ as *const i64;
        let mut rp = (*r).storage.i32_;
        let mut gp = (*g).storage.i32_;
        let mut bp = (*b).storage.i32_;
        let mut i = ((repeat + 7) >> 3) as i32;
        while i > 0 {
            let my = _mm256_load_si256(yp as *const __m256i);
            let mcb = _mm256_load_si256(cbp as *const __m256i);
            let mcr = _mm256_load_si256(crp as *const __m256i);

            let mut tg = _mm256_add_epi64(mcb, mcr);
            tg = _mm256_sub_epi64(my, avx2_mm256_srai_epi64(tg, 2, v2));
            let mut tb = _mm256_add_epi64(mcb, tg);
            let mut tr = _mm256_add_epi64(mcr, tg);

            let mut mr = _mm256_shuffle_epi32(tr, 0b00_00_10_00);
            mr = _mm256_and_si256(low_bits, mr);
            let mut mg = _mm256_shuffle_epi32(tg, 0b00_00_10_00);
            mg = _mm256_and_si256(low_bits, mg);
            let mut mb = _mm256_shuffle_epi32(tb, 0b00_00_10_00);
            mb = _mm256_and_si256(low_bits, mb);

            yp = yp.add(4);
            cbp = cbp.add(4);
            crp = crp.add(4);

            let my = _mm256_load_si256(yp as *const __m256i);
            let mcb = _mm256_load_si256(cbp as *const __m256i);
            let mcr = _mm256_load_si256(crp as *const __m256i);

            tg = _mm256_add_epi64(mcb, mcr);
            tg = _mm256_sub_epi64(my, avx2_mm256_srai_epi64(tg, 2, v2));
            tb = _mm256_add_epi64(mcb, tg);
            tr = _mm256_add_epi64(mcr, tg);

            tr = _mm256_shuffle_epi32(tr, 0b10_00_00_00);
            tr = _mm256_andnot_si256(low_bits, tr);
            mr = _mm256_or_si256(mr, tr);
            mr = _mm256_permute4x64_epi64(mr, 0b11_01_10_00);

            tg = _mm256_shuffle_epi32(tg, 0b10_00_00_00);
            tg = _mm256_andnot_si256(low_bits, tg);
            mg = _mm256_or_si256(mg, tg);
            mg = _mm256_permute4x64_epi64(mg, 0b11_01_10_00);

            tb = _mm256_shuffle_epi32(tb, 0b10_00_00_00);
            tb = _mm256_andnot_si256(low_bits, tb);
            mb = _mm256_or_si256(mb, tb);
            mb = _mm256_permute4x64_epi64(mb, 0b11_01_10_00);

            _mm256_store_si256(rp as *mut __m256i, mr);
            _mm256_store_si256(gp as *mut __m256i, mg);
            _mm256_store_si256(bp as *mut __m256i, mb);

            yp = yp.add(4);
            cbp = cbp.add(4);
            crp = crp.add(4);
            rp = rp.add(8);
            gp = gp.add(8);
            bp = bp.add(8);
            i -= 1;
        }
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
unsafe fn local_avx2_rev_convert_impl<const NLT_TYPE3: bool>(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    if (*src_line).flags & LFT_32BIT != 0 {
        if (*dst_line).flags & LFT_32BIT != 0 {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
            let s = shift as Si32;
            for _ in 0..width {
                let v = *sp;
                *dp = if NLT_TYPE3 {
                    if v >= 0 { v } else { -v - s }
                } else {
                    v + s
                };
                dp = dp.add(1);
                sp = sp.add(1);
            }
        } else {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i64_.add(dst_line_offset as usize);
            for _ in 0..width {
                let v = *sp as Si64;
                *dp = if NLT_TYPE3 {
                    if v >= 0 { v } else { -v - shift }
                } else {
                    v + shift
                };
                dp = dp.add(1);
                sp = sp.add(1);
            }
        }
    } else {
        debug_assert!((*src_line).flags & LFT_64BIT != 0);
        debug_assert!((*dst_line).flags & LFT_32BIT != 0);
        let mut sp = (*src_line).storage.i64_.add(src_line_offset as usize);
        let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
        for _ in 0..width {
            let v = *sp;
            *dp = if NLT_TYPE3 {
                if v >= 0 {
                    v as Si32
                } else {
                    (-v - shift) as Si32
                }
            } else {
                (*sp + shift) as Si32
            };
            dp = dp.add(1);
            sp = sp.add(1);
        }
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
unsafe fn local_avx2_irv_convert_to_integer_impl<const NLT_TYPE3: bool>(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    let mut sp = (*src_line).storage.f32_;
    let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
    let neg_limit = i32::MIN >> (32 - bit_depth);
    let mul = (1u64 << bit_depth) as f32;
    let fl_up_lim = -(neg_limit as f32);
    let fl_low_lim = neg_limit as f32;
    let s32_up_lim = i32::MAX >> (32 - bit_depth);
    let s32_low_lim = i32::MIN >> (32 - bit_depth);
    if is_signed {
        let bias = ((1u64 << (bit_depth - 1)) + 1) as Si32;
        for _ in 0..width {
            let t = *sp * mul;
            let mut v = ojph_round(t);
            v = if t >= fl_low_lim { v } else { s32_low_lim };
            v = if t < fl_up_lim { v } else { s32_up_lim };
            if NLT_TYPE3 {
                v = if v >= 0 { v } else { -v - bias };
            }
            *dp = v;
            dp = dp.add(1);
            sp = sp.add(1);
        }
    } else {
        let half = (1u64 << (bit_depth - 1)) as Si32;
        for _ in 0..width {
            let t = *sp * mul;
            let mut v = ojph_round(t);
            v = if t >= fl_low_lim { v } else { s32_low_lim };
            v = if t < fl_up_lim { v } else { s32_up_lim };
            *dp = v + half;
            dp = dp.add(1);
            sp = sp.add(1);
        }
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
unsafe fn local_avx2_irv_convert_to_float_impl<const NLT_TYPE3: bool>(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    let mul = (1.0f64 / ((1u64 << bit_depth) as f64)) as f32;
    let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
    let mut dp = (*dst_line).storage.f32_;
    if is_signed {
        let bias = ((1u64 << (bit_depth - 1)) + 1) as Si32;
        for _ in 0..width {
            let mut v = *sp;
            if NLT_TYPE3 {
                v = if v >= 0 { v } else { -v - bias };
            }
            *dp = v as f32 * mul;
            dp = dp.add(1);
            sp = sp.add(1);
        }
    } else {
        let half = (1u64 << (bit_depth - 1)) as Si32;
        for _ in 0..width {
            *dp = (*sp - half) as f32 * mul;
            dp = dp.add(1);
            sp = sp.add(1);
        }
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx2_irv_convert_to_float(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_avx2_irv_convert_to_float_impl::<false>(
        src_line,
        src_line_offset,
        dst_line,
        bit_depth,
        is_signed,
        width,
    );
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx2_irv_convert_to_float_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_avx2_irv_convert_to_float_impl::<true>(
        src_line,
        src_line_offset,
        dst_line,
        bit_depth,
        is_signed,
        width,
    );
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx2_irv_convert_to_integer(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_avx2_irv_convert_to_integer_impl::<false>(
        src_line,
        dst_line,
        dst_line_offset,
        bit_depth,
        is_signed,
        width,
    );
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx2_irv_convert_to_integer_nlt_type3(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_avx2_irv_convert_to_integer_impl::<true>(
        src_line,
        dst_line,
        dst_line_offset,
        bit_depth,
        is_signed,
        width,
    );
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx2_rct_forward(
    r: *const LineBuf,
    g: *const LineBuf,
    b: *const LineBuf,
    y: *mut LineBuf,
    cb: *mut LineBuf,
    cr: *mut LineBuf,
    repeat: Ui32,
) {
    if (*y).flags & LFT_32BIT != 0 {
        let mut rp = (*r).storage.i32_;
        let mut gp = (*g).storage.i32_;
        let mut bp = (*b).storage.i32_;
        let mut yp = (*y).storage.i32_;
        let mut cbp = (*cb).storage.i32_;
        let mut crp = (*cr).storage.i32_;
        for _ in 0..repeat {
            let rr = *rp;
            let gg = *gp;
            let bb = *bp;
            *yp = (rr + (gg << 1) + bb) >> 2;
            *cbp = bb - gg;
            *crp = rr - gg;
            rp = rp.add(1);
            gp = gp.add(1);
            bp = bp.add(1);
            yp = yp.add(1);
            cbp = cbp.add(1);
            crp = crp.add(1);
        }
    } else {
        let mut rp = (*r).storage.i32_;
        let mut gp = (*g).storage.i32_;
        let mut bp = (*b).storage.i32_;
        let mut yp = (*y).storage.i64_;
        let mut cbp = (*cb).storage.i64_;
        let mut crp = (*cr).storage.i64_;
        for _ in 0..repeat {
            let rr = *rp as Si64;
            let gg = *gp as Si64;
            let bb = *bp as Si64;
            *yp = (rr + (gg << 1) + bb) >> 2;
            *cbp = bb - gg;
            *crp = rr - gg;
            rp = rp.add(1);
            gp = gp.add(1);
            bp = bp.add(1);
            yp = yp.add(1);
            cbp = cbp.add(1);
            crp = crp.add(1);
        }
    }
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx2_rct_backward(
    y: *const LineBuf,
    cb: *const LineBuf,
    cr: *const LineBuf,
    r: *mut LineBuf,
    g: *mut LineBuf,
    b: *mut LineBuf,
    repeat: Ui32,
) {
    if (*y).flags & LFT_32BIT != 0 {
        let mut yp = (*y).storage.i32_;
        let mut cbp = (*cb).storage.i32_;
        let mut crp = (*cr).storage.i32_;
        let mut rp = (*r).storage.i32_;
        let mut gp = (*g).storage.i32_;
        let mut bp = (*b).storage.i32_;
        for _ in 0..repeat {
            let yy = *yp;
            let cbb = *cbp;
            let crr = *crp;
            let gg = yy - ((cbb + crr) >> 2);
            *rp = crr + gg;
            *gp = gg;
            *bp = cbb + gg;
            yp = yp.add(1);
            cbp = cbp.add(1);
            crp = crp.add(1);
            rp = rp.add(1);
            gp = gp.add(1);
            bp = bp.add(1);
        }
    } else {
        let mut yp = (*y).storage.i64_;
        let mut cbp = (*cb).storage.i64_;
        let mut crp = (*cr).storage.i64_;
        let mut rp = (*r).storage.i32_;
        let mut gp = (*g).storage.i32_;
        let mut bp = (*b).storage.i32_;
        for _ in 0..repeat {
            let yy = *yp;
            let cbb = *cbp;
            let crr = *crp;
            let gg = yy - ((cbb + crr) >> 2);
            *rp = (crr + gg) as Si32;
            *gp = gg as Si32;
            *bp = (cbb + gg) as Si32;
            yp = yp.add(1);
            cbp = cbp.add(1);
            crp = crp.add(1);
            rp = rp.add(1);
            gp = gp.add(1);
            bp = bp.add(1);
        }
    }
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx2_rev_convert(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    local_avx2_rev_convert_impl::<false>(
        src_line,
        src_line_offset,
        dst_line,
        dst_line_offset,
        shift,
        width,
    );
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx2_rev_convert_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    local_avx2_rev_convert_impl::<true>(
        src_line,
        src_line_offset,
        dst_line,
        dst_line_offset,
        shift,
        width,
    );
}
