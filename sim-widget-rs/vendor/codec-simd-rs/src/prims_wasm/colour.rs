#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::*;

/// wasm impl of [`crate::lincomb3_clip_u8`].  8/iter; `extmul` per
/// coefficient, sum + offset, round-shift, clip to u8.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
        let c0v = i16x8_splat(c0);
        let c1v = i16x8_splat(c1);
        let c2v = i16x8_splat(c2);
        let offv = i32x4_splat(offset);
        let (c0s, c1s, c2s) = (c0 as i32, c1 as i32, c2 as i32);
        let mut i = 0usize;
        while i + 8 <= n {
            let a16 = i16x8_extend_low_u8x16(v128_load64_zero(p0.add(i) as *const u64));
            let b16 = i16x8_extend_low_u8x16(v128_load64_zero(p1.add(i) as *const u64));
            let c16 = i16x8_extend_low_u8x16(v128_load64_zero(p2.add(i) as *const u64));
            let lo = i32x4_add(
                i32x4_add(
                    i32x4_add(
                        i32x4_extmul_low_i16x8(a16, c0v),
                        i32x4_extmul_low_i16x8(b16, c1v),
                    ),
                    i32x4_extmul_low_i16x8(c16, c2v),
                ),
                offv,
            );
            let hi = i32x4_add(
                i32x4_add(
                    i32x4_add(
                        i32x4_extmul_high_i16x8(a16, c0v),
                        i32x4_extmul_high_i16x8(b16, c1v),
                    ),
                    i32x4_extmul_high_i16x8(c16, c2v),
                ),
                offv,
            );
            let rlo = i32x4_shr(lo, shift);
            let rhi = i32x4_shr(hi, shift);
            let i16s = i16x8_narrow_i32x4(rlo, rhi);
            let u8s = u8x16_narrow_i16x8(i16s, i16s);
            v128_store64_lane::<0>(u8s, dst.add(i) as *mut u64);
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

/// wasm simd128 impl of [`crate::rct_fwd_i32`].  4 i32/iter.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
            let rr = v128_load(r.add(i) as *const v128);
            let gg = v128_load(g.add(i) as *const v128);
            let bb = v128_load(b.add(i) as *const v128);
            let yv = i32x4_shr(i32x4_add(i32x4_add(rr, i32x4_shl(gg, 1)), bb), 2);
            v128_store(y.add(i) as *mut v128, yv);
            v128_store(cb.add(i) as *mut v128, i32x4_sub(bb, gg));
            v128_store(cr.add(i) as *mut v128, i32x4_sub(rr, gg));
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

/// wasm simd128 impl of [`crate::rct_bwd_i32`].  4 i32/iter.
#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
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
            let yv = v128_load(y.add(i) as *const v128);
            let cbv = v128_load(cb.add(i) as *const v128);
            let crv = v128_load(cr.add(i) as *const v128);
            let gg = i32x4_sub(yv, i32x4_shr(i32x4_add(cbv, crv), 2));
            v128_store(g.add(i) as *mut v128, gg);
            v128_store(r.add(i) as *mut v128, i32x4_add(crv, gg));
            v128_store(b.add(i) as *mut v128, i32x4_add(cbv, gg));
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
