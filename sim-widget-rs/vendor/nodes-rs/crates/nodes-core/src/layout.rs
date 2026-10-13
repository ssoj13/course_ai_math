//! Static layout: node sizes, pin positions, wire endpoints, auto-layout.
//!
//! **Graph UV:** [`Node::view_pos`] is the **top-left** corner; +u right, +v down.
//! Geometry rules live in [`crate::chrome`]; this module builds [`LayoutCache`] for
//! `nodes-view` (SDF + pick) and `nodes-egui` (interaction).
//!
//! Call [`compute_static_layout`] each frame (or when topology/geometry changes).
//! Editor shortcut **L** calls [`auto_layout_subnet`].

use std::collections::HashMap;

use graph_layout::{
    Direction, LayoutEdge as GlEdge, LayoutEngine, LayoutGraph, LayoutNode as GlNode, LayoutParams,
    SugiyamaLayout,
};

use crate::chrome;
use crate::graph::NodeId;
use crate::graph_container::Graph;
use crate::node::Node;
use crate::view_mode::{NodeViewMode, visible_pin_indices};

pub use crate::chrome::PinSide;

pub use crate::chrome::{
    DEFAULT_FRAME_H, DEFAULT_FRAME_W, DEFAULT_NODE_W, DEFAULT_NOTE_H, DEFAULT_NOTE_W,
    FRAME_NODE_TYPE, GRID_STEP, MAX_NODE_H, MAX_NODE_W, MIN_NODE_H, MIN_NODE_W, NODE_ASPECT,
    NOTE_NODE_TYPE, PAD, PIN_LABEL_GAP, PIN_R, PORT_H, PORT_INLINE_H, PORT_INLINE_W, PORT_LABEL_W,
    PinEdgeLayout, PortRowParams, PortRowRects, TITLE_H, minimal_node_height, port_row_geometry,
    resolve_orientation,
};

/// Backdrop / frame node (no sockets, resizable box, hollow body pick).
pub fn is_frame_node(node: &Node) -> bool {
    node.type_name() == FRAME_NODE_TYPE
}

/// Sticky-note annotation node (no sockets, resizable box, draws its own text, SOLID pick).
pub fn is_note_node(node: &Node) -> bool {
    node.type_name() == crate::chrome::NOTE_NODE_TYPE
}

/// Backdrop-family node (Frame OR Note): the shared traits are "no sockets, user-resizable box,
/// excluded from the force sim, no title strip / chrome flags". Fill, pick style, and text drawing
/// still branch per concrete type (a Frame is a hollow translucent backdrop; a Note is a solid
/// text card), so use [`is_frame_node`] / [`is_note_node`] where those differ.
pub fn is_backdrop(node: &Node) -> bool {
    is_frame_node(node) || is_note_node(node)
}

/// World-space center and size for one node (from `view_pos` + [`compute_node_size`]).
#[derive(Debug, Clone, Default)]
pub struct LayoutNode {
    pub center: [f32; 2],
    pub size: [f32; 2],
    /// In-plane orientation (radians) of this node, mirrored from [`crate::node::Node::view_rotation`]
    /// by [`build_layout`]. The SINGLE authoritative angle every renderer reads (via [`Self::xform`])
    /// — so the body, pins, wires, inline widgets and text labels can never disagree about how the
    /// card is turned. `0` = axis-aligned.
    pub rotation: f32,
}

impl LayoutNode {
    /// The node's placement transform (center + rotation) — the SSOT used to rotate any
    /// node-attached geometry (points via [`NodeXform::point`], surfaces via a matrix built from it).
    pub fn xform(&self) -> NodeXform {
        NodeXform {
            center: self.center,
            rotation: self.rotation,
        }
    }
}

/// A node's in-plane placement transform: a rotation by `rotation` (radians) about `center`
/// (graph-UV). This is the ONE primitive every layer uses to place node-attached content, so
/// rotation can never desync between the body, pins, wires, inline widgets and text labels.
///
/// Node-attached geometry is authored in the node's UNROTATED local frame (as if `rotation == 0`);
/// applying this transform once — [`Self::point`] for a point, or the equivalent matrix for a
/// surface/quad — places it on the rotated card. [`Self::inverse_point`] maps a graph-UV point back
/// into the local frame (for hit-testing).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NodeXform {
    /// Rotation pivot (the node center) in graph-UV.
    pub center: [f32; 2],
    /// Rotation angle in radians (+ = the same sense as `shape.wgsl` / the widget/label transforms).
    pub rotation: f32,
}

impl NodeXform {
    /// `true` when this is the identity (no rotation) — the common fast path.
    pub fn is_identity(&self) -> bool {
        self.rotation == 0.0
    }

    /// Rotate a graph-UV `point` about [`Self::center`] by [`Self::rotation`] (local → placed).
    pub fn point(&self, p: [f32; 2]) -> [f32; 2] {
        if self.rotation == 0.0 {
            return p;
        }
        let (s, c) = self.rotation.sin_cos();
        let r = [p[0] - self.center[0], p[1] - self.center[1]];
        [
            self.center[0] + r[0] * c - r[1] * s,
            self.center[1] + r[0] * s + r[1] * c,
        ]
    }

    /// Inverse of [`Self::point`] (placed → local); used to hit-test in the node's local frame.
    pub fn inverse_point(&self, p: [f32; 2]) -> [f32; 2] {
        NodeXform {
            center: self.center,
            rotation: -self.rotation,
        }
        .point(p)
    }
}

#[derive(Debug, Clone)]
pub struct PinPos {
    /// World-space center of the pin circle.
    pub world: [f32; 2],
}

/// Cached geometry for one graph snapshot (read-only input to renderers).
#[derive(Debug, Clone, Default)]
pub struct LayoutCache {
    pub nodes: HashMap<NodeId, LayoutNode>,
    /// Straight-line wire endpoints (from output pin → to input pin), already finalized through
    /// each endpoint node's [`NodeXform`] (rotation-applied). Kept for consumers that draw plain
    /// 2D lines from the cache; the wgpu tube path in `nodes-view` does NOT read this — it takes
    /// pin endpoints via [`Self::pin_world`] (same transform), so both agree on rotated nodes.
    pub edges: Vec<([f32; 2], [f32; 2])>,
    /// Pin centers in each node's UNROTATED LOCAL frame (authoring space). Key:
    /// `(node_id, side, port_index)`. NOT the placed/rotated position — for that use
    /// [`Self::pin_world`], which is THE single accessor every WORLD-space consumer must go through.
    pub pin_positions: HashMap<(NodeId, PinSide, usize), PinPos>,
}

impl LayoutCache {
    /// Placed (rotation-applied) graph-UV center of a pin — the SINGLE source of truth for any
    /// WORLD-space consumer of pin positions (wire tubes, wire-drag preview, connection endpoints).
    ///
    /// # Why
    ///
    /// [`Self::pin_positions`] stores each pin in its owning node's UNROTATED local frame. The
    /// renderer draws the pin dot at that local position and rotates the whole node in-shader about
    /// [`LayoutNode::center`] (see `shape.wgsl`), so the DRAWN pin center is
    /// `node.xform().point(local)`. A consumer that read `pin_positions` directly (as the tube wire
    /// builder used to) would attach wires to the unrotated pin → wires visibly miss the sockets of
    /// any rotated node. Routing every world-space read through this method keeps wires, the pin
    /// dots and the node body in exact agreement at any rotation. Returns `None` if the pin or its
    /// node is absent.
    pub fn pin_world(&self, node: NodeId, side: PinSide, index: usize) -> Option<[f32; 2]> {
        let local = self.pin_positions.get(&(node, side, index))?.world;
        let xform = self
            .nodes
            .get(&node)
            .map(LayoutNode::xform)
            .unwrap_or_default();
        Some(xform.point(local))
    }
}

/// Height from the number of visible port rows (after view mode).
pub fn node_height_rows(visible_rows: usize) -> f32 {
    if visible_rows == 0 {
        TITLE_H + chrome::PAD
    } else {
        TITLE_H + visible_rows as f32 * PORT_H + chrome::PAD
    }
}

/// Height of a node given its port counts (full layout).
pub fn node_height(inputs: usize, outputs: usize) -> f32 {
    node_height_rows(inputs.max(outputs))
}

/// Height for a given width at [`NODE_ASPECT`] (16:9).
pub fn node_height_for_width(w: f32) -> f32 {
    w / NODE_ASPECT
}

/// `(width, height)` in pixels of a node's cooked primary output when it is an RGBA8 image
/// (`BoxValue::Buffer` U8 shape `[h, w, 4]`); `None` otherwise. The single source of truth for
/// "this node produced an image" — used both to SIZE the node to the picture's aspect here and to
/// draw the texture preview in `nodes-view`.
pub fn primary_image_dims(node: &Node) -> Option<(u32, u32)> {
    let name = node.output_names().into_iter().next()?;
    let v = node.output_value(&name)?;
    let (dtype, shape, _data) = v.as_buffer()?;
    if dtype == crate::DType::U8
        && shape.len() == 3
        && shape[2] == 4
        && shape[0] > 0
        && shape[1] > 0
    {
        Some((shape[1] as u32, shape[0] as u32)) // (w, h) from [h, w, 4]
    } else {
        None
    }
}

/// Full node size `[w, h]`: width from `view_size[0]`; height is 16:9 unless ports need more room.
pub fn compute_node_size(node: &Node) -> [f32; 2] {
    let vs = node.view_size();
    let w = vs[0].clamp(MIN_NODE_W, MAX_NODE_W);
    if is_backdrop(node) {
        let h = vs[1].clamp(MIN_NODE_H, MAX_NODE_H);
        return [w, h];
    }
    if node.view_mode() == NodeViewMode::Minimal {
        return [w, minimal_node_height()];
    }
    // Live2D image node: size the body to the picture's aspect so the preview isn't stretched. The
    // image fills `regions.body` (under the title), so node height = title + body(= w / aspect).
    // Width stays user/default; height follows. Reads the cooked output, so it tracks image edits.
    if node.view_kind() == crate::NodeViewKind::Live2D
        && let Some((iw, ih)) = primary_image_dims(node)
    {
        let body_h = w * ih as f32 / iw as f32;
        let h = (crate::chrome::TITLE_H + body_h).clamp(MIN_NODE_H, MAX_NODE_H);
        return [w, h];
    }

    let rows = pin_row_count(node);
    let mut h = if rows == 0 {
        minimal_node_height()
    } else {
        node_height_rows(rows)
    };
    if node.view_mode() == NodeViewMode::Full {
        // Full = Mid's content (already in `h`) + a processing-info line (see
        // `NodeViewMode` doc — levels are cumulative).
        h += crate::chrome::PROCESSING_INFO_H;
        // Read the DEDICATED Full-mode memory, not `view.size[1]` — that field is
        // overwritten with a short height every time the mode switches to Minimal/Mid, so
        // using it here would forget the original/custom Full height the moment the user
        // leaves Full mode (see `Node::full_view_height`).
        let stored_h = node.full_view_height().clamp(MIN_NODE_H, MAX_NODE_H);
        h = h.max(stored_h);
    }
    [w, h]
}

/// After size changes (e.g. view mode), keep the node center fixed in world space.
pub fn preserve_node_center_after_resize(node: &mut Node, old_size: [f32; 2], new_size: [f32; 2]) {
    let pos = node.view_pos();
    let cx = pos[0] + old_size[0] * 0.5;
    let cy = pos[1] + old_size[1] * 0.5;
    node.set_view_pos([cx - new_size[0] * 0.5, cy - new_size[1] * 0.5]);
}

/// Recompute node size from topology/view mode, write `view_size`, and preserve center.
pub fn apply_resized_view_size(node: &mut Node, old_size: [f32; 2]) {
    let new_size = compute_node_size(node);
    node.set_view_size(new_size);
    preserve_node_center_after_resize(node, old_size, new_size);
}

/// Change view mode, refresh `view_size`, and keep the node's TOP-LEFT (coord0) fixed.
///
/// The title bar carries the interactive chrome — the view-mode 3-strip button on the left and
/// the bypass/display flags on the right. This toggle is usually fired BY clicking that button,
/// so the top-left must stay pinned under the pointer: the node grows/shrinks DOWNWARD from the
/// title, and the button never slides out from under the cursor. (This deliberately differs from
/// [`apply_resized_view_size`], which keeps the center fixed for non-interactive re-fits.)
pub fn set_view_mode_and_resize(graph: &mut Graph, id: NodeId, new_mode: NodeViewMode) -> bool {
    let Some(node) = graph.nodes.get_mut(&id) else {
        return false;
    };
    if is_backdrop(node) || node.view_mode() == new_mode {
        return false;
    }
    node.set_view_mode(new_mode);
    node.set_dirty(true);
    // `set_view_size` leaves `view_pos` (the top-left) untouched, so the node resizes down/right
    // and the title bar stays put — no center-preserving shift here, on purpose.
    let new_size = compute_node_size(node);
    node.set_view_size(new_size);
    true
}

/// Cycle view mode and resize. Returns true if changed.
pub fn cycle_view_mode_and_resize(graph: &mut Graph, id: NodeId) -> bool {
    let mode = graph.nodes.get(&id).map(|n| n.view_mode().cycle());
    let Some(mode) = mode else {
        return false;
    };
    set_view_mode_and_resize(graph, id, mode)
}

/// Snap top-left position to [`GRID_STEP`].
pub fn snap_to_grid(pos: [f32; 2]) -> [f32; 2] {
    [
        (pos[0] / GRID_STEP).round() * GRID_STEP,
        (pos[1] / GRID_STEP).round() * GRID_STEP,
    ]
}

/// Gap after a layer (rank) before the next layer along **+u** (left → right).
const LAYOUT_GAP_RANK_U: f32 = 80.0;
/// Horizontal gap between siblings in the same layer.
const LAYOUT_GAP_NODE_U: f32 = 40.0;

/// Auto-place nodes with the default layered engine ([`SugiyamaLayout`]): DAG layers along
/// **+u** (left → right), siblings stacked along **+v**. Columns are snapped to the grid.
pub fn auto_layout_subnet(graph: &mut Graph) {
    auto_layout_subnet_with(graph, &SugiyamaLayout, true, None);
}

/// Build the generic [`graph_layout::LayoutGraph`] for exactly the nodes in `scope`
/// (`parent() == scope`, frame nodes excluded), plus the parallel `Vec<NodeId>` that maps engine
/// index → node id. Each [`GlNode::id`] is set to the node's index in the returned vec, so a
/// result position `pos(i)` maps straight back to `node_ids[i]`.
///
/// This is the SINGLE source of truth for turning the editor graph into layout-engine input,
/// shared by the one-shot [`auto_layout_subnet_with`] and the continuous
/// [`crate::force_sim::ForceSim`] — they must see the exact same nodes, seed positions and edges
/// or a "tidy" (L) and a "live" pass would disagree. Seed positions are each node's current
/// `view_pos` (top-left); per-socket cross-axis offsets come from the SAME
/// [`compute_static_layout_scoped`] the renderer uses, so the engine aligns to the exact sockets
/// the wires are drawn to (incl. view modes). Callers that read `graph.index` edges are
/// responsible for a fresh index (`rebuild_index`) beforehand.
pub fn build_layout_graph(graph: &Graph, scope: Option<NodeId>) -> (LayoutGraph, Vec<NodeId>) {
    let mut node_ids: Vec<NodeId> = graph
        .nodes
        .iter()
        .filter(|(_, n)| !is_backdrop(n) && n.parent() == scope)
        .map(|(id, _)| *id)
        .collect();
    node_ids.sort_by_key(|id| id.uuid());
    if node_ids.is_empty() {
        return (
            LayoutGraph {
                nodes: Vec::new(),
                edges: Vec::new(),
            },
            node_ids,
        );
    }

    // Map each NodeId to an opaque u64 (its sorted index) for the generic engine.
    let mut idx_by_id: HashMap<NodeId, u64> = HashMap::with_capacity(node_ids.len());
    for (i, id) in node_ids.iter().copied().enumerate() {
        idx_by_id.insert(id, i as u64);
    }

    // Socket geometry from the SAME function the renderer uses (single source of truth), so
    // the engine aligns to the exact socket the wires are actually drawn to (incl. view modes).
    // Scoped to the current edit level so offsets match the rendered scope.
    let pre = compute_static_layout_scoped(graph, scope);

    // Build the generic layout graph: sizes + seed positions + per-node socket cross-offsets
    // (y-offset of each input/output socket from the node top, for the left→right layout).
    let mut lnodes: Vec<GlNode> = Vec::with_capacity(node_ids.len());
    for (i, id) in node_ids.iter().copied().enumerate() {
        let node = graph.nodes.get(&id).expect("node id exists");
        let size = compute_node_size(node);
        let top_y = node.view_pos()[1];
        let center = size[1] * 0.5;
        let offsets = |side: PinSide, count: usize| -> Vec<f32> {
            (0..count)
                .map(|k| {
                    pre.pin_positions
                        .get(&(id, side, k))
                        .map(|p| p.world[1] - top_y)
                        .unwrap_or(center)
                })
                .collect()
        };
        let mut gn = GlNode::new(i as u64, size, node.view_pos());
        gn.inputs = offsets(PinSide::Input, node.input_names().len());
        gn.outputs = offsets(PinSide::Output, node.output_names().len());
        lnodes.push(gn);
    }

    // Edges carrying exact (output, input) socket indices, resolved exactly like
    // `compute_static_layout` (consumer-side `in_edge` + output-name lookup).
    let mut edges: Vec<GlEdge> = Vec::new();
    for &to in &node_ids {
        let Some(to_node) = graph.nodes.get(&to) else {
            continue;
        };
        for (in_idx, input) in to_node.input_names().iter().enumerate() {
            let Some(edge) = graph.index.in_edge(to, input) else {
                continue;
            };
            let Some(src) = graph.nodes.get(&edge.from_node) else {
                continue;
            };
            let Some(out_idx) = src
                .output_names()
                .iter()
                .position(|o| o == &edge.from_output)
            else {
                continue;
            };
            if let (Some(&a), Some(&b)) = (idx_by_id.get(&edge.from_node), idx_by_id.get(&to)) {
                edges.push(GlEdge {
                    from: a,
                    from_pin: out_idx,
                    to: b,
                    to_pin: in_idx,
                });
            }
        }
    }

    (
        LayoutGraph {
            nodes: lnodes,
            edges,
        },
        node_ids,
    )
}

/// Auto-place nodes using a specific [`LayoutEngine`] (so the editor can switch between the
/// CPU layered engine and the GPU force-directed one).
///
/// `snap_columns` snaps the flow axis (x) to the grid — correct for layered engines (tidy
/// columns) but wrong for organic engines (e.g. GPU force-directed), whose output has no
/// columns to snap; pass `false` there so the relaxed positions are kept verbatim.
///
/// `scope` limits the layout to one edit level: only nodes whose `parent() == scope` are
/// placed (and only edges between them), matching what the scoped viewport renders. Pass
/// `None` for the root level. Without this, pressing L inside a subnet would reflow every
/// level of the flat graph at once.
pub fn auto_layout_subnet_with(
    graph: &mut Graph,
    engine: &dyn LayoutEngine,
    snap_columns: bool,
    scope: Option<NodeId>,
) {
    if graph.nodes.is_empty() {
        return;
    }
    graph.rebuild_index();

    // SSOT graph construction (shared with the live `ForceSim`).
    let (lg, node_ids) = build_layout_graph(graph, scope);
    if node_ids.is_empty() {
        return;
    }
    let params = LayoutParams {
        direction: Direction::LeftRight,
        node_gap: LAYOUT_GAP_NODE_U,
        layer_gap: LAYOUT_GAP_RANK_U,
        // Editor "L" is a full global reflow: don't bias toward current positions.
        stability: 0.0,
    };
    // Run the chosen engine (layered Sugiyama or GPU force-directed) over the same graph.
    let result = engine.layout(&lg, &params);

    for (i, id) in node_ids.iter().copied().enumerate() {
        if let Some(top_left) = result.pos(i as u64)
            && let Some(node) = graph.nodes.get_mut(&id)
        {
            // For layered engines snap only the COLUMN axis (x, the flow axis) to the grid
            // so columns are tidy, keeping the cross axis (y) exact so socket-aligned wires
            // stay straight. Organic engines (force-directed) have no columns — keep both
            // axes verbatim (`snap_columns == false`).
            let x = if snap_columns {
                (top_left[0] / GRID_STEP).round() * GRID_STEP
            } else {
                top_left[0]
            };
            node.set_view_pos([x, top_left[1]]);
            node.set_dirty(true);
        }
    }
}

/// Axis-aligned UV bounds of all nodes in a layout cache.
pub fn layout_uv_bounds(layout: &LayoutCache) -> Option<([f32; 2], [f32; 2])> {
    let mut min = [f32::MAX, f32::MAX];
    let mut max = [f32::MIN, f32::MIN];
    for n in layout.nodes.values() {
        let hw = n.size[0] * 0.5;
        let hh = n.size[1] * 0.5;
        min[0] = min[0].min(n.center[0] - hw);
        min[1] = min[1].min(n.center[1] - hh);
        max[0] = max[0].max(n.center[0] + hw);
        max[1] = max[1].max(n.center[1] + hh);
    }
    if layout.nodes.is_empty() {
        None
    } else {
        Some((min, max))
    }
}

/// Bounds for a subset of nodes (by id).
pub fn layout_uv_bounds_selected(
    layout: &LayoutCache,
    selected: &[NodeId],
) -> Option<([f32; 2], [f32; 2])> {
    if selected.is_empty() {
        return None;
    }
    let mut min = [f32::MAX, f32::MAX];
    let mut max = [f32::MIN, f32::MIN];
    let mut any = false;
    for id in selected {
        let Some(n) = layout.nodes.get(id) else {
            continue;
        };
        any = true;
        let hw = n.size[0] * 0.5;
        let hh = n.size[1] * 0.5;
        min[0] = min[0].min(n.center[0] - hw);
        min[1] = min[1].min(n.center[1] - hh);
        max[0] = max[0].max(n.center[0] + hw);
        max[1] = max[1].max(n.center[1] + hh);
    }
    if any { Some((min, max)) } else { None }
}

/// Build layout restricted to nodes whose `parent` equals `scope`.
///
/// - `scope == None` → top-level nodes only (`parent == None`).
/// - `scope == Some(id)` → children of that compound node.
///
/// Use this for the editor viewport so that entering a Subnet shows only its
/// contents rather than the entire flat graph. Wire edges are only drawn between
/// two nodes that are both in scope.
/// Shared SSOT for both layout variants: place every node matching `keep`, and wire only edges
/// whose BOTH endpoints are kept (so a scoped layout never draws cross-scope wires).
fn build_layout(graph: &Graph, keep: impl Fn(&Node) -> bool) -> LayoutCache {
    let mut cache = LayoutCache::default();

    for (id, node) in &graph.nodes {
        if !keep(node) {
            continue;
        }
        let [w, h] = compute_node_size(node);
        let [px, py] = node.view_pos();

        let center = [px + w * 0.5, py + h * 0.5];
        cache.nodes.insert(
            *id,
            LayoutNode {
                center,
                size: [w, h],
                rotation: node.view_rotation(),
            },
        );

        if is_backdrop(node) {
            continue;
        }

        let vis_in = visible_pin_indices(node, PinSide::Input);
        let vis_out = visible_pin_indices(node, PinSide::Output);

        match node.view_mode() {
            NodeViewMode::Minimal => {
                layout_pins_minimal_merged(&mut cache, *id, [px, py, w, h], &vis_in, &vis_out);
            }
            NodeViewMode::Mid | NodeViewMode::Full => {
                let rows = pin_row_count(node);
                for &i in &vis_in {
                    cache.pin_positions.insert(
                        (*id, PinSide::Input, i),
                        PinPos {
                            world: chrome::body_pin_world(px, py, w, h, PinSide::Input, i, rows),
                        },
                    );
                }
                for &i in &vis_out {
                    cache.pin_positions.insert(
                        (*id, PinSide::Output, i),
                        PinPos {
                            world: chrome::body_pin_world(px, py, w, h, PinSide::Output, i, rows),
                        },
                    );
                }
            }
        }
    }

    // NOTE: `pin_positions` are stored in each node's UNROTATED LOCAL frame (authoring space).
    // The single node rotation ([`LayoutNode::xform`]) is applied downstream, uniformly: the
    // SDF/pin shapes and inline widgets/text labels each apply it once at draw time, and the wire
    // endpoints below are finalized through it here. This keeps rotation from ever desyncing.

    // Wire edges: only between two kept nodes. Endpoints are placed through each node's xform so a
    // rotated card's wires stay attached to its (rotated) sockets.
    for (to_id, to_node) in &graph.nodes {
        if !keep(to_node) {
            continue;
        }
        for (in_idx, input) in to_node.input_names().iter().enumerate() {
            let Some(edge) = graph.index.in_edge(*to_id, input) else {
                continue;
            };
            let Some(src) = graph.nodes.get(&edge.from_node) else {
                continue;
            };
            if !keep(src) {
                continue;
            }
            let Some(out_idx) = src
                .output_names()
                .iter()
                .position(|o| o == &edge.from_output)
            else {
                continue;
            };
            let out_key = (edge.from_node, PinSide::Output, out_idx);
            let in_key = (*to_id, PinSide::Input, in_idx);
            let ends = match (
                cache.pin_positions.get(&out_key),
                cache.pin_positions.get(&in_key),
            ) {
                (Some(a), Some(b)) => (a.world, b.world),
                _ => continue,
            };
            // Place each local socket through its own node's xform (owned copies → borrow of
            // `cache` ends before the `cache.edges` push below).
            let ax = cache
                .nodes
                .get(&edge.from_node)
                .map(|n| n.xform())
                .unwrap_or_default();
            let bx = cache
                .nodes
                .get(to_id)
                .map(|n| n.xform())
                .unwrap_or_default();
            cache.edges.push((ax.point(ends.0), bx.point(ends.1)));
        }
    }

    cache
}

/// Layout of ALL nodes (flattened, ignoring scope). O(N).
pub fn compute_static_layout(graph: &Graph) -> LayoutCache {
    build_layout(graph, |_| true)
}

/// Layout of one edit scope: only nodes whose `parent() == scope`, plus edges between them.
pub fn compute_static_layout_scoped(graph: &Graph, scope: Option<NodeId>) -> LayoutCache {
    build_layout(graph, move |n| n.parent() == scope)
}

/// Minimal view: every port on a side shares one merged socket position. `rect` is
/// the node's `[px, py, w, h]` in graph UV.
fn layout_pins_minimal_merged(
    cache: &mut LayoutCache,
    id: NodeId,
    rect: [f32; 4],
    vis_in: &[usize],
    vis_out: &[usize],
) {
    let [px, py, w, _] = rect;
    if !vis_in.is_empty() {
        let world = chrome::minimal_merged_pin_world(px, py, w, PinSide::Input);
        for &i in vis_in {
            cache
                .pin_positions
                .insert((id, PinSide::Input, i), PinPos { world });
        }
    }
    if !vis_out.is_empty() {
        let world = chrome::minimal_merged_pin_world(px, py, w, PinSide::Output);
        for &i in vis_out {
            cache
                .pin_positions
                .insert((id, PinSide::Output, i), PinPos { world });
        }
    }
}

/// Port rows for layout, sizing, and Y-aligned pins (shared across sides).
pub fn pin_row_count(node: &Node) -> usize {
    if is_backdrop(node) {
        return 0;
    }
    let vis_in = visible_pin_indices(node, PinSide::Input);
    let vis_out = visible_pin_indices(node, PinSide::Output);
    if vis_in.is_empty() && vis_out.is_empty() {
        return 0;
    }
    match node.view_mode() {
        NodeViewMode::Full => node
            .input_names()
            .len()
            .max(node.output_names().len())
            .max(1),
        _ => {
            let max_idx = vis_in
                .iter()
                .chain(vis_out.iter())
                .copied()
                .max()
                .unwrap_or(0);
            max_idx + 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PinSide, auto_layout_subnet, compute_node_size, compute_static_layout,
        compute_static_layout_scoped, pin_row_count, set_view_mode_and_resize,
    };
    use crate::view_mode::NodeViewMode;
    use crate::{Graph, NodeTypeRegistry, ValueTypes, register_builtins};

    // `register_builtins` registers the builtin VALUE types too (the `Builtins` library declares
    // them), so a separate `register_builtin_types` call would register each one twice.
    fn fixtures() -> (NodeTypeRegistry, ValueTypes) {
        let mut value_types = ValueTypes::new();
        let mut registry = NodeTypeRegistry::new();
        register_builtins(&mut registry, &mut value_types);
        (registry, value_types)
    }

    fn set_mode(graph: &mut Graph, id: crate::NodeId, mode: NodeViewMode) {
        graph.nodes.get_mut(&id).unwrap().set_view_mode(mode);
    }

    #[test]
    fn pins_inputs_left_outputs_right() {
        let (registry, value_types) = fixtures();
        let mut graph = Graph::new();
        let add = graph
            .create_node("Add", [100.0, 50.0], &registry, &value_types)
            .unwrap();
        set_mode(&mut graph, add, NodeViewMode::Full);
        let layout = compute_static_layout(&graph);
        let ln = layout.nodes.get(&add).unwrap();
        let px = graph.nodes.get(&add).unwrap().view_pos()[0];
        let w = ln.size[0];
        let in0 = layout.pin_positions.get(&(add, PinSide::Input, 0)).unwrap();
        let out0 = layout
            .pin_positions
            .get(&(add, PinSide::Output, 0))
            .unwrap();
        assert!((in0.world[0] - px).abs() < 0.01, "input pin on left border");
        assert!(
            (out0.world[0] - (px + w)).abs() < 0.01,
            "output pin on right border"
        );
        assert!((in0.world[0] - out0.world[0]).abs() > w * 0.5);
    }

    #[test]
    fn minimal_merged_pins_share_world_position() {
        let (registry, value_types) = fixtures();
        let mut graph = Graph::new();
        let id = graph
            .create_node("Add", [100.0, 50.0], &registry, &value_types)
            .unwrap();
        set_mode(&mut graph, id, NodeViewMode::Minimal);
        let layout = compute_static_layout(&graph);
        let in0 = layout.pin_positions.get(&(id, PinSide::Input, 0)).unwrap();
        let in1 = layout.pin_positions.get(&(id, PinSide::Input, 1)).unwrap();
        assert!((in0.world[0] - in1.world[0]).abs() < 1e-4);
        assert!((in0.world[1] - in1.world[1]).abs() < 1e-4);
        let out0 = layout.pin_positions.get(&(id, PinSide::Output, 0)).unwrap();
        assert!(
            (in0.world[1] - out0.world[1]).abs() < 1e-4,
            "merged in/out share Y"
        );
    }

    #[test]
    fn view_mode_resize_changes_height() {
        let (registry, value_types) = fixtures();
        let mut graph = Graph::new();
        let id = graph
            .create_node("Add", [100.0, 50.0], &registry, &value_types)
            .unwrap();
        {
            let n = graph.nodes.get_mut(&id).unwrap();
            n.set_view_mode(NodeViewMode::Full);
            // `compute_node_size`'s Full-mode floor reads `full_view_height`, not
            // `view.size[1]` (which Minimal/Mid overwrite) — set both explicitly so
            // this test exercises a deliberately-tall custom Full height rather than
            // relying on whatever the content-aware default seed happens to be.
            n.set_view_size([n.view_size()[0], 220.0]);
            n.set_full_view_height(220.0);
        }
        let full_h = compute_node_size(graph.nodes.get(&id).unwrap())[1];
        assert!(full_h > 100.0);
        assert!(set_view_mode_and_resize(
            &mut graph,
            id,
            NodeViewMode::Minimal
        ));
        let min_h = graph.nodes.get(&id).unwrap().view_size()[1];
        assert!(
            min_h < full_h * 0.6,
            "minimal should shrink below full height"
        );
    }

    #[test]
    fn body_pins_align_input_output_on_same_row() {
        let (registry, value_types) = fixtures();
        let mut graph = Graph::new();
        let id = graph
            .create_node("Add", [100.0, 50.0], &registry, &value_types)
            .unwrap();
        set_mode(&mut graph, id, NodeViewMode::Full);
        let layout = compute_static_layout(&graph);
        let in0 = layout.pin_positions.get(&(id, PinSide::Input, 0)).unwrap();
        let out0 = layout.pin_positions.get(&(id, PinSide::Output, 0)).unwrap();
        assert!((in0.world[1] - out0.world[1]).abs() < 1e-4);
    }

    #[test]
    fn pins_inputs_top_outputs_bottom_when_vertical() {
        let (registry, value_types) = fixtures();
        let mut graph = Graph::new();
        let id = graph
            .create_node("Add", [100.0, 50.0], &registry, &value_types)
            .unwrap();
        {
            let n = graph.nodes.get_mut(&id).unwrap();
            n.set_view_mode(NodeViewMode::Full);
            n.set_view_size([200.0, 420.0]);
            // `compute_node_size`'s Full-mode floor reads `full_view_height`, not
            // `view_size[1]` (see `Node::full_view_height`) — set both so this test's
            // 420px height actually takes effect.
            n.set_full_view_height(420.0);
        }
        let layout = compute_static_layout(&graph);
        let ln = layout.nodes.get(&id).unwrap();
        let py = graph.nodes.get(&id).unwrap().view_pos()[1];
        let h = ln.size[1];
        let in0 = layout.pin_positions.get(&(id, PinSide::Input, 0)).unwrap();
        let out0 = layout.pin_positions.get(&(id, PinSide::Output, 0)).unwrap();
        assert!((in0.world[1] - py).abs() < 0.01, "input pin on top border");
        assert!(
            (out0.world[1] - (py + h)).abs() < 0.01,
            "output pin on bottom border"
        );
        assert!((in0.world[1] - out0.world[1]).abs() > h * 0.5);
    }

    #[test]
    fn pin_row_count_mid_uses_max_linked_index() {
        let (registry, value_types) = fixtures();
        let mut graph = Graph::new();
        let a = graph
            .create_node("Add", [0.0, 0.0], &registry, &value_types)
            .unwrap();
        let b = graph
            .create_node("Add", [200.0, 0.0], &registry, &value_types)
            .unwrap();
        set_mode(&mut graph, a, NodeViewMode::Mid);
        graph
            .connect_idx(b, 0, a, 1, &registry, &value_types)
            .unwrap();
        let node = graph.nodes.get(&a).unwrap();
        assert_eq!(pin_row_count(node), 2);
    }

    #[test]
    fn full_node_height_from_port_rows_not_aspect() {
        let (registry, value_types) = fixtures();
        let mut graph = Graph::new();
        let id = graph
            .create_node("Add", [0.0, 0.0], &registry, &value_types)
            .unwrap();
        {
            let n = graph.nodes.get_mut(&id).unwrap();
            n.set_view_mode(NodeViewMode::Full);
            n.set_view_size([320.0, 0.0]);
            // Same as above: the Full-mode floor is `full_view_height`, not `view_size[1]`.
            // Zero it out too so port-row height is the only thing driving the result.
            n.set_full_view_height(0.0);
        }
        let h = compute_node_size(graph.nodes.get(&id).unwrap())[1];
        assert!(
            h < 120.0,
            "Full Add height should follow port rows, got {h}"
        );
    }

    #[test]
    fn auto_layout_siblings_spread_in_v() {
        let (registry, value_types) = fixtures();
        let mut graph = Graph::new();
        let mut ids = Vec::new();
        for _ in 0..4 {
            ids.push(
                graph
                    .create_node("Float", [0.0, 0.0], &registry, &value_types)
                    .unwrap(),
            );
        }
        auto_layout_subnet(&mut graph);
        let layout = compute_static_layout(&graph);
        let mut ys: Vec<f32> = ids
            .iter()
            .map(|id| layout.nodes.get(id).unwrap().center[1])
            .collect();
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(
            ys.last().copied().unwrap_or(0.0) - ys.first().copied().unwrap_or(0.0) > 80.0,
            "siblings should not collapse into one horizontal line, got {ys:?}"
        );
    }

    #[test]
    fn auto_layout_ranks_along_u_chain() {
        let (registry, value_types) = fixtures();
        let mut graph = Graph::new();
        let a = graph
            .create_node("Float", [0.0, 0.0], &registry, &value_types)
            .unwrap();
        let b = graph
            .create_node("Add", [0.0, 0.0], &registry, &value_types)
            .unwrap();
        let c = graph
            .create_node("Add", [0.0, 0.0], &registry, &value_types)
            .unwrap();
        graph
            .connect_idx(a, 0, b, 0, &registry, &value_types)
            .unwrap();
        graph
            .connect_idx(b, 0, c, 0, &registry, &value_types)
            .unwrap();
        auto_layout_subnet(&mut graph);
        let layout = compute_static_layout(&graph);
        let pa = layout.nodes.get(&a).unwrap().center;
        let pb = layout.nodes.get(&b).unwrap().center;
        let pc = layout.nodes.get(&c).unwrap().center;
        assert!(pb[0] > pa[0], "rank should advance along +u");
        assert!(pc[0] > pb[0], "rank should advance along +u");
        // Socket-aware alignment: the wire a.out -> b.in[0] should be (near) straight, i.e.
        // the connected sockets line up vertically. Node centers may differ by the socket
        // offset delta (that is the point of pin-aware placement), so check the SOCKETS.
        let a_out = layout
            .pin_positions
            .get(&(a, PinSide::Output, 0))
            .expect("a output pin")
            .world;
        let b_in = layout
            .pin_positions
            .get(&(b, PinSide::Input, 0))
            .expect("b input pin")
            .world;
        assert!(
            (a_out[1] - b_in[1]).abs() < 1.0,
            "wire a.out -> b.in should be straight (sockets aligned), got {a_out:?} {b_in:?}"
        );
    }

    #[test]
    fn auto_layout_parallel_nodes_share_rank_spread_v() {
        let (registry, value_types) = fixtures();
        let mut graph = Graph::new();
        let a = graph
            .create_node("Float", [0.0, 0.0], &registry, &value_types)
            .unwrap();
        let b = graph
            .create_node("Float", [0.0, 0.0], &registry, &value_types)
            .unwrap();
        let out = graph
            .create_node("Add", [0.0, 0.0], &registry, &value_types)
            .unwrap();
        graph
            .connect_idx(a, 0, out, 0, &registry, &value_types)
            .unwrap();
        graph
            .connect_idx(b, 0, out, 1, &registry, &value_types)
            .unwrap();
        auto_layout_subnet(&mut graph);
        let layout = compute_static_layout(&graph);
        let pa = layout.nodes.get(&a).unwrap().center;
        let pb = layout.nodes.get(&b).unwrap().center;
        let po = layout.nodes.get(&out).unwrap().center;
        assert!(
            (pb[0] - pa[0]).abs() < 5.0,
            "parallel nodes should share rank along +u, pa={pa:?} pb={pb:?}"
        );
        assert!(
            (pb[1] - pa[1]).abs() > 60.0,
            "parallel nodes should stack along +v, pa={pa:?} pb={pb:?}"
        );
        assert!(po[0] > pa[0] && po[0] > pb[0], "sink rank is farther +u");
    }

    /// C1 regression: entering a Subnet must show only its children, not root siblings.
    ///
    /// Graph shape:
    ///   root-scope:  subnet_node, sibling
    ///   subnet-scope (parent == subnet_node):  child_a, child_b
    ///
    /// `compute_static_layout_scoped(graph, None)` → {subnet_node, sibling}
    /// `compute_static_layout_scoped(graph, Some(subnet_node))` → {child_a, child_b}
    #[test]
    fn scoped_layout_shows_only_subnet_children() {
        let (registry, value_types) = fixtures();
        let mut graph = crate::Graph::new();

        // Create the subnet container node and an unrelated sibling at root level.
        let subnet_node = graph
            .create_node("Subnet", [0.0, 0.0], &registry, &value_types)
            .unwrap();
        let sibling = graph
            .create_node("Float", [200.0, 0.0], &registry, &value_types)
            .unwrap();

        // Create two child nodes inside the subnet.
        let child_a = graph
            .create_node("Float", [0.0, 0.0], &registry, &value_types)
            .unwrap();
        let child_b = graph
            .create_node("Float", [200.0, 0.0], &registry, &value_types)
            .unwrap();

        // Parent both children to the subnet node.
        graph.set_parent(child_a, Some(subnet_node));
        graph.set_parent(child_b, Some(subnet_node));

        // Root scope (None): only subnet_node and sibling are visible.
        let root_layout = compute_static_layout_scoped(&graph, None);
        assert!(
            root_layout.nodes.contains_key(&subnet_node),
            "root layout must include the subnet node"
        );
        assert!(
            root_layout.nodes.contains_key(&sibling),
            "root layout must include the sibling"
        );
        assert!(
            !root_layout.nodes.contains_key(&child_a),
            "root layout must NOT include child_a"
        );
        assert!(
            !root_layout.nodes.contains_key(&child_b),
            "root layout must NOT include child_b"
        );

        // Subnet scope (Some(subnet_node)): only the two children are visible.
        let subnet_layout = compute_static_layout_scoped(&graph, Some(subnet_node));
        assert!(
            subnet_layout.nodes.contains_key(&child_a),
            "subnet layout must include child_a"
        );
        assert!(
            subnet_layout.nodes.contains_key(&child_b),
            "subnet layout must include child_b"
        );
        assert!(
            !subnet_layout.nodes.contains_key(&subnet_node),
            "subnet layout must NOT include the subnet node itself"
        );
        assert!(
            !subnet_layout.nodes.contains_key(&sibling),
            "subnet layout must NOT include root sibling"
        );
    }
}
