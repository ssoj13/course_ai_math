//! `flatten_stack` and `track_trimmed_to_range` (ports of `stackAlgorithm.cpp`
//! and `trackAlgorithm.cpp`).
//!
//! These mutate the document: they clone tracks/subtrees and build a new
//! "Flattened" track. The intermediate clones (trimmed/padded tracks) are left
//! detached in the arena — a future arena-compaction pass can reclaim them.

use std::collections::HashMap;

use otio_core::{canonical_schema, Document, NodeId};
use otio_schema::{build, edit};
use otio_time::{RationalTime, TimeRange, DEFAULT_EPSILON_S};

use crate::error::AlgoError;
use crate::range::{duration, range_of_all_children, trimmed_range};

/// Whether a node is a transition (canonicalized).
fn is_transition(doc: &Document, id: NodeId) -> bool {
    doc.node(id)
        .map(|n| canonical_schema(n.schema_name()) == "Transition")
        .unwrap_or(false)
}

/// `Item::visible` semantics: a `Gap` is never visible; otherwise follow the
/// `enabled` flag (default `true`). Used to decide whether the top track's
/// content shows through or a lower track fills the hole.
fn visible(doc: &Document, id: NodeId) -> bool {
    let Some(node) = doc.node(id) else {
        return false;
    };
    if canonical_schema(node.schema_name()) == "Gap" {
        return false;
    }
    node.field("enabled")
        .and_then(|f| match f {
            otio_core::Field::Value(v) => v.as_bool(),
            _ => None,
        })
        .unwrap_or(true)
}

/// Find a child's precomputed range in a `range_of_all_children` result.
fn range_of(map: &[(NodeId, TimeRange)], child: NodeId) -> Option<TimeRange> {
    map.iter().find(|(id, _)| *id == child).map(|(_, r)| *r)
}

/// Create a `Gap` node spanning `dur` (source_range starting at zero).
///
/// Uses the canonical `build::gap` so the padding gap carries the full default
/// field set (name/metadata/effects/markers/enabled/color) the reference emits,
/// not just `source_range`.
fn make_gap(doc: &mut Document, dur: RationalTime) -> NodeId {
    let range = TimeRange::new(RationalTime::new(0.0, dur.rate()), dur);
    build::gap(doc, range)
}

/// Clone `track` trimmed to `trim_range`: drop children entirely outside the
/// range and trim the source range of partially-overlapping items. Errors if the
/// trim would fall in the middle of a transition. Returns the new track's id.
///
/// Port of `track_trimmed_to_range` (trackAlgorithm.cpp).
pub fn track_trimmed_to_range(
    doc: &mut Document,
    track: NodeId,
    trim_range: TimeRange,
) -> Result<NodeId, AlgoError> {
    let new_track = doc.deep_clone(track).ok_or(AlgoError::NodeNotFound)?;
    let map = range_of_all_children(doc, new_track)?;
    let children: Vec<NodeId> = doc
        .node(new_track)
        .ok_or(AlgoError::NodeNotFound)?
        .children("children")
        .to_vec();

    // Iterate in reverse so removals don't shift the indices we still need.
    for i in (0..children.len()).rev() {
        let child = children[i];
        let child_range = range_of(&map, child).ok_or_else(|| {
            AlgoError::TypeMismatch("child missing from track range map".into())
        })?;

        if !trim_range.intersects(child_range, DEFAULT_EPSILON_S) {
            doc.remove_child(new_track, i);
        } else if !trim_range.contains_range(child_range, DEFAULT_EPSILON_S) {
            if is_transition(doc, child) {
                return Err(AlgoError::CannotTrimTransition);
            }
            let mut csr = trimmed_range(doc, child)?;
            if trim_range.start_time() > child_range.start_time() {
                let amount = trim_range.start_time() - child_range.start_time();
                csr = TimeRange::new(csr.start_time() + amount, csr.duration() - amount);
            }
            let trim_end = trim_range.end_time_exclusive();
            let child_end = child_range.end_time_exclusive();
            if trim_end < child_end {
                let amount = child_end - trim_end;
                csr = TimeRange::new(csr.start_time(), csr.duration() - amount);
            }
            edit::set_source_range(doc, child, Some(csr));
        }
        // else: fully contained — keep as-is.
    }

    Ok(new_track)
}

/// Flatten a `Stack` of tracks into a single new "Flattened" `Track`.
///
/// The top track wins where it has visible content; gaps/invisible items are
/// filled by the track(s) below, trimmed to the hole. Port of `flatten_stack`
/// (stackAlgorithm.cpp). Returns the new track's id (added to `doc`).
pub fn flatten_stack(doc: &mut Document, stack: NodeId) -> Result<NodeId, AlgoError> {
    // 1. Collect enabled track children (error on any non-track child).
    let stack_children: Vec<NodeId> = doc
        .node(stack)
        .ok_or(AlgoError::NodeNotFound)?
        .children("children")
        .to_vec();
    let mut tracks = Vec::with_capacity(stack_children.len());
    for cid in stack_children {
        let node = doc.node(cid).ok_or(AlgoError::NodeNotFound)?;
        if canonical_schema(node.schema_name()) != "Track" {
            return Err(AlgoError::TypeMismatch(format!(
                "flatten_stack: expected Track, found '{}'",
                node.schema_name()
            )));
        }
        if visible_track(doc, cid) {
            tracks.push(cid);
        }
    }

    // 2. Pad shorter tracks with a tail gap so all tracks share the longest duration.
    normalize_track_lengths(doc, &mut tracks)?;

    // 3. Build the empty output track. The reference creates a plain
    //    `otio.schema.Track()` (default kind Video), so use the canonical
    //    builder to emit the full default field set, not a bare node.
    let flat = build::track(doc, "Flattened", "Video");

    // 4. Fill it, top track first.
    let mut cache: HashMap<NodeId, Vec<(NodeId, TimeRange)>> = HashMap::new();
    flatten_next_item(doc, &mut cache, flat, &tracks, -1, None)?;
    Ok(flat)
}

/// A track's own enabled flag (tracks are skipped entirely when disabled).
fn visible_track(doc: &Document, id: NodeId) -> bool {
    doc.node(id)
        .and_then(|n| n.field("enabled"))
        .and_then(|f| match f {
            otio_core::Field::Value(v) => v.as_bool(),
            _ => None,
        })
        .unwrap_or(true)
}

/// Pad each shorter track (cloning it) with a trailing gap to the longest length.
fn normalize_track_lengths(doc: &mut Document, tracks: &mut [NodeId]) -> Result<(), AlgoError> {
    let mut longest = RationalTime::new(0.0, 1.0);
    for &t in tracks.iter() {
        let d = duration(doc, t)?;
        if longest < d {
            longest = d;
        }
    }
    for slot in tracks.iter_mut() {
        let td = duration(doc, *slot)?;
        if td < longest {
            let padded = doc.deep_clone(*slot).ok_or(AlgoError::NodeNotFound)?;
            let gap = make_gap(doc, longest - td);
            doc.append_child(padded, gap)
                .map_err(|e| AlgoError::TypeMismatch(e.to_string()))?;
            *slot = padded;
        }
    }
    Ok(())
}

/// Recursive core of `flatten_stack` (port of `_flatten_next_item`).
fn flatten_next_item(
    doc: &mut Document,
    cache: &mut HashMap<NodeId, Vec<(NodeId, TimeRange)>>,
    flat_track: NodeId,
    tracks: &[NodeId],
    track_index: i64,
    trim_range: Option<TimeRange>,
) -> Result<(), AlgoError> {
    let track_index = if track_index < 0 {
        tracks.len() as i64 - 1
    } else {
        track_index
    };
    if track_index < 0 {
        return Ok(());
    }

    // Trim the track to the hole being filled, if any (creates a clone).
    let mut track = tracks[track_index as usize];
    let mut trimmed_clone = None;
    if let Some(tr) = trim_range {
        track = track_trimmed_to_range(doc, track, tr)?;
        trimmed_clone = Some(track);
    }

    let map = match cache.get(&track) {
        Some(m) => m.clone(),
        None => {
            let m = range_of_all_children(doc, track)?;
            cache.insert(track, m.clone());
            m
        }
    };

    let children: Vec<NodeId> = doc
        .node(track)
        .ok_or(AlgoError::NodeNotFound)?
        .children("children")
        .to_vec();

    for child in children {
        let item_like = !is_transition(doc, child);
        // Top track's visible content (and all transitions, and the bottom track)
        // are copied directly; otherwise a lower track shows through the hole.
        if !item_like || visible(doc, child) || track_index == 0 {
            let clone = doc.deep_clone(child).ok_or(AlgoError::NodeNotFound)?;
            doc.append_child(flat_track, clone)
                .map_err(|e| AlgoError::TypeMismatch(e.to_string()))?;
        } else {
            let mut hole = range_of(&map, child)
                .ok_or_else(|| AlgoError::TypeMismatch("child missing from range map".into()))?;
            if let Some(tr) = trim_range {
                hole = TimeRange::new(hole.start_time() + tr.start_time(), hole.duration());
            }
            flatten_next_item(doc, cache, flat_track, tracks, track_index - 1, Some(hole))?;
        }
    }

    // Drop the trimmed clone's cache entry (matches the C++ map cleanup).
    if let Some(tc) = trimmed_clone {
        cache.remove(&tc);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn load(name: &str) -> Document {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../_ref/OpenTimelineIO/tests/sample_data")
            .join(name);
        Document::from_json_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn tracks_stack(doc: &Document) -> NodeId {
        let root = doc.root_id().unwrap();
        doc.node(root).unwrap().child("tracks").unwrap()
    }

    #[test]
    fn flatten_single_track_is_identity_layout() {
        // A one-track stack flattens to the same child layout.
        let mut doc = load("simple_cut.otio");
        let stack = tracks_stack(&doc);
        let before = range_of_all_children(&doc, doc.node(stack).unwrap().children("children")[0])
            .unwrap()
            .iter()
            .map(|(_, r)| (r.start_time().value(), r.duration().value()))
            .collect::<Vec<_>>();

        let flat = flatten_stack(&mut doc, stack).unwrap();
        assert_eq!(doc.node(flat).unwrap().schema_name(), "Track");
        let after = range_of_all_children(&doc, flat)
            .unwrap()
            .iter()
            .map(|(_, r)| (r.start_time().value(), r.duration().value()))
            .collect::<Vec<_>>();
        assert_eq!(before, after);
    }

    #[test]
    fn flatten_multitrack_runs_and_fills() {
        // Multi-track stack: flatten must succeed and produce a non-empty track
        // whose total duration matches the longest input track.
        let mut doc = load("multitrack.otio");
        let stack = tracks_stack(&doc);
        let stack_dur = available_range_dur(&doc, stack);

        let flat = flatten_stack(&mut doc, stack).unwrap();
        let ranges = range_of_all_children(&doc, flat).unwrap();
        assert!(!ranges.is_empty());
        let flat_dur = duration(&doc, flat).unwrap().value();
        assert!((flat_dur - stack_dur).abs() < 1e-6, "{flat_dur} vs {stack_dur}");
    }

    fn available_range_dur(doc: &Document, stack: NodeId) -> f64 {
        crate::range::available_range(doc, stack)
            .unwrap()
            .duration()
            .value()
    }
}
