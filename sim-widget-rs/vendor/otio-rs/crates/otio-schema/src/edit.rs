//! Typed setters: write strong values back into the otio-core field store.
//!
//! Setters operate on `(&mut Document, NodeId)` (get an id from any view's
//! `id()`). They encode the typed value via [`crate::codec`] and replace the
//! field. Optional fields follow OTIO semantics: cleared to JSON `null` (the
//! reference's `optional<T>` writers emit `null`, never omit the key — see
//! `serialization.cpp`), not removed.

use box_rs::RustBox;
use otio_core::{Document, NodeId};
use otio_time::TimeRange;
use serde_json::Value;

use crate::codec::{self, Color};

/// Set a plain-value field on a node. Returns `false` if the node is absent.
fn set(doc: &mut Document, id: NodeId, key: &str, value: Value) -> bool {
    match doc.node_mut(id) {
        Some(node) => {
            node.set_value(key, value);
            true
        }
        None => false,
    }
}

/// Set (or clear, as JSON `null`) an item's `source_range`.
pub fn set_source_range(doc: &mut Document, id: NodeId, range: Option<TimeRange>) -> bool {
    set(
        doc,
        id,
        "source_range",
        range.map_or(Value::Null, codec::time_range_value),
    )
}

/// Set an item's `enabled` flag.
pub fn set_enabled(doc: &mut Document, id: NodeId, enabled: bool) -> bool {
    set(doc, id, "enabled", Value::Bool(enabled))
}

/// Set an object's `name`.
pub fn set_name(doc: &mut Document, id: NodeId, name: &str) -> bool {
    set(doc, id, "name", Value::String(name.to_string()))
}

/// Set (or clear, as JSON `null`) an item's display `color`.
pub fn set_color(doc: &mut Document, id: NodeId, color: Option<&Color>) -> bool {
    set(doc, id, "color", color.map_or(Value::Null, codec::color_value))
}

/// Set a track's `kind` (e.g. `"Video"`/`"Audio"`).
pub fn set_track_kind(doc: &mut Document, id: NodeId, kind: &str) -> bool {
    set(doc, id, "kind", Value::String(kind.to_string()))
}

/// Replace an object's `metadata` from a box-rs container.
pub fn set_metadata(doc: &mut Document, id: NodeId, meta: &RustBox) -> bool {
    match doc.node_mut(id) {
        Some(node) => {
            node.set_metadata(meta);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{Clip, ItemView, NodeView};
    use otio_time::RationalTime;
    use std::fs;
    use std::path::PathBuf;

    fn load(name: &str) -> Document {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../_ref/OpenTimelineIO/tests/sample_data")
            .join(name);
        Document::from_json_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    /// root -> tracks(Stack) -> first Track -> first child Clip.
    fn first_clip_id(doc: &Document) -> NodeId {
        let root = doc.root_id().unwrap();
        let stack = doc.node(root).unwrap().child("tracks").unwrap();
        let track = doc.node(stack).unwrap().children("children")[0];
        doc.node(track).unwrap().children("children")[0]
    }

    #[test]
    fn setters_persist_through_serialization() {
        let mut doc = load("simple_cut.otio");
        let clip = first_clip_id(&doc);

        assert!(set_name(&mut doc, clip, "Renamed"));
        assert!(set_enabled(&mut doc, clip, false));
        let tr = TimeRange::new(RationalTime::new(5.0, 24.0), RationalTime::new(2.0, 24.0));
        assert!(set_source_range(&mut doc, clip, Some(tr)));

        // Visible immediately via a typed view.
        let view = Clip::new(&doc, clip).unwrap();
        assert_eq!(view.name(), Some("Renamed"));
        assert!(!view.enabled());
        assert_eq!(view.source_range(), Some(tr));

        // Survives a serialize -> parse cycle.
        let out = doc.to_json_string().unwrap();
        let doc2 = Document::from_json_str(&out).unwrap();
        let view2 = Clip::new(&doc2, first_clip_id(&doc2)).unwrap();
        assert_eq!(view2.name(), Some("Renamed"));
        assert!(!view2.enabled());
        assert_eq!(view2.source_range(), Some(tr));
    }

    #[test]
    fn clear_source_range_writes_null() {
        let mut doc = load("simple_cut.otio");
        let clip = first_clip_id(&doc);
        assert!(set_source_range(&mut doc, clip, None));
        assert_eq!(Clip::new(&doc, clip).unwrap().source_range(), None);

        // Optional fields are serialized as JSON null (present), matching OTIO.
        let out = doc.to_json_string().unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let sr = &v["tracks"]["children"][0]["children"][0]["source_range"];
        assert!(sr.is_null(), "expected null, got {sr}");
    }
}
