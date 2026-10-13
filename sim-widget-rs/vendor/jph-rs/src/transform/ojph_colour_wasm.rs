//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_colour_wasm.cpp`

use crate::openjph::ojph_arch::ojph_round;
use crate::openjph::ojph_defs::{Si32, Si64, Ui32};
use crate::openjph::ojph_mem::{LFT_32BIT, LFT_64BIT, LFT_INTEGER, LineBuf};
use crate::transform::ojph_colour_local::CT_CNST;

#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::{
    f32x4_add, f32x4_convert_i32x4, f32x4_ge, f32x4_lt, f32x4_mul, f32x4_splat, f32x4_sub,
    i32x4_add, i32x4_gt, i32x4_lt, i32x4_shl, i32x4_shr, i32x4_shuffle, i32x4_splat, i32x4_sub,
    i32x4_trunc_sat_f32x4, i64x2_add, i64x2_extend_high_i32x4, i64x2_extend_low_i32x4, i64x2_lt,
    i64x2_shl, i64x2_shr, i64x2_splat, i64x2_sub, v128, v128_and, v128_andnot, v128_load, v128_or,
    v128_store,
};

#[cfg(target_arch = "wasm32")]
#[inline]
pub unsafe fn ojph_convert_float_to_i32(a: v128, zero: v128, half: v128) -> v128 {
    let c = f32x4_ge(a, zero);
    let p = f32x4_add(a, half);
    let n = f32x4_sub(a, half);
    let d = v128_and(c, p);
    let e = v128_andnot(n, c);
    let v = v128_or(d, e);
    i32x4_trunc_sat_f32x4(v)
}

#[cfg(target_arch = "wasm32")]
#[inline]
unsafe fn ojph_wasm_i32x4_max_ge(a: v128, b: v128, x: v128, y: v128) -> v128 {
    let c = f32x4_ge(x, y);
    let d = v128_and(c, a);
    let e = v128_andnot(b, c);
    v128_or(d, e)
}

#[cfg(target_arch = "wasm32")]
#[inline]
unsafe fn ojph_wasm_i32x4_min_lt(a: v128, b: v128, x: v128, y: v128) -> v128 {
    let c = f32x4_lt(x, y);
    let d = v128_and(c, a);
    let e = v128_andnot(b, c);
    v128_or(d, e)
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_rev_convert(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    if (*src_line).flags & crate::openjph::ojph_mem::LFT_32BIT != 0 {
        if (*dst_line).flags & crate::openjph::ojph_mem::LFT_32BIT != 0 {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
            let sh = i32x4_splat(shift as Si32);
            let mut i = ((width + 3) >> 2) as i32;
            while i > 0 {
                let mut s = v128_load(sp as *const v128);
                s = i32x4_add(s, sh);
                v128_store(dp as *mut v128, s);
                sp = sp.add(4);
                dp = dp.add(4);
                i -= 1;
            }
        } else {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i64_.add(dst_line_offset as usize);
            let sh = i64x2_splat(shift);
            let mut i = ((width + 3) >> 2) as i32;
            while i > 0 {
                let s = v128_load(sp as *const v128);
                let mut t = i64x2_extend_low_i32x4(s);
                t = i64x2_add(t, sh);
                v128_store(dp as *mut v128, t);
                t = i64x2_extend_high_i32x4(s);
                t = i64x2_add(t, sh);
                v128_store(dp.add(2) as *mut v128, t);
                sp = sp.add(4);
                dp = dp.add(4);
                i -= 1;
            }
        }
    } else {
        let mut sp = (*src_line).storage.i64_.add(src_line_offset as usize);
        let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
        let sh = i64x2_splat(shift);
        let mut i = ((width + 3) >> 2) as i32;
        while i > 0 {
            let mut s0 = v128_load(sp as *const v128);
            s0 = i64x2_add(s0, sh);
            let mut s1 = v128_load(sp.add(2) as *const v128);
            s1 = i64x2_add(s1, sh);
            s0 = i32x4_shuffle::<0, 2, 4, 6>(s0, s1);
            v128_store(dp as *mut v128, s0);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 1;
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_rev_convert_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    if (*src_line).flags & crate::openjph::ojph_mem::LFT_32BIT != 0 {
        if (*dst_line).flags & crate::openjph::ojph_mem::LFT_32BIT != 0 {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
            let sh = i32x4_splat((-shift) as Si32);
            let zero = i32x4_splat(0);
            let mut i = ((width + 3) >> 2) as i32;
            while i > 0 {
                let mut s = v128_load(sp as *const v128);
                let c = i32x4_lt(s, zero);
                let mut v_m_sh = sh;
                v_m_sh = core::arch::wasm32::i32x4_sub(v_m_sh, s);
                v_m_sh = v128_and(c, v_m_sh);
                s = v128_andnot(s, c);
                s = v128_or(s, v_m_sh);
                v128_store(dp as *mut v128, s);
                sp = sp.add(4);
                dp = dp.add(4);
                i -= 1;
            }
        } else {
            let mut sp = (*src_line).storage.i32_.add(src_line_offset as usize);
            let mut dp = (*dst_line).storage.i64_.add(dst_line_offset as usize);
            let sh = i64x2_splat(-shift);
            let zero = i64x2_splat(0);
            let mut i = ((width + 3) >> 2) as i32;
            while i > 0 {
                let s = v128_load(sp as *const v128);

                let mut u = i64x2_extend_low_i32x4(s);
                let mut c = i64x2_lt(u, zero);
                let mut v_m_sh = i64x2_sub(sh, u);
                v_m_sh = v128_and(c, v_m_sh);
                u = v128_andnot(u, c);
                u = v128_or(u, v_m_sh);
                v128_store(dp as *mut v128, u);

                u = i64x2_extend_high_i32x4(s);
                c = i64x2_lt(u, zero);
                v_m_sh = i64x2_sub(sh, u);
                v_m_sh = v128_and(c, v_m_sh);
                u = v128_andnot(u, c);
                u = v128_or(u, v_m_sh);
                v128_store(dp.add(2) as *mut v128, u);

                sp = sp.add(4);
                dp = dp.add(4);
                i -= 1;
            }
        }
    } else {
        let mut sp = (*src_line).storage.i64_.add(src_line_offset as usize);
        let mut dp = (*dst_line).storage.i32_.add(dst_line_offset as usize);
        let sh = i64x2_splat(-shift);
        let zero = i64x2_splat(0);
        let mut i = ((width + 3) >> 2) as i32;
        while i > 0 {
            let s = v128_load(sp as *const v128);
            let m = i64x2_lt(s, zero);
            let tm = i64x2_sub(sh, s);
            let n = v128_and(m, tm);
            let p = v128_andnot(s, m);
            let mut t0 = v128_or(n, p);

            let s = v128_load(sp.add(2) as *const v128);
            let m = i64x2_lt(s, zero);
            let tm = i64x2_sub(sh, s);
            let n = v128_and(m, tm);
            let p = v128_andnot(s, m);
            let t1 = v128_or(n, p);

            t0 = i32x4_shuffle::<0, 2, 4, 6>(t0, t1);
            v128_store(dp as *mut v128, t0);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 1;
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_ict_forward(
    mut r: *const f32,
    mut g: *const f32,
    mut b: *const f32,
    mut y: *mut f32,
    mut cb: *mut f32,
    mut cr: *mut f32,
    repeat: Ui32,
) {
    let alpha_rf = f32x4_splat(CT_CNST::ALPHA_RF);
    let alpha_gf = f32x4_splat(CT_CNST::ALPHA_GF);
    let alpha_bf = f32x4_splat(CT_CNST::ALPHA_BF);
    let beta_cbf = f32x4_splat(CT_CNST::BETA_CbF);
    let beta_crf = f32x4_splat(CT_CNST::BETA_CrF);
    let mut i = (repeat + 3) >> 2;
    while i > 0 {
        let mr = v128_load(r as *const v128);
        let mb = v128_load(b as *const v128);
        let mut my = f32x4_mul(alpha_rf, mr);
        my = f32x4_add(my, f32x4_mul(alpha_gf, v128_load(g as *const v128)));
        my = f32x4_add(my, f32x4_mul(alpha_bf, mb));
        v128_store(y as *mut v128, my);
        v128_store(cb as *mut v128, f32x4_mul(beta_cbf, f32x4_sub(mb, my)));
        v128_store(cr as *mut v128, f32x4_mul(beta_crf, f32x4_sub(mr, my)));
        r = r.add(4);
        g = g.add(4);
        b = b.add(4);
        y = y.add(4);
        cb = cb.add(4);
        cr = cr.add(4);
        i -= 1;
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_ict_backward(
    mut y: *const f32,
    mut cb: *const f32,
    mut cr: *const f32,
    mut r: *mut f32,
    mut g: *mut f32,
    mut b: *mut f32,
    repeat: Ui32,
) {
    let gamma_cr2g = f32x4_splat(CT_CNST::GAMMA_CR2G);
    let gamma_cb2g = f32x4_splat(CT_CNST::GAMMA_CB2G);
    let gamma_cr2r = f32x4_splat(CT_CNST::GAMMA_CR2R);
    let gamma_cb2b = f32x4_splat(CT_CNST::GAMMA_CB2B);
    let mut i = (repeat + 3) >> 2;
    while i > 0 {
        let my = v128_load(y as *const v128);
        let mcr = v128_load(cr as *const v128);
        let mcb = v128_load(cb as *const v128);
        let mg = f32x4_sub(my, f32x4_mul(gamma_cr2g, mcr));
        v128_store(g as *mut v128, f32x4_sub(mg, f32x4_mul(gamma_cb2g, mcb)));
        v128_store(r as *mut v128, f32x4_add(my, f32x4_mul(gamma_cr2r, mcr)));
        v128_store(b as *mut v128, f32x4_add(my, f32x4_mul(gamma_cb2b, mcb)));
        y = y.add(4);
        cb = cb.add(4);
        cr = cr.add(4);
        r = r.add(4);
        g = g.add(4);
        b = b.add(4);
        i -= 1;
    }
}

#[cfg(target_arch = "wasm32")]
unsafe fn local_wasm_irv_convert_to_integer<const NLT_TYPE3: bool>(
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
    let mul = f32x4_splat((1u64 << bit_depth) as f32);
    let fl_up_lim = f32x4_splat(-(neg_limit as f32));
    let fl_low_lim = f32x4_splat(neg_limit as f32);
    let s32_up_lim = i32x4_splat(i32::MAX >> (32 - bit_depth));
    let s32_low_lim = i32x4_splat(i32::MIN >> (32 - bit_depth));

    if is_signed {
        let zero = f32x4_splat(0.0);
        let half = f32x4_splat(0.5);
        let bias = i32x4_splat(-(((1u64 << (bit_depth - 1)) + 1) as i32));
        let mut i = width as i32;
        while i > 0 {
            let mut t = v128_load(sp as *const v128);
            t = f32x4_mul(t, mul);
            let mut u = ojph_convert_float_to_i32(t, zero, half);
            u = ojph_wasm_i32x4_max_ge(u, s32_low_lim, t, fl_low_lim);
            u = ojph_wasm_i32x4_min_lt(u, s32_up_lim, t, fl_up_lim);
            if NLT_TYPE3 {
                let c = i32x4_gt(zero, u);
                let mut neg = i32x4_sub(bias, u);
                neg = v128_and(c, neg);
                u = v128_andnot(u, c);
                u = v128_or(neg, u);
            }
            v128_store(dp as *mut v128, u);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 4;
        }
    } else {
        let zero = f32x4_splat(0.0);
        let half = f32x4_splat(0.5);
        let ihalf = i32x4_splat((1u64 << (bit_depth - 1)) as i32);
        let mut i = width as i32;
        while i > 0 {
            let mut t = v128_load(sp as *const v128);
            t = f32x4_mul(t, mul);
            let mut u = ojph_convert_float_to_i32(t, zero, half);
            u = ojph_wasm_i32x4_max_ge(u, s32_low_lim, t, fl_low_lim);
            u = ojph_wasm_i32x4_min_lt(u, s32_up_lim, t, fl_up_lim);
            u = i32x4_add(u, ihalf);
            v128_store(dp as *mut v128, u);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 4;
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_irv_convert_to_integer(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_wasm_irv_convert_to_integer::<false>(
        src_line,
        dst_line,
        dst_line_offset,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_irv_convert_to_integer_nlt_type3(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_wasm_irv_convert_to_integer::<true>(
        src_line,
        dst_line,
        dst_line_offset,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(target_arch = "wasm32")]
unsafe fn local_wasm_irv_convert_to_float<const NLT_TYPE3: bool>(
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
    let mul = f32x4_splat((1.0f64 / (1u64 << bit_depth) as f64) as f32);

    if is_signed {
        let zero = i32x4_splat(0);
        let bias = i32x4_splat(-(((1u64 << (bit_depth - 1)) + 1) as i32));
        let mut i = width as i32;
        while i > 0 {
            let mut t = v128_load(sp as *const v128);
            if NLT_TYPE3 {
                let c = i32x4_lt(t, zero);
                let neg = i32x4_sub(bias, t);
                let neg = v128_and(c, neg);
                let c2 = v128_andnot(t, c);
                t = v128_or(neg, c2);
            }
            let mut v = f32x4_convert_i32x4(t);
            v = f32x4_mul(v, mul);
            v128_store(dp as *mut v128, v);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 4;
        }
    } else {
        let half = i32x4_splat((1u64 << (bit_depth - 1)) as i32);
        let mut i = width as i32;
        while i > 0 {
            let mut t = v128_load(sp as *const v128);
            t = i32x4_sub(t, half);
            let mut v = f32x4_convert_i32x4(t);
            v = f32x4_mul(v, mul);
            v128_store(dp as *mut v128, v);
            sp = sp.add(4);
            dp = dp.add(4);
            i -= 4;
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_irv_convert_to_float(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_wasm_irv_convert_to_float::<false>(
        src_line,
        src_line_offset,
        dst_line,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_irv_convert_to_float_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    local_wasm_irv_convert_to_float::<true>(
        src_line,
        src_line_offset,
        dst_line,
        bit_depth,
        is_signed,
        width,
    );
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_rct_forward(
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
            let mr = v128_load(rp as *const v128);
            let mg = v128_load(gp as *const v128);
            let mb = v128_load(bp as *const v128);
            let mut t = i32x4_add(mr, mb);
            t = i32x4_add(t, i32x4_shl(mg, 1));
            v128_store(yp as *mut v128, i32x4_shr(t, 2));
            t = i32x4_sub(mb, mg);
            v128_store(cbp as *mut v128, t);
            t = i32x4_sub(mr, mg);
            v128_store(crp as *mut v128, t);
            rp = rp.add(4);
            gp = gp.add(4);
            bp = bp.add(4);
            yp = yp.add(4);
            cbp = cbp.add(4);
            crp = crp.add(4);
            i -= 1;
        }
    } else {
        let mut rp = (*r).storage.i32_;
        let mut gp = (*g).storage.i32_;
        let mut bp = (*b).storage.i32_;
        let mut yp = (*y).storage.i64_;
        let mut cbp = (*cb).storage.i64_;
        let mut crp = (*cr).storage.i64_;
        let mut i = ((repeat + 3) >> 2) as i32;
        while i > 0 {
            let mr32 = v128_load(rp as *const v128);
            let mg32 = v128_load(gp as *const v128);
            let mb32 = v128_load(bp as *const v128);

            let mut mr = i64x2_extend_low_i32x4(mr32);
            let mut mg = i64x2_extend_low_i32x4(mg32);
            let mut mb = i64x2_extend_low_i32x4(mb32);
            let mut t = i64x2_add(mr, mb);
            t = i64x2_add(t, i64x2_shl(mg, 1));
            v128_store(yp as *mut v128, i64x2_shr(t, 2));
            t = i64x2_sub(mb, mg);
            v128_store(cbp as *mut v128, t);
            t = i64x2_sub(mr, mg);
            v128_store(crp as *mut v128, t);
            yp = yp.add(2);
            cbp = cbp.add(2);
            crp = crp.add(2);

            mr = i64x2_extend_high_i32x4(mr32);
            mg = i64x2_extend_high_i32x4(mg32);
            mb = i64x2_extend_high_i32x4(mb32);
            t = i64x2_add(mr, mb);
            t = i64x2_add(t, i64x2_shl(mg, 1));
            v128_store(yp as *mut v128, i64x2_shr(t, 2));
            t = i64x2_sub(mb, mg);
            v128_store(cbp as *mut v128, t);
            t = i64x2_sub(mr, mg);
            v128_store(crp as *mut v128, t);
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

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_rct_backward(
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
            let my = v128_load(yp as *const v128);
            let mcb = v128_load(cbp as *const v128);
            let mcr = v128_load(crp as *const v128);
            let mut t = i32x4_add(mcb, mcr);
            t = i32x4_sub(my, i32x4_shr(t, 2));
            v128_store(gp as *mut v128, t);
            let mut u = i32x4_add(mcb, t);
            v128_store(bp as *mut v128, u);
            u = i32x4_add(mcr, t);
            v128_store(rp as *mut v128, u);
            yp = yp.add(4);
            cbp = cbp.add(4);
            crp = crp.add(4);
            rp = rp.add(4);
            gp = gp.add(4);
            bp = bp.add(4);
            i -= 1;
        }
    } else {
        let mut yp = (*y).storage.i64_;
        let mut cbp = (*cb).storage.i64_;
        let mut crp = (*cr).storage.i64_;
        let mut rp = (*r).storage.i32_;
        let mut gp = (*g).storage.i32_;
        let mut bp = (*b).storage.i32_;
        let mut i = ((repeat + 3) >> 2) as i32;
        while i > 0 {
            let mut my = v128_load(yp as *const v128);
            let mut mcb = v128_load(cbp as *const v128);
            let mut mcr = v128_load(crp as *const v128);
            let mut tg0 = i64x2_add(mcb, mcr);
            tg0 = i64x2_sub(my, i64x2_shr(tg0, 2));
            let mut tb0 = i64x2_add(mcb, tg0);
            let mut tr0 = i64x2_add(mcr, tg0);

            yp = yp.add(2);
            cbp = cbp.add(2);
            crp = crp.add(2);
            my = v128_load(yp as *const v128);
            mcb = v128_load(cbp as *const v128);
            mcr = v128_load(crp as *const v128);
            let mut tg1 = i64x2_add(mcb, mcr);
            tg1 = i64x2_sub(my, i64x2_shr(tg1, 2));
            let mut tb1 = i64x2_add(mcb, tg1);
            let mut tr1 = i64x2_add(mcr, tg1);

            tr0 = i32x4_shuffle::<0, 2, 4, 6>(tr0, tr1);
            tg0 = i32x4_shuffle::<0, 2, 4, 6>(tg0, tg1);
            tb0 = i32x4_shuffle::<0, 2, 4, 6>(tb0, tb1);
            v128_store(rp as *mut v128, tr0);
            v128_store(gp as *mut v128, tg0);
            v128_store(bp as *mut v128, tb0);

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

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_convert(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    if unsafe { (*src_line).flags & LFT_32BIT } != 0 {
        if unsafe { (*dst_line).flags & LFT_32BIT } != 0 {
            let mut sp = unsafe { (*src_line).storage.i32_.add(src_line_offset as usize) };
            let mut dp = unsafe { (*dst_line).storage.i32_.add(dst_line_offset as usize) };
            let s = shift as Si32;
            for _ in 0..width {
                unsafe {
                    *dp = *sp + s;
                    dp = dp.add(1);
                    sp = sp.add(1);
                }
            }
        } else {
            let mut sp = unsafe { (*src_line).storage.i32_.add(src_line_offset as usize) };
            let mut dp = unsafe { (*dst_line).storage.i64_.add(dst_line_offset as usize) };
            for _ in 0..width {
                unsafe {
                    *dp = *sp as Si64 + shift;
                    dp = dp.add(1);
                    sp = sp.add(1);
                }
            }
        }
    } else {
        debug_assert!(unsafe { (*src_line).flags & LFT_64BIT } != 0);
        debug_assert!(unsafe { (*dst_line).flags & LFT_32BIT } != 0);
        let mut sp = unsafe { (*src_line).storage.i64_.add(src_line_offset as usize) };
        let mut dp = unsafe { (*dst_line).storage.i32_.add(dst_line_offset as usize) };
        for _ in 0..width {
            unsafe {
                *dp = (*sp + shift) as Si32;
                dp = dp.add(1);
                sp = sp.add(1);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_convert_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    if unsafe { (*src_line).flags & LFT_32BIT } != 0 {
        if unsafe { (*dst_line).flags & LFT_32BIT } != 0 {
            let mut sp = unsafe { (*src_line).storage.i32_.add(src_line_offset as usize) };
            let mut dp = unsafe { (*dst_line).storage.i32_.add(dst_line_offset as usize) };
            let s = shift as Si32;
            for _ in 0..width {
                unsafe {
                    let v = *sp;
                    *dp = if v >= 0 { v } else { -v - s };
                    dp = dp.add(1);
                    sp = sp.add(1);
                }
            }
        } else {
            let mut sp = unsafe { (*src_line).storage.i32_.add(src_line_offset as usize) };
            let mut dp = unsafe { (*dst_line).storage.i64_.add(dst_line_offset as usize) };
            for _ in 0..width {
                unsafe {
                    let v = *sp as Si64;
                    *dp = if v >= 0 { v } else { -v - shift };
                    dp = dp.add(1);
                    sp = sp.add(1);
                }
            }
        }
    } else {
        debug_assert!(unsafe { (*src_line).flags & LFT_64BIT } != 0);
        debug_assert!(unsafe { (*dst_line).flags & LFT_32BIT } != 0);
        let mut sp = unsafe { (*src_line).storage.i64_.add(src_line_offset as usize) };
        let mut dp = unsafe { (*dst_line).storage.i32_.add(dst_line_offset as usize) };
        for _ in 0..width {
            unsafe {
                let v = *sp;
                *dp = if v >= 0 {
                    v as Si32
                } else {
                    (-v - shift) as Si32
                };
                dp = dp.add(1);
                sp = sp.add(1);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn local_wasm_irv_convert_to_integer_scalar<const NLT_TYPE3: bool>(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    debug_assert!(unsafe { (*src_line).flags & LFT_32BIT } != 0);
    debug_assert!(unsafe { (*src_line).flags & LFT_INTEGER } == 0);
    debug_assert!(unsafe { (*dst_line).flags & LFT_32BIT } != 0);
    debug_assert!(unsafe { (*dst_line).flags & LFT_INTEGER } != 0);
    debug_assert!(bit_depth <= 32);

    let mut sp = unsafe { (*src_line).storage.f32_ };
    let mut dp = unsafe { (*dst_line).storage.i32_.add(dst_line_offset as usize) };
    let neg_limit = i32::MIN >> (32 - bit_depth);
    let mul = (1u64 << bit_depth) as f32;
    let fl_up_lim = -(neg_limit as f32);
    let fl_low_lim = neg_limit as f32;
    let s32_up_lim = i32::MAX >> (32 - bit_depth);
    let s32_low_lim = i32::MIN >> (32 - bit_depth);

    if is_signed {
        let bias = ((1u64 << (bit_depth - 1)) + 1) as Si32;
        for _ in 0..width {
            unsafe {
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
        }
    } else {
        let half = (1u64 << (bit_depth - 1)) as Si32;
        for _ in 0..width {
            unsafe {
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
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_irv_convert_to_integer(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    unsafe {
        local_wasm_irv_convert_to_integer_scalar::<false>(
            src_line,
            dst_line,
            dst_line_offset,
            bit_depth,
            is_signed,
            width,
        )
    };
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_irv_convert_to_integer_nlt_type3(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    unsafe {
        local_wasm_irv_convert_to_integer_scalar::<true>(
            src_line,
            dst_line,
            dst_line_offset,
            bit_depth,
            is_signed,
            width,
        )
    };
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn local_wasm_irv_convert_to_float_scalar<const NLT_TYPE3: bool>(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    debug_assert!(unsafe { (*src_line).flags & LFT_32BIT } != 0);
    debug_assert!(unsafe { (*src_line).flags & LFT_INTEGER } != 0);
    debug_assert!(unsafe { (*dst_line).flags & LFT_32BIT } != 0);
    debug_assert!(unsafe { (*dst_line).flags & LFT_INTEGER } == 0);
    debug_assert!(bit_depth <= 32);

    let mul = (1.0f64 / ((1u64 << bit_depth) as f64)) as f32;
    let mut sp = unsafe { (*src_line).storage.i32_.add(src_line_offset as usize) };
    let mut dp = unsafe { (*dst_line).storage.f32_ };

    if is_signed {
        let bias = ((1u64 << (bit_depth - 1)) + 1) as Si32;
        for _ in 0..width {
            unsafe {
                let mut v = *sp;
                if NLT_TYPE3 {
                    v = if v >= 0 { v } else { -v - bias };
                }
                *dp = v as f32 * mul;
                dp = dp.add(1);
                sp = sp.add(1);
            }
        }
    } else {
        let half = (1u64 << (bit_depth - 1)) as Si32;
        for _ in 0..width {
            unsafe {
                let v = *sp - half;
                *dp = v as f32 * mul;
                dp = dp.add(1);
                sp = sp.add(1);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_irv_convert_to_float(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    unsafe {
        local_wasm_irv_convert_to_float_scalar::<false>(
            src_line,
            src_line_offset,
            dst_line,
            bit_depth,
            is_signed,
            width,
        )
    };
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_irv_convert_to_float_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    unsafe {
        local_wasm_irv_convert_to_float_scalar::<true>(
            src_line,
            src_line_offset,
            dst_line,
            bit_depth,
            is_signed,
            width,
        )
    };
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rct_forward(
    r: *const LineBuf,
    g: *const LineBuf,
    b: *const LineBuf,
    y: *mut LineBuf,
    cb: *mut LineBuf,
    cr: *mut LineBuf,
    repeat: Ui32,
) {
    debug_assert!(unsafe { (*y).flags & LFT_INTEGER } != 0);
    debug_assert!(unsafe { (*cb).flags & LFT_INTEGER } != 0);
    debug_assert!(unsafe { (*cr).flags & LFT_INTEGER } != 0);
    debug_assert!(unsafe { (*r).flags & LFT_INTEGER } != 0);
    debug_assert!(unsafe { (*g).flags & LFT_INTEGER } != 0);
    debug_assert!(unsafe { (*b).flags & LFT_INTEGER } != 0);

    if unsafe { (*y).flags & LFT_32BIT } != 0 {
        let mut rp = unsafe { (*r).storage.i32_ };
        let mut gp = unsafe { (*g).storage.i32_ };
        let mut bp = unsafe { (*b).storage.i32_ };
        let mut yp = unsafe { (*y).storage.i32_ };
        let mut cbp = unsafe { (*cb).storage.i32_ };
        let mut crp = unsafe { (*cr).storage.i32_ };
        for _ in 0..repeat {
            unsafe {
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
        }
    } else {
        let mut rp = unsafe { (*r).storage.i32_ };
        let mut gp = unsafe { (*g).storage.i32_ };
        let mut bp = unsafe { (*b).storage.i32_ };
        let mut yp = unsafe { (*y).storage.i64_ };
        let mut cbp = unsafe { (*cb).storage.i64_ };
        let mut crp = unsafe { (*cr).storage.i64_ };
        for _ in 0..repeat {
            unsafe {
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
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rct_backward(
    y: *const LineBuf,
    cb: *const LineBuf,
    cr: *const LineBuf,
    r: *mut LineBuf,
    g: *mut LineBuf,
    b: *mut LineBuf,
    repeat: Ui32,
) {
    if unsafe { (*y).flags & LFT_32BIT } != 0 {
        let mut yp = unsafe { (*y).storage.i32_ };
        let mut cbp = unsafe { (*cb).storage.i32_ };
        let mut crp = unsafe { (*cr).storage.i32_ };
        let mut rp = unsafe { (*r).storage.i32_ };
        let mut gp = unsafe { (*g).storage.i32_ };
        let mut bp = unsafe { (*b).storage.i32_ };
        for _ in 0..repeat {
            unsafe {
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
        }
    } else {
        let mut yp = unsafe { (*y).storage.i64_ };
        let mut cbp = unsafe { (*cb).storage.i64_ };
        let mut crp = unsafe { (*cr).storage.i64_ };
        let mut rp = unsafe { (*r).storage.i32_ };
        let mut gp = unsafe { (*g).storage.i32_ };
        let mut bp = unsafe { (*b).storage.i32_ };
        for _ in 0..repeat {
            unsafe {
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
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_ict_forward(
    r: *const f32,
    g: *const f32,
    b: *const f32,
    y: *mut f32,
    cb: *mut f32,
    cr: *mut f32,
    repeat: Ui32,
) {
    let mut r = r;
    let mut g = g;
    let mut b = b;
    let mut y = y;
    let mut cb = cb;
    let mut cr = cr;
    for _ in 0..repeat {
        unsafe {
            *y = CT_CNST::ALPHA_RF * *r + CT_CNST::ALPHA_GF * *g + CT_CNST::ALPHA_BF * *b;
            *cb = CT_CNST::BETA_CbF * (*b - *y);
            *cr = CT_CNST::BETA_CrF * (*r - *y);
            r = r.add(1);
            g = g.add(1);
            b = b.add(1);
            y = y.add(1);
            cb = cb.add(1);
            cr = cr.add(1);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_ict_backward(
    y: *const f32,
    cb: *const f32,
    cr: *const f32,
    r: *mut f32,
    g: *mut f32,
    b: *mut f32,
    repeat: Ui32,
) {
    let mut y = y;
    let mut cb = cb;
    let mut cr = cr;
    let mut r = r;
    let mut g = g;
    let mut b = b;
    for _ in 0..repeat {
        unsafe {
            *g = *y - CT_CNST::GAMMA_CR2G * *cr - CT_CNST::GAMMA_CB2G * *cb;
            *r = *y + CT_CNST::GAMMA_CR2R * *cr;
            *b = *y + CT_CNST::GAMMA_CB2B * *cb;
            y = y.add(1);
            cb = cb.add(1);
            cr = cr.add(1);
            r = r.add(1);
            g = g.add(1);
            b = b.add(1);
        }
    }
}
