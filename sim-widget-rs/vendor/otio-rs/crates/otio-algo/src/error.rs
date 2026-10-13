//! Error type for range/layout computations.

use thiserror::Error;

/// Failure modes for the range algorithms.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum AlgoError {
    /// A node id did not resolve in the document.
    #[error("node not found")]
    NodeNotFound,

    /// The object's kind has no intrinsic duration / available range
    /// (mirrors OTIO `OBJECT_WITHOUT_DURATION` / `NOT_IMPLEMENTED`).
    #[error("cannot compute available range for schema '{0}'")]
    NoAvailableRange(String),

    /// A child index is out of range (after negative-index adjustment).
    #[error("illegal index {index} for {len} children")]
    IllegalIndex {
        /// The requested index (as given, may be negative).
        index: i64,
        /// The number of children.
        len: usize,
    },

    /// A transition node was missing its `in_offset`/`out_offset`.
    #[error("malformed transition (missing offsets)")]
    BadTransition,

    /// A trim range fell partway across a transition (mirrors OTIO
    /// `CANNOT_TRIM_TRANSITION`).
    #[error("cannot trim in the middle of a transition")]
    CannotTrimTransition,

    /// An operation encountered a node of an unexpected kind.
    #[error("type mismatch: {0}")]
    TypeMismatch(String),

    /// A child was not found under the expected parent / ancestor (mirrors OTIO
    /// `NOT_A_CHILD_OF` / `NOT_DESCENDED_FROM`).
    #[error("node is not a descendant of the target")]
    NotDescended,
}
