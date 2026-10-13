//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codestream_avx.cpp`

use core::ffi::c_void;

#[cfg(target_arch = "x86")]
use core::arch::x86::{__m256, _mm256_setzero_ps, _mm256_storeu_ps};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{__m256, _mm256_setzero_ps, _mm256_storeu_ps};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx")]
pub unsafe fn avx_mem_clear(addr: *mut c_void, count: usize) {
    let mut p = addr as *mut f32;
    let zero: __m256 = _mm256_setzero_ps();
    let mut i = 0usize;
    while i < count {
        _mm256_storeu_ps(p, zero);
        p = p.add(8);
        i += 32;
    }
}
