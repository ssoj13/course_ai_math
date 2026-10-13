//! Transient adjacency index — derived from node `attrs.*.source` + `parent`/`children`.
//!
//! Never serialized; rebuilt on every structural mutation via
//! [`crate::graph_container::Graph::rebuild_index`]. It is the only source for
//! traversal, cook ordering, and connection lookup so there is no second source of
//! truth: the canonical edges live in each consumer node's `attrs.<input>.source`.

use std::collections::HashMap;

use crate::graph::NodeId;
use crate::node::Node;

/// One resolved incoming edge for an input: the source node and the **output attr
/// name** on that source (resolved from the stored source attr uuid).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InEdge {
    pub from_node: NodeId,
    pub from_output: String,
}

/// Derived graph topology (forward edges, reverse edges, children, topo order).
#[derive(Debug, Clone, Default)]
pub struct AdjacencyIndex {
    /// `(consumer_node, input_name) -> InEdge` (reverse: who feeds this input).
    in_edges: HashMap<(NodeId, String), InEdge>,
    /// `from_node -> [(to_node, to_input)]` (forward: where outputs go).
    out_edges: HashMap<NodeId, Vec<(NodeId, String)>>,
    /// `parent -> [child]` (mirrors each node's `children` list).
    children: HashMap<NodeId, Vec<NodeId>>,
}

impl AdjacencyIndex {
    /// Rebuild from the full node map. O(nodes + edges).
    pub fn build(nodes: &HashMap<NodeId, Node>) -> Self {
        let mut idx = Self::default();
        for (id, node) in nodes {
            // Connections: each input with a source.
            for input in node.input_names() {
                if let Some((from_node, from_attr)) = node.source_of(&input) {
                    // Resolve the source attr uuid to an output name on the source node.
                    let from_output = nodes
                        .get(&from_node)
                        .and_then(|src| src.attr_name_by_id(&from_attr));
                    if let Some(from_output) = from_output {
                        idx.in_edges.insert(
                            (*id, input.clone()),
                            InEdge {
                                from_node,
                                from_output: from_output.clone(),
                            },
                        );
                        idx.out_edges
                            .entry(from_node)
                            .or_default()
                            .push((*id, input.clone()));
                    }
                }
            }
            // Hierarchy.
            let kids = node.children();
            if !kids.is_empty() {
                idx.children.insert(*id, kids);
            }
        }
        idx
    }

    /// Resolved source feeding `(node, input)`.
    pub fn in_edge(&self, node: NodeId, input: &str) -> Option<&InEdge> {
        self.in_edges.get(&(node, input.to_string()))
    }

    /// All consumers fed by `node`'s outputs (forward edges).
    pub fn out_edges(&self, node: NodeId) -> &[(NodeId, String)] {
        self.out_edges.get(&node).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Direct children of `node`.
    pub fn children_of(&self, node: NodeId) -> &[NodeId] {
        self.children.get(&node).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Topological order over the data edges (Kahn). Cyclic / orphan nodes are
    /// appended in arbitrary (but stable per build) order so cook never drops a node.
    pub fn topo_order(&self, nodes: &HashMap<NodeId, Node>) -> Vec<NodeId> {
        let ids: Vec<NodeId> = nodes.keys().copied().collect();
        if ids.is_empty() {
            return Vec::new();
        }
        let mut in_degree: HashMap<NodeId, usize> = ids.iter().map(|id| (*id, 0)).collect();
        // in_degree counts incoming data edges.
        for ((to, _input), edge) in &self.in_edges {
            if nodes.contains_key(&edge.from_node) {
                *in_degree.entry(*to).or_default() += 1;
            }
        }
        let mut queue: Vec<NodeId> = in_degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(id, _)| *id)
            .collect();
        // Deterministic order for reproducibility.
        queue.sort_by_key(|id| id.uuid());
        let mut result = Vec::with_capacity(ids.len());
        let mut head = 0;
        while head < queue.len() {
            let nid = queue[head];
            head += 1;
            result.push(nid);
            let mut newly_ready: Vec<NodeId> = Vec::new();
            for (to, _input) in self.out_edges(nid) {
                if let Some(d) = in_degree.get_mut(to) {
                    *d -= 1;
                    if *d == 0 {
                        newly_ready.push(*to);
                    }
                }
            }
            newly_ready.sort_by_key(|id| id.uuid());
            queue.extend(newly_ready);
        }
        // Append any nodes left out by a cycle.
        if result.len() < ids.len() {
            let mut leftovers: Vec<NodeId> =
                ids.into_iter().filter(|id| !result.contains(id)).collect();
            leftovers.sort_by_key(|id| id.uuid());
            result.extend(leftovers);
        }
        result
    }
}
