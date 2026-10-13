//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_transform.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_transform.cpp`

use crate::codestream::ojph_params_local::{LiftingStep, ParamAtk};
use crate::openjph::ojph_arch::{
    X86_CPU_EXT_LEVEL_AVX, X86_CPU_EXT_LEVEL_AVX2, X86_CPU_EXT_LEVEL_AVX512, X86_CPU_EXT_LEVEL_SSE,
    X86_CPU_EXT_LEVEL_SSE2, force_wavelet_rev_sse2, get_cpu_ext_level,
};
use crate::openjph::ojph_defs::Ui32;
use crate::openjph::ojph_mem::{LFT_32BIT, LineBuf};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::transform::ojph_transform_avx::{
    avx_irv_horz_ana, avx_irv_horz_syn, avx_irv_vert_step, avx_irv_vert_times_k,
};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::transform::ojph_transform_avx2::{
    avx2_rev_horz_ana, avx2_rev_horz_syn, avx2_rev_vert_step,
};
#[cfg(target_arch = "x86_64")]
use crate::transform::ojph_transform_avx512::{
    avx512_irv_horz_ana, avx512_irv_horz_syn, avx512_irv_vert_step, avx512_irv_vert_times_k,
    avx512_rev_horz_ana, avx512_rev_horz_syn, avx512_rev_vert_step,
};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::transform::ojph_transform_sse::{
    sse_irv_horz_ana, sse_irv_horz_syn, sse_irv_vert_step, sse_irv_vert_times_k,
};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::transform::ojph_transform_sse2::{
    sse2_rev_horz_ana, sse2_rev_horz_syn, sse2_rev_vert_step,
};
#[cfg(target_arch = "wasm32")]
use crate::transform::ojph_transform_wasm::{
    wasm_irv_horz_ana, wasm_irv_horz_syn, wasm_irv_vert_step, wasm_irv_vert_times_k,
    wasm_rev_horz_ana, wasm_rev_horz_syn, wasm_rev_vert_step,
};
use std::sync::Once;

static WAVELET_INIT: Once = Once::new();

type RevVertStepFn =
    unsafe fn(*const LiftingStep, *const LineBuf, *const LineBuf, *const LineBuf, Ui32, bool);
type RevHorzAnaFn =
    unsafe fn(*const ParamAtk, *const LineBuf, *const LineBuf, *const LineBuf, Ui32, bool);
type RevHorzSynFn =
    unsafe fn(*const ParamAtk, *const LineBuf, *const LineBuf, *const LineBuf, Ui32, bool);
type IrvVertStepFn =
    unsafe fn(*const LiftingStep, *const LineBuf, *const LineBuf, *const LineBuf, Ui32, bool);
type IrvVertTimesKFn = unsafe fn(f32, *const LineBuf, Ui32);
type IrvHorzAnaFn =
    unsafe fn(*const ParamAtk, *const LineBuf, *const LineBuf, *const LineBuf, Ui32, bool);
type IrvHorzSynFn =
    unsafe fn(*const ParamAtk, *const LineBuf, *const LineBuf, *const LineBuf, Ui32, bool);

static mut REV_VERT_STEP: Option<RevVertStepFn> = None;
static mut REV_HORZ_ANA: Option<RevHorzAnaFn> = None;
static mut REV_HORZ_SYN: Option<RevHorzSynFn> = None;
static mut IRV_VERT_STEP: Option<IrvVertStepFn> = None;
static mut IRV_VERT_TIMES_K: Option<IrvVertTimesKFn> = None;
static mut IRV_HORZ_ANA: Option<IrvHorzAnaFn> = None;
static mut IRV_HORZ_SYN: Option<IrvHorzSynFn> = None;

pub fn init_wavelet_transform_functions() {
    WAVELET_INIT.call_once(|| unsafe {
        REV_VERT_STEP = Some(gen_rev_vert_step);
        REV_HORZ_ANA = Some(gen_rev_horz_ana);
        REV_HORZ_SYN = Some(gen_rev_horz_syn);
        IRV_VERT_STEP = Some(gen_irv_vert_step);
        IRV_VERT_TIMES_K = Some(gen_irv_vert_times_k);
        IRV_HORZ_ANA = Some(gen_irv_horz_ana);
        IRV_HORZ_SYN = Some(gen_irv_horz_syn);

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if get_cpu_ext_level() >= X86_CPU_EXT_LEVEL_SSE {
                IRV_VERT_STEP = Some(sse_irv_vert_step);
                IRV_VERT_TIMES_K = Some(sse_irv_vert_times_k);
                IRV_HORZ_ANA = Some(sse_irv_horz_ana);
                IRV_HORZ_SYN = Some(sse_irv_horz_syn);
            }

            if get_cpu_ext_level() >= X86_CPU_EXT_LEVEL_SSE2 {
                REV_VERT_STEP = Some(sse2_rev_vert_step);
                REV_HORZ_ANA = Some(sse2_rev_horz_ana);
                REV_HORZ_SYN = Some(sse2_rev_horz_syn);
            }

            if get_cpu_ext_level() >= X86_CPU_EXT_LEVEL_AVX {
                IRV_VERT_STEP = Some(avx_irv_vert_step);
                IRV_VERT_TIMES_K = Some(avx_irv_vert_times_k);
                IRV_HORZ_ANA = Some(avx_irv_horz_ana);
                IRV_HORZ_SYN = Some(avx_irv_horz_syn);
            }

            if get_cpu_ext_level() >= X86_CPU_EXT_LEVEL_AVX2 {
                REV_VERT_STEP = Some(avx2_rev_vert_step);
                REV_HORZ_ANA = Some(avx2_rev_horz_ana);
                REV_HORZ_SYN = Some(avx2_rev_horz_syn);
                if force_wavelet_rev_sse2() {
                    REV_VERT_STEP = Some(sse2_rev_vert_step);
                    REV_HORZ_ANA = Some(sse2_rev_horz_ana);
                    REV_HORZ_SYN = Some(sse2_rev_horz_syn);
                }
            }

            #[cfg(target_arch = "x86_64")]
            if get_cpu_ext_level() >= X86_CPU_EXT_LEVEL_AVX512 {
                REV_VERT_STEP = Some(avx512_rev_vert_step);
                REV_HORZ_ANA = Some(avx512_rev_horz_ana);
                REV_HORZ_SYN = Some(avx512_rev_horz_syn);
                IRV_VERT_STEP = Some(avx512_irv_vert_step);
                IRV_VERT_TIMES_K = Some(avx512_irv_vert_times_k);
                IRV_HORZ_ANA = Some(avx512_irv_horz_ana);
                IRV_HORZ_SYN = Some(avx512_irv_horz_syn);
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            REV_VERT_STEP = Some(wasm_rev_vert_step);
            REV_HORZ_ANA = Some(wasm_rev_horz_ana);
            REV_HORZ_SYN = Some(wasm_rev_horz_syn);
            IRV_VERT_STEP = Some(wasm_irv_vert_step);
            IRV_VERT_TIMES_K = Some(wasm_irv_vert_times_k);
            IRV_HORZ_ANA = Some(wasm_irv_horz_ana);
            IRV_HORZ_SYN = Some(wasm_irv_horz_syn);
        }
    });
}

unsafe fn gen_rev_vert_step32(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let a = unsafe { (*s).rev.aatk as i32 };
    let b = unsafe { (*s).rev.batk as i32 };
    let e = unsafe { (*s).rev.eatk as u32 };
    let mut dst = unsafe { (*aug).storage.i32_ };
    let mut src1 = unsafe { (*sig).storage.i32_ };
    let mut src2 = unsafe { (*other).storage.i32_ };

    if a == 1 {
        for _ in 0..repeat {
            unsafe {
                if synthesis {
                    *dst -= (b + *src1 + *src2) >> e;
                } else {
                    *dst += (b + *src1 + *src2) >> e;
                }
                dst = dst.add(1);
                src1 = src1.add(1);
                src2 = src2.add(1);
            }
        }
    } else if a == -1 && b == 1 && e == 1 {
        for _ in 0..repeat {
            unsafe {
                if synthesis {
                    *dst += (*src1 + *src2) >> e;
                } else {
                    *dst -= (*src1 + *src2) >> e;
                }
                dst = dst.add(1);
                src1 = src1.add(1);
                src2 = src2.add(1);
            }
        }
    } else if a == -1 {
        for _ in 0..repeat {
            unsafe {
                if synthesis {
                    *dst -= (b - (*src1 + *src2)) >> e;
                } else {
                    *dst += (b - (*src1 + *src2)) >> e;
                }
                dst = dst.add(1);
                src1 = src1.add(1);
                src2 = src2.add(1);
            }
        }
    } else {
        for _ in 0..repeat {
            unsafe {
                if synthesis {
                    *dst -= (b + a * (*src1 + *src2)) >> e;
                } else {
                    *dst += (b + a * (*src1 + *src2)) >> e;
                }
                dst = dst.add(1);
                src1 = src1.add(1);
                src2 = src2.add(1);
            }
        }
    }
}

unsafe fn gen_rev_vert_step64(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let a = unsafe { (*s).rev.aatk as i64 };
    let b = unsafe { (*s).rev.batk as i64 };
    let e = unsafe { (*s).rev.eatk as u32 };
    let mut dst = unsafe { (*aug).storage.i64_ };
    let mut src1 = unsafe { (*sig).storage.i64_ };
    let mut src2 = unsafe { (*other).storage.i64_ };

    if a == 1 {
        for _ in 0..repeat {
            unsafe {
                if synthesis {
                    *dst -= (b + *src1 + *src2) >> e;
                } else {
                    *dst += (b + *src1 + *src2) >> e;
                }
                dst = dst.add(1);
                src1 = src1.add(1);
                src2 = src2.add(1);
            }
        }
    } else if a == -1 && b == 1 && e == 1 {
        for _ in 0..repeat {
            unsafe {
                if synthesis {
                    *dst += (*src1 + *src2) >> e;
                } else {
                    *dst -= (*src1 + *src2) >> e;
                }
                dst = dst.add(1);
                src1 = src1.add(1);
                src2 = src2.add(1);
            }
        }
    } else if a == -1 {
        for _ in 0..repeat {
            unsafe {
                if synthesis {
                    *dst -= (b - (*src1 + *src2)) >> e;
                } else {
                    *dst += (b - (*src1 + *src2)) >> e;
                }
                dst = dst.add(1);
                src1 = src1.add(1);
                src2 = src2.add(1);
            }
        }
    } else {
        for _ in 0..repeat {
            unsafe {
                if synthesis {
                    *dst -= (b + a * (*src1 + *src2)) >> e;
                } else {
                    *dst += (b + a * (*src1 + *src2)) >> e;
                }
                dst = dst.add(1);
                src1 = src1.add(1);
                src2 = src2.add(1);
            }
        }
    }
}

pub unsafe fn gen_rev_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    if (!sig.is_null() && unsafe { (*sig).flags & LFT_32BIT } != 0)
        || (!aug.is_null() && unsafe { (*aug).flags & LFT_32BIT } != 0)
        || (!other.is_null() && unsafe { (*other).flags & LFT_32BIT } != 0)
    {
        unsafe { gen_rev_vert_step32(s, sig, other, aug, repeat, synthesis) };
    } else {
        unsafe { gen_rev_vert_step64(s, sig, other, aug, repeat, synthesis) };
    }
}

unsafe fn gen_rev_horz_ana32(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
    if width > 1 {
        let mut dph = unsafe { (*hdst).storage.i32_ };
        let mut dpl = unsafe { (*ldst).storage.i32_ };
        let mut sp = unsafe { (*src).storage.i32_ };
        let mut w = width;
        if !even {
            unsafe {
                *dph = *sp;
                dph = dph.add(1);
                sp = sp.add(1);
            }
            w -= 1;
        }
        while w > 1 {
            unsafe {
                *dpl = *sp;
                *dph = *sp.add(1);
                dpl = dpl.add(1);
                dph = dph.add(1);
                sp = sp.add(2);
            }
            w -= 2;
        }
        if w != 0 {
            unsafe {
                *dpl = *sp;
            }
        }

        let mut hp = unsafe { (*hdst).storage.i32_ };
        let mut lp = unsafe { (*ldst).storage.i32_ };
        let mut l_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut h_width = (width + if even { 0 } else { 1 }) >> 1;
        let num_steps = unsafe { (*atk).get_num_steps() };
        for j in (0..num_steps).rev() {
            let s = unsafe { (*atk).get_step(j) };
            let a = unsafe { (*s).rev.aatk as i32 };
            let b = unsafe { (*s).rev.batk as i32 };
            let e = unsafe { (*s).rev.eatk as u32 };

            unsafe {
                *lp.sub(1) = *lp;
                *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);
            }
            let mut sp = unsafe { lp.add(if even { 1 } else { 0 }) };
            let mut dp = hp;
            if a == 1 {
                for _ in 0..h_width {
                    unsafe {
                        *dp += (b + (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                for _ in 0..h_width {
                    unsafe {
                        *dp -= (*sp.sub(1) + *sp) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else if a == -1 {
                for _ in 0..h_width {
                    unsafe {
                        *dp += (b - (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else {
                for _ in 0..h_width {
                    unsafe {
                        *dp += (b + a * (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            }

            std::mem::swap(&mut lp, &mut hp);
            even = !even;
            std::mem::swap(&mut l_width, &mut h_width);
        }
    } else if even {
        unsafe {
            *(*ldst).storage.i32_ = *(*src).storage.i32_;
        }
    } else {
        unsafe {
            *(*hdst).storage.i32_ = *(*src).storage.i32_ << 1;
        }
    }
}

unsafe fn gen_rev_horz_ana64(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
    if width > 1 {
        let mut dph = unsafe { (*hdst).storage.i64_ };
        let mut dpl = unsafe { (*ldst).storage.i64_ };
        let mut sp = unsafe { (*src).storage.i64_ };
        let mut w = width;
        if !even {
            unsafe {
                *dph = *sp;
                dph = dph.add(1);
                sp = sp.add(1);
            }
            w -= 1;
        }
        while w > 1 {
            unsafe {
                *dpl = *sp;
                *dph = *sp.add(1);
                dpl = dpl.add(1);
                dph = dph.add(1);
                sp = sp.add(2);
            }
            w -= 2;
        }
        if w != 0 {
            unsafe {
                *dpl = *sp;
            }
        }

        let mut hp = unsafe { (*hdst).storage.i64_ };
        let mut lp = unsafe { (*ldst).storage.i64_ };
        let mut l_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut h_width = (width + if even { 0 } else { 1 }) >> 1;
        let num_steps = unsafe { (*atk).get_num_steps() };
        for j in (0..num_steps).rev() {
            let s = unsafe { (*atk).get_step(j) };
            let a = unsafe { (*s).rev.aatk as i64 };
            let b = unsafe { (*s).rev.batk as i64 };
            let e = unsafe { (*s).rev.eatk as u32 };

            unsafe {
                *lp.sub(1) = *lp;
                *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);
            }
            let mut sp = unsafe { lp.add(if even { 1 } else { 0 }) };
            let mut dp = hp;
            if a == 1 {
                for _ in 0..h_width {
                    unsafe {
                        *dp += (b + (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                for _ in 0..h_width {
                    unsafe {
                        *dp -= (*sp.sub(1) + *sp) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else if a == -1 {
                for _ in 0..h_width {
                    unsafe {
                        *dp += (b - (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else {
                for _ in 0..h_width {
                    unsafe {
                        *dp += (b + a * (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            }

            std::mem::swap(&mut lp, &mut hp);
            even = !even;
            std::mem::swap(&mut l_width, &mut h_width);
        }
    } else if even {
        unsafe {
            *(*ldst).storage.i64_ = *(*src).storage.i64_;
        }
    } else {
        unsafe {
            *(*hdst).storage.i64_ = *(*src).storage.i64_ << 1;
        }
    }
}

pub unsafe fn gen_rev_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if unsafe { (*src).flags & LFT_32BIT } != 0 {
        unsafe { gen_rev_horz_ana32(atk, ldst, hdst, src, width, even) };
    } else {
        unsafe { gen_rev_horz_ana64(atk, ldst, hdst, src, width, even) };
    }
}

unsafe fn gen_rev_horz_syn32(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if width > 1 {
        let mut ev = even;
        let mut oth = unsafe { (*hsrc).storage.i32_ };
        let mut aug = unsafe { (*lsrc).storage.i32_ };
        let mut aug_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut oth_width = (width + if even { 0 } else { 1 }) >> 1;
        let num_steps = unsafe { (*atk).get_num_steps() };
        for j in 0..num_steps {
            let s = unsafe { (*atk).get_step(j) };
            let a = unsafe { (*s).rev.aatk as i32 };
            let b = unsafe { (*s).rev.batk as i32 };
            let e = unsafe { (*s).rev.eatk as u32 };
            unsafe {
                *oth.sub(1) = *oth;
                *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
            }
            let mut sp = unsafe { oth.add(if ev { 0 } else { 1 }) };
            let mut dp = aug;
            if a == 1 {
                for _ in 0..aug_width {
                    unsafe {
                        *dp -= (b + (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                for _ in 0..aug_width {
                    unsafe {
                        *dp += (*sp.sub(1) + *sp) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else if a == -1 {
                for _ in 0..aug_width {
                    unsafe {
                        *dp -= (b - (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else {
                for _ in 0..aug_width {
                    unsafe {
                        *dp -= (b + a * (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            }
            std::mem::swap(&mut aug, &mut oth);
            ev = !ev;
            std::mem::swap(&mut aug_width, &mut oth_width);
        }

        let mut sph = unsafe { (*hsrc).storage.i32_ };
        let mut spl = unsafe { (*lsrc).storage.i32_ };
        let mut dp = unsafe { (*dst).storage.i32_ };
        let mut w = width;
        if !even {
            unsafe {
                *dp = *sph;
                dp = dp.add(1);
                sph = sph.add(1);
            }
            w -= 1;
        }
        while w > 1 {
            unsafe {
                *dp = *spl;
                *dp.add(1) = *sph;
                dp = dp.add(2);
                spl = spl.add(1);
                sph = sph.add(1);
            }
            w -= 2;
        }
        if w != 0 {
            unsafe {
                *dp = *spl;
            }
        }
    } else if even {
        unsafe {
            *(*dst).storage.i32_ = *(*lsrc).storage.i32_;
        }
    } else {
        unsafe {
            *(*dst).storage.i32_ = *(*hsrc).storage.i32_ >> 1;
        }
    }
}

unsafe fn gen_rev_horz_syn64(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if width > 1 {
        let mut ev = even;
        let mut oth = unsafe { (*hsrc).storage.i64_ };
        let mut aug = unsafe { (*lsrc).storage.i64_ };
        let mut aug_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut oth_width = (width + if even { 0 } else { 1 }) >> 1;
        let num_steps = unsafe { (*atk).get_num_steps() };
        for j in 0..num_steps {
            let s = unsafe { (*atk).get_step(j) };
            let a = unsafe { (*s).rev.aatk as i64 };
            let b = unsafe { (*s).rev.batk as i64 };
            let e = unsafe { (*s).rev.eatk as u32 };
            unsafe {
                *oth.sub(1) = *oth;
                *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
            }
            let mut sp = unsafe { oth.add(if ev { 0 } else { 1 }) };
            let mut dp = aug;
            if a == 1 {
                for _ in 0..aug_width {
                    unsafe {
                        *dp -= (b + (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                for _ in 0..aug_width {
                    unsafe {
                        *dp += (*sp.sub(1) + *sp) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else if a == -1 {
                for _ in 0..aug_width {
                    unsafe {
                        *dp -= (b - (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            } else {
                for _ in 0..aug_width {
                    unsafe {
                        *dp -= (b + a * (*sp.sub(1) + *sp)) >> e;
                        sp = sp.add(1);
                        dp = dp.add(1);
                    }
                }
            }
            std::mem::swap(&mut aug, &mut oth);
            ev = !ev;
            std::mem::swap(&mut aug_width, &mut oth_width);
        }

        let mut sph = unsafe { (*hsrc).storage.i64_ };
        let mut spl = unsafe { (*lsrc).storage.i64_ };
        let mut dp = unsafe { (*dst).storage.i64_ };
        let mut w = width;
        if !even {
            unsafe {
                *dp = *sph;
                dp = dp.add(1);
                sph = sph.add(1);
            }
            w -= 1;
        }
        while w > 1 {
            unsafe {
                *dp = *spl;
                *dp.add(1) = *sph;
                dp = dp.add(2);
                spl = spl.add(1);
                sph = sph.add(1);
            }
            w -= 2;
        }
        if w != 0 {
            unsafe {
                *dp = *spl;
            }
        }
    } else if even {
        unsafe {
            *(*dst).storage.i64_ = *(*lsrc).storage.i64_;
        }
    } else {
        unsafe {
            *(*dst).storage.i64_ = *(*hsrc).storage.i64_ >> 1;
        }
    }
}

pub unsafe fn gen_rev_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if unsafe { (*dst).flags & LFT_32BIT } != 0 {
        unsafe { gen_rev_horz_syn32(atk, dst, lsrc, hsrc, width, even) };
    } else {
        unsafe { gen_rev_horz_syn64(atk, dst, lsrc, hsrc, width, even) };
    }
}

pub unsafe fn gen_irv_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let mut a = unsafe { (*s).irv.aatk };
    if synthesis {
        a = -a;
    }
    let mut dst = unsafe { (*aug).storage.f32_ };
    let mut src1 = unsafe { (*sig).storage.f32_ };
    let mut src2 = unsafe { (*other).storage.f32_ };
    for _ in 0..repeat {
        unsafe {
            *dst += a * (*src1 + *src2);
            dst = dst.add(1);
            src1 = src1.add(1);
            src2 = src2.add(1);
        }
    }
}

pub unsafe fn gen_irv_vert_times_k(k: f32, aug: *const LineBuf, repeat: Ui32) {
    let mut dst = unsafe { (*aug).storage.f32_ };
    for _ in 0..repeat {
        unsafe {
            *dst *= k;
            dst = dst.add(1);
        }
    }
}

pub unsafe fn gen_irv_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
    if width > 1 {
        let mut dph = unsafe { (*hdst).storage.f32_ };
        let mut dpl = unsafe { (*ldst).storage.f32_ };
        let mut sp = unsafe { (*src).storage.f32_ };
        let mut w = width;
        if !even {
            unsafe {
                *dph = *sp;
                dph = dph.add(1);
                sp = sp.add(1);
            }
            w -= 1;
        }
        while w > 1 {
            unsafe {
                *dpl = *sp;
                *dph = *sp.add(1);
                dpl = dpl.add(1);
                dph = dph.add(1);
                sp = sp.add(2);
            }
            w -= 2;
        }
        if w != 0 {
            unsafe {
                *dpl = *sp;
            }
        }

        let mut hp = unsafe { (*hdst).storage.f32_ };
        let mut lp = unsafe { (*ldst).storage.f32_ };
        let mut l_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut h_width = (width + if even { 0 } else { 1 }) >> 1;
        let num_steps = unsafe { (*atk).get_num_steps() };
        for j in (0..num_steps).rev() {
            let s = unsafe { (*atk).get_step(j) };
            let a = unsafe { (*s).irv.aatk };
            unsafe {
                *lp.sub(1) = *lp;
                *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);
            }
            let mut sp = unsafe { lp.add(if even { 1 } else { 0 }) };
            let mut dp = hp;
            for _ in 0..h_width {
                unsafe {
                    *dp += a * (*sp.sub(1) + *sp);
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            }
            std::mem::swap(&mut lp, &mut hp);
            even = !even;
            std::mem::swap(&mut l_width, &mut h_width);
        }

        let k = unsafe { (*atk).get_k() };
        let k_inv = 1.0f32 / k;
        let mut dp = lp;
        for _ in 0..l_width {
            unsafe {
                *dp *= k_inv;
                dp = dp.add(1);
            }
        }
        let mut dp = hp;
        for _ in 0..h_width {
            unsafe {
                *dp *= k;
                dp = dp.add(1);
            }
        }
    } else if even {
        unsafe {
            *(*ldst).storage.f32_ = *(*src).storage.f32_;
        }
    } else {
        unsafe {
            *(*hdst).storage.f32_ = *(*src).storage.f32_ * 2.0f32;
        }
    }
}

pub unsafe fn gen_irv_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if width > 1 {
        let mut ev = even;
        let mut oth = unsafe { (*hsrc).storage.f32_ };
        let mut aug = unsafe { (*lsrc).storage.f32_ };
        let mut aug_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut oth_width = (width + if even { 0 } else { 1 }) >> 1;

        let k = unsafe { (*atk).get_k() };
        let k_inv = 1.0f32 / k;
        let mut dp = aug;
        for _ in 0..aug_width {
            unsafe {
                *dp *= k;
                dp = dp.add(1);
            }
        }
        let mut dp = oth;
        for _ in 0..oth_width {
            unsafe {
                *dp *= k_inv;
                dp = dp.add(1);
            }
        }

        let num_steps = unsafe { (*atk).get_num_steps() };
        for j in 0..num_steps {
            let s = unsafe { (*atk).get_step(j) };
            let a = unsafe { (*s).irv.aatk };
            unsafe {
                *oth.sub(1) = *oth;
                *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
            }
            let mut sp = unsafe { oth.add(if ev { 0 } else { 1 }) };
            let mut dp = aug;
            for _ in 0..aug_width {
                unsafe {
                    *dp -= a * (*sp.sub(1) + *sp);
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            }
            std::mem::swap(&mut aug, &mut oth);
            ev = !ev;
            std::mem::swap(&mut aug_width, &mut oth_width);
        }

        let mut sph = unsafe { (*hsrc).storage.f32_ };
        let mut spl = unsafe { (*lsrc).storage.f32_ };
        let mut dp = unsafe { (*dst).storage.f32_ };
        let mut w = width;
        if !even {
            unsafe {
                *dp = *sph;
                dp = dp.add(1);
                sph = sph.add(1);
            }
            w -= 1;
        }
        while w > 1 {
            unsafe {
                *dp = *spl;
                *dp.add(1) = *sph;
                dp = dp.add(2);
                spl = spl.add(1);
                sph = sph.add(1);
            }
            w -= 2;
        }
        if w != 0 {
            unsafe {
                *dp = *spl;
            }
        }
    } else if even {
        unsafe {
            *(*dst).storage.f32_ = *(*lsrc).storage.f32_;
        }
    } else {
        unsafe {
            *(*dst).storage.f32_ = *(*hsrc).storage.f32_ * 0.5f32;
        }
    }
}

pub unsafe fn rev_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    init_wavelet_transform_functions();
    if let Some(f) = REV_VERT_STEP {
        f(s, sig, other, aug, repeat, synthesis);
    }
}

pub unsafe fn rev_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    init_wavelet_transform_functions();
    if let Some(f) = REV_HORZ_ANA {
        f(atk, ldst, hdst, src, width, even);
    }
}

pub unsafe fn rev_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    init_wavelet_transform_functions();
    if let Some(f) = REV_HORZ_SYN {
        f(atk, dst, lsrc, hsrc, width, even);
    }
}

pub unsafe fn irv_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    init_wavelet_transform_functions();
    if let Some(f) = IRV_VERT_STEP {
        f(s, sig, other, aug, repeat, synthesis);
    }
}

pub unsafe fn irv_vert_times_k(k: f32, aug: *const LineBuf, repeat: Ui32) {
    init_wavelet_transform_functions();
    if let Some(f) = IRV_VERT_TIMES_K {
        f(k, aug, repeat);
    }
}

pub unsafe fn irv_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    init_wavelet_transform_functions();
    if let Some(f) = IRV_HORZ_ANA {
        f(atk, ldst, hdst, src, width, even);
    }
}

pub unsafe fn irv_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    init_wavelet_transform_functions();
    if let Some(f) = IRV_HORZ_SYN {
        f(atk, dst, lsrc, hsrc, width, even);
    }
}
