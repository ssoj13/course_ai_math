//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_colour.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_colour.cpp`

use crate::openjph::ojph_arch::ojph_round;
use crate::openjph::ojph_defs::{Si32, Si64, Ui32};
use crate::openjph::ojph_mem::{LFT_32BIT, LFT_64BIT, LFT_INTEGER, LineBuf};
use std::ptr::addr_of;
use std::sync::Once;

use crate::openjph::ojph_arch::{
    X86_CPU_EXT_LEVEL_AVX, X86_CPU_EXT_LEVEL_AVX2, X86_CPU_EXT_LEVEL_SSE, X86_CPU_EXT_LEVEL_SSE2,
    get_cpu_ext_level,
};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::transform::ojph_colour_avx::{avx_ict_backward, avx_ict_forward};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::transform::ojph_colour_avx2::{
    avx2_irv_convert_to_float, avx2_irv_convert_to_float_nlt_type3, avx2_irv_convert_to_integer,
    avx2_irv_convert_to_integer_nlt_type3, avx2_rct_backward, avx2_rct_forward, avx2_rev_convert,
    avx2_rev_convert_nlt_type3,
};
use crate::transform::ojph_colour_local::{
    CT_CNST, IctTransform, IrvConvertToFloat, IrvConvertToInteger, RctTransform, RevConvert,
};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::transform::ojph_colour_sse::{sse_ict_backward, sse_ict_forward};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::transform::ojph_colour_sse2::{
    sse2_irv_convert_to_float, sse2_irv_convert_to_float_nlt_type3, sse2_irv_convert_to_integer,
    sse2_irv_convert_to_integer_nlt_type3, sse2_rct_backward, sse2_rct_forward, sse2_rev_convert,
    sse2_rev_convert_nlt_type3,
};
#[cfg(target_arch = "wasm32")]
use crate::transform::ojph_colour_wasm::{
    wasm_ict_backward, wasm_ict_forward, wasm_irv_convert_to_float,
    wasm_irv_convert_to_float_nlt_type3, wasm_irv_convert_to_integer,
    wasm_irv_convert_to_integer_nlt_type3, wasm_rct_backward, wasm_rct_forward, wasm_rev_convert,
    wasm_rev_convert_nlt_type3,
};

static COLOUR_INIT: Once = Once::new();
static mut REV_CONVERT: Option<RevConvert> = None;
static mut REV_CONVERT_NLT_TYPE3: Option<RevConvert> = None;
static mut IRV_CONVERT_TO_INTEGER: Option<IrvConvertToInteger> = None;
static mut IRV_CONVERT_TO_FLOAT: Option<IrvConvertToFloat> = None;
static mut IRV_CONVERT_TO_INTEGER_NLT_TYPE3: Option<IrvConvertToInteger> = None;
static mut IRV_CONVERT_TO_FLOAT_NLT_TYPE3: Option<IrvConvertToFloat> = None;
static mut RCT_FORWARD: Option<RctTransform> = None;
static mut RCT_BACKWARD: Option<RctTransform> = None;
static mut ICT_FORWARD: Option<IctTransform> = None;
static mut ICT_BACKWARD: Option<IctTransform> = None;
const ALPHA_RF: f32 = CT_CNST::ALPHA_RF;
const ALPHA_GF: f32 = CT_CNST::ALPHA_GF;
const ALPHA_BF: f32 = CT_CNST::ALPHA_BF;
const BETA_CBF: f32 = CT_CNST::BETA_CbF;
const BETA_CRF: f32 = CT_CNST::BETA_CrF;
const GAMMA_CB2G: f32 = CT_CNST::GAMMA_CB2G;
const GAMMA_CR2G: f32 = CT_CNST::GAMMA_CR2G;
const GAMMA_CB2B: f32 = CT_CNST::GAMMA_CB2B;
const GAMMA_CR2R: f32 = CT_CNST::GAMMA_CR2R;

pub fn init_colour_transform_functions() {
    COLOUR_INIT.call_once(|| unsafe {
        REV_CONVERT = Some(gen_rev_convert);
        REV_CONVERT_NLT_TYPE3 = Some(gen_rev_convert_nlt_type3);
        IRV_CONVERT_TO_INTEGER = Some(gen_irv_convert_to_integer);
        IRV_CONVERT_TO_FLOAT = Some(gen_irv_convert_to_float);
        IRV_CONVERT_TO_INTEGER_NLT_TYPE3 = Some(gen_irv_convert_to_integer_nlt_type3);
        IRV_CONVERT_TO_FLOAT_NLT_TYPE3 = Some(gen_irv_convert_to_float_nlt_type3);
        RCT_FORWARD = Some(gen_rct_forward);
        RCT_BACKWARD = Some(gen_rct_backward);
        ICT_FORWARD = Some(gen_ict_forward);
        ICT_BACKWARD = Some(gen_ict_backward);

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if get_cpu_ext_level() >= X86_CPU_EXT_LEVEL_SSE {
                ICT_FORWARD = Some(sse_ict_forward);
                ICT_BACKWARD = Some(sse_ict_backward);
            }
            if get_cpu_ext_level() >= X86_CPU_EXT_LEVEL_SSE2 {
                REV_CONVERT = Some(sse2_rev_convert);
                REV_CONVERT_NLT_TYPE3 = Some(sse2_rev_convert_nlt_type3);
                IRV_CONVERT_TO_INTEGER = Some(sse2_irv_convert_to_integer);
                IRV_CONVERT_TO_FLOAT = Some(sse2_irv_convert_to_float);
                IRV_CONVERT_TO_INTEGER_NLT_TYPE3 = Some(sse2_irv_convert_to_integer_nlt_type3);
                IRV_CONVERT_TO_FLOAT_NLT_TYPE3 = Some(sse2_irv_convert_to_float_nlt_type3);
                RCT_FORWARD = Some(sse2_rct_forward);
                RCT_BACKWARD = Some(sse2_rct_backward);
            }
            if get_cpu_ext_level() >= X86_CPU_EXT_LEVEL_AVX {
                ICT_FORWARD = Some(avx_ict_forward);
                ICT_BACKWARD = Some(avx_ict_backward);
            }
            if get_cpu_ext_level() >= X86_CPU_EXT_LEVEL_AVX2 {
                REV_CONVERT = Some(avx2_rev_convert);
                REV_CONVERT_NLT_TYPE3 = Some(avx2_rev_convert_nlt_type3);
                IRV_CONVERT_TO_INTEGER = Some(avx2_irv_convert_to_integer);
                IRV_CONVERT_TO_FLOAT = Some(avx2_irv_convert_to_float);
                IRV_CONVERT_TO_INTEGER_NLT_TYPE3 = Some(avx2_irv_convert_to_integer_nlt_type3);
                IRV_CONVERT_TO_FLOAT_NLT_TYPE3 = Some(avx2_irv_convert_to_float_nlt_type3);
                RCT_FORWARD = Some(avx2_rct_forward);
                RCT_BACKWARD = Some(avx2_rct_backward);
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            REV_CONVERT = Some(wasm_rev_convert);
            REV_CONVERT_NLT_TYPE3 = Some(wasm_rev_convert_nlt_type3);
            IRV_CONVERT_TO_INTEGER = Some(wasm_irv_convert_to_integer);
            IRV_CONVERT_TO_FLOAT = Some(wasm_irv_convert_to_float);
            IRV_CONVERT_TO_INTEGER_NLT_TYPE3 = Some(wasm_irv_convert_to_integer_nlt_type3);
            IRV_CONVERT_TO_FLOAT_NLT_TYPE3 = Some(wasm_irv_convert_to_float_nlt_type3);
            RCT_FORWARD = Some(wasm_rct_forward);
            RCT_BACKWARD = Some(wasm_rct_backward);
            ICT_FORWARD = Some(wasm_ict_forward);
            ICT_BACKWARD = Some(wasm_ict_backward);
        }
    });
}

#[inline]
unsafe fn resolve_rev_convert() -> RevConvert {
    init_colour_transform_functions();
    let val = addr_of!(REV_CONVERT).read();
    debug_assert!(val.is_some());
    val.unwrap_or(gen_rev_convert)
}

#[inline]
unsafe fn resolve_rev_convert_nlt_type3() -> RevConvert {
    init_colour_transform_functions();
    let val = addr_of!(REV_CONVERT_NLT_TYPE3).read();
    debug_assert!(val.is_some());
    val.unwrap_or(gen_rev_convert_nlt_type3)
}

#[inline]
unsafe fn resolve_irv_convert_to_integer() -> IrvConvertToInteger {
    init_colour_transform_functions();
    let val = addr_of!(IRV_CONVERT_TO_INTEGER).read();
    debug_assert!(val.is_some());
    val.unwrap_or(gen_irv_convert_to_integer)
}

#[inline]
unsafe fn resolve_irv_convert_to_integer_nlt_type3() -> IrvConvertToInteger {
    init_colour_transform_functions();
    let val = addr_of!(IRV_CONVERT_TO_INTEGER_NLT_TYPE3).read();
    debug_assert!(val.is_some());
    val.unwrap_or(gen_irv_convert_to_integer_nlt_type3)
}

#[inline]
unsafe fn resolve_irv_convert_to_float() -> IrvConvertToFloat {
    init_colour_transform_functions();
    let val = addr_of!(IRV_CONVERT_TO_FLOAT).read();
    debug_assert!(val.is_some());
    val.unwrap_or(gen_irv_convert_to_float)
}

#[inline]
unsafe fn resolve_irv_convert_to_float_nlt_type3() -> IrvConvertToFloat {
    init_colour_transform_functions();
    let val = addr_of!(IRV_CONVERT_TO_FLOAT_NLT_TYPE3).read();
    debug_assert!(val.is_some());
    val.unwrap_or(gen_irv_convert_to_float_nlt_type3)
}

#[inline]
unsafe fn resolve_rct_forward() -> RctTransform {
    init_colour_transform_functions();
    let val = addr_of!(RCT_FORWARD).read();
    debug_assert!(val.is_some());
    val.unwrap_or(gen_rct_forward)
}

#[inline]
unsafe fn resolve_rct_backward() -> RctTransform {
    init_colour_transform_functions();
    let val = addr_of!(RCT_BACKWARD).read();
    debug_assert!(val.is_some());
    val.unwrap_or(gen_rct_backward)
}

#[inline]
unsafe fn resolve_ict_forward() -> IctTransform {
    init_colour_transform_functions();
    let val = addr_of!(ICT_FORWARD).read();
    debug_assert!(val.is_some());
    val.unwrap_or(gen_ict_forward)
}

#[inline]
unsafe fn resolve_ict_backward() -> IctTransform {
    init_colour_transform_functions();
    let val = addr_of!(ICT_BACKWARD).read();
    debug_assert!(val.is_some());
    val.unwrap_or(gen_ict_backward)
}

pub unsafe fn gen_rev_convert(
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

pub unsafe fn gen_rev_convert_nlt_type3(
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

unsafe fn local_irv_convert_to_integer<const NLT_TYPE3: bool>(
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

pub unsafe fn gen_irv_convert_to_integer(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    unsafe {
        local_irv_convert_to_integer::<false>(
            src_line,
            dst_line,
            dst_line_offset,
            bit_depth,
            is_signed,
            width,
        );
    }
}

pub unsafe fn gen_irv_convert_to_integer_nlt_type3(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    unsafe {
        local_irv_convert_to_integer::<true>(
            src_line,
            dst_line,
            dst_line_offset,
            bit_depth,
            is_signed,
            width,
        );
    }
}

unsafe fn local_irv_convert_to_float<const NLT_TYPE3: bool>(
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

pub unsafe fn gen_irv_convert_to_float(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    unsafe {
        local_irv_convert_to_float::<false>(
            src_line,
            src_line_offset,
            dst_line,
            bit_depth,
            is_signed,
            width,
        );
    }
}

pub unsafe fn gen_irv_convert_to_float_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    unsafe {
        local_irv_convert_to_float::<true>(
            src_line,
            src_line_offset,
            dst_line,
            bit_depth,
            is_signed,
            width,
        );
    }
}

pub unsafe fn gen_rct_forward(
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

pub unsafe fn gen_rct_backward(
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

pub unsafe fn gen_ict_forward(
    mut r: *const f32,
    mut g: *const f32,
    mut b: *const f32,
    mut y: *mut f32,
    mut cb: *mut f32,
    mut cr: *mut f32,
    repeat: Ui32,
) {
    for _ in 0..repeat {
        unsafe {
            *y = ALPHA_RF * *r + ALPHA_GF * *g + ALPHA_BF * *b;
            *cb = BETA_CBF * (*b - *y);
            *cr = BETA_CRF * (*r - *y);
            r = r.add(1);
            g = g.add(1);
            b = b.add(1);
            y = y.add(1);
            cb = cb.add(1);
            cr = cr.add(1);
        }
    }
}

pub unsafe fn gen_ict_backward(
    mut y: *const f32,
    mut cb: *const f32,
    mut cr: *const f32,
    mut r: *mut f32,
    mut g: *mut f32,
    mut b: *mut f32,
    repeat: Ui32,
) {
    for _ in 0..repeat {
        unsafe {
            *g = *y - GAMMA_CR2G * *cr - GAMMA_CB2G * *cb;
            *r = *y + GAMMA_CR2R * *cr;
            *b = *y + GAMMA_CB2B * *cb;
            y = y.add(1);
            cb = cb.add(1);
            cr = cr.add(1);
            r = r.add(1);
            g = g.add(1);
            b = b.add(1);
        }
    }
}

pub unsafe fn rev_convert(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    let f = unsafe { resolve_rev_convert() };
    unsafe {
        f(
            src_line,
            src_line_offset,
            dst_line,
            dst_line_offset,
            shift,
            width,
        )
    }
}

pub unsafe fn rev_convert_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    shift: Si64,
    width: Ui32,
) {
    let f = unsafe { resolve_rev_convert_nlt_type3() };
    unsafe {
        f(
            src_line,
            src_line_offset,
            dst_line,
            dst_line_offset,
            shift,
            width,
        )
    }
}

pub unsafe fn irv_convert_to_integer(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    let f = unsafe { resolve_irv_convert_to_integer() };
    unsafe {
        f(
            src_line,
            dst_line,
            dst_line_offset,
            bit_depth,
            is_signed,
            width,
        )
    }
}

pub unsafe fn irv_convert_to_integer_nlt_type3(
    src_line: *const LineBuf,
    dst_line: *mut LineBuf,
    dst_line_offset: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    let f = unsafe { resolve_irv_convert_to_integer_nlt_type3() };
    unsafe {
        f(
            src_line,
            dst_line,
            dst_line_offset,
            bit_depth,
            is_signed,
            width,
        )
    }
}

pub unsafe fn irv_convert_to_float(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    let f = unsafe { resolve_irv_convert_to_float() };
    unsafe {
        f(
            src_line,
            src_line_offset,
            dst_line,
            bit_depth,
            is_signed,
            width,
        )
    }
}

pub unsafe fn irv_convert_to_float_nlt_type3(
    src_line: *const LineBuf,
    src_line_offset: Ui32,
    dst_line: *mut LineBuf,
    bit_depth: Ui32,
    is_signed: bool,
    width: Ui32,
) {
    let f = unsafe { resolve_irv_convert_to_float_nlt_type3() };
    unsafe {
        f(
            src_line,
            src_line_offset,
            dst_line,
            bit_depth,
            is_signed,
            width,
        )
    }
}

pub unsafe fn rct_forward(
    r: *const LineBuf,
    g: *const LineBuf,
    b: *const LineBuf,
    y: *mut LineBuf,
    cb: *mut LineBuf,
    cr: *mut LineBuf,
    repeat: Ui32,
) {
    let f = unsafe { resolve_rct_forward() };
    unsafe { f(r, g, b, y, cb, cr, repeat) }
}

pub unsafe fn rct_backward(
    y: *const LineBuf,
    cb: *const LineBuf,
    cr: *const LineBuf,
    r: *mut LineBuf,
    g: *mut LineBuf,
    b: *mut LineBuf,
    repeat: Ui32,
) {
    let f = unsafe { resolve_rct_backward() };
    unsafe { f(y, cb, cr, r, g, b, repeat) }
}

pub unsafe fn ict_forward(
    r: *const f32,
    g: *const f32,
    b: *const f32,
    y: *mut f32,
    cb: *mut f32,
    cr: *mut f32,
    repeat: Ui32,
) {
    let f = unsafe { resolve_ict_forward() };
    unsafe { f(r, g, b, y, cb, cr, repeat) }
}

pub unsafe fn ict_backward(
    y: *const f32,
    cb: *const f32,
    cr: *const f32,
    r: *mut f32,
    g: *mut f32,
    b: *mut f32,
    repeat: Ui32,
) {
    let f = unsafe { resolve_ict_backward() };
    unsafe { f(y, cb, cr, r, g, b, repeat) }
}
