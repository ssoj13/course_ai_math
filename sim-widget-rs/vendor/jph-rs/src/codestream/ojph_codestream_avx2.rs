//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codestream_avx2.cpp`

use core::ffi::c_void;

use crate::openjph::ojph_defs::{Si32, Si64, Ui32, Ui64};

#[cfg(target_arch = "x86")]
use core::arch::x86::{
    __m128i, __m256, __m256i, _mm_cvtsi32_si128, _mm_extract_epi32, _mm_loadu_si128, _mm_or_si128,
    _mm_shuffle_epi32, _mm256_abs_epi32, _mm256_add_epi64, _mm256_and_si256, _mm256_andnot_si256,
    _mm256_castsi256_ps, _mm256_cmpgt_epi32, _mm256_cmpgt_epi64, _mm256_cvtepi32_ps,
    _mm256_cvtps_epi32, _mm256_load_si256, _mm256_loadu_ps, _mm256_loadu_si256, _mm256_mul_ps,
    _mm256_or_ps, _mm256_or_si256, _mm256_set_epi32, _mm256_set_epi64x, _mm256_set1_epi32,
    _mm256_set1_epi64x, _mm256_set1_ps, _mm256_setzero_si256, _mm256_sign_epi32, _mm256_sll_epi32,
    _mm256_sll_epi64, _mm256_srl_epi64, _mm256_srlv_epi32, _mm256_storeu_ps, _mm256_storeu_si256,
    _mm256_xor_si256,
};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{
    __m128i, __m256, __m256i, _mm_cvtsi32_si128, _mm_extract_epi32, _mm_extract_epi64,
    _mm_loadu_si128, _mm_or_si128, _mm_shuffle_epi32, _mm256_abs_epi32, _mm256_add_epi64,
    _mm256_and_si256, _mm256_andnot_si256, _mm256_castsi256_ps, _mm256_cmpgt_epi32,
    _mm256_cmpgt_epi64, _mm256_cvtepi32_ps, _mm256_cvtps_epi32, _mm256_load_si256, _mm256_loadu_ps,
    _mm256_loadu_si256, _mm256_mul_ps, _mm256_or_ps, _mm256_or_si256, _mm256_set_epi32,
    _mm256_set_epi64x, _mm256_set1_epi32, _mm256_set1_epi64x, _mm256_set1_ps, _mm256_setzero_si256,
    _mm256_sign_epi32, _mm256_sll_epi32, _mm256_sll_epi64, _mm256_srl_epi64, _mm256_srlv_epi32,
    _mm256_storeu_ps, _mm256_storeu_si256, _mm256_xor_si256,
};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_find_max_val32(address: *mut Ui32) -> Ui32 {
    let mut x0: __m128i = _mm_loadu_si128(address as *const __m128i);
    let x1 = _mm_loadu_si128((address as *const __m128i).add(1));
    x0 = _mm_or_si128(x0, x1);
    let mut x1 = _mm_shuffle_epi32(x0, 0xEE);
    x0 = _mm_or_si128(x0, x1);
    x1 = _mm_shuffle_epi32(x0, 0x55);
    x0 = _mm_or_si128(x0, x1);
    _mm_extract_epi32(x0, 0) as Ui32
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_find_max_val64(address: *mut Ui64) -> Ui64 {
    let mut x0: __m128i = _mm_loadu_si128(address as *const __m128i);
    let x1 = _mm_loadu_si128((address as *const __m128i).add(1));
    x0 = _mm_or_si128(x0, x1);
    let x1 = _mm_shuffle_epi32(x0, 0xEE);
    x0 = _mm_or_si128(x0, x1);

    #[cfg(target_arch = "x86_64")]
    {
        _mm_extract_epi64(x0, 0) as Ui64
    }
    #[cfg(target_arch = "x86")]
    {
        let mut t = _mm_extract_epi32(x0, 0) as Ui64;
        t |= (_mm_extract_epi32(x0, 1) as Ui64) << 32;
        t
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_rev_tx_to_cb32(
    sp: *const c_void,
    dp: *mut Ui32,
    k_max: Ui32,
    _delta_inv: f32,
    mut count: Ui32,
    max_val: *mut Ui32,
) {
    let shift = 31 - k_max;
    let m0 = _mm256_set1_epi32(i32::MIN);
    let mut tmax = _mm256_loadu_si256(max_val as *const __m256i);
    let mut p = sp as *const __m256i;
    let mut q = dp;
    while count >= 8 {
        let v = _mm256_loadu_si256(p);
        let sign = _mm256_and_si256(v, m0);
        let mut val = _mm256_abs_epi32(v);
        val = _mm256_sll_epi32(val, _mm_cvtsi32_si128(shift as i32));
        tmax = _mm256_or_si256(tmax, val);
        val = _mm256_or_si256(val, sign);
        _mm256_storeu_si256(q as *mut __m256i, val);
        count -= 8;
        p = p.add(1);
        q = q.add(8);
    }
    if count != 0 {
        let v = _mm256_loadu_si256(p);
        let sign = _mm256_and_si256(v, m0);
        let mut val = _mm256_abs_epi32(v);
        val = _mm256_sll_epi32(val, _mm_cvtsi32_si128(shift as i32));

        let mut c = _mm256_set1_epi32(count as Si32);
        let idx = _mm256_set_epi32(7, 6, 5, 4, 3, 2, 1, 0);
        let mask = _mm256_cmpgt_epi32(c, idx);
        c = _mm256_and_si256(val, mask);
        tmax = _mm256_or_si256(tmax, c);

        val = _mm256_or_si256(val, sign);
        _mm256_storeu_si256(q as *mut __m256i, val);
    }
    _mm256_storeu_si256(max_val as *mut __m256i, tmax);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_irv_tx_to_cb32(
    sp: *const c_void,
    dp: *mut Ui32,
    _k_max: Ui32,
    delta_inv: f32,
    mut count: Ui32,
    max_val: *mut Ui32,
) {
    let d: __m256 = _mm256_set1_ps(delta_inv);
    let m0 = _mm256_set1_epi32(i32::MIN);
    let mut tmax = _mm256_loadu_si256(max_val as *const __m256i);
    let mut p = sp as *const f32;
    let mut q = dp;
    while count >= 8 {
        let mut vf = _mm256_loadu_ps(p);
        vf = _mm256_mul_ps(vf, d);
        let mut val = _mm256_cvtps_epi32(vf);
        let sign = _mm256_and_si256(val, m0);
        val = _mm256_abs_epi32(val);
        tmax = _mm256_or_si256(tmax, val);
        val = _mm256_or_si256(val, sign);
        _mm256_storeu_si256(q as *mut __m256i, val);
        count -= 8;
        p = p.add(8);
        q = q.add(8);
    }
    if count != 0 {
        let mut vf = _mm256_loadu_ps(p);
        vf = _mm256_mul_ps(vf, d);
        let mut val = _mm256_cvtps_epi32(vf);
        let sign = _mm256_and_si256(val, m0);
        val = _mm256_abs_epi32(val);

        let mut c = _mm256_set1_epi32(count as Si32);
        let idx = _mm256_set_epi32(7, 6, 5, 4, 3, 2, 1, 0);
        let mask = _mm256_cmpgt_epi32(c, idx);
        c = _mm256_and_si256(val, mask);
        tmax = _mm256_or_si256(tmax, c);

        val = _mm256_or_si256(val, sign);
        _mm256_storeu_si256(q as *mut __m256i, val);
    }
    _mm256_storeu_si256(max_val as *mut __m256i, tmax);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_rev_tx_from_cb32(
    sp: *const Ui32,
    dp: *mut c_void,
    k_max: Ui32,
    _delta: f32,
    count: Ui32,
) {
    let shift = 31 - k_max;
    let m1 = _mm256_set1_epi32(i32::MAX);
    let mut p = sp;
    let mut q = dp as *mut Si32;
    let mut i = 0;
    while i < count {
        let v = _mm256_load_si256(p as *const __m256i);
        let mut val = _mm256_and_si256(v, m1);
        val = _mm256_srlv_epi32(val, _mm256_set1_epi32(shift as i32));
        val = _mm256_sign_epi32(val, v);
        _mm256_storeu_si256(q as *mut __m256i, val);
        i += 8;
        p = p.add(8);
        q = q.add(8);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_irv_tx_from_cb32(
    sp: *const Ui32,
    dp: *mut c_void,
    _k_max: Ui32,
    delta: f32,
    count: Ui32,
) {
    let m1 = _mm256_set1_epi32(i32::MAX);
    let d: __m256 = _mm256_set1_ps(delta);
    let mut p = sp;
    let mut q = dp as *mut f32;
    let mut i = 0;
    while i < count {
        let v = _mm256_load_si256(p as *const __m256i);
        let vali = _mm256_and_si256(v, m1);
        let mut valf = _mm256_cvtepi32_ps(vali);
        valf = _mm256_mul_ps(valf, d);
        let sign = _mm256_andnot_si256(m1, v);
        valf = _mm256_or_ps(valf, _mm256_castsi256_ps(sign));
        _mm256_storeu_ps(q, valf);
        i += 8;
        p = p.add(8);
        q = q.add(8);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_rev_tx_to_cb64(
    sp: *const c_void,
    dp: *mut Ui64,
    k_max: Ui32,
    _delta_inv: f32,
    mut count: Ui32,
    max_val: *mut Ui64,
) {
    let shift = 63 - k_max;
    let m0 = _mm256_set1_epi64x(i64::MIN);
    let zero = _mm256_setzero_si256();
    let one = _mm256_set1_epi64x(1);
    let mut tmax = _mm256_loadu_si256(max_val as *const __m256i);
    let mut p = sp as *const __m256i;
    let mut q = dp;
    while count >= 4 {
        let v = _mm256_loadu_si256(p);
        let mut sign = _mm256_cmpgt_epi64(zero, v);
        let mut val = _mm256_xor_si256(v, sign);
        let ones = _mm256_and_si256(sign, one);
        val = _mm256_add_epi64(val, ones);
        sign = _mm256_and_si256(sign, m0);
        val = _mm256_sll_epi64(val, _mm_cvtsi32_si128(shift as i32));
        tmax = _mm256_or_si256(tmax, val);
        val = _mm256_or_si256(val, sign);
        _mm256_storeu_si256(q as *mut __m256i, val);
        count -= 4;
        p = p.add(1);
        q = q.add(4);
    }
    if count != 0 {
        let v = _mm256_loadu_si256(p);
        let mut sign = _mm256_cmpgt_epi64(zero, v);
        let mut val = _mm256_xor_si256(v, sign);
        let ones = _mm256_and_si256(sign, one);
        val = _mm256_add_epi64(val, ones);
        sign = _mm256_and_si256(sign, m0);
        val = _mm256_sll_epi64(val, _mm_cvtsi32_si128(shift as i32));

        let mut c = _mm256_set1_epi64x(count as Si64);
        let idx = _mm256_set_epi64x(3, 2, 1, 0);
        let mask = _mm256_cmpgt_epi64(c, idx);
        c = _mm256_and_si256(val, mask);
        tmax = _mm256_or_si256(tmax, c);

        val = _mm256_or_si256(val, sign);
        _mm256_storeu_si256(q as *mut __m256i, val);
    }
    _mm256_storeu_si256(max_val as *mut __m256i, tmax);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_rev_tx_from_cb64(
    sp: *const Ui64,
    dp: *mut c_void,
    k_max: Ui32,
    _delta: f32,
    count: Ui32,
) {
    let shift = 63 - k_max;
    let m1 = _mm256_set1_epi64x(i64::MAX);
    let zero = _mm256_setzero_si256();
    let one = _mm256_set1_epi64x(1);
    let mut p = sp;
    let mut q = dp as *mut Si64;
    let mut i = 0;
    while i < count {
        let v = _mm256_load_si256(p as *const __m256i);
        let mut val = _mm256_and_si256(v, m1);
        val = _mm256_srl_epi64(val, _mm_cvtsi32_si128(shift as i32));
        let sign = _mm256_cmpgt_epi64(zero, v);
        val = _mm256_xor_si256(val, sign);
        let ones = _mm256_and_si256(sign, one);
        val = _mm256_add_epi64(val, ones);
        _mm256_storeu_si256(q as *mut __m256i, val);
        i += 4;
        p = p.add(4);
        q = q.add(4);
    }
}
