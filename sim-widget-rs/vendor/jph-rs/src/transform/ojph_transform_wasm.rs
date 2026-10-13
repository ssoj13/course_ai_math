//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_transform_wasm.cpp`

use crate::codestream::ojph_params_local::{LiftingStep, ParamAtk};
use crate::openjph::ojph_defs::Ui32;
use crate::openjph::ojph_mem::LineBuf;

#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::{
    f32x4_add, f32x4_mul, f32x4_splat, f32x4_sub, i32x4_add, i32x4_mul, i32x4_shr, i32x4_shuffle,
    i32x4_splat, i32x4_sub, i64x2_add, i64x2_mul, i64x2_shr, i64x2_shuffle, i64x2_splat, i64x2_sub,
    v128, v128_load, v128_store,
};

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_deinterleave32(
    mut dpl: *mut f32,
    mut dph: *mut f32,
    mut sp: *mut f32,
    mut width: i32,
) {
    while width > 0 {
        let a = v128_load(sp as *const v128);
        let b = v128_load(sp.add(4) as *const v128);
        let c = i32x4_shuffle::<0, 2, 4, 6>(a, b);
        let d = i32x4_shuffle::<1, 3, 5, 7>(a, b);
        v128_store(dpl as *mut v128, c);
        v128_store(dph as *mut v128, d);
        sp = sp.add(8);
        dpl = dpl.add(4);
        dph = dph.add(4);
        width -= 8;
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_interleave32(
    mut dp: *mut f32,
    mut spl: *mut f32,
    mut sph: *mut f32,
    mut width: i32,
) {
    while width > 0 {
        let a = v128_load(spl as *const v128);
        let b = v128_load(sph as *const v128);
        let c = i32x4_shuffle::<0, 4, 1, 5>(a, b);
        let d = i32x4_shuffle::<2, 6, 3, 7>(a, b);
        v128_store(dp as *mut v128, c);
        v128_store(dp.add(4) as *mut v128, d);
        dp = dp.add(8);
        spl = spl.add(4);
        sph = sph.add(4);
        width -= 8;
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_multiply_const(mut p: *mut f32, f: f32, mut width: i32) {
    let factor = f32x4_splat(f);
    while width > 0 {
        let s = v128_load(p as *const v128);
        v128_store(p as *mut v128, f32x4_mul(factor, s));
        p = p.add(4);
        width -= 4;
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_irv_vert_step(
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

    let factor = f32x4_splat(a);
    let mut dst = (*aug).storage.f32_;
    let mut src1 = (*sig).storage.f32_;
    let mut src2 = (*other).storage.f32_;
    let mut i = repeat as i32;
    while i > 0 {
        let s1 = v128_load(src1 as *const v128);
        let s2 = v128_load(src2 as *const v128);
        let d = v128_load(dst as *const v128);
        let d = f32x4_add(d, f32x4_mul(factor, f32x4_add(s1, s2)));
        v128_store(dst as *mut v128, d);
        dst = dst.add(4);
        src1 = src1.add(4);
        src2 = src2.add(4);
        i -= 4;
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_irv_vert_times_k(k: f32, aug: *const LineBuf, repeat: Ui32) {
    wasm_multiply_const((*aug).storage.f32_, k, repeat as i32);
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_irv_horz_ana(
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
            wasm_deinterleave32(dpl, dph, sp, width as i32);
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
            let f = f32x4_splat(a);
            if even {
                while i > 0 {
                    let m = v128_load(sp as *const v128);
                    let n = v128_load(sp.add(1) as *const v128);
                    let p = v128_load(dp as *const v128);
                    let p = f32x4_add(p, f32x4_mul(f, f32x4_add(m, n)));
                    v128_store(dp as *mut v128, p);
                    sp = sp.add(4);
                    dp = dp.add(4);
                    i -= 4;
                }
            } else {
                while i > 0 {
                    let m = v128_load(sp as *const v128);
                    let n = v128_load(sp.sub(1) as *const v128);
                    let p = v128_load(dp as *const v128);
                    let p = f32x4_add(p, f32x4_mul(f, f32x4_add(m, n)));
                    v128_store(dp as *mut v128, p);
                    sp = sp.add(4);
                    dp = dp.add(4);
                    i -= 4;
                }
            }

            core::mem::swap(&mut lp, &mut hp);
            even = !even;
            core::mem::swap(&mut l_width, &mut h_width);
        }

        let k = (*atk).get_k();
        let k_inv = 1.0f32 / k;
        wasm_multiply_const(lp, k_inv, l_width as i32);
        wasm_multiply_const(hp, k, h_width as i32);
    } else if even {
        *(*ldst).storage.f32_ = *(*src).storage.f32_;
    } else {
        *(*hdst).storage.f32_ = *(*src).storage.f32_ * 2.0f32;
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_irv_horz_syn(
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
        wasm_multiply_const(aug, k, aug_width as i32);
        wasm_multiply_const(oth, k_inv, oth_width as i32);

        let num_steps = (*atk).get_num_steps();
        for j in 0..num_steps {
            let s = (*atk).get_step(j);
            let a = (*s).irv.aatk;

            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);

            let mut sp = oth;
            let mut dp = aug;
            let mut i = aug_width as i32;
            let f = f32x4_splat(a);
            if ev {
                while i > 0 {
                    let m = v128_load(sp as *const v128);
                    let n = v128_load(sp.sub(1) as *const v128);
                    let p = v128_load(dp as *const v128);
                    let p = f32x4_sub(p, f32x4_mul(f, f32x4_add(m, n)));
                    v128_store(dp as *mut v128, p);
                    sp = sp.add(4);
                    dp = dp.add(4);
                    i -= 4;
                }
            } else {
                while i > 0 {
                    let m = v128_load(sp as *const v128);
                    let n = v128_load(sp.add(1) as *const v128);
                    let p = v128_load(dp as *const v128);
                    let p = f32x4_sub(p, f32x4_mul(f, f32x4_add(m, n)));
                    v128_store(dp as *mut v128, p);
                    sp = sp.add(4);
                    dp = dp.add(4);
                    i -= 4;
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
        wasm_interleave32(dp, spl, sph, width as i32);
    } else if even {
        *(*dst).storage.f32_ = *(*lsrc).storage.f32_;
    } else {
        *(*dst).storage.f32_ = *(*hsrc).storage.f32_ * 0.5f32;
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_deinterleave64(
    mut dpl: *mut f64,
    mut dph: *mut f64,
    mut sp: *mut f64,
    mut width: i32,
) {
    while width > 0 {
        // each v128 holds 2 f64; load 4 doubles as two v128 chunks
        let a = v128_load(sp as *const v128);
        let b = v128_load(sp.add(2) as *const v128);
        // even lanes (indices 0, 2) → low-pass; odd lanes (1, 3) → high-pass
        let c = i64x2_shuffle::<0, 2>(a, b);
        let d = i64x2_shuffle::<1, 3>(a, b);
        v128_store(dpl as *mut v128, c);
        v128_store(dph as *mut v128, d);
        sp = sp.add(4);
        dpl = dpl.add(2);
        dph = dph.add(2);
        width -= 4;
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_interleave64(
    mut dp: *mut f64,
    mut spl: *mut f64,
    mut sph: *mut f64,
    mut width: i32,
) {
    while width > 0 {
        let a = v128_load(spl as *const v128);
        let b = v128_load(sph as *const v128);
        // interleave: a[0],b[0] then a[1],b[1]
        let c = i64x2_shuffle::<0, 2>(a, b);
        let d = i64x2_shuffle::<1, 3>(a, b);
        v128_store(dp as *mut v128, c);
        v128_store(dp.add(2) as *mut v128, d);
        dp = dp.add(4);
        spl = spl.add(2);
        sph = sph.add(2);
        width -= 4;
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_rev_vert_step32(
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
    let va = i32x4_splat(a);
    let vb = i32x4_splat(b);

    let mut dst = (*aug).storage.i32_;
    let mut src1 = (*sig).storage.i32_;
    let mut src2 = (*other).storage.i32_;
    // The general definition of the wavelet in Part 2 is slightly
    // different to Part 1, although they are mathematically equivalent.
    // We identify the simpler Part 1 forms and use them directly.
    if a == 1 {
        // 5/3 update and any case with a == 1
        let mut i = repeat as i32;
        if synthesis {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i32x4_add(s1, s2);
                let v = i32x4_add(vb, t);
                let w = i32x4_shr(v, e);
                let d = i32x4_sub(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(4);
                src1 = src1.add(4);
                src2 = src2.add(4);
                i -= 4;
            }
        } else {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i32x4_add(s1, s2);
                let v = i32x4_add(vb, t);
                let w = i32x4_shr(v, e);
                let d = i32x4_add(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(4);
                src1 = src1.add(4);
                src2 = src2.add(4);
                i -= 4;
            }
        }
    } else if a == -1 && b == 1 && e == 1 {
        // 5/3 predict
        let mut i = repeat as i32;
        if synthesis {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i32x4_add(s1, s2);
                let w = i32x4_shr(t, e);
                let d = i32x4_add(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(4);
                src1 = src1.add(4);
                src2 = src2.add(4);
                i -= 4;
            }
        } else {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i32x4_add(s1, s2);
                let w = i32x4_shr(t, e);
                let d = i32x4_sub(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(4);
                src1 = src1.add(4);
                src2 = src2.add(4);
                i -= 4;
            }
        }
    } else if a == -1 {
        // any case with a == -1, which is not 5/3 predict
        let mut i = repeat as i32;
        if synthesis {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i32x4_add(s1, s2);
                let v = i32x4_sub(vb, t);
                let w = i32x4_shr(v, e);
                let d = i32x4_sub(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(4);
                src1 = src1.add(4);
                src2 = src2.add(4);
                i -= 4;
            }
        } else {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i32x4_add(s1, s2);
                let v = i32x4_sub(vb, t);
                let w = i32x4_shr(v, e);
                let d = i32x4_add(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(4);
                src1 = src1.add(4);
                src2 = src2.add(4);
                i -= 4;
            }
        }
    } else {
        // general case
        let mut i = repeat as i32;
        if synthesis {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i32x4_add(s1, s2);
                let u = i32x4_mul(va, t);
                let v = i32x4_add(vb, u);
                let w = i32x4_shr(v, e);
                let d = i32x4_sub(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(4);
                src1 = src1.add(4);
                src2 = src2.add(4);
                i -= 4;
            }
        } else {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i32x4_add(s1, s2);
                let u = i32x4_mul(va, t);
                let v = i32x4_add(vb, u);
                let w = i32x4_shr(v, e);
                let d = i32x4_add(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(4);
                src1 = src1.add(4);
                src2 = src2.add(4);
                i -= 4;
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_rev_vert_step64(
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
    let va = i64x2_splat(a as i64);
    let vb = i64x2_splat(b as i64);

    let mut dst = (*aug).storage.i64_;
    let mut src1 = (*sig).storage.i64_;
    let mut src2 = (*other).storage.i64_;
    if a == 1 {
        // 5/3 update and any case with a == 1
        let mut i = repeat as i32;
        if synthesis {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i64x2_add(s1, s2);
                let v = i64x2_add(vb, t);
                let w = i64x2_shr(v, e);
                let d = i64x2_sub(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(2);
                src1 = src1.add(2);
                src2 = src2.add(2);
                i -= 2;
            }
        } else {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i64x2_add(s1, s2);
                let v = i64x2_add(vb, t);
                let w = i64x2_shr(v, e);
                let d = i64x2_add(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(2);
                src1 = src1.add(2);
                src2 = src2.add(2);
                i -= 2;
            }
        }
    } else if a == -1 && b == 1 && e == 1 {
        // 5/3 predict
        let mut i = repeat as i32;
        if synthesis {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i64x2_add(s1, s2);
                let w = i64x2_shr(t, e);
                let d = i64x2_add(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(2);
                src1 = src1.add(2);
                src2 = src2.add(2);
                i -= 2;
            }
        } else {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i64x2_add(s1, s2);
                let w = i64x2_shr(t, e);
                let d = i64x2_sub(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(2);
                src1 = src1.add(2);
                src2 = src2.add(2);
                i -= 2;
            }
        }
    } else if a == -1 {
        // any case with a == -1, which is not 5/3 predict
        let mut i = repeat as i32;
        if synthesis {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i64x2_add(s1, s2);
                let v = i64x2_sub(vb, t);
                let w = i64x2_shr(v, e);
                let d = i64x2_sub(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(2);
                src1 = src1.add(2);
                src2 = src2.add(2);
                i -= 2;
            }
        } else {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i64x2_add(s1, s2);
                let v = i64x2_sub(vb, t);
                let w = i64x2_shr(v, e);
                let d = i64x2_add(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(2);
                src1 = src1.add(2);
                src2 = src2.add(2);
                i -= 2;
            }
        }
    } else {
        // general case
        let mut i = repeat as i32;
        if synthesis {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i64x2_add(s1, s2);
                let u = i64x2_mul(va, t);
                let v = i64x2_add(vb, u);
                let w = i64x2_shr(v, e);
                let d = i64x2_sub(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(2);
                src1 = src1.add(2);
                src2 = src2.add(2);
                i -= 2;
            }
        } else {
            while i > 0 {
                let s1 = v128_load(src1 as *const v128);
                let s2 = v128_load(src2 as *const v128);
                let d = v128_load(dst as *const v128);
                let t = i64x2_add(s1, s2);
                let u = i64x2_mul(va, t);
                let v = i64x2_add(vb, u);
                let w = i64x2_shr(v, e);
                let d = i64x2_add(d, w);
                v128_store(dst as *mut v128, d);
                dst = dst.add(2);
                src1 = src1.add(2);
                src2 = src2.add(2);
                i -= 2;
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_rev_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    use crate::openjph::ojph_mem::{LFT_32BIT, LFT_64BIT};
    if (!sig.is_null() && (*sig).flags & LFT_32BIT != 0)
        || (!aug.is_null() && (*aug).flags & LFT_32BIT != 0)
        || (!other.is_null() && (*other).flags & LFT_32BIT != 0)
    {
        debug_assert!(sig.is_null() || (*sig).flags & LFT_32BIT != 0);
        debug_assert!(other.is_null() || (*other).flags & LFT_32BIT != 0);
        debug_assert!(aug.is_null() || (*aug).flags & LFT_32BIT != 0);
        wasm_rev_vert_step32(s, sig, other, aug, repeat, synthesis);
    } else {
        debug_assert!(sig.is_null() || (*sig).flags & LFT_64BIT != 0);
        debug_assert!(other.is_null() || (*other).flags & LFT_64BIT != 0);
        debug_assert!(aug.is_null() || (*aug).flags & LFT_64BIT != 0);
        wasm_rev_vert_step64(s, sig, other, aug, repeat, synthesis);
    }
}

#[cfg(target_arch = "wasm32")]
unsafe fn wasm_rev_horz_ana32(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
    if width > 1 {
        {
            // deinterleave: split interleaved source into low/high subband buffers
            // use f32 view since the storage is bit-compatible before lifting steps
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
            wasm_deinterleave32(dpl, dph, sp, width as i32);
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
            let va = i32x4_splat(a);
            let vb = i32x4_splat(b);

            *lp.sub(1) = *lp;
            *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);

            let mut sp = lp as *const i32;
            let mut dp = hp;
            if a == 1 {
                // 5/3 update and any case with a == 1
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let v = i32x4_add(vb, t);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let v = i32x4_add(vb, t);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                // 5/3 predict
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let w = i32x4_shr(t, e);
                        let d = i32x4_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let w = i32x4_shr(t, e);
                        let d = i32x4_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else if a == -1 {
                // any case with a == -1, which is not 5/3 predict
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let v = i32x4_sub(vb, t);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let v = i32x4_sub(vb, t);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else {
                // general case
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let u = i32x4_mul(va, t);
                        let v = i32x4_add(vb, u);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let u = i32x4_mul(va, t);
                        let v = i32x4_add(vb, u);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
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

#[cfg(target_arch = "wasm32")]
unsafe fn wasm_rev_horz_ana64(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    mut even: bool,
) {
    if width > 1 {
        {
            // C++ uses ->p cast to double* because i64 and f64 share the same union slot
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
            let sp = (*src).storage.p as *mut f64;
            wasm_deinterleave64(dpl, dph, sp, width as i32);
        }

        let mut hp = (*hdst).storage.i64_;
        let mut lp = (*ldst).storage.i64_;
        let mut l_width = (width + if even { 1 } else { 0 }) >> 1;
        let mut h_width = (width + if even { 0 } else { 1 }) >> 1;
        let num_steps = (*atk).get_num_steps();
        for j in (0..num_steps).rev() {
            let s = (*atk).get_step(j);
            let a = (*s).rev.aatk as i32;
            let b = (*s).rev.batk as i32;
            let e = (*s).rev.eatk as u32;
            let va = i64x2_splat(a as i64);
            let vb = i64x2_splat(b as i64);

            *lp.sub(1) = *lp;
            *lp.add(l_width as usize) = *lp.add(l_width as usize - 1);

            let mut sp = lp as *const i64;
            let mut dp = hp;
            if a == 1 {
                // 5/3 update and any case with a == 1
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let v = i64x2_add(vb, t);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let v = i64x2_add(vb, t);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                // 5/3 predict
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let w = i64x2_shr(t, e);
                        let d = i64x2_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let w = i64x2_shr(t, e);
                        let d = i64x2_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else if a == -1 {
                // any case with a == -1, which is not 5/3 predict
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let v = i64x2_sub(vb, t);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let v = i64x2_sub(vb, t);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else {
                // general case
                let mut i = h_width as i32;
                if even {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let u = i64x2_mul(va, t);
                        let v = i64x2_add(vb, u);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let u = i64x2_mul(va, t);
                        let v = i64x2_add(vb, u);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
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

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_rev_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    use crate::openjph::ojph_mem::{LFT_32BIT, LFT_64BIT};
    if (*src).flags & LFT_32BIT != 0 {
        debug_assert!(ldst.is_null() || (*ldst).flags & LFT_32BIT != 0);
        debug_assert!(hdst.is_null() || (*hdst).flags & LFT_32BIT != 0);
        wasm_rev_horz_ana32(atk, ldst, hdst, src, width, even);
    } else {
        debug_assert!(ldst.is_null() || (*ldst).flags & LFT_64BIT != 0);
        debug_assert!(hdst.is_null() || (*hdst).flags & LFT_64BIT != 0);
        debug_assert!(src.is_null() || (*src).flags & LFT_64BIT != 0);
        wasm_rev_horz_ana64(atk, ldst, hdst, src, width, even);
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_rev_horz_syn32(
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
            let va = i32x4_splat(a);
            let vb = i32x4_splat(b);

            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);

            let mut sp = oth as *const i32;
            let mut dp = aug;
            if a == 1 {
                // 5/3 update and any case with a == 1
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let v = i32x4_add(vb, t);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let v = i32x4_add(vb, t);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                // 5/3 predict
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let w = i32x4_shr(t, e);
                        let d = i32x4_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let w = i32x4_shr(t, e);
                        let d = i32x4_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else if a == -1 {
                // any case with a == -1, which is not 5/3 predict
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let v = i32x4_sub(vb, t);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let v = i32x4_sub(vb, t);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            } else {
                // general case
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let u = i32x4_mul(va, t);
                        let v = i32x4_add(vb, u);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i32x4_add(s1, s2);
                        let u = i32x4_mul(va, t);
                        let v = i32x4_add(vb, u);
                        let w = i32x4_shr(v, e);
                        let d = i32x4_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(4);
                        dp = dp.add(4);
                        i -= 4;
                    }
                }
            }

            core::mem::swap(&mut aug, &mut oth);
            ev = !ev;
            core::mem::swap(&mut aug_width, &mut oth_width);
        }

        {
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
            wasm_interleave32(dp, spl, sph, width as i32);
        }
    } else if even {
        *(*dst).storage.i32_ = *(*lsrc).storage.i32_;
    } else {
        *(*dst).storage.i32_ = *(*hsrc).storage.i32_ >> 1;
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_rev_horz_syn64(
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
            let a = (*s).rev.aatk as i32;
            let b = (*s).rev.batk as i32;
            let e = (*s).rev.eatk as u32;
            let va = i64x2_splat(a as i64);
            let vb = i64x2_splat(b as i64);

            *oth.sub(1) = *oth;
            *oth.add(oth_width as usize) = *oth.add(oth_width as usize - 1);

            let mut sp = oth as *const i64;
            let mut dp = aug;
            if a == 1 {
                // 5/3 update and any case with a == 1
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let v = i64x2_add(vb, t);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let v = i64x2_add(vb, t);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else if a == -1 && b == 1 && e == 1 {
                // 5/3 predict
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let w = i64x2_shr(t, e);
                        let d = i64x2_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let w = i64x2_shr(t, e);
                        let d = i64x2_add(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else if a == -1 {
                // any case with a == -1, which is not 5/3 predict
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let v = i64x2_sub(vb, t);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let v = i64x2_sub(vb, t);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            } else {
                // general case
                let mut i = aug_width as i32;
                if ev {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.sub(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let u = i64x2_mul(va, t);
                        let v = i64x2_add(vb, u);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                } else {
                    while i > 0 {
                        let s1 = v128_load(sp as *const v128);
                        let s2 = v128_load(sp.add(1) as *const v128);
                        let d = v128_load(dp as *const v128);
                        let t = i64x2_add(s1, s2);
                        let u = i64x2_mul(va, t);
                        let v = i64x2_add(vb, u);
                        let w = i64x2_shr(v, e);
                        let d = i64x2_sub(d, w);
                        v128_store(dp as *mut v128, d);
                        sp = sp.add(2);
                        dp = dp.add(2);
                        i -= 2;
                    }
                }
            }

            core::mem::swap(&mut aug, &mut oth);
            ev = !ev;
            core::mem::swap(&mut aug_width, &mut oth_width);
        }

        {
            // C++ uses ->p cast to double* for the interleave output
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
            wasm_interleave64(dp, spl, sph, width as i32);
        }
    } else if even {
        *(*dst).storage.i64_ = *(*lsrc).storage.i64_;
    } else {
        *(*dst).storage.i64_ = *(*hsrc).storage.i64_ >> 1;
    }
}

#[cfg(target_arch = "wasm32")]
pub unsafe fn wasm_rev_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    use crate::openjph::ojph_mem::{LFT_32BIT, LFT_64BIT};
    if (*dst).flags & LFT_32BIT != 0 {
        debug_assert!(lsrc.is_null() || (*lsrc).flags & LFT_32BIT != 0);
        debug_assert!(hsrc.is_null() || (*hsrc).flags & LFT_32BIT != 0);
        wasm_rev_horz_syn32(atk, dst, lsrc, hsrc, width, even);
    } else {
        debug_assert!(dst.is_null() || (*dst).flags & LFT_64BIT != 0);
        debug_assert!(lsrc.is_null() || (*lsrc).flags & LFT_64BIT != 0);
        debug_assert!(hsrc.is_null() || (*hsrc).flags & LFT_64BIT != 0);
        wasm_rev_horz_syn64(atk, dst, lsrc, hsrc, width, even);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_deinterleave32(dpl: *mut f32, dph: *mut f32, sp: *mut f32, width: i32) {
    let _ = (dpl, dph, sp, width);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_interleave32(dp: *mut f32, spl: *mut f32, sph: *mut f32, width: i32) {
    let _ = (dp, spl, sph, width);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_multiply_const(p: *mut f32, f: f32, width: i32) {
    let _ = (p, f, width);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_irv_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let _ = (s, sig, other, aug, repeat, synthesis);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_irv_vert_times_k(k: f32, aug: *const LineBuf, repeat: Ui32) {
    let _ = (k, aug, repeat);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_irv_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    let _ = (atk, ldst, hdst, src, width, even);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_irv_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    let _ = (atk, dst, lsrc, hsrc, width, even);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_deinterleave64(dpl: *mut f64, dph: *mut f64, sp: *mut f64, width: i32) {
    let _ = (dpl, dph, sp, width);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_interleave64(dp: *mut f64, spl: *mut f64, sph: *mut f64, width: i32) {
    let _ = (dp, spl, sph, width);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_vert_step32(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let _ = (s, sig, other, aug, repeat, synthesis);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_vert_step64(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let _ = (s, sig, other, aug, repeat, synthesis);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_vert_step(
    s: *const LiftingStep,
    sig: *const LineBuf,
    other: *const LineBuf,
    aug: *const LineBuf,
    repeat: Ui32,
    synthesis: bool,
) {
    let _ = (s, sig, other, aug, repeat, synthesis);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_horz_ana(
    atk: *const ParamAtk,
    ldst: *const LineBuf,
    hdst: *const LineBuf,
    src: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    let _ = (atk, ldst, hdst, src, width, even);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_horz_syn32(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    let _ = (atk, dst, lsrc, hsrc, width, even);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_horz_syn64(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    let _ = (atk, dst, lsrc, hsrc, width, even);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_horz_syn(
    atk: *const ParamAtk,
    dst: *const LineBuf,
    lsrc: *const LineBuf,
    hsrc: *const LineBuf,
    width: Ui32,
    even: bool,
) {
    let _ = (atk, dst, lsrc, hsrc, width, even);
}
