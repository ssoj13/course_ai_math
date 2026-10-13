/// Scalar reference for [`crate::abs_diff_u8`].
pub(crate) unsafe fn abs_diff_u8(a: *const u8, b: *const u8, dst: *mut u8, n: usize) {
    for i in 0..n {
        let av = unsafe { *a.add(i) };
        let bv = unsafe { *b.add(i) };
        unsafe {
            *dst.add(i) = av.abs_diff(bv);
        }
    }
}

/// Scalar reference for [`crate::transpose_4x4_i16`].
///
/// Row-major 4×4 transpose: `dst[i*4 + j] = src[j*4 + i]`.
///
/// # Safety
/// `src` valid for 16 `i16` reads; `dst` valid for 16 `i16` writes;
/// non-overlapping.
pub(crate) unsafe fn transpose_4x4_i16(src: *const i16, dst: *mut i16) {
    for i in 0..4 {
        for j in 0..4 {
            let v = unsafe { *src.add(j * 4 + i) };
            unsafe {
                *dst.add(i * 4 + j) = v;
            }
        }
    }
}

/// Scalar reference for [`crate::transpose_8x8_i16`].
///
/// Row-major 8×8 transpose.
///
/// # Safety
/// `src` valid for 64 `i16` reads; `dst` valid for 64 `i16` writes;
/// non-overlapping.
pub(crate) unsafe fn transpose_8x8_i16(src: *const i16, dst: *mut i16) {
    for i in 0..8 {
        for j in 0..8 {
            let v = unsafe { *src.add(j * 8 + i) };
            unsafe {
                *dst.add(i * 8 + j) = v;
            }
        }
    }
}

/// Scalar reference for [`crate::sat_add_i16`].
pub(crate) unsafe fn sat_add_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    for i in 0..n {
        let av = unsafe { *a.add(i) } as i32;
        let bv = unsafe { *b.add(i) } as i32;
        let s = (av + bv).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        unsafe {
            *dst.add(i) = s;
        }
    }
}

/// Scalar reference for [`crate::sat_sub_i16`].
pub(crate) unsafe fn sat_sub_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    for i in 0..n {
        let av = unsafe { *a.add(i) } as i32;
        let bv = unsafe { *b.add(i) } as i32;
        let s = (av - bv).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        unsafe {
            *dst.add(i) = s;
        }
    }
}

/// Scalar reference for [`crate::abs_i16`].
pub(crate) unsafe fn abs_i16(src: *const i16, dst: *mut i16, n: usize) {
    for i in 0..n {
        let v = unsafe { *src.add(i) };
        let a = if v == i16::MIN { i16::MAX } else { v.abs() };
        unsafe {
            *dst.add(i) = a;
        }
    }
}

/// Scalar reference for [`crate::mul_hi_round_i16`].
///
/// Per lane: `dst[i] = ((a[i] as i32 * b[i] as i32 + (1 << 14)) >> 15) as i16`.
/// Saturating on overflow (only happens for `a == b == i16::MIN`).
pub(crate) unsafe fn mul_hi_round_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    for i in 0..n {
        let av = unsafe { *a.add(i) } as i32;
        let bv = unsafe { *b.add(i) } as i32;
        let p = av.wrapping_mul(bv).wrapping_add(1 << 14) >> 15;
        let s = p.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        unsafe {
            *dst.add(i) = s;
        }
    }
}

/// Scalar reference for [`crate::dequant_block_i16`].
///
/// In-place uniform dequant: `buf[i] = sat_i16((buf[i]*scale + round) >>
/// shift)`, `round = if shift==0 {0} else {1<<(shift-1)}`.  `scale` is
/// `i16` (covers typical dequant scales); the product `i16×i16` fits i32.
pub(crate) unsafe fn dequant_block_i16(buf: *mut i16, n: usize, scale: i16, shift: u32) {
    let sc = scale as i32;
    let round = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
    for i in 0..n {
        let p = unsafe { buf.add(i) };
        let v = ((unsafe { *p } as i32) * sc + round) >> shift;
        unsafe {
            *p = v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
    }
}

/// Scalar reference for [`crate::prefix_sum_bias_u8_inplace`].
///
/// `buf[0]` is left unchanged; every later byte becomes
/// `buf[i] = buf[i-1] + buf[i] - 128` (wrapping `u8` arithmetic), using the
/// ALREADY-UPDATED `buf[i-1]` (a true running prefix sum, not an elementwise
/// diff of the original array) — the OpenEXR ZIP/RLE first-difference
/// predictor's DECODE side (`reconstruct`, `internal_zip.c`'s scalar
/// fallback, `t[-1] + t[0] - 128`).
pub(crate) unsafe fn prefix_sum_bias_u8_inplace(buf: *mut u8, n: usize) {
    if n == 0 {
        return;
    }
    let mut prev = unsafe { *buf };
    for i in 1..n {
        let p = unsafe { buf.add(i) };
        let cur = prev.wrapping_add(unsafe { *p }).wrapping_sub(128);
        unsafe {
            *p = cur;
        }
        prev = cur;
    }
}

/// Scalar reference for [`crate::interleave_halves_u8`].
///
/// `out[2k] = t1[k]`, `out[2k+1] = t2[k]`, for `k` in range — the OpenEXR
/// ZIP/RLE byte-reorder predictor's DECODE-side inverse (`interleave`,
/// `internal_zip.c`'s scalar fallback). `out.len()` may be even or odd;
/// `t1`/`t2` must each be at least `ceil(out.len()/2)`/`floor(out.len()/2)`
/// long respectively (the caller's own `t2_start = ceil(n/2)` split).
pub(crate) unsafe fn interleave_halves_u8(t1: *const u8, t2: *const u8, out: *mut u8, n: usize) {
    let mut i1 = 0usize;
    let mut i2 = 0usize;
    let mut k = 0usize;
    while k < n {
        unsafe {
            *out.add(k) = *t1.add(i1);
        }
        i1 += 1;
        k += 1;
        if k < n {
            unsafe {
                *out.add(k) = *t2.add(i2);
            }
            i2 += 1;
            k += 1;
        }
    }
}
