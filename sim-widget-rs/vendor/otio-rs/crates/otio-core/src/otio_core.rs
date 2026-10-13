//! `otio-core` — the OpenTimelineIO object model and `.otio` JSON round-trip.
//!
//! A document is a flat generational arena ([`Document`]) of schema [`Node`]s.
//! OTIO is an ordered single-parent tree, so each node carries a `parent` and an
//! **ordered** list of fields. A [`Field`] is either:
//!
//! - [`Field::Value`] — a verbatim JSON value (scalars, metadata, and the OTIO
//!   value-type schemas `RationalTime`/`TimeRange`/`Color`/…), or
//! - [`Field::Child`] / [`Field::Children`] — a structural link to nested
//!   `SerializableObject` node(s) (e.g. `tracks`, `children`, `media_reference`,
//!   `effects`, `markers`).
//!
//! Because every non-structural value is preserved verbatim and structure is
//! reconstructed from the links, JSON round-trips losslessly — there is no
//! hand-written per-schema serializer to drift out of sync with the reference.
//! Typed, schema-aware accessors are layered on top (the `otio-schema` crate).
//!
//! See `../../PLAN.md` (Phase 2) for the design rationale.

mod document;
mod error;
mod schema;

pub use document::{Document, Field, Node, NodeId};
pub use error::Error;
pub use schema::{canonical_schema, is_value_schema, parse_schema};
