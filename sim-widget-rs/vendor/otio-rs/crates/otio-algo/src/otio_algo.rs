//! `otio-algo` — range / layout algorithms over the [`otio_core`] model.
//!
//! Read-only computations that turn an OTIO tree into positional data:
//!
//! - [`duration`] / [`trimmed_range`] / [`available_range`] of an item.
//! - [`range_of_child_at_index`] — a child's range in its parent's time.
//! - [`range_of_all_children`] — every child's range (the layout a timeline
//!   widget consumes), with OTIO's transition handling (transitions overlap
//!   their neighbors and don't advance the playhead).
//!
//! Plus [`edit`] — the pure *edit* algebra (reorder / ripple / trim / blade /
//! slide / roll) over a slice of spans, with no document coupling. A contiguous
//! reorder there is total by construction, which is what makes a legal clip
//! drag unable to fail.
//!
//! Faithful ports of `track.cpp` / `stack.cpp` / `item.cpp` from the reference.
//! See `../../PLAN.md` (Phase 2 / otio-algo).

pub mod edit;
mod error;
mod flatten;
mod range;
mod transform;

pub use edit::{
    EditError, Reorder, blade, drop_index, reorder_contiguous, ripple_move, roll, slide, trim_in,
    trim_out,
};
pub use error::AlgoError;
pub use flatten::{flatten_stack, track_trimmed_to_range};
pub use range::{
    available_range, duration, range_of_all_children, range_of_child_at_index, trimmed_range,
};
pub use transform::{
    index_of_child, range_of_child, transformed_time, transformed_time_range,
};
