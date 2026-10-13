//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codestream_sse.cpp`

use core::ffi::c_void;

#[cfg(target_arch = "x86")]
use core::arch::x86::{__m128, _mm_setzero_ps, _mm_storeu_ps};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{__m128, _mm_setzero_ps, _mm_storeu_ps};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse")]
pub unsafe fn sse_mem_clear(addr: *mut c_void, count: usize) {
    let mut p = addr as *mut f32;
    let zero: __m128 = _mm_setzero_ps();
    let mut i = 0usize;
    while i < count {
        _mm_storeu_ps(p, zero);
        p = p.add(4);
        i += 16;
    }
}
