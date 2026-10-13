#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

/// NEON impl of [`crate::lincomb3_clip_u8`].  8/iter; `vmull_n_s16` per
/// coefficient, sum + offset, round-shift, clip to u8.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn lincomb3_clip_u8(
    p0: *const u8,
    p1: *const u8,
    p2: *const u8,
    dst: *mut u8,
    n: usize,
    c0: i16,
    c1: i16,
    c2: i16,
    offset: i32,
    shift: u32,
) {
    unsafe {
        let offv = vdupq_n_s32(offset);
        let neg = vdupq_n_s32(-(shift as i32));
        let (c0s, c1s, c2s) = (c0 as i32, c1 as i32, c2 as i32);
        let mut i = 0usize;
        while i + 8 <= n {
            let a16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(p0.add(i))));
            let b16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(p1.add(i))));
            let c16 = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(p2.add(i))));
            let lo = vaddq_s32(
                vaddq_s32(
                    vaddq_s32(
                        vmull_n_s16(vget_low_s16(a16), c0),
                        vmull_n_s16(vget_low_s16(b16), c1),
                    ),
                    vmull_n_s16(vget_low_s16(c16), c2),
                ),
                offv,
            );
            let hi = vaddq_s32(
                vaddq_s32(
                    vaddq_s32(
                        vmull_n_s16(vget_high_s16(a16), c0),
                        vmull_n_s16(vget_high_s16(b16), c1),
                    ),
                    vmull_n_s16(vget_high_s16(c16), c2),
                ),
                offv,
            );
            let rlo = vshlq_s32(lo, neg);
            let rhi = vshlq_s32(hi, neg);
            let u16x8 = vcombine_u16(vqmovun_s32(rlo), vqmovun_s32(rhi));
            vst1_u8(dst.add(i), vqmovn_u16(u16x8));
            i += 8;
        }
        while i < n {
            let acc = c0s * (*p0.add(i) as i32)
                + c1s * (*p1.add(i) as i32)
                + c2s * (*p2.add(i) as i32)
                + offset;
            *dst.add(i) = (acc >> shift).clamp(0, 255) as u8;
            i += 1;
        }
    }
}

/// NEON impl of [`crate::rct_fwd_i32`].  4 i32/iter.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn rct_fwd_i32(
    r: *const i32,
    g: *const i32,
    b: *const i32,
    y: *mut i32,
    cb: *mut i32,
    cr: *mut i32,
    n: usize,
) {
    unsafe {
        let mut i = 0usize;
        while i + 4 <= n {
            let rr = vld1q_s32(r.add(i));
            let gg = vld1q_s32(g.add(i));
            let bb = vld1q_s32(b.add(i));
            let yv = vshrq_n_s32::<2>(vaddq_s32(vaddq_s32(rr, vshlq_n_s32::<1>(gg)), bb));
            vst1q_s32(y.add(i), yv);
            vst1q_s32(cb.add(i), vsubq_s32(bb, gg));
            vst1q_s32(cr.add(i), vsubq_s32(rr, gg));
            i += 4;
        }
        while i < n {
            let (rr, gg, bb) = (*r.add(i), *g.add(i), *b.add(i));
            *y.add(i) = (rr + (gg << 1) + bb) >> 2;
            *cb.add(i) = bb - gg;
            *cr.add(i) = rr - gg;
            i += 1;
        }
    }
}

/// NEON impl of [`crate::rct_bwd_i32`].  4 i32/iter.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn rct_bwd_i32(
    y: *const i32,
    cb: *const i32,
    cr: *const i32,
    r: *mut i32,
    g: *mut i32,
    b: *mut i32,
    n: usize,
) {
    unsafe {
        let mut i = 0usize;
        while i + 4 <= n {
            let yv = vld1q_s32(y.add(i));
            let cbv = vld1q_s32(cb.add(i));
            let crv = vld1q_s32(cr.add(i));
            let gg = vsubq_s32(yv, vshrq_n_s32::<2>(vaddq_s32(cbv, crv)));
            vst1q_s32(g.add(i), gg);
            vst1q_s32(r.add(i), vaddq_s32(crv, gg));
            vst1q_s32(b.add(i), vaddq_s32(cbv, gg));
            i += 4;
        }
        while i < n {
            let (yy, cbb, crr) = (*y.add(i), *cb.add(i), *cr.add(i));
            let gv = yy - ((cbb + crr) >> 2);
            *g.add(i) = gv;
            *r.add(i) = crr + gv;
            *b.add(i) = cbb + gv;
            i += 1;
        }
    }
}
