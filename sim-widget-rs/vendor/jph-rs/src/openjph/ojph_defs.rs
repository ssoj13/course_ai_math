//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/openjph/ojph_defs.h`

pub type Ui8 = u8;
pub type Si8 = i8;
pub type Ui16 = u16;
pub type Si16 = i16;
pub type Ui32 = u32;
pub type Si32 = i32;
pub type Ui64 = u64;
pub type Si64 = i64;

pub const NUM_FRAC_BITS: i32 = 13;
pub use NUM_FRAC_BITS as NumFracBits;

#[inline]
pub const fn ojph_div_ceil(a: u32, b: u32) -> u32 {
    // `(a + b - 1) / b` wraps for `a` near `u32::MAX` (SIZ extents come straight from the file).
    a.div_ceil(b)
}

#[inline]
pub fn ojph_max<T: Ord + Copy>(a: T, b: T) -> T {
    if a > b { a } else { b }
}

#[inline]
pub fn ojph_min<T: Ord + Copy>(a: T, b: T) -> T {
    if a < b { a } else { b }
}

#[inline]
pub const fn ojph_unused<T>(_value: &T) {}
