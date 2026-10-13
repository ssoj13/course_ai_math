#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

/// NEON impl of [`crate::sat_pack_i16_to_u8`].
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn sat_pack_i16_to_u8(src: *const i16, dst: *mut u8, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 16 <= n {
            let lo = vld1q_s16(src.add(i));
            let hi = vld1q_s16(src.add(i + 8));
            let plo = vqmovun_s16(lo);
            let phi = vqmovun_s16(hi);
            vst1q_u8(dst.add(i), vcombine_u8(plo, phi));
            i += 16;
        }
        while i < n {
            let v = *src.add(i);
            *dst.add(i) = v.clamp(0, 255) as u8;
            i += 1;
        }
    }
}

/// NEON impl of [`crate::sat_pack_i32_to_u16`].
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn sat_pack_i32_to_u16(src: *const i32, dst: *mut u16, n: usize, max: u16) {
    unsafe {
        let zero = vdupq_n_s32(0);
        let maxv = vdupq_n_s32(max as i32);
        let mut i = 0usize;
        while i + 8 <= n {
            let lo = vld1q_s32(src.add(i));
            let lo = vmaxq_s32(lo, zero);
            let lo = vminq_s32(lo, maxv);
            let hi = vld1q_s32(src.add(i + 4));
            let hi = vmaxq_s32(hi, zero);
            let hi = vminq_s32(hi, maxv);
            vst1q_u16(dst.add(i), vcombine_u16(vqmovun_s32(lo), vqmovun_s32(hi)));
            i += 8;
        }
        let max_i32 = max as i32;
        while i < n {
            *dst.add(i) = (*src.add(i)).max(0).min(max_i32) as u16;
            i += 1;
        }
    }
}

/// NEON impl of [`crate::add_clip_to_pixel_u16`].
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn add_clip_to_pixel_u16(
    pred: *const u16,
    res: *const i16,
    dst: *mut u16,
    n: usize,
    max: u16,
) {
    unsafe {
        let zero = vdupq_n_s32(0);
        let maxv = vdupq_n_s32(max as i32);
        let mut i = 0usize;
        while i + 8 <= n {
            let p = vld1q_u16(pred.add(i));
            let r = vld1q_s16(res.add(i));
            let p_lo = vreinterpretq_s32_u32(vmovl_u16(vget_low_u16(p)));
            let p_hi = vreinterpretq_s32_u32(vmovl_high_u16(p));
            let r_lo = vmovl_s16(vget_low_s16(r));
            let r_hi = vmovl_high_s16(r);
            let s_lo = vminq_s32(vmaxq_s32(vaddq_s32(p_lo, r_lo), zero), maxv);
            let s_hi = vminq_s32(vmaxq_s32(vaddq_s32(p_hi, r_hi), zero), maxv);
            vst1q_u16(
                dst.add(i),
                vcombine_u16(vqmovun_s32(s_lo), vqmovun_s32(s_hi)),
            );
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

/// NEON impl of [`crate::add_clip_to_pixel_u8`].  8 lanes/iter.
///
/// Widens `u8` pred (`vmovl_u8` → u16, then `vmovl_u16` → i32) and `i16` res
/// (`vmovl_s16` → i32), adds, clamps to `[0, 255]` in i32
/// (`vmaxq_s32`/`vminq_s32`), then narrows i32 → u16 (`vqmovun_s32`) → u8
/// (`vqmovn_u16`).  The explicit clamp makes it bit-exact with scalar even
/// though the saturating narrows would also bound the value.
/// Accumulating in i32 is mandatory: `255 + i16::MAX` overflows i16.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn add_clip_to_pixel_u8(
    pred: *const u8,
    res: *const i16,
    dst: *mut u8,
    n: usize,
) {
    unsafe {
        let zero = vdupq_n_s32(0);
        let maxv = vdupq_n_s32(255);
        let mut i = 0usize;
        while i + 8 <= n {
            let pw = vmovl_u8(vld1_u8(pred.add(i))); // 8 u8 -> 8 u16
            let p_lo = vreinterpretq_s32_u32(vmovl_u16(vget_low_u16(pw)));
            let p_hi = vreinterpretq_s32_u32(vmovl_high_u16(pw));
            let r = vld1q_s16(res.add(i));
            let r_lo = vmovl_s16(vget_low_s16(r));
            let r_hi = vmovl_high_s16(r);
            let s_lo = vminq_s32(vmaxq_s32(vaddq_s32(p_lo, r_lo), zero), maxv);
            let s_hi = vminq_s32(vmaxq_s32(vaddq_s32(p_hi, r_hi), zero), maxv);
            let u16v = vcombine_u16(vqmovun_s32(s_lo), vqmovun_s32(s_hi));
            vst1_u8(dst.add(i), vqmovn_u16(u16v));
            i += 8;
        }
        while i < n {
            let p = *pred.add(i) as i32;
            let r = *res.add(i) as i32;
            *dst.add(i) = (p + r).clamp(0, 255) as u8;
            i += 1;
        }
    }
}

/// NEON impl of [`crate::clamp_i32_to_i16`].
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn clamp_i32_to_i16(buf: *mut i32, n: usize) {
    unsafe {
        const MIN: i32 = i16::MIN as i32;
        const MAX: i32 = i16::MAX as i32;
        let minv = vdupq_n_s32(MIN);
        let maxv = vdupq_n_s32(MAX);
        let mut i = 0usize;
        while i + 4 <= n {
            let v = vld1q_s32(buf.add(i));
            vst1q_s32(buf.add(i), vminq_s32(vmaxq_s32(v, minv), maxv));
            i += 4;
        }
        while i < n {
            let p = buf.add(i);
            *p = (*p).max(MIN).min(MAX);
            i += 1;
        }
    }
}

/// NEON impl of [`crate::mac_widen_i16_to_i32`].
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mac_widen_i16_to_i32(a: *const i16, b: *const i16, acc: *mut i32, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            let av = vld1q_s16(a.add(i));
            let bv = vld1q_s16(b.add(i));
            let acc_lo = vld1q_s32(acc.add(i));
            let acc_hi = vld1q_s32(acc.add(i + 4));
            let s_lo = vmlal_s16(acc_lo, vget_low_s16(av), vget_low_s16(bv));
            let s_hi = vmlal_high_s16(acc_hi, av, bv);
            vst1q_s32(acc.add(i), s_lo);
            vst1q_s32(acc.add(i + 4), s_hi);
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

/// NEON impl of [`crate::round_shift_narrow_i32_to_i16`].
///
/// `vrshlq_s32` with negative shift count = arithmetic right shift with
/// rounding (round towards +inf based on the dropped bit).  Then
/// `vqmovn_s32` narrows i32 → i16 with saturation.  8 lanes/iter.
///
/// `vrshl` rounds via "round halfway away from zero" semantics — same as
/// the scalar reference's `(v + (1 << (shift-1))) >> shift` for positive
/// values and bit-identical for non-overflowing negative shifts (subtle
/// but holds for the codec use case where shift is small ≤ 16).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn round_shift_narrow_i32_to_i16(
    src: *const i32,
    dst: *mut i16,
    n: usize,
    shift: u32,
) {
    unsafe {
        // Use explicit add-round + arithmetic shift right + saturating
        // narrow to exactly match scalar semantics.  vrshl variants vary
        // in their rounding mode across i32 ranges, so avoid them and
        // compute the conventional path explicitly.
        let round = vdupq_n_s32(1i32 << (shift - 1));
        let neg_shift = vdupq_n_s32(-(shift as i32));
        let mut i = 0usize;
        while i + 8 <= n {
            let lo = vld1q_s32(src.add(i));
            let lo = vshlq_s32(vaddq_s32(lo, round), neg_shift);
            let hi = vld1q_s32(src.add(i + 4));
            let hi = vshlq_s32(vaddq_s32(hi, round), neg_shift);
            let combined = vcombine_s16(vqmovn_s32(lo), vqmovn_s32(hi));
            vst1q_s16(dst.add(i), combined);
            i += 8;
        }
        let round_i = 1i32 << (shift - 1);
        while i < n {
            let v = *src.add(i);
            let r = v.wrapping_add(round_i) >> shift;
            *dst.add(i) = r.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}
