#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

/// NEON impl of [`crate::abs_diff_u8`].
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn abs_diff_u8(a: *const u8, b: *const u8, dst: *mut u8, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 16 <= n {
            vst1q_u8(dst.add(i), vabdq_u8(vld1q_u8(a.add(i)), vld1q_u8(b.add(i))));
            i += 16;
        }
        while i < n {
            let av = *a.add(i);
            let bv = *b.add(i);
            *dst.add(i) = if av > bv { av - bv } else { bv - av };
            i += 1;
        }
    }
}

/// NEON impl of [`crate::transpose_4x4_i16`].
///
/// `vtrn1_s16` / `vtrn2_s16` + `vtrn1_s32` / `vtrn2_s32` do 4×4 transpose
/// in two stages.  Each row is `int16x4_t` (64-bit).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn transpose_4x4_i16(src: *const i16, dst: *mut i16) {
    unsafe {
        let r0 = vld1_s16(src.add(0));
        let r1 = vld1_s16(src.add(4));
        let r2 = vld1_s16(src.add(8));
        let r3 = vld1_s16(src.add(12));

        // Stage 1: interleave 16-bit lanes.
        let a0 = vtrn1_s16(r0, r1);
        let a1 = vtrn2_s16(r0, r1);
        let a2 = vtrn1_s16(r2, r3);
        let a3 = vtrn2_s16(r2, r3);

        // Stage 2: interleave 32-bit lanes via reinterpret.
        let b0 = vreinterpret_s16_s32(vtrn1_s32(
            vreinterpret_s32_s16(a0),
            vreinterpret_s32_s16(a2),
        ));
        let b1 = vreinterpret_s16_s32(vtrn1_s32(
            vreinterpret_s32_s16(a1),
            vreinterpret_s32_s16(a3),
        ));
        let b2 = vreinterpret_s16_s32(vtrn2_s32(
            vreinterpret_s32_s16(a0),
            vreinterpret_s32_s16(a2),
        ));
        let b3 = vreinterpret_s16_s32(vtrn2_s32(
            vreinterpret_s32_s16(a1),
            vreinterpret_s32_s16(a3),
        ));

        vst1_s16(dst.add(0), b0);
        vst1_s16(dst.add(4), b1);
        vst1_s16(dst.add(8), b2);
        vst1_s16(dst.add(12), b3);
    }
}

/// NEON impl of [`crate::transpose_8x8_i16`].
///
/// Classic 8×8 transpose via 3 stages of trn1/trn2 at widths 16/32/64.
/// Each row is `int16x8_t` (128-bit); 8 loads, 8 stores, 24 trn ops.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn transpose_8x8_i16(src: *const i16, dst: *mut i16) {
    unsafe {
        let r0 = vld1q_s16(src.add(0));
        let r1 = vld1q_s16(src.add(8));
        let r2 = vld1q_s16(src.add(16));
        let r3 = vld1q_s16(src.add(24));
        let r4 = vld1q_s16(src.add(32));
        let r5 = vld1q_s16(src.add(40));
        let r6 = vld1q_s16(src.add(48));
        let r7 = vld1q_s16(src.add(56));

        // Stage 1: 16-bit interleave.
        let a0 = vtrn1q_s16(r0, r1);
        let a1 = vtrn2q_s16(r0, r1);
        let a2 = vtrn1q_s16(r2, r3);
        let a3 = vtrn2q_s16(r2, r3);
        let a4 = vtrn1q_s16(r4, r5);
        let a5 = vtrn2q_s16(r4, r5);
        let a6 = vtrn1q_s16(r6, r7);
        let a7 = vtrn2q_s16(r6, r7);

        // Stage 2: 32-bit interleave.
        let b0 = vreinterpretq_s16_s32(vtrn1q_s32(
            vreinterpretq_s32_s16(a0),
            vreinterpretq_s32_s16(a2),
        ));
        let b1 = vreinterpretq_s16_s32(vtrn1q_s32(
            vreinterpretq_s32_s16(a1),
            vreinterpretq_s32_s16(a3),
        ));
        let b2 = vreinterpretq_s16_s32(vtrn2q_s32(
            vreinterpretq_s32_s16(a0),
            vreinterpretq_s32_s16(a2),
        ));
        let b3 = vreinterpretq_s16_s32(vtrn2q_s32(
            vreinterpretq_s32_s16(a1),
            vreinterpretq_s32_s16(a3),
        ));
        let b4 = vreinterpretq_s16_s32(vtrn1q_s32(
            vreinterpretq_s32_s16(a4),
            vreinterpretq_s32_s16(a6),
        ));
        let b5 = vreinterpretq_s16_s32(vtrn1q_s32(
            vreinterpretq_s32_s16(a5),
            vreinterpretq_s32_s16(a7),
        ));
        let b6 = vreinterpretq_s16_s32(vtrn2q_s32(
            vreinterpretq_s32_s16(a4),
            vreinterpretq_s32_s16(a6),
        ));
        let b7 = vreinterpretq_s16_s32(vtrn2q_s32(
            vreinterpretq_s32_s16(a5),
            vreinterpretq_s32_s16(a7),
        ));

        // Stage 3: 64-bit interleave.
        let t0 = vreinterpretq_s16_s64(vtrn1q_s64(
            vreinterpretq_s64_s16(b0),
            vreinterpretq_s64_s16(b4),
        ));
        let t1 = vreinterpretq_s16_s64(vtrn1q_s64(
            vreinterpretq_s64_s16(b1),
            vreinterpretq_s64_s16(b5),
        ));
        let t2 = vreinterpretq_s16_s64(vtrn1q_s64(
            vreinterpretq_s64_s16(b2),
            vreinterpretq_s64_s16(b6),
        ));
        let t3 = vreinterpretq_s16_s64(vtrn1q_s64(
            vreinterpretq_s64_s16(b3),
            vreinterpretq_s64_s16(b7),
        ));
        let t4 = vreinterpretq_s16_s64(vtrn2q_s64(
            vreinterpretq_s64_s16(b0),
            vreinterpretq_s64_s16(b4),
        ));
        let t5 = vreinterpretq_s16_s64(vtrn2q_s64(
            vreinterpretq_s64_s16(b1),
            vreinterpretq_s64_s16(b5),
        ));
        let t6 = vreinterpretq_s16_s64(vtrn2q_s64(
            vreinterpretq_s64_s16(b2),
            vreinterpretq_s64_s16(b6),
        ));
        let t7 = vreinterpretq_s16_s64(vtrn2q_s64(
            vreinterpretq_s64_s16(b3),
            vreinterpretq_s64_s16(b7),
        ));

        vst1q_s16(dst.add(0), t0);
        vst1q_s16(dst.add(8), t1);
        vst1q_s16(dst.add(16), t2);
        vst1q_s16(dst.add(24), t3);
        vst1q_s16(dst.add(32), t4);
        vst1q_s16(dst.add(40), t5);
        vst1q_s16(dst.add(48), t6);
        vst1q_s16(dst.add(56), t7);
    }
}

/// NEON impl of [`crate::sat_add_i16`].  8 lanes/iter via `vqaddq_s16`.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn sat_add_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            vst1q_s16(
                dst.add(i),
                vqaddq_s16(vld1q_s16(a.add(i)), vld1q_s16(b.add(i))),
            );
            i += 8;
        }
        while i < n {
            let av = *a.add(i) as i32;
            let bv = *b.add(i) as i32;
            *dst.add(i) = (av + bv).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}

/// NEON impl of [`crate::sat_sub_i16`].  8 lanes/iter via `vqsubq_s16`.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn sat_sub_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            vst1q_s16(
                dst.add(i),
                vqsubq_s16(vld1q_s16(a.add(i)), vld1q_s16(b.add(i))),
            );
            i += 8;
        }
        while i < n {
            let av = *a.add(i) as i32;
            let bv = *b.add(i) as i32;
            *dst.add(i) = (av - bv).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}

/// NEON impl of [`crate::abs_i16`].
///
/// `vqabsq_s16` — saturating abs.  `vqabsq_s16(-0x8000) = 0x7FFF`, matching
/// the scalar reference.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn abs_i16(src: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            vst1q_s16(dst.add(i), vqabsq_s16(vld1q_s16(src.add(i))));
            i += 8;
        }
        while i < n {
            let v = *src.add(i);
            *dst.add(i) = if v == i16::MIN { i16::MAX } else { v.abs() };
            i += 1;
        }
    }
}

/// NEON impl of [`crate::mul_hi_round_i16`].
///
/// `vqrdmulhq_s16` — saturating rounding doubling multiply high.
/// Computes `((a * b * 2 + (1 << 15)) >> 16)` lane-wise, with saturation.
/// Equivalent to `((a * b) + (1 << 14)) >> 15` to within rounding.
/// Saturates `i16::MIN × i16::MIN` to `i16::MAX`.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn mul_hi_round_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            vst1q_s16(
                dst.add(i),
                vqrdmulhq_s16(vld1q_s16(a.add(i)), vld1q_s16(b.add(i))),
            );
            i += 8;
        }
        while i < n {
            let av = *a.add(i) as i32;
            let bv = *b.add(i) as i32;
            let p = av.wrapping_mul(bv).wrapping_add(1 << 14) >> 15;
            *dst.add(i) = p.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}

/// NEON impl of [`crate::dequant_block_i16`].  8 i16/iter; `vmull_n_s16`
/// widening multiply by the scalar, round-shift, `vqmovn_s32`.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn dequant_block_i16(buf: *mut i16, n: usize, scale: i16, shift: u32) {
    unsafe {
        let sc = scale as i32;
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = vdupq_n_s32(rscalar);
        let neg = vdupq_n_s32(-(shift as i32));
        let mut i = 0usize;
        while i + 8 <= n {
            let v = vld1q_s16(buf.add(i));
            let lo = vmull_n_s16(vget_low_s16(v), scale);
            let hi = vmull_n_s16(vget_high_s16(v), scale);
            let rlo = vshlq_s32(vaddq_s32(lo, bias), neg);
            let rhi = vshlq_s32(vaddq_s32(hi, bias), neg);
            vst1q_s16(buf.add(i), vcombine_s16(vqmovn_s32(rlo), vqmovn_s32(rhi)));
            i += 8;
        }
        while i < n {
            let p = buf.add(i);
            let v = ((*p as i32) * sc + rscalar) >> shift;
            *p = v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            i += 1;
        }
    }
}

/// NEON impl of [`crate::prefix_sum_bias_u8_inplace`] — port of OpenEXR's
/// own NEON `reconstruct` (`internal_zip.c:95-143`).
///
/// Same shift-and-add prefix-sum tree as the SSE4.1 backend
/// (`prims_sse41::prefix_sum_bias_u8_inplace`), using `vextq_u8(zero, d, 16-k)`
/// in place of `_mm_slli_si128(d, k)` (both are "shift the 128-bit register
/// right by `k` bytes, zero-filling from the left" in element order) and
/// `vqtbl1q_u8` in place of `pshufb` to broadcast the previous block's last
/// lane. See that function's doc for the `buf[0]` pre-adjustment reasoning
/// (identical here).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn prefix_sum_bias_u8_inplace(buf: *mut u8, n: usize) {
    unsafe {
        const CHUNK: usize = 16;
        let v_n = n / CHUNK;

        if n > 0 {
            *buf = (*buf).wrapping_sub(128);
        }

        let c = vdupq_n_u8(0x80); // -128 as u8 bit pattern
        let shuffle_mask = vdupq_n_u8(15);
        let zero = vdupq_n_u8(0);
        let mut v_prev = vdupq_n_u8(0);

        let mut i = 0usize;
        while i < v_n {
            let p = buf.add(i * CHUNK);
            let mut d = vaddq_u8(vld1q_u8(p), c);

            d = vaddq_u8(d, vextq_u8(zero, d, 16 - 1));
            d = vaddq_u8(d, vextq_u8(zero, d, 16 - 2));
            d = vaddq_u8(d, vextq_u8(zero, d, 16 - 4));
            d = vaddq_u8(d, vextq_u8(zero, d, 16 - 8));
            d = vaddq_u8(d, v_prev);

            vst1q_u8(p, d);
            v_prev = vqtbl1q_u8(d, shuffle_mask);
            i += 1;
        }

        let prev0 = vgetq_lane_u8(v_prev, 15);
        let mut prev = prev0;
        let mut j = v_n * CHUNK;
        while j < n {
            let p = buf.add(j);
            let d = prev.wrapping_add(*p).wrapping_sub(128);
            *p = d;
            prev = d;
            j += 1;
        }
    }
}

/// NEON impl of [`crate::interleave_halves_u8`] — port of OpenEXR's own
/// NEON `interleave` (`internal_zip.c:192-219`).
///
/// 32 output bytes/iteration via `vzip1q_u8`/`vzip2q_u8` (the low/high
/// interleave, ARM's equivalent of x86's `unpacklo`/`unpackhi`). Scalar tail,
/// same parity reasoning as the SSE2 backend.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn interleave_halves_u8(t1: *const u8, t2: *const u8, out: *mut u8, n: usize) {
    unsafe {
        const HALF_CHUNK: usize = 16;
        let v_n = n / (2 * HALF_CHUNK);

        let mut v1 = t1;
        let mut v2 = t2;
        let mut o = out;
        let mut i = 0usize;
        while i < v_n {
            let a = vld1q_u8(v1);
            let b = vld1q_u8(v2);
            let lo = vzip1q_u8(a, b);
            let hi = vzip2q_u8(a, b);
            vst1q_u8(o, lo);
            vst1q_u8(o.add(HALF_CHUNK), hi);
            v1 = v1.add(HALF_CHUNK);
            v2 = v2.add(HALF_CHUNK);
            o = o.add(2 * HALF_CHUNK);
            i += 1;
        }

        let mut k = v_n * 2 * HALF_CHUNK;
        let mut i1 = v_n * HALF_CHUNK;
        let mut i2 = v_n * HALF_CHUNK;
        while k < n {
            *out.add(k) = *t1.add(i1);
            i1 += 1;
            k += 1;
            if k < n {
                *out.add(k) = *t2.add(i2);
                i2 += 1;
                k += 1;
            }
        }
    }
}
