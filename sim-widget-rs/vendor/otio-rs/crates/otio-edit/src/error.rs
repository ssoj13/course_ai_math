//! Error type for the editing layer.

use thiserror::Error;

/// An editing-operation failure.
///
/// Edit ops return `Result<_, EditError>` and never panic on bad input — a
/// rejected edit (out-of-bounds index, trim past media bounds, trimming into a
/// transition) is an error the UI surfaces, not a crash.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EditError {
    /// A referenced node id does not resolve in the document.
    #[error("node not found")]
    NodeNotFound,

    /// An index (child / track / field) is out of range.
    #[error("index {index} out of range (len {len})")]
    IndexOutOfRange {
        /// The offending index.
        index: usize,
        /// The container length it exceeded.
        len: usize,
    },

    /// The target node is not of the schema the operation requires.
    #[error("expected schema '{expected}', found '{found}'")]
    WrongSchema {
        /// The schema the op needs (canonical, e.g. `"Track"`).
        expected: String,
        /// The schema actually found.
        found: String,
    },

    /// The edit would fall inside a transition (e.g. a mid-transition trim).
    #[error("cannot edit across a transition")]
    AcrossTransition,

    /// The edit would move outside the clip's media `available_range`.
    #[error("edit exceeds media available range")]
    OutOfMediaBounds,

    /// A structural mutation rejected by the document model.
    #[error("structural error: {0}")]
    Structural(String),

    /// An underlying algorithm error (range/transform/flatten).
    #[error("algorithm error: {0}")]
    Algo(String),
}

impl From<otio_core::Error> for EditError {
    fn from(e: otio_core::Error) -> Self {
        EditError::Structural(e.to_string())
    }
}

impl From<otio_algo::AlgoError> for EditError {
    /// Range/layout algorithm failures surface as [`EditError::Algo`]; the
    /// transition-trim case is mapped to the dedicated [`EditError::AcrossTransition`]
    /// so ops can guard on it directly.
    fn from(e: otio_algo::AlgoError) -> Self {
        match e {
            otio_algo::AlgoError::CannotTrimTransition => EditError::AcrossTransition,
            otio_algo::AlgoError::NodeNotFound => EditError::NodeNotFound,
            other => EditError::Algo(other.to_string()),
        }
    }
}
