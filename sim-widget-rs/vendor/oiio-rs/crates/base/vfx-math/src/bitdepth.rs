//! Bit-depth rescaling — OIIO's `bit_range_convert` (`fmath.h:1064-1077`).
//!
//! Several codecs across the workspace decode sub-container bit depths (10-bit RLA color in a
//! 16-bit container, 12-bit JPEG2000/HTJ2K components, etc.) and need to rescale a raw
//! `from_bits`-wide value onto a `to_bits`-wide container's full range. OIIO does this by bit
//! replication rather than a multiply, which maps the full source range onto the full
//! destination range exactly at both ends (12-bit `0xFFF` becomes `0xFFFF`, not `0xFFF0`, the way
//! a naive `v << (to_bits - from_bits)` would).
//!
//! This was previously copy-pasted into `vfx-io`'s `jp2.rs`, `jph.rs`, and `rla.rs` — this is now
//! the single definition; those three call it from here.

/// Replicates the `from_bits`-wide value `v` across a `to_bits`-wide container.
///
/// Identity when `from_bits == to_bits`, so callers can apply it unconditionally rather than
/// special-casing the "already full width" case.
///
/// # Example
///
/// ```rust
/// use vfx_math::bit_range_convert;
///
/// // A 12-bit maximum value fills the full 16-bit range, not just its top 12 bits.
/// assert_eq!(bit_range_convert(0x0FFF, 12, 16), 0xFFFF);
/// assert_eq!(bit_range_convert(0, 12, 16), 0);
/// // Identity when the widths already match.
/// assert_eq!(bit_range_convert(0x1234, 16, 16), 0x1234);
/// ```
pub fn bit_range_convert(v: u32, from_bits: u32, to_bits: u32) -> u32 {
    if from_bits == 0 {
        return 0;
    }
    let mut out = 0u32;
    let mut shift = to_bits as i32 - from_bits as i32;
    while shift > 0 {
        out |= v << shift;
        shift -= from_bits as i32;
    }
    out | (v >> -shift)
}

/// Rescales `v` from `0..=from_max` onto `0..=to_max` by multiply and truncating divide, for
/// sources whose maximum is not `2^n - 1` (PNM `maxval`, any value 1..=65535). Exact at both ends
/// (`from_max` -> `to_max`), `v` above `from_max` is clamped. For power-of-two ranges OIIO's
/// replication, [`bit_range_convert`], is the one to use.
///
/// ```rust
/// use vfx_math::rescale_int;
///
/// assert_eq!(rescale_int(100, 100, 65535), 65535);
/// assert_eq!(rescale_int(1, 2, 255), 127);
/// assert_eq!(rescale_int(9, 7, 255), 255); // clamped
/// ```
pub fn rescale_int(v: u32, from_max: u32, to_max: u32) -> u32 {
    if from_max == 0 {
        return 0;
    }
    (u64::from(v.min(from_max)) * u64::from(to_max) / u64::from(from_max)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_range_convert_saturates_both_ends() {
        assert_eq!(bit_range_convert(0, 12, 16), 0);
        assert_eq!(bit_range_convert(0x0FFF, 12, 16), 0xFFFF);
        assert_eq!(bit_range_convert(0xFF, 8, 16), 0xFFFF);
        assert_eq!(bit_range_convert(1, 1, 8), 0xFF);
        assert_eq!(bit_range_convert(0x1234, 16, 16), 0x1234);
    }

    /// The inverse (right-shift by `to_bits - from_bits`) recovers the original value exactly —
    /// relied on by RLA's writer to reduce a full-range value back to its declared bit depth.
    #[test]
    fn round_trips_via_shift() {
        for bits in [1u32, 2, 4, 8, 10, 12, 16] {
            let max_val = (1u32 << bits) - 1;
            for v in [0, 1, max_val / 2, max_val] {
                let full = bit_range_convert(v, bits, 16);
                let back = full >> (16 - bits);
                assert_eq!(back, v, "bits={bits} v={v}");
            }
        }
    }
}
