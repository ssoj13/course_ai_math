#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

/// NEON impl of [`crate::sad_block_u8`].
///
/// `vabdq_u8` for per-byte abs-diff, accumulated into u16 via repeated
/// `vpadalq_u8` (pairwise-add-long-and-accumulate into u16x8), then
/// horizontally summed at the end via `vaddlvq_u16` → u32.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn sad_block_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    w: usize,
    h: usize,
) -> u32 {
    unsafe {
        let mut acc = vdupq_n_u16(0);
        let mut scalar_acc: u32 = 0;
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let av = vld1q_u8(a_row.add(col));
                let bv = vld1q_u8(b_row.add(col));
                let d = vabdq_u8(av, bv);
                acc = vpadalq_u8(acc, d);
                col += 16;
            }
            if col + 8 <= w {
                let av = vld1_u8(a_row.add(col));
                let bv = vld1_u8(b_row.add(col));
                let d = vabd_u8(av, bv);
                // Pairwise add-long into u16x4, then add to acc lo 4 lanes.
                let d_u16 = vpaddl_u8(d);
                // Combine d_u16 (u16x4) with zero high half, accumulate.
                let d_extended = vcombine_u16(d_u16, vdup_n_u16(0));
                acc = vaddq_u16(acc, d_extended);
                col += 8;
            }
            while col < w {
                scalar_acc += (*a_row.add(col)).abs_diff(*b_row.add(col)) as u32;
                col += 1;
            }
        }
        scalar_acc + vaddlvq_u16(acc)
    }
}

/// NEON impl of [`crate::satd_block_4x4_u8`].
///
/// 4 rows of 4 i16 fit into 4 `int16x4_t` D-registers.  Butterfly is
/// 8 `vadd_s16/vsub_s16` per stage.  Transpose via `vtrn1`/`vtrn2` at
/// widths 16/32.  Abs via `vabs_s16` (NEON `vqabs` is saturating but
/// pre-clamped SATD outputs fit in i16, so `vabs_s16` suffices).
/// Final reduction via widening pairwise add + `vaddv`.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn satd_block_4x4_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
) -> u32 {
    unsafe {
        // Load 4 bytes from each row, widen u8 → i16 (4 lanes in D-reg).
        // `read_unaligned`: the row pointers carry arbitrary strides (no 4-byte
        // alignment guarantee), so a plain `*(_ as *const u32)` deref is UB and
        // aborts under debug's misaligned-pointer check.
        let load_diff = |a_row: *const u8, b_row: *const u8| -> int16x4_t {
            let a4 = (a_row as *const u32).read_unaligned();
            let b4 = (b_row as *const u32).read_unaligned();
            let av_u8 = vcreate_u8(a4 as u64);
            let bv_u8 = vcreate_u8(b4 as u64);
            // Widen u8 → u16 (low 4 lanes), reinterpret as i16, subtract.
            let av = vreinterpret_s16_u16(vget_low_u16(vmovl_u8(av_u8)));
            let bv = vreinterpret_s16_u16(vget_low_u16(vmovl_u8(bv_u8)));
            vsub_s16(av, bv)
        };

        let d0 = load_diff(a, b);
        let d1 = load_diff(a.offset(a_stride), b.offset(b_stride));
        let d2 = load_diff(a.offset(2 * a_stride), b.offset(2 * b_stride));
        let d3 = load_diff(a.offset(3 * a_stride), b.offset(3 * b_stride));

        // First 1D Hadamard (across rows = vertical).
        let a0 = vadd_s16(d0, d1);
        let a1 = vsub_s16(d0, d1);
        let a2 = vadd_s16(d2, d3);
        let a3 = vsub_s16(d2, d3);
        let b0 = vadd_s16(a0, a2);
        let b1 = vadd_s16(a1, a3);
        let b2 = vsub_s16(a0, a2);
        let b3 = vsub_s16(a1, a3);

        // Transpose 4×4 via 2 stages of vtrn at widths 16 then 32.
        // Stage 1: 16-bit interleave.
        let t01 = vtrn_s16(b0, b1); // .0 = [b0_0, b1_0, b0_2, b1_2], .1 = [b0_1, b1_1, b0_3, b1_3]
        let t23 = vtrn_s16(b2, b3);
        // Stage 2: 32-bit interleave (treat pairs as i32).
        let p01_lo = vreinterpret_s16_s32(vtrn1_s32(
            vreinterpret_s32_s16(t01.0),
            vreinterpret_s32_s16(t23.0),
        ));
        let p23_lo = vreinterpret_s16_s32(vtrn2_s32(
            vreinterpret_s32_s16(t01.0),
            vreinterpret_s32_s16(t23.0),
        ));
        let p01_hi = vreinterpret_s16_s32(vtrn1_s32(
            vreinterpret_s32_s16(t01.1),
            vreinterpret_s32_s16(t23.1),
        ));
        let p23_hi = vreinterpret_s16_s32(vtrn2_s32(
            vreinterpret_s32_s16(t01.1),
            vreinterpret_s32_s16(t23.1),
        ));
        // After transpose, rows are: p01_lo (row 0), p01_hi (row 1),
        // p23_lo (row 2), p23_hi (row 3).
        let r0 = p01_lo;
        let r1 = p01_hi;
        let r2 = p23_lo;
        let r3 = p23_hi;

        // Second 1D Hadamard.
        let a0 = vadd_s16(r0, r1);
        let a1 = vsub_s16(r0, r1);
        let a2 = vadd_s16(r2, r3);
        let a3 = vsub_s16(r2, r3);
        let b0 = vadd_s16(a0, a2);
        let b1 = vadd_s16(a1, a3);
        let b2 = vsub_s16(a0, a2);
        let b3 = vsub_s16(a1, a3);

        // Abs each row.
        let ab0 = vabs_s16(b0);
        let ab1 = vabs_s16(b1);
        let ab2 = vabs_s16(b2);
        let ab3 = vabs_s16(b3);

        // Sum: combine to 8-lane vectors then widen-add to u32.
        let s01 = vcombine_s16(ab0, ab1); // 8 × i16
        let s23 = vcombine_s16(ab2, ab3);
        // Pairwise add-long: i16x8 -> i32x4.
        let p01 = vpaddlq_s16(s01);
        let p23 = vpaddlq_s16(s23);
        let p = vaddq_s32(p01, p23);
        let sum = vaddvq_s32(p) as u32;
        (sum + 1) >> 1
    }
}

/// 3-stage radix-2 Walsh-Hadamard butterfly over 8 `int16x8_t` lanes
/// (element-wise across the 8 registers). Intermediates fit i16 (max
/// `255*8 = 2040` after one pass, `*8 = 16320` after two — both < 32767).
#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn wht8_neon(r: [int16x8_t; 8]) -> [int16x8_t; 8] {
    unsafe {
        let s0 = vaddq_s16(r[0], r[4]);
        let s1 = vaddq_s16(r[1], r[5]);
        let s2 = vaddq_s16(r[2], r[6]);
        let s3 = vaddq_s16(r[3], r[7]);
        let s4 = vsubq_s16(r[0], r[4]);
        let s5 = vsubq_s16(r[1], r[5]);
        let s6 = vsubq_s16(r[2], r[6]);
        let s7 = vsubq_s16(r[3], r[7]);
        let p0 = vaddq_s16(s0, s2);
        let p1 = vaddq_s16(s1, s3);
        let p2 = vsubq_s16(s0, s2);
        let p3 = vsubq_s16(s1, s3);
        let p4 = vaddq_s16(s4, s6);
        let p5 = vaddq_s16(s5, s7);
        let p6 = vsubq_s16(s4, s6);
        let p7 = vsubq_s16(s5, s7);
        [
            vaddq_s16(p0, p1),
            vsubq_s16(p0, p1),
            vaddq_s16(p2, p3),
            vsubq_s16(p2, p3),
            vaddq_s16(p4, p5),
            vsubq_s16(p4, p5),
            vaddq_s16(p6, p7),
            vsubq_s16(p6, p7),
        ]
    }
}

/// Transpose 8 `int16x8_t` rows in registers (3 stages of trn at 16/32/64,
/// mirrors `transpose_8x8_i16`).
#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn transpose8_neon(r: [int16x8_t; 8]) -> [int16x8_t; 8] {
    unsafe {
        let a0 = vtrn1q_s16(r[0], r[1]);
        let a1 = vtrn2q_s16(r[0], r[1]);
        let a2 = vtrn1q_s16(r[2], r[3]);
        let a3 = vtrn2q_s16(r[2], r[3]);
        let a4 = vtrn1q_s16(r[4], r[5]);
        let a5 = vtrn2q_s16(r[4], r[5]);
        let a6 = vtrn1q_s16(r[6], r[7]);
        let a7 = vtrn2q_s16(r[6], r[7]);
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
        [
            vreinterpretq_s16_s64(vtrn1q_s64(
                vreinterpretq_s64_s16(b0),
                vreinterpretq_s64_s16(b4),
            )),
            vreinterpretq_s16_s64(vtrn1q_s64(
                vreinterpretq_s64_s16(b1),
                vreinterpretq_s64_s16(b5),
            )),
            vreinterpretq_s16_s64(vtrn1q_s64(
                vreinterpretq_s64_s16(b2),
                vreinterpretq_s64_s16(b6),
            )),
            vreinterpretq_s16_s64(vtrn1q_s64(
                vreinterpretq_s64_s16(b3),
                vreinterpretq_s64_s16(b7),
            )),
            vreinterpretq_s16_s64(vtrn2q_s64(
                vreinterpretq_s64_s16(b0),
                vreinterpretq_s64_s16(b4),
            )),
            vreinterpretq_s16_s64(vtrn2q_s64(
                vreinterpretq_s64_s16(b1),
                vreinterpretq_s64_s16(b5),
            )),
            vreinterpretq_s16_s64(vtrn2q_s64(
                vreinterpretq_s64_s16(b2),
                vreinterpretq_s64_s16(b6),
            )),
            vreinterpretq_s16_s64(vtrn2q_s64(
                vreinterpretq_s64_s16(b3),
                vreinterpretq_s64_s16(b7),
            )),
        ]
    }
}

/// NEON impl of [`crate::satd_block_8x8_u8`].
///
/// Loads 8 rows of `(a-b)` as `int16x8_t`, applies the WHT down the columns
/// (butterfly across the 8 registers), transposes 8×8 in registers, applies
/// the WHT again, then abs-sums all 64 lanes (`vabsq_s16` + `vpadalq_s16`).
/// Result `(sum + 2) >> 2`. Bit-identical to the scalar reference (the WHT is
/// orthogonal so the abs-sum is butterfly-order invariant).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn satd_block_8x8_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
) -> u32 {
    unsafe {
        let load = |ar: *const u8, br: *const u8| -> int16x8_t {
            let av = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(ar)));
            let bv = vreinterpretq_s16_u16(vmovl_u8(vld1_u8(br)));
            vsubq_s16(av, bv)
        };
        let rows = [
            load(a, b),
            load(a.offset(a_stride), b.offset(b_stride)),
            load(a.offset(2 * a_stride), b.offset(2 * b_stride)),
            load(a.offset(3 * a_stride), b.offset(3 * b_stride)),
            load(a.offset(4 * a_stride), b.offset(4 * b_stride)),
            load(a.offset(5 * a_stride), b.offset(5 * b_stride)),
            load(a.offset(6 * a_stride), b.offset(6 * b_stride)),
            load(a.offset(7 * a_stride), b.offset(7 * b_stride)),
        ];
        let v = wht8_neon(rows);
        let vt = transpose8_neon(v);
        let o = wht8_neon(vt);
        let mut acc = vdupq_n_s32(0);
        for reg in o {
            acc = vpadalq_s16(acc, vabsq_s16(reg));
        }
        let sum = vaddvq_s32(acc) as u32;
        (sum + 2) >> 2
    }
}

/// NEON impl of [`crate::sse_block_u8`].
///
/// Widens u8 to i16 via `vmovl_u8` then `vreinterpret_s16_u16`, subtracts
/// to get i16 diff, uses `vmlal_s16` / `vmlal_high_s16` to widening-
/// multiply-accumulate `d * d` into an i32x4 accumulator.  Drained per
/// row to u64.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn sse_block_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    w: usize,
    h: usize,
) -> u64 {
    unsafe {
        let mut sum: u64 = 0;
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let mut acc = vdupq_n_s32(0);
            let mut col = 0usize;
            while col + 16 <= w {
                let av = vld1q_u8(a_row.add(col));
                let bv = vld1q_u8(b_row.add(col));
                // Widen u8 → i16 (zero-extend then reinterpret as signed),
                // since a-b can be negative.
                let a_lo = vreinterpretq_s16_u16(vmovl_u8(vget_low_u8(av)));
                let a_hi = vreinterpretq_s16_u16(vmovl_high_u8(av));
                let b_lo = vreinterpretq_s16_u16(vmovl_u8(vget_low_u8(bv)));
                let b_hi = vreinterpretq_s16_u16(vmovl_high_u8(bv));
                let d_lo = vsubq_s16(a_lo, b_lo);
                let d_hi = vsubq_s16(a_hi, b_hi);
                // d² widening MAC into i32x4.
                acc = vmlal_s16(acc, vget_low_s16(d_lo), vget_low_s16(d_lo));
                acc = vmlal_high_s16(acc, d_lo, d_lo);
                acc = vmlal_s16(acc, vget_low_s16(d_hi), vget_low_s16(d_hi));
                acc = vmlal_high_s16(acc, d_hi, d_hi);
                col += 16;
            }
            if col + 8 <= w {
                let av = vld1_u8(a_row.add(col));
                let bv = vld1_u8(b_row.add(col));
                let a16 = vreinterpretq_s16_u16(vmovl_u8(av));
                let b16 = vreinterpretq_s16_u16(vmovl_u8(bv));
                let d = vsubq_s16(a16, b16);
                acc = vmlal_s16(acc, vget_low_s16(d), vget_low_s16(d));
                acc = vmlal_high_s16(acc, d, d);
                col += 8;
            }
            sum += vaddvq_s32(acc) as u32 as u64;
            while col < w {
                let av = *a_row.add(col) as i32;
                let bv = *b_row.add(col) as i32;
                let d = av - bv;
                sum += (d * d) as u64;
                col += 1;
            }
        }
        sum
    }
}

/// NEON impl of [`crate::variance_block_u8`].
///
/// `sum` accumulates pairwise-widened bytes into a `u32x4`
/// (`vpadalq_u16(acc, vpaddlq_u8(v))`), summed whole-block.  `sum_sq`
/// widens u8→i16 then `vmlal_s16` / `vmlal_high_s16` to MAC `v²` into an
/// `i32x4`, drained per row to u64.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn variance_block_u8(
    src: *const u8,
    stride: isize,
    w: usize,
    h: usize,
) -> (u32, u64) {
    unsafe {
        let mut sum_acc = vdupq_n_u32(0);
        let mut sum_sq: u64 = 0;
        let mut sum_scalar: u32 = 0;
        for row in 0..h {
            let s_row = src.offset(row as isize * stride);
            let mut sq_acc = vdupq_n_s32(0);
            let mut col = 0usize;
            while col + 16 <= w {
                let v = vld1q_u8(s_row.add(col));
                sum_acc = vpadalq_u16(sum_acc, vpaddlq_u8(v));
                let lo = vreinterpretq_s16_u16(vmovl_u8(vget_low_u8(v)));
                let hi = vreinterpretq_s16_u16(vmovl_high_u8(v));
                sq_acc = vmlal_s16(sq_acc, vget_low_s16(lo), vget_low_s16(lo));
                sq_acc = vmlal_high_s16(sq_acc, lo, lo);
                sq_acc = vmlal_s16(sq_acc, vget_low_s16(hi), vget_low_s16(hi));
                sq_acc = vmlal_high_s16(sq_acc, hi, hi);
                col += 16;
            }
            if col + 8 <= w {
                let v = vld1_u8(s_row.add(col));
                let w16 = vmovl_u8(v);
                sum_acc = vpadalq_u16(sum_acc, w16);
                let s16 = vreinterpretq_s16_u16(w16);
                sq_acc = vmlal_s16(sq_acc, vget_low_s16(s16), vget_low_s16(s16));
                sq_acc = vmlal_high_s16(sq_acc, s16, s16);
                col += 8;
            }
            sum_sq += vaddvq_s32(sq_acc) as u32 as u64;
            while col < w {
                let vv = *s_row.add(col) as u32;
                sum_scalar += vv;
                sum_sq += (vv * vv) as u64;
                col += 1;
            }
        }
        let sum = sum_scalar + vaddvq_u32(sum_acc);
        (sum, sum_sq)
    }
}

/// NEON impl of [`crate::sad_x4_block_u8`].  `vabdq_u8` + `vpadalq_u8`
/// per candidate; `a` loaded once per chunk.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn sad_x4_block_u8(
    a: *const u8,
    a_stride: isize,
    b0: *const u8,
    b1: *const u8,
    b2: *const u8,
    b3: *const u8,
    b_stride: isize,
    w: usize,
    h: usize,
) -> [u32; 4] {
    unsafe {
        let bs = [b0, b1, b2, b3];
        let mut acc = [vdupq_n_u16(0); 4];
        let mut scalar = [0u32; 4];
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let off = row as isize * b_stride;
            let br = [
                bs[0].offset(off),
                bs[1].offset(off),
                bs[2].offset(off),
                bs[3].offset(off),
            ];
            let mut col = 0usize;
            while col + 16 <= w {
                let av = vld1q_u8(a_row.add(col));
                for k in 0..4 {
                    let bv = vld1q_u8(br[k].add(col));
                    acc[k] = vpadalq_u8(acc[k], vabdq_u8(av, bv));
                }
                col += 16;
            }
            if col + 8 <= w {
                let av = vld1_u8(a_row.add(col));
                for k in 0..4 {
                    let bv = vld1_u8(br[k].add(col));
                    let d = vpaddl_u8(vabd_u8(av, bv));
                    acc[k] = vaddq_u16(acc[k], vcombine_u16(d, vdup_n_u16(0)));
                }
                col += 8;
            }
            while col < w {
                let av = *a_row.add(col);
                for k in 0..4 {
                    scalar[k] += av.abs_diff(*br[k].add(col)) as u32;
                }
                col += 1;
            }
        }
        let mut out = [0u32; 4];
        for k in 0..4 {
            out[k] = scalar[k] + vaddlvq_u16(acc[k]);
        }
        out
    }
}

/// NEON impl of [`crate::hadamard_4x4_i16`].  4 rows in `int16x4_t`
/// D-registers; column butterfly via `vadd/vsub_s16`, in-register
/// transpose via `vtrn` at widths 16/32, twice, with a final transpose
/// to restore row-major order.  Same algorithm as the host-verified SSE2
/// path (identical transpose orientation).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub(crate) unsafe fn hadamard_4x4_i16(src: *const i16, dst: *mut i16) {
    unsafe {
        let vb = |r0, r1, r2, r3| -> (int16x4_t, int16x4_t, int16x4_t, int16x4_t) {
            let a0 = vadd_s16(r0, r1);
            let a1 = vsub_s16(r0, r1);
            let a2 = vadd_s16(r2, r3);
            let a3 = vsub_s16(r2, r3);
            (
                vadd_s16(a0, a2),
                vadd_s16(a1, a3),
                vsub_s16(a0, a2),
                vsub_s16(a1, a3),
            )
        };
        let transpose = |b0, b1, b2, b3| -> (int16x4_t, int16x4_t, int16x4_t, int16x4_t) {
            let t01 = vtrn_s16(b0, b1);
            let t23 = vtrn_s16(b2, b3);
            let p01_lo = vreinterpret_s16_s32(vtrn1_s32(
                vreinterpret_s32_s16(t01.0),
                vreinterpret_s32_s16(t23.0),
            ));
            let p23_lo = vreinterpret_s16_s32(vtrn2_s32(
                vreinterpret_s32_s16(t01.0),
                vreinterpret_s32_s16(t23.0),
            ));
            let p01_hi = vreinterpret_s16_s32(vtrn1_s32(
                vreinterpret_s32_s16(t01.1),
                vreinterpret_s32_s16(t23.1),
            ));
            let p23_hi = vreinterpret_s16_s32(vtrn2_s32(
                vreinterpret_s32_s16(t01.1),
                vreinterpret_s32_s16(t23.1),
            ));
            (p01_lo, p01_hi, p23_lo, p23_hi)
        };
        let d0 = vld1_s16(src);
        let d1 = vld1_s16(src.add(4));
        let d2 = vld1_s16(src.add(8));
        let d3 = vld1_s16(src.add(12));
        let (m0, m1, m2, m3) = vb(d0, d1, d2, d3);
        let (mt0, mt1, mt2, mt3) = transpose(m0, m1, m2, m3);
        let (n0, n1, n2, n3) = vb(mt0, mt1, mt2, mt3);
        let (y0, y1, y2, y3) = transpose(n0, n1, n2, n3);
        vst1_s16(dst, y0);
        vst1_s16(dst.add(4), y1);
        vst1_s16(dst.add(8), y2);
        vst1_s16(dst.add(12), y3);
    }
}
