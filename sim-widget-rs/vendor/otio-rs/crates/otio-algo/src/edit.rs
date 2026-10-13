//! Pure span-edit algebra: the *maths* of an editorial edit, with no document,
//! no node ids, and no host coupling.
//!
//! # Why this module exists
//!
//! A host that lays clips out end-to-end on one track and then implements a
//! *reorder* as "place the dragged clip at an arbitrary time, then validate that
//! nothing overlaps" has encoded the wrong operation. On a contiguous track a
//! reorder is a **permutation followed by a contiguous re-layout**, and such a
//! result *cannot* overlap — so the operation is **total**: it has no failure
//! mode to report. Expressing it as a fallible free placement is what makes a
//! legal drag get rejected (drag left and you overlap the predecessor, drag
//! right and you overrun the successor).
//!
//! That distinction is encoded in the signatures here:
//!
//! - [`reorder_contiguous`] and [`drop_index`] return **no** `Result`. They are
//!   total by construction.
//! - The sparse/general operations ([`ripple_move`], [`trim_in`], [`trim_out`],
//!   [`blade`], [`slide`], [`roll`]) return [`EditError`] only where the
//!   operation genuinely has no defined result (an empty span, a cut outside
//!   the span, a non-adjacent roll). A *legal* edit never errors.
//!
//! # Representation
//!
//! A track is a slice of [`TimeRange`] in record time, in track order. That is
//! the whole model: no ids, no kinds, no media. A host maps its own clips onto
//! `&[TimeRange]` by index and maps the results back the same way — which is
//! why [`reorder_contiguous`] also returns the index permutation, so the caller
//! can re-associate its clips without comparing floating-point times.
//!
//! Times use [`RationalTime`]; a host that thinks in whole frames can use
//! `rate = 1.0` and stay exact (integers are exact in `f64` well past any
//! plausible frame count).

use otio_time::{RationalTime, TimeRange};
use thiserror::Error;

/// Failure modes of the sparse/general edit operations.
///
/// Deliberately *not* returned by [`reorder_contiguous`] / [`drop_index`]:
/// those are total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EditError {
    /// An element index addressed past the end of the track.
    #[error("index {index} out of bounds for {len} spans")]
    IndexOutOfBounds {
        /// The requested index.
        index: usize,
        /// The number of spans.
        len: usize,
    },

    /// The edit would leave a span with zero or negative duration.
    #[error("edit would collapse a span to zero duration")]
    EmptySpan,

    /// A blade point that is not strictly inside the span (a cut on an edge
    /// produces one empty half, which is not a cut).
    #[error("cut point lies outside the span")]
    CutOutsideSpan,

    /// A ripple placement that starts before the end of the preceding span.
    /// A forward ripple only pushes *later* spans; it cannot make room behind.
    #[error("placement would overlap the preceding span")]
    WouldOverlapPredecessor,

    /// A roll was asked for on two spans that do not share an edge.
    #[error("roll requires two spans that share an edge")]
    NotAdjacent,

    /// A slide moves a span *within* the hole between its neighbours, so both
    /// neighbours must exist to absorb the movement.
    #[error("slide requires both a preceding and a following span")]
    NeedsNeighbours,
}

/// The outcome of a contiguous reorder: the new track order and its layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Reorder {
    /// The permutation, as `order[new_index] = old_index`. Always a permutation
    /// of `0..spans.len()`.
    pub order: Vec<usize>,
    /// The re-laid spans, in the new order, packed end-to-end from the original
    /// first span's start time.
    pub spans: Vec<TimeRange>,
}

/// Move element `from` to index `to` on a contiguous track and re-lay the whole
/// track end-to-end.
///
/// **Total — it cannot fail.** Out-of-range indices are clamped into the track
/// and an empty track yields an empty result, because there is no reorder of a
/// track that is not expressible; refusing one would only push a meaningless
/// error onto the caller.
///
/// Guaranteed invariants (pinned by tests):
/// - the multiset of durations is preserved;
/// - the total duration and the first start time are preserved;
/// - the result is contiguous (`spans[i].end == spans[i + 1].start`), so it can
///   never overlap and never needs overlap validation;
/// - `order` is a permutation of `0..len`, equal to the source order with
///   `from` removed and re-inserted at `to`;
/// - `to == from` is an exact no-op.
pub fn reorder_contiguous(spans: &[TimeRange], from: usize, to: usize) -> Reorder {
    let len = spans.len();
    if len == 0 {
        return Reorder {
            order: Vec::new(),
            spans: Vec::new(),
        };
    }
    let last = len - 1;
    let from = from.min(last);
    let to = to.min(last);

    let mut order: Vec<usize> = (0..len).collect();
    let moved = order.remove(from);
    order.insert(to, moved);

    let origin = spans[0].start_time();
    Reorder {
        spans: lay_contiguously(origin, order.iter().map(|&i| spans[i].duration())),
        order,
    }
}

/// Which index a drag of element `from` lands on when its leading edge is
/// dropped at `drop_start`.
///
/// **Total — it cannot fail.** This is the companion of
/// [`reorder_contiguous`]: it turns a drop *position in time* into the
/// destination *index*, so a host never hand-rolls that arithmetic (getting it
/// subtly wrong per call site is how drag semantics drift between widgets).
///
/// Rule: the dragged element is removed, the remainder is re-packed from the
/// track origin, and the destination is the first remaining element whose
/// midpoint lies after `drop_start` — i.e. you must drag past half of a
/// neighbour to displace it. This makes the mapping monotonic in `drop_start`
/// and makes a drop at the element's own start an exact no-op
/// (`drop_index(spans, i, spans[i].start_time()) == i`).
///
/// The result is always a valid destination for [`reorder_contiguous`]
/// (`0..len`, clamped for an empty track).
pub fn drop_index(spans: &[TimeRange], from: usize, drop_start: RationalTime) -> usize {
    let len = spans.len();
    if len == 0 {
        return 0;
    }
    let from = from.min(len - 1);
    let mut cursor = spans[0].start_time();
    for (i, span) in spans.iter().enumerate().filter(|(i, _)| *i != from) {
        let mid = cursor + half(span.duration());
        if drop_start < mid {
            // `i` counts in source indices; subtract the removed element when it
            // lay before this one, to get the index in the remainder.
            return if i > from { i - 1 } else { i };
        }
        cursor += span.duration();
    }
    len - 1
}

/// Place `spans[index]` at `new_start` on a sparse track, pushing every later
/// span right by the minimum amount that removes the resulting overlap.
///
/// Unlike [`reorder_contiguous`] this is a *free placement*, so it has a real
/// failure mode: a ripple only makes room ahead of the moved span, never
/// behind it. Starts of the preceding spans are left untouched.
///
/// Guarantees: every duration is preserved; no span starts before its
/// predecessor ends; no span moves left.
pub fn ripple_move(
    spans: &[TimeRange],
    index: usize,
    new_start: RationalTime,
) -> Result<Vec<TimeRange>, EditError> {
    let len = spans.len();
    let span = *spans.get(index).ok_or(EditError::IndexOutOfBounds { index, len })?;
    if let Some(prev) = index.checked_sub(1).and_then(|i| spans.get(i))
        && new_start < prev.end_time_exclusive()
    {
        return Err(EditError::WouldOverlapPredecessor);
    }

    let mut out = spans.to_vec();
    out[index] = TimeRange::new(new_start, span.duration());
    let mut cursor = out[index].end_time_exclusive();
    for later in out.iter_mut().skip(index + 1) {
        if later.start_time() < cursor {
            *later = TimeRange::new(cursor, later.duration());
        }
        cursor = later.end_time_exclusive();
    }
    Ok(out)
}

/// Move a span's in-edge to `new_start`, holding its out-edge fixed.
///
/// Guarantee: the out-edge is unchanged and the result has a positive duration,
/// or [`EditError::EmptySpan`].
pub fn trim_in(span: TimeRange, new_start: RationalTime) -> Result<TimeRange, EditError> {
    let duration = span.end_time_exclusive() - new_start;
    positive(duration)?;
    Ok(TimeRange::new(new_start, duration))
}

/// Move a span's out-edge to `new_end`, holding its in-edge fixed.
///
/// Guarantee: the in-edge is unchanged and the result has a positive duration,
/// or [`EditError::EmptySpan`].
pub fn trim_out(span: TimeRange, new_end: RationalTime) -> Result<TimeRange, EditError> {
    let duration = new_end - span.start_time();
    positive(duration)?;
    Ok(TimeRange::new(span.start_time(), duration))
}

/// Cut a span at `at`, yielding the `(left, right)` halves.
///
/// Guarantee: both halves have a positive duration, they are contiguous, and
/// their durations sum to the original — or [`EditError::CutOutsideSpan`] when
/// `at` is not strictly inside the span (a cut on an edge is not a cut).
pub fn blade(span: TimeRange, at: RationalTime) -> Result<(TimeRange, TimeRange), EditError> {
    let left = TimeRange::new(span.start_time(), at - span.start_time());
    let right = TimeRange::new(at, span.end_time_exclusive() - at);
    if positive(left.duration()).is_err() || positive(right.duration()).is_err() {
        return Err(EditError::CutOutsideSpan);
    }
    Ok((left, right))
}

/// Slide `spans[index]` by `delta` *within the hole between its neighbours*:
/// the preceding span's out-edge and the following span's in-edge move with it.
///
/// This is the editorial slide, so both neighbours must exist to absorb the
/// movement ([`EditError::NeedsNeighbours`] otherwise) and neither may be
/// collapsed ([`EditError::EmptySpan`]).
///
/// Guarantee: the slid span's duration and the track's total extent are
/// unchanged; only the two neighbours' durations change, by `+delta` and
/// `-delta`.
pub fn slide(
    spans: &[TimeRange],
    index: usize,
    delta: RationalTime,
) -> Result<Vec<TimeRange>, EditError> {
    let len = spans.len();
    if index >= len {
        return Err(EditError::IndexOutOfBounds { index, len });
    }
    if index == 0 || index + 1 >= len {
        return Err(EditError::NeedsNeighbours);
    }
    let mut out = spans.to_vec();
    let moved = out[index];
    let prev = out[index - 1];
    let next = out[index + 1];

    let new_prev = trim_out(prev, prev.end_time_exclusive() + delta)?;
    let new_next = trim_in(next, next.start_time() + delta)?;
    out[index - 1] = new_prev;
    out[index] = TimeRange::new(moved.start_time() + delta, moved.duration());
    out[index + 1] = new_next;
    Ok(out)
}

/// Move the shared cut between two adjacent spans to `new_edge`.
///
/// Guarantee: the pair's outer edges and combined duration are unchanged, or
/// [`EditError::NotAdjacent`] / [`EditError::EmptySpan`].
pub fn roll(
    left: TimeRange,
    right: TimeRange,
    new_edge: RationalTime,
) -> Result<(TimeRange, TimeRange), EditError> {
    if left.end_time_exclusive() != right.start_time() {
        return Err(EditError::NotAdjacent);
    }
    Ok((trim_out(left, new_edge)?, trim_in(right, new_edge)?))
}

/// Pack `durations` end-to-end starting at `origin`.
fn lay_contiguously(
    origin: RationalTime,
    durations: impl Iterator<Item = RationalTime>,
) -> Vec<TimeRange> {
    let mut cursor = origin;
    durations
        .map(|duration| {
            let range = TimeRange::new(cursor, duration);
            cursor += duration;
            range
        })
        .collect()
}

/// Half a duration, at the same rate.
fn half(t: RationalTime) -> RationalTime {
    RationalTime::new(t.value() / 2.0, t.rate())
}

/// Reject a non-positive duration (rate-independent, so it is safe across rates).
fn positive(duration: RationalTime) -> Result<(), EditError> {
    if duration.to_seconds() > 0.0 {
        Ok(())
    } else {
        Err(EditError::EmptySpan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 24.0;

    /// Build a contiguous track from durations, starting at `origin`.
    fn track(origin: f64, durations: &[f64]) -> Vec<TimeRange> {
        lay_contiguously(
            RationalTime::new(origin, RATE),
            durations.iter().map(|&d| RationalTime::new(d, RATE)),
        )
    }

    fn durations(spans: &[TimeRange]) -> Vec<f64> {
        spans.iter().map(|s| s.duration().value()).collect()
    }

    fn starts(spans: &[TimeRange]) -> Vec<f64> {
        spans.iter().map(|s| s.start_time().value()).collect()
    }

    /// The reference permutation: remove `from`, insert at `to`.
    fn expected_order(len: usize, from: usize, to: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..len).collect();
        let moved = order.remove(from);
        order.insert(to, moved);
        order
    }

    /// The invariants [`reorder_contiguous`] promises, checked on one case.
    fn assert_reorder_invariants(spans: &[TimeRange], from: usize, to: usize) {
        let r = reorder_contiguous(spans, from, to);
        let len = spans.len();

        assert_eq!(r.order, expected_order(len, from, to), "permutation");
        assert_eq!(r.spans.len(), len);

        // Same multiset of durations.
        let mut before = durations(spans);
        let mut after = durations(&r.spans);
        before.sort_by(f64::total_cmp);
        after.sort_by(f64::total_cmp);
        assert_eq!(before, after, "duration multiset");

        // Same origin, same total extent, and contiguous by construction.
        assert_eq!(r.spans[0].start_time(), spans[0].start_time(), "origin");
        assert_eq!(
            r.spans[len - 1].end_time_exclusive(),
            spans[len - 1].end_time_exclusive(),
            "total extent"
        );
        for pair in r.spans.windows(2) {
            assert_eq!(pair[0].end_time_exclusive(), pair[1].start_time(), "contiguous");
        }

        // The durations follow the permutation.
        let permuted: Vec<f64> = r.order.iter().map(|&i| spans[i].duration().value()).collect();
        assert_eq!(durations(&r.spans), permuted, "order follows permutation");
    }

    /// Exhaustive over every (len, from, to) for 1..=5 spans, with uneven
    /// durations so a wrong permutation cannot pass by symmetry.
    ///
    /// The workspace has no proptest/quickcheck dev-dependency, so the
    /// invariants are pinned by exhaustive small-case enumeration instead of
    /// randomised property testing — for a permutation algebra of this size the
    /// enumeration is a superset of what a property test would sample.
    #[test]
    fn reorder_invariants_hold_for_every_small_case() {
        let all = [7.0, 3.0, 11.0, 5.0, 2.0];
        for len in 1..=all.len() {
            let spans = track(100.0, &all[..len]);
            for from in 0..len {
                for to in 0..len {
                    assert_reorder_invariants(&spans, from, to);
                }
            }
        }
    }

    #[test]
    fn reorder_to_same_index_is_a_no_op() {
        let spans = track(0.0, &[7.0, 3.0, 11.0, 5.0]);
        for i in 0..spans.len() {
            let r = reorder_contiguous(&spans, i, i);
            assert_eq!(r.spans, spans);
            assert_eq!(r.order, vec![0, 1, 2, 3]);
        }
    }

    #[test]
    fn reorder_first_to_last_and_last_to_first() {
        let spans = track(0.0, &[7.0, 3.0, 11.0, 5.0]);

        let first_to_last = reorder_contiguous(&spans, 0, 3);
        assert_eq!(first_to_last.order, vec![1, 2, 3, 0]);
        assert_eq!(durations(&first_to_last.spans), vec![3.0, 11.0, 5.0, 7.0]);
        assert_eq!(starts(&first_to_last.spans), vec![0.0, 3.0, 14.0, 19.0]);

        let last_to_first = reorder_contiguous(&spans, 3, 0);
        assert_eq!(last_to_first.order, vec![3, 0, 1, 2]);
        assert_eq!(durations(&last_to_first.spans), vec![5.0, 7.0, 3.0, 11.0]);
        assert_eq!(starts(&last_to_first.spans), vec![0.0, 5.0, 12.0, 15.0]);
    }

    /// Moving from `i` to `j` and straight back restores the track exactly.
    #[test]
    fn reorder_round_trip_restores_the_track() {
        let spans = track(0.0, &[7.0, 3.0, 11.0, 5.0, 2.0]);
        for from in 0..spans.len() {
            for to in 0..spans.len() {
                let there = reorder_contiguous(&spans, from, to);
                let back = reorder_contiguous(&there.spans, to, from);
                assert_eq!(back.spans, spans, "round trip {from} -> {to}");
            }
        }
    }

    /// Out-of-range indices clamp instead of failing (the operation is total).
    #[test]
    fn reorder_clamps_out_of_range_indices() {
        let spans = track(0.0, &[7.0, 3.0, 11.0]);
        assert_eq!(
            reorder_contiguous(&spans, 99, 0),
            reorder_contiguous(&spans, 2, 0)
        );
        assert_eq!(reorder_contiguous(&[], 3, 4).spans, Vec::new());
    }

    /// Dropping an element back on its own leading edge selects its own index.
    #[test]
    fn drop_on_own_start_is_a_no_op_for_every_element() {
        let all = [7.0, 3.0, 11.0, 5.0, 2.0];
        for len in 1..=all.len() {
            let spans = track(100.0, &all[..len]);
            for i in 0..len {
                assert_eq!(drop_index(&spans, i, spans[i].start_time()), i, "len {len} idx {i}");
            }
        }
    }

    #[test]
    fn drop_index_needs_half_a_neighbour_to_displace_it() {
        // starts: 0, 10, 20 with durations 10, 10, 10.
        let spans = track(0.0, &[10.0, 10.0, 10.0]);
        let at = |v: f64| RationalTime::new(v, RATE);

        // Dragging clip 0 right: past 5 (half of clip 1) it takes index 1.
        assert_eq!(drop_index(&spans, 0, at(4.0)), 0);
        assert_eq!(drop_index(&spans, 0, at(6.0)), 1);
        assert_eq!(drop_index(&spans, 0, at(16.0)), 2);
        assert_eq!(drop_index(&spans, 0, at(999.0)), 2);

        // Dragging clip 2 left: below 15 (midpoint of clip 1) it takes index 1.
        assert_eq!(drop_index(&spans, 2, at(14.0)), 1);
        assert_eq!(drop_index(&spans, 2, at(4.0)), 0);
        assert_eq!(drop_index(&spans, 2, at(-50.0)), 0);
    }

    /// The mapping must never jump backwards as the drop point moves right.
    #[test]
    fn drop_index_is_monotonic() {
        let spans = track(0.0, &[7.0, 3.0, 11.0, 5.0]);
        for from in 0..spans.len() {
            let mut last = 0;
            for step in -20..60 {
                let idx = drop_index(&spans, from, RationalTime::new(f64::from(step), RATE));
                assert!(idx >= last, "monotonic at {step} from {from}");
                assert!(idx < spans.len());
                last = idx;
            }
        }
    }

    /// A drop always names an index `reorder_contiguous` accepts, so the pair
    /// composes into a total drag.
    #[test]
    fn drop_then_reorder_is_total() {
        let spans = track(0.0, &[7.0, 3.0, 11.0, 5.0]);
        for from in 0..spans.len() {
            for step in -20..60 {
                let to = drop_index(&spans, from, RationalTime::new(f64::from(step), RATE));
                let r = reorder_contiguous(&spans, from, to);
                assert_eq!(r.order.len(), spans.len());
                assert_eq!(
                    r.spans[3].end_time_exclusive(),
                    spans[3].end_time_exclusive()
                );
            }
        }
    }

    #[test]
    fn ripple_move_pushes_only_later_spans() {
        let spans = track(0.0, &[10.0, 10.0, 10.0]);
        let out = ripple_move(&spans, 1, RationalTime::new(15.0, RATE)).unwrap();
        assert_eq!(starts(&out), vec![0.0, 15.0, 25.0]);
        assert_eq!(durations(&out), vec![10.0, 10.0, 10.0]);
    }

    #[test]
    fn ripple_move_refuses_to_overlap_backwards() {
        let spans = track(0.0, &[10.0, 10.0, 10.0]);
        assert_eq!(
            ripple_move(&spans, 1, RationalTime::new(5.0, RATE)),
            Err(EditError::WouldOverlapPredecessor)
        );
        assert_eq!(
            ripple_move(&spans, 9, RationalTime::new(5.0, RATE)),
            Err(EditError::IndexOutOfBounds { index: 9, len: 3 })
        );
    }

    #[test]
    fn trims_hold_the_opposite_edge() {
        let span = TimeRange::from_floats(10.0, 10.0, RATE);
        let trimmed = trim_in(span, RationalTime::new(13.0, RATE)).unwrap();
        assert_eq!(trimmed, TimeRange::from_floats(13.0, 7.0, RATE));
        let trimmed = trim_out(span, RationalTime::new(17.0, RATE)).unwrap();
        assert_eq!(trimmed, TimeRange::from_floats(10.0, 7.0, RATE));

        assert_eq!(trim_in(span, RationalTime::new(20.0, RATE)), Err(EditError::EmptySpan));
        assert_eq!(trim_out(span, RationalTime::new(10.0, RATE)), Err(EditError::EmptySpan));
    }

    #[test]
    fn blade_splits_and_conserves_duration() {
        let span = TimeRange::from_floats(10.0, 10.0, RATE);
        let (left, right) = blade(span, RationalTime::new(14.0, RATE)).unwrap();
        assert_eq!(left, TimeRange::from_floats(10.0, 4.0, RATE));
        assert_eq!(right, TimeRange::from_floats(14.0, 6.0, RATE));
        assert_eq!(left.end_time_exclusive(), right.start_time());

        for edge in [10.0, 20.0, 9.0, 21.0] {
            assert_eq!(
                blade(span, RationalTime::new(edge, RATE)),
                Err(EditError::CutOutsideSpan),
                "edge {edge}"
            );
        }
    }

    #[test]
    fn slide_moves_only_the_neighbours_edges() {
        let spans = track(0.0, &[10.0, 10.0, 10.0]);
        let out = slide(&spans, 1, RationalTime::new(3.0, RATE)).unwrap();
        assert_eq!(starts(&out), vec![0.0, 13.0, 23.0]);
        assert_eq!(durations(&out), vec![13.0, 10.0, 7.0]);
        assert_eq!(
            out[2].end_time_exclusive(),
            spans[2].end_time_exclusive(),
            "total extent"
        );

        assert_eq!(slide(&spans, 0, RationalTime::new(1.0, RATE)), Err(EditError::NeedsNeighbours));
        assert_eq!(slide(&spans, 2, RationalTime::new(1.0, RATE)), Err(EditError::NeedsNeighbours));
        assert_eq!(slide(&spans, 1, RationalTime::new(10.0, RATE)), Err(EditError::EmptySpan));
    }

    #[test]
    fn roll_moves_the_cut_and_conserves_the_pair() {
        let left = TimeRange::from_floats(0.0, 10.0, RATE);
        let right = TimeRange::from_floats(10.0, 10.0, RATE);
        let (l, r) = roll(left, right, RationalTime::new(14.0, RATE)).unwrap();
        assert_eq!(l, TimeRange::from_floats(0.0, 14.0, RATE));
        assert_eq!(r, TimeRange::from_floats(14.0, 6.0, RATE));
        assert_eq!(r.end_time_exclusive(), right.end_time_exclusive());

        let detached = TimeRange::from_floats(50.0, 10.0, RATE);
        assert_eq!(
            roll(left, detached, RationalTime::new(5.0, RATE)),
            Err(EditError::NotAdjacent)
        );
        assert_eq!(roll(left, right, RationalTime::new(0.0, RATE)), Err(EditError::EmptySpan));
    }
}
