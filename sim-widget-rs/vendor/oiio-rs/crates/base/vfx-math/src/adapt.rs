//! Chromatic Adaptation Transforms (CAT).
//!
//! This module provides matrices and functions for adapting colors
//! between different illuminants (white points).
//!
//! # What is Chromatic Adaptation?
//!
//! When viewing a white object under tungsten light (warm) vs daylight (cool),
//! our visual system adapts so that both appear "white". Chromatic adaptation
//! transforms simulate this effect mathematically.
//!
//! # Common Use Cases
//!
//! - Converting between D65 and D50 white points
//! - Adapting camera white balance
//! - Converting between color spaces with different illuminants
//!
//! # Supported Methods
//!
//! - [`BRADFORD`] - Best overall accuracy (default choice)
//! - [`VON_KRIES`] - Classic cone response model
//! - [`CAT02`] - From CIECAM02 color appearance model
//! - [`XYZ_SCALING`] - Simple but less accurate
//!
//! # Usage
//!
//! ```rust
//! use vfx_math::{adapt_matrix, BRADFORD, Mat3, Vec3};
//!
//! // White points as XYZ (Y = 1) from their xy chromaticities; `vfx_primaries::white_xyz` does
//! // this for the named whites (`D65_XY`, `D50_XY`, ...).
//! let xyz = |x: f32, y: f32| Vec3::new(x / y, 1.0, (1.0 - x - y) / y);
//! let (d65, d50) = (xyz(0.3127, 0.3290), xyz(0.3457, 0.3585));
//!
//! // Create adaptation matrix from D65 to D50 and adapt a colour
//! let d65_to_d50 = adapt_matrix(BRADFORD, d65, d50);
//! let xyz_d50 = d65_to_d50 * d65;
//! assert!((xyz_d50 - d50).length() < 1e-5);
//! ```
//!
//! This module holds the adaptation *method* only. White points are data about colour spaces and
//! live in `vfx-primaries` as xy chromaticities (ROADMAP G16): adapting between tabulated XYZ
//! whites (e.g. ASTM D65 `0.95047, 1.08883`) while the RGB->XYZ matrices use the chromaticity
//! whites leaves white off white (B13), so this crate no longer carries XYZ white constants or
//! matrices precomputed from them.

use crate::{Mat3, Vec3};

// ============================================================================
// Chromatic Adaptation Matrices
// ============================================================================

/// Bradford chromatic adaptation matrix.
///
/// Transforms XYZ to a "sharpened" cone response space.
/// Generally considered the best overall method for most applications.
///
/// # Reference
///
/// Lam, K.M. (1985). Metamerism and Colour Constancy.
pub const BRADFORD: Mat3 = Mat3::from_rows([
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
]);

/// Inverse Bradford matrix.
pub const BRADFORD_INV: Mat3 = Mat3::from_rows([
    [0.9869929, -0.1470543, 0.1599627],
    [0.4323053, 0.5183603, 0.0492912],
    [-0.0085287, 0.0400428, 0.9684867],
]);

/// Von Kries chromatic adaptation matrix.
///
/// Classic cone response model using Hunt-Pointer-Estevez transformation.
/// Simpler than Bradford but less accurate for large white point changes.
pub const VON_KRIES: Mat3 = Mat3::from_rows([
    [0.40024, 0.70760, -0.08081],
    [-0.22630, 1.16532, 0.04570],
    [0.00000, 0.00000, 0.91822],
]);

/// Inverse Von Kries matrix.
pub const VON_KRIES_INV: Mat3 = Mat3::from_rows([
    [1.8599364, -1.1293816, 0.2198974],
    [0.3611914, 0.6388125, -0.0000064],
    [0.0000000, 0.0000000, 1.0890636],
]);

/// CAT02 chromatic adaptation matrix.
///
/// From the CIECAM02 color appearance model.
/// Good balance between accuracy and computational efficiency.
pub const CAT02: Mat3 = Mat3::from_rows([
    [0.7328, 0.4296, -0.1624],
    [-0.7036, 1.6975, 0.0061],
    [0.0030, 0.0136, 0.9834],
]);

/// Inverse CAT02 matrix.
pub const CAT02_INV: Mat3 = Mat3::from_rows([
    [1.0961238, -0.278_869, 0.1827452],
    [0.454_369, 0.4735332, 0.0720978],
    [-0.0096276, -0.0056980, 1.0153256],
]);

/// XYZ Scaling (simple diagonal adaptation).
///
/// The simplest method - just scales XYZ components.
/// Fast but inaccurate for large white point differences.
pub const XYZ_SCALING: Mat3 = Mat3::IDENTITY;

/// Inverse XYZ Scaling matrix.
pub const XYZ_SCALING_INV: Mat3 = Mat3::IDENTITY;

// ============================================================================
// Adaptation Functions
// ============================================================================

/// Computes a chromatic adaptation matrix between two white points.
///
/// The resulting matrix transforms XYZ values from the source illuminant
/// to the destination illuminant.
///
/// # Arguments
///
/// * `method` - The CAT matrix to use ([`BRADFORD`], [`VON_KRIES`], etc.)
/// * `src_white` - Source white point in XYZ
/// * `dst_white` - Destination white point in XYZ
///
/// # Example
///
/// ```rust
/// use vfx_math::{adapt_matrix, BRADFORD, Vec3};
///
/// let xyz = |x: f32, y: f32| Vec3::new(x / y, 1.0, (1.0 - x - y) / y);
/// let (d65, d50) = (xyz(0.3127, 0.3290), xyz(0.3457, 0.3585));
/// let d65_to_d50 = adapt_matrix(BRADFORD, d65, d50);
///
/// // Verify white point transforms correctly
/// let result = d65_to_d50 * d65;
/// assert!((result - d50).length() < 1e-5);
/// ```
pub fn adapt_matrix(method: Mat3, src_white: Vec3, dst_white: Vec3) -> Mat3 {
    // Get the inverse of the method matrix
    let method_inv = method.inverse().unwrap_or(Mat3::IDENTITY);

    // Transform white points to cone/adapted space
    let src_cone = method * src_white;
    let dst_cone = method * dst_white;

    // Create diagonal scaling matrix
    let scale = Mat3::diagonal(
        dst_cone.x / src_cone.x,
        dst_cone.y / src_cone.y,
        dst_cone.z / src_cone.z,
    );

    // Combine: M^-1 * S * M
    method_inv * scale * method
}

/// Computes the inverse adaptation matrix.
///
/// Equivalent to `adapt_matrix(method, dst_white, src_white)`.
#[inline]
pub fn adapt_matrix_inv(method: Mat3, src_white: Vec3, dst_white: Vec3) -> Mat3 {
    adapt_matrix(method, dst_white, src_white)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// XYZ (Y = 1) of an xy chromaticity.
    fn xyz(x: f32, y: f32) -> Vec3 {
        Vec3::new(x / y, 1.0, (1.0 - x - y) / y)
    }

    fn d65() -> Vec3 {
        xyz(0.3127, 0.3290)
    }

    fn d50() -> Vec3 {
        xyz(0.3457, 0.3585)
    }

    #[test]
    fn adapt_maps_source_white_to_destination_white() {
        for (src, dst) in [
            (d65(), d50()),
            (d50(), d65()),
            (d65(), xyz(0.32168, 0.33767)),
        ] {
            let r = adapt_matrix(BRADFORD, src, dst) * src;
            assert!((r - dst).length() < 1e-5, "{r:?} vs {dst:?}");
        }
    }

    #[test]
    fn test_adapt_matrix_roundtrip() {
        let d65_to_d50 = adapt_matrix(BRADFORD, d65(), d50());
        let d50_to_d65 = adapt_matrix(BRADFORD, d50(), d65());

        // Should be approximately inverse
        let roundtrip = d50_to_d65 * d65_to_d50;

        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (roundtrip.m[i][j] - expected).abs() < 0.001,
                    "roundtrip[{}][{}] = {} (expected {})",
                    i,
                    j,
                    roundtrip.m[i][j],
                    expected
                );
            }
        }
    }

    #[test]
    fn test_adapt_identity() {
        // Adapting to same white point should be identity
        let same = adapt_matrix(BRADFORD, d65(), d65());

        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((same.m[i][j] - expected).abs() < 0.001);
            }
        }
    }
}
