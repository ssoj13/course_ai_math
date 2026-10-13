//! Canonical node builders.
//!
//! Construct fully-populated OTIO nodes whose field sets match what the reference
//! library (OpenTimelineIO 0.18) emits, so documents built programmatically (e.g.
//! by adapters) serialize identically. Field order is irrelevant (the round-trip
//! gate compares DOMs), but the exact field *set* and defaults matter.

use otio_core::{Document, NodeId};
use otio_time::{RationalTime, TimeRange};
use serde_json::{json, Value};

use crate::codec;

/// `Option<TimeRange>` -> JSON (a `TimeRange.1` object or `null`).
fn sr_value(source_range: Option<TimeRange>) -> Value {
    source_range.map_or(Value::Null, codec::time_range_value)
}

/// Set the metadata + Item base fields shared by clips, gaps, tracks and stacks.
fn set_item_base(doc: &mut Document, id: NodeId, name: &str, source_range: Option<TimeRange>) {
    if let Some(n) = doc.node_mut(id) {
        n.set_value("metadata", json!({}));
        n.set_value("name", json!(name));
        n.set_value("source_range", sr_value(source_range));
        n.set_value("effects", json!([]));
        n.set_value("markers", json!([]));
        n.set_value("enabled", json!(true));
        n.set_value("color", Value::Null);
    }
}

/// Set the MediaReference base fields.
fn set_mediaref_base(doc: &mut Document, id: NodeId) {
    if let Some(n) = doc.node_mut(id) {
        n.set_value("metadata", json!({}));
        n.set_value("name", json!(""));
        n.set_value("available_range", Value::Null);
        n.set_value("available_image_bounds", Value::Null);
    }
}

/// Build a `Timeline.1` containing an empty `tracks` `Stack`. Returns
/// `(timeline_id, stack_id)`. Does not set the document root.
pub fn timeline(doc: &mut Document, name: &str) -> (NodeId, NodeId) {
    let tl = doc.create_node("Timeline", 1);
    if let Some(n) = doc.node_mut(tl) {
        n.set_value("metadata", json!({}));
        n.set_value("name", json!(name));
        n.set_value("global_start_time", Value::Null);
    }
    let st = stack(doc, "tracks");
    let _ = doc.set_child(tl, "tracks", st);
    (tl, st)
}

/// Build an empty `SerializableCollection.1` (a flat container of objects).
pub fn serializable_collection(doc: &mut Document, name: &str) -> NodeId {
    let id = doc.create_node("SerializableCollection", 1);
    if let Some(n) = doc.node_mut(id) {
        n.set_value("metadata", json!({}));
        n.set_value("name", json!(name));
        n.set_value("children", json!([]));
    }
    id
}

/// Build an empty `Stack.1`.
pub fn stack(doc: &mut Document, name: &str) -> NodeId {
    let id = doc.create_node("Stack", 1);
    set_item_base(doc, id, name, None);
    if let Some(n) = doc.node_mut(id) {
        n.set_value("children", json!([]));
    }
    id
}

/// Build an empty `Track.1` of the given `kind` (`"Video"`/`"Audio"`).
pub fn track(doc: &mut Document, name: &str, kind: &str) -> NodeId {
    let id = doc.create_node("Track", 1);
    set_item_base(doc, id, name, None);
    if let Some(n) = doc.node_mut(id) {
        n.set_value("children", json!([]));
        n.set_value("kind", json!(kind));
    }
    id
}

/// Build a `Gap.1` spanning `source_range`.
pub fn gap(doc: &mut Document, source_range: TimeRange) -> NodeId {
    let id = doc.create_node("Gap", 1);
    set_item_base(doc, id, "", Some(source_range));
    id
}

/// Build a `Clip.2` whose active media reference is `media_ref`.
pub fn clip(
    doc: &mut Document,
    name: &str,
    source_range: Option<TimeRange>,
    media_ref: NodeId,
) -> NodeId {
    let id = doc.create_node("Clip", 2);
    set_item_base(doc, id, name, source_range);
    let _ = doc.set_child_in_map(id, "media_references", "DEFAULT_MEDIA", media_ref);
    if let Some(n) = doc.node_mut(id) {
        n.set_value("active_media_reference_key", json!("DEFAULT_MEDIA"));
    }
    id
}

/// Build an `ExternalReference.1` pointing at `target_url`.
pub fn external_reference(doc: &mut Document, target_url: &str) -> NodeId {
    let id = doc.create_node("ExternalReference", 1);
    set_mediaref_base(doc, id);
    if let Some(n) = doc.node_mut(id) {
        n.set_value("target_url", json!(target_url));
    }
    id
}

/// Build an `ImageSequenceReference.1` (frame_step 1, missing_frame_policy error).
#[allow(clippy::too_many_arguments)]
pub fn image_sequence_reference(
    doc: &mut Document,
    target_url_base: &str,
    name_prefix: &str,
    name_suffix: &str,
    start_frame: i64,
    frame_zero_padding: i64,
    rate: f64,
    available_range: TimeRange,
) -> NodeId {
    let id = doc.create_node("ImageSequenceReference", 1);
    if let Some(n) = doc.node_mut(id) {
        n.set_value("metadata", json!({}));
        n.set_value("name", json!(""));
        n.set_value("available_range", codec::time_range_value(available_range));
        n.set_value("available_image_bounds", Value::Null);
        n.set_value("target_url_base", json!(target_url_base));
        n.set_value("name_prefix", json!(name_prefix));
        n.set_value("name_suffix", json!(name_suffix));
        n.set_value("start_frame", json!(start_frame));
        n.set_value("frame_step", json!(1));
        n.set_value("rate", json!(rate));
        n.set_value("frame_zero_padding", json!(frame_zero_padding));
        n.set_value("missing_frame_policy", json!("error"));
    }
    id
}

/// Build a `MissingReference.1`.
pub fn missing_reference(doc: &mut Document) -> NodeId {
    let id = doc.create_node("MissingReference", 1);
    set_mediaref_base(doc, id);
    id
}

/// Build a `GeneratorReference.1` of the given `generator_kind` (e.g. `"black"`).
pub fn generator_reference(doc: &mut Document, generator_kind: &str) -> NodeId {
    let id = doc.create_node("GeneratorReference", 1);
    set_mediaref_base(doc, id);
    if let Some(n) = doc.node_mut(id) {
        n.set_value("generator_kind", json!(generator_kind));
        n.set_value("parameters", json!({}));
    }
    id
}

/// Build a `FreezeFrame.1` effect.
pub fn freeze_frame(doc: &mut Document) -> NodeId {
    let id = doc.create_node("FreezeFrame", 1);
    if let Some(n) = doc.node_mut(id) {
        n.set_value("metadata", json!({}));
        n.set_value("name", json!(""));
        n.set_value("effect_name", json!("FreezeFrame"));
        n.set_value("enabled", json!(true));
        n.set_value("time_scalar", json!(0.0));
    }
    id
}

/// Build a `LinearTimeWarp.1` effect.
pub fn linear_time_warp(doc: &mut Document, time_scalar: f64) -> NodeId {
    let id = doc.create_node("LinearTimeWarp", 1);
    if let Some(n) = doc.node_mut(id) {
        n.set_value("metadata", json!({}));
        n.set_value("name", json!(""));
        n.set_value("effect_name", json!("LinearTimeWarp"));
        n.set_value("enabled", json!(true));
        n.set_value("time_scalar", json!(time_scalar));
    }
    id
}

/// Build a `Marker.2`. `color` is the OTIO marker color enum (e.g. `"RED"`);
/// `raw_color` is the original string stored under `metadata.cmx_3600.color`.
pub fn marker(
    doc: &mut Document,
    name: &str,
    marked_range: TimeRange,
    color: &str,
    raw_color: &str,
) -> NodeId {
    let id = doc.create_node("Marker", 2);
    if let Some(n) = doc.node_mut(id) {
        n.set_value("metadata", json!({ "cmx_3600": { "color": raw_color } }));
        n.set_value("name", json!(name));
        n.set_value("color", json!(color));
        n.set_value("marked_range", codec::time_range_value(marked_range));
        n.set_value("comment", json!(""));
    }
    id
}

/// Build a `Transition.1`.
pub fn transition(
    doc: &mut Document,
    name: &str,
    transition_type: &str,
    in_offset: RationalTime,
    out_offset: RationalTime,
) -> NodeId {
    let id = doc.create_node("Transition", 1);
    if let Some(n) = doc.node_mut(id) {
        n.set_value("metadata", json!({}));
        n.set_value("name", json!(name));
        n.set_value("in_offset", codec::rational_time_value(in_offset));
        n.set_value("out_offset", codec::rational_time_value(out_offset));
        n.set_value("transition_type", json!(transition_type));
    }
    id
}
