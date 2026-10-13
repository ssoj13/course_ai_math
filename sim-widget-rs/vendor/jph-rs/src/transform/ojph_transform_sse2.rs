//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_transform_sse2.cpp`

use crate::codestream::ojph_params_local::{LiftingStep, ParamAtk};
use crate::openjph::ojph_defs::Ui32;
use crate::openjph::ojph_mem::{LFT_32BIT, LineBuf};

#[cfg(target_arch = "x86")]
use core::arch::x86::{
    __m128, __m128d, __m128i, _mm_add_epi32, _mm_add_epi64, _mm_cvtsi32_si128, _mm_load_pd,
    _mm_load_ps, _mm_load_si128, _mm_loadu_si128, _mm_set1_epi32, _mm_set1_epi64x, _mm_shuffle_pd,
    _mm_shuffle_ps, _mm_sra_epi32, _mm_srl_epi64, _mm_store_pd, _mm_store_ps, _mm_store_si128,
    _mm_sub_epi32, _mm_sub_epi64, _mm_unpackhi_pd, _mm_unpackhi_ps, _mm_unpacklo_pd,
    _mm_unpacklo_ps, _mm_xor_si128,
};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{
    __m128, __m128d, __m128i, _mm_add_epi32, _mm_add_epi64, _mm_cvtsi32_si128, _mm_load_pd,
    _mm_load_ps, _mm_load_si128, _mm_loadu_si128, _mm_set1_epi32, _mm_set1_epi64x, _mm_shuffle_pd,
    _mm_shuffle_ps, _mm_sra_epi32, _mm_srl_epi64, _mm_store_pd, _mm_store_ps, _mm_store_si128,
    _mm_sub_epi32, _mm_sub_epi64, _mm_unpackhi_pd, _mm_unpackhi_ps, _mm_unpacklo_pd,
    _mm_unpacklo_ps, _mm_xor_si128,
};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_mm_srai_epi64(a: __m128i, amt: i32, m: __m128i) -> __m128i {
    let mut x = _mm_srl_epi64(a, _mm_set1_epi64x(amt as i64));
    x = _mm_xor_si128(x, m);
    _mm_sub_epi64(x, m)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "sse2")]
unsafe fn sse2_deinterleave32(
    mut dpl: *mut f32,
    mut dph: *mut f32,
    mut sp: *mut f32,
    mut width: i32,
) {
    while width > 0 {
        let a = _mm_load_ps(sp);
        let b = _mm_load_ps(sp.add(4));
        let c: __m128 = _mm_shuffle_ps(a, b, 0b10_00_10_00);
        let d: __m128 = _mm_shuffle_ps(a, b, 0b11_01_11_01);
        _mm_store_ps(dpl, c);
        _mm_store_ps(dph, d);
        sp = sp.add(8);
        dpl = dpl.add(4);
        dph = dph.add(4);
        width -= 8;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "sse2")]
unsafe fn sse2_interleave32(
    mut dp: *mut f32,
    mut spl: *mut f32,
    mut sph: *mut f32,
    mut width: i32,
) {
    while width > 0 {
        let a = _mm_load_ps(spl);
        let b = _mm_load_ps(sph);
        let c: __m128 = _mm_unpacklo_ps(a, b);
        let d: __m128 = _mm_unpackhi_ps(a, b);
        _mm_store_ps(dp, c);
        _mm_store_ps(dp.add(4), d);
        dp = dp.add(8);
        spl = spl.add(4);
        sph = sph.add(4);
        width -= 8;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "sse2")]
unsafe fn sse2_deinterleave64(
    mut dpl: *mut f64,
    mut dph: *mut f64,
    mut sp: *mut f64,
    mut width: i32,
) {
    while width > 0 {
        let a = _mm_load_pd(sp);
        let b = _mm_load_pd(sp.add(2));
        let c: __m128d = _mm_shuffle_pd(a, b, 0);
        let d: __m128d = _mm_shuffle_pd(a, b, 3);
        _mm_store_pd(dpl, c);
        _mm_store_pd(dph, d);
        sp = sp.add(4);
        dpl = dpl.add(2);
        dph = dph.add(2);
        width -= 4;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
#[target_feature(enable = "sse2")]
unsafe fn sse2_interleave64(
    mut dp: *mut f64,
    mut spl: *mut f64,
    mut sph: *mut f64,
    mut width: i32,
) {
    while width > 0 {
        let a = _mm_load_pd(spl);
        let b = _mm_load_pd(sph);
        let c: __m128d = _mm_unpacklo_pd(a, b);
        let d: __m128d = _mm_unpackhi_pd(a, b);
        _mm_store_pd(dp, c);
        _mm_store_pd(dp.add(2), d);
        dp = dp.add(4);
        spl = spl.add(2);
        sph = sph.add(2);
        width -= 4;
    }
}

unsafe fn local_sse2_rev_vert_step32(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let a = (*s).rev.aatk as i32;
    let b = (*s).rev.batk as i32;
    let e = (*s).rev.eatk as u32;
    let mut dst = (*aug).storage.i32_;
    let mut src1 = (*sig).storage.i32_;
    let mut src2 = (*other).storage.i32_;
    if a == 1 {
        for _ in 0..repeat {
            if synthesis {
                *dst -= (b + (*src1 + *src2)) >> e;
            } else {
                *dst += (b + (*src1 + *src2)) >> e;
            }
            dst = dst.add(1);
            src1 = src1.add(1);
            src2 = src2.add(1);
        }
    } else if a == -1 && b == 1 && e == 1 {
        for _ in 0..repeat {
            if synthesis {
                *dst += (*src1 + *src2) >> e;
            } else {
                *dst -= (*src1 + *src2) >> e;
            }
            dst = dst.add(1);
            src1 = src1.add(1);
            src2 = src2.add(1);
        }
    } else if a == -1 {
        for _ in 0..repeat {
            if synthesis {
                *dst -= (b - (*src1 + *src2)) >> e;
            } else {
                *dst += (b - (*src1 + *src2)) >> e;
            }
            dst = dst.add(1);
            src1 = src1.add(1);
            src2 = src2.add(1);
        }
    } else {
        for _ in 0..repeat {
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

unsafe fn local_sse2_rev_vert_step64(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let a = (*s).rev.aatk as i64;
    let b = (*s).rev.batk as i64;
    let e = (*s).rev.eatk as u32;
    let mut dst = (*aug).storage.i64_;
    let mut src1 = (*sig).storage.i64_;
    let mut src2 = (*other).storage.i64_;
    if a == 1 {
        for _ in 0..repeat {
            if synthesis {
                *dst -= (b + (*src1 + *src2)) >> e;
            } else {
                *dst += (b + (*src1 + *src2)) >> e;
            }
            dst = dst.add(1);
            src1 = src1.add(1);
            src2 = src2.add(1);
        }
    } else if a == -1 && b == 1 && e == 1 {
        for _ in 0..repeat {
            if synthesis {
                *dst += (*src1 + *src2) >> e;
            } else {
                *dst -= (*src1 + *src2) >> e;
            }
            dst = dst.add(1);
            src1 = src1.add(1);
            src2 = src2.add(1);
        }
    } else if a == -1 {
        for _ in 0..repeat {
            if synthesis {
                *dst -= (b - (*src1 + *src2)) >> e;
            } else {
                *dst += (b - (*src1 + *src2)) >> e;
            }
            dst = dst.add(1);
            src1 = src1.add(1);
            src2 = src2.add(1);
        }
    } else {
        for _ in 0..repeat {
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

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
unsafe fn local_sse2_rev_vert_step(
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
        local_sse2_rev_vert_step32(s, sig, other, aug, repeat, synthesis);
    } else {
        local_sse2_rev_vert_step64(s, sig, other, aug, repeat, synthesis);
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
unsafe fn local_sse2_rev_horz_ana32(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
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
            if a == 1 {
                for _ in 0..h_width {
                    *dp += (b + (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else if a == -1 && b == 1 && e == 1 {
                for _ in 0..h_width {
                    *dp -= (*sp.sub(1) + *sp) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else if a == -1 {
                for _ in 0..h_width {
                    *dp += (b - (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else {
                for _ in 0..h_width {
                    *dp += (b + a * (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
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
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
unsafe fn local_sse2_rev_horz_ana64(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
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
            if a == 1 {
                for _ in 0..h_width {
                    *dp += (b + (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else if a == -1 && b == 1 && e == 1 {
                for _ in 0..h_width {
                    *dp -= (*sp.sub(1) + *sp) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else if a == -1 {
                for _ in 0..h_width {
                    *dp += (b - (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else {
                for _ in 0..h_width {
                    *dp += (b + a * (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
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

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
unsafe fn local_sse2_rev_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if (*src).flags & LFT_32BIT != 0 {
        local_sse2_rev_horz_ana32(atk, ldst, hdst, src, width, even);
    } else {
        local_sse2_rev_horz_ana64(atk, ldst, hdst, src, width, even);
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
unsafe fn local_sse2_rev_horz_syn32(
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
        for j in 0..num_steps {
            let s = (*atk).get_step(j);
            let a = (*s).rev.aatk as i32;
            let b = (*s).rev.batk as i32;
            let e = (*s).rev.eatk as u32;
            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
            let mut sp = oth.add(if ev { 0 } else { 1 });
            let mut dp = aug;
            if a == 1 {
                for _ in 0..aug_width {
                    *dp -= (b + (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else if a == -1 && b == 1 && e == 1 {
                for _ in 0..aug_width {
                    *dp += (*sp.sub(1) + *sp) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else if a == -1 {
                for _ in 0..aug_width {
                    *dp -= (b - (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else {
                for _ in 0..aug_width {
                    *dp -= (b + a * (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
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
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
unsafe fn local_sse2_rev_horz_syn64(
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
        for j in 0..num_steps {
            let s = (*atk).get_step(j);
            let a = (*s).rev.aatk as i64;
            let b = (*s).rev.batk as i64;
            let e = (*s).rev.eatk as u32;
            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
            let mut sp = oth.add(if ev { 0 } else { 1 });
            let mut dp = aug;
            if a == 1 {
                for _ in 0..aug_width {
                    *dp -= (b + (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else if a == -1 && b == 1 && e == 1 {
                for _ in 0..aug_width {
                    *dp += (*sp.sub(1) + *sp) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else if a == -1 {
                for _ in 0..aug_width {
                    *dp -= (b - (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else {
                for _ in 0..aug_width {
                    *dp -= (b + a * (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
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

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
unsafe fn local_sse2_rev_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if (*dst).flags & LFT_32BIT != 0 {
        local_sse2_rev_horz_syn32(atk, dst, lsrc, hsrc, width, even);
    } else {
        local_sse2_rev_horz_syn64(atk, dst, lsrc, hsrc, width, even);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
unsafe fn sse2_rev_vert_step32(
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
    let vb = _mm_set1_epi32(b);

    let mut dst = (*aug).storage.i32_;
    let mut src1 = (*sig).storage.i32_;
    let mut src2 = (*other).storage.i32_;

    if a == 1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm_load_si128(src1 as *const __m128i);
            let s2 = _mm_load_si128(src2 as *const __m128i);
            let mut d = _mm_load_si128(dst as *const __m128i);
            let t = _mm_add_epi32(s1, s2);
            let v = _mm_add_epi32(vb, t);
            let w = _mm_sra_epi32(v, _mm_cvtsi32_si128(e));
            d = if synthesis {
                _mm_sub_epi32(d, w)
            } else {
                _mm_add_epi32(d, w)
            };
            _mm_store_si128(dst as *mut __m128i, d);
            dst = dst.add(4);
            src1 = src1.add(4);
            src2 = src2.add(4);
            i -= 4;
        }
    } else if a == -1 && b == 1 && e == 1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm_load_si128(src1 as *const __m128i);
            let s2 = _mm_load_si128(src2 as *const __m128i);
            let mut d = _mm_load_si128(dst as *const __m128i);
            let t = _mm_add_epi32(s1, s2);
            let w = _mm_sra_epi32(t, _mm_cvtsi32_si128(e));
            d = if synthesis {
                _mm_add_epi32(d, w)
            } else {
                _mm_sub_epi32(d, w)
            };
            _mm_store_si128(dst as *mut __m128i, d);
            dst = dst.add(4);
            src1 = src1.add(4);
            src2 = src2.add(4);
            i -= 4;
        }
    } else if a == -1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm_load_si128(src1 as *const __m128i);
            let s2 = _mm_load_si128(src2 as *const __m128i);
            let mut d = _mm_load_si128(dst as *const __m128i);
            let t = _mm_add_epi32(s1, s2);
            let v = _mm_sub_epi32(vb, t);
            let w = _mm_sra_epi32(v, _mm_cvtsi32_si128(e));
            d = if synthesis {
                _mm_sub_epi32(d, w)
            } else {
                _mm_add_epi32(d, w)
            };
            _mm_store_si128(dst as *mut __m128i, d);
            dst = dst.add(4);
            src1 = src1.add(4);
            src2 = src2.add(4);
            i -= 4;
        }
    } else {
        local_sse2_rev_vert_step32(s, sig, other, aug, repeat, synthesis);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
unsafe fn sse2_rev_vert_step64(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let a = (*s).rev.aatk as i64;
    let b = (*s).rev.batk as i64;
    let e = (*s).rev.eatk as i32;
    let vb = _mm_set1_epi64x(b);
    let ve = _mm_set1_epi64x(1i64 << (63 - e));

    let mut dst = (*aug).storage.i64_;
    let mut src1 = (*sig).storage.i64_;
    let mut src2 = (*other).storage.i64_;

    if a == 1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm_load_si128(src1 as *const __m128i);
            let s2 = _mm_load_si128(src2 as *const __m128i);
            let mut d = _mm_load_si128(dst as *const __m128i);
            let t = _mm_add_epi64(s1, s2);
            let v = _mm_add_epi64(vb, t);
            let w = sse2_mm_srai_epi64(v, e, ve);
            d = if synthesis {
                _mm_sub_epi64(d, w)
            } else {
                _mm_add_epi64(d, w)
            };
            _mm_store_si128(dst as *mut __m128i, d);
            dst = dst.add(2);
            src1 = src1.add(2);
            src2 = src2.add(2);
            i -= 2;
        }
    } else if a == -1 && b == 1 && e == 1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm_load_si128(src1 as *const __m128i);
            let s2 = _mm_load_si128(src2 as *const __m128i);
            let mut d = _mm_load_si128(dst as *const __m128i);
            let t = _mm_add_epi64(s1, s2);
            let w = sse2_mm_srai_epi64(t, e, ve);
            d = if synthesis {
                _mm_add_epi64(d, w)
            } else {
                _mm_sub_epi64(d, w)
            };
            _mm_store_si128(dst as *mut __m128i, d);
            dst = dst.add(2);
            src1 = src1.add(2);
            src2 = src2.add(2);
            i -= 2;
        }
    } else if a == -1 {
        let mut i = repeat as i32;
        while i > 0 {
            let s1 = _mm_load_si128(src1 as *const __m128i);
            let s2 = _mm_load_si128(src2 as *const __m128i);
            let mut d = _mm_load_si128(dst as *const __m128i);
            let t = _mm_add_epi64(s1, s2);
            let v = _mm_sub_epi64(vb, t);
            let w = sse2_mm_srai_epi64(v, e, ve);
            d = if synthesis {
                _mm_sub_epi64(d, w)
            } else {
                _mm_add_epi64(d, w)
            };
            _mm_store_si128(dst as *mut __m128i, d);
            dst = dst.add(2);
            src1 = src1.add(2);
            src2 = src2.add(2);
            i -= 2;
        }
    } else {
        local_sse2_rev_vert_step64(s, sig, other, aug, repeat, synthesis);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_rev_vert_step(
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
        sse2_rev_vert_step32(s, sig, other, aug, repeat, synthesis);
    } else {
        sse2_rev_vert_step64(s, sig, other, aug, repeat, synthesis);
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn sse2_rev_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    local_sse2_rev_vert_step(s, sig, other, aug, repeat, synthesis);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
unsafe fn sse2_rev_horz_ana32(
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
        sse2_deinterleave32(dpl, dph, (*src).storage.f32_, width as i32);

        let mut hp = (*hdst).storage.i32_;
        let mut lp = (*ldst).storage.i32_;
        let mut l_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut h_width = (width + if even { 0 } else { 1 }) >> 1;
        let num_steps = (*atk).get_num_steps();
        for j in (0..num_steps).rev() {
            let s = (*atk).get_step(j);
            let a = (*s).rev.aatk as i32;
            let b = (*s).rev.batk as i32;
            let e = (*s).rev.eatk as i32;
            let vb = _mm_set1_epi32(b);

            *lp.sub(1) = *lp;
            *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);
            let mut sp = lp;
            let mut dp = hp;
            if a == 1 {
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let v = _mm_add_epi32(vb, t);
                        let w = _mm_sra_epi32(v, _mm_cvtsi32_si128(e));
                        d = _mm_add_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let v = _mm_add_epi32(vb, t);
                        let w = _mm_sra_epi32(v, _mm_cvtsi32_si128(e));
                        d = _mm_add_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let w = _mm_sra_epi32(t, _mm_cvtsi32_si128(e));
                        d = _mm_sub_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let w = _mm_sra_epi32(t, _mm_cvtsi32_si128(e));
                        d = _mm_sub_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else if a == -1 {
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let v = _mm_sub_epi32(vb, t);
                        let w = _mm_sra_epi32(v, _mm_cvtsi32_si128(e));
                        d = _mm_add_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let v = _mm_sub_epi32(vb, t);
                        let w = _mm_sra_epi32(v, _mm_cvtsi32_si128(e));
                        d = _mm_add_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else if even {
                for _ in 0..h_width {
                    *dp += (b + a * (*sp + *sp.add(1))) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else {
                for _ in 0..h_width {
                    *dp += (b + a * (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            }

            core::mem::swap(&mut lp, &mut hp);
            even = !even;
            core::mem::swap(&mut l_width, &mut h_width);
        }
    } else if even {
        (*ldst).storage.i32_.write((*src).storage.i32_.read());
    } else {
        (*hdst).storage.i32_.write((*src).storage.i32_.read() << 1);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
unsafe fn sse2_rev_horz_ana64(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
    if width > 1 {
        let dpl = if even {
            (*ldst).storage.i64_ as *mut f64
        } else {
            (*hdst).storage.i64_ as *mut f64
        };
        let dph = if even {
            (*hdst).storage.i64_ as *mut f64
        } else {
            (*ldst).storage.i64_ as *mut f64
        };
        let sp = (*src).storage.i64_ as *mut f64;
        sse2_deinterleave64(dpl, dph, sp, width as i32);

        let mut hp = (*hdst).storage.i64_;
        let mut lp = (*ldst).storage.i64_;
        let mut l_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut h_width = (width + if even { 0 } else { 1 }) >> 1;
        let num_steps = (*atk).get_num_steps();
        for j in (0..num_steps).rev() {
            let s = (*atk).get_step(j);
            let a = (*s).rev.aatk as i64;
            let b = (*s).rev.batk as i64;
            let e = (*s).rev.eatk as i32;
            let vb = _mm_set1_epi64x(b);
            let ve = _mm_set1_epi64x(1i64 << (63 - e));

            *lp.sub(1) = *lp;
            *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);
            let mut sp = lp;
            let mut dp = hp;
            if a == 1 {
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let v = _mm_add_epi64(vb, t);
                        let w = sse2_mm_srai_epi64(v, e, ve);
                        d = _mm_add_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let v = _mm_add_epi64(vb, t);
                        let w = sse2_mm_srai_epi64(v, e, ve);
                        d = _mm_add_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let w = sse2_mm_srai_epi64(t, e, ve);
                        d = _mm_sub_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let w = sse2_mm_srai_epi64(t, e, ve);
                        d = _mm_sub_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else if a == -1 {
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let v = _mm_sub_epi64(vb, t);
                        let w = sse2_mm_srai_epi64(v, e, ve);
                        d = _mm_add_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let v = _mm_sub_epi64(vb, t);
                        let w = sse2_mm_srai_epi64(v, e, ve);
                        d = _mm_add_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else if even {
                for _ in 0..h_width {
                    *dp += (b + a * (*sp + *sp.add(1))) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else {
                for _ in 0..h_width {
                    *dp += (b + a * (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            }

            core::mem::swap(&mut lp, &mut hp);
            even = !even;
            core::mem::swap(&mut l_width, &mut h_width);
        }
    } else if even {
        (*ldst).storage.i64_.write((*src).storage.i64_.read());
    } else {
        (*hdst).storage.i64_.write((*src).storage.i64_.read() << 1);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_rev_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if (*src).flags & LFT_32BIT != 0 {
        sse2_rev_horz_ana32(atk, ldst, hdst, src, width, even);
    } else {
        sse2_rev_horz_ana64(atk, ldst, hdst, src, width, even);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
unsafe fn sse2_rev_horz_syn32(
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
        for j in 0..num_steps {
            let s = (*atk).get_step(j);
            let a = (*s).rev.aatk as i32;
            let b = (*s).rev.batk as i32;
            let e = (*s).rev.eatk as i32;
            let vb = _mm_set1_epi32(b);

            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
            let mut sp = oth;
            let mut dp = aug;
            if a == 1 {
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let v = _mm_add_epi32(vb, t);
                        let w = _mm_sra_epi32(v, _mm_cvtsi32_si128(e));
                        d = _mm_sub_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let v = _mm_add_epi32(vb, t);
                        let w = _mm_sra_epi32(v, _mm_cvtsi32_si128(e));
                        d = _mm_sub_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let w = _mm_sra_epi32(t, _mm_cvtsi32_si128(e));
                        d = _mm_add_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let w = _mm_sra_epi32(t, _mm_cvtsi32_si128(e));
                        d = _mm_add_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else if a == -1 {
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let v = _mm_sub_epi32(vb, t);
                        let w = _mm_sra_epi32(v, _mm_cvtsi32_si128(e));
                        d = _mm_sub_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi32(s1, s2);
                        let v = _mm_sub_epi32(vb, t);
                        let w = _mm_sra_epi32(v, _mm_cvtsi32_si128(e));
                        d = _mm_sub_epi32(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else if ev {
                for _ in 0..aug_width {
                    *dp -= (b + a * (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else {
                for _ in 0..aug_width {
                    *dp -= (b + a * (*sp + *sp.add(1))) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
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
        sse2_interleave32(dp, spl, sph, width as i32);
    } else if even {
        (*dst).storage.i32_.write((*lsrc).storage.i32_.read());
    } else {
        (*dst).storage.i32_.write((*hsrc).storage.i32_.read() >> 1);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
unsafe fn sse2_rev_horz_syn64(
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
        for j in 0..num_steps {
            let s = (*atk).get_step(j);
            let a = (*s).rev.aatk as i64;
            let b = (*s).rev.batk as i64;
            let e = (*s).rev.eatk as i32;
            let vb = _mm_set1_epi64x(b);
            let ve = _mm_set1_epi64x(1i64 << (63 - e));

            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);
            let mut sp = oth;
            let mut dp = aug;
            if a == 1 {
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let v = _mm_add_epi64(vb, t);
                        let w = sse2_mm_srai_epi64(v, e, ve);
                        d = _mm_sub_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let v = _mm_add_epi64(vb, t);
                        let w = sse2_mm_srai_epi64(v, e, ve);
                        d = _mm_sub_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let w = sse2_mm_srai_epi64(t, e, ve);
                        d = _mm_add_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let w = sse2_mm_srai_epi64(t, e, ve);
                        d = _mm_add_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else if a == -1 {
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.sub(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let v = _mm_sub_epi64(vb, t);
                        let w = sse2_mm_srai_epi64(v, e, ve);
                        d = _mm_sub_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = _mm_load_si128(sp as *const __m128i);
                        let s2 = _mm_loadu_si128(sp.add(1) as *const __m128i);
                        let mut d = _mm_load_si128(dp as *const __m128i);
                        let t = _mm_add_epi64(s1, s2);
                        let v = _mm_sub_epi64(vb, t);
                        let w = sse2_mm_srai_epi64(v, e, ve);
                        d = _mm_sub_epi64(d, w);
                        _mm_store_si128(dp as *mut __m128i, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else if ev {
                for _ in 0..aug_width {
                    *dp -= (b + a * (*sp.sub(1) + *sp)) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            } else {
                for _ in 0..aug_width {
                    *dp -= (b + a * (*sp + *sp.add(1))) >> e;
                    sp = sp.add(1);
                    dp = dp.add(1);
                }
            }

            core::mem::swap(&mut aug, &mut oth);
            ev = !ev;
            core::mem::swap(&mut aug_width, &mut oth_width);
        }

        let dp = (*dst).storage.i64_ as *mut f64;
        let spl = if even {
            (*lsrc).storage.i64_ as *mut f64
        } else {
            (*hsrc).storage.i64_ as *mut f64
        };
        let sph = if even {
            (*hsrc).storage.i64_ as *mut f64
        } else {
            (*lsrc).storage.i64_ as *mut f64
        };
        sse2_interleave64(dp, spl, sph, width as i32);
    } else if even {
        (*dst).storage.i64_.write((*lsrc).storage.i64_.read());
    } else {
        (*dst).storage.i64_.write((*hsrc).storage.i64_.read() >> 1);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_rev_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    if (*dst).flags & LFT_32BIT != 0 {
        sse2_rev_horz_syn32(atk, dst, lsrc, hsrc, width, even);
    } else {
        sse2_rev_horz_syn64(atk, dst, lsrc, hsrc, width, even);
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn sse2_rev_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    local_sse2_rev_horz_ana(atk, ldst, hdst, src, width, even);
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn sse2_rev_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    local_sse2_rev_horz_syn(atk, dst, lsrc, hsrc, width, even);
}
