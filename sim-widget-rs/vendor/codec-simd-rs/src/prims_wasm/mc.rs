#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::*;

/// wasm impl of [`crate::mc_avg_u8`].  `u8x16_avgr_u` — single-instruction
/// unsigned rounding average.  16 bytes/iter.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
                let av = v128_load(a_row.add(col) as *const v128);
                let bv = v128_load(b_row.add(col) as *const v128);
                v128_store(d_row.add(col) as *mut v128, u8x16_avgr(av, bv));
                col += 16;
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

/// wasm `simd128` impl of [`crate::mc_8tap_h_u8`].  8 output columns per
/// iteration.
///
/// Source bytes widen u8→i16; `i32x4_extmul_{low,high}_i16x8` form the
/// exact widening product with the broadcast tap, accumulated into i32x4
/// lanes (cols 0-3 / 4-7).  After 8 taps: `(acc + 64) >> 7` (arithmetic),
/// signed-narrow to i16 then unsigned-narrow to u8 (= clamp [0,255]).
/// Loads use `v128_load64_zero` (8 bytes) to respect the `w+7` source
/// padding contract.  Tail scalar.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let round = i32x4_splat(64);
        let mut ft = [i16x8_splat(0); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..8 {
                    let s8 = v128_load64_zero(s_row.add(col + i) as *const u64);
                    let s16 = i16x8_extend_low_u8x16(s8);
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let r_lo = i32x4_shr(i32x4_add(acc_lo, round), 7);
                let r_hi = i32x4_shr(i32x4_add(acc_hi, round), 7);
                let i16s = i16x8_narrow_i32x4(r_lo, r_hi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm `simd128` impl of [`crate::mc_8tap_v_u8`].  Vertical 8-tap, 8
/// output columns per iteration.
///
/// Each tap loads a whole source row (`v128_load64_zero`, 8 bytes) and
/// `i32x4_extmul_{low,high}_i16x8` forms the exact widening product with
/// the broadcast tap into i32x4 lanes (cols 0-3 / 4-7).  Round-shift
/// `i32x4_shr` by 7, then signed+unsigned narrow for the clamp.  Tail
/// scalar.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let round = i32x4_splat(64);
        let mut ft = [i16x8_splat(0); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..8 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = i16x8_extend_low_u8x16(v128_load64_zero(s_ptr as *const u64));
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let r_lo = i32x4_shr(i32x4_add(acc_lo, round), 7);
                let r_hi = i32x4_shr(i32x4_add(acc_hi, round), 7);
                let i16s = i16x8_narrow_i32x4(r_lo, r_hi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm `simd128` impl of [`crate::mc_4tap_h_u8`].  Horizontal 4-tap,
/// 8 cols/iter, runtime `shift`.  `i32x4_extmul_*` widening product;
/// round bias added then `i32x4_shr` (arithmetic) + dual narrow.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let mut ft = [i16x8_splat(0); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..4 {
                    let s16 =
                        i16x8_extend_low_u8x16(v128_load64_zero(s_row.add(col + i) as *const u64));
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let t_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let t_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                let i16s = i16x8_narrow_i32x4(t_lo, t_hi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm `simd128` impl of [`crate::mc_4tap_v_u8`].  Vertical 4-tap,
/// 8 cols/iter, runtime `shift`.  Each tap loads a whole source row.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let mut ft = [i16x8_splat(0); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..4 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = i16x8_extend_low_u8x16(v128_load64_zero(s_ptr as *const u64));
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let t_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let t_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                let i16s = i16x8_narrow_i32x4(t_lo, t_hi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm `simd128` impl of [`crate::mc_6tap_h_u8`].  Horizontal 6-tap,
/// 8 cols/iter, runtime `shift`.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let mut ft = [i16x8_splat(0); 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..6 {
                    let s16 =
                        i16x8_extend_low_u8x16(v128_load64_zero(s_row.add(col + i) as *const u64));
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let t_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let t_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                let i16s = i16x8_narrow_i32x4(t_lo, t_hi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm `simd128` impl of [`crate::mc_6tap_v_u8`].  Vertical 6-tap,
/// 8 cols/iter, runtime `shift`.  Each tap loads a whole source row.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let mut ft = [i16x8_splat(0); 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = 1i32 << (shift - 1);
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..6 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = i16x8_extend_low_u8x16(v128_load64_zero(s_ptr as *const u64));
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let t_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let t_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                let i16s = i16x8_narrow_i32x4(t_lo, t_hi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm `simd128` impl of [`crate::mc_bilinear_h_u8`].  Horizontal
/// 2-tap, 8 cols/iter, runtime `shift`.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let ft = [i16x8_splat(*filter), i16x8_splat(*filter.add(1))];
        let f = [*filter as i32, *filter.add(1) as i32];
        let rscalar = 1i32 << (shift - 1);
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..2 {
                    let s16 =
                        i16x8_extend_low_u8x16(v128_load64_zero(s_row.add(col + i) as *const u64));
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let t_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let t_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                let i16s = i16x8_narrow_i32x4(t_lo, t_hi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm `simd128` impl of [`crate::mc_bilinear_v_u8`].  Vertical 2-tap,
/// 8 cols/iter, runtime `shift`.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let ft = [i16x8_splat(*filter), i16x8_splat(*filter.add(1))];
        let f = [*filter as i32, *filter.add(1) as i32];
        let rscalar = 1i32 << (shift - 1);
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..2 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = i16x8_extend_low_u8x16(v128_load64_zero(s_ptr as *const u64));
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let t_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let t_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                let i16s = i16x8_narrow_i32x4(t_lo, t_hi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm `simd128` impl of [`crate::mc_8tap_h_u8_to_i16`].  i16-output
/// horizontal 8-tap, 8 cols/iter, runtime `shift`.  `i32x4_extmul`
/// product, bias + `i32x4_shr`, `i16x8_narrow_i32x4` (saturating) — no
/// pixel clip.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let mut ft = [i16x8_splat(0); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..8 {
                    let s16 =
                        i16x8_extend_low_u8x16(v128_load64_zero(s_row.add(col + i) as *const u64));
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let r_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let r_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                let i16s = i16x8_narrow_i32x4(r_lo, r_hi);
                v128_store(d_row.add(col) as *mut v128, i16s);
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

/// wasm `simd128` impl of [`crate::mc_8tap_v_i16_to_u8`].  Vertical
/// 8-tap over an i16 intermediate, 8 cols/iter, runtime `shift`, clipped
/// to u8.  Loads i16 rows with `v128_load`; `i32x4_extmul` product.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let mut ft = [i16x8_splat(0); 8];
        let mut f = [0i32; 8];
        for i in 0..8 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..8 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = v128_load(s_ptr as *const v128);
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let t_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let t_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                let i16s = i16x8_narrow_i32x4(t_lo, t_hi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm impl of [`crate::mc_4tap_h_u8_to_i16`].  i16-output horizontal
/// 4-tap, 8 cols/iter, runtime `shift`.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let mut ft = [i16x8_splat(0); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..4 {
                    let s16 =
                        i16x8_extend_low_u8x16(v128_load64_zero(s_row.add(col + i) as *const u64));
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let r_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let r_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                v128_store(d_row.add(col) as *mut v128, i16x8_narrow_i32x4(r_lo, r_hi));
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

/// wasm impl of [`crate::mc_4tap_v_i16_to_u8`].  i16-input vertical
/// 4-tap, 8 cols/iter, runtime `shift`, clipped to u8.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let mut ft = [i16x8_splat(0); 4];
        let mut f = [0i32; 4];
        for i in 0..4 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..4 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = v128_load(s_ptr as *const v128);
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let t_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let t_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                let i16s = i16x8_narrow_i32x4(t_lo, t_hi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm impl of [`crate::mc_6tap_h_u8_to_i16`].  8 cols/iter, runtime shift.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let mut ft = [i16x8_splat(0); 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let s_row = src.offset(row as isize * src_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..6 {
                    let s16 =
                        i16x8_extend_low_u8x16(v128_load64_zero(s_row.add(col + i) as *const u64));
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let r_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let r_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                v128_store(d_row.add(col) as *mut v128, i16x8_narrow_i32x4(r_lo, r_hi));
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

/// wasm impl of [`crate::mc_6tap_v_i16_to_u8`].  8 cols/iter, runtime shift.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let mut ft = [i16x8_splat(0); 6];
        let mut f = [0i32; 6];
        for i in 0..6 {
            ft[i] = i16x8_splat(*filter.add(i));
            f[i] = *filter.add(i) as i32;
        }
        let rscalar = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
        let bias = i32x4_splat(rscalar);
        for row in 0..h {
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let mut acc_lo = i32x4_splat(0);
                let mut acc_hi = i32x4_splat(0);
                for i in 0..6 {
                    let s_ptr = src.offset((row + i) as isize * src_stride).add(col);
                    let s16 = v128_load(s_ptr as *const v128);
                    acc_lo = i32x4_add(acc_lo, i32x4_extmul_low_i16x8(s16, ft[i]));
                    acc_hi = i32x4_add(acc_hi, i32x4_extmul_high_i16x8(s16, ft[i]));
                }
                let t_lo = i32x4_shr(i32x4_add(acc_lo, bias), shift);
                let t_hi = i32x4_shr(i32x4_add(acc_hi, bias), shift);
                let i16s = i16x8_narrow_i32x4(t_lo, t_hi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm impl of [`crate::weighted_avg_u8`].  8 cols/iter; `extmul` per
/// weight, sum + offset, round-shift, clip to u8.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let w0v = i16x8_splat(w0);
        let w1v = i16x8_splat(w1);
        let offv = i32x4_splat(offset);
        let (w0s, w1s) = (w0 as i32, w1 as i32);
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let d_row = dst.offset(row as isize * dst_stride);
            let mut col = 0usize;
            while col + 8 <= w {
                let a16 = i16x8_extend_low_u8x16(v128_load64_zero(a_row.add(col) as *const u64));
                let b16 = i16x8_extend_low_u8x16(v128_load64_zero(b_row.add(col) as *const u64));
                let lo = i32x4_add(
                    i32x4_add(
                        i32x4_extmul_low_i16x8(a16, w0v),
                        i32x4_extmul_low_i16x8(b16, w1v),
                    ),
                    offv,
                );
                let hi = i32x4_add(
                    i32x4_add(
                        i32x4_extmul_high_i16x8(a16, w0v),
                        i32x4_extmul_high_i16x8(b16, w1v),
                    ),
                    offv,
                );
                let rlo = i32x4_shr(lo, shift);
                let rhi = i32x4_shr(hi, shift);
                let i16s = i16x8_narrow_i32x4(rlo, rhi);
                let u8s = u8x16_narrow_i16x8(i16s, i16s);
                v128_store64_lane::<0>(u8s, d_row.add(col) as *mut u64);
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

/// wasm impl of [`crate::mc_avg_u16`].  No native u16 rounding average, so
/// emulate the overflow-free `(a + b + 1) >> 1` via
/// `(a | b) - ((a ^ b) >> 1)`.  8 u16/iter.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
                let av = v128_load(a_row.add(col) as *const v128);
                let bv = v128_load(b_row.add(col) as *const v128);
                let avg = i16x8_sub(v128_or(av, bv), u16x8_shr(v128_xor(av, bv), 1));
                v128_store(d_row.add(col) as *mut v128, avg);
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
