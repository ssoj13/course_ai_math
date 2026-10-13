//! Flat graph container: `HashMap<Uuid, Node>` + per-graph [`DataBus`] + a
//! transient [`AdjacencyIndex`].
//!
//! Edges live in each consumer node's `attrs.<input>.source`; hierarchy lives in
//! per-node `parent`/`children`. The [`AdjacencyIndex`] is derived (never
//! serialized) and rebuilt after every structural mutation.

use std::collections::HashMap;

use uuid::Uuid;

use crate::bus::DataBus;
use crate::error::{CoreError, CoreResult};
use crate::graph::NodeId;
use crate::index::AdjacencyIndex;
use crate::name::{NameIndex, name_base};
use crate::node::Node;
use crate::registry::NodeTypeRegistry;
use crate::value_types::ValueTypes;
use crate::worker::WorkerSet;

/// One graph level. Nodes are flat; nesting is expressed via `parent`/`children`.
pub struct Graph {
    pub id: Uuid,
    pub nodes: HashMap<NodeId, Node>,
    /// Named slots + transient handles visible to all nodes in this graph.
    ///
    /// NOT public: during a cook the driver hands the bus to the cooking node as `ctx.bus` via a
    /// disjoint field borrow, so an outside observer must never be able to reach it mid-cook (and
    /// a panicking cook can never lose it — it is never moved out of the graph). Read/write it
    /// through [`Graph::bus`] / [`Graph::bus_mut`].
    pub(crate) bus: DataBus,
    /// Background workers owned by the nodes of this graph (see [`WorkerSet`]).
    ///
    /// Held here rather than by the application so a worker cannot outlive the
    /// node that owns it: [`Self::remove_node`] stops it, and dropping the
    /// graph stops the rest. Transient — never serialized, empty after a load,
    /// restarted by the driver on the next cook.
    pub(crate) workers: WorkerSet,
    /// Transient topology (derived; rebuilt on mutation, never serialized).
    pub index: AdjacencyIndex,
    /// Per-scope name allocation registry (derived, never serialized). Unlike [`Self::index`]
    /// this is **eager** — maintained incrementally by [`Self::create_node`] /
    /// [`Self::remove_node`] / [`Self::set_parent`] so name allocation stays correct mid-bulk
    /// (allocation is read-modify-write and cannot tolerate a deferred/stale view). It is also
    /// reconstructed from scratch by [`Self::rebuild_index`], so every cold/bypass insert path
    /// (load/merge/script/cook) is covered for free.
    pub(crate) names: NameIndex,
    /// When set (inside [`Graph::bulk`]), mutating ops skip the per-op index rebuild and the
    /// index is rebuilt ONCE at the end — turns bulk edits (scene gen/clear) from O(n²) to O(n).
    defer_index: bool,
    /// Monotonic cook generation, bumped once at the start of every `cook` / `cook_dirty` pass.
    /// Each node the pass (re)computes is stamped with this value (`Node::set_cooked_gen`), so
    /// consumers can cheaply detect "this node produced fresh output" without diffing values —
    /// the freshness signal for live node previews (M8). Runtime-only, never serialized.
    cook_gen: u64,
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

impl Graph {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            nodes: HashMap::new(),
            bus: DataBus::new(),
            workers: WorkerSet::new(),
            index: AdjacencyIndex::default(),
            names: NameIndex::default(),
            defer_index: false,
            cook_gen: 0,
        }
    }

    pub fn with_id(id: Uuid) -> Self {
        Self {
            id,
            nodes: HashMap::new(),
            bus: DataBus::new(),
            workers: WorkerSet::new(),
            index: AdjacencyIndex::default(),
            names: NameIndex::default(),
            defer_index: false,
            cook_gen: 0,
        }
    }

    /// The graph's data bus (slots + transient handles). Read-only view; inside a cook the node
    /// gets `&mut DataBus` as `ctx.bus` instead (see [`crate::cook::CookCtx`]).
    pub fn bus(&self) -> &DataBus {
        &self.bus
    }

    /// Mutable access to the data bus, for hosts seeding a slot/handle BEFORE a cook (e.g.
    /// publishing a heavy `Arc` input the graph will consume).
    pub fn bus_mut(&mut self) -> &mut DataBus {
        &mut self.bus
    }

    /// The background workers this graph's nodes own.
    pub fn workers(&self) -> &WorkerSet {
        &self.workers
    }

    /// Mutable access, for a host installing or restarting a worker outside a
    /// cook (the driver installs them during one — see [`crate::NodeType`]).
    pub fn workers_mut(&mut self) -> &mut WorkerSet {
        &mut self.workers
    }

    /// Current cook generation (see the field docs). Bumped by the executor per pass.
    pub fn cook_gen(&self) -> u64 {
        self.cook_gen
    }

    /// Advance the cook generation by one and return the new value (executor-only, called at
    /// the start of each `cook` / `cook_dirty` pass so every node it touches gets a fresh stamp).
    pub(crate) fn bump_cook_gen(&mut self) -> u64 {
        self.cook_gen = self.cook_gen.wrapping_add(1);
        self.cook_gen
    }

    /// Rebuild the transient derived indices (adjacency + name registry) from current node
    /// data. This is the SSOT reconstruction for both: any path that bulk-inserts nodes
    /// directly into `self.nodes` MUST call this before the graph is used interactively
    /// (the adjacency index has the same precondition).
    pub fn rebuild_index(&mut self) {
        self.index = AdjacencyIndex::build(&self.nodes);
        self.names = NameIndex::build(&self.nodes);
        // Debug-only guard: catches any path that produced duplicate sibling names without
        // going through normalization (e.g. a script supplying colliding names). Release
        // builds skip it; the registry simply de-duplicates such names into one slot.
        #[cfg(debug_assertions)]
        NameIndex::debug_assert_unique(&self.nodes);
    }

    /// Rebuild the index unless we're inside a [`Graph::bulk`] batch (deferred to the end).
    fn maybe_rebuild_index(&mut self) {
        if !self.defer_index {
            self.rebuild_index();
        }
    }

    /// Run `f` with per-op index rebuilds deferred, then rebuild the index ONCE. Use for bulk
    /// mutations (e.g. generating or clearing a whole scene) to avoid O(n²) rebuilds. Nesting
    /// is safe — only the outermost call rebuilds. Bulk ops must not READ `self.index`
    /// mid-batch (it is stale until the closure returns).
    pub fn bulk<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let prev = self.defer_index;
        self.defer_index = true;
        let r = f(self);
        self.defer_index = prev;
        if !self.defer_index {
            self.rebuild_index();
        }
        r
    }

    /// Create a node of `type_name` at `view_pos`.
    pub fn create_node(
        &mut self,
        type_name: &str,
        view_pos: [f32; 2],
        registry: &NodeTypeRegistry,
        value_types: &ValueTypes,
    ) -> CoreResult<NodeId> {
        let def = registry.require(type_name)?;
        let id = NodeId::new();
        // New nodes are born at root scope (`parent == None`); a later `set_parent` /
        // `reparent_many` re-keys the name if the node is moved into a subnet. Registry
        // allocation is O(1) amortized and stays correct mid-bulk (eager).
        let node_name = self.names.alloc(None, &name_base(&def.type_name));
        let mut node = Node::from_def(id, def, &node_name, value_types);

        node.set_view_pos(crate::layout::snap_to_grid(view_pos));
        if &*def.type_name == crate::layout::FRAME_NODE_TYPE {
            node.set_view_size([
                crate::layout::DEFAULT_FRAME_W,
                crate::layout::DEFAULT_FRAME_H,
            ]);
        } else if &*def.type_name == crate::chrome::NOTE_NODE_TYPE {
            // Sticky note: user-resizable box + a placeholder body so a fresh note is visible and
            // immediately hints that it is editable.
            node.set_view_size([crate::chrome::DEFAULT_NOTE_W, crate::chrome::DEFAULT_NOTE_H]);
            node.set_note_text("Note");
        } else {
            let w = crate::layout::DEFAULT_NODE_W;
            // Content-aware default height: the same formula `compute_node_size`'s
            // Full-mode branch uses (port rows + the processing-info line), NOT a flat
            // 16:9-of-320 constant. A flat default meant every node type (a 1-port
            // Subnet, a 12-port Debug node, ...) got the identical 180px floor
            // regardless of how many rows it actually needed, so `full_view_height`
            // routinely disagreed with the node's real content height (the Subnet's
            // Full-mode status line floated ~126px away from its true bottom edge).
            let rows = node
                .input_names()
                .len()
                .max(node.output_names().len())
                .max(1);
            let h = crate::layout::node_height_rows(rows) + crate::chrome::PROCESSING_INFO_H;
            node.set_view_size([w, h]);
            // Remember this as the Full-mode body height so cycling through Minimal/Mid
            // and back to Full restores it (see `Node::full_view_height`).
            node.set_full_view_height(h);
        }
        // Image nodes preview their result by default — a fresh `LoadImage` shows its picture on the
        // node body without the user having to toggle the view kind (P). The renderer draws the
        // texture when `view_kind == Live2D` and the primary output is an RGBA8 buffer.
        if &*def.type_name == "LoadImage" {
            node.set_view_kind(crate::NodeViewKind::Live2D);
        }
        // `slot` (BusRead/BusWrite) and `key` (MapGet/MapInsert) are now regular
        // declared input ports — `Node::from_def` already seeded their defaults
        // above, so no special-casing is needed here anymore.

        self.nodes.insert(id, node);
        self.maybe_rebuild_index();
        Ok(id)
    }

    /// Connect `from_node`'s output `from_output` to `to_node`'s input `to_input`
    /// (by attribute name).
    ///
    /// Rejects with [`CoreError::InvalidWire`] when the output and input types are incompatible
    /// according to [`ValueTypes::compatible`] — the SAME kernel the cook driver runs on the real
    /// value, so the editor can never accept a wire whose value cook would then drop (which is
    /// exactly what happened to host-registered custom types while compatibility and conversion
    /// were two separate hardcoded tables).
    pub fn connect(
        &mut self,
        from_node: NodeId,
        from_output: &str,
        to_node: NodeId,
        to_input: &str,
        registry: &NodeTypeRegistry,
        value_types: &ValueTypes,
    ) -> CoreResult<()> {
        // Resolve source attr uuid and type.
        let (from_attr, from_ty) = {
            let src = self
                .nodes
                .get(&from_node)
                .ok_or_else(|| CoreError::InvalidWire("unknown source node".into()))?;
            let attr = src
                .output_attr_id(from_output)
                .ok_or_else(|| CoreError::InvalidWire(format!("unknown output {from_output:?}")))?;
            // Look up declared type from the NodeDef (if registered).
            let ty = registry
                .get(src.type_name())
                .and_then(|d| d.output_type(from_output))
                .unwrap_or("Any")
                .to_string();
            (attr, ty)
        };

        // Resolve consumer attr existence and type.
        let to_ty = {
            let consumer = self
                .nodes
                .get(&to_node)
                .ok_or_else(|| CoreError::InvalidWire("unknown consumer node".into()))?;
            if consumer.input_attr_id(to_input).is_none() {
                return Err(CoreError::InvalidWire(format!(
                    "unknown input {to_input:?}"
                )));
            }
            registry
                .get(consumer.type_name())
                .and_then(|d| d.input_type(to_input))
                .unwrap_or("Any")
                .to_string()
        };

        // Type-compatibility gate (value-type registry = the SSOT).
        if !value_types.compatible(&from_ty, &to_ty) {
            return Err(CoreError::InvalidWire(format!(
                "incompatible types: output '{from_output}' is '{from_ty}' \
                 but input '{to_input}' expects '{to_ty}'"
            )));
        }

        // Re-borrow mutably (the checks above held it shared). Infallible in practice — nothing
        // removes a node in between — but this fn already returns `CoreResult`, so it costs nothing
        // to say so with an error instead of a panic.
        let consumer = self
            .nodes
            .get_mut(&to_node)
            .ok_or(CoreError::NodeNotFound(to_node.0))?;
        consumer.set_source(to_input, from_node, &from_attr);
        consumer.set_dirty(true);
        self.maybe_rebuild_index();
        Ok(())
    }

    /// Index-based connect convenience: resolves port names then delegates to
    /// [`connect`](Self::connect).
    pub fn connect_idx(
        &mut self,
        from_node: NodeId,
        from_output: usize,
        to_node: NodeId,
        to_input: usize,
        registry: &NodeTypeRegistry,
        value_types: &ValueTypes,
    ) -> CoreResult<()> {
        let out_name = self
            .nodes
            .get(&from_node)
            .and_then(|n| n.output_names().get(from_output).cloned())
            .ok_or_else(|| CoreError::InvalidWire("output index out of range".into()))?;
        let in_name = self
            .nodes
            .get(&to_node)
            .and_then(|n| n.input_names().get(to_input).cloned())
            .ok_or_else(|| CoreError::InvalidWire("input index out of range".into()))?;
        self.connect(
            from_node,
            &out_name,
            to_node,
            &in_name,
            registry,
            value_types,
        )
    }

    /// Clear the source on `to_node`'s input `to_input` (by name) and rebuild.
    pub fn disconnect_input(&mut self, to_node: NodeId, to_input: &str) {
        if let Some(node) = self.nodes.get_mut(&to_node) {
            node.clear_source(to_input);
            node.set_dirty(true);
        }
        self.maybe_rebuild_index();
    }

    /// Remove `id` from the graph, cleaning up all dangling references:
    ///
    /// - Detaches the node from its parent (if any).
    /// - Clears the `parent` ref on all direct children and removes them from the
    ///   `children` list on the removed node (children are NOT recursively removed;
    ///   they become scope-root orphans with `parent == None`). Cascade-delete of a
    ///   Subnet's contents is intentionally left to higher-level UX (future work).
    /// - Clears `attrs.<input>.source` on every other node whose source points at
    ///   the removed node.
    /// - Removes the node and rebuilds the index.
    ///
    /// Returns `true` if the node existed, `false` otherwise.
    pub fn remove_node(&mut self, id: NodeId) -> bool {
        if !self.nodes.contains_key(&id) {
            return false;
        }
        // Before anything else: a deleted node must not leave its websocket,
        // poller or model thread running.
        self.workers.stop(id);

        // Capture the removed node's own (scope, name) for registry cleanup.
        let (rm_scope, rm_name) = self
            .nodes
            .get(&id)
            .map(|n| (n.parent(), n.name().to_string()))
            .unwrap_or((None, String::new()));

        // Detach from parent.
        let parent = self.nodes.get(&id).and_then(|n| n.parent());
        if let Some(pnode) = parent.and_then(|p| self.nodes.get_mut(&p)) {
            pnode.remove_child(id);
        }

        // Clear parent ref on all direct children (they become top-level). Their name scope
        // changes from `Some(id)` to `None`, so re-key the registry (uniquify on collision).
        let children: Vec<NodeId> = self
            .nodes
            .get(&id)
            .map(|n| n.children())
            .unwrap_or_default();
        for child in &children {
            let cname = self.nodes.get(child).map(|n| n.name().to_string());
            if let Some(cname) = cname {
                let final_name = self.names.reparent(Some(id), None, &cname);
                if let Some(cnode) = self.nodes.get_mut(child) {
                    cnode.set_parent(None);
                    if final_name != cname {
                        cnode.set_name(&final_name);
                    }
                }
            } else if let Some(cnode) = self.nodes.get_mut(child) {
                cnode.set_parent(None);
            }
        }

        // Clear all source refs on other nodes that pointed at `id`.
        let ids_to_patch: Vec<NodeId> = self.nodes.keys().copied().filter(|k| *k != id).collect();
        for nid in ids_to_patch {
            if let Some(node) = self.nodes.get_mut(&nid) {
                let inputs = node.input_names();
                for input in inputs {
                    if node
                        .source_of(&input)
                        .map(|(src_node, _)| src_node == id)
                        .unwrap_or(false)
                    {
                        node.clear_source(&input);
                        node.set_dirty(true);
                    }
                }
            }
        }

        self.nodes.remove(&id);
        self.names.remove(rm_scope, &rm_name);
        self.maybe_rebuild_index();
        true
    }

    /// Remove many nodes in a single O(n + edges) batch (e.g. "delete selection").
    ///
    /// Looping [`Self::remove_node`] is O(n²): each call rescans every node for dangling
    /// source refs AND triggers a full index/name rebuild. This batches all of it: one pass
    /// to clear sources feeding the removed set, surviving children reparented to root (via
    /// [`Self::set_parent`], whose index rebuild is deferred by the surrounding
    /// [`Self::bulk`]), then a single rebuild reconstructs the derived indices.
    pub fn remove_many(&mut self, ids: &[NodeId]) {
        use std::collections::HashSet;
        let remove: HashSet<NodeId> = ids
            .iter()
            .copied()
            .filter(|id| self.nodes.contains_key(id))
            .collect();
        if remove.is_empty() {
            return;
        }
        for id in &remove {
            self.workers.stop(*id);
        }
        self.bulk(|g| {
            // 1. Clear dangling source refs on survivors that point into the removed set.
            let survivors: Vec<NodeId> = g
                .nodes
                .keys()
                .copied()
                .filter(|k| !remove.contains(k))
                .collect();
            for nid in &survivors {
                if let Some(node) = g.nodes.get_mut(nid) {
                    for input in node.input_names() {
                        if node
                            .source_of(&input)
                            .map(|(src, _)| remove.contains(&src))
                            .unwrap_or(false)
                        {
                            node.clear_source(&input);
                            node.set_dirty(true);
                        }
                    }
                }
            }
            // 2. Detach removed nodes from surviving parents; reparent surviving children to
            //    root (set_parent re-keys the name registry + uniquifies on collision).
            for &id in &remove {
                let children: Vec<NodeId> =
                    g.nodes.get(&id).map(|n| n.children()).unwrap_or_default();
                for child in children {
                    if !remove.contains(&child) {
                        g.set_parent(child, None);
                    }
                }
                let parent = g.nodes.get(&id).and_then(|n| n.parent());
                if let Some(p) = parent
                    && !remove.contains(&p)
                    && let Some(pn) = g.nodes.get_mut(&p)
                {
                    pn.remove_child(id);
                }
            }
            // 3. Drop the nodes; the bulk-end rebuild reconstructs index + name registry
            //    from the final node map (removed names vanish automatically).
            for &id in &remove {
                g.nodes.remove(&id);
            }
        });
    }

    /// Set the parent of `child` to `parent`, keeping both `children` lists and the
    /// child's `parent` ref consistent, then rebuild the index.
    pub fn set_parent(&mut self, child: NodeId, parent: Option<NodeId>) {
        let old_parent = self.nodes.get(&child).and_then(|n| n.parent());
        if old_parent == parent {
            return;
        }
        if let Some(node) = old_parent.and_then(|old| self.nodes.get_mut(&old)) {
            node.remove_child(child);
        }
        // Re-key the name from the old scope to the new one; uniquify on collision (the new
        // scope may already hold this name). Renames are wire-safe — edges store uuids, not
        // names — so this only affects display / OpPath addressing.
        let cname = self.nodes.get(&child).map(|n| n.name().to_string());
        if let Some(node) = self.nodes.get_mut(&child) {
            node.set_parent(parent);
        }
        if let Some(cname) = cname {
            let final_name = self.names.reparent(old_parent, parent, &cname);
            if final_name != cname
                && let Some(node) = self.nodes.get_mut(&child)
            {
                node.set_name(&final_name);
            }
        }
        if let Some(node) = parent.and_then(|p| self.nodes.get_mut(&p)) {
            node.add_child(child);
        }
        self.maybe_rebuild_index();
    }

    /// Reparent many nodes under `parent` in one O(n) pass: re-keys each name in the registry
    /// (uniquify on collision), updates each child's `parent` ref, and appends them all to the
    /// parent's `children` list with a SINGLE rewrite (vs per-node [`Self::set_parent`], whose
    /// repeated `add_child` re-serializes the growing list — O(n²)). Use for bulk scene
    /// generation into a subnet scope.
    pub fn reparent_many(&mut self, children: &[NodeId], parent: Option<NodeId>) {
        for &child in children {
            let old_parent = self.nodes.get(&child).and_then(|n| n.parent());
            if old_parent == parent {
                continue;
            }
            if let Some(node) = old_parent.and_then(|old| self.nodes.get_mut(&old)) {
                node.remove_child(child);
            }
            let cname = self.nodes.get(&child).map(|n| n.name().to_string());
            if let Some(node) = self.nodes.get_mut(&child) {
                node.set_parent(parent);
            }
            if let Some(cname) = cname {
                let final_name = self.names.reparent(old_parent, parent, &cname);
                if final_name != cname
                    && let Some(node) = self.nodes.get_mut(&child)
                {
                    node.set_name(&final_name);
                }
            }
        }
        // One batched append into the new parent's children list.
        if let Some(p) = parent
            && let Some(pnode) = self.nodes.get_mut(&p)
        {
            pnode.add_children(children);
        }
        self.maybe_rebuild_index();
    }

    /// Direct children of `id` (from the index).
    pub fn children_of(&self, id: NodeId) -> Vec<NodeId> {
        self.index.children_of(id).to_vec()
    }

    /// Enable the display flag on `active` and clear it on all other nodes.
    pub fn set_display_node(&mut self, active: NodeId) {
        for (id, node) in &mut self.nodes {
            node.set_display_flag(*id == active);
        }
    }

    /// Toggle bypass on a node and mark it dirty.
    pub fn toggle_bypass(&mut self, id: NodeId) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.set_bypass(!node.bypass());
            node.set_dirty(true);
        }
    }
}
