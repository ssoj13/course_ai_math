//! Per-track layout cache for the timeline widget.
//!
//! The widget needs each item's `(start, duration)` in track coordinates to draw
//! clip rectangles. That comes from `otio_algo::range_of_all_children`, which is
//! O(n) per track. This cache memoizes the per-track layout `Vec` and recomputes
//! it only when the [`Editor`]'s revision advances. The Vec is start-sorted (as
//! the algorithm produces it), so the widget can binary-search the visible
//! `[t0,t1]` window and draw only on-screen items.
//!
//! v1 invalidation is coarse (any edit bumps the global revision → every track
//! recomputes on next access). A per-track dirty set derived from the committed
//! transaction's touched nodes is the v2 refinement (see PLAN.md).

use std::collections::HashMap;

use otio_algo::range_of_all_children;
use otio_core::NodeId;
use otio_time::TimeRange;

use crate::editor::Editor;

/// Memoized per-track child layouts, keyed by track `NodeId`.
#[derive(Default)]
pub struct LayoutCache {
    /// track id -> (revision it was computed at, layout).
    tracks: HashMap<NodeId, (u64, Vec<(NodeId, TimeRange)>)>,
}

impl LayoutCache {
    /// A fresh empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// The child layout for `track`, recomputed if the editor changed since it
    /// was last cached. Returns an empty slice if `track` is not a composition.
    pub fn track_layout(&mut self, ed: &Editor, track: NodeId) -> &[(NodeId, TimeRange)] {
        let rev = ed.revision();
        let entry = self.tracks.entry(track).or_insert((u64::MAX, Vec::new()));
        if entry.0 != rev {
            entry.1 = range_of_all_children(ed.document(), track).unwrap_or_default();
            entry.0 = rev;
        }
        &entry.1
    }

    /// Drop all cached layouts (e.g. after a structural change to which tracks
    /// exist). Individual tracks otherwise self-refresh on revision change.
    pub fn clear(&mut self) {
        self.tracks.clear();
    }
}
