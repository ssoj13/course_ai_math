//! Shared constant tables owned by `codec-simd` so every consumer (ffmpeg-rs codecs, `jpg-rs`)
//! reads ONE copy.

/// The 8x8 zig-zag scan: `ZIGZAG_8X8[i]` is the row-major (natural) index of the i-th coefficient in
/// zig-zag order. Identical to FFmpeg `ff_zigzag_direct[64]` (`libavcodec/mathtables.c:137`) and to
/// the first 64 entries of libjpeg `jpeg_natural_order[]` (`jutils.c`) / T.81 Figure A.6.
#[rustfmt::skip]
pub const ZIGZAG_8X8: [u8; 64] = [
     0,  1,  8, 16,  9,  2,  3, 10,
    17, 24, 32, 25, 18, 11,  4,  5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13,  6,  7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

/// Inverse of [`ZIGZAG_8X8`]: `INV_ZIGZAG_8X8[n]` is the zig-zag position of natural index `n`.
pub const INV_ZIGZAG_8X8: [u8; 64] = {
    let mut inv = [0u8; 64];
    let mut i = 0;
    while i < 64 {
        inv[ZIGZAG_8X8[i] as usize] = i as u8;
        i += 1;
    }
    inv
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zigzag_is_a_permutation_and_inverts() {
        let mut seen = [false; 64];
        for (i, &n) in ZIGZAG_8X8.iter().enumerate() {
            assert!(!seen[n as usize], "duplicate natural index {n}");
            seen[n as usize] = true;
            assert_eq!(INV_ZIGZAG_8X8[n as usize] as usize, i);
        }
    }

    /// Anchor points of T.81 Figure A.6 (row-major index of the first zig-zag steps).
    #[test]
    fn zigzag_matches_t81_figure_a6() {
        assert_eq!(&ZIGZAG_8X8[..10], &[0, 1, 8, 16, 9, 2, 3, 10, 17, 24]);
        assert_eq!(ZIGZAG_8X8[63], 63);
    }
}
