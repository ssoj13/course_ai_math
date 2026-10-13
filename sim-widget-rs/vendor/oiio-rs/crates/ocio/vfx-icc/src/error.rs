//! ICC error types.

use thiserror::Error;

/// Result type for ICC operations.
pub type IccResult<T> = Result<T, IccError>;

/// Errors that can occur during ICC operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum IccError {
    /// Failed to load profile from file.
    #[error("failed to load profile: {0}")]
    LoadFailed(String),

    /// Failed to create profile.
    #[error("failed to create profile: {0}")]
    CreateFailed(String),

    /// Failed to create transform.
    #[error("failed to create transform: {0}")]
    TransformFailed(String),

    /// Invalid profile data.
    #[error("invalid profile data: {0}")]
    InvalidProfile(String),

    /// Profile color space mismatch.
    #[error("color space mismatch: expected {expected}, got {actual}")]
    ColorSpaceMismatch {
        /// Expected color space.
        expected: String,
        /// Actual color space.
        actual: String,
    },

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A pixel buffer handed to a transform does not have a usable shape.
    ///
    /// These were `assert!`/`assert_eq!` panics. In `apply_buffer` the length check is still
    /// load-bearing — it guards the `bytemuck` reinterpretation of the flat `[f32]` as
    /// `[[f32; 3]]`, which panics on a non-divisible length — so it still runs, it simply
    /// reports instead of aborting the process.
    #[error("invalid pixel buffer: {reason}")]
    InvalidBuffer {
        /// What was wrong with the buffer's length or shape.
        reason: String,
    },

    /// The color engine failed to build the transfer curve for a [`crate::StandardProfile`]'s
    /// TRC (tone reproduction curve).
    ///
    /// Every caller reaches `ToneCurve::build_gamma`/`build_parametric` with fixed,
    /// crate-internal coefficients — this can only fire if the engine rejects them, which must
    /// surface as a typed error rather than an `.expect()` panic in a color pipeline.
    #[error("failed to build TRC curve for standard profile {profile}: {reason}")]
    StandardCurveFailed {
        /// Name of the [`crate::StandardProfile`] variant whose curve failed to build.
        profile: &'static str,
        /// The underlying `lcms-rs` error, stringified.
        reason: String,
    },

    /// The color engine failed to combine a white point, primaries, and transfer curve into an
    /// RGB profile for a [`crate::StandardProfile`].
    #[error("failed to build standard profile {profile}: {reason}")]
    StandardProfileFailed {
        /// Name of the [`crate::StandardProfile`] variant that failed to build.
        profile: &'static str,
        /// The underlying `lcms-rs` error, stringified.
        reason: String,
    },

    /// A gamma value passed to a gray-profile constructor is not usable to build a transfer
    /// curve.
    ///
    /// `ToneCurve::build_gamma(gamma)` validates the curve *type*, not the gamma *value*: a
    /// non-finite or non-positive gamma still yields a curve, which would then produce silently
    /// wrong colorimetry downstream. This crate rejects the value up front instead.
    #[error("invalid gamma for gray profile: {0} (must be finite and > 0.0)")]
    InvalidGamma(f64),
}
