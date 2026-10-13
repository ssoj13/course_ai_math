//! `otio-schema` — typed, schema-aware accessors over [`otio_core`].
//!
//! [`otio_core`] keeps every field verbatim (lossless round-trip). This crate
//! adds the *meaning*: a [`codec`] that turns the OTIO value-type schemas
//! (`RationalTime`/`TimeRange`/`TimeTransform`/`Color`) into [`otio_time`] types,
//! and lightweight borrow-only [`view`]s (`Timeline`, `Stack`, `Track`, `Clip`,
//! …) that decode those fields on demand.
//!
//! The views hold **no parallel copy** of the data — they borrow `&Document` and
//! `&Node` and decode lazily — so they cannot drift from what serializes. They
//! are read-only for now; mutation (writing typed values back through the field
//! store, with `box-rs` for metadata) is the next step.

pub mod build;
pub mod codec;
pub mod edit;
pub mod view;

pub use view::{
    AnyItem, Clip, Effect, Gap, Marker, MediaRef, Stack, Timeline, Track, Transition,
};
