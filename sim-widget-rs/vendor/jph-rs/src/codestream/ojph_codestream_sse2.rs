//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codestream_sse2.cpp`

use core::ffi::c_void;

use crate::openjph::ojph_defs::{Si32, Si64, Ui32, Ui64};

#[cfg(target_arch = "x86")]
use core::arch::x86::{
    __m128, __m128i, _mm_add_epi32, _mm_add_epi64, _mm_and_si128, _mm_andnot_si128,
    _mm_castsi128_ps, _mm_cmpgt_epi32, _mm_cmplt_epi32, _mm_cvtepi32_ps, _mm_cvtps_epi32,
    _mm_cvtsi32_si128, _mm_load_si128, _mm_loadu_ps, _mm_loadu_si128, _mm_mul_ps, _mm_or_ps,
    _mm_or_si128, _mm_set_epi32, _mm_set1_epi32, _mm_set1_epi64x, _mm_set1_ps, _mm_setzero_si128,
    _mm_shuffle_epi32, _mm_sll_epi32, _mm_sll_epi64, _mm_slli_epi32, _mm_srl_epi32, _mm_srl_epi64,
    _mm_storeu_ps, _mm_storeu_si128, _mm_xor_si128,
};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{
    __m128, __m128i, _mm_add_epi32, _mm_add_epi64, _mm_and_si128, _mm_andnot_si128,
    _mm_castsi128_ps, _mm_cmpgt_epi32, _mm_cmplt_epi32, _mm_cvtepi32_ps, _mm_cvtps_epi32,
    _mm_cvtsi32_si128, _mm_load_si128, _mm_loadu_ps, _mm_loadu_si128, _mm_mul_ps, _mm_or_ps,
    _mm_or_si128, _mm_set_epi32, _mm_set1_epi32, _mm_set1_epi64x, _mm_set1_ps, _mm_setzero_si128,
    _mm_shuffle_epi32, _mm_sll_epi32, _mm_sll_epi64, _mm_slli_epi32, _mm_srl_epi32, _mm_srl_epi64,
    _mm_storeu_ps, _mm_storeu_si128, _mm_xor_si128,
};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_find_max_val32(address: *mut Ui32) -> Ui32 {
    let mut x0: __m128i = _mm_loadu_si128(address as *const __m128i);
    let mut x1 = _mm_shuffle_epi32(x0, 0xEE);
    x0 = _mm_or_si128(x0, x1);
    x1 = _mm_shuffle_epi32(x0, 0x55);
    x0 = _mm_or_si128(x0, x1);
    _mm_storeu_si128(address as *mut __m128i, x0);
    *address
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_find_max_val64(address: *mut Ui64) -> Ui64 {
    let mut x0: __m128i = _mm_loadu_si128(address as *const __m128i);
    let x1 = _mm_shuffle_epi32(x0, 0xEE);
    x0 = _mm_or_si128(x0, x1);
    _mm_storeu_si128(address as *mut __m128i, x0);
    *address
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_rev_tx_to_cb32(
    sp: *const c_void,
    dp: *mut Ui32,
    k_max: Ui32,
    _delta_inv: f32,
    mut count: Ui32,
    max_val: *mut Ui32,
) {
    let shift = 31 - k_max;
    let m0 = _mm_set1_epi32(i32::MIN);
    let zero = _mm_setzero_si128();
    let one = _mm_set1_epi32(1);
    let mut tmax = _mm_loadu_si128(max_val as *const __m128i);
    let mut p = sp as *const __m128i;
    let mut q = dp;
    while count >= 4 {
        let v = _mm_loadu_si128(p);
        let mut sign = _mm_cmplt_epi32(v, zero);
        let mut val = _mm_xor_si128(v, sign);
        let ones = _mm_and_si128(sign, one);
        val = _mm_add_epi32(val, ones);
        sign = _mm_and_si128(sign, m0);
        val = _mm_sll_epi32(val, _mm_cvtsi32_si128(shift as i32));
        tmax = _mm_or_si128(tmax, val);
        val = _mm_or_si128(val, sign);
        _mm_storeu_si128(q as *mut __m128i, val);
        count -= 4;
        p = p.add(1);
        q = q.add(4);
    }
    if count != 0 {
        let v = _mm_loadu_si128(p);
        let mut sign = _mm_cmplt_epi32(v, zero);
        let mut val = _mm_xor_si128(v, sign);
        let ones = _mm_and_si128(sign, one);
        val = _mm_add_epi32(val, ones);
        sign = _mm_and_si128(sign, m0);
        val = _mm_sll_epi32(val, _mm_cvtsi32_si128(shift as i32));

        let mut c = _mm_set1_epi32(count as Si32);
        let idx = _mm_set_epi32(3, 2, 1, 0);
        let mask = _mm_cmpgt_epi32(c, idx);
        c = _mm_and_si128(val, mask);
        tmax = _mm_or_si128(tmax, c);

        val = _mm_or_si128(val, sign);
        _mm_storeu_si128(q as *mut __m128i, val);
    }
    _mm_storeu_si128(max_val as *mut __m128i, tmax);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_irv_tx_to_cb32(
    sp: *const c_void,
    dp: *mut Ui32,
    _k_max: Ui32,
    delta_inv: f32,
    mut count: Ui32,
    max_val: *mut Ui32,
) {
    let d: __m128 = _mm_set1_ps(delta_inv);
    let zero = _mm_setzero_si128();
    let one = _mm_set1_epi32(1);
    let mut tmax = _mm_loadu_si128(max_val as *const __m128i);
    let mut p = sp as *const f32;
    let mut q = dp;
    while count >= 4 {
        let mut vf = _mm_loadu_ps(p);
        vf = _mm_mul_ps(vf, d);
        let mut val = _mm_cvtps_epi32(vf);
        let mut sign = _mm_cmplt_epi32(val, zero);
        val = _mm_xor_si128(val, sign);
        let ones = _mm_and_si128(sign, one);
        val = _mm_add_epi32(val, ones);
        tmax = _mm_or_si128(tmax, val);
        sign = _mm_slli_epi32(sign, 31);
        val = _mm_or_si128(val, sign);
        _mm_storeu_si128(q as *mut __m128i, val);
        count -= 4;
        p = p.add(4);
        q = q.add(4);
    }
    if count != 0 {
        let mut vf = _mm_loadu_ps(p);
        vf = _mm_mul_ps(vf, d);
        let mut val = _mm_cvtps_epi32(vf);
        let mut sign = _mm_cmplt_epi32(val, zero);
        val = _mm_xor_si128(val, sign);
        let ones = _mm_and_si128(sign, one);
        val = _mm_add_epi32(val, ones);

        let mut c = _mm_set1_epi32(count as Si32);
        let idx = _mm_set_epi32(3, 2, 1, 0);
        let mask = _mm_cmpgt_epi32(c, idx);
        c = _mm_and_si128(val, mask);
        tmax = _mm_or_si128(tmax, c);

        sign = _mm_slli_epi32(sign, 31);
        val = _mm_or_si128(val, sign);
        _mm_storeu_si128(q as *mut __m128i, val);
    }
    _mm_storeu_si128(max_val as *mut __m128i, tmax);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_rev_tx_from_cb32(
    sp: *const Ui32,
    dp: *mut c_void,
    k_max: Ui32,
    _delta: f32,
    count: Ui32,
) {
    let shift = 31 - k_max;
    let m1 = _mm_set1_epi32(i32::MAX);
    let zero = _mm_setzero_si128();
    let one = _mm_set1_epi32(1);
    let mut p = sp;
    let mut q = dp as *mut Si32;
    let mut i = 0;
    while i < count {
        let v = _mm_load_si128(p as *const __m128i);
        let mut val = _mm_and_si128(v, m1);
        val = _mm_srl_epi32(val, _mm_cvtsi32_si128(shift as i32));
        let sign = _mm_cmplt_epi32(v, zero);
        val = _mm_xor_si128(val, sign);
        let ones = _mm_and_si128(sign, one);
        val = _mm_add_epi32(val, ones);
        _mm_storeu_si128(q as *mut __m128i, val);
        i += 4;
        p = p.add(4);
        q = q.add(4);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_irv_tx_from_cb32(
    sp: *const Ui32,
    dp: *mut c_void,
    _k_max: Ui32,
    delta: f32,
    count: Ui32,
) {
    let m1 = _mm_set1_epi32(i32::MAX);
    let d: __m128 = _mm_set1_ps(delta);
    let mut p = sp;
    let mut q = dp as *mut f32;
    let mut i = 0;
    while i < count {
        let v = _mm_load_si128(p as *const __m128i);
        let vali = _mm_and_si128(v, m1);
        let mut valf = _mm_cvtepi32_ps(vali);
        valf = _mm_mul_ps(valf, d);
        let sign = _mm_andnot_si128(m1, v);
        valf = _mm_or_ps(valf, _mm_castsi128_ps(sign));
        _mm_storeu_ps(q, valf);
        i += 4;
        p = p.add(4);
        q = q.add(4);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_rev_tx_to_cb64(
    sp: *const c_void,
    dp: *mut Ui64,
    k_max: Ui32,
    _delta_inv: f32,
    mut count: Ui32,
    max_val: *mut Ui64,
) {
    let shift = 63 - k_max;
    let m0 = _mm_set1_epi64x(i64::MIN);
    let zero = _mm_setzero_si128();
    let one = _mm_set1_epi64x(1);
    let mut tmax = _mm_loadu_si128(max_val as *const __m128i);
    let mut p = sp as *const __m128i;
    let mut q = dp;
    while count >= 2 {
        let v = _mm_loadu_si128(p);
        let mut sign = _mm_cmplt_epi32(v, zero);
        sign = _mm_shuffle_epi32(sign, 0xF5);
        let mut val = _mm_xor_si128(v, sign);
        let ones = _mm_and_si128(sign, one);
        val = _mm_add_epi64(val, ones);
        sign = _mm_and_si128(sign, m0);
        val = _mm_sll_epi64(val, _mm_cvtsi32_si128(shift as i32));
        tmax = _mm_or_si128(tmax, val);
        val = _mm_or_si128(val, sign);
        _mm_storeu_si128(q as *mut __m128i, val);
        count -= 2;
        p = p.add(1);
        q = q.add(2);
    }
    if count != 0 {
        let v = _mm_loadu_si128(p);
        let mut sign = _mm_cmplt_epi32(v, zero);
        sign = _mm_shuffle_epi32(sign, 0xF5);
        let mut val = _mm_xor_si128(v, sign);
        let ones = _mm_and_si128(sign, one);
        val = _mm_add_epi64(val, ones);
        sign = _mm_and_si128(sign, m0);
        val = _mm_sll_epi64(val, _mm_cvtsi32_si128(shift as i32));

        let mut c = _mm_set_epi32(0, 0, -1, -1);
        c = _mm_and_si128(val, c);
        tmax = _mm_or_si128(tmax, c);

        val = _mm_or_si128(val, sign);
        _mm_storeu_si128(q as *mut __m128i, val);
    }
    _mm_storeu_si128(max_val as *mut __m128i, tmax);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse2")]
pub unsafe fn sse2_rev_tx_from_cb64(
    sp: *const Ui64,
    dp: *mut c_void,
    k_max: Ui32,
    _delta: f32,
    count: Ui32,
) {
    let shift = 63 - k_max;
    let m1 = _mm_set1_epi64x(i64::MAX);
    let zero = _mm_setzero_si128();
    let one = _mm_set1_epi64x(1);
    let mut p = sp;
    let mut q = dp as *mut Si64;
    let mut i = 0;
    while i < count {
        let v = _mm_load_si128(p as *const __m128i);
        let mut val = _mm_and_si128(v, m1);
        val = _mm_srl_epi64(val, _mm_cvtsi32_si128(shift as i32));
        let mut sign = _mm_cmplt_epi32(v, zero);
        sign = _mm_shuffle_epi32(sign, 0xF5);
        val = _mm_xor_si128(val, sign);
        let ones = _mm_and_si128(sign, one);
        val = _mm_add_epi64(val, ones);
        _mm_storeu_si128(q as *mut __m128i, val);
        i += 2;
        p = p.add(2);
        q = q.add(2);
    }
}
