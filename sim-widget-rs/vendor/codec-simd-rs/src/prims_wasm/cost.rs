#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::*;

/// wasm impl of [`crate::sad_block_u8`].
///
/// No native `sad`-style instruction; emulate via `u8x16_max - u8x16_min`
/// (per-byte abs diff) then `u16x8_extadd_pairwise_u8x16` (8 pairwise
/// sums into u16), then `u32x4_extadd_pairwise_u16x8` (4 pairwise sums
/// into u32), then horizontal sum.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn sad_block_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    w: usize,
    h: usize,
) -> u32 {
    unsafe {
        let mut acc = u32x4_splat(0);
        let mut scalar_acc: u32 = 0;
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let av = v128_load(a_row.add(col) as *const v128);
                let bv = v128_load(b_row.add(col) as *const v128);
                let d = u8x16_sub(u8x16_max(av, bv), u8x16_min(av, bv));
                let widened = u16x8_extadd_pairwise_u8x16(d);
                acc = u32x4_add(acc, u32x4_extadd_pairwise_u16x8(widened));
                col += 16;
            }
            while col < w {
                scalar_acc += (*a_row.add(col)).abs_diff(*b_row.add(col)) as u32;
                col += 1;
            }
        }
        // Horizontal sum of u32x4.
        let h0 = u32x4_extract_lane::<0>(acc);
        let h1 = u32x4_extract_lane::<1>(acc);
        let h2 = u32x4_extract_lane::<2>(acc);
        let h3 = u32x4_extract_lane::<3>(acc);
        scalar_acc + h0 + h1 + h2 + h3
    }
}

/// wasm impl of [`crate::satd_block_4x4_u8`].
///
/// 4 rows of 4 i16 each.  Load each row into the low 4 lanes of a
/// `v128` (upper 4 lanes left as zero).  Butterfly stages use
/// `i16x8_add / i16x8_sub` operating on all 8 lanes (upper 4 are
/// safely zero throughout).  Transpose uses `i16x8_shuffle` with
/// const lane indices.  Abs via `i16x8_abs` (input range
/// `[-4080, 4080]` from SATD math, never hits `i16::MIN`).
/// Reduction via widening pairwise add into u32 lanes.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn satd_block_4x4_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
) -> u32 {
    unsafe {
        // Load 4 bytes from each row, widen u8 → i16 (4 lanes in low 64).
        let load_diff = |a_row: *const u8, b_row: *const u8| -> v128 {
            // read_unaligned: row pointers carry arbitrary strides (no 4-byte align).
            let a4 = (a_row as *const u32).read_unaligned();
            let b4 = (b_row as *const u32).read_unaligned();
            // Splat the u32 into low lane, then widen u8→i16 of low 8 bytes.
            let av = u64x2(a4 as u64, 0);
            let bv = u64x2(b4 as u64, 0);
            // u8x16 -> i16x8 (low half) sign-extended for u8 zero-extended.
            // i16x8_extend_low_u8x16: zero-extend low 8 u8 lanes to 8 i16.
            let a16 = i16x8_extend_low_u8x16(av);
            let b16 = i16x8_extend_low_u8x16(bv);
            i16x8_sub(a16, b16)
        };

        let d0 = load_diff(a, b);
        let d1 = load_diff(a.offset(a_stride), b.offset(b_stride));
        let d2 = load_diff(a.offset(2 * a_stride), b.offset(2 * b_stride));
        let d3 = load_diff(a.offset(3 * a_stride), b.offset(3 * b_stride));

        // First 1D Hadamard (across rows = vertical).
        // Upper 4 lanes of each d_i are zero, so ops are correct in low 4.
        let a0 = i16x8_add(d0, d1);
        let a1 = i16x8_sub(d0, d1);
        let a2 = i16x8_add(d2, d3);
        let a3 = i16x8_sub(d2, d3);
        let b0 = i16x8_add(a0, a2);
        let b1 = i16x8_add(a1, a3);
        let b2 = i16x8_sub(a0, a2);
        let b3 = i16x8_sub(a1, a3);

        // Transpose 4×4 via shuffle.  We have 4 vectors with valid data
        // in low 4 lanes [r_0, r_1, r_2, r_3, 0, 0, 0, 0].  Build the
        // transposed rows by interleaving pairs:
        //
        // Stage 1 (16-bit pair-interleave):
        //   t01 = [b0_0, b1_0, b0_1, b1_1, b0_2, b1_2, b0_3, b1_3]
        //   t23 = [b2_0, b3_0, b2_1, b3_1, b2_2, b3_2, b2_3, b3_3]
        let t01 = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(b0, b1);
        let t23 = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(b2, b3);
        // Stage 2 (32-bit pair-interleave): operate on t01/t23 as i32x4.
        // After: r0 = [b0_0, b1_0, b2_0, b3_0, b0_1, b1_1, b2_1, b3_1]
        //        r2 = [b0_2, b1_2, b2_2, b3_2, b0_3, b1_3, b2_3, b3_3]
        let r02 = i32x4_shuffle::<0, 4, 1, 5>(t01, t23);
        let r13 = i32x4_shuffle::<2, 6, 3, 7>(t01, t23);
        // r02 holds rows 0 and 2 (interleaved as pairs of 64-bit halves);
        // r13 holds rows 1 and 3.  Extract:
        let r0 = i64x2_shuffle::<0, 2>(r02, u64x2(0, 0)); // low 64 of r02 = row 0
        let r2 = i64x2_shuffle::<1, 3>(r02, u64x2(0, 0)); // high 64 of r02 = row 2
        let r1 = i64x2_shuffle::<0, 2>(r13, u64x2(0, 0));
        let r3 = i64x2_shuffle::<1, 3>(r13, u64x2(0, 0));

        // Second 1D Hadamard.
        let a0 = i16x8_add(r0, r1);
        let a1 = i16x8_sub(r0, r1);
        let a2 = i16x8_add(r2, r3);
        let a3 = i16x8_sub(r2, r3);
        let b0 = i16x8_add(a0, a2);
        let b1 = i16x8_add(a1, a3);
        let b2 = i16x8_sub(a0, a2);
        let b3 = i16x8_sub(a1, a3);

        // Abs (values in [-4080, 4080], safe from i16::MIN saturation).
        let ab0 = i16x8_abs(b0);
        let ab1 = i16x8_abs(b1);
        let ab2 = i16x8_abs(b2);
        let ab3 = i16x8_abs(b3);

        // Sum: each ab_i has 4 valid lanes in low 64 (upper 4 are 0).
        // Sum across all 32 elements.
        let s01 = i16x8_add(ab0, ab1);
        let s23 = i16x8_add(ab2, ab3);
        let s = i16x8_add(s01, s23);
        // Widen-pairwise: i16x8 -> i32x4 (sums pairs).
        let s32 = i32x4_extadd_pairwise_i16x8(s);
        // Horizontal sum of 4 i32 lanes.
        let h0 = i32x4_extract_lane::<0>(s32);
        let h1 = i32x4_extract_lane::<1>(s32);
        let h2 = i32x4_extract_lane::<2>(s32);
        let h3 = i32x4_extract_lane::<3>(s32);
        let sum = (h0 + h1 + h2 + h3) as u32;
        (sum + 1) >> 1
    }
}

/// wasm impl of [`crate::sse_block_u8`].
///
/// `i32x4_extmul_low_i16x8(d, d)` and `i32x4_extmul_high_i16x8(d, d)`
/// do widening i16² into i32x4 — equivalent to NEON's vmull_s16.
/// Accumulate into an i32x4, drain per row to u64.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
            let mut acc = i32x4_splat(0);
            let mut col = 0usize;
            while col + 16 <= w {
                let av = v128_load(a_row.add(col) as *const v128);
                let bv = v128_load(b_row.add(col) as *const v128);
                // Widen u8 → i16 (zero-extend).
                let a_lo = i16x8_extend_low_u8x16(av);
                let a_hi = i16x8_extend_high_u8x16(av);
                let b_lo = i16x8_extend_low_u8x16(bv);
                let b_hi = i16x8_extend_high_u8x16(bv);
                let d_lo = i16x8_sub(a_lo, b_lo);
                let d_hi = i16x8_sub(a_hi, b_hi);
                // Widening i16² → i32.
                acc = i32x4_add(acc, i32x4_extmul_low_i16x8(d_lo, d_lo));
                acc = i32x4_add(acc, i32x4_extmul_high_i16x8(d_lo, d_lo));
                acc = i32x4_add(acc, i32x4_extmul_low_i16x8(d_hi, d_hi));
                acc = i32x4_add(acc, i32x4_extmul_high_i16x8(d_hi, d_hi));
                col += 16;
            }
            // Drain.
            let h0 = i32x4_extract_lane::<0>(acc);
            let h1 = i32x4_extract_lane::<1>(acc);
            let h2 = i32x4_extract_lane::<2>(acc);
            let h3 = i32x4_extract_lane::<3>(acc);
            sum += (h0 + h1 + h2 + h3) as u32 as u64;
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

/// wasm impl of [`crate::variance_block_u8`].
///
/// `sum` via `u16x8_extadd_pairwise_u8x16` then
/// `u32x4_extadd_pairwise_u16x8`, accumulated whole-block in a `u32x4`.
/// `sum_sq` widens u8→i16 then `i32x4_extmul_*_i16x8(v, v)` for the
/// widening square, drained per row to u64.  16 bytes/iter + scalar tail.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn variance_block_u8(
    src: *const u8,
    stride: isize,
    w: usize,
    h: usize,
) -> (u32, u64) {
    unsafe {
        let mut sum_acc = u32x4_splat(0);
        let mut sum_sq: u64 = 0;
        let mut sum_scalar: u32 = 0;
        for row in 0..h {
            let s_row = src.offset(row as isize * stride);
            let mut sq_acc = i32x4_splat(0);
            let mut col = 0usize;
            while col + 16 <= w {
                let v = v128_load(s_row.add(col) as *const v128);
                let w16 = u16x8_extadd_pairwise_u8x16(v);
                sum_acc = u32x4_add(sum_acc, u32x4_extadd_pairwise_u16x8(w16));
                let lo = i16x8_extend_low_u8x16(v);
                let hi = i16x8_extend_high_u8x16(v);
                sq_acc = i32x4_add(sq_acc, i32x4_extmul_low_i16x8(lo, lo));
                sq_acc = i32x4_add(sq_acc, i32x4_extmul_high_i16x8(lo, lo));
                sq_acc = i32x4_add(sq_acc, i32x4_extmul_low_i16x8(hi, hi));
                sq_acc = i32x4_add(sq_acc, i32x4_extmul_high_i16x8(hi, hi));
                col += 16;
            }
            // Drain sq_acc.
            let s0 = i32x4_extract_lane::<0>(sq_acc);
            let s1 = i32x4_extract_lane::<1>(sq_acc);
            let s2 = i32x4_extract_lane::<2>(sq_acc);
            let s3 = i32x4_extract_lane::<3>(sq_acc);
            sum_sq += (s0 + s1 + s2 + s3) as u32 as u64;
            while col < w {
                let vv = *s_row.add(col) as u32;
                sum_scalar += vv;
                sum_sq += (vv * vv) as u64;
                col += 1;
            }
        }
        let h0 = u32x4_extract_lane::<0>(sum_acc);
        let h1 = u32x4_extract_lane::<1>(sum_acc);
        let h2 = u32x4_extract_lane::<2>(sum_acc);
        let h3 = u32x4_extract_lane::<3>(sum_acc);
        let sum = sum_scalar + h0 + h1 + h2 + h3;
        (sum, sum_sq)
    }
}

/// wasm impl of [`crate::sad_x4_block_u8`].  Emulated SAD
/// (`max-min` + pairwise widen) per candidate; `a` loaded once per chunk.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let mut acc = [u32x4_splat(0); 4];
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
                let av = v128_load(a_row.add(col) as *const v128);
                for k in 0..4 {
                    let bv = v128_load(br[k].add(col) as *const v128);
                    let d = u8x16_sub(u8x16_max(av, bv), u8x16_min(av, bv));
                    let widened = u16x8_extadd_pairwise_u8x16(d);
                    acc[k] = u32x4_add(acc[k], u32x4_extadd_pairwise_u16x8(widened));
                }
                col += 16;
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
            out[k] = scalar[k]
                + u32x4_extract_lane::<0>(acc[k])
                + u32x4_extract_lane::<1>(acc[k])
                + u32x4_extract_lane::<2>(acc[k])
                + u32x4_extract_lane::<3>(acc[k]);
        }
        out
    }
}

/// wasm impl of [`crate::hadamard_4x4_i16`].  4 rows in the low 64 bits
/// of 4 `v128`; column butterfly via `i16x8_add/sub`, shuffle-based 4×4
/// transpose, twice, with a final transpose to restore row-major order.
/// Same algorithm as the host-verified SSE2 path.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn hadamard_4x4_i16(src: *const i16, dst: *mut i16) {
    unsafe {
        let vb = |r0, r1, r2, r3| -> (v128, v128, v128, v128) {
            let a0 = i16x8_add(r0, r1);
            let a1 = i16x8_sub(r0, r1);
            let a2 = i16x8_add(r2, r3);
            let a3 = i16x8_sub(r2, r3);
            (
                i16x8_add(a0, a2),
                i16x8_add(a1, a3),
                i16x8_sub(a0, a2),
                i16x8_sub(a1, a3),
            )
        };
        let transpose = |b0, b1, b2, b3| -> (v128, v128, v128, v128) {
            let t01 = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(b0, b1);
            let t23 = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(b2, b3);
            let r02 = i32x4_shuffle::<0, 4, 1, 5>(t01, t23);
            let r13 = i32x4_shuffle::<2, 6, 3, 7>(t01, t23);
            let z = u64x2(0, 0);
            let r0 = i64x2_shuffle::<0, 2>(r02, z);
            let r2 = i64x2_shuffle::<1, 3>(r02, z);
            let r1 = i64x2_shuffle::<0, 2>(r13, z);
            let r3 = i64x2_shuffle::<1, 3>(r13, z);
            (r0, r1, r2, r3)
        };
        let d0 = v128_load64_zero(src as *const u64);
        let d1 = v128_load64_zero(src.add(4) as *const u64);
        let d2 = v128_load64_zero(src.add(8) as *const u64);
        let d3 = v128_load64_zero(src.add(12) as *const u64);
        let (m0, m1, m2, m3) = vb(d0, d1, d2, d3);
        let (mt0, mt1, mt2, mt3) = transpose(m0, m1, m2, m3);
        let (n0, n1, n2, n3) = vb(mt0, mt1, mt2, mt3);
        let (y0, y1, y2, y3) = transpose(n0, n1, n2, n3);
        v128_store64_lane::<0>(y0, dst as *mut u64);
        v128_store64_lane::<0>(y1, dst.add(4) as *mut u64);
        v128_store64_lane::<0>(y2, dst.add(8) as *mut u64);
        v128_store64_lane::<0>(y3, dst.add(12) as *mut u64);
    }
}
