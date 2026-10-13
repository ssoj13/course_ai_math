/// Scalar reference for [`crate::sad_block_u8`].
///
/// Sum of absolute differences across a `w × h` u8 block.
///
/// # Safety
/// `a` valid for reads at `a + row*a_stride + col` for `row ∈ 0..h`,
/// `col ∈ 0..w`; same for `b`.
pub(crate) unsafe fn sad_block_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    w: usize,
    h: usize,
) -> u32 {
    let mut sum: u32 = 0;
    for row in 0..h {
        let a_row = unsafe { a.offset(row as isize * a_stride) };
        let b_row = unsafe { b.offset(row as isize * b_stride) };
        for col in 0..w {
            let av = unsafe { *a_row.add(col) };
            let bv = unsafe { *b_row.add(col) };
            sum += av.abs_diff(bv) as u32;
        }
    }
    sum
}

/// Scalar reference for [`crate::satd_block_4x4_u8`].
///
/// Hadamard-transformed SAD on a 4×4 u8 block.  Computes
/// `sum |H * D * H'|` where `H` is the 4×4 Walsh-Hadamard matrix and
/// `D` is the diff matrix `(a - b)`.  Result is `(sum + 1) >> 1`
/// (round-to-nearest divide-by-2, matching ffmpeg `satd_4x4_8_c`).
///
/// # Safety
/// `a` and `b` valid for `3*stride + 4` bytes each.
pub(crate) unsafe fn satd_block_4x4_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
) -> u32 {
    // Step 1: load (a - b) as 16 i16 in row-major order.
    let mut d = [0i16; 16];
    for r in 0..4isize {
        let a_row = unsafe { a.offset(r * a_stride) };
        let b_row = unsafe { b.offset(r * b_stride) };
        for c in 0..4 {
            let av = unsafe { *a_row.add(c) } as i16;
            let bv = unsafe { *b_row.add(c) } as i16;
            d[(r as usize) * 4 + c] = av - bv;
        }
    }
    // Step 2: horizontal Hadamard (per row).
    let mut h = [0i16; 16];
    for r in 0..4 {
        let r0 = d[r * 4];
        let r1 = d[r * 4 + 1];
        let r2 = d[r * 4 + 2];
        let r3 = d[r * 4 + 3];
        let t0 = r0 + r1;
        let t1 = r0 - r1;
        let t2 = r2 + r3;
        let t3 = r2 - r3;
        h[r * 4] = t0 + t2;
        h[r * 4 + 1] = t1 + t3;
        h[r * 4 + 2] = t0 - t2;
        h[r * 4 + 3] = t1 - t3;
    }
    // Step 3: vertical Hadamard (per column).
    let mut v = [0i16; 16];
    for c in 0..4 {
        let r0 = h[c];
        let r1 = h[4 + c];
        let r2 = h[8 + c];
        let r3 = h[12 + c];
        let t0 = r0 + r1;
        let t1 = r0 - r1;
        let t2 = r2 + r3;
        let t3 = r2 - r3;
        v[c] = t0 + t2;
        v[4 + c] = t1 + t3;
        v[8 + c] = t0 - t2;
        v[12 + c] = t1 - t3;
    }
    // Step 4: sum of abs, then (sum + 1) >> 1.
    let mut sum: u32 = 0;
    for i in 0..16 {
        sum += (v[i] as i32).unsigned_abs();
    }
    (sum + 1) >> 1
}

/// Unnormalised 8-point fast Walsh-Hadamard butterfly (radix-2, 3 stages).
/// The transform is orthogonal, so the multiset of outputs (hence the sum of
/// absolute values) is invariant to the butterfly ordering — that is why any
/// valid WHT yields the same SATD.
#[inline]
fn wht8(mut x: [i32; 8]) -> [i32; 8] {
    // Stage 1 (stride 4).
    for i in 0..4 {
        let t = x[i];
        x[i] = t + x[i + 4];
        x[i + 4] = t - x[i + 4];
    }
    // Stage 2 (stride 2 within each half).
    for base in [0usize, 4] {
        for i in 0..2 {
            let t = x[base + i];
            x[base + i] = t + x[base + i + 2];
            x[base + i + 2] = t - x[base + i + 2];
        }
    }
    // Stage 3 (stride 1 within each quarter).
    for base in [0usize, 2, 4, 6] {
        let t = x[base];
        x[base] = t + x[base + 1];
        x[base + 1] = t - x[base + 1];
    }
    x
}

/// Scalar reference for [`crate::satd_block_8x8_u8`].
///
/// Hadamard-transformed SAD on an 8×8 u8 block: `sum |H8 * D * H8'|` with the
/// HEVC/HM normalisation `(sum + 2) >> 2` (as in kvazaar `satd_8x8_subblock`).
/// Uses an `i32` accumulator (the 2D WHT gain is 64; max coeff magnitude
/// `255*64 = 16320` and the 64-term abs-sum both fit comfortably).
///
/// # Safety
/// `a` and `b` valid for `7*stride + 8` bytes each.
pub(crate) unsafe fn satd_block_8x8_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
) -> u32 {
    // Load (a - b) as 64 i32 row-major.
    let mut d = [0i32; 64];
    for r in 0..8isize {
        let a_row = unsafe { a.offset(r * a_stride) };
        let b_row = unsafe { b.offset(r * b_stride) };
        for c in 0..8 {
            let av = unsafe { *a_row.add(c) } as i32;
            let bv = unsafe { *b_row.add(c) } as i32;
            d[(r as usize) * 8 + c] = av - bv;
        }
    }
    // Horizontal WHT (per row).
    let mut h = [0i32; 64];
    for r in 0..8 {
        let row = [
            d[r * 8],
            d[r * 8 + 1],
            d[r * 8 + 2],
            d[r * 8 + 3],
            d[r * 8 + 4],
            d[r * 8 + 5],
            d[r * 8 + 6],
            d[r * 8 + 7],
        ];
        let out = wht8(row);
        h[r * 8..r * 8 + 8].copy_from_slice(&out);
    }
    // Vertical WHT (per column) + abs-sum.
    let mut sum: u32 = 0;
    for c in 0..8 {
        let col = [
            h[c],
            h[8 + c],
            h[16 + c],
            h[24 + c],
            h[32 + c],
            h[40 + c],
            h[48 + c],
            h[56 + c],
        ];
        let out = wht8(col);
        for v in out {
            sum += v.unsigned_abs();
        }
    }
    (sum + 2) >> 2
}

/// Scalar reference for [`crate::sse_block_u8`].
///
/// Sum of squared errors across a `w × h` u8 block.  Per-pixel:
/// `(a - b) as i32 * (a - b) as i32`.  Result is `u64` for safety
/// at large blocks; consumers typically receive u32-range values.
///
/// # Safety
/// `a` and `b` valid for the geometry described by their strides.
pub(crate) unsafe fn sse_block_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    w: usize,
    h: usize,
) -> u64 {
    let mut sum: u64 = 0;
    for row in 0..h {
        let a_row = unsafe { a.offset(row as isize * a_stride) };
        let b_row = unsafe { b.offset(row as isize * b_stride) };
        for col in 0..w {
            let av = unsafe { *a_row.add(col) } as i32;
            let bv = unsafe { *b_row.add(col) } as i32;
            let d = av - bv;
            sum += (d * d) as u64;
        }
    }
    sum
}

/// Scalar reference for [`crate::variance_block_u8`].
///
/// Returns `(sum, sum_sq)` over a `w × h` u8 block, where
/// `sum = Σ src[i]` (fits `u32`: 128×128×255 < u32::MAX) and
/// `sum_sq = Σ src[i]²` (returned `u64` for headroom).
///
/// # Safety
/// `src` valid for reads at `src + row*stride + col` for `row ∈ 0..h`,
/// `col ∈ 0..w`.
pub(crate) unsafe fn variance_block_u8(
    src: *const u8,
    stride: isize,
    w: usize,
    h: usize,
) -> (u32, u64) {
    let mut sum: u32 = 0;
    let mut sum_sq: u64 = 0;
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * stride) };
        for col in 0..w {
            let v = unsafe { *s_row.add(col) } as u32;
            sum += v;
            sum_sq += (v * v) as u64;
        }
    }
    (sum, sum_sq)
}

/// Scalar reference for [`crate::sad_x4_block_u8`].
///
/// Sum of absolute differences of one `a` block against four candidate
/// `b` blocks (shared `b_stride`), returning `[sad0, sad1, sad2, sad3]`.
/// The encoder motion-search inner loop (evaluate 4 MV candidates at
/// once, reusing the `a` load).
///
/// # Safety
/// `a` and each `b{0..3}` valid for the `w×h` geometry at their strides.
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
    let bs = [b0, b1, b2, b3];
    let mut sums = [0u32; 4];
    for row in 0..h {
        let a_row = unsafe { a.offset(row as isize * a_stride) };
        for k in 0..4 {
            let b_row = unsafe { bs[k].offset(row as isize * b_stride) };
            let mut s = 0u32;
            for col in 0..w {
                let av = unsafe { *a_row.add(col) };
                let bv = unsafe { *b_row.add(col) };
                s += av.abs_diff(bv) as u32;
            }
            sums[k] += s;
        }
    }
    sums
}

/// Scalar reference for [`crate::hadamard_4x4_i16`].
///
/// 2D Walsh-Hadamard transform `Y = H·X·H` (H the 4×4 ±1 matrix,
/// `H = H^T`).  Row pass then column pass; row-major in and out.  The
/// transform is codec-agnostic (no constants).  Used inside 8×8 SATD and
/// for transform-domain analysis.
pub(crate) unsafe fn hadamard_4x4_i16(src: *const i16, dst: *mut i16) {
    let mut s = [0i16; 16];
    for i in 0..16 {
        s[i] = unsafe { *src.add(i) };
    }
    // Horizontal pass (per row).
    let mut h = [0i16; 16];
    for r in 0..4 {
        let (r0, r1, r2, r3) = (s[r * 4], s[r * 4 + 1], s[r * 4 + 2], s[r * 4 + 3]);
        let t0 = r0 + r1;
        let t1 = r0 - r1;
        let t2 = r2 + r3;
        let t3 = r2 - r3;
        h[r * 4] = t0 + t2;
        h[r * 4 + 1] = t1 + t3;
        h[r * 4 + 2] = t0 - t2;
        h[r * 4 + 3] = t1 - t3;
    }
    // Vertical pass (per column).
    for c in 0..4 {
        let (r0, r1, r2, r3) = (h[c], h[4 + c], h[8 + c], h[12 + c]);
        let t0 = r0 + r1;
        let t1 = r0 - r1;
        let t2 = r2 + r3;
        let t3 = r2 - r3;
        unsafe {
            *dst.add(c) = t0 + t2;
            *dst.add(4 + c) = t1 + t3;
            *dst.add(8 + c) = t0 - t2;
            *dst.add(12 + c) = t1 - t3;
        }
    }
}

// =========================================================================
// === High-bit-depth (u16) encoder-cost siblings (Tier-D scaffold) ===
// The u8 kernels above priced 8-bit blocks; these price 10/12/16-bit blocks
// on the u16 plane. Strides are in ELEMENTS (u16), matching the u8 kernels'
// element strides. Intermediates widen to i32 (a u16 diff and the Hadamard
// gain overflow i16); for u8-range inputs the result is byte-identical to the
// u8 kernels (proven in the tests below). Scalar-only for now: every SIMD tier
// falls through to these, per the crate's documented backfill policy.
// =========================================================================

/// Scalar reference for [`crate::sad_block_u16`].
///
/// Sum of absolute differences across a `w × h` u16 block. `sum` fits `u32`
/// (64×64 × 65535 ≈ 268M < u32::MAX).
///
/// # Safety
/// `a` valid for reads at `a + row*a_stride + col` for `row ∈ 0..h`,
/// `col ∈ 0..w` (element offsets); same for `b`.
pub(crate) unsafe fn sad_block_u16(
    a: *const u16,
    a_stride: isize,
    b: *const u16,
    b_stride: isize,
    w: usize,
    h: usize,
) -> u32 {
    let mut sum: u32 = 0;
    for row in 0..h {
        let a_row = unsafe { a.offset(row as isize * a_stride) };
        let b_row = unsafe { b.offset(row as isize * b_stride) };
        for col in 0..w {
            let av = unsafe { *a_row.add(col) };
            let bv = unsafe { *b_row.add(col) };
            sum += av.abs_diff(bv) as u32;
        }
    }
    sum
}

/// Scalar reference for [`crate::satd_block_4x4_u16`].
///
/// Hadamard-transformed SAD on a 4×4 u16 block, `(sum + 1) >> 1`. Same
/// butterfly as [`satd_block_4x4_u8`] but with `i32` intermediates (a u16
/// diff and the ×16 WHT gain overflow i16).
///
/// # Safety
/// `a` and `b` valid for `3*stride + 4` elements each.
pub(crate) unsafe fn satd_block_4x4_u16(
    a: *const u16,
    a_stride: isize,
    b: *const u16,
    b_stride: isize,
) -> u32 {
    let mut d = [0i32; 16];
    for r in 0..4isize {
        let a_row = unsafe { a.offset(r * a_stride) };
        let b_row = unsafe { b.offset(r * b_stride) };
        for c in 0..4 {
            let av = unsafe { *a_row.add(c) } as i32;
            let bv = unsafe { *b_row.add(c) } as i32;
            d[(r as usize) * 4 + c] = av - bv;
        }
    }
    let mut h = [0i32; 16];
    for r in 0..4 {
        let r0 = d[r * 4];
        let r1 = d[r * 4 + 1];
        let r2 = d[r * 4 + 2];
        let r3 = d[r * 4 + 3];
        let t0 = r0 + r1;
        let t1 = r0 - r1;
        let t2 = r2 + r3;
        let t3 = r2 - r3;
        h[r * 4] = t0 + t2;
        h[r * 4 + 1] = t1 + t3;
        h[r * 4 + 2] = t0 - t2;
        h[r * 4 + 3] = t1 - t3;
    }
    let mut v = [0i32; 16];
    for c in 0..4 {
        let r0 = h[c];
        let r1 = h[4 + c];
        let r2 = h[8 + c];
        let r3 = h[12 + c];
        let t0 = r0 + r1;
        let t1 = r0 - r1;
        let t2 = r2 + r3;
        let t3 = r2 - r3;
        v[c] = t0 + t2;
        v[4 + c] = t1 + t3;
        v[8 + c] = t0 - t2;
        v[12 + c] = t1 - t3;
    }
    let mut sum: u32 = 0;
    for i in 0..16 {
        sum += v[i].unsigned_abs();
    }
    (sum + 1) >> 1
}

/// Scalar reference for [`crate::satd_block_8x8_u16`].
///
/// Hadamard-transformed SAD on an 8×8 u16 block, `(sum + 2) >> 2`. Verbatim
/// [`satd_block_8x8_u8`] over `*const u16` (it already accumulates in `i32`).
///
/// # Safety
/// `a` and `b` valid for `7*stride + 8` elements each.
pub(crate) unsafe fn satd_block_8x8_u16(
    a: *const u16,
    a_stride: isize,
    b: *const u16,
    b_stride: isize,
) -> u32 {
    let mut d = [0i32; 64];
    for r in 0..8isize {
        let a_row = unsafe { a.offset(r * a_stride) };
        let b_row = unsafe { b.offset(r * b_stride) };
        for c in 0..8 {
            let av = unsafe { *a_row.add(c) } as i32;
            let bv = unsafe { *b_row.add(c) } as i32;
            d[(r as usize) * 8 + c] = av - bv;
        }
    }
    let mut h = [0i32; 64];
    for r in 0..8 {
        let row = [
            d[r * 8],
            d[r * 8 + 1],
            d[r * 8 + 2],
            d[r * 8 + 3],
            d[r * 8 + 4],
            d[r * 8 + 5],
            d[r * 8 + 6],
            d[r * 8 + 7],
        ];
        let out = wht8(row);
        h[r * 8..r * 8 + 8].copy_from_slice(&out);
    }
    let mut sum: u32 = 0;
    for c in 0..8 {
        let col = [
            h[c],
            h[8 + c],
            h[16 + c],
            h[24 + c],
            h[32 + c],
            h[40 + c],
            h[48 + c],
            h[56 + c],
        ];
        let out = wht8(col);
        for v in out {
            sum += v.unsigned_abs();
        }
    }
    (sum + 2) >> 2
}

/// Scalar reference for [`crate::sse_block_u16`].
///
/// Sum of squared errors across a `w × h` u16 block. `u64` accumulator (a u16
/// diff squared is ≤ 65535² ≈ 4.29e9, summed over the block, fits u64).
///
/// # Safety
/// `a`/`b` valid for `(h-1)*|stride| + w` elements each.
pub(crate) unsafe fn sse_block_u16(
    a: *const u16,
    a_stride: isize,
    b: *const u16,
    b_stride: isize,
    w: usize,
    h: usize,
) -> u64 {
    let mut sum: u64 = 0;
    for row in 0..h {
        let a_row = unsafe { a.offset(row as isize * a_stride) };
        let b_row = unsafe { b.offset(row as isize * b_stride) };
        for col in 0..w {
            let av = unsafe { *a_row.add(col) } as i32;
            let bv = unsafe { *b_row.add(col) } as i32;
            let d = av - bv;
            sum += (d * d) as u64;
        }
    }
    sum
}

#[cfg(test)]
mod u16_cost_tests {
    use super::*;

    // For u8-range inputs the u16 kernels must equal the u8 kernels byte-for-byte
    // (identical arithmetic; the wider i32 intermediates never change an in-range
    // result). A reproducible pattern exercises non-trivial diffs.
    fn u8_pat(i: usize) -> u8 {
        ((i * 37 + 11) & 0xff) as u8
    }
    fn u16_of(v: &[u8]) -> Vec<u16> {
        v.iter().map(|&x| x as u16).collect()
    }

    #[test]
    fn sad_u16_matches_u8_on_u8_range() {
        let (w, h) = (16usize, 16usize);
        let a8: Vec<u8> = (0..w * h).map(u8_pat).collect();
        let b8: Vec<u8> = (0..w * h).map(|i| u8_pat(i + 7)).collect();
        let a16 = u16_of(&a8);
        let b16 = u16_of(&b8);
        let s8 = unsafe { sad_block_u8(a8.as_ptr(), w as isize, b8.as_ptr(), w as isize, w, h) };
        let s16 =
            unsafe { sad_block_u16(a16.as_ptr(), w as isize, b16.as_ptr(), w as isize, w, h) };
        assert_eq!(s8, s16);
    }

    #[test]
    fn satd_u16_matches_u8_on_u8_range() {
        let a8: Vec<u8> = (0..64).map(u8_pat).collect();
        let b8: Vec<u8> = (0..64).map(|i| u8_pat(i + 3)).collect();
        let a16 = u16_of(&a8);
        let b16 = u16_of(&b8);
        let s8_4 = unsafe { satd_block_4x4_u8(a8.as_ptr(), 4, b8.as_ptr(), 4) };
        let s16_4 = unsafe { satd_block_4x4_u16(a16.as_ptr(), 4, b16.as_ptr(), 4) };
        assert_eq!(s8_4, s16_4, "satd 4x4");
        let s8_8 = unsafe { satd_block_8x8_u8(a8.as_ptr(), 8, b8.as_ptr(), 8) };
        let s16_8 = unsafe { satd_block_8x8_u16(a16.as_ptr(), 8, b16.as_ptr(), 8) };
        assert_eq!(s8_8, s16_8, "satd 8x8");
    }

    #[test]
    fn sad_u16_uses_full_range() {
        // A flat max-vs-zero block: 4*4 * 65535 = 1_048_560 (would overflow if the
        // kernel narrowed to u16/i16 anywhere).
        let a = [65535u16; 16];
        let b = [0u16; 16];
        let s = unsafe { sad_block_u16(a.as_ptr(), 4, b.as_ptr(), 4, 4, 4) };
        assert_eq!(s, 16 * 65535);
    }

    #[test]
    fn sse_u16_matches_u8_on_u8_range() {
        let (w, h) = (16usize, 16usize);
        let a8: Vec<u8> = (0..w * h).map(u8_pat).collect();
        let b8: Vec<u8> = (0..w * h).map(|i| u8_pat(i + 5)).collect();
        let a16 = u16_of(&a8);
        let b16 = u16_of(&b8);
        let s8 = unsafe { sse_block_u8(a8.as_ptr(), w as isize, b8.as_ptr(), w as isize, w, h) };
        let s16 =
            unsafe { sse_block_u16(a16.as_ptr(), w as isize, b16.as_ptr(), w as isize, w, h) };
        assert_eq!(s8, s16);
    }
}
