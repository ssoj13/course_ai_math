//! Hierarchy traversal: `index_of_child`, `range_of_child`, and `transformed_time`
//! (ports of `composition.cpp` / `item.cpp`).
//!
//! These map a time or range between coordinate spaces of items at different
//! depths in the tree by walking parent pointers.

use otio_core::{Document, NodeId};
use otio_time::{RationalTime, TimeRange};

use crate::error::AlgoError;
use crate::range::{range_of_child_at_index, trimmed_range};

/// A node's parent, if any.
fn parent_of(doc: &Document, id: NodeId) -> Option<NodeId> {
    doc.node(id).and_then(|n| n.parent())
}

/// The topmost ancestor reachable by following parent pointers.
fn highest_ancestor(doc: &Document, mut id: NodeId) -> NodeId {
    while let Some(p) = parent_of(doc, id) {
        id = p;
    }
    id
}

/// The index of `child` within `parent`'s `children`, or `None` if absent.
pub fn index_of_child(doc: &Document, parent: NodeId, child: NodeId) -> Option<usize> {
    doc.node(parent)?
        .children("children")
        .iter()
        .position(|&c| c == child)
}

/// The chain of compositions from `child`'s parent up to and including `ancestor`.
fn path_from_child(
    doc: &Document,
    ancestor: NodeId,
    child: NodeId,
) -> Result<Vec<NodeId>, AlgoError> {
    let mut current = parent_of(doc, child).ok_or(AlgoError::NotDescended)?;
    let mut path = vec![current];
    while current != ancestor {
        current = parent_of(doc, current).ok_or(AlgoError::NotDescended)?;
        path.push(current);
    }
    Ok(path)
}

/// The range of `child` expressed in `ancestor`'s coordinate space.
///
/// `ancestor` must be a composition that (transitively) contains `child`. Port of
/// `Composition::range_of_child`: the duration is the child's innermost range,
/// the start accumulates each enclosing composition's offset.
pub fn range_of_child(
    doc: &Document,
    ancestor: NodeId,
    child: NodeId,
) -> Result<TimeRange, AlgoError> {
    let path = path_from_child(doc, ancestor, child)?;
    let mut result: Option<TimeRange> = None;
    let mut current = child;
    for parent in path {
        let index = index_of_child(doc, parent, current).ok_or(AlgoError::NotDescended)?;
        let parent_range = range_of_child_at_index(doc, parent, index as i64)?;
        result = Some(match result {
            None => parent_range,
            Some(r) => TimeRange::new(r.start_time() + parent_range.start_time(), r.duration()),
        });
        current = parent;
    }
    result.ok_or(AlgoError::NotDescended)
}

/// Map `time` from `from_item`'s coordinate space into `to_item`'s.
///
/// Port of `Item::transformed_time`: walk up from `from_item` to the common
/// ancestor (or the tree root), then back down to `to_item`.
pub fn transformed_time(
    doc: &Document,
    from_item: NodeId,
    time: RationalTime,
    to_item: NodeId,
) -> Result<RationalTime, AlgoError> {
    if from_item == to_item {
        return Ok(time);
    }
    let root = highest_ancestor(doc, from_item);
    let mut result = time;

    // Walk up from `from_item` toward the root, stopping early at `to_item`.
    let mut item = from_item;
    while item != root && item != to_item {
        let parent = parent_of(doc, item).ok_or(AlgoError::NotDescended)?;
        result -= trimmed_range(doc, item)?.start_time();
        result += range_of_child(doc, parent, item)?.start_time();
        item = parent;
    }
    let ancestor = item;

    // Walk up from `to_item` toward that ancestor, undoing each offset.
    let mut item = to_item;
    while item != root && item != ancestor {
        let parent = parent_of(doc, item).ok_or(AlgoError::NotDescended)?;
        result += trimmed_range(doc, item)?.start_time();
        result -= range_of_child(doc, parent, item)?.start_time();
        item = parent;
    }

    Ok(result)
}

/// Map `range` from `from_item`'s coordinate space into `to_item`'s.
///
/// Only the start time is transformed; the duration is unchanged (port of
/// `Item::transformed_time_range`).
pub fn transformed_time_range(
    doc: &Document,
    from_item: NodeId,
    range: TimeRange,
    to_item: NodeId,
) -> Result<TimeRange, AlgoError> {
    Ok(TimeRange::new(
        transformed_time(doc, from_item, range.start_time(), to_item)?,
        range.duration(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::range::range_of_all_children;
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
    fn transform_clip_to_stack_matches_track_position() {
        // For a flat (one nesting level) timeline, a clip's local start mapped to
        // the top stack equals its start position on the track.
        let doc = load("simple_cut.otio");
        let stack = tracks_stack(&doc);
        let track = doc.node(stack).unwrap().children("children")[0];
        let layout = range_of_all_children(&doc, track).unwrap();

        for &(clip, range) in &layout {
            let local_start = trimmed_range(&doc, clip).unwrap().start_time();
            let mapped = transformed_time(&doc, clip, local_start, stack).unwrap();
            // Stack adds zero offset, so the mapped time is the clip's track start.
            assert_eq!(mapped, range.start_time());
        }
    }

    #[test]
    fn transform_to_self_is_identity() {
        let doc = load("simple_cut.otio");
        let stack = tracks_stack(&doc);
        let track = doc.node(stack).unwrap().children("children")[0];
        let clip = doc.node(track).unwrap().children("children")[0];
        let t = RationalTime::new(2.0, 24.0);
        assert_eq!(transformed_time(&doc, clip, t, clip).unwrap(), t);
    }
}
