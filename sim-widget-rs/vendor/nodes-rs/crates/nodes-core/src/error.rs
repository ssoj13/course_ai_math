//! Shared error type for graph operations (wiring, cook order, names, paths).

use thiserror::Error;

/// Result alias used across `nodes-core` public API.
pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("unknown node type: {0}")]
    UnknownNodeType(String),
    #[error("unknown value type: {0}")]
    UnknownValueType(String),
    #[error("cycle detected in graph")]
    Cycle,
    #[error("invalid wire: {0}")]
    InvalidWire(String),
    #[error("subnet not found: {0}")]
    SubnetNotFound(uuid::Uuid),
    #[error("node not found: {0}")]
    NodeNotFound(uuid::Uuid),
    #[error("invalid node name: {0}")]
    InvalidName(String),
    #[error("duplicate node name: {0}")]
    DuplicateName(String),
    #[error("path not found: {0}")]
    PathNotFound(String),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("{0}")]
    Other(String),
}
