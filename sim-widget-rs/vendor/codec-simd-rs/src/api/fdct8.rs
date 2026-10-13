//! 8x8 integer FORWARD DCT kernels shared by every codec in the workspace (pure safe Rust, scalar).
//!
//! Two byte-exact families, each with ONE generic implementation:
//!
//! * [`fdct_islow`]: the IJG "slow but accurate" LL&M DCT. FFmpeg's `jfdctint_template.c`
//!   (`ff_jpeg_fdct_islow_8` / `_10`, the encoder default of mjpeg and ProRes) and libjpeg(-turbo)'s `jfdctint.c`
//!   (`jpeg_fdct_islow`, the 8-bit SIMD-layout unit and the 12-bit unit) are the same algorithm; they differ only in
//!   three constants and the storage type of the in-place work array, which are the generic parameters.
//! * [`fdct_ifast`]: FFmpeg's `ff_fdct_ifast` (`jfdctfst.c`, the AAN kernel behind `-dct fastint`).
//!
//! Everything runs in the reference's 32-bit integer arithmetic. C keeps the in-place work array in `int16_t`
//! (FFmpeg, libjpeg's `DCTELEM = short` 8-bit unit) or `int` (libjpeg's 12-bit unit), so the pass-1 results are
//! truncated to the element type between the passes; `z*`/`MULTIPLY` operands wrap exactly like the two's-complement
//! machine arithmetic the reference runs on (FFmpeg's `(int)((var) * (unsigned)(const))` and libjpeg's plain
//! `(var) * (const)` are the same wrapping 32-bit product, so no separate multiply form is needed).
//!
//! Inputs are level-shifted samples (`sample - 2^(bits-1)`), the outputs are the DCT scaled up by 8, as in C.

/// Element type of the in-place work array: `i16` (`int16_t`) or `i32` (`int`).
pub trait DctElem: Copy + sealed::Sealed {
    /// Widen to the `int` the kernel computes in.
    fn get(self) -> i32;
    /// Store an `int` result with C's truncating conversion to the element type.
    fn put(v: i32) -> Self;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for i16 {}
    impl Sealed for i32 {}
}

impl DctElem for i16 {
    #[inline(always)]
    fn get(self) -> i32 {
        i32::from(self)
    }
    #[inline(always)]
    fn put(v: i32) -> Self {
        v as i16
    }
}

impl DctElem for i32 {
    #[inline(always)]
    fn get(self) -> i32 {
        self
    }
    #[inline(always)]
    fn put(v: i32) -> Self {
        v
    }
}

// `CONST_BITS == 13` constants (`jfdctint_template.c:111-122`, `jfdctint.c:96-107`).
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

/// `DESCALE(x, n) = ((int)(x) + (1 << (n - 1))) >> n`.
#[inline(always)]
fn descale(x: i32, n: u32) -> i32 {
    ad(x, 1 << (n - 1)) >> n
}

/// One 1-D LL&M butterfly over the eight inputs `i`. `dc(v)` finishes the two outputs without a multiplier
/// (`i[0] +- i[4]` parts) and `sh` is the right shift of the five multiplied outputs. Returns the outputs in
/// natural order 0..8.
#[inline(always)]
fn llm_1d(i: [i32; 8], dc: impl Fn(i32) -> i32, sh: u32) -> [i32; 8] {
    let tmp0 = ad(i[0], i[7]);
    let tmp7 = sb(i[0], i[7]);
    let tmp1 = ad(i[1], i[6]);
    let tmp6 = sb(i[1], i[6]);
    let tmp2 = ad(i[2], i[5]);
    let tmp5 = sb(i[2], i[5]);
    let tmp3 = ad(i[3], i[4]);
    let tmp4 = sb(i[3], i[4]);

    // Even part per LL&M figure 1 (the published rotator "sqrt(2)*c1" should be "sqrt(2)*c6").
    let tmp10 = ad(tmp0, tmp3);
    let tmp13 = sb(tmp0, tmp3);
    let tmp11 = ad(tmp1, tmp2);
    let tmp12 = sb(tmp1, tmp2);

    let o0 = dc(ad(tmp10, tmp11));
    let o4 = dc(sb(tmp10, tmp11));
    let z1 = m(ad(tmp12, tmp13), FIX_0_541196100);
    let o2 = descale(ad(z1, m(tmp13, FIX_0_765366865)), sh);
    let o6 = descale(ad(z1, m(tmp12, -FIX_1_847759065)), sh);

    // Odd part per figure 8 (i0..i3 of the paper are tmp4..tmp7).
    let mut z1 = ad(tmp4, tmp7);
    let mut z2 = ad(tmp5, tmp6);
    let mut z3 = ad(tmp4, tmp6);
    let mut z4 = ad(tmp5, tmp7);
    let z5 = m(ad(z3, z4), FIX_1_175875602);

    let tmp4 = m(tmp4, FIX_0_298631336);
    let tmp5 = m(tmp5, FIX_2_053119869);
    let tmp6 = m(tmp6, FIX_3_072711026);
    let tmp7 = m(tmp7, FIX_1_501321110);
    z1 = m(z1, -FIX_0_899976223);
    z2 = m(z2, -FIX_2_562915447);
    z3 = m(z3, -FIX_1_961570560);
    z4 = m(z4, -FIX_0_390180644);

    z3 = ad(z3, z5);
    z4 = ad(z4, z5);

    [
        o0,
        descale(ad(ad(tmp7, z1), z4), sh),
        o2,
        descale(ad(ad(tmp6, z2), z3), sh),
        o4,
        descale(ad(ad(tmp5, z2), z4), sh),
        o6,
        descale(ad(ad(tmp4, z1), z3), sh),
    ]
}

/// The IJG LL&M integer forward DCT (`jfdctint_template.c` / `jfdctint.c`), in place.
///
/// * `CONST_BITS` is 13 in every reference unit (the `FIX_*` constants are the 13-bit ones; enforced at compile time).
/// * `PASS1_BITS` is the fractional precision kept between the passes: FFmpeg 8-bit 4, FFmpeg 10-bit 1,
///   libjpeg 8-bit 2, libjpeg 12-bit 1.
/// * `OUT_SHIFT` is the final descale of pass 2 (`OUT_SHIFT` of FFmpeg: 4 / 2; libjpeg uses `PASS1_BITS`).
/// * `E` is the work-array element: `i16` or `i32`.
///
/// Named instances: [`fdct_islow_ff8`], [`fdct_islow_ff10`], [`fdct_islow_tj8`], [`fdct_islow_tj12`].
#[inline]
pub fn fdct_islow<
    const CONST_BITS: u32,
    const PASS1_BITS: u32,
    const OUT_SHIFT: u32,
    E: DctElem,
>(
    block: &mut [E; 64],
) {
    const {
        assert!(
            CONST_BITS == 13,
            "only the CONST_BITS == 13 constant set is ported"
        )
    };
    const { assert!(PASS1_BITS >= 1 && PASS1_BITS < CONST_BITS && OUT_SHIFT >= 1 && OUT_SHIFT <= 8) };

    // Pass 1: rows, scaled up by 2**PASS1_BITS.
    for r in 0..8 {
        let row: [i32; 8] = std::array::from_fn(|k| block[8 * r + k].get());
        let o = llm_1d(row, |v| m(v, 1 << PASS1_BITS), CONST_BITS - PASS1_BITS);
        for (k, v) in o.into_iter().enumerate() {
            block[8 * r + k] = E::put(v);
        }
    }
    // Pass 2: columns; removes the PASS1_BITS scaling, leaves the overall factor of 8.
    for c in 0..8 {
        let col: [i32; 8] = std::array::from_fn(|k| block[8 * k + c].get());
        let o = llm_1d(col, |v| descale(v, OUT_SHIFT), CONST_BITS + OUT_SHIFT);
        for (k, v) in o.into_iter().enumerate() {
            block[8 * k + c] = E::put(v);
        }
    }
}

/// `ff_jpeg_fdct_islow_8` (`<13, 4, 4>`, `int16_t` work array): FFmpeg's 8-bit encoder DCT (mjpeg, mpeg*).
#[inline]
pub fn fdct_islow_ff8(block: &mut [i16; 64]) {
    fdct_islow::<13, 4, 4, i16>(block);
}

/// `ff_jpeg_fdct_islow_10` (`<13, 1, 2>`, `int16_t`): FFmpeg's 9/10-bit DCT, also the ProRes forward transform.
#[inline]
pub fn fdct_islow_ff10(block: &mut [i16; 64]) {
    fdct_islow::<13, 1, 2, i16>(block);
}

/// libjpeg-turbo `jpeg_fdct_islow`, 8-bit unit with `DCTELEM = short` (`<13, 2, 2>`).
#[inline]
pub fn fdct_islow_tj8(block: &mut [i16; 64]) {
    fdct_islow::<13, 2, 2, i16>(block);
}

/// libjpeg-turbo `jpeg12_fdct_islow`, 12-bit unit with `DCTELEM = JLONG` (`<13, 1, 1>`).
#[inline]
pub fn fdct_islow_tj12(block: &mut [i32; 64]) {
    fdct_islow::<13, 1, 1, i32>(block);
}

// ---------------------------------------------------------------------------------------------
// ff_fdct_ifast
// ---------------------------------------------------------------------------------------------

const IF_0_382683433: i32 = 98;
const IF_0_541196100: i32 = 139;
const IF_0_707106781: i32 = 181;
const IF_1_306562965: i32 = 334;

/// `MULTIPLY(var, const) = (DCTELEM)((var) * (const) >> CONST_BITS)` of `jfdctfst.c` (no rounding term: the file
/// leaves `USE_ACCURATE_ROUNDING` undefined), narrowed to the element type: `int16_t` in FFmpeg, `int` / `JLONG` in
/// the portable libjpeg-turbo units.
#[inline(always)]
fn imul<E: DctElem>(var: i32, k: i32) -> i32 {
    E::put(m(var, k) >> 8).get()
}

/// One 1-D AAN butterfly (`row_fdct` / the column loop of `ff_fdct_ifast`); outputs in natural order.
#[inline(always)]
fn aan_1d<E: DctElem>(i: [i32; 8]) -> [i32; 8] {
    let tmp0 = ad(i[0], i[7]);
    let tmp7 = sb(i[0], i[7]);
    let tmp1 = ad(i[1], i[6]);
    let tmp6 = sb(i[1], i[6]);
    let tmp2 = ad(i[2], i[5]);
    let tmp5 = sb(i[2], i[5]);
    let tmp3 = ad(i[3], i[4]);
    let tmp4 = sb(i[3], i[4]);

    // Even part
    let tmp10 = ad(tmp0, tmp3);
    let tmp13 = sb(tmp0, tmp3);
    let tmp11 = ad(tmp1, tmp2);
    let tmp12 = sb(tmp1, tmp2);

    let o0 = ad(tmp10, tmp11);
    let o4 = sb(tmp10, tmp11);
    let z1 = imul::<E>(ad(tmp12, tmp13), IF_0_707106781);
    let o2 = ad(tmp13, z1);
    let o6 = sb(tmp13, z1);

    // Odd part
    let tmp10 = ad(tmp4, tmp5);
    let tmp11 = ad(tmp5, tmp6);
    let tmp12 = ad(tmp6, tmp7);

    let z5 = imul::<E>(sb(tmp10, tmp12), IF_0_382683433);
    let z2 = ad(imul::<E>(tmp10, IF_0_541196100), z5);
    let z4 = ad(imul::<E>(tmp12, IF_1_306562965), z5);
    let z3 = imul::<E>(tmp11, IF_0_707106781);

    let z11 = ad(tmp7, z3);
    let z13 = sb(tmp7, z3);

    [
        o0,
        ad(z11, z4),
        o2,
        sb(z13, z2),
        o4,
        ad(z13, z2),
        o6,
        sb(z11, z4),
    ]
}

/// The AAN fast integer forward DCT (`jfdctfst.c`), in place; `E` is the work-array element (`i16` FFmpeg, `i32`
/// libjpeg-turbo's portable `DCTELEM = int` / 12-bit `JLONG`). The output is scaled per coefficient by the AAN scale
/// factors (the quantiser folds that in). Instances: [`fdct_ifast`], [`fdct_ifast_tj`].
#[inline]
pub fn fdct_ifast_g<E: DctElem>(block: &mut [E; 64]) {
    for r in 0..8 {
        let row: [i32; 8] = std::array::from_fn(|k| block[8 * r + k].get());
        for (k, v) in aan_1d::<E>(row).into_iter().enumerate() {
            block[8 * r + k] = E::put(v);
        }
    }
    for c in 0..8 {
        let col: [i32; 8] = std::array::from_fn(|k| block[8 * k + c].get());
        for (k, v) in aan_1d::<E>(col).into_iter().enumerate() {
            block[8 * k + c] = E::put(v);
        }
    }
}

/// `ff_fdct_ifast`: FFmpeg's `-dct fastint`, on an `int16_t` block.
#[inline]
pub fn fdct_ifast(block: &mut [i16; 64]) {
    fdct_ifast_g(block);
}

/// libjpeg-turbo `jpeg_fdct_ifast` / `jpeg12_fdct_ifast` in the portable C build (`DCTELEM = int` / `JLONG`).
#[inline]
pub fn fdct_ifast_tj(block: &mut [i32; 64]) {
    fdct_ifast_g(block);
}

/// libjpeg-turbo `jpeg_fdct_islow` in the portable C build (8-bit `DCTELEM = int`, `<13, 2, 2>`): what a build
/// without SIMD runs; identical to [`fdct_islow_tj8`] for every sample block (the `short` work array only differs on
/// inputs outside the sample range).
#[inline]
pub fn fdct_islow_tj8_int(block: &mut [i32; 64]) {
    fdct_islow::<13, 2, 2, i32>(block);
}

/// libjpeg `jpeg_fdct_float` (`jfdctflt.c`): the AAN forward DCT in single precision, in place on a level-shifted
/// block; the output is scaled per coefficient by the AAN factors (`jcdctmgr.c` folds them into the divisors).
/// Every operation is a plain `float` multiply or add in C's order (no fused multiply-add).
#[inline]
// The constants are the 9-digit decimals of `jfdctflt.c`, not `FRAC_1_SQRT_2`: the last float bits differ.
#[allow(clippy::approx_constant)]
pub fn fdct_float_tj(block: &mut [f32; 64]) {
    #[inline(always)]
    fn pass(i: [f32; 8]) -> [f32; 8] {
        let tmp0 = i[0] + i[7];
        let tmp7 = i[0] - i[7];
        let tmp1 = i[1] + i[6];
        let tmp6 = i[1] - i[6];
        let tmp2 = i[2] + i[5];
        let tmp5 = i[2] - i[5];
        let tmp3 = i[3] + i[4];
        let tmp4 = i[3] - i[4];

        // Even part
        let tmp10 = tmp0 + tmp3;
        let tmp13 = tmp0 - tmp3;
        let tmp11 = tmp1 + tmp2;
        let tmp12 = tmp1 - tmp2;

        let o0 = tmp10 + tmp11;
        let o4 = tmp10 - tmp11;
        let z1 = (tmp12 + tmp13) * (0.707106781f64 as f32);
        let o2 = tmp13 + z1;
        let o6 = tmp13 - z1;

        // Odd part
        let tmp10 = tmp4 + tmp5;
        let tmp11 = tmp5 + tmp6;
        let tmp12 = tmp6 + tmp7;

        let z5 = (tmp10 - tmp12) * (0.382683433f64 as f32);
        let z2 = (0.541196100f64 as f32) * tmp10 + z5;
        let z4 = (1.306562965f64 as f32) * tmp12 + z5;
        let z3 = tmp11 * (0.707106781f64 as f32);

        let z11 = tmp7 + z3;
        let z13 = tmp7 - z3;

        [o0, z11 + z4, o2, z13 - z2, o4, z13 + z2, o6, z11 - z4]
    }
    for r in 0..8 {
        let o = pass(std::array::from_fn(|k| block[8 * r + k]));
        block[8 * r..8 * r + 8].copy_from_slice(&o);
    }
    for c in 0..8 {
        let o = pass(std::array::from_fn(|k| block[8 * k + c]));
        for (k, v) in o.into_iter().enumerate() {
            block[8 * k + c] = v;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// ff_faandct (float AAN)
// ---------------------------------------------------------------------------------------------

/// `B0..B7` of `faandct.c`: `(cos(pi*k/16) * sqrt(2))^-1`. Copied verbatim (36 digits) from the C source.
#[allow(clippy::excessive_precision)]
const FAAN_B: [f64; 8] = [
    1.000000000000000000000000000000000000,
    0.720959822006947913789091890943021267,
    0.765366864730179543456919968060797734,
    0.850430094767256448766702844371412325,
    1.000000000000000000000000000000000000,
    1.272758580572833938461007018281767032,
    1.847759065022573512256366378793576574,
    3.624509785411551372409941227504289587,
];
/// `A1 = M_SQRT1_2`, `A2`, `A5`, `A4` (`faandct.c:50-53`): DOUBLE constants, so every float operand they touch is
/// computed in double and rounded back to float on assignment, exactly as C's usual arithmetic conversions do.
#[allow(clippy::excessive_precision)] // verbatim from the C source
const FAAN_A2: f64 = 0.54119610014619698435;
#[allow(clippy::excessive_precision)]
const FAAN_A5: f64 = 0.38268343236508977170;
#[allow(clippy::excessive_precision)]
const FAAN_A4: f64 = 1.30656296487637652774;
const FAAN_A1: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// `postscale[64]`: `static const FLOAT` initialised from double products.
fn faan_postscale() -> [f32; 64] {
    std::array::from_fn(|i| (FAAN_B[i / 8] * FAAN_B[i % 8]) as f32)
}

/// One 1-D float AAN butterfly (`row_fdct` body / the column loop of `ff_faandct`), outputs in natural order.
#[inline(always)]
fn faan_1d(i: [f32; 8]) -> [f32; 8] {
    let tmp0 = i[0] + i[7];
    let tmp7 = i[0] - i[7];
    let tmp1 = i[1] + i[6];
    let tmp6 = i[1] - i[6];
    let tmp2 = i[2] + i[5];
    let tmp5 = i[2] - i[5];
    let tmp3 = i[3] + i[4];
    let tmp4 = i[3] - i[4];

    let tmp10 = tmp0 + tmp3;
    let tmp13 = tmp0 - tmp3;
    let tmp11 = tmp1 + tmp2;
    let tmp12 = tmp1 - tmp2;

    let o0 = tmp10 + tmp11;
    let o4 = tmp10 - tmp11;

    let tmp12 = ((tmp12 + tmp13) as f64 * FAAN_A1) as f32;
    let o2 = tmp13 + tmp12;
    let o6 = tmp13 - tmp12;

    let tmp4 = tmp4 + tmp5;
    let tmp5 = tmp5 + tmp6;
    let tmp6 = tmp6 + tmp7;

    let z2 = (f64::from(tmp4) * (FAAN_A2 + FAAN_A5) - f64::from(tmp6) * FAAN_A5) as f32;
    let z4 = (f64::from(tmp6) * (FAAN_A4 - FAAN_A5) + f64::from(tmp4) * FAAN_A5) as f32;

    let tmp5 = (f64::from(tmp5) * FAAN_A1) as f32;

    let z11 = tmp7 + tmp5;
    let z13 = tmp7 - tmp5;

    [o0, z11 + z4, o2, z13 - z2, o4, z13 + z2, o6, z11 - z4]
}

/// `ff_faandct` (`faandct.c:115`, `-dct faan`): the float AAN forward DCT on an `int16_t` block, results rounded
/// with `lrintf` (ties to even) and stored into `int16_t`. Output is in the islow scaling (x8).
#[inline]
pub fn fdct_faan(block: &mut [i16; 64]) {
    let post = faan_postscale();
    let mut temp = [0f32; 64];
    for r in 0..8 {
        let row: [f32; 8] = std::array::from_fn(|k| f32::from(block[8 * r + k]));
        temp[8 * r..8 * r + 8].copy_from_slice(&faan_1d(row));
    }
    for c in 0..8 {
        let col: [f32; 8] = std::array::from_fn(|k| temp[8 * k + c]);
        for (k, v) in faan_1d(col).into_iter().enumerate() {
            block[8 * k + c] = (post[8 * k + c] * v).round_ties_even() as i32 as i16;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A flat block transforms to DC only (value = 8 * mean for the islow scaling, per unit).
    #[test]
    fn islow_flat_block_is_dc_only() {
        let mut b = [10i16; 64];
        fdct_islow_ff8(&mut b);
        assert_eq!(b[0], 640); // row: 80 << 4; column: (8 * 1280 + 8) >> 4
        assert!(b[1..].iter().all(|&v| v == 0));
        let mut b12 = [-100i32; 64];
        fdct_islow_tj12(&mut b12);
        assert!(b12[1..].iter().all(|&v| v == 0));
    }
}
