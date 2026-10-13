//! Generic node-graph layout engine — **graph in, positions out**.
//!
//! Dependency-free and decoupled from the rest of the workspace (no `nodes-core`, `glam`,
//! or wgpu): a host describes its graph with the plain types here (opaque `u64` node ids,
//! `[f32; 2]` sizes/positions, pins as cross-axis offsets, edges referencing pin indices),
//! runs a [`LayoutEngine`], and writes the resulting positions back into its own model.
//!
//! # Engines
//! - [`SugiyamaLayout`] — the real layered ("Sugiyama-style") engine: longest-path ranking,
//!   barycenter crossing reduction, and a size-/pin-/stability-aware cross-axis placement.
//! - [`IdentityLayout`] — a no-op stub that returns each node's incoming `pos` unchanged
//!   (used where a real layout is not wanted).
//!
//! # Modes (one engine, driven by inputs)
//! - **Global reflow:** every node has `pinned == false`.
//! - **Local tidy:** only the nodes to arrange have `pinned == false`; the rest are
//!   `pinned == true` and act as fixed obstacles/anchors the movable nodes align to.
//! - **Incremental stability:** [`LayoutParams::stability`] `> 0` biases movable nodes
//!   toward their seed `pos`, and the initial ordering is seeded from the current layout, so
//!   small edits don't reshuffle the whole graph.
//!
//! # Axes
//! Internally the algorithm works in *primary* (the flow axis, along which layers advance)
//! and *cross* (perpendicular, within a layer) coordinates; [`Direction`] maps those to
//! screen `x`/`y` at the boundary. Pins ([`LayoutNode::inputs`]/[`LayoutNode::outputs`]) are
//! cross-axis offsets, so the same swap handles them.

use std::collections::HashMap;

mod sugiyama;
pub use sugiyama::SugiyamaLayout;

/// A node to be placed. `id` is opaque to this crate — the host maps its own node key to a
/// `u64` and back.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutNode {
    /// Host-chosen opaque identity.
    pub id: u64,
    /// Bounding box `[w, h]`.
    pub size: [f32; 2],
    /// Current / seed top-left position. Used as the incremental-stability anchor and to
    /// seed the initial in-layer ordering; returned verbatim by [`IdentityLayout`].
    pub pos: [f32; 2],
    /// Input-pin attachment points as **cross-axis** offsets from the node's top-left
    /// (for `LeftRight` this is the y-offset of each left-side pin). Empty ⇒ edges attach to
    /// the node's cross-axis center.
    pub inputs: Vec<f32>,
    /// Output-pin attachment points (cross-axis offsets). Empty ⇒ center attachment.
    pub outputs: Vec<f32>,
    /// When `true`, the engine MUST NOT move this node — it stays at `pos` and acts as a
    /// fixed obstacle/anchor (used for local "tidy selection" layouts).
    pub pinned: bool,
}

impl LayoutNode {
    /// Convenience constructor for a movable node with center pin attachment.
    pub fn new(id: u64, size: [f32; 2], pos: [f32; 2]) -> Self {
        Self {
            id,
            size,
            pos,
            inputs: Vec::new(),
            outputs: Vec::new(),
            pinned: false,
        }
    }
}

/// A directed edge `from.output[from_pin] → to.input[to_pin]`. Pin indices out of range fall
/// back to center attachment. Edges naming unknown node ids are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutEdge {
    pub from: u64,
    pub from_pin: usize,
    pub to: u64,
    pub to_pin: usize,
}

impl LayoutEdge {
    /// Edge using the first output/input pin (or center if the node has none).
    pub fn simple(from: u64, to: u64) -> Self {
        Self { from, from_pin: 0, to, to_pin: 0 }
    }
}

/// The whole graph handed to a [`LayoutEngine`].
#[derive(Debug, Clone, Default)]
pub struct LayoutGraph {
    pub nodes: Vec<LayoutNode>,
    pub edges: Vec<LayoutEdge>,
}

/// Primary flow direction of the layered layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    /// Edges flow left → right; layers are columns. Typical for node editors.
    #[default]
    LeftRight,
    /// Edges flow top → bottom; layers are rows. Typical for flowcharts.
    TopBottom,
}

/// Tunables. Engines may ignore fields that don't apply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutParams {
    /// Flow direction.
    pub direction: Direction,
    /// Gap (px) between sibling nodes within the same layer (cross axis).
    pub node_gap: f32,
    /// Gap (px) between adjacent layers (primary axis).
    pub layer_gap: f32,
    /// Incremental-stability weight in `0..=1`: `0` ignores the seed `pos`, higher values
    /// pull movable nodes toward where they already are so small edits don't reshuffle.
    pub stability: f32,
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            direction: Direction::LeftRight,
            node_gap: 40.0,
            layer_gap: 80.0,
            stability: 0.0,
        }
    }
}

/// Computed routing for one edge (optional, for hosts that want straight long-edge routing).
/// `waypoints` are the cross-layer bend points (from dummy nodes), in screen space, ordered
/// source→target. `backward` is set when the edge was reversed to break a cycle (it visually
/// runs against the flow and the host may draw it as a back-arc).
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeRoute {
    pub from: u64,
    pub from_pin: usize,
    pub to: u64,
    pub to_pin: usize,
    pub waypoints: Vec<[f32; 2]>,
    pub backward: bool,
}

/// Computed placement: `id → top-left position` plus optional edge routes.
#[derive(Debug, Clone, Default)]
pub struct GraphLayout {
    pub positions: HashMap<u64, [f32; 2]>,
    pub routes: Vec<EdgeRoute>,
}

impl GraphLayout {
    /// Position computed for `id`, if any.
    pub fn pos(&self, id: u64) -> Option<[f32; 2]> {
        self.positions.get(&id).copied()
    }
}

/// A pluggable layout algorithm. Hosts depend only on this trait, so swapping engines is a
/// one-line change.
pub trait LayoutEngine {
    /// Compute positions for every node in `graph` under `params`.
    fn layout(&self, graph: &LayoutGraph, params: &LayoutParams) -> GraphLayout;
}

/// No-op placeholder engine: returns each node's incoming `pos` unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentityLayout;

impl LayoutEngine for IdentityLayout {
    fn layout(&self, graph: &LayoutGraph, _params: &LayoutParams) -> GraphLayout {
        GraphLayout {
            positions: graph.nodes.iter().map(|n| (n.id, n.pos)).collect(),
            routes: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_returns_input_positions() {
        let graph = LayoutGraph {
            nodes: vec![
                LayoutNode::new(10, [100.0, 30.0], [5.0, 7.0]),
                LayoutNode::new(20, [100.0, 30.0], [9.0, 1.0]),
            ],
            edges: vec![LayoutEdge::simple(10, 20)],
        };
        let out = IdentityLayout.layout(&graph, &LayoutParams::default());
        assert_eq!(out.pos(10), Some([5.0, 7.0]));
        assert_eq!(out.pos(20), Some([9.0, 1.0]));
        assert_eq!(out.positions.len(), 2);
    }
}
