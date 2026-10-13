//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_transform_avx.cpp`

use crate::codestream::ojph_params_local::{LiftingStep, ParamAtk};
use crate::openjph::ojph_defs::Ui32;
use crate::openjph::ojph_mem::LineBuf;

#[cfg(target_arch = "x86")]
use core::arch::x86::{
    __m256, _mm256_add_ps, _mm256_load_ps, _mm256_loadu_ps, _mm256_mul_ps, _mm256_permute2f128_ps,
    _mm256_set1_ps, _mm256_shuffle_ps, _mm256_store_ps, _mm256_sub_ps, _mm256_unpackhi_ps,
    _mm256_unpacklo_ps,
};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{
    __m256, _mm256_add_ps, _mm256_load_ps, _mm256_loadu_ps, _mm256_mul_ps, _mm256_permute2f128_ps,
    _mm256_set1_ps, _mm256_shuffle_ps, _mm256_store_ps, _mm256_sub_ps, _mm256_unpackhi_ps,
    _mm256_unpacklo_ps,
};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
pub unsafe fn avx_multiply_const(mut p: *mut f32, f: f32, mut width: i32) {
    let factor: __m256 = _mm256_set1_ps(f);
    while width > 0 {
        let s = _mm256_load_ps(p);
        _mm256_store_ps(p, _mm256_mul_ps(factor, s));
        p = p.add(8);
        width -= 8;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
pub unsafe fn avx_deinterleave32(
    mut dpl: *mut f32,
    mut dph: *mut f32,
    mut sp: *mut f32,
    mut width: i32,
) {
    while width > 0 {
        let a = _mm256_load_ps(sp);
        let b = _mm256_load_ps(sp.add(8));
        let c = _mm256_permute2f128_ps(a, b, (2 << 4) | 0);
        let d = _mm256_permute2f128_ps(a, b, (3 << 4) | 1);
        let e = _mm256_shuffle_ps(c, d, 0b10_00_10_00);
        let f = _mm256_shuffle_ps(c, d, 0b11_01_11_01);
        _mm256_store_ps(dpl, e);
        _mm256_store_ps(dph, f);
        sp = sp.add(16);
        dpl = dpl.add(8);
        dph = dph.add(8);
        width -= 16;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
pub unsafe fn avx_interleave32(
    mut dp: *mut f32,
    mut spl: *mut f32,
    mut sph: *mut f32,
    mut width: i32,
) {
    while width > 0 {
        let a = _mm256_load_ps(spl);
        let b = _mm256_load_ps(sph);
        let c = _mm256_unpacklo_ps(a, b);
        let d = _mm256_unpackhi_ps(a, b);
        let e = _mm256_permute2f128_ps(c, d, (2 << 4) | 0);
        let f = _mm256_permute2f128_ps(c, d, (3 << 4) | 1);
        _mm256_store_ps(dp, e);
        _mm256_store_ps(dp.add(8), f);
        dp = dp.add(16);
        spl = spl.add(8);
        sph = sph.add(8);
        width -= 16;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
pub unsafe fn avx_irv_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let mut a = (*s).irv.aatk;
    if synthesis {
        a = -a;
    }
    let factor = _mm256_set1_ps(a);
    let mut dst = (*aug).storage.f32_;
    let mut src1 = (*sig).storage.f32_;
    let mut src2 = (*other).storage.f32_;
    let mut i = repeat as i32;
    while i > 0 {
        let s1 = _mm256_load_ps(src1);
        let s2 = _mm256_load_ps(src2);
        let d = _mm256_load_ps(dst);
        let out = _mm256_add_ps(d, _mm256_mul_ps(factor, _mm256_add_ps(s1, s2)));
        _mm256_store_ps(dst, out);
        dst = dst.add(8);
        src1 = src1.add(8);
        src2 = src2.add(8);
        i -= 8;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
pub unsafe fn avx_irv_vert_times_k(k: f32, aug: *const LineBuf, repeat: Ui32) {
    avx_multiply_const((*aug).storage.f32_, k, repeat as i32);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
pub unsafe fn avx_irv_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
    if width > 1 {
        {
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
            let sp = (*src).storage.f32_;
            avx_deinterleave32(dpl, dph, sp, width as i32);
        }

        let mut hp = (*hdst).storage.f32_;
        let mut lp = (*ldst).storage.f32_;
        let mut l_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut h_width = (width + if even { 0 } else { 1 }) >> 1;
        let num_steps = (*atk).get_num_steps();
        for j in (0..num_steps).rev() {
            let s = (*atk).get_step(j);
            let a = (*s).irv.aatk;

            *lp.sub(1) = *lp;
            *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);

            let mut sp = lp;
            let mut dp = hp;
            let mut i = h_width as i32;
            let f = _mm256_set1_ps(a);
            if even {
                while i > 0 {
                    let m = _mm256_load_ps(sp);
                    let n = _mm256_loadu_ps(sp.add(1));
                    let p = _mm256_load_ps(dp);
                    let p = _mm256_add_ps(p, _mm256_mul_ps(f, _mm256_add_ps(m, n)));
                    _mm256_store_ps(dp, p);
                    sp = sp.add(8);
                    dp = dp.add(8);
                    i -= 8;
                }
            } else {
                while i > 0 {
                    let m = _mm256_load_ps(sp);
                    let n = _mm256_loadu_ps(sp.sub(1));
                    let p = _mm256_load_ps(dp);
                    let p = _mm256_add_ps(p, _mm256_mul_ps(f, _mm256_add_ps(m, n)));
                    _mm256_store_ps(dp, p);
                    sp = sp.add(8);
                    dp = dp.add(8);
                    i -= 8;
                }
            }

            core::mem::swap(&mut lp, &mut hp);
            even = !even;
            core::mem::swap(&mut l_width, &mut h_width);
        }

        let k = (*atk).get_k();
        let k_inv = 1.0f32 / k;
        avx_multiply_const(lp, k_inv, l_width as i32);
        avx_multiply_const(hp, k, h_width as i32);
    } else if even {
        *(*ldst).storage.f32_ = *(*src).storage.f32_;
    } else {
        *(*hdst).storage.f32_ = *(*src).storage.f32_ * 2.0f32;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
pub unsafe fn avx_irv_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if width > 1 {
        let mut ev = even;
        let mut oth = (*hsrc).storage.f32_;
        let mut aug = (*lsrc).storage.f32_;
        let mut aug_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut oth_width = (width + if even { 0 } else { 1 }) >> 1;

        let k = (*atk).get_k();
        let k_inv = 1.0f32 / k;
        avx_multiply_const(aug, k, aug_width as i32);
        avx_multiply_const(oth, k_inv, oth_width as i32);

        let num_steps = (*atk).get_num_steps();
        for j in 0..num_steps {
            let s = (*atk).get_step(j);
            let a = (*s).irv.aatk;

            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);

            let mut sp = oth;
            let mut dp = aug;
            let mut i = aug_width as i32;
            let f = _mm256_set1_ps(a);
            if ev {
                while i > 0 {
                    let m = _mm256_load_ps(sp);
                    let n = _mm256_loadu_ps(sp.sub(1));
                    let p = _mm256_load_ps(dp);
                    let p = _mm256_sub_ps(p, _mm256_mul_ps(f, _mm256_add_ps(m, n)));
                    _mm256_store_ps(dp, p);
                    sp = sp.add(8);
                    dp = dp.add(8);
                    i -= 8;
                }
            } else {
                while i > 0 {
                    let m = _mm256_load_ps(sp);
                    let n = _mm256_loadu_ps(sp.add(1));
                    let p = _mm256_load_ps(dp);
                    let p = _mm256_sub_ps(p, _mm256_mul_ps(f, _mm256_add_ps(m, n)));
                    _mm256_store_ps(dp, p);
                    sp = sp.add(8);
                    dp = dp.add(8);
                    i -= 8;
                }
            }

            core::mem::swap(&mut aug, &mut oth);
            ev = !ev;
            core::mem::swap(&mut aug_width, &mut oth_width);
        }

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
        avx_interleave32(dp, spl, sph, width as i32);
    } else if even {
        *(*dst).storage.f32_ = *(*lsrc).storage.f32_;
    } else {
        *(*dst).storage.f32_ = *(*hsrc).storage.f32_ * 0.5f32;
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx_multiply_const(p: *mut f32, f: f32, width: i32) {
    let _ = (p, f, width);
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx_deinterleave32(dpl: *mut f32, dph: *mut f32, sp: *mut f32, width: i32) {
    let _ = (dpl, dph, sp, width);
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx_interleave32(dp: *mut f32, spl: *mut f32, sph: *mut f32, width: i32) {
    let _ = (dp, spl, sph, width);
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx_irv_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let _ = (s, sig, other, aug, repeat, synthesis);
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx_irv_vert_times_k(k: f32, aug: *const LineBuf, repeat: Ui32) {
    let _ = (k, aug, repeat);
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx_irv_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    let _ = (atk, ldst, hdst, src, width, even);
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx_irv_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    let _ = (atk, dst, lsrc, hsrc, width, even);
}
