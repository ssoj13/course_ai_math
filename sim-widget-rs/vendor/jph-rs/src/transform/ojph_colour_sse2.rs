//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_colour_sse2.cpp`

use crate::openjph::ojph_defs::{Si64, Ui32};
use crate::openjph::ojph_mem::{LFT_32BIT, LFT_INTEGER, LineBuf};
use crate::transform::ojph_transform_sse2::sse2_mm_srai_epi64;

#[cfg(target_arch = "x86")]
#[allow(deprecated)] // _mm_getcsr/_mm_setcsr: needed for MXCSR rounding mode control
use core::arch::x86::{
    __m128, __m128i, _mm_add_epi32, _mm_add_epi64, _mm_and_si128, _mm_andnot_si128,
    _mm_castps_si128, _mm_cmpge_ps, _mm_cmpgt_epi32, _mm_cmplt_epi32, _mm_cmplt_ps,
    _mm_cvtepi32_ps, _mm_cvtps_epi32, _mm_getcsr, _mm_load_si128, _mm_loadu_ps, _mm_loadu_si128,
    _mm_mul_ps, _mm_or_si128, _mm_set_epi64x, _mm_set1_epi32, _mm_set1_epi64x, _mm_set1_ps,
    _mm_setcsr, _mm_setzero_si128, _mm_shuffle_epi32, _mm_slli_epi32, _mm_slli_epi64,
    _mm_srai_epi32, _mm_store_si128, _mm_storeu_ps, _mm_storeu_si128, _mm_sub_epi32, _mm_sub_epi64,
    _mm_unpackhi_epi32, _mm_unpacklo_epi32,
};
#[cfg(target_arch = "x86_64")]
#[allow(deprecated)] // _mm_getcsr/_mm_setcsr: needed for MXCSR rounding mode control
use core::arch::x86_64::{
    __m128, __m128i, _mm_add_epi32, _mm_add_epi64, _mm_and_si128, _mm_andnot_si128,
    _mm_castps_si128, _mm_cmpge_ps, _mm_cmpgt_epi32, _mm_cmplt_epi32, _mm_cmplt_ps,
    _mm_cvtepi32_ps, _mm_cvtps_epi32, _mm_getcsr, _mm_load_si128, _mm_loadu_ps, _mm_loadu_si128,
    _mm_mul_ps, _mm_or_si128, _mm_set_epi64x, _mm_set1_epi32, _mm_set1_epi64x, _mm_set1_ps,
    _mm_setcsr, _mm_setzero_si128, _mm_shuffle_epi32, _mm_slli_epi32, _mm_slli_epi64,
    _mm_srai_epi32, _mm_store_si128, _mm_storeu_ps, _mm_storeu_si128, _mm_sub_epi32, _mm_sub_epi64,
    _mm_unpackhi_epi32, _mm_unpacklo_epi32,
};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "sse2")]
unsafe fn ojph_mm_max_ge_epi32(a: __m128i, b: __m128i, x: __m128, y: __m128) -> __m128i {
    let ct = _mm_cmpge_ps(x, y);
    let c = _mm_castps_si128(ct);
    let d = _mm_and_si128(c, a);
    let e = _mm_andnot_si128(c, b);
    _mm_or_si128(d, e)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "sse2")]
unsafe fn ojph_mm_min_lt_epi32(a: __m128i, b: __m128i, x: __m128, y: __m128) -> __m128i {
    let ct = _mm_cmplt_ps(x, y);
    let c = _mm_castps_si128(ct);
    let d = _mm_and_si128(c, a);
    let e = _mm_andnot_si128(c, b);
    _mm_or_si128(d, e)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "sse2")]
unsafe fn sse2_cvtlo_epi32_epi64(a: __m128i, zero: __m128i) -> __m128i {
    let t = _mm_cmplt_epi32(a, zero);
    _mm_unpacklo_epi32(a, t)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "sse2")]
unsafe fn sse2_cvthi_epi32_epi64(a: __m128i, zero: __m128i) -> __m128i {
    let t = _mm_cmplt_epi32(a, zero);
    _mm_unpackhi_epi32(a, t)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_rev_convert(
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
            let sh = _mm_set1_epi32(shift as i32);
            let mut i = ((width + 3) >> 2) as i32;
            while i > 0 {
                let mut s = _mm_loadu_si128(sp as *const __m128i);
                s = _mm_add_epi32(s, sh);
                _mm_storeu_si128(dp as *mut __m128i, s);
                sp = sp.add(4);
                dp = dp.add(4);
                i -= 1;
            }
        } else {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i64_.add(dst_line_offset as usize);
            let zero = _mm_setzero_si128();
            let sh = _mm_set1_epi64x(shift);
            let mut i = ((width + 3) >> 2) as i32;
            while i > 0 {
                let s = _mm_loadu_si128(sp as *const __m128i);
                let mut t = sse2_cvtlo_epi32_epi64(s, zero);
                t = _mm_add_epi64(t, sh);
                _mm_storeu_si128(dp as *mut __m128i, t);
                t = sse2_cvthi_epi32_epi64(s, zero);
                t = _mm_add_epi64(t, sh);
                _mm_storeu_si128(dp.add(2) as *mut __m128i, t);
                sp = sp.add(4);
                dp = dp.add(4);
                i -= 1;
            }
        }
    } else {
        let mut sp = (*src_line).storage.i64_.add(src_line_offset as usize);
        let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
        let low_bits = _mm_set_epi64x(0, -1);
        let sh = _mm_set1_epi64x(shift);
        let mut i = ((width + 3) >> 2) as i32;
        while i > 0 {
            let mut s = _mm_loadu_si128(sp as *const __m128i);
            s = _mm_add_epi64(s, sh);
            let mut t = _mm_shuffle_epi32(s, 0b00_00_10_00);
            t = _mm_and_si128(low_bits, t);
            s = _mm_loadu_si128(sp.add(2) as *const __m128i);
            s = _mm_add_epi64(s, sh);
            s = _mm_shuffle_epi32(s, 0b10_00_00_00);
            s = _mm_andnot_si128(low_bits, s);
            t = _mm_or_si128(s, t);
            _mm_storeu_si128(dp as *mut __m128i, t);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 1;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_rev_convert_nlt_type3(
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
            let sh = _mm_set1_epi32((-shift) as i32);
            let zero = _mm_setzero_si128();
            let mut i = ((width + 3) >> 2) as i32;
            while i > 0 {
                let mut s = _mm_loadu_si128(sp as *const __m128i);
                let c = _mm_cmplt_epi32(s, zero);
                let mut v_m_sh = _mm_sub_epi32(sh, s);
                v_m_sh = _mm_and_si128(c, v_m_sh);
                s = _mm_andnot_si128(c, s);
                s = _mm_or_si128(s, v_m_sh);
                _mm_storeu_si128(dp as *mut __m128i, s);
                sp = sp.add(4);
                dp = dp.add(4);
                i -= 1;
            }
        } else {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i64_.add(dst_line_offset as usize);
            let sh = _mm_set1_epi64x(-shift);
            let zero = _mm_setzero_si128();
            let mut i = ((width + 3) >> 2) as i32;
            while i > 0 {
                let s = _mm_loadu_si128(sp as *const __m128i);
                let t = _mm_cmplt_epi32(s, zero);
                let mut u = _mm_unpacklo_epi32(s, t);
                let mut c = _mm_unpacklo_epi32(t, t);
                let mut v_m_sh = _mm_sub_epi64(sh, u);
                v_m_sh = _mm_and_si128(c, v_m_sh);
                u = _mm_andnot_si128(c, u);
                u = _mm_or_si128(u, v_m_sh);
                _mm_storeu_si128(dp as *mut __m128i, u);

                u = _mm_unpackhi_epi32(s, t);
                c = _mm_unpackhi_epi32(t, t);
                v_m_sh = _mm_sub_epi64(sh, u);
                v_m_sh = _mm_and_si128(c, v_m_sh);
                u = _mm_andnot_si128(c, u);
                u = _mm_or_si128(u, v_m_sh);
                _mm_storeu_si128(dp.add(2) as *mut __m128i, u);
                sp = sp.add(4);
                dp = dp.add(4);
                i -= 1;
            }
        }
    } else {
        let mut sp = (*src_line).storage.i64_.add(src_line_offset as usize);
        let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
        let sh = _mm_set1_epi64x(-shift);
        let zero = _mm_setzero_si128();
        let half_mask = _mm_set_epi64x(0, -1);
        let mut i = ((width + 3) >> 2) as i32;
        while i > 0 {
            let mut s = _mm_loadu_si128(sp as *const __m128i);
            let mut tm = _mm_cmplt_epi32(s, zero);
            let mut m = _mm_shuffle_epi32(tm, 0b11_11_01_01);
            tm = _mm_sub_epi64(sh, s);
            let mut n = _mm_and_si128(m, tm);
            let mut p = _mm_andnot_si128(m, s);
            tm = _mm_or_si128(n, p);
            tm = _mm_shuffle_epi32(tm, 0b00_00_10_00);
            let mut t = _mm_and_si128(half_mask, tm);

            s = _mm_loadu_si128(sp.add(2) as *const __m128i);
            tm = _mm_cmplt_epi32(s, zero);
            m = _mm_shuffle_epi32(tm, 0b11_11_01_01);
            tm = _mm_sub_epi64(sh, s);
            n = _mm_and_si128(m, tm);
            p = _mm_andnot_si128(m, s);
            tm = _mm_or_si128(n, p);
            tm = _mm_shuffle_epi32(tm, 0b10_00_00_00);
            tm = _mm_andnot_si128(half_mask, tm);
            t = _mm_or_si128(t, tm);
            _mm_storeu_si128(dp as *mut __m128i, t);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 1;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
#[allow(deprecated)] // _mm_getcsr/_mm_setcsr: matching C++ reference MXCSR save/restore
unsafe fn local_sse2_irv_convert_to_integer<const NLT_TYPE3: bool>(
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

    // _mm_cvtps_epi32 converts using the current MXCSR rounding mode;
    // force nearest to match the C++ reference behaviour and restore afterwards.
    const MM_ROUND_MASK: u32 = 0x6000;
    const MM_ROUND_NEAREST: u32 = 0x0000;
    let saved_rounding_mode = _mm_getcsr() & MM_ROUND_MASK;
    _mm_setcsr((_mm_getcsr() & !MM_ROUND_MASK) | MM_ROUND_NEAREST);

    let mut sp = (*src_line).storage.f32_;
    let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
    let neg_limit = i32::MIN >> (32 - bit_depth);
    let mul = _mm_set1_ps((1u64 << bit_depth) as f32);
    let fl_up_lim = _mm_set1_ps(-(neg_limit as f32));
    let fl_low_lim = _mm_set1_ps(neg_limit as f32);
    let s32_up_lim = _mm_set1_epi32(i32::MAX >> (32 - bit_depth));
    let s32_low_lim = _mm_set1_epi32(i32::MIN >> (32 - bit_depth));

    if is_signed {
        let zero = _mm_setzero_si128();
        let bias = _mm_set1_epi32(-(((1u64 << (bit_depth - 1)) + 1) as i32));
        let mut i = width as i32;
        while i > 0 {
            let mut t = _mm_loadu_ps(sp);
            t = _mm_mul_ps(t, mul);
            let mut u = _mm_cvtps_epi32(t);
            u = ojph_mm_max_ge_epi32(u, s32_low_lim, t, fl_low_lim);
            u = ojph_mm_min_lt_epi32(u, s32_up_lim, t, fl_up_lim);
            if NLT_TYPE3 {
                let c = _mm_cmpgt_epi32(zero, u);
                let mut neg = _mm_sub_epi32(bias, u);
                neg = _mm_and_si128(c, neg);
                u = _mm_andnot_si128(c, u);
                u = _mm_or_si128(neg, u);
            }
            _mm_storeu_si128(dp as *mut __m128i, u);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 4;
        }
    } else {
        let half = _mm_set1_epi32((1u64 << (bit_depth - 1)) as i32);
        let mut i = width as i32;
        while i > 0 {
            let mut t = _mm_loadu_ps(sp);
            t = _mm_mul_ps(t, mul);
            let mut u = _mm_cvtps_epi32(t);
            u = ojph_mm_max_ge_epi32(u, s32_low_lim, t, fl_low_lim);
            u = ojph_mm_min_lt_epi32(u, s32_up_lim, t, fl_up_lim);
            u = _mm_add_epi32(u, half);
            _mm_storeu_si128(dp as *mut __m128i, u);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 4;
        }
    }

    _mm_setcsr((_mm_getcsr() & !MM_ROUND_MASK) | saved_rounding_mode);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_irv_convert_to_integer(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_sse2_irv_convert_to_integer::<false>(
        src_line,
        dst_line,
        dst_line_offset,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_irv_convert_to_integer_nlt_type3(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_sse2_irv_convert_to_integer::<true>(
        src_line,
        dst_line,
        dst_line_offset,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
unsafe fn local_sse2_irv_convert_to_float<const NLT_TYPE3: bool>(
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

    let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
    let mut dp = (*dst_line).storage.f32_;
    let mul = _mm_set1_ps((1.0f64 / (1u64 << bit_depth) as f64) as f32);

    if is_signed {
        let zero = _mm_setzero_si128();
        let bias = _mm_set1_epi32(-(((1u64 << (bit_depth - 1)) + 1) as i32));
        let mut i = width as i32;
        while i > 0 {
            let mut t = _mm_loadu_si128(sp as *const __m128i);
            if NLT_TYPE3 {
                let c = _mm_cmplt_epi32(t, zero);
                let mut neg = _mm_sub_epi32(bias, t);
                neg = _mm_and_si128(c, neg);
                let c2 = _mm_andnot_si128(c, t);
                t = _mm_or_si128(neg, c2);
            }
            let mut v = _mm_cvtepi32_ps(t);
            v = _mm_mul_ps(v, mul);
            _mm_storeu_ps(dp, v);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 4;
        }
    } else {
        let half = _mm_set1_epi32((1u64 << (bit_depth - 1)) as i32);
        let mut i = width as i32;
        while i > 0 {
            let mut t = _mm_loadu_si128(sp as *const __m128i);
            t = _mm_sub_epi32(t, half);
            let mut v = _mm_cvtepi32_ps(t);
            v = _mm_mul_ps(v, mul);
            _mm_storeu_ps(dp, v);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 4;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_irv_convert_to_float(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_sse2_irv_convert_to_float::<false>(
        src_line,
        src_line_offset,
        dst_line,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_irv_convert_to_float_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_sse2_irv_convert_to_float::<true>(
        src_line,
        src_line_offset,
        dst_line,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_rct_forward(
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
        let mut i = ((repeat + 3) >> 2) as i32;
        while i > 0 {
            let mr = _mm_load_si128(rp as *const __m128i);
            let mg = _mm_load_si128(gp as *const __m128i);
            let mb = _mm_load_si128(bp as *const __m128i);
            let mut t = _mm_add_epi32(mr, mb);
            t = _mm_add_epi32(t, _mm_slli_epi32(mg, 1));
            _mm_store_si128(yp as *mut __m128i, _mm_srai_epi32(t, 2));
            t = _mm_sub_epi32(mb, mg);
            _mm_store_si128(cbp as *mut __m128i, t);
            t = _mm_sub_epi32(mr, mg);
            _mm_store_si128(crp as *mut __m128i, t);
            rp = rp.add(4);
            gp = gp.add(4);
            bp = bp.add(4);
            yp = yp.add(4);
            cbp = cbp.add(4);
            crp = crp.add(4);
            i -= 1;
        }
    } else {
        let zero = _mm_setzero_si128();
        let v2 = _mm_set1_epi64x(1i64 << (63 - 2));
        let mut rp = (*r).storage.i32_;
        let mut gp = (*g).storage.i32_;
        let mut bp = (*b).storage.i32_;
        let mut yp = (*y).storage.i64_;
        let mut cbp = (*cb).storage.i64_;
        let mut crp = (*cr).storage.i64_;
        let mut i = ((repeat + 3) >> 2) as i32;
        while i > 0 {
            let mr32 = _mm_load_si128(rp as *const __m128i);
            let mg32 = _mm_load_si128(gp as *const __m128i);
            let mb32 = _mm_load_si128(bp as *const __m128i);

            let mut mr = sse2_cvtlo_epi32_epi64(mr32, zero);
            let mut mg = sse2_cvtlo_epi32_epi64(mg32, zero);
            let mut mb = sse2_cvtlo_epi32_epi64(mb32, zero);
            let mut t = _mm_add_epi64(mr, mb);
            t = _mm_add_epi64(t, _mm_slli_epi64(mg, 1));
            _mm_store_si128(yp as *mut __m128i, sse2_mm_srai_epi64(t, 2, v2));
            t = _mm_sub_epi64(mb, mg);
            _mm_store_si128(cbp as *mut __m128i, t);
            t = _mm_sub_epi64(mr, mg);
            _mm_store_si128(crp as *mut __m128i, t);
            yp = yp.add(2);
            cbp = cbp.add(2);
            crp = crp.add(2);

            mr = sse2_cvthi_epi32_epi64(mr32, zero);
            mg = sse2_cvthi_epi32_epi64(mg32, zero);
            mb = sse2_cvthi_epi32_epi64(mb32, zero);
            t = _mm_add_epi64(mr, mb);
            t = _mm_add_epi64(t, _mm_slli_epi64(mg, 1));
            _mm_store_si128(yp as *mut __m128i, sse2_mm_srai_epi64(t, 2, v2));
            t = _mm_sub_epi64(mb, mg);
            _mm_store_si128(cbp as *mut __m128i, t);
            t = _mm_sub_epi64(mr, mg);
            _mm_store_si128(crp as *mut __m128i, t);
            rp = rp.add(4);
            gp = gp.add(4);
            bp = bp.add(4);
            yp = yp.add(2);
            cbp = cbp.add(2);
            crp = crp.add(2);
            i -= 1;
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_rct_backward(
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
        let mut i = ((repeat + 3) >> 2) as i32;
        while i > 0 {
            let my = _mm_load_si128(yp as *const __m128i);
            let mcb = _mm_load_si128(cbp as *const __m128i);
            let mcr = _mm_load_si128(crp as *const __m128i);
            let mut t = _mm_add_epi32(mcb, mcr);
            t = _mm_sub_epi32(my, _mm_srai_epi32(t, 2));
            _mm_store_si128(gp as *mut __m128i, t);
            let mut u = _mm_add_epi32(mcb, t);
            _mm_store_si128(bp as *mut __m128i, u);
            u = _mm_add_epi32(mcr, t);
            _mm_store_si128(rp as *mut __m128i, u);
            yp = yp.add(4);
            cbp = cbp.add(4);
            crp = crp.add(4);
            rp = rp.add(4);
            gp = gp.add(4);
            bp = bp.add(4);
            i -= 1;
        }
    } else {
        let v2 = _mm_set1_epi64x(1i64 << (63 - 2));
        let low_bits = _mm_set_epi64x(0, -1);
        let mut yp = (*y).storage.i64_;
        let mut cbp = (*cb).storage.i64_;
        let mut crp = (*cr).storage.i64_;
        let mut rp = (*r).storage.i32_;
        let mut gp = (*g).storage.i32_;
        let mut bp = (*b).storage.i32_;
        let mut i = ((repeat + 3) >> 2) as i32;
        while i > 0 {
            let mut my = _mm_load_si128(yp as *const __m128i);
            let mut mcb = _mm_load_si128(cbp as *const __m128i);
            let mut mcr = _mm_load_si128(crp as *const __m128i);
            let mut tg = _mm_add_epi64(mcb, mcr);
            tg = _mm_sub_epi64(my, sse2_mm_srai_epi64(tg, 2, v2));
            let mut tb = _mm_add_epi64(mcb, tg);
            let mut tr = _mm_add_epi64(mcr, tg);

            let mut mr = _mm_shuffle_epi32(tr, 0b00_00_10_00);
            mr = _mm_and_si128(low_bits, mr);
            let mut mg = _mm_shuffle_epi32(tg, 0b00_00_10_00);
            mg = _mm_and_si128(low_bits, mg);
            let mut mb = _mm_shuffle_epi32(tb, 0b00_00_10_00);
            mb = _mm_and_si128(low_bits, mb);

            yp = yp.add(2);
            cbp = cbp.add(2);
            crp = crp.add(2);
            my = _mm_load_si128(yp as *const __m128i);
            mcb = _mm_load_si128(cbp as *const __m128i);
            mcr = _mm_load_si128(crp as *const __m128i);
            tg = _mm_add_epi64(mcb, mcr);
            tg = _mm_sub_epi64(my, sse2_mm_srai_epi64(tg, 2, v2));
            tb = _mm_add_epi64(mcb, tg);
            tr = _mm_add_epi64(mcr, tg);

            tr = _mm_shuffle_epi32(tr, 0b10_00_00_00);
            tr = _mm_andnot_si128(low_bits, tr);
            mr = _mm_or_si128(mr, tr);
            tg = _mm_shuffle_epi32(tg, 0b10_00_00_00);
            tg = _mm_andnot_si128(low_bits, tg);
            mg = _mm_or_si128(mg, tg);
            tb = _mm_shuffle_epi32(tb, 0b10_00_00_00);
            tb = _mm_andnot_si128(low_bits, tb);
            mb = _mm_or_si128(mb, tb);
            _mm_store_si128(rp as *mut __m128i, mr);
            _mm_store_si128(gp as *mut __m128i, mg);
            _mm_store_si128(bp as *mut __m128i, mb);

            yp = yp.add(2);
            cbp = cbp.add(2);
            crp = crp.add(2);
            rp = rp.add(4);
            gp = gp.add(4);
            bp = bp.add(4);
            i -= 1;
        }
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
unsafe fn local_sse2_rev_convert_impl<const NLT_TYPE3: bool>(
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
unsafe fn local_sse2_irv_convert_to_integer_impl<const NLT_TYPE3: bool>(
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
unsafe fn local_sse2_irv_convert_to_float_impl<const NLT_TYPE3: bool>(
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
pub unsafe fn sse2_irv_convert_to_float(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_sse2_irv_convert_to_float_impl::<false>(
        src_line,
        src_line_offset,
        dst_line,
        bit_depth,
        is_signed,
        width,
    );
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn sse2_irv_convert_to_float_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_sse2_irv_convert_to_float_impl::<true>(
        src_line,
        src_line_offset,
        dst_line,
        bit_depth,
        is_signed,
        width,
    );
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn sse2_irv_convert_to_integer(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_sse2_irv_convert_to_integer_impl::<false>(
        src_line,
        dst_line,
        dst_line_offset,
        bit_depth,
        is_signed,
        width,
    );
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn sse2_irv_convert_to_integer_nlt_type3(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_sse2_irv_convert_to_integer_impl::<true>(
        src_line,
        dst_line,
        dst_line_offset,
        bit_depth,
        is_signed,
        width,
    );
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn sse2_rct_forward(
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
pub unsafe fn sse2_rct_backward(
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
pub unsafe fn sse2_rev_convert(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    local_sse2_rev_convert_impl::<false>(
        src_line,
        src_line_offset,
        dst_line,
        dst_line_offset,
        shift,
        width,
    );
}
#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn sse2_rev_convert_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    local_sse2_rev_convert_impl::<true>(
        src_line,
        src_line_offset,
        dst_line,
        dst_line_offset,
        shift,
        width,
    );
}
