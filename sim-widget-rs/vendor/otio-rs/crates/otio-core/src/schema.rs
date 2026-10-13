//! Schema-name helpers.
//!
//! OTIO tags every serialized object with `"OTIO_SCHEMA": "Name.Version"`. Most
//! tagged objects are `SerializableObject`s (tree nodes); a small fixed set are
//! *value types* that are serialized with a schema tag but carry no children and
//! are kept inline as plain values.

use crate::error::Error;

/// The value-type schemas: serialized with an `OTIO_SCHEMA` tag, but stored
/// inline (never promoted to arena nodes).
pub fn is_value_schema(name: &str) -> bool {
    matches!(
        name,
        "RationalTime"
            | "TimeRange"
            | "TimeTransform"
            | "Color"
            | "Box2d"
            | "V2d"
            // Serialized inline as a reference id, not a tree node. Decoded to a
            // value in the reference (deserialization.cpp: `SerializableObjectRef.1`).
            // Emitted for shared objects under OTIO_INSTANCING_SUPPORT; keeping it
            // here prevents misclassifying a ref as a phantom node.
            | "SerializableObjectRef"
    )
}

/// Map a legacy schema-name alias to its canonical name.
///
/// OTIO registers a few aliases via `register_type_from_existing_type`
/// (typeRegistry.cpp): `Sequence`->`Track`, `Filler`->`Gap`, and
/// `SerializeableCollection`->`SerializableCollection` (note the historical
/// misspelling). Storage keeps the original name verbatim (lossless round-trip);
/// this mapping is only for dispatch / typed views / algorithms.
pub fn canonical_schema(name: &str) -> &str {
    match name {
        "Sequence" => "Track",
        "Filler" => "Gap",
        "SerializeableCollection" => "SerializableCollection",
        other => other,
    }
}

/// Split an `OTIO_SCHEMA` tag (`"Track.1"`) into `(name, version)`.
pub fn parse_schema(tag: &str) -> Result<(String, u32), Error> {
    tag.rsplit_once('.')
        .and_then(|(name, ver)| ver.parse::<u32>().ok().map(|v| (name.to_string(), v)))
        .ok_or_else(|| Error::BadSchema(tag.to_string()))
}
