//! `otio-time` — pure-Rust port of OpenTimelineIO's `opentime` module.
//!
//! Provides the three value types at the heart of every OTIO timeline:
//!
//! - [`RationalTime`] — a time expressed as `value` samples at a `rate` (fps).
//! - [`TimeRange`] — a `start_time` + `duration`, with Allen's 13 interval
//!   relations.
//! - [`TimeTransform`] — a 1-D affine time transform (offset + scale + rate).
//!
//! These mirror `_ref/OpenTimelineIO/src/opentime` (header + `rationalTime.cpp`)
//! semantics exactly, including SMPTE timecode and drop-frame conversion, so
//! files and computations round-trip with the reference implementation. The
//! types are `Copy` value types with `const fn` constructors where the C++ is
//! `constexpr`.
//!
//! Naming: C++ overloads (e.g. `contains(RationalTime)` vs `contains(TimeRange)`)
//! become distinctly-named methods in Rust (`contains` vs `contains_range`); the
//! per-method docs note the C++ counterpart.

mod error;
mod rational_time;
mod time_range;
mod time_transform;

pub use error::TimeError;
pub use rational_time::{IsDropFrameRate, RationalTime};
pub use time_range::{TimeRange, DEFAULT_EPSILON_S};
pub use time_transform::TimeTransform;
