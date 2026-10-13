//! Layout helpers for render hosts.

use nodes_core::{LayoutCache, Subnet, compute_static_layout};

/// Recompute [`LayoutCache`] from subnet node geometry (pin positions, node rects).
///
/// Prefer passing the result through [`crate::RenderParams::layout`] when the host
/// already called [`nodes_core::compute_static_layout`] for interaction in the same frame.
pub fn sync_layout_cache(subnet: &Subnet) -> LayoutCache {
    compute_static_layout(subnet)
}
