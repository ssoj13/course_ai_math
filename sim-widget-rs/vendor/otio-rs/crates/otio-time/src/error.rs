//! Error type for fallible time conversions.
//!
//! Mirrors the relevant members of `opentime::ErrorStatus::Outcome`. OTIO returns
//! a status struct + an "invalid" sentinel value; the Rust port returns
//! `Result<_, TimeError>` instead, which is the idiomatic equivalent.

use thiserror::Error;

/// Failure modes for timecode / time-string conversion.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum TimeError {
    /// The rate is not one of the SMPTE timecode rates.
    #[error("invalid timecode rate: {0}")]
    InvalidTimecodeRate(f64),

    /// The timecode string could not be parsed into HH:MM:SS:FF fields.
    #[error("invalid timecode string: '{0}'")]
    InvalidTimecodeString(String),

    /// The time string (`HH:MM:SS.sss`) was malformed.
    #[error("invalid time string: '{0}'")]
    InvalidTimeString(String),

    /// The frame field exceeds the nominal fps implied by the rate.
    #[error("timecode rate mismatch: '{timecode}' has frames beyond {max_frame}")]
    TimecodeRateMismatch {
        /// The offending timecode string.
        timecode: String,
        /// The largest valid frame index for the rate (`nominal_fps - 1`).
        max_frame: i32,
    },

    /// A negative time cannot be expressed as timecode.
    #[error("negative value not allowed for timecode")]
    NegativeValue,

    /// Drop-frame was requested for a rate that does not support it.
    #[error("rate {0} is not a valid drop frame rate")]
    InvalidRateForDropFrameTimecode(f64),
}
