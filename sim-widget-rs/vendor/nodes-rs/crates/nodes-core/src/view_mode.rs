//! Per-node display density (Blender-style minimal / mid / full).
//!
//! [`visible_pin_indices`] filters which ports [`crate::layout`] places and
//! `nodes-view` draws. Toggle with editor shortcut **M** on selection.

use crate::graph::NodeId;
use crate::graph_container::Graph;
use crate::layout::{PinSide, is_backdrop};
use crate::node::Node;

/// How much of a node is drawn in the graph editor. Cumulative/monotonic by design: each
/// level shows everything the previous level shows, plus more — not a different filtered
/// view (corrected 2026-07-05; Mid used to show only connected pins with no widgets, which
/// meant "2 bars" showed less than what users expect from a mid-detail level).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeViewMode {
    /// Title bar + summary strip only; no per-port labels or widgets (still wireable — pins
    /// exist, just not labeled).
    Minimal,
    /// Everything in Minimal, plus every pin labeled and inline widgets on unconnected
    /// inputs (identical port/widget content to Full; Full only adds processing info).
    Mid,
    /// Everything in Mid, plus a processing-info line (dirty/cooked status).
    #[default]
    Full,
}

impl NodeViewMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Minimal => Self::Mid,
            Self::Mid => Self::Full,
            Self::Full => Self::Minimal,
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Minimal => "Min",
            Self::Mid => "Mid",
            Self::Full => "Full",
        }
    }

    /// Numeric density level: `Minimal = 0`, `Mid = 1`, `Full = 2`. A port is DRAWN when its
    /// per-port display level (see [`Node::port_display_level`]) is `<=` this — so level-0 ports
    /// show even collapsed, level-1 from Mid up, level-2 only in Full.
    pub fn level(self) -> u8 {
        match self {
            Self::Minimal => 0,
            Self::Mid => 1,
            Self::Full => 2,
        }
    }
}

/// What a node's *body* renders (orthogonal to [`NodeViewMode`], which is title/port density).
///
/// - `Flat2D` (default): the plain rounded-rect body — today's look for every node.
/// - `Live2D`: a live 2D preview swatch driven by the node's cooked output (e.g. a colour /
///   scalar / image op result). `nodes-view` draws it in the body and regenerates it when the
///   node's [`crate::Node::cooked_gen`] bumps.
/// - `Live3D`: a 3D thumbnail (mesh / material / scene fragment). Reserved — the enum + storage
///   exist so hosts can already carry the intent; the 3D render path is a later phase.
///
/// Stored per-node in `view.view_kind` (int 0/1/2; JSON round-trips generically, rhai via the
/// optional `view_kind(id, kind)` builder). Node *types* may later declare a default kind; for
/// now every node defaults to `Flat2D` and the user toggles per node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeViewKind {
    /// Plain rounded-rect body (default).
    #[default]
    Flat2D,
    /// Live 2D preview swatch from cooked output.
    Live2D,
    /// Live 3D thumbnail (reserved; render path is a later phase).
    Live3D,
}

impl NodeViewKind {
    /// Cycle Flat2D -> Live2D -> Live3D -> Flat2D (editor toggle).
    pub fn cycle(self) -> Self {
        match self {
            Self::Flat2D => Self::Live2D,
            Self::Live2D => Self::Live3D,
            Self::Live3D => Self::Flat2D,
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Flat2D => "2D",
            Self::Live2D => "Live2D",
            Self::Live3D => "Live3D",
        }
    }

    /// Stable on-disk / rhai integer code.
    pub fn to_i64(self) -> i64 {
        match self {
            Self::Flat2D => 0,
            Self::Live2D => 1,
            Self::Live3D => 2,
        }
    }

    /// Inverse of [`Self::to_i64`]; unknown codes fall back to `Flat2D`.
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => Self::Live2D,
            2 => Self::Live3D,
            _ => Self::Flat2D,
        }
    }
}

/// Whether a pin is connected. For inputs this checks the consumer's own source;
/// for outputs it checks the forward edges in the transient index.
pub fn pin_is_connected(graph: &Graph, node: NodeId, side: PinSide, index: usize) -> bool {
    let Some(n) = graph.nodes.get(&node) else {
        return false;
    };
    match side {
        PinSide::Input => n
            .input_names()
            .get(index)
            .map(|name| n.is_connected(name))
            .unwrap_or(false),
        PinSide::Output => {
            let Some(out_name) = n.output_names().get(index).cloned() else {
                return false;
            };
            graph.index.out_edges(node).iter().any(|(to, input)| {
                graph
                    .index
                    .in_edge(*to, input)
                    .map(|e| e.from_node == node && e.from_output == out_name)
                    .unwrap_or(false)
            })
        }
    }
}

/// Port indices to lay out, pick, and label for this node.
pub fn visible_pin_indices(node: &Node, side: PinSide) -> Vec<usize> {
    if is_backdrop(node) {
        return Vec::new();
    }
    let count = match side {
        PinSide::Input => node.input_names().len(),
        PinSide::Output => node.output_names().len(),
    };
    // Mid and Full show the identical pin set (see the enum doc: levels are cumulative, not
    // separately filtered) — Mid used to show only *connected* pins, which left no visible
    // unconnected-input pin for the inline-widget code to anchor a control to.
    match node.view_mode() {
        NodeViewMode::Full | NodeViewMode::Mid => (0..count).collect(),
        NodeViewMode::Minimal => {
            if count == 0 {
                Vec::new()
            } else {
                (0..count).collect()
            }
        }
    }
}

/// Pin indices that get a DRAWN socket + name label for this node's CURRENT view mode.
///
/// A port is drawn when its per-port display level ([`Node::port_display_level`]) is `<=` the view
/// mode's [`NodeViewMode::level`] — level-0 ports show even collapsed, level-1 from Mid up, level-2
/// only in Full. This is the single source of truth for "what to draw", shared by node bodies
/// (`shape_scene`) and plane labels (`text_labels`).
///
/// Every pin still EXISTS for wiring / layout via [`visible_pin_indices`] (positions for all
/// ports, so connections never dangle); this only decides which get a visible socket + label.
/// NOTE: in Minimal the layout MERGES all sockets to one point, so a caller drawing collapsed
/// nodes should still take only the FIRST of these to avoid overlap (see `shape_scene`).
pub fn displayed_pin_indices(node: &Node, side: PinSide) -> Vec<usize> {
    let vmax = node.view_mode().level();
    visible_pin_indices(node, side)
        .into_iter()
        .filter(|&i| node.port_display_level(side, i) <= vmax)
        .collect()
}

/// Pin indices whose socket + name label are ACTUALLY drawn for this node's current view mode:
/// [`displayed_pin_indices`] with the Minimal-mode collapse applied.
///
/// In Minimal the layout MERGES every socket on a side to one point, so drawing more than the
/// first would stack them; this returns just the first there, and the full displayed set in
/// Mid/Full. It is the SINGLE source of truth for "which sockets/labels to draw", shared by node
/// bodies (`shape_scene`) and plane labels (`text_labels`) so a socket and its label can never
/// drift apart. (Wiring/layout still positions EVERY pin via [`visible_pin_indices`].)
pub fn drawn_pin_indices(node: &Node, side: PinSide) -> Vec<usize> {
    let displayed = displayed_pin_indices(node, side);
    if node.view_mode() == NodeViewMode::Minimal {
        displayed.into_iter().take(1).collect()
    } else {
        displayed
    }
}
