//! THE LUT size limits and size checks for every parser in this crate (ROADMAP C2).
//!
//! Port of OpenColorIO's `LutLimits.h` (`Max1DLUTLength = 300000`, `Max3DLUTLength = 129`). A
//! size read from a file is untrusted: before this module each parser either had its own copy of
//! these numbers or none at all, so a hostile header could ask for a multi-gigabyte allocation or
//! a cube edge of 1 (which divides by `size - 1`). Every parser checks a file-derived size here,
//! before it allocates or divides.

use crate::{LutError, LutResult};

/// Largest 1D LUT length, OCIO `Max1DLUTLength`.
pub const MAX_1D_LUT_LENGTH: usize = 300_000;

/// Largest 3D LUT edge length, OCIO `Max3DLUTLength`. A 129^3 cube is ~2.1M RGB triples.
pub const MAX_3D_LUT_LENGTH: usize = 129;

/// Checks a 1D LUT length read from a file: `2..=MAX_1D_LUT_LENGTH`. Returns it unchanged.
pub(crate) fn check_1d_length(format: &'static str, length: usize) -> LutResult<usize> {
    if !(2..=MAX_1D_LUT_LENGTH).contains(&length) {
        return Err(LutError::SizeOutOfRange {
            format,
            dims: vec![length as u64],
            reason: format!("1D LUT length must be between 2 and {MAX_1D_LUT_LENGTH}"),
        });
    }
    Ok(length)
}

/// Checks a 3D LUT edge length read from a file (`2..=MAX_3D_LUT_LENGTH`) and returns the entry
/// count `size^3`, computed with checked arithmetic (the bound makes overflow impossible today;
/// the checked multiply keeps it impossible if the bound ever grows).
pub(crate) fn check_3d_size(format: &'static str, size: usize) -> LutResult<usize> {
    if !(2..=MAX_3D_LUT_LENGTH).contains(&size) {
        return Err(LutError::SizeOutOfRange {
            format,
            dims: vec![size as u64],
            reason: format!("3D LUT size must be between 2 and {MAX_3D_LUT_LENGTH}"),
        });
    }
    size.checked_mul(size)
        .and_then(|sq| sq.checked_mul(size))
        .ok_or_else(|| LutError::SizeOutOfRange {
            format,
            dims: vec![size as u64],
            reason: "3D LUT size cubed overflows".into(),
        })
}

/// [`check_3d_size`] for a caller that wants the edge length back rather than the entry count.
pub(crate) fn check_3d_edge(format: &'static str, size: usize) -> LutResult<usize> {
    check_3d_size(format, size)?;
    Ok(size)
}

/// The edge length of a 3D LUT whose size is implied by its entry count (formats with no size
/// header): `count` must be `n^3` for an `n` that passes [`check_3d_size`].
pub(crate) fn cube_edge_from_count(format: &'static str, count: usize) -> LutResult<usize> {
    let n = (count as f64).cbrt().round() as usize;
    if n.checked_pow(3) != Some(count) {
        return Err(LutError::SizeOutOfRange {
            format,
            dims: vec![count as u64],
            reason: format!("{count} 3D LUT entries is not a perfect cube"),
        });
    }
    check_3d_size(format, n)?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_are_inclusive_and_match_ocio() {
        assert!(check_1d_length("t", 1).is_err());
        assert_eq!(check_1d_length("t", 2).unwrap(), 2);
        assert_eq!(check_1d_length("t", 300_000).unwrap(), 300_000);
        assert!(check_1d_length("t", 300_001).is_err());
        assert!(check_3d_size("t", 0).is_err());
        assert!(check_3d_size("t", 1).is_err());
        assert_eq!(check_3d_size("t", 2).unwrap(), 8);
        assert_eq!(check_3d_size("t", 129).unwrap(), 129 * 129 * 129);
        assert!(check_3d_size("t", 130).is_err());
        assert!(check_3d_size("t", usize::MAX).is_err());
    }

    #[test]
    fn edge_from_count_needs_a_perfect_cube_in_range() {
        assert_eq!(cube_edge_from_count("t", 17 * 17 * 17).unwrap(), 17);
        assert!(cube_edge_from_count("t", 17 * 17 * 17 + 1).is_err());
        assert!(cube_edge_from_count("t", 1).is_err());
        assert!(cube_edge_from_count("t", 0).is_err());
        assert!(cube_edge_from_count("t", 130 * 130 * 130).is_err());
    }
}
