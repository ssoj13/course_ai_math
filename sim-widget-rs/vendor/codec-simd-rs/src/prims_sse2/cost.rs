#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
use core::arch::x86_64::*;

/// SSE2 impl of [`crate::sad_block_u8`].
///
/// `_mm_sad_epu8` does the whole "8 byte pairs → sum of abs diffs" in
/// one instruction, producing two u16 partial sums in the low halves of
/// each 64-bit lane.  16 bytes per row per instruction.  Tail (rows of
/// width 1..15) handled scalar.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn sad_block_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    w: usize,
    h: usize,
) -> u32 {
    unsafe {
        let mut acc = _mm_setzero_si128();
        let mut scalar_acc: u32 = 0;
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let mut col = 0usize;
            while col + 16 <= w {
                let av = _mm_loadu_si128(a_row.add(col) as *const __m128i);
                let bv = _mm_loadu_si128(b_row.add(col) as *const __m128i);
                acc = _mm_add_epi64(acc, _mm_sad_epu8(av, bv));
                col += 16;
            }
            if col + 8 <= w {
                // Load 8 bytes each into low 64 bits, upper 8 bytes zero.
                let av = _mm_loadl_epi64(a_row.add(col) as *const __m128i);
                let bv = _mm_loadl_epi64(b_row.add(col) as *const __m128i);
                acc = _mm_add_epi64(acc, _mm_sad_epu8(av, bv));
                col += 8;
            }
            while col < w {
                scalar_acc += (*a_row.add(col)).abs_diff(*b_row.add(col)) as u32;
                col += 1;
            }
        }
        // Horizontal sum of acc (low 64 + high 64).
        let hi = _mm_shuffle_epi32::<0b00_00_11_10>(acc);
        let sum = _mm_add_epi64(acc, hi);
        scalar_acc + _mm_cvtsi128_si32(sum) as u32
    }
}

/// SSE2 impl of [`crate::satd_block_4x4_u8`].
///
/// 4×4 Hadamard-transformed SAD.  Each row's 4 i16 diff values live in
/// the low 64 bits of an `__m128i`; we have 4 such vectors and operate
/// across them.
///
/// Algorithm:
///   1. Load 4 rows of u8 diff, widen to i16 (4 lanes per vec).
///   2. Stage-A butterfly (across rows): pairwise add/sub of (d0,d1)
///      and (d2,d3) — this is the vertical Hadamard, since we treat
///      each vector as a column of the transposed matrix.
///   3. Stage-B butterfly (across rows): pairwise add/sub of the
///      stage-A outputs.
///   4. In-register 4×4 transpose (the classic SSE2 unpack pattern).
///   5. Repeat stages A+B on the transposed rows.
///   6. abs each (via max(v, sat_negate(v))) and sum.
///
/// Output: `(sum + 1) >> 1` per the HEVC/ffmpeg convention.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn satd_block_4x4_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
) -> u32 {
    unsafe {
        let zero = _mm_setzero_si128();

        // Helper: load 4 bytes from a row, widen u8→i16 in low 64 bits.
        let load_diff = |a_row: *const u8, b_row: *const u8| -> __m128i {
            // read_unaligned: row pointers carry arbitrary strides (no 4-byte align).
            let av = _mm_cvtsi32_si128((a_row as *const i32).read_unaligned());
            let bv = _mm_cvtsi32_si128((b_row as *const i32).read_unaligned());
            let a16 = _mm_unpacklo_epi8(av, zero);
            let b16 = _mm_unpacklo_epi8(bv, zero);
            _mm_sub_epi16(a16, b16)
        };

        // Load 4 diff rows (low 64 bits of each xmm).
        let d0 = load_diff(a, b);
        let d1 = load_diff(a.offset(a_stride), b.offset(b_stride));
        let d2 = load_diff(a.offset(2 * a_stride), b.offset(2 * b_stride));
        let d3 = load_diff(a.offset(3 * a_stride), b.offset(3 * b_stride));

        // --- First 1D Hadamard (treats each vector as a column) ---
        // Stage A: pair (d0,d1) and (d2,d3).
        let a0 = _mm_add_epi16(d0, d1);
        let a1 = _mm_sub_epi16(d0, d1);
        let a2 = _mm_add_epi16(d2, d3);
        let a3 = _mm_sub_epi16(d2, d3);
        // Stage B: pair (a0,a2) and (a1,a3).
        let b0 = _mm_add_epi16(a0, a2);
        let b1 = _mm_add_epi16(a1, a3);
        let b2 = _mm_sub_epi16(a0, a2);
        let b3 = _mm_sub_epi16(a1, a3);

        // --- Transpose 4×4 (only low 4 lanes of each vec matter) ---
        // Standard SSE2 transpose: unpack 16-bit then unpack 32-bit pairs.
        let t01 = _mm_unpacklo_epi16(b0, b1); // [b0_0, b1_0, b0_1, b1_1, b0_2, b1_2, b0_3, b1_3]
        let t23 = _mm_unpacklo_epi16(b2, b3); // [b2_0, b3_0, b2_1, b3_1, b2_2, b3_2, b2_3, b3_3]
        let r0 = _mm_unpacklo_epi32(t01, t23); // row 0: [b0_0, b1_0, b2_0, b3_0, b0_1, b1_1, b2_1, b3_1]
        let r1 = _mm_unpackhi_epi32(t01, t23); // row 2-ish
        // Note: rows we want in low 64 bits.
        let row0 = r0;
        let row1 = _mm_shuffle_epi32::<0b11_10_11_10>(r0); // get row 1 from high 64 of r0
        let row2 = r1;
        let row3 = _mm_shuffle_epi32::<0b11_10_11_10>(r1);

        // --- Second 1D Hadamard (acts as horizontal on original) ---
        let a0 = _mm_add_epi16(row0, row1);
        let a1 = _mm_sub_epi16(row0, row1);
        let a2 = _mm_add_epi16(row2, row3);
        let a3 = _mm_sub_epi16(row2, row3);
        let b0 = _mm_add_epi16(a0, a2);
        let b1 = _mm_add_epi16(a1, a3);
        let b2 = _mm_sub_epi16(a0, a2);
        let b3 = _mm_sub_epi16(a1, a3);

        // --- Abs each vector (low 64 bits hold valid lanes) ---
        // SSE2 abs via max(v, sat_neg(v)).
        let abs = |v: __m128i| -> __m128i { _mm_max_epi16(v, _mm_subs_epi16(zero, v)) };
        let ab0 = abs(b0);
        let ab1 = abs(b1);
        let ab2 = abs(b2);
        let ab3 = abs(b3);

        // --- Horizontal sum of all 16 lanes ---
        // Sum 4 vectors of 4 valid i16 lanes (in low 64 bits each).
        // Add pairwise: s01 = ab0 + ab1, s23 = ab2 + ab3.
        let s01 = _mm_add_epi16(ab0, ab1);
        let s23 = _mm_add_epi16(ab2, ab3);
        let s = _mm_add_epi16(s01, s23); // 4 i16 partial sums in low 64.
        // Now sum the 4 i16 lanes.  Widen to i32 (sign-extend not needed
        // since values are non-negative absolutes), then horizontal add.
        let s_lo = _mm_unpacklo_epi16(s, zero); // 4 × i32 in 128 bits
        // Sum the 4 i32 lanes.
        let hi = _mm_shuffle_epi32::<0b00_00_11_10>(s_lo);
        let s2 = _mm_add_epi32(s_lo, hi);
        let hi2 = _mm_shuffle_epi32::<0b00_00_00_01>(s2);
        let total = _mm_add_epi32(s2, hi2);
        let sum = _mm_cvtsi128_si32(total) as u32;
        (sum + 1) >> 1
    }
}

/// SSE2 impl of [`crate::sse_block_u8`].
///
/// `_mm_madd_epi16(diff, diff)` does i16² pair-sums into i32 lanes —
/// effectively `(d0² + d1², d2² + d3², d4² + d5², d6² + d7²)`.  16
/// bytes/row split into low/high i16 halves processed in parallel.
/// i32 accumulator drained to u64 per row to handle large blocks.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn sse_block_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    w: usize,
    h: usize,
) -> u64 {
    unsafe {
        let zero = _mm_setzero_si128();
        let mut sum: u64 = 0;
        for row in 0..h {
            let a_row = a.offset(row as isize * a_stride);
            let b_row = b.offset(row as isize * b_stride);
            let mut acc = _mm_setzero_si128(); // 4 × i32, holds this row's sum
            let mut col = 0usize;
            while col + 16 <= w {
                let av = _mm_loadu_si128(a_row.add(col) as *const __m128i);
                let bv = _mm_loadu_si128(b_row.add(col) as *const __m128i);
                let a_lo = _mm_unpacklo_epi8(av, zero);
                let a_hi = _mm_unpackhi_epi8(av, zero);
                let b_lo = _mm_unpacklo_epi8(bv, zero);
                let b_hi = _mm_unpackhi_epi8(bv, zero);
                let d_lo = _mm_sub_epi16(a_lo, b_lo);
                let d_hi = _mm_sub_epi16(a_hi, b_hi);
                acc = _mm_add_epi32(acc, _mm_madd_epi16(d_lo, d_lo));
                acc = _mm_add_epi32(acc, _mm_madd_epi16(d_hi, d_hi));
                col += 16;
            }
            if col + 8 <= w {
                let av = _mm_loadl_epi64(a_row.add(col) as *const __m128i);
                let bv = _mm_loadl_epi64(b_row.add(col) as *const __m128i);
                let a16 = _mm_unpacklo_epi8(av, zero);
                let b16 = _mm_unpacklo_epi8(bv, zero);
                let d = _mm_sub_epi16(a16, b16);
                acc = _mm_add_epi32(acc, _mm_madd_epi16(d, d));
                col += 8;
            }
            // Horizontal sum of acc (4 × i32 → u32).
            let hi = _mm_shuffle_epi32::<0b00_00_11_10>(acc);
            let s = _mm_add_epi32(acc, hi);
            let hi2 = _mm_shuffle_epi32::<0b00_00_00_01>(s);
            let total = _mm_add_epi32(s, hi2);
            sum += _mm_cvtsi128_si32(total) as u32 as u64;
            // Scalar tail per-row.
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

/// SSE2 impl of [`crate::variance_block_u8`].
///
/// `sum` via `_mm_sad_epu8(v, zero)` (byte-sum into u64 lanes; whole-block
/// accumulation is safe because 128×128×255 < u32::MAX).  `sum_sq` via
/// widen u8→i16 (`unpack` with zero) then `_mm_madd_epi16(v, v)` (i16²
/// pair-sums into i32 lanes), drained to u64 once per row to keep the
/// i32 accumulator from overflowing.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn variance_block_u8(
    src: *const u8,
    stride: isize,
    w: usize,
    h: usize,
) -> (u32, u64) {
    unsafe {
        let zero = _mm_setzero_si128();
        let mut sum_acc = _mm_setzero_si128(); // 2 × u64 byte-sum partials
        let mut sum_sq: u64 = 0;
        let mut sum_scalar: u32 = 0;
        for row in 0..h {
            let s_row = src.offset(row as isize * stride);
            let mut sq_acc = _mm_setzero_si128(); // 4 × i32, this row's Σv²
            let mut col = 0usize;
            while col + 16 <= w {
                let v = _mm_loadu_si128(s_row.add(col) as *const __m128i);
                sum_acc = _mm_add_epi64(sum_acc, _mm_sad_epu8(v, zero));
                let lo = _mm_unpacklo_epi8(v, zero);
                let hi = _mm_unpackhi_epi8(v, zero);
                sq_acc = _mm_add_epi32(sq_acc, _mm_madd_epi16(lo, lo));
                sq_acc = _mm_add_epi32(sq_acc, _mm_madd_epi16(hi, hi));
                col += 16;
            }
            if col + 8 <= w {
                let v = _mm_loadl_epi64(s_row.add(col) as *const __m128i);
                sum_acc = _mm_add_epi64(sum_acc, _mm_sad_epu8(v, zero));
                let lo = _mm_unpacklo_epi8(v, zero);
                sq_acc = _mm_add_epi32(sq_acc, _mm_madd_epi16(lo, lo));
                col += 8;
            }
            // Drain sq_acc (4 × i32) → u64.
            let hi = _mm_shuffle_epi32::<0b00_00_11_10>(sq_acc);
            let s = _mm_add_epi32(sq_acc, hi);
            let hi2 = _mm_shuffle_epi32::<0b00_00_00_01>(s);
            let total = _mm_add_epi32(s, hi2);
            sum_sq += _mm_cvtsi128_si32(total) as u32 as u64;
            // Scalar tail (both accumulators).
            while col < w {
                let vv = *s_row.add(col) as u32;
                sum_scalar += vv;
                sum_sq += (vv * vv) as u64;
                col += 1;
            }
        }
        // Horizontal sum of sum_acc (low 64 + high 64).
        let hi = _mm_shuffle_epi32::<0b00_00_11_10>(sum_acc);
        let s = _mm_add_epi64(sum_acc, hi);
        let sum = sum_scalar + _mm_cvtsi128_si32(s) as u32;
        (sum, sum_sq)
    }
}

/// SSE2 impl of [`crate::sad_x4_block_u8`].  Loads each `a` chunk once
/// and runs `_mm_sad_epu8` against all four candidates.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
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
        let mut acc = [_mm_setzero_si128(); 4];
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
                let av = _mm_loadu_si128(a_row.add(col) as *const __m128i);
                for k in 0..4 {
                    let bv = _mm_loadu_si128(br[k].add(col) as *const __m128i);
                    acc[k] = _mm_add_epi64(acc[k], _mm_sad_epu8(av, bv));
                }
                col += 16;
            }
            if col + 8 <= w {
                let av = _mm_loadl_epi64(a_row.add(col) as *const __m128i);
                for k in 0..4 {
                    let bv = _mm_loadl_epi64(br[k].add(col) as *const __m128i);
                    acc[k] = _mm_add_epi64(acc[k], _mm_sad_epu8(av, bv));
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
            let hi = _mm_shuffle_epi32::<0b00_00_11_10>(acc[k]);
            let sum = _mm_add_epi64(acc[k], hi);
            out[k] = scalar[k] + _mm_cvtsi128_si32(sum) as u32;
        }
        out
    }
}

/// SSE2 impl of [`crate::hadamard_4x4_i16`].  Each of the 4 rows lives in
/// the low 64 bits of an `__m128i`; the column (vertical) butterfly is
/// register add/sub, the row butterfly is the same after an in-register
/// 4×4 transpose, with a final transpose to restore row-major order.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
#[target_feature(enable = "sse2")]
pub(crate) unsafe fn hadamard_4x4_i16(src: *const i16, dst: *mut i16) {
    unsafe {
        // Vertical butterfly across 4 row-registers.
        let vb = |r0, r1, r2, r3| -> (__m128i, __m128i, __m128i, __m128i) {
            let a0 = _mm_add_epi16(r0, r1);
            let a1 = _mm_sub_epi16(r0, r1);
            let a2 = _mm_add_epi16(r2, r3);
            let a3 = _mm_sub_epi16(r2, r3);
            (
                _mm_add_epi16(a0, a2),
                _mm_add_epi16(a1, a3),
                _mm_sub_epi16(a0, a2),
                _mm_sub_epi16(a1, a3),
            )
        };
        // In-register transpose of 4 rows (valid data in low 64 of each).
        let transpose = |b0, b1, b2, b3| -> (__m128i, __m128i, __m128i, __m128i) {
            let t01 = _mm_unpacklo_epi16(b0, b1);
            let t23 = _mm_unpacklo_epi16(b2, b3);
            let r0 = _mm_unpacklo_epi32(t01, t23);
            let r1 = _mm_unpackhi_epi32(t01, t23);
            (
                r0,
                _mm_shuffle_epi32::<0b11_10_11_10>(r0),
                r1,
                _mm_shuffle_epi32::<0b11_10_11_10>(r1),
            )
        };
        let d0 = _mm_loadl_epi64(src as *const __m128i);
        let d1 = _mm_loadl_epi64(src.add(4) as *const __m128i);
        let d2 = _mm_loadl_epi64(src.add(8) as *const __m128i);
        let d3 = _mm_loadl_epi64(src.add(12) as *const __m128i);
        let (m0, m1, m2, m3) = vb(d0, d1, d2, d3); // H·X
        let (mt0, mt1, mt2, mt3) = transpose(m0, m1, m2, m3);
        let (n0, n1, n2, n3) = vb(mt0, mt1, mt2, mt3); // H·(H·X)^T
        let (y0, y1, y2, y3) = transpose(n0, n1, n2, n3); // → H·X·H
        _mm_storel_epi64(dst as *mut __m128i, y0);
        _mm_storel_epi64(dst.add(4) as *mut __m128i, y1);
        _mm_storel_epi64(dst.add(8) as *mut __m128i, y2);
        _mm_storel_epi64(dst.add(12) as *mut __m128i, y3);
    }
}
