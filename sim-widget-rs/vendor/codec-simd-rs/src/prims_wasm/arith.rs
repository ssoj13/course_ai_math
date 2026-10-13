#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::*;

/// wasm impl of [`crate::abs_diff_u8`].
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn abs_diff_u8(a: *const u8, b: *const u8, dst: *mut u8, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 16 <= n {
            let av = v128_load(a.add(i) as *const v128);
            let bv = v128_load(b.add(i) as *const v128);
            v128_store(
                dst.add(i) as *mut v128,
                u8x16_sub(u8x16_max(av, bv), u8x16_min(av, bv)),
            );
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

/// wasm impl of [`crate::transpose_4x4_i16`].
///
/// wasm `simd128` doesn't have dedicated transpose intrinsics; uses
/// `i16x8_shuffle` with constant lane indices.  Loads two 8-lane vecs
/// (rows 0-1 and 2-3, each row 4 lanes), reshuffles, stores.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn transpose_4x4_i16(src: *const i16, dst: *mut i16) {
    unsafe {
        let r01 = v128_load(src.add(0) as *const v128); // rows 0+1 (8 lanes)
        let r23 = v128_load(src.add(8) as *const v128); // rows 2+3 (8 lanes)
        // Lane indices: row[i] starts at lane i (in transposed dst).
        // Original layout: r01 = [r0_0..3, r1_0..3]; r23 = [r2_0..3, r3_0..3].
        // Transposed row 0 = [r0_0, r1_0, r2_0, r3_0, r0_1, r1_1, r2_1, r3_1].
        // Transposed row 1 follows; pack as two 8-lane outputs.
        let row01 = i16x8_shuffle::<0, 4, 8, 12, 1, 5, 9, 13>(r01, r23);
        let row23 = i16x8_shuffle::<2, 6, 10, 14, 3, 7, 11, 15>(r01, r23);
        v128_store(dst.add(0) as *mut v128, row01);
        v128_store(dst.add(8) as *mut v128, row23);
    }
}

/// wasm impl of [`crate::transpose_8x8_i16`].
///
/// Same 3-stage interleave shape as SSE2/NEON, expressed via
/// `i16x8_shuffle` + `i32x4_shuffle` + `i64x2_shuffle`.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn transpose_8x8_i16(src: *const i16, dst: *mut i16) {
    unsafe {
        let r0 = v128_load(src.add(0) as *const v128);
        let r1 = v128_load(src.add(8) as *const v128);
        let r2 = v128_load(src.add(16) as *const v128);
        let r3 = v128_load(src.add(24) as *const v128);
        let r4 = v128_load(src.add(32) as *const v128);
        let r5 = v128_load(src.add(40) as *const v128);
        let r6 = v128_load(src.add(48) as *const v128);
        let r7 = v128_load(src.add(56) as *const v128);

        // Stage 1: 16-bit interleave (low/high pairs).
        let a0 = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(r0, r1);
        let a1 = i16x8_shuffle::<4, 12, 5, 13, 6, 14, 7, 15>(r0, r1);
        let a2 = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(r2, r3);
        let a3 = i16x8_shuffle::<4, 12, 5, 13, 6, 14, 7, 15>(r2, r3);
        let a4 = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(r4, r5);
        let a5 = i16x8_shuffle::<4, 12, 5, 13, 6, 14, 7, 15>(r4, r5);
        let a6 = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(r6, r7);
        let a7 = i16x8_shuffle::<4, 12, 5, 13, 6, 14, 7, 15>(r6, r7);

        // Stage 2: 32-bit interleave — shuffle a-values pairwise across
        // 32-bit boundaries. Convert to i32x4 indices for clarity.
        let b0 = i32x4_shuffle::<0, 4, 1, 5>(a0, a2);
        let b1 = i32x4_shuffle::<2, 6, 3, 7>(a0, a2);
        let b2 = i32x4_shuffle::<0, 4, 1, 5>(a1, a3);
        let b3 = i32x4_shuffle::<2, 6, 3, 7>(a1, a3);
        let b4 = i32x4_shuffle::<0, 4, 1, 5>(a4, a6);
        let b5 = i32x4_shuffle::<2, 6, 3, 7>(a4, a6);
        let b6 = i32x4_shuffle::<0, 4, 1, 5>(a5, a7);
        let b7 = i32x4_shuffle::<2, 6, 3, 7>(a5, a7);

        // Stage 3: 64-bit interleave.
        let t0 = i64x2_shuffle::<0, 2>(b0, b4);
        let t1 = i64x2_shuffle::<1, 3>(b0, b4);
        let t2 = i64x2_shuffle::<0, 2>(b1, b5);
        let t3 = i64x2_shuffle::<1, 3>(b1, b5);
        let t4 = i64x2_shuffle::<0, 2>(b2, b6);
        let t5 = i64x2_shuffle::<1, 3>(b2, b6);
        let t6 = i64x2_shuffle::<0, 2>(b3, b7);
        let t7 = i64x2_shuffle::<1, 3>(b3, b7);

        v128_store(dst.add(0) as *mut v128, t0);
        v128_store(dst.add(8) as *mut v128, t1);
        v128_store(dst.add(16) as *mut v128, t2);
        v128_store(dst.add(24) as *mut v128, t3);
        v128_store(dst.add(32) as *mut v128, t4);
        v128_store(dst.add(40) as *mut v128, t5);
        v128_store(dst.add(48) as *mut v128, t6);
        v128_store(dst.add(56) as *mut v128, t7);
    }
}

/// wasm impl of [`crate::sat_add_i16`].  8 lanes/iter via `i16x8_add_sat`.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn sat_add_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            let av = v128_load(a.add(i) as *const v128);
            let bv = v128_load(b.add(i) as *const v128);
            v128_store(dst.add(i) as *mut v128, i16x8_add_sat(av, bv));
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

/// wasm impl of [`crate::sat_sub_i16`].  8 lanes/iter via `i16x8_sub_sat`.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn sat_sub_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            let av = v128_load(a.add(i) as *const v128);
            let bv = v128_load(b.add(i) as *const v128);
            v128_store(dst.add(i) as *mut v128, i16x8_sub_sat(av, bv));
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

/// wasm impl of [`crate::abs_i16`].
///
/// `i16x8_abs` does NOT saturate `i16::MIN`: it returns `0x8000` =
/// `i16::MIN` (since `-(-32768)` overflows).  Use the `max(v, sat_neg(v))`
/// trick instead, where `i16x8_sub_sat(zero, v)` saturating-negates
/// (producing `i16::MAX` for `i16::MIN` input).
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn abs_i16(src: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let zero = i16x8_splat(0);
        let mut i = 0usize;
        while i + 8 <= n {
            let v = v128_load(src.add(i) as *const v128);
            let neg = i16x8_sub_sat(zero, v);
            v128_store(dst.add(i) as *mut v128, i16x8_max(v, neg));
            i += 8;
        }
        while i < n {
            let v = *src.add(i);
            *dst.add(i) = if v == i16::MIN { i16::MAX } else { v.abs() };
            i += 1;
        }
    }
}

/// wasm impl of [`crate::mul_hi_round_i16`].
///
/// `i16x8_q15mulr_sat` — Q15 saturating rounding multiply.  Semantically
/// `((a * b) + (1 << 14)) >> 15` with saturation.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn mul_hi_round_i16(a: *const i16, b: *const i16, dst: *mut i16, n: usize) {
    unsafe {
        let mut i = 0usize;
        while i + 8 <= n {
            let av = v128_load(a.add(i) as *const v128);
            let bv = v128_load(b.add(i) as *const v128);
            v128_store(dst.add(i) as *mut v128, i16x8_q15mulr_sat(av, bv));
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

/// wasm impl of [`crate::dequant_block_i16`].  8 i16/iter; `extmul`
/// widening multiply, round-shift, `i16x8_narrow_i32x4`.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
pub(crate) unsafe fn dequant_block_i16(buf: *mut i16, n: usize, scale: i16, shift: u32) {
    unsafe {
        let scv = i16x8_splat(scale);
        let sc = scale as i32;
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = i32x4_splat(rscalar);
        let mut i = 0usize;
        while i + 8 <= n {
            let v = v128_load(buf.add(i) as *const v128);
            let lo = i32x4_extmul_low_i16x8(v, scv);
            let hi = i32x4_extmul_high_i16x8(v, scv);
            let rlo = i32x4_shr(i32x4_add(lo, bias), shift);
            let rhi = i32x4_shr(i32x4_add(hi, bias), shift);
            v128_store(buf.add(i) as *mut v128, i16x8_narrow_i32x4(rlo, rhi));
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
