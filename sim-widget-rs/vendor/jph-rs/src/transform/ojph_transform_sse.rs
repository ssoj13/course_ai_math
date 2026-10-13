//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_transform_sse.cpp`

use crate::codestream::ojph_params_local::{LiftingStep, ParamAtk};
use crate::openjph::ojph_defs::Ui32;
use crate::openjph::ojph_mem::LineBuf;

#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse")]
unsafe fn sse_deinterleave32(
    mut dpl: *mut f32,
    mut dph: *mut f32,
    mut sp: *mut f32,
    mut width: i32,
) {
    while width > 0 {
        let a = unsafe { _mm_load_ps(sp) };
        let b = unsafe { _mm_load_ps(sp.add(4)) };
        let c = _mm_shuffle_ps(a, b, 0b10_00_10_00);
        let d = _mm_shuffle_ps(a, b, 0b11_01_11_01);
        unsafe {
            _mm_store_ps(dpl, c);
            _mm_store_ps(dph, d);
            sp = sp.add(8);
            dpl = dpl.add(4);
            dph = dph.add(4);
        }
        width -= 8;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse")]
unsafe fn sse_interleave32(mut dp: *mut f32, mut spl: *mut f32, mut sph: *mut f32, mut width: i32) {
    while width > 0 {
        let a = unsafe { _mm_load_ps(spl) };
        let b = unsafe { _mm_load_ps(sph) };
        let c = _mm_unpacklo_ps(a, b);
        let d = _mm_unpackhi_ps(a, b);
        unsafe {
            _mm_store_ps(dp, c);
            _mm_store_ps(dp.add(4), d);
            dp = dp.add(8);
            spl = spl.add(4);
            sph = sph.add(4);
        }
        width -= 8;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse")]
unsafe fn sse_multiply_const(mut p: *mut f32, f: f32, mut width: i32) {
    let factor = _mm_set1_ps(f);
    while width > 0 {
        let s = unsafe { _mm_load_ps(p) };
        unsafe {
            _mm_store_ps(p, _mm_mul_ps(factor, s));
            p = p.add(4);
        }
        width -= 4;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse")]
pub unsafe fn sse_irv_vert_step(
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
    let factor = _mm_set1_ps(a);

    let mut dst = unsafe { (*aug).storage.f32_ };
    let mut src1 = unsafe { (*sig).storage.f32_ };
    let mut src2 = unsafe { (*other).storage.f32_ };
    let mut i = repeat as i32;
    while i > 0 {
        let s1 = unsafe { _mm_load_ps(src1) };
        let s2 = unsafe { _mm_load_ps(src2) };
        let d = unsafe { _mm_load_ps(dst) };
        let d = _mm_add_ps(d, _mm_mul_ps(factor, _mm_add_ps(s1, s2)));
        unsafe {
            _mm_store_ps(dst, d);
            dst = dst.add(4);
            src1 = src1.add(4);
            src2 = src2.add(4);
        }
        i -= 4;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse")]
pub unsafe fn sse_irv_vert_times_k(k: f32, aug: *const LineBuf, repeat: Ui32) {
    unsafe { sse_multiply_const((*aug).storage.f32_, k, repeat as i32) };
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse")]
pub unsafe fn sse_irv_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
    if width > 1 {
        unsafe {
            let dpl = if even {
                (*ldst).storage.f32_
            } else {
                (*hdst).storage.f32_
            };
            let dph = if even {
                (*hdst).storage.f32_
            } else {
                (*ldst).storage.f32_
            };
            sse_deinterleave32(dpl, dph, (*src).storage.f32_, width as i32);
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
            let mut sp = lp;
            let mut dp = hp;
            let mut i = h_width as i32;
            let f = _mm_set1_ps(a);
            if even {
                while i > 0 {
                    let m = unsafe { _mm_load_ps(sp) };
                    let n = unsafe { _mm_loadu_ps(sp.add(1)) };
                    let p = unsafe { _mm_load_ps(dp) };
                    let p = _mm_add_ps(p, _mm_mul_ps(f, _mm_add_ps(m, n)));
                    unsafe {
                        _mm_store_ps(dp, p);
                        sp = sp.add(4);
                        dp = dp.add(4);
                    }
                    i -= 4;
                }
            } else {
                while i > 0 {
                    let m = unsafe { _mm_load_ps(sp) };
                    let n = unsafe { _mm_loadu_ps(sp.sub(1)) };
                    let p = unsafe { _mm_load_ps(dp) };
                    let p = _mm_add_ps(p, _mm_mul_ps(f, _mm_add_ps(m, n)));
                    unsafe {
                        _mm_store_ps(dp, p);
                        sp = sp.add(4);
                        dp = dp.add(4);
                    }
                    i -= 4;
                }
            }

            std::mem::swap(&mut lp, &mut hp);
            even = !even;
            std::mem::swap(&mut l_width, &mut h_width);
        }

        let k = unsafe { (*atk).get_k() };
        let k_inv = 1.0f32 / k;
        unsafe {
            sse_multiply_const(lp, k_inv, l_width as i32);
            sse_multiply_const(hp, k, h_width as i32);
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

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse")]
pub unsafe fn sse_irv_horz_syn(
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
        unsafe {
            sse_multiply_const(aug, k, aug_width as i32);
            sse_multiply_const(oth, k_inv, oth_width as i32);
        }

        let num_steps = unsafe { (*atk).get_num_steps() };
        for j in 0..num_steps {
            let s = unsafe { (*atk).get_step(j) };
            let a = unsafe { (*s).irv.aatk };

            unsafe {
                *oth.sub(1) = *oth;
                *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
            }
            let mut sp = oth;
            let mut dp = aug;
            let mut i = aug_width as i32;
            let f = _mm_set1_ps(a);
            if ev {
                while i > 0 {
                    let m = unsafe { _mm_load_ps(sp) };
                    let n = unsafe { _mm_loadu_ps(sp.sub(1)) };
                    let p = unsafe { _mm_load_ps(dp) };
                    let p = _mm_sub_ps(p, _mm_mul_ps(f, _mm_add_ps(m, n)));
                    unsafe {
                        _mm_store_ps(dp, p);
                        sp = sp.add(4);
                        dp = dp.add(4);
                    }
                    i -= 4;
                }
            } else {
                while i > 0 {
                    let m = unsafe { _mm_load_ps(sp) };
                    let n = unsafe { _mm_loadu_ps(sp.add(1)) };
                    let p = unsafe { _mm_load_ps(dp) };
                    let p = _mm_sub_ps(p, _mm_mul_ps(f, _mm_add_ps(m, n)));
                    unsafe {
                        _mm_store_ps(dp, p);
                        sp = sp.add(4);
                        dp = dp.add(4);
                    }
                    i -= 4;
                }
            }

            std::mem::swap(&mut aug, &mut oth);
            ev = !ev;
            std::mem::swap(&mut aug_width, &mut oth_width);
        }

        unsafe {
            let dp = (*dst).storage.f32_;
            let spl = if even {
                (*lsrc).storage.f32_
            } else {
                (*hsrc).storage.f32_
            };
            let sph = if even {
                (*hsrc).storage.f32_
            } else {
                (*lsrc).storage.f32_
            };
            sse_interleave32(dp, spl, sph, width as i32);
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
