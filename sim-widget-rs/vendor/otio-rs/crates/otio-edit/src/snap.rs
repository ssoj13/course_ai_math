//! Snapping — pure model-math helpers the timeline widget uses to magnet a
//! dragged time onto meaningful timeline points (clip edges, track start, the
//! playhead the caller supplies).
//!
//! These functions are intentionally **Editor-free and pixel-free**: they take a
//! `candidate` time and a list of snap `targets`, both already in track-local
//! time, plus a `threshold` (a duration). The widget converts its pixel hysteresis
//! (e.g. "8 px") to a `threshold` `RationalTime` at the current zoom and passes it
//! in. Keeping the math here (not in the widget) means the snapping rule is tested
//! once, in the model's own units, and reused by any front-end.

use otio_algo::range_of_all_children;
use otio_core::{Document, NodeId};
use otio_time::RationalTime;

/// Snap `candidate` to the nearest `target` within `threshold`, else return
/// `candidate` unchanged.
///
/// "Nearest" is measured in seconds (rate-independent: each target is compared via
/// the [`RationalTime`] difference, whose magnitude is taken in seconds), so a
/// 24 fps candidate snaps correctly to a 48 fps edge. On an exact tie the *first*
/// equally-near target in `targets` wins (stable, order-defined). The returned
/// time is the matched target itself (so the caller lands exactly on the edge,
/// preserving its rate), not a rounded `candidate`.
pub fn snap(
    targets: &[RationalTime],
    candidate: RationalTime,
    threshold: RationalTime,
) -> RationalTime {
    // Compare distances in seconds so mixed-rate targets are ordered correctly.
    let thr = threshold.to_seconds().abs();
    let cand_s = candidate.to_seconds();

    // Tie tolerance: distances within this many seconds count as equal, so a
    // float-ULP difference between two genuinely-equidistant edges does not flip
    // the tie-break. Reuse the time module's default epsilon.
    let tie_eps = otio_time::DEFAULT_EPSILON_S;
    let mut best: Option<(f64, RationalTime)> = None;
    for &t in targets {
        let dist = (t.to_seconds() - cand_s).abs();
        if dist > thr {
            continue;
        }
        // Replace only on a *strictly* nearer target (beyond the tie tolerance);
        // an equal-or-farther target leaves the first winner in place.
        match best {
            Some((bd, _)) if dist >= bd - tie_eps => {}
            _ => best = Some((dist, t)),
        }
    }
    best.map(|(_, t)| t).unwrap_or(candidate)
}

/// Collect the snap targets a single `track` offers: the track start (`0` at the
/// track's own rate, from the children's rate) plus every child's `start_time`
/// and `end_time_exclusive` (the clip edges), in layout order.
///
/// Pure read over `range_of_all_children`; the widget calls this once per visible
/// track and feeds the union into [`snap`]. Duplicate edges (a clip's end equals
/// the next clip's start) are intentionally kept — [`snap`]'s tie-break makes that
/// harmless, and de-duping would cost an O(n²) compare for no behavioural gain.
pub fn snap_targets(doc: &Document, track: NodeId) -> Vec<RationalTime> {
    let ranges = match range_of_all_children(doc, track) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    // Track start at the layout's rate (fall back to 1 fps for an empty track).
    let rate = ranges
        .first()
        .map(|(_, r)| r.start_time().rate())
        .unwrap_or(1.0);
    let mut out = Vec::with_capacity(ranges.len() * 2 + 1);
    out.push(RationalTime::new(0.0, rate));
    for (_, r) in &ranges {
        out.push(r.start_time());
        out.push(r.end_time_exclusive());
    }
    out
}
