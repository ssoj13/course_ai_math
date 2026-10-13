//! Error types for vfx-core operations.
//!
//! This module provides a unified error handling system for all image and color
//! processing operations in the VFX pipeline.
//!
//! # Overview
//!
//! The [`Error`] enum covers all failure modes that can occur during:
//! - Image buffer operations (allocation, bounds checking)
//! - Pixel format conversions
//! - Color space transformations
//! - I/O operations (when integrated with vfx-io)
//!
//! # Usage
//!
//! ```rust
//! use vfx_core::{Error, Result};
//!
//! fn process_pixel(x: u32, y: u32, width: u32, height: u32) -> Result<()> {
//!     if x >= width || y >= height {
//!         return Err(Error::OutOfBounds {
//!             x,
//!             y,
//!             width,
//!             height,
//!         });
//!     }
//!     Ok(())
//! }
//! ```
//!
//! # Dependencies
//!
//! - [`thiserror`] - For derive macro error implementation
//!
//! # Used By
//!
//! - [`crate::image::Image`] - Buffer operations
//! - [`crate::image::ImageView`] - View bounds checking
//! - `vfx-io` - File I/O errors
//! - `vfx-lut` - LUT parsing errors

use thiserror::Error;

/// Result type alias using [`Error`] as the error type.
///
/// Convenience alias for `std::result::Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during VFX image processing operations.
///
/// This enum uses [`thiserror`] for automatic [`std::error::Error`] and
/// [`std::fmt::Display`] implementations.
///
/// # Categories
///
/// - **Bounds errors**: [`OutOfBounds`](Error::OutOfBounds), [`InvalidRegion`](Error::InvalidRegion)
/// - **Allocation errors**: [`AllocationFailed`](Error::AllocationFailed)
/// - **Format errors**: [`UnsupportedFormat`](Error::UnsupportedFormat), [`ChannelMismatch`](Error::ChannelMismatch)
/// - **Dimension errors**: [`DimensionMismatch`](Error::DimensionMismatch), [`InvalidDimensions`](Error::InvalidDimensions)
/// - **I/O errors**: [`Io`](Error::Io)
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// Pixel coordinates are outside image bounds.
    ///
    /// Returned when attempting to access a pixel at (x, y) where
    /// `x >= width` or `y >= height`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Error;
    ///
    /// let err = Error::OutOfBounds {
    ///     x: 100,
    ///     y: 50,
    ///     width: 80,
    ///     height: 60,
    /// };
    /// assert!(err.to_string().contains("100"));
    /// ```
    #[error("pixel ({x}, {y}) out of bounds for image {width}x{height}")]
    OutOfBounds {
        /// X coordinate that was out of bounds
        x: u32,
        /// Y coordinate that was out of bounds
        y: u32,
        /// Image width
        width: u32,
        /// Image height
        height: u32,
    },

    /// Region of interest extends beyond image bounds.
    ///
    /// Returned when a [`crate::rect::Rect`] or ROI doesn't fit within
    /// the image dimensions.
    #[error("region ({rx}, {ry}, {rw}x{rh}) exceeds image bounds {width}x{height}")]
    InvalidRegion {
        /// Region X origin
        rx: u32,
        /// Region Y origin
        ry: u32,
        /// Region width
        rw: u32,
        /// Region height
        rh: u32,
        /// Image width
        width: u32,
        /// Image height
        height: u32,
    },

    /// Memory allocation failed.
    ///
    /// Returned when the system cannot allocate enough memory for an
    /// image buffer. This typically happens with very large images.
    ///
    /// # Fields
    ///
    /// - `requested` - Number of bytes requested
    /// - `reason` - Optional description of why allocation failed
    #[error("failed to allocate {requested} bytes: {reason}")]
    AllocationFailed {
        /// Bytes requested
        requested: usize,
        /// Failure reason
        reason: String,
    },

    /// Pixel format is not supported for this operation.
    ///
    /// Some operations only work with specific pixel formats (e.g., f32
    /// for HDR processing, u8 for display).
    #[error("unsupported pixel format: {format}")]
    UnsupportedFormat {
        /// Format name or description
        format: String,
    },

    /// Channel count mismatch between source and destination.
    ///
    /// Returned when trying to copy/convert between images with different
    /// channel counts without explicit conversion.
    #[error("channel mismatch: expected {expected}, got {got}")]
    ChannelMismatch {
        /// Expected channel count
        expected: u8,
        /// Actual channel count
        got: u8,
    },

    /// Image dimensions don't match for the operation.
    ///
    /// Returned when an operation requires images of the same size
    /// (e.g., blending, compositing).
    #[error("dimension mismatch: {a_width}x{a_height} vs {b_width}x{b_height}")]
    DimensionMismatch {
        /// First image width
        a_width: u32,
        /// First image height
        a_height: u32,
        /// Second image width
        b_width: u32,
        /// Second image height
        b_height: u32,
    },

    /// Invalid image dimensions.
    ///
    /// Returned when width or height is zero, or dimensions would cause
    /// integer overflow in buffer size calculations.
    #[error("invalid dimensions: {width}x{height} ({reason})")]
    InvalidDimensions {
        /// Requested width
        width: u32,
        /// Requested height
        height: u32,
        /// Reason why dimensions are invalid
        reason: String,
    },

    /// Stride is too small for the given width and pixel size.
    ///
    /// When creating an image from raw data with custom stride, the stride
    /// must be at least `width * bytes_per_pixel`.
    #[error("stride {stride} is less than minimum {min_stride} for width {width}")]
    InvalidStride {
        /// Provided stride
        stride: usize,
        /// Minimum required stride
        min_stride: usize,
        /// Image width
        width: u32,
    },

    /// I/O error during file operations.
    ///
    /// Wraps [`std::io::Error`] for file reading/writing operations.
    /// Primarily used by `vfx-io` crate.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Generic error with custom message.
    ///
    /// Catch-all for errors that don't fit other categories.
    /// Prefer specific error variants when possible.
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Creates an [`Error::OutOfBounds`] error.
    ///
    /// # Arguments
    ///
    /// * `x` - X coordinate that was accessed
    /// * `y` - Y coordinate that was accessed
    /// * `width` - Image width
    /// * `height` - Image height
    #[inline]
    pub fn out_of_bounds(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self::OutOfBounds {
            x,
            y,
            width,
            height,
        }
    }

    /// Creates an [`Error::InvalidRegion`] error.
    #[inline]
    pub fn invalid_region(rx: u32, ry: u32, rw: u32, rh: u32, width: u32, height: u32) -> Self {
        Self::InvalidRegion {
            rx,
            ry,
            rw,
            rh,
            width,
            height,
        }
    }

    /// Creates an [`Error::AllocationFailed`] error.
    #[inline]
    pub fn allocation_failed(requested: usize, reason: impl Into<String>) -> Self {
        Self::AllocationFailed {
            requested,
            reason: reason.into(),
        }
    }

    /// Creates an [`Error::InvalidDimensions`] error.
    #[inline]
    pub fn invalid_dimensions(width: u32, height: u32, reason: impl Into<String>) -> Self {
        Self::InvalidDimensions {
            width,
            height,
            reason: reason.into(),
        }
    }

    /// Creates an [`Error::DimensionMismatch`] error.
    #[inline]
    pub fn dimension_mismatch(a: (u32, u32), b: (u32, u32)) -> Self {
        Self::DimensionMismatch {
            a_width: a.0,
            a_height: a.1,
            b_width: b.0,
            b_height: b.1,
        }
    }

    /// Creates an [`Error::ChannelMismatch`] error.
    #[inline]
    pub fn channel_mismatch(expected: u8, got: u8) -> Self {
        Self::ChannelMismatch { expected, got }
    }

    /// Creates an [`Error::UnsupportedFormat`] error.
    #[inline]
    pub fn unsupported_format(format: impl Into<String>) -> Self {
        Self::UnsupportedFormat {
            format: format.into(),
        }
    }

    /// Creates an [`Error::Other`] error.
    #[inline]
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }

    /// Returns `true` if this is a bounds-related error.
    #[inline]
    pub fn is_bounds_error(&self) -> bool {
        matches!(self, Self::OutOfBounds { .. } | Self::InvalidRegion { .. })
    }

    /// Returns `true` if this is an allocation error.
    #[inline]
    pub fn is_allocation_error(&self) -> bool {
        matches!(self, Self::AllocationFailed { .. })
    }

    /// Returns `true` if this is an I/O error.
    #[inline]
    pub fn is_io_error(&self) -> bool {
        matches!(self, Self::Io(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_out_of_bounds() {
        let err = Error::out_of_bounds(100, 50, 80, 60);
        let msg = err.to_string();
        assert!(msg.contains("100"));
        assert!(msg.contains("50"));
        assert!(msg.contains("80"));
        assert!(msg.contains("60"));
        assert!(err.is_bounds_error());
    }

    #[test]
    fn test_allocation_failed() {
        let err = Error::allocation_failed(1024 * 1024 * 1024, "out of memory");
        assert!(err.to_string().contains("out of memory"));
        assert!(err.is_allocation_error());
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(err.is_io_error());
    }

    #[test]
    fn test_dimension_mismatch() {
        let err = Error::dimension_mismatch((100, 100), (200, 200));
        let msg = err.to_string();
        assert!(msg.contains("100x100"));
        assert!(msg.contains("200x200"));
    }
}
