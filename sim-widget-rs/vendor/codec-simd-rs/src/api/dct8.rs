//! 8x8 integer inverse DCT kernels shared by every codec in the workspace (pure safe Rust, scalar).
//!
//! Two byte-exact families, each with ONE implementation:
//!
//! * [`SimpleIdct`]: FFmpeg's `simple_idct` (`libavcodec/simple_idct_template.c`, row/column codelets
//!   `idctRowCondDC` + `idctSparseCol`/`idctSparseColPut`). Used by the JPEG decoder (`mjpegdec` selects
//!   `ff_simple_idct_put_int16_{8,10,12}bit`) and, with a fused dequantiser, by ProRes
//!   (`proresdsp.c:56-80`). The instances differ only in the cosine constants and the three shifts.
//! * [`idct_islow`]: libjpeg(-turbo) `jpeg_idct_islow` (`jidctint.c`), the 8-bit and 12-bit units.
//!
//! Everything runs in the reference's integer arithmetic: the simple_idct accumulators are C `SUINT`
//! (wrapping `u32`) with a final signed `>>`; islow works in C `long`, which is 32 bits on the
//! reference platform (Windows), so it wraps in `i32` (only reachable with out-of-spec coefficients).

/// Output sample of a put kernel (`u8` for 8-bit, `u16` above).
pub trait Pixel: Copy + sealed::Sealed {
    /// Narrow an already range-limited sample.
    fn from_i32(v: i32) -> Self;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u16 {}
}

impl Pixel for u8 {
    #[inline(always)]
    fn from_i32(v: i32) -> Self {
        v as u8
    }
}

impl Pixel for u16 {
    #[inline(always)]
    fn from_i32(v: i32) -> Self {
        v as u16
    }
}

/// `MUL(w, x)` / one `MAC` term of the template: wrapping `u32` product (`(SUINT)(w) * (x)`); a negative
/// constant contributes its two's-complement pattern exactly like `(SUINT)(-W)`.
#[inline(always)]
fn mul(w: i32, x: i32) -> u32 {
    (w as u32).wrapping_mul(x as u32)
}

/// Constants of one `simple_idct_template.c` instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleIdct {
    /// `W1..W7` (`w[0]` = W1). `W4` = `w[3]`.
    w: [i32; 7],
    /// `ROW_SHIFT`, excluding the caller-supplied `extra_shift`.
    row_shift: u32,
    /// `COL_SHIFT`.
    col_shift: u32,
    /// `DC_SHIFT` of the DC-only row fast path (negative for the 12-bit instance).
    dc_shift: i32,
}

/// `BIT_DEPTH == 8` (`simple_idct_template.c:46-60`): `ff_simple_idct_put_int16_8bit`.
pub const SIMPLE_IDCT_8: SimpleIdct = SimpleIdct {
    w: [22725, 21407, 19266, 16383, 12873, 8867, 4520],
    row_shift: 11,
    col_shift: 20,
    dc_shift: 3,
};

/// `BIT_DEPTH == 10`, `IN_IDCT_DEPTH == 16` (`:62-78`): `ff_simple_idct_put_int16_10bit`.
pub const SIMPLE_IDCT_10: SimpleIdct = SimpleIdct {
    w: [22725, 21407, 19265, 16384, 12873, 8867, 4520],
    row_shift: 12,
    col_shift: 19,
    dc_shift: 2,
};

/// `BIT_DEPTH == 10` with `EXTRA_SHIFT` (`:67-71`): the ProRes 10-bit transform (`extra_shift = 2`).
pub const SIMPLE_IDCT_PRORES_10: SimpleIdct = SimpleIdct {
    w: [22725, 21407, 19265, 16384, 12873, 8867, 4520],
    row_shift: 13,
    col_shift: 18,
    dc_shift: 1,
};

/// `BIT_DEPTH == 12` (`:80-91`): `ff_simple_idct_put_int16_12bit` and the ProRes 12-bit transform.
pub const SIMPLE_IDCT_12: SimpleIdct = SimpleIdct {
    w: [45451, 42813, 38531, 32767, 25746, 17734, 9041],
    row_shift: 16,
    col_shift: 17,
    dc_shift: -1,
};

impl SimpleIdct {
    /// `idctRowCondDC` (`simple_idct_template.c:112-207`) in place on one 8-coefficient row. The row
    /// result is stored back as `int16` (C `row[k] = (int)(...) >> shift`, truncating).
    ///
    /// The DC-only fast path is part of the reference result, not an optimisation: for the 12-bit
    /// instance `W4 = 32767` is not a power of two, so `(row0 + 1) >> 1` differs from the general
    /// path on odd `row0` (see the ProRes regression KATs).
    #[inline(always)]
    pub fn row(&self, row: &mut [i16], extra_shift: u32) {
        let row: &mut [i16; 8] = row.try_into().expect("row of 8");
        let [w1, w2, w3, w4, w5, w6, w7] = self.w;
        let r = row.map(i32::from);
        if (r[1] | r[2] | r[3] | r[4] | r[5] | r[6] | r[7]) == 0 {
            let extra = extra_shift as i32;
            let t = if self.dc_shift - extra >= 0 {
                r[0] << (self.dc_shift - extra)
            } else {
                let s = extra - self.dc_shift;
                (r[0] + (1 << (s - 1))) >> s
            };
            row.fill((t & 0xffff) as i16);
            return;
        }
        let rs = self.row_shift + extra_shift;
        let mut a0 = mul(w4, r[0]).wrapping_add(1u32 << (rs - 1));
        let mut a1 = a0;
        let mut a2 = a0;
        let mut a3 = a0;
        a0 = a0.wrapping_add(mul(w2, r[2]));
        a1 = a1.wrapping_add(mul(w6, r[2]));
        a2 = a2.wrapping_sub(mul(w6, r[2]));
        a3 = a3.wrapping_sub(mul(w2, r[2]));
        let mut b0 = mul(w1, r[1]).wrapping_add(mul(w3, r[3]));
        let mut b1 = mul(w3, r[1]).wrapping_sub(mul(w7, r[3]));
        let mut b2 = mul(w5, r[1]).wrapping_sub(mul(w1, r[3]));
        let mut b3 = mul(w7, r[1]).wrapping_sub(mul(w5, r[3]));
        // The C code skips these terms when row[4..8] == 0; they are then `+ 0`, so always adding is exact.
        a0 = a0.wrapping_add(mul(w4, r[4])).wrapping_add(mul(w6, r[6]));
        a1 = a1.wrapping_sub(mul(w4, r[4])).wrapping_sub(mul(w2, r[6]));
        a2 = a2.wrapping_sub(mul(w4, r[4])).wrapping_add(mul(w2, r[6]));
        a3 = a3.wrapping_add(mul(w4, r[4])).wrapping_sub(mul(w6, r[6]));
        b0 = b0.wrapping_add(mul(w5, r[5])).wrapping_add(mul(w7, r[7]));
        b1 = b1.wrapping_sub(mul(w1, r[5])).wrapping_sub(mul(w5, r[7]));
        b2 = b2.wrapping_add(mul(w7, r[5])).wrapping_add(mul(w3, r[7]));
        b3 = b3.wrapping_add(mul(w3, r[5])).wrapping_sub(mul(w1, r[7]));
        row[0] = ((a0.wrapping_add(b0) as i32) >> rs) as i16;
        row[7] = ((a0.wrapping_sub(b0) as i32) >> rs) as i16;
        row[1] = ((a1.wrapping_add(b1) as i32) >> rs) as i16;
        row[6] = ((a1.wrapping_sub(b1) as i32) >> rs) as i16;
        row[2] = ((a2.wrapping_add(b2) as i32) >> rs) as i16;
        row[5] = ((a2.wrapping_sub(b2) as i32) >> rs) as i16;
        row[3] = ((a3.wrapping_add(b3) as i32) >> rs) as i16;
        row[4] = ((a3.wrapping_sub(b3) as i32) >> rs) as i16;
    }

    /// `IDCT_COLS` (`:210-258`): the eight column outputs `(a0+b0, a1+b1, a2+b2, a3+b3, a3-b3, a2-b2,
    /// a1-b1, a0-b0)` as raw `SUINT` sums for column `c` of `block` (`col[8*k]`), not yet shifted.
    #[inline(always)]
    fn col_sums(&self, block: &[i16; 64], c: usize) -> [u32; 8] {
        let [w1, w2, w3, w4, w5, w6, w7] = self.w;
        let x = |k: usize| i32::from(block[8 * k + c]);
        let (c0, c1, c2, c3, c4, c5, c6, c7) = (x(0), x(1), x(2), x(3), x(4), x(5), x(6), x(7));
        let rnd = (1i32 << (self.col_shift - 1)) / w4;
        let mut a0 = mul(w4, c0.wrapping_add(rnd));
        let mut a1 = a0;
        let mut a2 = a0;
        let mut a3 = a0;
        a0 = a0.wrapping_add(mul(w2, c2));
        a1 = a1.wrapping_add(mul(w6, c2));
        a2 = a2.wrapping_sub(mul(w6, c2));
        a3 = a3.wrapping_sub(mul(w2, c2));
        let mut b0 = mul(w1, c1).wrapping_add(mul(w3, c3));
        let mut b1 = mul(w3, c1).wrapping_sub(mul(w7, c3));
        let mut b2 = mul(w5, c1).wrapping_sub(mul(w1, c3));
        let mut b3 = mul(w7, c1).wrapping_sub(mul(w5, c3));
        // `if (col[8*k])` guards in C skip `+ 0` terms: always adding is exact.
        a0 = a0.wrapping_add(mul(w4, c4));
        a1 = a1.wrapping_sub(mul(w4, c4));
        a2 = a2.wrapping_sub(mul(w4, c4));
        a3 = a3.wrapping_add(mul(w4, c4));
        b0 = b0.wrapping_add(mul(w5, c5));
        b1 = b1.wrapping_sub(mul(w1, c5));
        b2 = b2.wrapping_add(mul(w7, c5));
        b3 = b3.wrapping_add(mul(w3, c5));
        a0 = a0.wrapping_add(mul(w6, c6));
        a1 = a1.wrapping_sub(mul(w2, c6));
        a2 = a2.wrapping_add(mul(w2, c6));
        a3 = a3.wrapping_sub(mul(w6, c6));
        b0 = b0.wrapping_add(mul(w7, c7));
        b1 = b1.wrapping_sub(mul(w5, c7));
        b2 = b2.wrapping_add(mul(w3, c7));
        b3 = b3.wrapping_sub(mul(w1, c7));
        [
            a0.wrapping_add(b0),
            a1.wrapping_add(b1),
            a2.wrapping_add(b2),
            a3.wrapping_add(b3),
            a3.wrapping_sub(b3),
            a2.wrapping_sub(b2),
            a1.wrapping_sub(b1),
            a0.wrapping_sub(b0),
        ]
    }

    /// `idctSparseCol` (`:262-282`): column `c` transformed in place, each result truncated to `int16`.
    #[inline(always)]
    pub fn col_store(&self, block: &mut [i16; 64], c: usize) {
        let s = self.col_sums(block, c);
        for (k, v) in s.iter().enumerate() {
            block[8 * k + c] = ((*v as i32) >> self.col_shift) as i16;
        }
    }

    /// Full `ff_simple_idct_put_int16_NNbit`: rows (in place, `int16`), then columns clipped to
    /// `0..=(1 << bits) - 1` (`idctSparseColPut`, `av_clip_pixel` of the unshifted-to-int16 sums) and
    /// written to `dst` (`stride` in samples). `block` is destroyed.
    ///
    /// # Panics
    /// If `dst.len() < 7 * stride + 8`.
    pub fn put<P: Pixel>(&self, dst: &mut [P], stride: usize, block: &mut [i16; 64], bits: u32) {
        assert!(
            dst.len() >= 7 * stride + 8,
            "codec-simd: SimpleIdct::put destination too small"
        );
        let max = (1i32 << bits) - 1;
        for r in 0..8 {
            self.row(&mut block[8 * r..8 * r + 8], 0);
        }
        for c in 0..8 {
            let s = self.col_sums(block, c);
            for (k, v) in s.iter().enumerate() {
                dst[k * stride + c] = P::from_i32(((*v as i32) >> self.col_shift).clamp(0, max));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// libjpeg islow
// ---------------------------------------------------------------------------------------------

const CONST_BITS: u32 = 13;
const FIX_0_298631336: i32 = 2446;
const FIX_0_390180644: i32 = 3196;
const FIX_0_541196100: i32 = 4433;
const FIX_0_765366865: i32 = 6270;
const FIX_0_899976223: i32 = 7373;
const FIX_1_175875602: i32 = 9633;
const FIX_1_501321110: i32 = 12299;
const FIX_1_847759065: i32 = 15137;
const FIX_1_961570560: i32 = 16069;
const FIX_2_053119869: i32 = 16819;
const FIX_2_562915447: i32 = 20995;
const FIX_3_072711026: i32 = 25172;

#[inline(always)]
fn m(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b)
}

#[inline(always)]
fn ad(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

#[inline(always)]
fn sb(a: i32, b: i32) -> i32 {
    a.wrapping_sub(b)
}

/// `DESCALE(x, n) = (x + (1 << (n - 1))) >> n`.
#[inline(always)]
fn descale(x: i32, n: u32) -> i32 {
    ad(x, 1 << (n - 1)) >> n
}

/// One 1-D islow butterfly on eight (already dequantised) inputs `i0..i7`: returns the eight
/// pre-descale outputs `tmp10+tmp3, tmp11+tmp2, tmp12+tmp1, tmp13+tmp0, tmp13-tmp0, tmp12-tmp1,
/// tmp11-tmp2, tmp10-tmp3` in output order (`jidctint.c:222-284`).
#[inline(always)]
fn islow_1d(i: [i32; 8]) -> [i32; 8] {
    let (mut z2, mut z3) = (i[2], i[6]);
    let mut z1 = m(ad(z2, z3), FIX_0_541196100);
    let mut tmp2 = ad(z1, m(z3, -FIX_1_847759065));
    let mut tmp3 = ad(z1, m(z2, FIX_0_765366865));
    z2 = i[0];
    z3 = i[4];
    let mut tmp0 = ad(z2, z3).wrapping_shl(CONST_BITS);
    let mut tmp1 = sb(z2, z3).wrapping_shl(CONST_BITS);
    let tmp10 = ad(tmp0, tmp3);
    let tmp13 = sb(tmp0, tmp3);
    let tmp11 = ad(tmp1, tmp2);
    let tmp12 = sb(tmp1, tmp2);

    tmp0 = i[7];
    tmp1 = i[5];
    tmp2 = i[3];
    tmp3 = i[1];
    z1 = ad(tmp0, tmp3);
    z2 = ad(tmp1, tmp2);
    z3 = ad(tmp0, tmp2);
    let mut z4 = ad(tmp1, tmp3);
    let z5 = m(ad(z3, z4), FIX_1_175875602);
    tmp0 = m(tmp0, FIX_0_298631336);
    tmp1 = m(tmp1, FIX_2_053119869);
    tmp2 = m(tmp2, FIX_3_072711026);
    tmp3 = m(tmp3, FIX_1_501321110);
    z1 = m(z1, -FIX_0_899976223);
    z2 = m(z2, -FIX_2_562915447);
    z3 = m(z3, -FIX_1_961570560);
    z4 = m(z4, -FIX_0_390180644);
    z3 = ad(z3, z5);
    z4 = ad(z4, z5);
    tmp0 = ad(tmp0, ad(z1, z3));
    tmp1 = ad(tmp1, ad(z2, z4));
    tmp2 = ad(tmp2, ad(z2, z3));
    tmp3 = ad(tmp3, ad(z1, z4));
    [
        ad(tmp10, tmp3),
        ad(tmp11, tmp2),
        ad(tmp12, tmp1),
        ad(tmp13, tmp0),
        sb(tmp13, tmp0),
        sb(tmp12, tmp1),
        sb(tmp11, tmp2),
        sb(tmp10, tmp3),
    ]
}

/// `range_limit[x & RANGE_MASK]` of the post-IDCT table (`jdmaster.c:prepare_range_limit_table`):
/// level-shifts by `CENTERJSAMPLE`, clamps to `0..=MAXJSAMPLE` and wraps modulo `4 * (MAXJSAMPLE + 1)`
/// exactly like the C lookup does for far out-of-range values.
#[inline(always)]
fn range_limit(x: i32, bits: u32) -> i32 {
    let max = (1i32 << bits) - 1;
    let center = 1i32 << (bits - 1);
    let i = x & (4 * (max + 1) - 1);
    if i < center {
        center + i
    } else if i < 2 * (max + 1) {
        max
    } else if i < 4 * (max + 1) - center {
        0
    } else {
        i - (4 * (max + 1) - center)
    }
}

/// `jpeg_idct_islow` (`jidctint.c:171-349`): dequantise `coef` by `quant` (both natural order), inverse
/// transform (columns, then rows) and write the range-limited samples to `dst` (`stride` in samples).
///
/// `bits` selects the unit: 8 -> `PASS1_BITS = 2`, 12 -> `PASS1_BITS = 1` (`jidctint.c:101-108`). The
/// caller prepares `quant` as the C multiplier type does: truncated to `short` for the 8-bit unit of a
/// SIMD build, full `int` for the 12-bit unit.
///
/// # Panics
/// If `bits` is not 8 or 12, or `dst.len() < 7 * stride + 8`.
pub fn idct_islow<P: Pixel>(
    coef: &[i16; 64],
    quant: &[i32; 64],
    dst: &mut [P],
    stride: usize,
    bits: u32,
) {
    assert!(
        bits == 8 || bits == 12,
        "codec-simd: islow supports 8 and 12 bit"
    );
    assert!(
        dst.len() >= 7 * stride + 8,
        "codec-simd: idct_islow destination too small"
    );
    let pass1 = if bits == 8 { 2 } else { 1 };
    let mut ws = [0i32; 64];
    for c in 0..8 {
        let dq = |k: usize| m(i32::from(coef[8 * k + c]), quant[8 * k + c]);
        if (1..8).all(|k| coef[8 * k + c] == 0) {
            let dc = dq(0).wrapping_shl(pass1);
            for k in 0..8 {
                ws[8 * k + c] = dc;
            }
            continue;
        }
        let o = islow_1d([dq(0), dq(1), dq(2), dq(3), dq(4), dq(5), dq(6), dq(7)]);
        // Output order of `islow_1d` is rows 0,1,2,3,4,5,6,7 of this column.
        for k in 0..8 {
            ws[8 * k + c] = descale(o[k], CONST_BITS - pass1);
        }
    }
    for r in 0..8 {
        let w = &ws[8 * r..8 * r + 8];
        let out = &mut dst[r * stride..r * stride + 8];
        if w[1..].iter().all(|&v| v == 0) {
            let dc = range_limit(descale(w[0], pass1 + 3), bits);
            out.fill(P::from_i32(dc));
            continue;
        }
        let o = islow_1d([w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7]]);
        for k in 0..8 {
            out[k] = P::from_i32(range_limit(descale(o[k], CONST_BITS + pass1 + 3), bits));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DC-only blocks are flat; the 8-bit zero block decodes to the neutral level 128.
    #[test]
    fn islow_flat_blocks() {
        let mut dst = [0u8; 64];
        idct_islow(&[0i16; 64], &[1i32; 64], &mut dst, 8, 8);
        assert!(dst.iter().all(|&v| v == 128));
        let mut c = [0i16; 64];
        c[0] = 80; // +80/8 = +10 over the centre
        idct_islow(&c, &[1i32; 64], &mut dst, 8, 8);
        assert!(dst.iter().all(|&v| v == 138));
        let mut d12 = [0u16; 64];
        idct_islow(&[0i16; 64], &[1i32; 64], &mut d12, 8, 12);
        assert!(d12.iter().all(|&v| v == 2048));
    }

    /// The 8-bit simple_idct fed the level-shift bias `4 << bits` (what the JPEG DC predictor starts
    /// from) decodes a zero block to mid-grey.
    #[test]
    fn simple_idct_neutral_with_dc_bias() {
        for (cfg, bits) in [
            (SIMPLE_IDCT_8, 8u32),
            (SIMPLE_IDCT_10, 10),
            (SIMPLE_IDCT_12, 12),
        ] {
            let mut b = [0i16; 64];
            b[0] = (4i32 << bits) as i16;
            let mut d = [0u16; 64];
            cfg.put(&mut d, 8, &mut b, bits);
            assert!(
                d.iter().all(|&v| v == 1 << (bits - 1)),
                "bits {bits}: {:?}",
                &d[..8]
            );
        }
    }

    #[test]
    fn range_limit_wraps_like_the_c_table() {
        assert_eq!(range_limit(0, 8), 128);
        assert_eq!(range_limit(127, 8), 255);
        assert_eq!(range_limit(128, 8), 255);
        assert_eq!(range_limit(-128, 8), 0);
        assert_eq!(range_limit(-129, 8), 0);
        assert_eq!(range_limit(1023 - 127, 8), 0); // far below: still the low clamp
        assert_eq!(range_limit(1024, 8), 128); // wraps modulo 4 * 256
    }
}
