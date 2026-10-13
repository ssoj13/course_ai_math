//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/transform/ojph_colour_sse.cpp`

use crate::openjph::ojph_defs::Ui32;
use crate::transform::ojph_colour_local::CT_CNST;

#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse")]
pub unsafe fn sse_ict_forward(
    mut r: *const f32,
    mut g: *const f32,
    mut b: *const f32,
    mut y: *mut f32,
    mut cb: *mut f32,
    mut cr: *mut f32,
    repeat: Ui32,
) {
    let alpha_rf = _mm_set1_ps(CT_CNST::ALPHA_RF);
    let alpha_gf = _mm_set1_ps(CT_CNST::ALPHA_GF);
    let alpha_bf = _mm_set1_ps(CT_CNST::ALPHA_BF);
    let beta_cbf = _mm_set1_ps(CT_CNST::BETA_CbF);
    let beta_crf = _mm_set1_ps(CT_CNST::BETA_CrF);
    for _ in 0..((repeat + 3) >> 2) {
        let mr = unsafe { _mm_load_ps(r) };
        let mb = unsafe { _mm_load_ps(b) };
        let mut my = _mm_mul_ps(alpha_rf, mr);
        my = _mm_add_ps(my, unsafe { _mm_mul_ps(alpha_gf, _mm_load_ps(g)) });
        my = _mm_add_ps(my, _mm_mul_ps(alpha_bf, mb));
        unsafe { _mm_store_ps(y, my) };
        unsafe { _mm_store_ps(cb, _mm_mul_ps(beta_cbf, _mm_sub_ps(mb, my))) };
        unsafe { _mm_store_ps(cr, _mm_mul_ps(beta_crf, _mm_sub_ps(mr, my))) };

        r = unsafe { r.add(4) };
        g = unsafe { g.add(4) };
        b = unsafe { b.add(4) };
        y = unsafe { y.add(4) };
        cb = unsafe { cb.add(4) };
        cr = unsafe { cr.add(4) };
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse")]
pub unsafe fn sse_ict_backward(
    mut y: *const f32,
    mut cb: *const f32,
    mut cr: *const f32,
    mut r: *mut f32,
    mut g: *mut f32,
    mut b: *mut f32,
    repeat: Ui32,
) {
    let gamma_cr2g = _mm_set1_ps(CT_CNST::GAMMA_CR2G);
    let gamma_cb2g = _mm_set1_ps(CT_CNST::GAMMA_CB2G);
    let gamma_cr2r = _mm_set1_ps(CT_CNST::GAMMA_CR2R);
    let gamma_cb2b = _mm_set1_ps(CT_CNST::GAMMA_CB2B);
    for _ in 0..((repeat + 3) >> 2) {
        let my = unsafe { _mm_load_ps(y) };
        let mcr = unsafe { _mm_load_ps(cr) };
        let mcb = unsafe { _mm_load_ps(cb) };
        let mg = _mm_sub_ps(my, _mm_mul_ps(gamma_cr2g, mcr));
        unsafe { _mm_store_ps(g, _mm_sub_ps(mg, _mm_mul_ps(gamma_cb2g, mcb))) };
        unsafe { _mm_store_ps(r, _mm_add_ps(my, _mm_mul_ps(gamma_cr2r, mcr))) };
        unsafe { _mm_store_ps(b, _mm_add_ps(my, _mm_mul_ps(gamma_cb2b, mcb))) };

        y = unsafe { y.add(4) };
        cb = unsafe { cb.add(4) };
        cr = unsafe { cr.add(4) };
        r = unsafe { r.add(4) };
        g = unsafe { g.add(4) };
        b = unsafe { b.add(4) };
    }
}
