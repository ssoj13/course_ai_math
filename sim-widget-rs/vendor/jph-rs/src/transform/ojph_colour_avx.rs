//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_colour_avx.cpp`

use crate::openjph::ojph_defs::Ui32;
use crate::transform::ojph_colour_local::CT_CNST;

#[cfg(target_arch = "x86")]
use core::arch::x86::{
    __m256, _mm256_add_ps, _mm256_load_ps, _mm256_mul_ps, _mm256_set1_ps, _mm256_store_ps,
    _mm256_sub_ps,
};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{
    __m256, _mm256_add_ps, _mm256_load_ps, _mm256_mul_ps, _mm256_set1_ps, _mm256_store_ps,
    _mm256_sub_ps,
};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
pub unsafe fn avx_ict_forward(
    mut r: *const f32,
    mut g: *const f32,
    mut b: *const f32,
    mut y: *mut f32,
    mut cb: *mut f32,
    mut cr: *mut f32,
    repeat: Ui32,
) {
    let alpha_rf: __m256 = _mm256_set1_ps(CT_CNST::ALPHA_RF);
    let alpha_gf: __m256 = _mm256_set1_ps(CT_CNST::ALPHA_GF);
    let alpha_bf: __m256 = _mm256_set1_ps(CT_CNST::ALPHA_BF);
    let beta_cbf: __m256 = _mm256_set1_ps(CT_CNST::BETA_CbF);
    let beta_crf: __m256 = _mm256_set1_ps(CT_CNST::BETA_CrF);
    let mut i = ((repeat + 7) >> 3) as i32;
    while i > 0 {
        let mr = _mm256_load_ps(r);
        let mb = _mm256_load_ps(b);
        let mut my = _mm256_mul_ps(alpha_rf, mr);
        my = _mm256_add_ps(my, _mm256_mul_ps(alpha_gf, _mm256_load_ps(g)));
        my = _mm256_add_ps(my, _mm256_mul_ps(alpha_bf, mb));
        _mm256_store_ps(y, my);
        _mm256_store_ps(cb, _mm256_mul_ps(beta_cbf, _mm256_sub_ps(mb, my)));
        _mm256_store_ps(cr, _mm256_mul_ps(beta_crf, _mm256_sub_ps(mr, my)));

        r = r.add(8);
        g = g.add(8);
        b = b.add(8);
        y = y.add(8);
        cb = cb.add(8);
        cr = cr.add(8);
        i -= 1;
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx_ict_forward(
    mut r: *const f32,
    mut g: *const f32,
    mut b: *const f32,
    mut y: *mut f32,
    mut cb: *mut f32,
    mut cr: *mut f32,
    repeat: Ui32,
) {
    for _ in 0..repeat {
        let my = CT_CNST::ALPHA_RF * *r + CT_CNST::ALPHA_GF * *g + CT_CNST::ALPHA_BF * *b;
        *y = my;
        *cb = CT_CNST::BETA_CbF * (*b - my);
        *cr = CT_CNST::BETA_CrF * (*r - my);
        r = r.add(1);
        g = g.add(1);
        b = b.add(1);
        y = y.add(1);
        cb = cb.add(1);
        cr = cr.add(1);
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
pub unsafe fn avx_ict_backward(
    mut y: *const f32,
    mut cb: *const f32,
    mut cr: *const f32,
    mut r: *mut f32,
    mut g: *mut f32,
    mut b: *mut f32,
    repeat: Ui32,
) {
    let gamma_cr2g: __m256 = _mm256_set1_ps(CT_CNST::GAMMA_CR2G);
    let gamma_cb2g: __m256 = _mm256_set1_ps(CT_CNST::GAMMA_CB2G);
    let gamma_cr2r: __m256 = _mm256_set1_ps(CT_CNST::GAMMA_CR2R);
    let gamma_cb2b: __m256 = _mm256_set1_ps(CT_CNST::GAMMA_CB2B);
    let mut i = ((repeat + 7) >> 3) as i32;
    while i > 0 {
        let my = _mm256_load_ps(y);
        let mcr = _mm256_load_ps(cr);
        let mcb = _mm256_load_ps(cb);
        let mg = _mm256_sub_ps(my, _mm256_mul_ps(gamma_cr2g, mcr));
        _mm256_store_ps(g, _mm256_sub_ps(mg, _mm256_mul_ps(gamma_cb2g, mcb)));
        _mm256_store_ps(r, _mm256_add_ps(my, _mm256_mul_ps(gamma_cr2r, mcr)));
        _mm256_store_ps(b, _mm256_add_ps(my, _mm256_mul_ps(gamma_cb2b, mcb)));

        y = y.add(8);
        cb = cb.add(8);
        cr = cr.add(8);
        r = r.add(8);
        g = g.add(8);
        b = b.add(8);
        i -= 1;
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub unsafe fn avx_ict_backward(
    mut y: *const f32,
    mut cb: *const f32,
    mut cr: *const f32,
    mut r: *mut f32,
    mut g: *mut f32,
    mut b: *mut f32,
    repeat: Ui32,
) {
    for _ in 0..repeat {
        *g = *y - CT_CNST::GAMMA_CR2G * *cr - CT_CNST::GAMMA_CB2G * *cb;
        *r = *y + CT_CNST::GAMMA_CR2R * *cr;
        *b = *y + CT_CNST::GAMMA_CB2B * *cb;
        y = y.add(1);
        cb = cb.add(1);
        cr = cr.add(1);
        r = r.add(1);
        g = g.add(1);
        b = b.add(1);
    }
}
