//! Fixed-point conversions and fast-floor helpers.
//!
//! Direct, bit-exact ports of the primitives lcms2 defines in `cmsplugin.c` and the
//! inline helpers in `lcms2_internal.h`. These sit on the hottest paths (interpolation,
//! gamma, encoding), so their exact rounding behaviour is load-bearing for conformance
//! — do **not** "clean up" the arithmetic.

/// ICC signed 15.16 fixed-point number (`cmsS15Fixed16Number`).
pub type S15Fixed16 = i32;

/// Convert 15.16 fixed-point to `f64` (`_cms15Fixed16toDouble`).
#[inline]
pub const fn s15f16_to_f64(fix32: S15Fixed16) -> f64 {
    fix32 as f64 / 65536.0
}

/// Convert `f64` to 15.16 fixed-point (`_cmsDoubleTo15Fixed16`).
///
/// C computes `(cmsS15Fixed16Number) floor(v*65536.0 + 0.5)`. The C cast on an
/// out-of-range float is undefined; Rust's `as i32` saturates instead. For every value
/// lcms2 actually feeds here (gamma, XYZ, matrices) the input stays in range, so the
/// results are identical while the Rust version is also memory-safe.
#[inline]
pub fn f64_to_s15f16(v: f64) -> S15Fixed16 {
    (v * 65536.0 + 0.5).floor() as i32
}

/// Convert 8.8 fixed-point to `f64` (`_cms8Fixed8toDouble`).
#[inline]
pub const fn u8f8_to_f64(fixed8: u16) -> f64 {
    fixed8 as f64 / 256.0
}

/// Convert `f64` to 8.8 fixed-point (`_cmsDoubleTo8Fixed8`).
///
/// Mirrors C exactly: go through 15.16, then `(x >> 8) & 0xFFFF`.
#[inline]
pub fn f64_to_u8f8(val: f64) -> u16 {
    ((f64_to_s15f16(val) >> 8) & 0xFFFF) as u16
}

/// Map an integer into the interpolation fixed domain (`_cmsToFixedDomain`).
#[inline]
pub const fn to_fixed_domain(a: i32) -> i32 {
    a + (a + 0x7fff) / 0xffff
}

/// Map back out of the interpolation fixed domain (`_cmsFromFixedDomain`).
#[inline]
pub const fn from_fixed_domain(a: S15Fixed16) -> i32 {
    a - ((a + 0x7fff) >> 16)
}

/// Fast floor to `int`, valid for the range roughly -32767..+32767
/// (`_cmsQuickFloor`, the default `CMS_DONT_USE_FAST_FLOOR` = off path).
///
/// Ports the Sree Kotay / Stuart Nixon magic-number trick: adding `2^36 * 1.5` forces
/// the integer part into the low bits of the IEEE-754 double, which are then read as a
/// 15.16 fixed value. The C code aliases the double through a `union { double; int[2]; }`
/// and reads `halves[0]` on little-endian; [`f64::to_bits`] gives us those exact bytes
/// without any `unsafe`, and `bits as u32` is the little-endian low half.
#[inline]
pub fn quick_floor(val: f64) -> i32 {
    // 2^36 * 1.5; (52 - 16 = 36) leaves the mantissa holding a 15.16 fixed value.
    const DOUBLE2FIXMAGIC: f64 = 68719476736.0 * 1.5;
    let bits = (val + DOUBLE2FIXMAGIC).to_bits();
    (bits as u32 as i32) >> 16
}

/// Fast floor restricted to 0..65535.0 (`_cmsQuickFloorWord`).
///
/// C: `(cmsUInt16Number) _cmsQuickFloor(d - 32767.0) + 32767U`. The `u16` cast wraps to
/// 16 bits before the `+ 32767`, so we replicate with `wrapping_add`.
#[inline]
pub fn quick_floor_word(d: f64) -> u16 {
    (quick_floor(d - 32767.0) as u16).wrapping_add(32767)
}

/// Floor to word with saturation (`_cmsQuickSaturateWord`).
#[inline]
pub fn quick_saturate_word(d: f64) -> u16 {
    let d = d + 0.5;
    if d <= 0.0 {
        return 0;
    }
    if d >= 65535.0 {
        return 0xffff;
    }
    quick_floor_word(d)
}

/// Floor to byte with saturation (`_cmsQuickSaturateByte`).
#[inline]
pub fn quick_saturate_byte(d: f64) -> u8 {
    let d = d + 0.5;
    if d <= 0.0 {
        return 0;
    }
    if d >= 255.0 {
        return 255;
    }
    quick_floor_word(d) as u8
}

/// `FROM_8_TO_16` — expand an 8-bit sample to 16-bit by bit-replication (`(n<<8)|n`).
#[inline]
pub const fn from_8_to_16(rgb: u8) -> u16 {
    ((rgb as u16) << 8) | rgb as u16
}

/// `FROM_16_TO_8` — reduce a 16-bit sample to 8-bit with rounding
/// (`((n*65281 + 8388608) >> 24) & 0xFF`).
#[inline]
pub const fn from_16_to_8(rgb: u16) -> u8 {
    (((rgb as u32 * 65281 + 8388608) >> 24) & 0xFF) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_15_16_roundtrip() {
        // Exact power-of-two encodings are certain.
        assert_eq!(f64_to_s15f16(1.0), 0x0001_0000);
        assert_eq!(s15f16_to_f64(0x0001_0000), 1.0);
        assert_eq!(s15f16_to_f64(0x0000_8000), 0.5);
        // D50 X (0.9642): floor(0.9642*65536 + 0.5) = floor(63190.19) = 63190.
        assert_eq!(f64_to_s15f16(0.9642), 63190);
    }

    #[test]
    fn fixed_8_8() {
        assert_eq!(u8f8_to_f64(0x0100), 1.0);
        assert_eq!(u8f8_to_f64(0x0080), 0.5);
        // gamma 2.2: 15.16 = floor(2.2*65536 + 0.5) = 144179 (0x2_3333); >>8 = 0x233.
        assert_eq!(f64_to_u8f8(2.2), 0x0233);
    }

    #[test]
    fn quick_floor_matches_std_floor_in_range() {
        for i in -30000..=30000 {
            let v = i as f64 * 0.5 + 0.123;
            assert_eq!(quick_floor(v), v.floor() as i32, "mismatch at {v}");
        }
    }

    #[test]
    fn saturate_word_clamps() {
        assert_eq!(quick_saturate_word(-5.0), 0);
        assert_eq!(quick_saturate_word(70000.0), 0xffff);
        assert_eq!(quick_saturate_word(0.0), 0);
        assert_eq!(quick_saturate_word(65535.0), 0xffff);
        // 0.5*65535 rounds to nearest.
        assert_eq!(quick_saturate_word(32767.0), 32767);
    }
}
