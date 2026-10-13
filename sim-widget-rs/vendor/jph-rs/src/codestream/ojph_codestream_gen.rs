//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.31.0/src/core/codestream/ojph_codestream_gen.cpp`

use core::ffi::c_void;

use crate::openjph::ojph_arch::ojph_trunc;
use crate::openjph::ojph_defs::{Si32, Si64, Ui32, Ui64};

#[inline]
pub unsafe fn gen_mem_clear(addr: *mut c_void, count: usize) {
    let mut p = addr as *mut Si64;
    let mut i = 0usize;
    while i < count {
        *p = 0;
        p = p.add(1);
        i += 8;
    }
}

#[inline]
pub unsafe fn gen_find_max_val32(addr: *mut Ui32) -> Ui32 {
    *addr
}

#[inline]
pub unsafe fn gen_find_max_val64(addr: *mut Ui64) -> Ui64 {
    *addr
}

#[inline]
pub unsafe fn gen_rev_tx_to_cb32(
    sp: *const c_void,
    dp: *mut Ui32,
    k_max: Ui32,
    _delta_inv: f32,
    count: Ui32,
    max_val: *mut Ui32,
) {
    let shift = 31 - k_max;
    let mut tmax = *max_val;
    let mut p = sp as *const Si32;
    let mut q = dp;
    for _ in 0..count {
        let v = *p;
        p = p.add(1);
        let sign = if v >= 0 { 0 } else { 0x8000_0000 };
        let mut val = if v >= 0 { v as Ui32 } else { (-v) as Ui32 };
        val <<= shift;
        *q = sign | val;
        q = q.add(1);
        tmax |= val;
    }
    *max_val = tmax;
}

#[inline]
pub unsafe fn gen_rev_tx_to_cb64(
    sp: *const c_void,
    dp: *mut Ui64,
    k_max: Ui32,
    _delta_inv: f32,
    count: Ui32,
    max_val: *mut Ui64,
) {
    let shift = 63 - k_max;
    let mut tmax = *max_val;
    let mut p = sp as *const Si64;
    let mut q = dp;
    for _ in 0..count {
        let v = *p;
        p = p.add(1);
        let sign = if v >= 0 { 0 } else { 0x8000_0000_0000_0000 };
        let mut val = if v >= 0 { v as Ui64 } else { (-v) as Ui64 };
        val <<= shift;
        *q = sign | val;
        q = q.add(1);
        tmax |= val;
    }
    *max_val = tmax;
}

#[inline]
pub unsafe fn gen_irv_tx_to_cb32(
    sp: *const c_void,
    dp: *mut Ui32,
    _k_max: Ui32,
    delta_inv: f32,
    count: Ui32,
    max_val: *mut Ui32,
) {
    let mut tmax = *max_val;
    let mut p = sp as *const f32;
    let mut q = dp;
    for _ in 0..count {
        let v = *p;
        p = p.add(1);
        let t = ojph_trunc(v * delta_inv);
        let sign = if t >= 0 { 0 } else { 0x8000_0000 };
        let val = if t >= 0 { t as Ui32 } else { (-t) as Ui32 };
        *q = sign | val;
        q = q.add(1);
        tmax |= val;
    }
    *max_val = tmax;
}

#[inline]
pub unsafe fn gen_rev_tx_from_cb32(
    sp: *const Ui32,
    dp: *mut c_void,
    k_max: Ui32,
    _delta: f32,
    count: Ui32,
) {
    let shift = 31 - k_max;
    let mut p = sp;
    let mut q = dp as *mut Si32;
    for _ in 0..count {
        let v = *p;
        p = p.add(1);
        let val = ((v & 0x7FFF_FFFF) >> shift) as Si32;
        *q = if (v & 0x8000_0000) != 0 { -val } else { val };
        q = q.add(1);
    }
}

#[inline]
pub unsafe fn gen_rev_tx_from_cb64(
    sp: *const Ui64,
    dp: *mut c_void,
    k_max: Ui32,
    _delta: f32,
    count: Ui32,
) {
    let shift = 63 - k_max;
    let mut p = sp;
    let mut q = dp as *mut Si64;
    for _ in 0..count {
        let v = *p;
        p = p.add(1);
        let val = ((v & 0x7FFF_FFFF_FFFF_FFFF) >> shift) as Si64;
        *q = if (v & 0x8000_0000_0000_0000) != 0 {
            -val
        } else {
            val
        };
        q = q.add(1);
    }
}

#[inline]
pub unsafe fn gen_irv_tx_from_cb32(
    sp: *const Ui32,
    dp: *mut c_void,
    _k_max: Ui32,
    delta: f32,
    count: Ui32,
) {
    let mut p = sp;
    let mut q = dp as *mut f32;
    for _ in 0..count {
        let v = *p;
        p = p.add(1);
        let val = (v & 0x7FFF_FFFF) as f32 * delta;
        *q = if (v & 0x8000_0000) != 0 { -val } else { val };
        q = q.add(1);
    }
}

/// Added in OpenJPH v0.27+: the 64-bit irreversible from-codeblock conversion was
/// previously missing (`tx_from_cb64` stayed `None` for the irreversible/64-bit
/// combination — see `ojph_codeblock_fun.rs`). Mirrors `gen_irv_tx_from_cb32` above,
/// widened to 64 bits.
#[inline]
pub unsafe fn gen_irv_tx_from_cb64(
    sp: *const Ui64,
    dp: *mut c_void,
    _k_max: Ui32,
    delta: f32,
    count: Ui32,
) {
    let mut p = sp;
    let mut q = dp as *mut f32;
    for _ in 0..count {
        let v = *p;
        p = p.add(1);
        let val = (v & 0x7FFF_FFFF_FFFF_FFFF) as f32 * delta;
        *q = if (v & 0x8000_0000_0000_0000) != 0 {
            -val
        } else {
            val
        };
        q = q.add(1);
    }
}
