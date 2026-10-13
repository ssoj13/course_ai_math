use thiserror::Error;

#[derive(Debug, Error)]
pub enum BoxError {
    #[error("box is frozen")]
    Frozen,
    #[error("path not found: {0}")]
    PathNotFound(String),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("type mismatch at path: {0}")]
    TypeMismatch(String),
    #[error("duplicate attribute mapping for key `{key}` (also `{other}`)")]
    DuplicateAttribute { key: String, other: String },
    #[error("recast failed for `{path}`: {reason}")]
    RecastFailed { path: String, reason: String },
    #[error("reserved key `{0}` cannot be used as attribute")]
    ReservedKey(String),
}

#[derive(Debug, Error)]
pub enum MergeError {
    #[error("merge conflict at `{path}`: {reason}")]
    Conflict { path: String, reason: String },
    #[error("type conflict at `{path}`: left={left}, right={right}")]
    TypeConflict {
        path: String,
        left: String,
        right: String,
    },
    #[error("array zip strict length mismatch at `{path}`")]
    ZipStrictLength { path: String },
    #[error("merge mode `{0}` requires `MergeOptions.base` to be set")]
    MissingBase(String),
    #[error(transparent)]
    Box(#[from] BoxError),
}

#[derive(Debug, Error)]
pub enum DiffError {
    #[error("diff failed at `{path}`: {reason}")]
    Failed { path: String, reason: String },
}

#[derive(Debug, Error)]
pub enum SubtractError {
    #[error("subtract failed at `{path}`: {reason}")]
    Failed { path: String, reason: String },
    #[error(transparent)]
    Merge(#[from] MergeError),
    #[error(transparent)]
    Box(#[from] BoxError),
}
