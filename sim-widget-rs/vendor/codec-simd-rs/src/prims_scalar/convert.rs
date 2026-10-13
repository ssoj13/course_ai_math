/// Scalar reference for [`crate::sat_pack_i16_to_u8`].
pub(crate) unsafe fn sat_pack_i16_to_u8(src: *const i16, dst: *mut u8, n: usize) {
    for i in 0..n {
        let v = unsafe { *src.add(i) };
        unsafe {
            *dst.add(i) = v.clamp(0, 255) as u8;
        }
    }
}

/// Scalar reference for [`crate::sat_pack_i32_to_u16`].
pub(crate) unsafe fn sat_pack_i32_to_u16(src: *const i32, dst: *mut u16, n: usize, max: u16) {
    let max_i32 = max as i32;
    for i in 0..n {
        let v = unsafe { *src.add(i) };
        unsafe {
            *dst.add(i) = v.max(0).min(max_i32) as u16;
        }
    }
}

/// Scalar reference for [`crate::add_clip_to_pixel_u16`].
pub(crate) unsafe fn add_clip_to_pixel_u16(
    pred: *const u16,
    res: *const i16,
    dst: *mut u16,
    n: usize,
    max: u16,
) {
    let max_i32 = max as i32;
    for i in 0..n {
        let p = unsafe { *pred.add(i) } as i32;
        let r = unsafe { *res.add(i) } as i32;
        let v = (p + r).max(0).min(max_i32) as u16;
        unsafe {
            *dst.add(i) = v;
        }
    }
}

/// Scalar reference for [`crate::add_clip_to_pixel_u8`].
///
/// Per element: `dst[i] = (pred[i] as i32 + res[i] as i32).clamp(0, 255) as u8`.
/// The u8 sibling of [`add_clip_to_pixel_u16`] with a fixed clamp ceiling of
/// 255.  Accumulates in `i32` because `pred + res` can exceed `i16` range
/// (255 + 32767 = 33022).
pub(crate) unsafe fn add_clip_to_pixel_u8(
    pred: *const u8,
    res: *const i16,
    dst: *mut u8,
    n: usize,
) {
    for i in 0..n {
        let p = unsafe { *pred.add(i) } as i32;
        let r = unsafe { *res.add(i) } as i32;
        let v = (p + r).clamp(0, 255) as u8;
        unsafe {
            *dst.add(i) = v;
        }
    }
}

/// Scalar reference for [`crate::clamp_i32_to_i16`].
pub(crate) unsafe fn clamp_i32_to_i16(buf: *mut i32, n: usize) {
    const MIN: i32 = i16::MIN as i32;
    const MAX: i32 = i16::MAX as i32;
    for i in 0..n {
        let p = unsafe { buf.add(i) };
        let v = unsafe { *p };
        unsafe {
            *p = v.clamp(MIN, MAX);
        }
    }
}

/// Scalar reference for [`crate::mac_widen_i16_to_i32`].
pub(crate) unsafe fn mac_widen_i16_to_i32(a: *const i16, b: *const i16, acc: *mut i32, n: usize) {
    for i in 0..n {
        let av = unsafe { *a.add(i) } as i32;
        let bv = unsafe { *b.add(i) } as i32;
        let p = unsafe { acc.add(i) };
        unsafe {
            *p = (*p).wrapping_add(av.wrapping_mul(bv));
        }
    }
}

/// Scalar reference for [`crate::round_shift_narrow_i32_to_i16`].
///
/// Per lane: `dst[i] = saturating((src[i] + (1 << (shift-1))) >> shift) as i16`.
/// `shift` must be in `1..=31`.
pub(crate) unsafe fn round_shift_narrow_i32_to_i16(
    src: *const i32,
    dst: *mut i16,
    n: usize,
    shift: u32,
) {
    let round = 1i32 << (shift - 1);
    for i in 0..n {
        let v = unsafe { *src.add(i) };
        let r = v.wrapping_add(round) >> shift;
        let s = r.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        unsafe {
            *dst.add(i) = s;
        }
    }
}
