//! Error handling.
//!
//! lcms2 reports problems through the variadic `cmsSignalError(ContextID, code,
//! text, ...)` side-channel and then returns `NULL`/`FALSE`. The Rust port replaces
//! that with a plain [`Result`]: fallible operations return `Err(Error)` carrying the
//! same `cmsERROR_*` classification (see [`ErrorCode`]) plus a human-readable message.
//! A context-level logger hook (for lcms-compatible diagnostics) is layered on top in
//! the `context` module later; the error *values* live here so every subsystem can use
//! them from Phase 0 onward.

use core::fmt;

/// Error classification, 1:1 with the `cmsERROR_*` constants in `lcms2.h`.
///
/// The numeric values match the C header so they can be surfaced unchanged to any
/// lcms-compatible logging hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ErrorCode {
    /// `cmsERROR_UNDEFINED` — unclassified failure.
    Undefined = 0,
    /// `cmsERROR_FILE` — file could not be opened/accessed.
    File = 1,
    /// `cmsERROR_RANGE` — a value was outside its permitted range.
    Range = 2,
    /// `cmsERROR_INTERNAL` — internal invariant violated (a bug).
    Internal = 3,
    /// `cmsERROR_NULL` — a required value was absent.
    Null = 4,
    /// `cmsERROR_READ` — read past end / truncated stream.
    Read = 5,
    /// `cmsERROR_SEEK` — seek failed.
    Seek = 6,
    /// `cmsERROR_WRITE` — write failed.
    Write = 7,
    /// `cmsERROR_UNKNOWN_EXTENSION` — unsupported tag/type/format extension.
    UnknownExtension = 8,
    /// `cmsERROR_COLORSPACE_CHECK` — color-space mismatch between profiles.
    ColorspaceCheck = 9,
    /// `cmsERROR_ALREADY_DEFINED` — duplicate definition.
    AlreadyDefined = 10,
    /// `cmsERROR_BAD_SIGNATURE` — bad/unexpected signature.
    BadSignature = 11,
    /// `cmsERROR_CORRUPTION_DETECTED` — malformed data.
    CorruptionDetected = 12,
    /// `cmsERROR_NOT_SUITABLE` — operation not applicable to this object.
    NotSuitable = 13,
}

/// A color-management error: a [`ErrorCode`] classification plus a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The `cmsERROR_*`-equivalent classification.
    pub code: ErrorCode,
    /// Human-readable description (mirrors the `cmsSignalError` format string, already
    /// formatted).
    pub message: String,
}

impl Error {
    /// Construct an error with a classification and message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

/// Crate-wide result type.
pub type Result<T> = core::result::Result<T, Error>;

/// `LCMS_VERSION` (`lcms2.h:99`) — the reference release this port tracks, 2.19.
pub const LCMS_VERSION: i32 = 2190;

/// `cmsGetEncodedCMMversion` (`cmserr.c:30`) — [`LCMS_VERSION`] as an integer.
///
/// C exposes it so an application can detect a header/shared-object version mismatch; there is
/// no such split here, but callers porting from lcms2 expect the entry point to exist.
pub fn encoded_cmm_version() -> i32 {
    LCMS_VERSION
}

/// Build an [`Error`] with a `format!`-style message, mirroring `cmsSignalError`.
///
/// ```
/// # use lcms_rs::{cms_err, error::ErrorCode};
/// let e = cms_err!(ErrorCode::Range, "value {} out of range", 5);
/// assert_eq!(e.code, ErrorCode::Range);
/// ```
#[macro_export]
macro_rules! cms_err {
    ($code:expr, $($arg:tt)*) => {
        $crate::error::Error::new($code, format!($($arg)*))
    };
}

