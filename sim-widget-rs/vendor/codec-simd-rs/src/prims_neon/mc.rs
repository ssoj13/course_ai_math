#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

/// NEON impl of [`crate::mc_avg_u8`].  `vrhaddq_u8` — single-instruction
/// rounding halving add.  16 bytes/iter.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_avg_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    dst: *mut u8,
    dst_stride: isize,
    w: usize,
    h: usize,
) {
    unsafe {
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                vst1q_u8(
                    d_row.add(col),
                    vrhaddq_u8(vld1q_u8(a_row.add(col)), vld1q_u8(b_row.add(col))),
                );
                col += 16;
            }
            if col + 8 <= w {
                vst1_u8(
                    d_row.add(col),
                    vrhadd_u8(vld1_u8(a_row.add(col)), vld1_u8(b_row.add(col))),
                );
                col += 8;
            }
            while col < w {
                let av = *a_row.add(col) as u16;
                let bv = *b_row.add(col) as u16;
                *d_row.add(col) = ((av + bv + 1) >> 1) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_8tap_h_u8`].  8 output columns per iteration.
///
/// Source bytes widen u8→i16; `vmlal_s16` accumulates the widening
/// product with the broadcast tap directly into i32x4 lanes (cols 0-3 /
/// 4-7).  `vqrshrun_n_s32::<7>` does round-shift-by-7 + clamp-to-≥0 in
/// one instruction; `vqmovn_u16` then clamps to ≤255.  Tail scalar.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_8tap_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
) {
    unsafe {
        let mut ft = [0i16; 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..8 {
                    let s8 = vld1_u8(s_row.add(col + i));
                    let s16 = vreinterpretq_s16_u16(vmovl_u8(s8));
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let r_lo = vqrshrun_n_s32::<7>(acc_lo);
                let r_hi = vqrshrun_n_s32::<7>(acc_hi);
                let r = vcombine_u16(r_lo, r_hi);
                vst1_u8(d_row.add(col), vqmovn_u16(r));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + 64) >> 7).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_8tap_v_u8`].  Vertical 8-tap, 8 output
/// columns per iteration.
///
/// Each tap loads a whole source row and `vmlal_s16` accumulates the
/// widening product with the broadcast tap into i32x4 lanes
/// (cols 0-3 / 4-7).  `vqrshrun_n_s32::<7>` + `vqmovn_u16` do the
/// round-shift and clamp.  Tail scalar.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_8tap_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
) {
    unsafe {
        let mut ft = [0i16; 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..8 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(s_ptr)));
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let r_lo = vqrshrun_n_s32::<7>(acc_lo);
                let r_hi = vqrshrun_n_s32::<7>(acc_hi);
                let r = vcombine_u16(r_lo, r_hi);
                vst1_u8(d_row.add(col), vqmovn_u16(r));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + 64) >> 7).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_4tap_h_u8`].  Horizontal 4-tap, 8 cols/iter,
/// runtime `shift`.  `vmlal_s16` widening MAC; round bias added manually
/// then `vshlq_s32` (arithmetic right by `shift`) + `vqmovun_s32`
/// (clamp ≥0) + `vqmovn_u16` (clamp ≤255).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_4tap_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let mut ft = [0i16; 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..4 {
                    let s16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(s_row.add(col + i))));
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let t_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let t_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                let u16x8 = vcombine_u16(vqmovun_s32(t_lo), vqmovun_s32(t_hi));
                vst1_u8(d_row.add(col), vqmovn_u16(u16x8));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_4tap_v_u8`].  Vertical 4-tap, 8 cols/iter,
/// runtime `shift`.  Each tap loads a whole source row.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_4tap_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let mut ft = [0i16; 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..4 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(s_ptr)));
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let t_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let t_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                let u16x8 = vcombine_u16(vqmovun_s32(t_lo), vqmovun_s32(t_hi));
                vst1_u8(d_row.add(col), vqmovn_u16(u16x8));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_6tap_h_u8`].  Horizontal 6-tap, 8 cols/iter,
/// runtime `shift`.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_6tap_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let mut ft = [0i16; 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..6 {
                    let s16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(s_row.add(col + i))));
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let t_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let t_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                let u16x8 = vcombine_u16(vqmovun_s32(t_lo), vqmovun_s32(t_hi));
                vst1_u8(d_row.add(col), vqmovn_u16(u16x8));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..6 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_6tap_v_u8`].  Vertical 6-tap, 8 cols/iter,
/// runtime `shift`.  Each tap loads a whole source row.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_6tap_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let mut ft = [0i16; 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..6 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(s_ptr)));
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let t_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let t_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                let u16x8 = vcombine_u16(vqmovun_s32(t_lo), vqmovun_s32(t_hi));
                vst1_u8(d_row.add(col), vqmovn_u16(u16x8));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..6 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_bilinear_h_u8`].  Horizontal 2-tap, 8
/// cols/iter, runtime `shift`.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_bilinear_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let ft = [*filter, *filter.add(1)];
        let f = [*filter as i32, *filter.add(1) as i32];
        let rscalar = 1i32 << (shift - 1);
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..2 {
                    let s16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(s_row.add(col + i))));
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let t_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let t_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                let u16x8 = vcombine_u16(vqmovun_s32(t_lo), vqmovun_s32(t_hi));
                vst1_u8(d_row.add(col), vqmovn_u16(u16x8));
                col += 8;
            }
            while col < w {
                let acc = *s_row.add(col) as i32 * f[0] + *s_row.add(col + 1) as i32 * f[1];
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_bilinear_v_u8`].  Vertical 2-tap, 8
/// cols/iter, runtime `shift`.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_bilinear_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let ft = [*filter, *filter.add(1)];
        let f = [*filter as i32, *filter.add(1) as i32];
        let rscalar = 1i32 << (shift - 1);
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..2 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(s_ptr)));
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let t_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let t_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                let u16x8 = vcombine_u16(vqmovun_s32(t_lo), vqmovun_s32(t_hi));
                vst1_u8(d_row.add(col), vqmovn_u16(u16x8));
                col += 8;
            }
            while col < w {
                let a = *src.offset(row as isize * src_stride).add(col) as i32;
                let b = *src.offset((row + 1) as isize * src_stride).add(col) as i32;
                *d_row.add(col) = ((a * f[0] + b * f[1] + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_8tap_h_u8_to_i16`].  i16-output horizontal
/// 8-tap, 8 cols/iter, runtime `shift`.  `vmlal_s16` MAC, bias +
/// `vshlq_s32`, then `vqmovn_s32` (saturating i32→i16) — no pixel clip.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_8tap_h_u8_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let mut ft = [0i16; 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..8 {
                    let s16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(s_row.add(col + i))));
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let r_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let r_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                let i16s = vcombine_s16(vqmovn_s32(r_lo), vqmovn_s32(r_hi));
                vst1q_s16(d_row.add(col), i16s);
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) =
                    ((acc + rscalar) >> shift).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_8tap_v_i16_to_u8`].  Vertical 8-tap over an
/// i16 intermediate, 8 cols/iter, runtime `shift`, clipped to u8.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_8tap_v_i16_to_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let mut ft = [0i16; 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..8 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = vld1q_s16(s_ptr);
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let t_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let t_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                let u16x8 = vcombine_u16(vqmovun_s32(t_lo), vqmovun_s32(t_hi));
                vst1_u8(d_row.add(col), vqmovn_u16(u16x8));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..8 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_4tap_h_u8_to_i16`].  i16-output horizontal
/// 4-tap, 8 cols/iter, runtime `shift`.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_4tap_h_u8_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let mut ft = [0i16; 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..4 {
                    let s16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(s_row.add(col + i))));
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let r_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let r_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                vst1q_s16(
                    d_row.add(col),
                    vcombine_s16(vqmovn_s32(r_lo), vqmovn_s32(r_hi)),
                );
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) =
                    ((acc + rscalar) >> shift).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_4tap_v_i16_to_u8`].  i16-input vertical
/// 4-tap, 8 cols/iter, runtime `shift`, clipped to u8.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_4tap_v_i16_to_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let mut ft = [0i16; 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..4 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = vld1q_s16(s_ptr);
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let t_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let t_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                let u16x8 = vcombine_u16(vqmovun_s32(t_lo), vqmovun_s32(t_hi));
                vst1_u8(d_row.add(col), vqmovn_u16(u16x8));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..4 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_6tap_h_u8_to_i16`].  8 cols/iter, runtime shift.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_6tap_h_u8_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let mut ft = [0i16; 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..6 {
                    let s16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(s_row.add(col + i))));
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let r_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let r_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                vst1q_s16(
                    d_row.add(col),
                    vcombine_s16(vqmovn_s32(r_lo), vqmovn_s32(r_hi)),
                );
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..6 {
                    acc += *s_row.add(col + i) as i32 * f[i];
                }
                *d_row.add(col) =
                    ((acc + rscalar) >> shift).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_6tap_v_i16_to_u8`].  8 cols/iter, runtime shift.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_6tap_v_i16_to_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    unsafe {
        let mut ft = [0i16; 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = *filter.add(i);
            f[i] = ft[i] as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = vdupq_n_s32(0);
                let mut acc_hi = vdupq_n_s32(0);
                for i in 0..6 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = vld1q_s16(s_ptr);
                    let tap = vdup_n_s16(ft[i]);
                    acc_lo = vmlal_s16(acc_lo, vget_low_s16(s16), tap);
                    acc_hi = vmlal_s16(acc_hi, vget_high_s16(s16), tap);
                }
                let t_lo = vshlq_s32(vaddq_s32(acc_lo, bias), neg);
                let t_hi = vshlq_s32(vaddq_s32(acc_hi, bias), neg);
                let u16x8 = vcombine_u16(vqmovun_s32(t_lo), vqmovun_s32(t_hi));
                vst1_u8(d_row.add(col), vqmovn_u16(u16x8));
                col += 8;
            }
            while col < w {
                let mut acc: i32 = 0;
                for i in 0..6 {
                    acc += *src.offset((row + i) as isize * src_stride).add(col) as i32 * f[i];
                }
                *d_row.add(col) = ((acc + rscalar) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::weighted_avg_u8`].  8 cols/iter; `vmull_n_s16`
/// per weight, sum + offset, round-shift, clip to u8.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn weighted_avg_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    dst: *mut u8,
    dst_stride: isize,
    w: usize,
    h: usize,
    w0: i16,
    w1: i16,
    offset: i32,
    shift: u32,
) {
    unsafe {
        let offv = vdupq_n_s32(offset);
        let neg = vdupq_n_s32(-(shift as i32));
        let (w0s, w1s) = (w0 as i32, w1 as i32);
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let a16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(a_row.add(col))));
                let b16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(b_row.add(col))));
                let lo = vaddq_s32(
                    vaddq_s32(
                        vmull_n_s16(vget_low_s16(a16), w0),
                        vmull_n_s16(vget_low_s16(b16), w1),
                    ),
                    offv,
                );
                let hi = vaddq_s32(
                    vaddq_s32(
                        vmull_n_s16(vget_high_s16(a16), w0),
                        vmull_n_s16(vget_high_s16(b16), w1),
                    ),
                    offv,
                );
                let rlo = vshlq_s32(lo, neg);
                let rhi = vshlq_s32(hi, neg);
                let u16x8 = vcombine_u16(vqmovun_s32(rlo), vqmovun_s32(rhi));
                vst1_u8(d_row.add(col), vqmovn_u16(u16x8));
                col += 8;
            }
            while col < w {
                let av = *a_row.add(col) as i32;
                let bv = *b_row.add(col) as i32;
                *d_row.add(col) = ((w0s * av + w1s * bv + offset) >> shift).clamp(0, 255) as u8;
                col += 1;
            }
        }
    }
}

/// NEON impl of [`crate::mc_avg_u16`].  `vrhaddq_u16` — single-instruction
/// rounding halving add.  8 u16/iter.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mc_avg_u16(
    a: *const u16,
    a_stride: isize,
    b: *const u16,
    b_stride: isize,
    dst: *mut u16,
    dst_stride: isize,
    w: usize,
    h: usize,
) {
    unsafe {
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let av = vld1q_u16(a_row.add(col));
                let bv = vld1q_u16(b_row.add(col));
                vst1q_u16(d_row.add(col), vrhaddq_u16(av, bv));
                col += 8;
            }
            while col < w {
                let av = *a_row.add(col) as u32;
                let bv = *b_row.add(col) as u32;
                *d_row.add(col) = ((av + bv + 1) >> 1) as u16;
                col += 1;
            }
        }
    }
}
