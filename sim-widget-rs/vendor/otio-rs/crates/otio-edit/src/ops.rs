//! Edit operations — the verbs the widget invokes.
//!
//! Each op runs through [`Editor::edit`] so it is one atomic, undoable step. Ops
//! are free functions taking `&mut Editor` (matching the otio-algo style); they
//! compute positions/ranges from otio-algo + the model and apply changes only
//! via the recording [`Tx`]. This is the minimal proven v1 core; the full v1 set
//! (razor / overwrite / lift / trim / slip / tracks / markers) builds on the
//! same primitives — see PLAN.md.

use otio_algo::{available_range, range_of_all_children, trimmed_range};
use otio_core::{Document, NodeId, canonical_schema};
use otio_schema::codec;
use otio_time::{RationalTime, TimeRange};
use serde_json::{Value, json};

use crate::editor::Editor;
use crate::error::EditError;
use crate::tx::Tx;

/// Rename a node (`name` field).
pub fn set_name(ed: &mut Editor, id: NodeId, name: &str) -> Result<(), EditError> {
    ed.edit("set name", |tx| tx.set_value(id, "name", json!(name)))
}

/// Enable / disable an item (`enabled` field).
pub fn set_enabled(ed: &mut Editor, id: NodeId, enabled: bool) -> Result<(), EditError> {
    ed.edit("set enabled", |tx| {
        tx.set_value(id, "enabled", json!(enabled))
    })
}

/// Set (or clear) an item's `source_range`.
///
/// `None` writes JSON `null` (OTIO's "no source range"); `Some(range)` writes the
/// canonical RationalTime-pair encoding via the schema codec.
pub fn set_source_range(
    ed: &mut Editor,
    id: NodeId,
    range: Option<TimeRange>,
) -> Result<(), EditError> {
    let value = match range {
        Some(tr) => codec::time_range_value(tr),
        None => Value::Null,
    };
    ed.edit("set source range", |tx| {
        tx.set_value(id, "source_range", value)
    })
}

/// Insert `child` into `parent`'s `children` at `index` (clamped to the end).
pub fn insert_child(
    ed: &mut Editor,
    parent: NodeId,
    index: usize,
    child: NodeId,
) -> Result<(), EditError> {
    ed.edit("insert", |tx| tx.insert_child(parent, index, child))
}

/// Append `child` to the end of `parent`'s `children`.
pub fn append_child(ed: &mut Editor, parent: NodeId, child: NodeId) -> Result<(), EditError> {
    ed.edit("append", |tx| tx.append_child(parent, child))
}

/// Ripple-delete the child at `index`: remove it and let downstream items shift
/// earlier (closing the gap). Position is implied by order, so no range math is
/// needed. After removal, newly-adjacent `Gap`s are merged (and any zero-duration
/// gap dropped) so a track never carries two touching gaps.
pub fn ripple_delete(ed: &mut Editor, parent: NodeId, index: usize) -> Result<(), EditError> {
    ed.edit("ripple delete", |tx| {
        tx.remove_child(parent, index)?;
        merge_adjacent_gaps(tx, parent)?;
        Ok(())
    })
}

// ===========================================================================
// v1 edit ops — trims, slip, razor, lift, overwrite, tracks, markers.
//
// All range math reads via `tx.doc()` + otio-algo BEFORE any mutation; changes
// then go through `Tx`. Formulas follow PLAN.md "Edit ops — v1". Transition and
// media-bounds violations are hard errors (never silent clamps), unless the
// formula explicitly clamps.
// ===========================================================================

/// Whether `id` is a `Transition` (canonicalized).
fn is_transition(doc: &Document, id: NodeId) -> bool {
    doc.node(id)
        .map(|n| canonical_schema(n.schema_name()) == "Transition")
        .unwrap_or(false)
}

/// Whether `id` is a `Gap` (canonicalized).
fn is_gap(doc: &Document, id: NodeId) -> bool {
    doc.node(id)
        .map(|n| canonical_schema(n.schema_name()) == "Gap")
        .unwrap_or(false)
}

/// Locate a clip's track (its parent) and index within the track's `children`.
fn track_and_index(doc: &Document, clip: NodeId) -> Result<(NodeId, usize), EditError> {
    let track = doc
        .node(clip)
        .and_then(|n| n.parent())
        .ok_or(EditError::NodeNotFound)?;
    let idx = doc
        .node(track)
        .ok_or(EditError::NodeNotFound)?
        .children("children")
        .iter()
        .position(|&c| c == clip)
        .ok_or(EditError::NodeNotFound)?;
    Ok((track, idx))
}

/// Trim a clip's in-point by `delta`: `sr' = (s + delta, d - delta)`.
///
/// The out-point stays put, so positive `delta` shortens from the head. Errors
/// with [`EditError::OutOfMediaBounds`] if the new start would precede the media
/// `available_range` start or the new duration would be `<= 0`, and with
/// [`EditError::AcrossTransition`] if the previous sibling on the track is a
/// transition (trimming the head across a transition is disallowed).
pub fn trim_in(ed: &mut Editor, clip: NodeId, delta: RationalTime) -> Result<(), EditError> {
    ed.edit("trim in", |tx| {
        let tr = trimmed_range(tx.doc(), clip)?;
        let avail = available_range(tx.doc(), clip)?;
        let s = tr.start_time();
        let d = tr.duration();
        let new_start = s + delta;
        let new_dur = d - delta;
        if new_start < avail.start_time() || new_dur.value() <= 0.0 {
            return Err(EditError::OutOfMediaBounds);
        }
        let (track, idx) = track_and_index(tx.doc(), clip)?;
        if idx > 0 {
            let prev = tx
                .doc()
                .node(track)
                .map(|n| n.children("children")[idx - 1]);
            if let Some(prev) = prev
                && is_transition(tx.doc(), prev)
            {
                return Err(EditError::AcrossTransition);
            }
        }
        tx.set_value(
            clip,
            "source_range",
            codec::time_range_value(TimeRange::new(new_start, new_dur)),
        )
    })
}

/// Record-side trim of a clip's in-point while preserving its timeline out-point.
///
/// Positive `delta` moves the record in-point right: the clip source range becomes
/// `(start + delta, duration - delta)` and a preceding Gap is grown (or inserted) by
/// the same amount. Negative `delta` extends left by consuming a preceding Gap while
/// expanding the source range. The total `Gap + clip` duration stays constant, so the
/// clip's record end and every following item's placement are unchanged.
///
/// The old [`trim_in`] remains the source-only primitive. A negative extension with no
/// preceding Gap, or one that exceeds it, returns [`EditError::Structural`] with an
/// explicit record-trim headroom message.
pub fn trim_in_record(ed: &mut Editor, clip: NodeId, delta: RationalTime) -> Result<(), EditError> {
    ed.edit("record trim in", |tx| {
        if delta.value() == 0.0 {
            return Ok(());
        }

        let tr = trimmed_range(tx.doc(), clip)?;
        let avail = available_range(tx.doc(), clip)?;
        let source_delta = delta.rescaled_to(tr.duration().rate());
        let new_start = tr.start_time() + source_delta;
        let new_duration = tr.duration() - source_delta;
        if new_start < avail.start_time() || new_duration.value() <= 0.0 {
            return Err(EditError::OutOfMediaBounds);
        }

        let (track, index) = track_and_index(tx.doc(), clip)?;
        let previous = index.checked_sub(1).and_then(|previous_index| {
            tx.doc()
                .node(track)
                .and_then(|node| node.children("children").get(previous_index).copied())
        });
        if previous.is_some_and(|node| is_transition(tx.doc(), node)) {
            return Err(EditError::AcrossTransition);
        }
        let previous_gap = previous.filter(|node| is_gap(tx.doc(), *node));
        let previous_gap_duration = previous_gap
            .map(|gap| trimmed_range(tx.doc(), gap).map(|range| range.duration()))
            .transpose()?;

        if source_delta.value() < 0.0 {
            let (gap, gap_duration) = previous_gap
                .zip(previous_gap_duration)
                .ok_or_else(record_trim_headroom_error)?;
            let gap_delta = source_delta.rescaled_to(gap_duration.rate());
            let new_gap_duration = gap_duration + gap_delta;
            if new_gap_duration.to_seconds() < -EPS {
                return Err(record_trim_headroom_error());
            }
            if new_gap_duration.to_seconds().abs() <= EPS {
                tx.remove_child(track, index - 1)?;
            } else {
                tx.set_value(
                    gap,
                    "source_range",
                    codec::time_range_value(TimeRange::new(
                        RationalTime::new(0.0, new_gap_duration.rate()),
                        new_gap_duration,
                    )),
                )?;
            }
        } else if let Some((gap, gap_duration)) = previous_gap.zip(previous_gap_duration) {
            let new_gap_duration = gap_duration + source_delta.rescaled_to(gap_duration.rate());
            tx.set_value(
                gap,
                "source_range",
                codec::time_range_value(TimeRange::new(
                    RationalTime::new(0.0, new_gap_duration.rate()),
                    new_gap_duration,
                )),
            )?;
        } else {
            let gap = tx.build_gap(TimeRange::new(
                RationalTime::new(0.0, source_delta.rate()),
                source_delta,
            ));
            tx.insert_child(track, index, gap)?;
        }

        tx.set_value(
            clip,
            "source_range",
            codec::time_range_value(TimeRange::new(new_start, new_duration)),
        )
    })
}

fn record_trim_headroom_error() -> EditError {
    EditError::Structural(
        "record trim in requires a preceding Gap large enough for the extension".into(),
    )
}

/// Trim a clip's out-point by `delta`: `sr' = (s, d + delta)`.
///
/// The in-point stays put, so positive `delta` lengthens the tail. Errors with
/// [`EditError::OutOfMediaBounds`] if the new tail would exceed the media
/// `available_range` end or the new duration would be `<= 0`, and with
/// [`EditError::AcrossTransition`] if the next sibling is a transition.
pub fn trim_out(ed: &mut Editor, clip: NodeId, delta: RationalTime) -> Result<(), EditError> {
    ed.edit("trim out", |tx| {
        let tr = trimmed_range(tx.doc(), clip)?;
        let avail = available_range(tx.doc(), clip)?;
        let s = tr.start_time();
        let d = tr.duration();
        let new_dur = d + delta;
        if new_dur.value() <= 0.0 || (s + new_dur) > avail.end_time_exclusive() {
            return Err(EditError::OutOfMediaBounds);
        }
        let (track, idx) = track_and_index(tx.doc(), clip)?;
        let next = tx
            .doc()
            .node(track)
            .and_then(|n| n.children("children").get(idx + 1).copied());
        if let Some(next) = next
            && is_transition(tx.doc(), next)
        {
            return Err(EditError::AcrossTransition);
        }
        tx.set_value(
            clip,
            "source_range",
            codec::time_range_value(TimeRange::new(s, new_dur)),
        )
    })
}

/// Slip a clip's media under a fixed window: `sr' = (s + delta, d)`.
///
/// Duration and timeline position are unchanged; only which media frames play
/// shift. A slip that would pull either edge outside the media `available_range`
/// is an [`EditError::OutOfMediaBounds`] error (clamp is an error here too — no
/// silent fixup). No neighbor changes, so no transition guard is needed.
pub fn slip(ed: &mut Editor, clip: NodeId, delta: RationalTime) -> Result<(), EditError> {
    ed.edit("slip", |tx| {
        let tr = trimmed_range(tx.doc(), clip)?;
        let avail = available_range(tx.doc(), clip)?;
        let new_start = tr.start_time() + delta;
        let new_end = new_start + tr.duration();
        if new_start < avail.start_time() || new_end > avail.end_time_exclusive() {
            return Err(EditError::OutOfMediaBounds);
        }
        tx.set_value(
            clip,
            "source_range",
            codec::time_range_value(TimeRange::new(new_start, tr.duration())),
        )
    })
}

/// Razor (split) the track child whose range contains track-local `time` into
/// two abutting items. The left keeps `(s, off)`, the right gets `(s + off,
/// d - off)`, where `off = time - child_start` (rescaled to the child's source
/// rate). The right item is a deep clone inserted at `child_index + 1`.
///
/// Errors [`EditError::AcrossTransition`] if the cut lands on a transition, and
/// [`EditError::OutOfMediaBounds`] if `time` is past the track end (no child's
/// range contains it) or lands exactly on an existing edge (`off <= 0` or
/// `off >= d`) — either would create a zero-duration item.
pub fn razor(ed: &mut Editor, track: NodeId, time: RationalTime) -> Result<(), EditError> {
    ed.edit("razor", |tx| {
        let ranges = range_of_all_children(tx.doc(), track)?;
        // A transition is centered on a cut and its range overlaps its neighbors;
        // if `time` falls inside ANY transition's span the cut is illegal, even
        // though a neighboring clip also "contains" it.
        if ranges
            .iter()
            .any(|(cid, r)| is_transition(tx.doc(), *cid) && r.contains(time))
        {
            return Err(EditError::AcrossTransition);
        }
        // The (non-transition) child whose range contains `time` (exclusive of the
        // end, so a cut on a boundary belongs to the following item — which then
        // gets off==0 and is rejected below, matching "no-op on an existing cut").
        let hit = ranges
            .iter()
            .find(|(cid, r)| !is_transition(tx.doc(), *cid) && r.contains(time))
            .copied()
            .ok_or(EditError::OutOfMediaBounds)?;
        let (child, child_range) = hit;
        let tr = trimmed_range(tx.doc(), child)?;
        let s = tr.start_time();
        let d = tr.duration();
        // Offset into the child, in the child's source rate.
        let off = (time - child_range.start_time()).rescaled_to(d.rate());
        if off.value() <= 0.0 || off >= d {
            return Err(EditError::OutOfMediaBounds);
        }
        let (_, idx) = track_and_index(tx.doc(), child)?;
        let right = tx.deep_clone(child)?;
        // Right item plays the back half; left item keeps the front half.
        tx.set_value(
            right,
            "source_range",
            codec::time_range_value(TimeRange::new(s + off, d - off)),
        )?;
        tx.set_value(
            child,
            "source_range",
            codec::time_range_value(TimeRange::new(s, off)),
        )?;
        tx.insert_child(track, idx + 1, right)?;
        Ok(())
    })
}

/// Lift the child at `index`: replace it with a same-duration `Gap`, leaving a
/// hole (the rest of the track does not shift). The removed item is detached
/// (pinned by history); a canonical gap of the removed item's *range* duration
/// takes its place at the same index.
pub fn lift(ed: &mut Editor, track: NodeId, index: usize) -> Result<(), EditError> {
    ed.edit("lift", |tx| {
        let ranges = range_of_all_children(tx.doc(), track)?;
        let (child, dur) = ranges.get(index).map(|(c, r)| (*c, r.duration())).ok_or(
            EditError::IndexOutOfRange {
                index,
                len: ranges.len(),
            },
        )?;
        // A Transition's range duration is in_offset+out_offset but a transition
        // does NOT advance the playhead, while a Gap does. Replacing a transition
        // with a same-"duration" gap would shove every downstream item right by
        // in+out and pull the cross-faded pair apart. Refuse it (mirrors the
        // trim/razor transition guards).
        if is_transition(tx.doc(), child) {
            return Err(EditError::AcrossTransition);
        }
        tx.remove_child(track, index)?;
        let gap = tx.build_gap(TimeRange::new(RationalTime::new(0.0, dur.rate()), dur));
        tx.insert_child(track, index, gap)?;
        // Enforce the global invariant: a lift next to an existing gap must not
        // leave two touching gaps.
        merge_adjacent_gaps(tx, track)?;
        Ok(())
    })
}

/// Lift the clip identified by `clip` (clip-addressed [`lift`]): replace it with a
/// same-duration `Gap` so downstream items stay put. The widget's plain Delete.
pub fn lift_clip(ed: &mut Editor, clip: NodeId) -> Result<(), EditError> {
    let (track, index) = track_and_index(ed.document(), clip)?;
    lift(ed, track, index)
}

/// Ripple-delete the clip identified by `clip` (clip-addressed [`ripple_delete`]):
/// remove it and let downstream items shift earlier, closing the gap. The
/// widget's Shift+Delete.
pub fn ripple_delete_clip(ed: &mut Editor, clip: NodeId) -> Result<(), EditError> {
    let (track, index) = track_and_index(ed.document(), clip)?;
    ripple_delete(ed, track, index)
}

/// Overwrite the track with `src_clip` placed at track-local `time` for the
/// clip's own duration `Dp`, leaving downstream items in place (no ripple).
///
/// Mirrors the flatten partial-trim math (`flatten.rs:88-99`): the item
/// straddling the window start is trimmed to end at `time`; the item straddling
/// the window end is trimmed to start at `time + Dp`; items fully covered are
/// removed; `src_clip` is inserted into the resulting hole; and if the window
/// runs past the current track end a trailing `Gap` pads the lead-in.
///
/// Covered & tested: overwrite landing inside one clip (splits it via head/tail
/// trims with `src_clip` between), spanning whole clips (removes them), and past
/// the track end (pads a lead-in gap). Transitions inside the window are rejected
/// with [`EditError::AcrossTransition`] (no transition-aware overwrite in v1).
pub fn overwrite(
    ed: &mut Editor,
    track: NodeId,
    src_clip: NodeId,
    time: RationalTime,
) -> Result<(), EditError> {
    ed.edit("overwrite", |tx| overwrite_in_tx(tx, track, src_clip, time))
}

/// The overwrite primitive, working inside an existing [`Tx`] so composite ops
/// (`move_clip`, `three_point`) can reuse the exact same placement math within
/// their own transaction. See [`overwrite`] for the semantics; this is the body.
fn overwrite_in_tx(
    tx: &mut Tx,
    track: NodeId,
    src_clip: NodeId,
    time: RationalTime,
) -> Result<(), EditError> {
    {
        // `src_clip` must NOT already live on `track`: the window scan below would
        // classify it (covered / straddle), rewrite its source_range or clone a
        // tail, then re-insert the SAME NodeId → one node double-attached, a
        // corrupt tree. Callers moving a clip within its own track (`move_clip`)
        // lift it out first; reject the unlifted case loudly rather than corrupt.
        if tx.doc().node(src_clip).and_then(|n| n.parent()) == Some(track) {
            return Err(EditError::Structural(
                "overwrite: src_clip is already a child of the target track (lift it first)".into(),
            ));
        }
        let dp = trimmed_range(tx.doc(), src_clip)?.duration();
        let win = TimeRange::new(time, dp);
        let win_start = win.start_time();
        let win_end = win.end_time_exclusive();

        let ranges = range_of_all_children(tx.doc(), track)?;
        let track_end = ranges
            .last()
            .map(|(_, r)| r.end_time_exclusive())
            .unwrap_or(RationalTime::new(0.0, dp.rate()));

        // Reject any transition overlapping the window (v1 limitation).
        for (cid, r) in &ranges {
            if is_transition(tx.doc(), *cid) && r.intersects(win, EPS) {
                return Err(EditError::AcrossTransition);
            }
        }

        // Classify each non-transition child against the window. Apply structural
        // changes after the scan, highest-index-first, so indices stay valid.
        let mut covered: Vec<usize> = Vec::new();
        // (clip, head_source_range, optional tail_source_range): a clip straddling
        // the window keeps its head and, if it also straddles the far edge, a
        // freshly-cloned tail inserted after it (a true split). Positions are
        // re-derived from the live doc at apply time, so no index is stored.
        let mut splits: Vec<(NodeId, TimeRange, Option<TimeRange>)> = Vec::new();
        for (idx, (cid, r)) in ranges.iter().enumerate() {
            if is_transition(tx.doc(), *cid) {
                continue;
            }
            let rs = r.start_time();
            let re = r.end_time_exclusive();
            if re <= win_start || rs >= win_end {
                continue; // no overlap → untouched
            }
            let straddle_start = rs < win_start && re > win_start;
            let straddle_end = rs < win_end && re > win_end;
            if !straddle_start && !straddle_end {
                covered.push(idx); // fully inside the window → remove
                continue;
            }
            let csr = trimmed_range(tx.doc(), *cid)?;
            // Head portion [rs, win_start) when straddling the start; otherwise the
            // surviving piece is the tail [win_end, re).
            let (head, tail) = if straddle_start && straddle_end {
                // Split: head keeps [rs, win_start), tail keeps [win_end, re).
                let head_dur = win_start - rs;
                let tail_drop = win_end - rs;
                let head_sr = TimeRange::new(csr.start_time(), head_dur);
                // A Gap is canonical with source_range.start == 0 (it has no media
                // offset); a clip carries its real media offset. Normalize the gap
                // tail so the split never emits a non-canonical gap.
                let tail_start = if is_gap(tx.doc(), *cid) {
                    RationalTime::new(0.0, csr.duration().rate())
                } else {
                    csr.start_time() + tail_drop
                };
                let tail_sr = TimeRange::new(tail_start, csr.duration() - tail_drop);
                (head_sr, Some(tail_sr))
            } else if straddle_start {
                let amount = re - win_start;
                (
                    TimeRange::new(csr.start_time(), csr.duration() - amount),
                    None,
                )
            } else {
                // straddle_end only: keep the tail, trim the head off.
                let amount = win_end - rs;
                (
                    TimeRange::new(csr.start_time() + amount, csr.duration() - amount),
                    None,
                )
            };
            splits.push((*cid, head, tail));
        }

        // Apply: remove fully-covered first (highest index first).
        for &idx in covered.iter().rev() {
            tx.remove_child(track, idx)?;
        }
        // Then trim the straddling clips and clone tails as needed. Process
        // highest-index-first so an inserted tail does not shift earlier indices.
        for &(cid, head, tail) in splits.iter().rev() {
            tx.set_value(cid, "source_range", codec::time_range_value(head))?;
            if let Some(tail_sr) = tail {
                let tail_clip = tx.deep_clone(cid)?;
                tx.set_value(tail_clip, "source_range", codec::time_range_value(tail_sr))?;
                // Insert the tail right after the (head) clip's current position.
                let (_, head_idx) = track_and_index(tx.doc(), cid)?;
                tx.insert_child(track, head_idx + 1, tail_clip)?;
            }
        }

        // Recompute the insertion index: the first child that now starts at or
        // after the window start. After the head trim + covered removal no
        // surviving child straddles win_start (the straddler now ends exactly at
        // win_start, the far straddler starts at win_end), so `start >= win_start`
        // alone is correct — the old `|| end_excl > win_start` disjunct was a dead,
        // float-fragile path and is dropped.
        let ranges2 = range_of_all_children(tx.doc(), track)?;
        let mut insert_at = ranges2.len();
        for (idx, (_, r)) in ranges2.iter().enumerate() {
            if r.start_time() >= win_start {
                insert_at = idx;
                break;
            }
        }

        // If the window starts past the current track end, pad a lead-in gap so
        // the inserted clip lands at `time`.
        if win_start > track_end {
            let pad = win_start - track_end;
            if pad.value() > 0.0 {
                let gap = tx.build_gap(TimeRange::new(RationalTime::new(0.0, pad.rate()), pad));
                tx.insert_child(track, insert_at, gap)?;
                insert_at += 1;
            }
        }
        tx.insert_child(track, insert_at, src_clip)?;
        // A removed-covered clip or a trailing pad can leave the inserted clip
        // adjacent to a pre-existing gap; keep the no-two-touching-gaps invariant.
        merge_adjacent_gaps(tx, track)?;
        Ok(())
    }
}

/// Append a new empty `Track` of `kind` (`"Video"`/`"Audio"`) to `stack` and
/// return its id. The track is built canonically (full default field set).
pub fn add_track(
    ed: &mut Editor,
    stack: NodeId,
    name: &str,
    kind: &str,
) -> Result<NodeId, EditError> {
    // `Editor::edit` returns `Result<(), _>`; capture the new id via a cell the
    // closure fills, so the create stays journaled inside the transaction.
    let mut out: Option<NodeId> = None;
    ed.edit("add track", |tx| {
        let track = tx.build_track(name, kind);
        tx.append_child(stack, track)?;
        out = Some(track);
        Ok(())
    })?;
    out.ok_or(EditError::Structural("add_track produced no id".into()))
}

/// Remove the track at `index` from `stack`.
pub fn remove_track(ed: &mut Editor, stack: NodeId, index: usize) -> Result<(), EditError> {
    ed.edit("remove track", |tx| {
        tx.remove_child(stack, index)?;
        Ok(())
    })
}

/// Reorder a track within `stack`: remove it from `from` and insert at `to`.
///
/// `to` is interpreted against the list *after* the removal (insert clamps), so
/// moving a track to a higher index lands it past the items that shifted down.
pub fn reorder_track(
    ed: &mut Editor,
    stack: NodeId,
    from: usize,
    to: usize,
) -> Result<(), EditError> {
    ed.edit("reorder track", |tx| {
        let track = tx.remove_child(stack, from)?;
        tx.insert_child(stack, to, track)?;
        Ok(())
    })
}

/// Add a `Marker` spanning `marked_range` (in the item's own time) with `color`
/// (an OTIO marker-color enum, e.g. `"RED"`) to `item`'s `markers` list.
pub fn add_marker(
    ed: &mut Editor,
    item: NodeId,
    marked_range: TimeRange,
    color: &str,
) -> Result<(), EditError> {
    ed.edit("add marker", |tx| {
        let marker = tx.build_marker("", marked_range, color, color);
        tx.push_named_child(item, "markers", marker)?;
        Ok(())
    })
}

/// Remove the marker at `index` from `item`'s `markers` list.
pub fn remove_marker(ed: &mut Editor, item: NodeId, index: usize) -> Result<(), EditError> {
    ed.edit("remove marker", |tx| {
        tx.remove_named_child(item, "markers", index)?;
        Ok(())
    })
}

// ===========================================================================
// v2 edit ops — roll, slide, move_clip, three_point.
//
// Formulas follow PLAN.md "Edit ops — v2". Like v1, range math reads via
// `tx.doc()` + otio-algo BEFORE any mutation; changes go through `Tx`. Media-
// bounds and transition violations are hard errors (never silent clamps).
// ===========================================================================

/// The child id immediately following `clip` on its track, plus the track id and
/// `clip`'s index — or an error if `clip` is the last child (no right sibling).
fn right_sibling(doc: &Document, clip: NodeId) -> Result<(NodeId, usize, NodeId), EditError> {
    let (track, idx) = track_and_index(doc, clip)?;
    let next = doc
        .node(track)
        .and_then(|n| n.children("children").get(idx + 1).copied())
        .ok_or(EditError::IndexOutOfRange {
            index: idx + 1,
            len: doc
                .node(track)
                .map(|n| n.children("children").len())
                .unwrap_or(0),
        })?;
    Ok((track, idx, next))
}

/// Roll the shared edit point between `left_clip` (A) and its right sibling (B):
/// A keeps its start and grows by `delta` (`sr_A' = (sA, dA + delta)`); B shrinks
/// its head by the same amount (`sr_B' = (sB + delta, dB - delta)`). The boundary
/// between them moves by `delta` while the pair's total footprint is unchanged.
///
/// Errors:
/// - [`EditError::AcrossTransition`] if A or B is a transition, or if a transition
///   sits between them.
/// - [`EditError::OutOfMediaBounds`] if either edit leaves real media bounds:
///   A's new end (`sA + dA + delta`) past A's `available_range` end, B's new start
///   (`sB + delta`) before B's `available_range` start, or either new duration
///   `<= 0`.
///
/// Gap handling: a `Gap` has no media `available_range`, so when A or B is a gap
/// only its duration is resized (the `<= 0` guard still applies); the media-end /
/// media-start checks are skipped for that side.
pub fn roll(ed: &mut Editor, left_clip: NodeId, delta: RationalTime) -> Result<(), EditError> {
    ed.edit("roll", |tx| {
        // `b` is the child at idx+1 (roll only resizes A and B — no structural
        // change — so the track id / index are not needed). A transition AS that
        // idx+1 child is exactly the "transition between A and B" case, caught here.
        let (_track, _idx, b) = right_sibling(tx.doc(), left_clip)?;
        let a = left_clip;
        if is_transition(tx.doc(), a) || is_transition(tx.doc(), b) {
            return Err(EditError::AcrossTransition);
        }

        let a_tr = trimmed_range(tx.doc(), a)?;
        let b_tr = trimmed_range(tx.doc(), b)?;
        let a_new_dur = a_tr.duration() + delta;
        // A clip's media in-point advances by `delta`; a Gap has no media, so its
        // source_range start stays anchored at 0 (its timeline shift is carried by
        // the layout's cumulative durations, not by a non-canonical start drift).
        let b_new_start = if is_gap(tx.doc(), b) {
            b_tr.start_time()
        } else {
            b_tr.start_time() + delta
        };
        let b_new_dur = b_tr.duration() - delta;
        if a_new_dur.value() <= 0.0 || b_new_dur.value() <= 0.0 {
            return Err(EditError::OutOfMediaBounds);
        }
        // Media-bounds checks only where a real `available_range` exists (skip gaps).
        if !is_gap(tx.doc(), a) {
            let a_avail = available_range(tx.doc(), a)?;
            if a_tr.start_time() + a_new_dur > a_avail.end_time_exclusive() {
                return Err(EditError::OutOfMediaBounds);
            }
        }
        if !is_gap(tx.doc(), b) && b_new_start < available_range(tx.doc(), b)?.start_time() {
            return Err(EditError::OutOfMediaBounds);
        }

        tx.set_value(
            a,
            "source_range",
            codec::time_range_value(TimeRange::new(a_tr.start_time(), a_new_dur)),
        )?;
        tx.set_value(
            b,
            "source_range",
            codec::time_range_value(TimeRange::new(b_new_start, b_new_dur)),
        )?;
        Ok(())
    })
}

/// Slide `clip` along its track by `delta` WITHOUT changing its own
/// `source_range` (the media window it plays is fixed; only its timeline position
/// moves). The previous neighbor absorbs `+delta` on its tail and the next
/// neighbor absorbs `-delta` on its head, so downstream items past the next
/// neighbor stay put (a non-rippling drag).
///
/// Neighbor semantics (the v2 choice, documented):
/// - **Gap neighbor** → resized: its duration grows (`prev`) or shrinks (`next`)
///   by `delta`. A resize that would drop a neighbor gap to `<= 0` is refused
///   ([`EditError::OutOfMediaBounds`]); the caller should ripple/trim instead.
/// - **Clip neighbor** → that shared edge is a roll: `prev` grows its tail by
///   `delta`, `next` shrinks its head by `delta`, both clamped to media bounds
///   (reusing roll's math). A clip neighbor without the media room refuses.
/// - **Missing neighbor** (clip at the track start/end): the open side just
///   shifts — a lead-in `Gap` is inserted/extended (`prev` missing, `delta > 0`)
///   or trimmed (`delta < 0`), and symmetrically for a missing `next`. Adjacent
///   gaps are merged afterwards so the no-two-touching-gaps invariant holds.
///
/// A transition as either neighbor is refused ([`EditError::AcrossTransition`]).
pub fn slide(ed: &mut Editor, clip: NodeId, delta: RationalTime) -> Result<(), EditError> {
    ed.edit("slide", |tx| {
        let (track, idx) = track_and_index(tx.doc(), clip)?;
        let prev = if idx > 0 {
            tx.doc()
                .node(track)
                .map(|n| n.children("children")[idx - 1])
        } else {
            None
        };
        let next = tx
            .doc()
            .node(track)
            .and_then(|n| n.children("children").get(idx + 1).copied());

        if let Some(p) = prev
            && is_transition(tx.doc(), p)
        {
            return Err(EditError::AcrossTransition);
        }
        if let Some(nx) = next
            && is_transition(tx.doc(), nx)
        {
            return Err(EditError::AcrossTransition);
        }
        let rate = trimmed_range(tx.doc(), clip)?.duration().rate();

        // --- previous side: absorb +delta on its tail ---
        match prev {
            Some(p) if is_gap(tx.doc(), p) => {
                let pd = trimmed_range(tx.doc(), p)?.duration();
                let nd = pd + delta;
                if nd.value() <= 0.0 {
                    return Err(EditError::OutOfMediaBounds);
                }
                tx.set_value(
                    p,
                    "source_range",
                    codec::time_range_value(TimeRange::new(RationalTime::new(0.0, nd.rate()), nd)),
                )?;
            }
            Some(p) => {
                // Clip neighbor → roll its tail by +delta (grow tail).
                let tr = trimmed_range(tx.doc(), p)?;
                let avail = available_range(tx.doc(), p)?;
                let nd = tr.duration() + delta;
                if nd.value() <= 0.0 || tr.start_time() + nd > avail.end_time_exclusive() {
                    return Err(EditError::OutOfMediaBounds);
                }
                tx.set_value(
                    p,
                    "source_range",
                    codec::time_range_value(TimeRange::new(tr.start_time(), nd)),
                )?;
            }
            None => {
                // No previous neighbor: shift the open lead-in.
                // delta > 0 → insert/extend a lead-in gap; delta < 0 would pull the
                // clip before the track start (no room) → refuse.
                if delta.value() > 0.0 {
                    let gap = tx.build_gap(TimeRange::new(RationalTime::new(0.0, rate), delta));
                    tx.insert_child(track, idx, gap)?;
                } else if delta.value() < 0.0 {
                    return Err(EditError::OutOfMediaBounds);
                }
            }
        }

        // --- next side: absorb -delta on its head ---
        // Re-locate the clip's index (a lead-in gap insert may have shifted it).
        let (track, idx) = track_and_index(tx.doc(), clip)?;
        let next = tx
            .doc()
            .node(track)
            .and_then(|n| n.children("children").get(idx + 1).copied());
        match next {
            Some(nx) if is_gap(tx.doc(), nx) => {
                let nd = trimmed_range(tx.doc(), nx)?.duration() - delta;
                if nd.value() <= 0.0 {
                    return Err(EditError::OutOfMediaBounds);
                }
                tx.set_value(
                    nx,
                    "source_range",
                    codec::time_range_value(TimeRange::new(RationalTime::new(0.0, nd.rate()), nd)),
                )?;
            }
            Some(nx) => {
                // Clip neighbor → roll its head by +delta (shrink head, start later).
                let tr = trimmed_range(tx.doc(), nx)?;
                let avail = available_range(tx.doc(), nx)?;
                let ns = tr.start_time() + delta;
                let nd = tr.duration() - delta;
                if nd.value() <= 0.0 || ns < avail.start_time() {
                    return Err(EditError::OutOfMediaBounds);
                }
                tx.set_value(
                    nx,
                    "source_range",
                    codec::time_range_value(TimeRange::new(ns, nd)),
                )?;
            }
            None => {
                // No next neighbor: a trailing gap would absorb -delta; with no
                // downstream item there is nothing to keep in place, so a slide of a
                // last clip with no prev/next neighbor is a no-op on that side.
                // delta < 0 pulling the clip's tail past the track end needs a
                // lead-in handled above; nothing to do here.
            }
        }

        // A gap resize to a positive duration cannot create adjacency by itself,
        // but a lead-in gap insert next to a pre-existing gap can; keep the
        // invariant.
        merge_adjacent_gaps(tx, track)?;
        Ok(())
    })
}

/// Move `clip` to `to_index` on its track using **lift + overwrite** semantics
/// (the common non-rippling NLE drag): the clip is lifted from its current slot
/// (leaving a same-duration `Gap`, so everything downstream stays put), then
/// overwritten back in at the position implied by `to_index` (computed from the
/// post-lift layout). Downstream items never shift — only the dragged clip and the
/// material it lands on change.
///
/// `to_index` is interpreted against the track's child list. The clip is placed at
/// the start time of the child currently occupying `to_index` (clamped to the
/// track end), so dropping it "onto slot N" lands it where slot N begins.
/// Reuses [`lift`] and [`overwrite`]; undo round-trips as one atomic step.
pub fn move_clip(ed: &mut Editor, clip: NodeId, to_index: usize) -> Result<(), EditError> {
    ed.edit("move clip", |tx| {
        let (track, from_idx) = track_and_index(tx.doc(), clip)?;
        let ranges = range_of_all_children(tx.doc(), track)?;
        // The moved clip's own rate, used as the empty-track fallback origin rate
        // instead of a hardcoded 24 (the clip is always on `track`, so the
        // fallback is defensive, but it should still carry the right rate).
        let rate = trimmed_range(tx.doc(), clip)?.duration().rate();
        // Target timeline position: the start of the child at `to_index`, or the
        // track end if `to_index` is past the last child.
        let target = ranges
            .get(to_index)
            .map(|(_, r)| r.start_time())
            .unwrap_or_else(|| {
                ranges
                    .last()
                    .map(|(_, r)| r.end_time_exclusive())
                    .unwrap_or_else(|| RationalTime::new(0.0, rate))
            });

        // Lift: replace the clip with a same-duration gap so the rest stays put.
        // (Inlined from `lift` so the whole move is one transaction; `lift` itself
        // is a public op running its own `edit`.)
        let dur =
            ranges
                .get(from_idx)
                .map(|(_, r)| r.duration())
                .ok_or(EditError::IndexOutOfRange {
                    index: from_idx,
                    len: ranges.len(),
                })?;
        tx.remove_child(track, from_idx)?;
        let gap = tx.build_gap(TimeRange::new(RationalTime::new(0.0, dur.rate()), dur));
        tx.insert_child(track, from_idx, gap)?;
        merge_adjacent_gaps(tx, track)?;

        // Overwrite the clip back in at the target position.
        overwrite_in_tx(tx, track, clip, target)?;
        Ok(())
    })
}

/// Move `clip` along its track by a time `delta` (a free reposition), using the
/// same lift + overwrite as [`move_clip`] but driven by a delta instead of a
/// target index — the natural fit for a timeline body-drag. The clip is lifted
/// (leaving a same-duration `Gap`, so everything downstream stays put), then
/// overwritten back in at `current_start + delta` (clamped to `>= 0`).
///
/// The clip's own `source_range` is UNCHANGED and neighbours are NOT rolled —
/// contrast [`slide`], which keeps the clip put and makes the previous/next
/// neighbours absorb `delta` (rolling the previous clip's tail, i.e. its out-
/// point, to follow). `move_by` is the "drag a clip and leave a gap" gesture;
/// `slide` is the "shuffle a clip between its neighbours" gesture.
pub fn move_by(ed: &mut Editor, clip: NodeId, delta: RationalTime) -> Result<(), EditError> {
    ed.edit("move", |tx| {
        let (track, from_idx) = track_and_index(tx.doc(), clip)?;
        let ranges = range_of_all_children(tx.doc(), track)?;
        let (dur, start) = ranges
            .get(from_idx)
            .map(|(_, r)| (r.duration(), r.start_time()))
            .ok_or(EditError::IndexOutOfRange {
                index: from_idx,
                len: ranges.len(),
            })?;
        // Target timeline position. A drag that would push the head before the
        // track start is clamped to 0 (overwrite pads no negative lead-in).
        let mut target = start + delta;
        if target.value() < 0.0 {
            target = RationalTime::new(0.0, target.rate());
        }
        // Lift: replace the clip with a same-duration gap so the rest stays put.
        tx.remove_child(track, from_idx)?;
        let gap = tx.build_gap(TimeRange::new(RationalTime::new(0.0, dur.rate()), dur));
        tx.insert_child(track, from_idx, gap)?;
        merge_adjacent_gaps(tx, track)?;
        // Overwrite the clip back in at the target position.
        overwrite_in_tx(tx, track, clip, target)?;
        Ok(())
    })
}

/// Move `clip` off its current track and onto a DIFFERENT `dst_track`, landing at
/// timeline-local `dst_start`. Exactly [`move_by`], but the overwrite targets another
/// track: the source track keeps a same-duration `Gap` in the clip's old slot (so
/// everything downstream stays put, like lift), and `dst_track` is OVERWRITTEN at
/// `dst_start` for the clip's own duration. One atomic, undoable step.
///
/// Errors [`EditError::Structural`] if `dst_track` is the clip's current track (use
/// [`move_by`] for a same-track drag), and [`EditError::AcrossTransition`] if the clip
/// is a transition or a transition overlaps the destination window.
pub fn move_clip_cross_track(
    ed: &mut Editor,
    clip: NodeId,
    dst_track: NodeId,
    dst_start: RationalTime,
) -> Result<(), EditError> {
    ed.edit("move to track", |tx| {
        let (src_track, from_idx) = track_and_index(tx.doc(), clip)?;
        // Same-track drops are `move_by`'s job; this op is strictly cross-track.
        if src_track == dst_track {
            return Err(EditError::Structural(
                "move_clip_cross_track: dst_track equals the source track (use move_by)".into(),
            ));
        }
        // Mirror lift's transition guard.
        if is_transition(tx.doc(), clip) {
            return Err(EditError::AcrossTransition);
        }
        // Duration for the same-duration source-side gap.
        let ranges = range_of_all_children(tx.doc(), src_track)?;
        let dur =
            ranges
                .get(from_idx)
                .map(|(_, r)| r.duration())
                .ok_or(EditError::IndexOutOfRange {
                    index: from_idx,
                    len: ranges.len(),
                })?;
        // Clamp a negative lead-in (overwrite pads no negative time), as `move_by` does.
        let mut start = dst_start;
        if start.value() < 0.0 {
            start = RationalTime::new(0.0, start.rate());
        }
        // Source side: inlined lift — leave a same-duration gap so downstream stays put.
        tx.remove_child(src_track, from_idx)?;
        let gap = tx.build_gap(TimeRange::new(RationalTime::new(0.0, dur.rate()), dur));
        tx.insert_child(src_track, from_idx, gap)?;
        merge_adjacent_gaps(tx, src_track)?;
        // Destination side: overwrite the now-detached clip in at `start`.
        overwrite_in_tx(tx, dst_track, clip, start)?;
        Ok(())
    })
}

/// Three-point edit: set `src_clip`'s `source_range` to the source window
/// `(src_in, src_out - src_in)` (the IN/OUT marked on the source), then overwrite
/// it onto `track` at `timeline_in` (the IN marked on the timeline). The OUT on
/// the timeline is implied by the source window's duration — the classic
/// "three marks define the fourth" edit.
///
/// Errors [`EditError::OutOfMediaBounds`] if `src_out <= src_in` (empty/inverted
/// window). The overwrite leg inherits [`overwrite`]'s behaviour, including its v1
/// refusal of a transition overlapping the window.
pub fn three_point(
    ed: &mut Editor,
    track: NodeId,
    src_clip: NodeId,
    src_in: RationalTime,
    src_out: RationalTime,
    timeline_in: RationalTime,
) -> Result<(), EditError> {
    ed.edit("three-point", |tx| {
        let dur = src_out - src_in;
        if dur.value() <= 0.0 {
            return Err(EditError::OutOfMediaBounds);
        }
        tx.set_value(
            src_clip,
            "source_range",
            codec::time_range_value(TimeRange::new(src_in, dur)),
        )?;
        overwrite_in_tx(tx, track, src_clip, timeline_in)?;
        Ok(())
    })
}

/// Comparison tolerance for range overlap tests in the edit ops.
const EPS: f64 = otio_time::DEFAULT_EPSILON_S;

/// Merge runs of adjacent `Gap`s in `track`'s `children` into a single gap (the
/// summed duration), and drop any zero-duration gap. Keeps the v1 invariant
/// "never two touching gaps, never a zero gap". Operates highest-index first so
/// earlier indices stay valid as later ones are removed.
fn merge_adjacent_gaps(tx: &mut Tx, track: NodeId) -> Result<(), EditError> {
    let children: Vec<NodeId> = tx
        .doc()
        .node(track)
        .ok_or(EditError::NodeNotFound)?
        .children("children")
        .to_vec();

    // Pass 1: fold each gap that is immediately preceded by a gap into its
    // predecessor (summing durations), then remove the now-redundant successor.
    // Walk left-to-right collecting merges, apply removals right-to-left.
    let mut i = 0usize;
    while i + 1 < children.len() {
        // Re-read the live children each round (durations/indices change).
        let live: Vec<NodeId> = tx
            .doc()
            .node(track)
            .ok_or(EditError::NodeNotFound)?
            .children("children")
            .to_vec();
        if i + 1 >= live.len() {
            break;
        }
        let a = live[i];
        let b = live[i + 1];
        if is_gap(tx.doc(), a) && is_gap(tx.doc(), b) {
            let da = trimmed_range(tx.doc(), a)?.duration();
            let db = trimmed_range(tx.doc(), b)?.duration();
            let sum = da + db;
            tx.set_value(
                a,
                "source_range",
                codec::time_range_value(TimeRange::new(RationalTime::new(0.0, sum.rate()), sum)),
            )?;
            tx.remove_child(track, i + 1)?;
            // Stay at `i`: the new successor may also be a gap.
            continue;
        }
        i += 1;
    }

    // Pass 2: drop any zero-duration gap.
    loop {
        let live: Vec<NodeId> = tx
            .doc()
            .node(track)
            .ok_or(EditError::NodeNotFound)?
            .children("children")
            .to_vec();
        let mut removed = false;
        for (idx, &c) in live.iter().enumerate() {
            if is_gap(tx.doc(), c) && trimmed_range(tx.doc(), c)?.duration().value() <= 0.0 {
                tx.remove_child(track, idx)?;
                removed = true;
                break;
            }
        }
        if !removed {
            break;
        }
    }
    Ok(())
}
