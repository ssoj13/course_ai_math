//! Error type for the core model and JSON round-trip.

use thiserror::Error;

/// Failure modes when parsing/emitting an OTIO document.
#[derive(Debug, Error)]
pub enum Error {
    /// Underlying JSON (de)serialization error.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    /// The top-level value was not a JSON object.
    #[error("root is not a JSON object")]
    NotAnObject,

    /// A serializable object was missing its `OTIO_SCHEMA` tag.
    #[error("object missing OTIO_SCHEMA")]
    MissingSchema,

    /// An `OTIO_SCHEMA` tag was not of the form `Name.Version`.
    #[error("malformed OTIO_SCHEMA: '{0}'")]
    BadSchema(String),

    /// The document has no root node to serialize.
    #[error("document is empty")]
    Empty,

    /// A structural mutation referenced a missing node or a field of the wrong
    /// shape (e.g. appending to a `children` field that holds something else).
    #[error("structural error: {0}")]
    Structural(String),
}
