//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_transform_avx512.cpp`

use crate::codestream::ojph_params_local::{LiftingStep, ParamAtk};
use crate::openjph::ojph_defs::{Si32, Si64, Ui32};
use crate::openjph::ojph_mem::{LFT_32BIT, LFT_64BIT, LineBuf};

#[cfg(not(target_arch = "x86_64"))]
unsafe fn local_avx512_irv_vert_step(
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
    let mut dst = (*aug).storage.f32_;
    let mut src1 = (*sig).storage.f32_;
    let mut src2 = (*other).storage.f32_;
    for _ in 0..repeat {
        *dst += a * (*src1 + *src2);
        dst = dst.add(1);
        src1 = src1.add(1);
        src2 = src2.add(1);
    }
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn local_avx512_irv_vert_times_k(k: f32, aug: *const LineBuf, repeat: Ui32) {
    let mut dst = (*aug).storage.f32_;
    for _ in 0..repeat {
        *dst *= k;
        dst = dst.add(1);
    }
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn local_avx512_irv_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
    if width > 1 {
        let mut dph = (*hdst).storage.f32_;
        let mut dpl = (*ldst).storage.f32_;
        let mut sp = (*src).storage.f32_;
        let mut w = width;
        if !even {
            *dph = *sp;
            dph = dph.add(1);
            sp = sp.add(1);
            w -= 1;
        }
        while w > 1 {
            *dpl = *sp;
            *dph = *sp.add(1);
            dpl = dpl.add(1);
            dph = dph.add(1);
            sp = sp.add(2);
            w -= 2;
        }
        if w != 0 {
            *dpl = *sp;
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
            let mut sp = lp.add(if even { 1 } else { 0 });
            let mut dp = hp;
            for _ in 0..h_width {
                *dp += a * (*sp.sub(1) + *sp);
                sp = sp.add(1);
                dp = dp.add(1);
            }
            core::mem::swap(&mut lp, &mut hp);
            even = !even;
            core::mem::swap(&mut l_width, &mut h_width);
        }
        let k = (*atk).get_k();
        let k_inv = 1.0f32 / k;
        let mut dp = lp;
        for _ in 0..l_width {
            *dp *= k_inv;
            dp = dp.add(1);
        }
        let mut dp = hp;
        for _ in 0..h_width {
            *dp *= k;
            dp = dp.add(1);
        }
    } else if even {
        *(*ldst).storage.f32_ = *(*src).storage.f32_;
    } else {
        *(*hdst).storage.f32_ = *(*src).storage.f32_ * 2.0f32;
    }
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn local_avx512_irv_horz_syn(
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
        let mut dp = aug;
        for _ in 0..aug_width {
            *dp *= k;
            dp = dp.add(1);
        }
        let mut dp = oth;
        for _ in 0..oth_width {
            *dp *= k_inv;
            dp = dp.add(1);
        }
        let num_steps = (*atk).get_num_steps();
        for j in 0..num_steps {
            let s = (*atk).get_step(j);
            let a = (*s).irv.aatk;
            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
            let mut sp = oth.add(if ev { 0 } else { 1 });
            let mut dp = aug;
            for _ in 0..aug_width {
                *dp -= a * (*sp.sub(1) + *sp);
                sp = sp.add(1);
                dp = dp.add(1);
            }
            core::mem::swap(&mut aug, &mut oth);
            ev = !ev;
            core::mem::swap(&mut aug_width, &mut oth_width);
        }
        let mut sph = (*hsrc).storage.f32_;
        let mut spl = (*lsrc).storage.f32_;
        let mut dp = (*dst).storage.f32_;
        let mut w = width;
        if !even {
            *dp = *sph;
            dp = dp.add(1);
            sph = sph.add(1);
            w -= 1;
        }
        while w > 1 {
            *dp = *spl;
            *dp.add(1) = *sph;
            dp = dp.add(2);
            spl = spl.add(1);
            sph = sph.add(1);
            w -= 2;
        }
        if w != 0 {
            *dp = *spl;
        }
    } else if even {
        *(*dst).storage.f32_ = *(*lsrc).storage.f32_;
    } else {
        *(*dst).storage.f32_ = *(*hsrc).storage.f32_ * 0.5f32;
    }
}

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_deinterleave32(
    mut dpl: *mut f32,
    mut dph: *mut f32,
    mut sp: *mut f32,
    mut width: i32,
) {
    let idx1 = _mm512_set_epi32(
        0x1E, 0x1C, 0x1A, 0x18, 0x16, 0x14, 0x12, 0x10, 0x0E, 0x0C, 0x0A, 0x08, 0x06, 0x04, 0x02,
        0x00,
    );
    let idx2 = _mm512_set_epi32(
        0x1F, 0x1D, 0x1B, 0x19, 0x17, 0x15, 0x13, 0x11, 0x0F, 0x0D, 0x0B, 0x09, 0x07, 0x05, 0x03,
        0x01,
    );

    while width > 16 {
        let a = _mm512_load_ps(sp);
        let b = _mm512_load_ps(sp.add(16));
        let c = _mm512_permutex2var_ps(a, idx1, b);
        let d = _mm512_permutex2var_ps(a, idx2, b);
        _mm512_store_ps(dpl, c);
        _mm512_store_ps(dph, d);
        width -= 32;
        sp = sp.add(32);
        dpl = dpl.add(16);
        dph = dph.add(16);
    }

    while width > 0 {
        let a = _mm256_load_ps(sp);
        let b = _mm256_load_ps(sp.add(8));
        let c = _mm256_permute2f128_ps(a, b, (2 << 4) | 0);
        let d = _mm256_permute2f128_ps(a, b, (3 << 4) | 1);
        let e = _mm256_shuffle_ps(c, d, 0b10_00_10_00);
        let f = _mm256_shuffle_ps(c, d, 0b11_01_11_01);
        _mm256_store_ps(dpl, e);
        _mm256_store_ps(dph, f);
        width -= 16;
        sp = sp.add(16);
        dpl = dpl.add(8);
        dph = dph.add(8);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_interleave32(
    mut dp: *mut f32,
    mut spl: *mut f32,
    mut sph: *mut f32,
    mut width: i32,
) {
    let idx1 = _mm512_set_epi32(
        0x17, 0x7, 0x16, 0x6, 0x15, 0x5, 0x14, 0x4, 0x13, 0x3, 0x12, 0x2, 0x11, 0x1, 0x10, 0x0,
    );
    let idx2 = _mm512_set_epi32(
        0x1F, 0xF, 0x1E, 0xE, 0x1D, 0xD, 0x1C, 0xC, 0x1B, 0xB, 0x1A, 0xA, 0x19, 0x9, 0x18, 0x8,
    );

    while width > 16 {
        let a = _mm512_load_ps(spl);
        let b = _mm512_load_ps(sph);
        let c = _mm512_permutex2var_ps(a, idx1, b);
        let d = _mm512_permutex2var_ps(a, idx2, b);
        _mm512_store_ps(dp, c);
        _mm512_store_ps(dp.add(16), d);
        width -= 32;
        dp = dp.add(32);
        spl = spl.add(16);
        sph = sph.add(16);
    }

    while width > 0 {
        let a = _mm256_load_ps(spl);
        let b = _mm256_load_ps(sph);
        let c = _mm256_unpacklo_ps(a, b);
        let d = _mm256_unpackhi_ps(a, b);
        let e = _mm256_permute2f128_ps(c, d, (2 << 4) | 0);
        let f = _mm256_permute2f128_ps(c, d, (3 << 4) | 1);
        _mm256_store_ps(dp, e);
        _mm256_store_ps(dp.add(8), f);
        width -= 16;
        dp = dp.add(16);
        spl = spl.add(8);
        sph = sph.add(8);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_deinterleave64(
    mut dpl: *mut f64,
    mut dph: *mut f64,
    mut sp: *mut f64,
    mut width: i32,
) {
    let idx1 = _mm512_set_epi64(0x0E, 0x0C, 0x0A, 0x08, 0x06, 0x04, 0x02, 0x00);
    let idx2 = _mm512_set_epi64(0x0F, 0x0D, 0x0B, 0x09, 0x07, 0x05, 0x03, 0x01);

    while width > 8 {
        let a = _mm512_load_pd(sp);
        let b = _mm512_load_pd(sp.add(8));
        let c = _mm512_permutex2var_pd(a, idx1, b);
        let d = _mm512_permutex2var_pd(a, idx2, b);
        _mm512_store_pd(dpl, c);
        _mm512_store_pd(dph, d);
        width -= 16;
        sp = sp.add(16);
        dpl = dpl.add(8);
        dph = dph.add(8);
    }

    while width > 0 {
        let a = _mm256_load_pd(sp);
        let b = _mm256_load_pd(sp.add(4));
        let c = _mm256_permute2f128_pd(a, b, (2 << 4) | 0);
        let d = _mm256_permute2f128_pd(a, b, (3 << 4) | 1);
        let e = _mm256_shuffle_pd(c, d, 0x0);
        let f = _mm256_shuffle_pd(c, d, 0xF);
        _mm256_store_pd(dpl, e);
        _mm256_store_pd(dph, f);
        width -= 8;
        sp = sp.add(8);
        dpl = dpl.add(4);
        dph = dph.add(4);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_interleave64(
    mut dp: *mut f64,
    mut spl: *mut f64,
    mut sph: *mut f64,
    mut width: i32,
) {
    let idx1 = _mm512_set_epi64(0xB, 0x3, 0xA, 0x2, 0x9, 0x1, 0x8, 0x0);
    let idx2 = _mm512_set_epi64(0xF, 0x7, 0xE, 0x6, 0xD, 0x5, 0xC, 0x4);

    while width > 8 {
        let a = _mm512_load_pd(spl);
        let b = _mm512_load_pd(sph);
        let c = _mm512_permutex2var_pd(a, idx1, b);
        let d = _mm512_permutex2var_pd(a, idx2, b);
        _mm512_store_pd(dp, c);
        _mm512_store_pd(dp.add(8), d);
        width -= 16;
        dp = dp.add(16);
        spl = spl.add(8);
        sph = sph.add(8);
    }

    while width > 0 {
        let a = _mm256_load_pd(spl);
        let b = _mm256_load_pd(sph);
        let c = _mm256_unpacklo_pd(a, b);
        let d = _mm256_unpackhi_pd(a, b);
        let e = _mm256_permute2f128_pd(c, d, (2 << 4) | 0);
        let f = _mm256_permute2f128_pd(c, d, (3 << 4) | 1);
        _mm256_store_pd(dp, e);
        _mm256_store_pd(dp.add(4), f);
        width -= 8;
        dp = dp.add(8);
        spl = spl.add(4);
        sph = sph.add(4);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_multiply_const(mut p: *mut f32, f: f32, mut width: i32) {
    let factor = _mm512_set1_ps(f);
    while width > 0 {
        let s = _mm512_load_ps(p);
        _mm512_store_ps(p, _mm512_mul_ps(factor, s));
        width -= 16;
        p = p.add(16);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
pub unsafe fn avx512_irv_vert_step(
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
    let factor = _mm512_set1_ps(a);

    let mut dst = (*aug).storage.f32_;
    let mut src1 = (*sig).storage.f32_;
    let mut src2 = (*other).storage.f32_;
    let mut i = repeat as i32;
    while i > 0 {
        let s1 = _mm512_load_ps(src1);
        let s2 = _mm512_load_ps(src2);
        let d = _mm512_load_ps(dst);
        let out = _mm512_add_ps(d, _mm512_mul_ps(factor, _mm512_add_ps(s1, s2)));
        _mm512_store_ps(dst, out);
        i -= 16;
        dst = dst.add(16);
        src1 = src1.add(16);
        src2 = src2.add(16);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
pub unsafe fn avx512_irv_vert_times_k(k: f32, aug: *const LineBuf, repeat: Ui32) {
    avx512_multiply_const((*aug).storage.f32_, k, repeat as i32);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
pub unsafe fn avx512_irv_horz_ana(
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
            avx512_deinterleave32(dpl, dph, sp, width as i32);
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
            let f = _mm512_set1_ps(a);
            if even {
                while i > 0 {
                    let m = _mm512_load_ps(sp);
                    let n = _mm512_loadu_ps(sp.add(1));
                    let p = _mm512_load_ps(dp);
                    let p = _mm512_add_ps(p, _mm512_mul_ps(f, _mm512_add_ps(m, n)));
                    _mm512_store_ps(dp, p);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
                }
            } else {
                while i > 0 {
                    let m = _mm512_load_ps(sp);
                    let n = _mm512_loadu_ps(sp.sub(1));
                    let p = _mm512_load_ps(dp);
                    let p = _mm512_add_ps(p, _mm512_mul_ps(f, _mm512_add_ps(m, n)));
                    _mm512_store_ps(dp, p);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
                }
            }

            core::mem::swap(&mut lp, &mut hp);
            even = !even;
            core::mem::swap(&mut l_width, &mut h_width);
        }

        let k = (*atk).get_k();
        let k_inv = 1.0f32 / k;
        avx512_multiply_const(lp, k_inv, l_width as i32);
        avx512_multiply_const(hp, k, h_width as i32);
    } else if even {
        *(*ldst).storage.f32_ = *(*src).storage.f32_;
    } else {
        *(*hdst).storage.f32_ = *(*src).storage.f32_ * 2.0f32;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
pub unsafe fn avx512_irv_horz_syn(
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
        avx512_multiply_const(aug, k, aug_width as i32);
        avx512_multiply_const(oth, k_inv, oth_width as i32);

        let num_steps = (*atk).get_num_steps();
        for j in 0..num_steps {
            let s = (*atk).get_step(j);
            let a = (*s).irv.aatk;

            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);

            let mut sp = oth;
            let mut dp = aug;
            let mut i = aug_width as i32;
            let f = _mm512_set1_ps(a);
            if ev {
                while i > 0 {
                    let m = _mm512_load_ps(sp);
                    let n = _mm512_loadu_ps(sp.sub(1));
                    let p = _mm512_load_ps(dp);
                    let p = _mm512_sub_ps(p, _mm512_mul_ps(f, _mm512_add_ps(m, n)));
                    _mm512_store_ps(dp, p);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
                }
            } else {
                while i > 0 {
                    let m = _mm512_load_ps(sp);
                    let n = _mm512_loadu_ps(sp.add(1));
                    let p = _mm512_load_ps(dp);
                    let p = _mm512_sub_ps(p, _mm512_mul_ps(f, _mm512_add_ps(m, n)));
                    _mm512_store_ps(dp, p);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
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
        avx512_interleave32(dp, spl, sph, width as i32);
    } else if even {
        *(*dst).storage.f32_ = *(*lsrc).storage.f32_;
    } else {
        *(*dst).storage.f32_ = *(*hsrc).storage.f32_ * 0.5f32;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_rev_vert_step32(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let a = (*s).rev.aatk as i32;
    let b = (*s).rev.batk as i32;
    let e = (*s).rev.eatk as i32;
    let va = _mm512_set1_epi32(a);
    let vb = _mm512_set1_epi32(b);

    let mut dst = (*aug).storage.i32_;
    let mut src1 = (*sig).storage.i32_ as *const Si32;
    let mut src2 = (*other).storage.i32_ as *const Si32;

    if a == 1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm512_load_si512(src1 as *const __m512i);
            let s2 = _mm512_load_si512(src2 as *const __m512i);
            let mut d = _mm512_load_si512(dst as *const __m512i);
            let t = _mm512_add_epi32(s1, s2);
            let v = _mm512_add_epi32(vb, t);
            let w = _mm512_sra_epi32(v, _mm_cvtsi32_si128(e));
            d = if synthesis {
                _mm512_sub_epi32(d, w)
            } else {
                _mm512_add_epi32(d, w)
            };
            _mm512_store_si512(dst as *mut __m512i, d);
            i -= 16;
            dst = dst.add(16);
            src1 = src1.add(16);
            src2 = src2.add(16);
        }
    } else if a == -1 && b == 1 && e == 1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm512_load_si512(src1 as *const __m512i);
            let s2 = _mm512_load_si512(src2 as *const __m512i);
            let mut d = _mm512_load_si512(dst as *const __m512i);
            let t = _mm512_add_epi32(s1, s2);
            let w = _mm512_sra_epi32(t, _mm_cvtsi32_si128(e));
            d = if synthesis {
                _mm512_add_epi32(d, w)
            } else {
                _mm512_sub_epi32(d, w)
            };
            _mm512_store_si512(dst as *mut __m512i, d);
            i -= 16;
            dst = dst.add(16);
            src1 = src1.add(16);
            src2 = src2.add(16);
        }
    } else if a == -1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm512_load_si512(src1 as *const __m512i);
            let s2 = _mm512_load_si512(src2 as *const __m512i);
            let mut d = _mm512_load_si512(dst as *const __m512i);
            let t = _mm512_add_epi32(s1, s2);
            let v = _mm512_sub_epi32(vb, t);
            let w = _mm512_sra_epi32(v, _mm_cvtsi32_si128(e));
            d = if synthesis {
                _mm512_sub_epi32(d, w)
            } else {
                _mm512_add_epi32(d, w)
            };
            _mm512_store_si512(dst as *mut __m512i, d);
            i -= 16;
            dst = dst.add(16);
            src1 = src1.add(16);
            src2 = src2.add(16);
        }
    } else {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm512_load_si512(src1 as *const __m512i);
            let s2 = _mm512_load_si512(src2 as *const __m512i);
            let mut d = _mm512_load_si512(dst as *const __m512i);
            let t = _mm512_add_epi32(s1, s2);
            let u = _mm512_mullo_epi32(va, t);
            let v = _mm512_add_epi32(vb, u);
            let w = _mm512_sra_epi32(v, _mm_cvtsi32_si128(e));
            d = if synthesis {
                _mm512_sub_epi32(d, w)
            } else {
                _mm512_add_epi32(d, w)
            };
            _mm512_store_si512(dst as *mut __m512i, d);
            i -= 16;
            dst = dst.add(16);
            src1 = src1.add(16);
            src2 = src2.add(16);
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_rev_vert_step64(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let a = (*s).rev.aatk as i32;
    let b = (*s).rev.batk as i64;
    let e = (*s).rev.eatk as i32;
    let vb = _mm512_set1_epi64(b);

    let mut dst = (*aug).storage.i64_;
    let mut src1 = (*sig).storage.i64_ as *const Si64;
    let mut src2 = (*other).storage.i64_ as *const Si64;

    if a == 1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm512_load_si512(src1 as *const __m512i);
            let s2 = _mm512_load_si512(src2 as *const __m512i);
            let mut d = _mm512_load_si512(dst as *const __m512i);
            let t = _mm512_add_epi64(s1, s2);
            let v = _mm512_add_epi64(vb, t);
            let w = _mm512_sra_epi64(v, _mm_cvtsi32_si128(e));
            d = if synthesis {
                _mm512_sub_epi64(d, w)
            } else {
                _mm512_add_epi64(d, w)
            };
            _mm512_store_si512(dst as *mut __m512i, d);
            i -= 8;
            dst = dst.add(8);
            src1 = src1.add(8);
            src2 = src2.add(8);
        }
    } else if a == -1 && b == 1 && e == 1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm512_load_si512(src1 as *const __m512i);
            let s2 = _mm512_load_si512(src2 as *const __m512i);
            let mut d = _mm512_load_si512(dst as *const __m512i);
            let t = _mm512_add_epi64(s1, s2);
            let w = _mm512_sra_epi64(t, _mm_cvtsi32_si128(e));
            d = if synthesis {
                _mm512_add_epi64(d, w)
            } else {
                _mm512_sub_epi64(d, w)
            };
            _mm512_store_si512(dst as *mut __m512i, d);
            i -= 8;
            dst = dst.add(8);
            src1 = src1.add(8);
            src2 = src2.add(8);
        }
    } else if a == -1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm512_load_si512(src1 as *const __m512i);
            let s2 = _mm512_load_si512(src2 as *const __m512i);
            let mut d = _mm512_load_si512(dst as *const __m512i);
            let t = _mm512_add_epi64(s1, s2);
            let v = _mm512_sub_epi64(vb, t);
            let w = _mm512_sra_epi64(v, _mm_cvtsi32_si128(e));
            d = if synthesis {
                _mm512_sub_epi64(d, w)
            } else {
                _mm512_add_epi64(d, w)
            };
            _mm512_store_si512(dst as *mut __m512i, d);
            i -= 8;
            dst = dst.add(8);
            src1 = src1.add(8);
            src2 = src2.add(8);
        }
    } else {
        let mut i = repeat;
        while i > 0 {
            let delta = (b + a as i64 * (*src1 + *src2)) >> e;
            *dst = if synthesis {
                (*dst).wrapping_sub(delta)
            } else {
                (*dst).wrapping_add(delta)
            };
            i -= 1;
            dst = dst.add(1);
            src1 = src1.add(1);
            src2 = src2.add(1);
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
pub unsafe fn avx512_rev_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    if (!sig.is_null() && (*sig).flags & LFT_32BIT != 0)
        || (!aug.is_null() && (*aug).flags & LFT_32BIT != 0)
        || (!other.is_null() && (*other).flags & LFT_32BIT != 0)
    {
        debug_assert!(sig.is_null() || (*sig).flags & LFT_32BIT != 0);
        debug_assert!(other.is_null() || (*other).flags & LFT_32BIT != 0);
        debug_assert!(aug.is_null() || (*aug).flags & LFT_32BIT != 0);
        avx512_rev_vert_step32(s, sig, other, aug, repeat, synthesis);
    } else {
        debug_assert!(sig.is_null() || (*sig).flags & LFT_64BIT != 0);
        debug_assert!(other.is_null() || (*other).flags & LFT_64BIT != 0);
        debug_assert!(aug.is_null() || (*aug).flags & LFT_64BIT != 0);
        avx512_rev_vert_step64(s, sig, other, aug, repeat, synthesis);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_rev_horz_ana32(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
    if width > 1 {
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
        let sp0 = (*src).storage.f32_;
        avx512_deinterleave32(dpl, dph, sp0, width as i32);

        let mut hp = (*hdst).storage.i32_;
        let mut lp = (*ldst).storage.i32_;
        let mut l_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut h_width = (width + if even { 0 } else { 1 }) >> 1;
        let mut j = (*atk).get_num_steps();
        while j > 0 {
            let s = (*atk).get_step(j - 1);
            let a = (*s).rev.aatk as i32;
            let b = (*s).rev.batk as i32;
            let e = (*s).rev.eatk as i32;
            let va = _mm512_set1_epi32(a);
            let vb = _mm512_set1_epi32(b);

            *lp.sub(1) = *lp;
            *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);
            let mut sp = lp as *const Si32;
            let mut dp = hp;

            if a == 1 {
                let mut i = h_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if even {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi32(s1, s2);
                    let v = _mm512_add_epi32(vb, t);
                    let w = _mm512_sra_epi32(v, _mm_cvtsi32_si128(e));
                    d = _mm512_add_epi32(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
                }
            } else if a == -1 && b == 1 && e == 1 {
                let mut i = h_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if even {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi32(s1, s2);
                    let w = _mm512_sra_epi32(t, _mm_cvtsi32_si128(e));
                    d = _mm512_sub_epi32(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
                }
            } else if a == -1 {
                let mut i = h_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if even {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi32(s1, s2);
                    let v = _mm512_sub_epi32(vb, t);
                    let w = _mm512_sra_epi32(v, _mm_cvtsi32_si128(e));
                    d = _mm512_add_epi32(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
                }
            } else {
                let mut i = h_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if even {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi32(s1, s2);
                    let u = _mm512_mullo_epi32(va, t);
                    let v = _mm512_add_epi32(vb, u);
                    let w = _mm512_sra_epi32(v, _mm_cvtsi32_si128(e));
                    d = _mm512_add_epi32(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
                }
            }

            core::mem::swap(&mut lp, &mut hp);
            even = !even;
            core::mem::swap(&mut l_width, &mut h_width);
            j -= 1;
        }
    } else if even {
        *(*ldst).storage.i32_ = *(*src).storage.i32_;
    } else {
        *(*hdst).storage.i32_ = *(*src).storage.i32_ << 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_rev_horz_ana64(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
    if width > 1 {
        let dpl = if even {
            (*ldst).storage.p
        } else {
            (*hdst).storage.p
        } as *mut f64;
        let dph = if even {
            (*hdst).storage.p
        } else {
            (*ldst).storage.p
        } as *mut f64;
        let sp0 = (*src).storage.p as *mut f64;
        avx512_deinterleave64(dpl, dph, sp0, width as i32);

        let mut hp = (*hdst).storage.i64_;
        let mut lp = (*ldst).storage.i64_;
        let mut l_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut h_width = (width + if even { 0 } else { 1 }) >> 1;
        let mut j = (*atk).get_num_steps();
        while j > 0 {
            let s = (*atk).get_step(j - 1);
            let a = (*s).rev.aatk as i32;
            let b = (*s).rev.batk as i64;
            let e = (*s).rev.eatk as i32;
            let vb = _mm512_set1_epi64(b);

            *lp.sub(1) = *lp;
            *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);
            let mut sp = lp as *const Si64;
            let mut dp = hp;

            if a == 1 {
                let mut i = h_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if even {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi64(s1, s2);
                    let v = _mm512_add_epi64(vb, t);
                    let w = _mm512_sra_epi64(v, _mm_cvtsi32_si128(e));
                    d = _mm512_add_epi64(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 8;
                    sp = sp.add(8);
                    dp = dp.add(8);
                }
            } else if a == -1 && b == 1 && e == 1 {
                let mut i = h_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if even {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi64(s1, s2);
                    let w = _mm512_sra_epi64(t, _mm_cvtsi32_si128(e));
                    d = _mm512_sub_epi64(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 8;
                    sp = sp.add(8);
                    dp = dp.add(8);
                }
            } else if a == -1 {
                let mut i = h_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if even {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi64(s1, s2);
                    let v = _mm512_sub_epi64(vb, t);
                    let w = _mm512_sra_epi64(v, _mm_cvtsi32_si128(e));
                    d = _mm512_add_epi64(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 8;
                    sp = sp.add(8);
                    dp = dp.add(8);
                }
            } else {
                let mut i = h_width;
                while i > 0 {
                    *dp = if even {
                        (*dp).wrapping_add((b + a as i64 * (*sp + *sp.add(1))) >> e)
                    } else {
                        (*dp).wrapping_add((b + a as i64 * (*sp.sub(1) + *sp)) >> e)
                    };
                    i -= 1;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            }

            core::mem::swap(&mut lp, &mut hp);
            even = !even;
            core::mem::swap(&mut l_width, &mut h_width);
            j -= 1;
        }
    } else if even {
        *(*ldst).storage.i64_ = *(*src).storage.i64_;
    } else {
        *(*hdst).storage.i64_ = *(*src).storage.i64_ << 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
pub unsafe fn avx512_rev_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if (*src).flags & LFT_32BIT != 0 {
        debug_assert!(ldst.is_null() || (*ldst).flags & LFT_32BIT != 0);
        debug_assert!(hdst.is_null() || (*hdst).flags & LFT_32BIT != 0);
        avx512_rev_horz_ana32(atk, ldst, hdst, src, width, even);
    } else {
        debug_assert!(ldst.is_null() || (*ldst).flags & LFT_64BIT != 0);
        debug_assert!(hdst.is_null() || (*hdst).flags & LFT_64BIT != 0);
        debug_assert!(src.is_null() || (*src).flags & LFT_64BIT != 0);
        avx512_rev_horz_ana64(atk, ldst, hdst, src, width, even);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_rev_horz_syn32(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if width > 1 {
        let mut ev = even;
        let mut oth = (*hsrc).storage.i32_;
        let mut aug = (*lsrc).storage.i32_;
        let mut aug_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut oth_width = (width + if even { 0 } else { 1 }) >> 1;
        let num_steps = (*atk).get_num_steps();
        let mut j = 0;
        while j < num_steps {
            let s = (*atk).get_step(j);
            let a = (*s).rev.aatk as i32;
            let b = (*s).rev.batk as i32;
            let e = (*s).rev.eatk as i32;
            let va = _mm512_set1_epi32(a);
            let vb = _mm512_set1_epi32(b);

            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
            let mut sp = oth as *const Si32;
            let mut dp = aug;

            if a == 1 {
                let mut i = aug_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if ev {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi32(s1, s2);
                    let v = _mm512_add_epi32(vb, t);
                    let w = _mm512_sra_epi32(v, _mm_cvtsi32_si128(e));
                    d = _mm512_sub_epi32(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
                }
            } else if a == -1 && b == 1 && e == 1 {
                let mut i = aug_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if ev {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi32(s1, s2);
                    let w = _mm512_sra_epi32(t, _mm_cvtsi32_si128(e));
                    d = _mm512_add_epi32(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
                }
            } else if a == -1 {
                let mut i = aug_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if ev {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi32(s1, s2);
                    let v = _mm512_sub_epi32(vb, t);
                    let w = _mm512_sra_epi32(v, _mm_cvtsi32_si128(e));
                    d = _mm512_sub_epi32(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
                }
            } else {
                let mut i = aug_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if ev {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi32(s1, s2);
                    let u = _mm512_mullo_epi32(va, t);
                    let v = _mm512_add_epi32(vb, u);
                    let w = _mm512_sra_epi32(v, _mm_cvtsi32_si128(e));
                    d = _mm512_sub_epi32(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 16;
                    sp = sp.add(16);
                    dp = dp.add(16);
                }
            }

            core::mem::swap(&mut aug, &mut oth);
            ev = !ev;
            core::mem::swap(&mut aug_width, &mut oth_width);
            j += 1;
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
        avx512_interleave32(dp, spl, sph, width as i32);
    } else if even {
        *(*dst).storage.i32_ = *(*lsrc).storage.i32_;
    } else {
        *(*dst).storage.i32_ = *(*hsrc).storage.i32_ >> 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn avx512_rev_horz_syn64(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if width > 1 {
        let mut ev = even;
        let mut oth = (*hsrc).storage.i64_;
        let mut aug = (*lsrc).storage.i64_;
        let mut aug_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut oth_width = (width + if even { 0 } else { 1 }) >> 1;
        let num_steps = (*atk).get_num_steps();
        let mut j = 0;
        while j < num_steps {
            let s = (*atk).get_step(j);
            let a = (*s).rev.aatk as i32;
            let b = (*s).rev.batk as i64;
            let e = (*s).rev.eatk as i32;
            let vb = _mm512_set1_epi64(b);

            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
            let mut sp = oth as *const Si64;
            let mut dp = aug;

            if a == 1 {
                let mut i = aug_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if ev {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi64(s1, s2);
                    let v = _mm512_add_epi64(vb, t);
                    let w = _mm512_sra_epi64(v, _mm_cvtsi32_si128(e));
                    d = _mm512_sub_epi64(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 8;
                    sp = sp.add(8);
                    dp = dp.add(8);
                }
            } else if a == -1 && b == 1 && e == 1 {
                let mut i = aug_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if ev {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi64(s1, s2);
                    let w = _mm512_sra_epi64(t, _mm_cvtsi32_si128(e));
                    d = _mm512_add_epi64(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 8;
                    sp = sp.add(8);
                    dp = dp.add(8);
                }
            } else if a == -1 {
                let mut i = aug_width as i32;
                while i > 0 {
                    let s1 = _mm512_load_si512(sp as *const __m512i);
                    let s2 = if ev {
                        _mm512_loadu_si512(sp.sub(1) as *const __m512i)
                    } else {
                        _mm512_loadu_si512(sp.add(1) as *const __m512i)
                    };
                    let mut d = _mm512_load_si512(dp as *const __m512i);
                    let t = _mm512_add_epi64(s1, s2);
                    let v = _mm512_sub_epi64(vb, t);
                    let w = _mm512_sra_epi64(v, _mm_cvtsi32_si128(e));
                    d = _mm512_sub_epi64(d, w);
                    _mm512_store_si512(dp as *mut __m512i, d);
                    i -= 8;
                    sp = sp.add(8);
                    dp = dp.add(8);
                }
            } else {
                let mut i = aug_width;
                while i > 0 {
                    *dp = if ev {
                        (*dp).wrapping_sub((b + a as i64 * (*sp.sub(1) + *sp)) >> e)
                    } else {
                        (*dp).wrapping_sub((b + a as i64 * (*sp + *sp.add(1))) >> e)
                    };
                    i -= 1;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            }

            core::mem::swap(&mut aug, &mut oth);
            ev = !ev;
            core::mem::swap(&mut aug_width, &mut oth_width);
            j += 1;
        }

        let dp = (*dst).storage.p as *mut f64;
        let spl = if even {
            (*lsrc).storage.p
        } else {
            (*hsrc).storage.p
        } as *mut f64;
        let sph = if even {
            (*hsrc).storage.p
        } else {
            (*lsrc).storage.p
        } as *mut f64;
        avx512_interleave64(dp, spl, sph, width as i32);
    } else if even {
        *(*dst).storage.i64_ = *(*lsrc).storage.i64_;
    } else {
        *(*dst).storage.i64_ = *(*hsrc).storage.i64_ >> 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
pub unsafe fn avx512_rev_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if (*dst).flags & LFT_32BIT != 0 {
        debug_assert!(lsrc.is_null() || (*lsrc).flags & LFT_32BIT != 0);
        debug_assert!(hsrc.is_null() || (*hsrc).flags & LFT_32BIT != 0);
        avx512_rev_horz_syn32(atk, dst, lsrc, hsrc, width, even);
    } else {
        debug_assert!(dst.is_null() || (*dst).flags & LFT_64BIT != 0);
        debug_assert!(lsrc.is_null() || (*lsrc).flags & LFT_64BIT != 0);
        debug_assert!(hsrc.is_null() || (*hsrc).flags & LFT_64BIT != 0);
        avx512_rev_horz_syn64(atk, dst, lsrc, hsrc, width, even);
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn avx512_rev_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    if ((!sig.is_null() && (*sig).flags & LFT_32BIT != 0)
        || (!aug.is_null() && (*aug).flags & LFT_32BIT != 0)
        || (!other.is_null() && (*other).flags & LFT_32BIT != 0))
    {
        let a = (*s).rev.aatk as i32;
        let b = (*s).rev.batk as i32;
        let e = (*s).rev.eatk as u32;
        let mut dst = (*aug).storage.i32_;
        let mut src1 = (*sig).storage.i32_;
        let mut src2 = (*other).storage.i32_;
        for _ in 0..repeat {
            let v = if a == 1 {
                (b + (*src1 + *src2)) >> e
            } else if a == -1 && b == 1 && e == 1 {
                (*src1 + *src2) >> e
            } else if a == -1 {
                (b - (*src1 + *src2)) >> e
            } else {
                (b + a * (*src1 + *src2)) >> e
            };
            if synthesis {
                if a == -1 && b == 1 && e == 1 {
                    *dst += v;
                } else {
                    *dst -= v;
                }
            } else if a == -1 && b == 1 && e == 1 {
                *dst -= v;
            } else {
                *dst += v;
            }
            dst = dst.add(1);
            src1 = src1.add(1);
            src2 = src2.add(1);
        }
    } else {
        let a = (*s).rev.aatk as i64;
        let b = (*s).rev.batk as i64;
        let e = (*s).rev.eatk as u32;
        let mut dst = (*aug).storage.i64_;
        let mut src1 = (*sig).storage.i64_;
        let mut src2 = (*other).storage.i64_;
        for _ in 0..repeat {
            let v = if a == 1 {
                (b + (*src1 + *src2)) >> e
            } else if a == -1 && b == 1 && e == 1 {
                (*src1 + *src2) >> e
            } else if a == -1 {
                (b - (*src1 + *src2)) >> e
            } else {
                (b + a * (*src1 + *src2)) >> e
            };
            if synthesis {
                if a == -1 && b == 1 && e == 1 {
                    *dst += v;
                } else {
                    *dst -= v;
                }
            } else if a == -1 && b == 1 && e == 1 {
                *dst -= v;
            } else {
                *dst += v;
            }
            dst = dst.add(1);
            src1 = src1.add(1);
            src2 = src2.add(1);
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn avx512_rev_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if (*src).flags & LFT_32BIT != 0 {
        let mut even = even;
        if width > 1 {
            let mut dph = (*hdst).storage.i32_;
            let mut dpl = (*ldst).storage.i32_;
            let mut sp = (*src).storage.i32_;
            let mut w = width;
            if !even {
                *dph = *sp;
                dph = dph.add(1);
                sp = sp.add(1);
                w -= 1;
            }
            while w > 1 {
                *dpl = *sp;
                *dph = *sp.add(1);
                dpl = dpl.add(1);
                dph = dph.add(1);
                sp = sp.add(2);
                w -= 2;
            }
            if w != 0 {
                *dpl = *sp;
            }
            let mut hp = (*hdst).storage.i32_;
            let mut lp = (*ldst).storage.i32_;
            let mut l_width = (width + if even { 1 } else { 0 }) >> 1;
            let mut h_width = (width + if even { 0 } else { 1 }) >> 1;
            let num_steps = (*atk).get_num_steps();
            for j in (0..num_steps).rev() {
                let s = (*atk).get_step(j);
                let a = (*s).rev.aatk as i32;
                let b = (*s).rev.batk as i32;
                let e = (*s).rev.eatk as u32;
                *lp.sub(1) = *lp;
                *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);
                let mut sp = lp.add(if even { 1 } else { 0 });
                let mut dp = hp;
                for _ in 0..h_width {
                    let v = if a == 1 {
                        (b + (*sp.sub(1) + *sp)) >> e
                    } else if a == -1 && b == 1 && e == 1 {
                        (*sp.sub(1) + *sp) >> e
                    } else if a == -1 {
                        (b - (*sp.sub(1) + *sp)) >> e
                    } else {
                        (b + a * (*sp.sub(1) + *sp)) >> e
                    };
                    if a == -1 && b == 1 && e == 1 {
                        *dp -= v;
                    } else {
                        *dp += v;
                    }
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
                core::mem::swap(&mut lp, &mut hp);
                even = !even;
                core::mem::swap(&mut l_width, &mut h_width);
            }
        } else if even {
            *(*ldst).storage.i32_ = *(*src).storage.i32_;
        } else {
            *(*hdst).storage.i32_ = *(*src).storage.i32_ << 1;
        }
    } else {
        let mut even = even;
        if width > 1 {
            let mut dph = (*hdst).storage.i64_;
            let mut dpl = (*ldst).storage.i64_;
            let mut sp = (*src).storage.i64_;
            let mut w = width;
            if !even {
                *dph = *sp;
                dph = dph.add(1);
                sp = sp.add(1);
                w -= 1;
            }
            while w > 1 {
                *dpl = *sp;
                *dph = *sp.add(1);
                dpl = dpl.add(1);
                dph = dph.add(1);
                sp = sp.add(2);
                w -= 2;
            }
            if w != 0 {
                *dpl = *sp;
            }
            let mut hp = (*hdst).storage.i64_;
            let mut lp = (*ldst).storage.i64_;
            let mut l_width = (width + if even { 1 } else { 0 }) >> 1;
            let mut h_width = (width + if even { 0 } else { 1 }) >> 1;
            let num_steps = (*atk).get_num_steps();
            for j in (0..num_steps).rev() {
                let s = (*atk).get_step(j);
                let a = (*s).rev.aatk as i64;
                let b = (*s).rev.batk as i64;
                let e = (*s).rev.eatk as u32;
                *lp.sub(1) = *lp;
                *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);
                let mut sp = lp.add(if even { 1 } else { 0 });
                let mut dp = hp;
                for _ in 0..h_width {
                    let v = if a == 1 {
                        (b + (*sp.sub(1) + *sp)) >> e
                    } else if a == -1 && b == 1 && e == 1 {
                        (*sp.sub(1) + *sp) >> e
                    } else if a == -1 {
                        (b - (*sp.sub(1) + *sp)) >> e
                    } else {
                        (b + a * (*sp.sub(1) + *sp)) >> e
                    };
                    if a == -1 && b == 1 && e == 1 {
                        *dp -= v;
                    } else {
                        *dp += v;
                    }
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
                core::mem::swap(&mut lp, &mut hp);
                even = !even;
                core::mem::swap(&mut l_width, &mut h_width);
            }
        } else if even {
            *(*ldst).storage.i64_ = *(*src).storage.i64_;
        } else {
            *(*hdst).storage.i64_ = *(*src).storage.i64_ << 1;
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn avx512_rev_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if (*dst).flags & LFT_32BIT != 0 {
        let mut ev = even;
        if width > 1 {
            let mut oth = (*hsrc).storage.i32_;
            let mut aug = (*lsrc).storage.i32_;
            let mut aug_width = (width + if even { 1 } else { 0 }) >> 1;
            let mut oth_width = (width + if even { 0 } else { 1 }) >> 1;
            let num_steps = (*atk).get_num_steps();
            for j in 0..num_steps {
                let s = (*atk).get_step(j);
                let a = (*s).rev.aatk as i32;
                let b = (*s).rev.batk as i32;
                let e = (*s).rev.eatk as u32;
                *oth.sub(1) = *oth;
                *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
                let mut sp = oth.add(if ev { 0 } else { 1 });
                let mut dp = aug;
                for _ in 0..aug_width {
                    let v = if a == 1 {
                        (b + (*sp.sub(1) + *sp)) >> e
                    } else if a == -1 && b == 1 && e == 1 {
                        (*sp.sub(1) + *sp) >> e
                    } else if a == -1 {
                        (b - (*sp.sub(1) + *sp)) >> e
                    } else {
                        (b + a * (*sp.sub(1) + *sp)) >> e
                    };
                    if a == -1 && b == 1 && e == 1 {
                        *dp += v;
                    } else {
                        *dp -= v;
                    }
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
                core::mem::swap(&mut aug, &mut oth);
                ev = !ev;
                core::mem::swap(&mut aug_width, &mut oth_width);
            }
            let mut sph = (*hsrc).storage.i32_;
            let mut spl = (*lsrc).storage.i32_;
            let mut dp = (*dst).storage.i32_;
            let mut w = width;
            if !even {
                *dp = *sph;
                dp = dp.add(1);
                sph = sph.add(1);
                w -= 1;
            }
            while w > 1 {
                *dp = *spl;
                *dp.add(1) = *sph;
                dp = dp.add(2);
                spl = spl.add(1);
                sph = sph.add(1);
                w -= 2;
            }
            if w != 0 {
                *dp = *spl;
            }
        } else if even {
            *(*dst).storage.i32_ = *(*lsrc).storage.i32_;
        } else {
            *(*dst).storage.i32_ = *(*hsrc).storage.i32_ >> 1;
        }
    } else {
        let mut ev = even;
        if width > 1 {
            let mut oth = (*hsrc).storage.i64_;
            let mut aug = (*lsrc).storage.i64_;
            let mut aug_width = (width + if even { 1 } else { 0 }) >> 1;
            let mut oth_width = (width + if even { 0 } else { 1 }) >> 1;
            let num_steps = (*atk).get_num_steps();
            for j in 0..num_steps {
                let s = (*atk).get_step(j);
                let a = (*s).rev.aatk as i64;
                let b = (*s).rev.batk as i64;
                let e = (*s).rev.eatk as u32;
                *oth.sub(1) = *oth;
                *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
                let mut sp = oth.add(if ev { 0 } else { 1 });
                let mut dp = aug;
                for _ in 0..aug_width {
                    let v = if a == 1 {
                        (b + (*sp.sub(1) + *sp)) >> e
                    } else if a == -1 && b == 1 && e == 1 {
                        (*sp.sub(1) + *sp) >> e
                    } else if a == -1 {
                        (b - (*sp.sub(1) + *sp)) >> e
                    } else {
                        (b + a * (*sp.sub(1) + *sp)) >> e
                    };
                    if a == -1 && b == 1 && e == 1 {
                        *dp += v;
                    } else {
                        *dp -= v;
                    }
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
                core::mem::swap(&mut aug, &mut oth);
                ev = !ev;
                core::mem::swap(&mut aug_width, &mut oth_width);
            }
            let mut sph = (*hsrc).storage.i64_;
            let mut spl = (*lsrc).storage.i64_;
            let mut dp = (*dst).storage.i64_;
            let mut w = width;
            if !even {
                *dp = *sph;
                dp = dp.add(1);
                sph = sph.add(1);
                w -= 1;
            }
            while w > 1 {
                *dp = *spl;
                *dp.add(1) = *sph;
                dp = dp.add(2);
                spl = spl.add(1);
                sph = sph.add(1);
                w -= 2;
            }
            if w != 0 {
                *dp = *spl;
            }
        } else if even {
            *(*dst).storage.i64_ = *(*lsrc).storage.i64_;
        } else {
            *(*dst).storage.i64_ = *(*hsrc).storage.i64_ >> 1;
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn avx512_irv_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    local_avx512_irv_vert_step(s, sig, other, aug, repeat, synthesis);
}

#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn avx512_irv_vert_times_k(k: f32, aug: *const LineBuf, repeat: Ui32) {
    local_avx512_irv_vert_times_k(k, aug, repeat);
}

#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn avx512_irv_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    local_avx512_irv_horz_ana(atk, ldst, hdst, src, width, even);
}

#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn avx512_irv_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    local_avx512_irv_horz_syn(atk, dst, lsrc, hsrc, width, even);
}
