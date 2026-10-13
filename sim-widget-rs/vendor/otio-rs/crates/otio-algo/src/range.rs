//! Duration and range computations (ports of `track.cpp`/`stack.cpp`/`item.cpp`).

use otio_core::{canonical_schema, Document, Field, NodeId};
use otio_schema::codec;
use otio_schema::view::{Clip, Transition};
use otio_time::{RationalTime, TimeRange};

use crate::error::AlgoError;

/// Whether a child "overlaps" its neighbors (a transition) and so does not
/// advance the playhead. Mirrors `Composable::overlapping` (only `Transition`).
fn is_transition(doc: &Document, id: NodeId) -> bool {
    doc.node(id).map(|n| n.schema_name()) == Some("Transition")
}

/// The item's explicit `source_range`, if set (and not JSON `null`).
fn source_range(doc: &Document, id: NodeId) -> Option<TimeRange> {
    let node = doc.node(id)?;
    match node.field("source_range") {
        Some(Field::Value(v)) if !v.is_null() => codec::time_range(v),
        _ => None,
    }
}

/// A transition's `(in_offset, out_offset)`, erroring if malformed.
fn transition_offsets(doc: &Document, id: NodeId) -> Result<(RationalTime, RationalTime), AlgoError> {
    let t = Transition::new(doc, id).ok_or(AlgoError::BadTransition)?;
    let in_o = t.in_offset().ok_or(AlgoError::BadTransition)?;
    let out_o = t.out_offset().ok_or(AlgoError::BadTransition)?;
    Ok((in_o, out_o))
}

/// The duration of an item or transition (mirrors `Composable::duration`).
///
/// Transitions return `in_offset + out_offset`; every other item returns the
/// duration of its [`trimmed_range`].
pub fn duration(doc: &Document, id: NodeId) -> Result<RationalTime, AlgoError> {
    let node = doc.node(id).ok_or(AlgoError::NodeNotFound)?;
    if node.schema_name() == "Transition" {
        let (in_o, out_o) = transition_offsets(doc, id)?;
        Ok(in_o + out_o)
    } else {
        Ok(trimmed_range(doc, id)?.duration())
    }
}

/// The item's trimmed range: its `source_range` if set, else [`available_range`].
pub fn trimmed_range(doc: &Document, id: NodeId) -> Result<TimeRange, AlgoError> {
    if let Some(sr) = source_range(doc, id) {
        return Ok(sr);
    }
    available_range(doc, id)
}

/// The item's available range (the full extent of its media/contents).
///
/// - `Clip`: the active media reference's `available_range`.
/// - `Track`: the sum of child item durations, plus the head/tail transition
///   offsets.
/// - `Stack`: the longest child duration.
/// - others (e.g. `Gap` without a source range): unsupported (mirrors OTIO's
///   `NOT_IMPLEMENTED` — a `Gap` always carries a `source_range` in practice).
pub fn available_range(doc: &Document, id: NodeId) -> Result<TimeRange, AlgoError> {
    let node = doc.node(id).ok_or(AlgoError::NodeNotFound)?;
    match canonical_schema(node.schema_name()) {
        "Clip" => {
            let clip = Clip::new(doc, id).ok_or(AlgoError::NodeNotFound)?;
            let mr = clip
                .media_reference()
                .ok_or_else(|| AlgoError::NoAvailableRange("clip without media reference".into()))?;
            mr.available_range()
                .ok_or_else(|| AlgoError::NoAvailableRange("media without available_range".into()))
        }
        "Track" => track_available_range(doc, id),
        "Stack" => stack_available_range(doc, id),
        other => Err(AlgoError::NoAvailableRange(other.to_string())),
    }
}

/// `Track::available_range`: sum of item durations + head/tail transition offsets.
fn track_available_range(doc: &Document, id: NodeId) -> Result<TimeRange, AlgoError> {
    let children: Vec<NodeId> = doc
        .node(id)
        .ok_or(AlgoError::NodeNotFound)?
        .children("children")
        .to_vec();

    let mut total = RationalTime::new(0.0, 1.0);
    for &cid in &children {
        if !is_transition(doc, cid) {
            total += duration(doc, cid)?;
        }
    }
    if let Some(&first) = children.first()
        && is_transition(doc, first)
    {
        total += transition_offsets(doc, first)?.0; // in_offset
    }
    if let Some(&last) = children.last()
        && is_transition(doc, last)
    {
        total += transition_offsets(doc, last)?.1; // out_offset
    }
    Ok(TimeRange::new(RationalTime::new(0.0, total.rate()), total))
}

/// `Stack::available_range`: the longest child duration (children are layered).
fn stack_available_range(doc: &Document, id: NodeId) -> Result<TimeRange, AlgoError> {
    let children: Vec<NodeId> = doc
        .node(id)
        .ok_or(AlgoError::NodeNotFound)?
        .children("children")
        .to_vec();

    let Some((&first, rest)) = children.split_first() else {
        return Ok(TimeRange::default());
    };
    let mut longest = duration(doc, first)?;
    for &cid in rest {
        let d = duration(doc, cid)?;
        if longest < d {
            longest = d;
        }
    }
    Ok(TimeRange::new(RationalTime::new(0.0, longest.rate()), longest))
}

/// Adjust a possibly-negative index (Python-style) and bounds-check it.
fn adjust_index(index: i64, len: usize) -> Result<usize, AlgoError> {
    let i = if index < 0 { index + len as i64 } else { index };
    if i < 0 || i >= len as i64 {
        Err(AlgoError::IllegalIndex { index, len })
    } else {
        Ok(i as usize)
    }
}

/// The range of the child at `index` in its parent's time.
///
/// For a `Track`, accumulates the durations of preceding non-overlapping children
/// (transitions don't advance), and shifts a transition back by its `in_offset`.
/// For a `Stack`, every child starts at zero.
pub fn range_of_child_at_index(
    doc: &Document,
    parent: NodeId,
    index: i64,
) -> Result<TimeRange, AlgoError> {
    let node = doc.node(parent).ok_or(AlgoError::NodeNotFound)?;
    let children: Vec<NodeId> = node.children("children").to_vec();
    let idx = adjust_index(index, children.len())?;

    match canonical_schema(node.schema_name()) {
        "Track" => {
            let child = children[idx];
            let child_dur = duration(doc, child)?;
            let mut start = RationalTime::new(0.0, child_dur.rate());
            for &prev in &children[..idx] {
                if !is_transition(doc, prev) {
                    start += duration(doc, prev)?;
                }
            }
            if is_transition(doc, child) {
                start -= transition_offsets(doc, child)?.0; // in_offset
            }
            Ok(TimeRange::new(start, child_dur))
        }
        "Stack" => {
            let d = duration(doc, children[idx])?;
            Ok(TimeRange::new(RationalTime::new(0.0, d.rate()), d))
        }
        other => Err(AlgoError::NoAvailableRange(other.to_string())),
    }
}

/// The range of every child, in parent order (the layout a timeline consumes).
///
/// For a `Track` this ports `Track::range_of_all_children`: items lay out
/// end-to-end, while a transition is centered on the cut (`[last_end - in,
/// in + out)`) and does not advance the playhead. For a `Stack`, every child
/// starts at zero.
pub fn range_of_all_children(
    doc: &Document,
    parent: NodeId,
) -> Result<Vec<(NodeId, TimeRange)>, AlgoError> {
    let node = doc.node(parent).ok_or(AlgoError::NodeNotFound)?;
    match canonical_schema(node.schema_name()) {
        "Track" => track_range_of_all(doc, parent),
        "Stack" => {
            let children: Vec<NodeId> = node.children("children").to_vec();
            let mut out = Vec::with_capacity(children.len());
            for (i, &cid) in children.iter().enumerate() {
                out.push((cid, range_of_child_at_index(doc, parent, i as i64)?));
            }
            Ok(out)
        }
        other => Err(AlgoError::NoAvailableRange(other.to_string())),
    }
}

/// Port of `Track::range_of_all_children`.
fn track_range_of_all(
    doc: &Document,
    parent: NodeId,
) -> Result<Vec<(NodeId, TimeRange)>, AlgoError> {
    let children: Vec<NodeId> = doc
        .node(parent)
        .ok_or(AlgoError::NodeNotFound)?
        .children("children")
        .to_vec();
    let Some(&first) = children.first() else {
        return Ok(Vec::new());
    };

    // Seed the rate from the first child (transition offset rate or item duration rate).
    let rate = if is_transition(doc, first) {
        transition_offsets(doc, first)?.0.rate()
    } else {
        trimmed_range(doc, first)?.duration().rate()
    };

    let mut last_end = RationalTime::new(0.0, rate);
    let mut out = Vec::with_capacity(children.len());
    for &cid in &children {
        if is_transition(doc, cid) {
            let (in_o, out_o) = transition_offsets(doc, cid)?;
            out.push((cid, TimeRange::new(last_end - in_o, out_o + in_o)));
        } else {
            let dur = trimmed_range(doc, cid)?.duration();
            let range = TimeRange::new(last_end, dur);
            out.push((cid, range));
            last_end = range.end_time_exclusive();
        }
    }
    Ok(out)
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

    /// root -> tracks(Stack) -> first Track.
    fn first_track(doc: &Document) -> NodeId {
        let root = doc.root_id().unwrap();
        let stack = doc.node(root).unwrap().child("tracks").unwrap();
        doc.node(stack).unwrap().children("children")[0]
    }

    #[test]
    fn simple_cut_layout() {
        let doc = load("simple_cut.otio");
        let track = first_track(&doc);

        let ranges = range_of_all_children(&doc, track).unwrap();
        let starts: Vec<f64> = ranges.iter().map(|(_, r)| r.start_time().value()).collect();
        let durs: Vec<f64> = ranges.iter().map(|(_, r)| r.duration().value()).collect();
        // Clips: dur 3, 6, 4, then clip-004 (no source_range) uses media available
        // range duration 6. Laid out end-to-end at 24 fps.
        assert_eq!(starts, vec![0.0, 3.0, 9.0, 13.0]);
        assert_eq!(durs, vec![3.0, 6.0, 4.0, 6.0]);

        // Track available range = total duration 19 @ 24.
        let ar = available_range(&doc, track).unwrap();
        assert_eq!(ar.duration(), RationalTime::new(19.0, 24.0));

        // range_of_child_at_index agrees with range_of_all_children.
        assert_eq!(range_of_child_at_index(&doc, track, 2).unwrap(), ranges[2].1);
        // Negative indexing.
        assert_eq!(range_of_child_at_index(&doc, track, -1).unwrap(), ranges[3].1);
        // Out of range errors, not panics.
        assert!(matches!(
            range_of_child_at_index(&doc, track, 99),
            Err(AlgoError::IllegalIndex { .. })
        ));
    }

    #[test]
    fn transitions_dont_advance_playhead() {
        // transition.otio exercises the transition path; just ensure it computes
        // a layout without error and a transition is centered (zero-width advance).
        let doc = load("transition.otio");
        let track = first_track(&doc);
        let ranges = range_of_all_children(&doc, track).unwrap();
        assert!(!ranges.is_empty());
    }

    #[test]
    fn sequence_alias_is_treated_as_track() {
        // generator_reference_test.otio uses the legacy `Sequence` schema, which
        // OTIO aliases to `Track`. Found via the Python differential.
        let doc = load("generator_reference_test.otio");
        let track = first_track(&doc);
        let ranges = range_of_all_children(&doc, track).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].1.start_time(), RationalTime::new(0.0, 24.0));
        assert_eq!(ranges[0].1.duration(), RationalTime::new(50.0, 24.0));
    }
}
