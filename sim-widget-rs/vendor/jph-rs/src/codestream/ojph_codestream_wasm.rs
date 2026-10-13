//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codestream_wasm.cpp`

use core::ffi::c_void;

use crate::openjph::ojph_defs::{Si32, Si64, Ui32, Ui64};

#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::{
    f32x4_convert_i32x4, f32x4_mul, f32x4_splat, i8x16_bitmask, i32x4_add, i32x4_extract_lane,
    i32x4_gt, i32x4_lt, i32x4_make, i32x4_shl, i32x4_shuffle, i32x4_splat, i32x4_trunc_sat_f32x4,
    i64x2_add, i64x2_extract_lane, i64x2_lt, i64x2_make, i64x2_shl, i64x2_shuffle, i64x2_splat,
    u32x4_shr, u64x2_shr, v128, v128_and, v128_andnot, v128_load, v128_or, v128_store, v128_xor,
};

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub unsafe fn wasm_mem_clear(addr: *mut c_void, mut count: usize) {
    let mut p = addr as *mut f32;
    let zero = i32x4_splat(0);
    while count > 0 {
        v128_store(p as *mut v128, zero);
        p = p.add(4);
        count -= 16;
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_mem_clear(addr: *mut c_void, count: usize) {
    let mut p = addr as *mut Si64;
    let mut i = 0usize;
    while i < count {
        unsafe { *p = 0 };
        p = unsafe { p.add(1) };
        i += 8;
    }
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub unsafe fn wasm_find_max_val32(address: *mut Ui32) -> Ui32 {
    let mut x0 = v128_load(address as *const v128);
    let mut x1 = i32x4_shuffle::<2, 3, 2, 3>(x0, x0);
    x0 = v128_or(x0, x1);
    x1 = i32x4_shuffle::<1, 1, 1, 1>(x0, x0);
    x0 = v128_or(x0, x1);
    i32x4_extract_lane::<0>(x0) as Ui32
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_find_max_val32(address: *mut Ui32) -> Ui32 {
    unsafe { *address }
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub unsafe fn wasm_find_max_val64(address: *mut Ui64) -> Ui64 {
    let mut x0 = v128_load(address as *const v128);
    let x1 = i64x2_shuffle::<1, 1>(x0, x0);
    x0 = v128_or(x0, x1);
    i64x2_extract_lane::<0>(x0) as Ui64
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_find_max_val64(address: *mut Ui64) -> Ui64 {
    unsafe { *address }
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub unsafe fn wasm_rev_tx_to_cb32(
    sp: *const c_void,
    dp: *mut Ui32,
    k_max: Ui32,
    _delta_inv: f32,
    mut count: Ui32,
    max_val: *mut Ui32,
) {
    let shift = 31 - k_max;
    let m0 = i32x4_splat(i32::MIN);
    let zero = i32x4_splat(0);
    let one = i32x4_splat(1);
    let mut tmax = v128_load(max_val as *const v128);
    let mut p = sp as *const Si32;
    let mut q = dp;
    while count >= 4 {
        let v = v128_load(p as *const v128);
        let mut sign = i32x4_lt(v, zero);
        let mut val = v128_xor(v, sign);
        let ones = v128_and(sign, one);
        val = i32x4_add(val, ones);
        sign = v128_and(sign, m0);
        val = i32x4_shl(val, shift);
        tmax = v128_or(tmax, val);
        val = v128_or(val, sign);
        v128_store(q as *mut v128, val);
        count -= 4;
        p = p.add(4);
        q = q.add(4);
    }
    if count != 0 {
        let v = v128_load(p as *const v128);
        let mut sign = i32x4_lt(v, zero);
        let mut val = v128_xor(v, sign);
        let ones = v128_and(sign, one);
        val = i32x4_add(val, ones);
        sign = v128_and(sign, m0);
        val = i32x4_shl(val, shift);

        let c = i32x4_splat(count as Si32);
        let idx = i32x4_make(0, 1, 2, 3);
        let mask = i32x4_gt(c, idx);
        let c = v128_and(val, mask);
        tmax = v128_or(tmax, c);

        val = v128_or(val, sign);
        v128_store(q as *mut v128, val);
    }
    v128_store(max_val as *mut v128, tmax);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_tx_to_cb32(
    sp: *const c_void,
    dp: *mut Ui32,
    k_max: Ui32,
    _delta_inv: f32,
    count: Ui32,
    max_val: *mut Ui32,
) {
    let shift = 31 - k_max;
    let mut tmax = unsafe { *max_val };
    let mut p = sp as *const Si32;
    let mut q = dp;
    for _ in 0..count {
        let v = unsafe { *p };
        p = unsafe { p.add(1) };
        let sign = if v >= 0 { 0 } else { 0x8000_0000 };
        let mut val = if v >= 0 { v as Ui32 } else { (-v) as Ui32 };
        val <<= shift;
        unsafe { *q = sign | val };
        q = unsafe { q.add(1) };
        tmax |= val;
    }
    unsafe { *max_val = tmax };
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub unsafe fn wasm_irv_tx_to_cb32(
    sp: *const c_void,
    dp: *mut Ui32,
    _k_max: Ui32,
    delta_inv: f32,
    mut count: Ui32,
    max_val: *mut Ui32,
) {
    let d = f32x4_splat(delta_inv);
    let zero = i32x4_splat(0);
    let one = i32x4_splat(1);
    let mut tmax = v128_load(max_val as *const v128);
    let mut p = sp as *const f32;
    let mut q = dp;
    while count >= 4 {
        let mut vf = v128_load(p as *const v128);
        vf = f32x4_mul(vf, d);
        let mut val = i32x4_trunc_sat_f32x4(vf);
        let mut sign = i32x4_lt(val, zero);
        val = v128_xor(val, sign);
        let ones = v128_and(sign, one);
        val = i32x4_add(val, ones);
        tmax = v128_or(tmax, val);
        sign = i32x4_shl(sign, 31);
        val = v128_or(val, sign);
        v128_store(q as *mut v128, val);
        count -= 4;
        p = p.add(4);
        q = q.add(4);
    }
    if count != 0 {
        let mut vf = v128_load(p as *const v128);
        vf = f32x4_mul(vf, d);
        let mut val = i32x4_trunc_sat_f32x4(vf);
        let mut sign = i32x4_lt(val, zero);
        val = v128_xor(val, sign);
        let ones = v128_and(sign, one);
        val = i32x4_add(val, ones);

        let c = i32x4_splat(count as Si32);
        let idx = i32x4_make(0, 1, 2, 3);
        let mask = i32x4_gt(c, idx);
        let c = v128_and(val, mask);
        tmax = v128_or(tmax, c);

        sign = i32x4_shl(sign, 31);
        val = v128_or(val, sign);
        v128_store(q as *mut v128, val);
    }
    v128_store(max_val as *mut v128, tmax);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_irv_tx_to_cb32(
    sp: *const c_void,
    dp: *mut Ui32,
    _k_max: Ui32,
    delta_inv: f32,
    count: Ui32,
    max_val: *mut Ui32,
) {
    let mut tmax = unsafe { *max_val };
    let mut p = sp as *const f32;
    let mut q = dp;
    for _ in 0..count {
        let v = unsafe { *p };
        p = unsafe { p.add(1) };
        let t = crate::openjph::ojph_arch::ojph_trunc(v * delta_inv);
        let sign = if t >= 0 { 0 } else { 0x8000_0000 };
        let val = if t >= 0 { t as Ui32 } else { (-t) as Ui32 };
        unsafe { *q = sign | val };
        q = unsafe { q.add(1) };
        tmax |= val;
    }
    unsafe { *max_val = tmax };
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub unsafe fn wasm_rev_tx_from_cb32(
    sp: *const Ui32,
    dp: *mut c_void,
    k_max: Ui32,
    _delta: f32,
    count: Ui32,
) {
    let shift = 31 - k_max;
    let m1 = i32x4_splat(i32::MAX);
    let zero = i32x4_splat(0);
    let one = i32x4_splat(1);
    let mut p = sp;
    let mut q = dp as *mut Si32;
    let mut i = 0;
    while i < count {
        let v = v128_load(p as *const v128);
        let mut val = v128_and(v, m1);
        val = u32x4_shr(val, shift);
        let sign = i32x4_lt(v, zero);
        val = v128_xor(val, sign);
        let ones = v128_and(sign, one);
        val = i32x4_add(val, ones);
        v128_store(q as *mut v128, val);
        i += 4;
        p = p.add(4);
        q = q.add(4);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_tx_from_cb32(
    sp: *const Ui32,
    dp: *mut c_void,
    k_max: Ui32,
    delta: f32,
    count: Ui32,
) {
    let _ = delta;
    let shift = 31 - k_max;
    let mut p = sp;
    let mut q = dp as *mut Si32;
    for _ in 0..count {
        let v = unsafe { *p };
        p = unsafe { p.add(1) };
        let val = ((v & 0x7FFF_FFFF) >> shift) as Si32;
        unsafe { *q = if (v & 0x8000_0000) != 0 { -val } else { val } };
        q = unsafe { q.add(1) };
    }
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub unsafe fn wasm_irv_tx_from_cb32(
    sp: *const Ui32,
    dp: *mut c_void,
    _k_max: Ui32,
    delta: f32,
    count: Ui32,
) {
    let m1 = i32x4_splat(i32::MAX);
    let d = f32x4_splat(delta);
    let mut p = sp;
    let mut q = dp as *mut f32;
    let mut i = 0;
    while i < count {
        let v = v128_load(p as *const v128);
        let vali = v128_and(v, m1);
        let mut valf = f32x4_convert_i32x4(vali);
        valf = f32x4_mul(valf, d);
        let sign = v128_andnot(v, m1);
        valf = v128_or(valf, sign);
        v128_store(q as *mut v128, valf);
        i += 4;
        p = p.add(4);
        q = q.add(4);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_irv_tx_from_cb32(
    sp: *const Ui32,
    dp: *mut c_void,
    k_max: Ui32,
    delta: f32,
    count: Ui32,
) {
    let _ = k_max;
    let mut p = sp;
    let mut q = dp as *mut f32;
    for _ in 0..count {
        let v = unsafe { *p };
        p = unsafe { p.add(1) };
        let val = (v & 0x7FFF_FFFF) as f32 * delta;
        unsafe { *q = if (v & 0x8000_0000) != 0 { -val } else { val } };
        q = unsafe { q.add(1) };
    }
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub unsafe fn wasm_rev_tx_to_cb64(
    sp: *const c_void,
    dp: *mut Ui64,
    k_max: Ui32,
    _delta_inv: f32,
    mut count: Ui32,
    max_val: *mut Ui64,
) {
    let shift = 63 - k_max;
    let m0 = i64x2_splat(i64::MIN);
    let zero = i64x2_splat(0);
    let one = i64x2_splat(1);
    let mut tmax = v128_load(max_val as *const v128);
    let mut p = sp as *const Si64;
    let mut q = dp;
    while count >= 2 {
        let v = v128_load(p as *const v128);
        let mut sign = i64x2_lt(v, zero);
        let mut val = v128_xor(v, sign);
        let ones = v128_and(sign, one);
        val = i64x2_add(val, ones);
        sign = v128_and(sign, m0);
        val = i64x2_shl(val, shift);
        tmax = v128_or(tmax, val);
        val = v128_or(val, sign);
        v128_store(q as *mut v128, val);
        count -= 2;
        p = p.add(2);
        q = q.add(2);
    }
    if count != 0 {
        let v = v128_load(p as *const v128);
        let mut sign = i64x2_lt(v, zero);
        let mut val = v128_xor(v, sign);
        let ones = v128_and(sign, one);
        val = i64x2_add(val, ones);
        sign = v128_and(sign, m0);
        val = i64x2_shl(val, shift);

        let c = i32x4_make(-1, -1, 0, 0);
        let c = v128_and(val, c);
        tmax = v128_or(tmax, c);

        val = v128_or(val, sign);
        v128_store(q as *mut v128, val);
    }
    v128_store(max_val as *mut v128, tmax);
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_tx_to_cb64(
    sp: *const c_void,
    dp: *mut Ui64,
    k_max: Ui32,
    delta_inv: f32,
    count: Ui32,
    max_val: *mut Ui64,
) {
    let _ = delta_inv;
    let shift = 63 - k_max;
    let mut tmax = unsafe { *max_val };
    let mut p = sp as *const Si64;
    let mut q = dp;
    for _ in 0..count {
        let v = unsafe { *p };
        p = unsafe { p.add(1) };
        let sign = if v >= 0 { 0 } else { 0x8000_0000_0000_0000 };
        let mut val = if v >= 0 { v as Ui64 } else { (-v) as Ui64 };
        val <<= shift;
        unsafe { *q = sign | val };
        q = unsafe { q.add(1) };
        tmax |= val;
    }
    unsafe { *max_val = tmax };
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub unsafe fn wasm_rev_tx_from_cb64(
    sp: *const Ui64,
    dp: *mut c_void,
    k_max: Ui32,
    _delta: f32,
    count: Ui32,
) {
    let shift = 63 - k_max;
    let m1 = i64x2_splat(i64::MAX);
    let zero = i64x2_splat(0);
    let one = i64x2_splat(1);
    let mut p = sp;
    let mut q = dp as *mut Si64;
    let mut i = 0;
    while i < count {
        let v = v128_load(p as *const v128);
        let mut val = v128_and(v, m1);
        val = u64x2_shr(val, shift);
        let sign = i64x2_lt(v, zero);
        val = v128_xor(val, sign);
        let ones = v128_and(sign, one);
        val = i64x2_add(val, ones);
        v128_store(q as *mut v128, val);
        i += 2;
        p = p.add(2);
        q = q.add(2);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn wasm_rev_tx_from_cb64(
    sp: *const Ui64,
    dp: *mut c_void,
    k_max: Ui32,
    delta: f32,
    count: Ui32,
) {
    let _ = delta;
    let shift = 63 - k_max;
    let mut p = sp;
    let mut q = dp as *mut Si64;
    for _ in 0..count {
        let v = unsafe { *p };
        p = unsafe { p.add(1) };
        let val = ((v & 0x7FFF_FFFF_FFFF_FFFF) >> shift) as Si64;
        unsafe {
            *q = if (v & 0x8000_0000_0000_0000) != 0 {
                -val
            } else {
                val
            }
        };
        q = unsafe { q.add(1) };
    }
}
