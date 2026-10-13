#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::*;

/// wasm impl of [`crate::sat_pack_i16_to_u8`].
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn sat_pack_i16_to_u8(src: *const i16, dst: *mut u8, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 16 <= n {
            let lo = v128_load(src.add(i) as *const v128);
            let hi = v128_load(src.add(i + 8) as *const v128);
            v128_store(dst.add(i) as *mut v128, u8x16_narrow_i16x8(lo, hi));
            i += 16;
        }
        while i < n {
            *dst.add(i) = (*src.add(i)).clamp(0, 255) as u8;
            i += 1;
        }
    }
}

/// wasm impl of [`crate::sat_pack_i32_to_u16`].
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn sat_pack_i32_to_u16(src: *const i32, dst: *mut u16, n: usize, max: u16) {
    unsafe {
        let zero = i32x4_splat(0);
        let maxv = i32x4_splat(max as i32);
        let mut i = 0usize;
        while i + 8 <= n {
            let lo = i32x4_min(i32x4_max(v128_load(src.add(i) as *const v128), zero), maxv);
            let hi = i32x4_min(
                i32x4_max(v128_load(src.add(i + 4) as *const v128), zero),
                maxv,
            );
            v128_store(dst.add(i) as *mut v128, u16x8_narrow_i32x4(lo, hi));
            i += 8;
        }
        let max_i32 = max as i32;
        while i < n {
            *dst.add(i) = (*src.add(i)).max(0).min(max_i32) as u16;
            i += 1;
        }
    }
}

/// wasm impl of [`crate::add_clip_to_pixel_u16`].
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn add_clip_to_pixel_u16(
    pred: *const u16,
    res: *const i16,
    dst: *mut u16,
    n: usize,
    max: u16,
) {
    unsafe {
        let zero = i32x4_splat(0);
        let maxv = i32x4_splat(max as i32);
        let mut i = 0usize;
        while i + 8 <= n {
            let p = v128_load(pred.add(i) as *const v128);
            let r = v128_load(res.add(i) as *const v128);
            let s_lo = i32x4_min(
                i32x4_max(
                    i32x4_add(i32x4_extend_low_u16x8(p), i32x4_extend_low_i16x8(r)),
                    zero,
                ),
                maxv,
            );
            let s_hi = i32x4_min(
                i32x4_max(
                    i32x4_add(i32x4_extend_high_u16x8(p), i32x4_extend_high_i16x8(r)),
                    zero,
                ),
                maxv,
            );
            v128_store(dst.add(i) as *mut v128, u16x8_narrow_i32x4(s_lo, s_hi));
            i += 8;
        }
        let max_i32 = max as i32;
        while i < n {
            let p = *pred.add(i) as i32;
            let r = *res.add(i) as i32;
            *dst.add(i) = (p + r).max(0).min(max_i32) as u16;
            i += 1;
        }
    }
}

/// wasm impl of [`crate::add_clip_to_pixel_u8`].  16 lanes/iter.
///
/// Widens 16 `u8` pred (`u16x8_extend_*_u8x16` → `i32x4_extend_*_u16x8`) and
/// 16 `i16` res (`i32x4_extend_*_i16x8`) to four `i32x4` vectors each, adds,
/// clamps to `[0, 255]` in i32 (`i32x4_max`/`i32x4_min`), then narrows
/// i32 → u16 (`u16x8_narrow_i32x4`) → u8 (`u8x16_narrow_i16x8`).
/// Accumulating in i32 is mandatory: `255 + i16::MAX` overflows i16.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn add_clip_to_pixel_u8(
    pred: *const u8,
    res: *const i16,
    dst: *mut u8,
    n: usize,
) {
    unsafe {
        let zero = i32x4_splat(0);
        let maxv = i32x4_splat(255);
        let mut i = 0usize;
        while i + 16 <= n {
            let p = v128_load(pred.add(i) as *const v128); // 16 u8
            let r0 = v128_load(res.add(i) as *const v128); // 8 i16
            let r1 = v128_load(res.add(i + 8) as *const v128); // 8 i16

            let p16lo = u16x8_extend_low_u8x16(p);
            let p16hi = u16x8_extend_high_u8x16(p);
            let p0 = i32x4_extend_low_u16x8(p16lo);
            let p1 = i32x4_extend_high_u16x8(p16lo);
            let p2 = i32x4_extend_low_u16x8(p16hi);
            let p3 = i32x4_extend_high_u16x8(p16hi);

            let r_0 = i32x4_extend_low_i16x8(r0);
            let r_1 = i32x4_extend_high_i16x8(r0);
            let r_2 = i32x4_extend_low_i16x8(r1);
            let r_3 = i32x4_extend_high_i16x8(r1);

            let s0 = i32x4_min(i32x4_max(i32x4_add(p0, r_0), zero), maxv);
            let s1 = i32x4_min(i32x4_max(i32x4_add(p1, r_1), zero), maxv);
            let s2 = i32x4_min(i32x4_max(i32x4_add(p2, r_2), zero), maxv);
            let s3 = i32x4_min(i32x4_max(i32x4_add(p3, r_3), zero), maxv);

            let u16lo = u16x8_narrow_i32x4(s0, s1);
            let u16hi = u16x8_narrow_i32x4(s2, s3);
            let out = u8x16_narrow_i16x8(u16lo, u16hi);
            v128_store(dst.add(i) as *mut v128, out);
            i += 16;
        }
        while i < n {
            let p = *pred.add(i) as i32;
            let r = *res.add(i) as i32;
            *dst.add(i) = (p + r).clamp(0, 255) as u8;
            i += 1;
        }
    }
}

/// wasm impl of [`crate::clamp_i32_to_i16`].
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn clamp_i32_to_i16(buf: *mut i32, n: usize) {
    unsafe {
        const MIN: i32 = i16::MIN as i32;
        const MAX: i32 = i16::MAX as i32;
        let minv = i32x4_splat(MIN);
        let maxv = i32x4_splat(MAX);
        let mut i = 0usize;
        while i + 4 <= n {
            let v = v128_load(buf.add(i) as *const v128);
            v128_store(buf.add(i) as *mut v128, i32x4_min(i32x4_max(v, minv), maxv));
            i += 4;
        }
        while i < n {
            let p = buf.add(i);
            *p = (*p).max(MIN).min(MAX);
            i += 1;
        }
    }
}

/// wasm impl of [`crate::mac_widen_i16_to_i32`].
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn mac_widen_i16_to_i32(a: *const i16, b: *const i16, acc: *mut i32, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            let av = v128_load(a.add(i) as *const v128);
            let bv = v128_load(b.add(i) as *const v128);
            let m_lo = i32x4_mul(i32x4_extend_low_i16x8(av), i32x4_extend_low_i16x8(bv));
            let m_hi = i32x4_mul(i32x4_extend_high_i16x8(av), i32x4_extend_high_i16x8(bv));
            v128_store(
                acc.add(i) as *mut v128,
                i32x4_add(v128_load(acc.add(i) as *const v128), m_lo),
            );
            v128_store(
                acc.add(i + 4) as *mut v128,
                i32x4_add(v128_load(acc.add(i + 4) as *const v128), m_hi),
            );
            i += 8;
        }
        while i < n {
            let av = *a.add(i) as i32;
            let bv = *b.add(i) as i32;
            let p = acc.add(i);
            *p = (*p).wrapping_add(av.wrapping_mul(bv));
            i += 1;
        }
    }
}
