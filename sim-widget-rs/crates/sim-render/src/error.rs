//! Addressable rendering failures, returned before publication or allocation.
use thiserror::Error;
/// Errors do not mutate the current valid scene.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("capability {name}: required {required}, observed {observed}")]
    Capability {
        name: String,
        required: u64,
        observed: u64,
    },
    #[error("invalid {address}: {reason}")]
    Invalid { address: String, reason: String },
    #[error("GPU validation: {0}")]
    Gpu(String),
    #[error("resource budget: {0}")]
    Budget(String),
    #[error("stale completion: {0}")]
    Stale(String),
    #[error("renderer disposed")]
    Disposed,
}
/// Rendering result.
pub type Result<T> = std::result::Result<T, RenderError>;
/// Construct an addressed validation failure.
pub fn invalid(address: impl Into<String>, reason: impl Into<String>) -> RenderError {
    RenderError::Invalid {
        address: address.into(),
        reason: reason.into(),
    }
}
