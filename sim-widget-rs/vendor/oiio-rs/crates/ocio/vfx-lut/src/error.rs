//! LUT error types.

use thiserror::Error;

/// Result type for LUT operations.
pub type LutResult<T> = Result<T, LutError>;

/// Errors that can occur during LUT operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LutError {
    /// Invalid LUT size.
    #[error("invalid LUT size: {0}")]
    InvalidSize(String),

    /// Invalid input range.
    #[error("invalid input range: [{min}, {max}]")]
    InvalidRange {
        /// Minimum value
        min: f32,
        /// Maximum value
        max: f32,
    },

    /// Parse error when loading LUT files.
    #[error("parse error: {0}")]
    ParseError(String),

    /// A parameter outside the range the format's specification allows (for example a CDL slope
    /// below 0, see `cdl::validate_cdl_params`). The message is the reference's.
    #[error("{0}")]
    InvalidParameter(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A keyword/attribute/element appeared that this parser does not recognise.
    ///
    /// LUT parsers read untrusted input, so an unknown token must name itself in the error
    /// rather than be swallowed by a catch-all arm — silently ignoring it yields a LUT that is
    /// wrong in a way nothing downstream can detect.
    #[error("{format}: unknown {kind} {token:?}{}", .line.map(|l| format!(" at line {l}")).unwrap_or_default())]
    UnknownToken {
        /// File format being parsed (e.g. "CLF", "SPI3D")
        format: &'static str,
        /// What kind of token it was (e.g. "attribute", "keyword", "element")
        kind: &'static str,
        /// The token itself, as it appeared in the file
        token: String,
        /// 1-based line number, when the parser tracks one
        line: Option<usize>,
    },

    /// A recognised feature that this parser does not yet implement.
    ///
    /// Distinct from [`LutError::UnknownToken`]: the token is valid, we simply cannot honour it,
    /// and producing a LUT that ignores it would be silently wrong.
    #[error("{format}: unsupported {feature}: {detail}")]
    Unsupported {
        /// File format being parsed
        format: &'static str,
        /// The feature name as it appears in the specification
        feature: &'static str,
        /// Why it cannot be honoured, or what value was seen
        detail: String,
    },

    /// A bit depth was declared that is not part of the format's specification.
    #[error("{format}: invalid bit depth {value:?}")]
    InvalidBitDepth {
        /// File format being parsed
        format: &'static str,
        /// The bit depth token as it appeared in the file
        value: String,
    },

    /// A size read from a file overflowed, or is outside the range the format permits.
    ///
    /// Carries the offending dimensions so a corrupt file can be diagnosed without a re-parse.
    #[error("{format}: LUT dimensions {dims:?} out of range ({reason})")]
    SizeOutOfRange {
        /// File format being parsed
        format: &'static str,
        /// The dimensions as read from the file
        dims: Vec<u64>,
        /// Why they were rejected (overflow, below minimum, above maximum)
        reason: String,
    },

    /// A 3D LUT data line addressed a grid position that was already written.
    #[error("{format}: duplicate entry for index [{r}, {g}, {b}]")]
    DuplicateIndex {
        /// File format being parsed
        format: &'static str,
        /// Red grid index
        r: usize,
        /// Green grid index
        g: usize,
        /// Blue grid index
        b: usize,
    },

    /// A 3D LUT declared a grid size but did not supply a value for every grid position.
    #[error("{format}: incomplete LUT, {missing} of {expected} grid entries never written")]
    IncompleteCoverage {
        /// File format being parsed
        format: &'static str,
        /// Number of grid positions left unwritten
        missing: usize,
        /// Total grid positions implied by the declared size
        expected: usize,
    },
}
