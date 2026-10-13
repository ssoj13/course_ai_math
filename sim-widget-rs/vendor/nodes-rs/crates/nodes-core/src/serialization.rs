//! JSON save/load for [`Graph`] and clipboard merge (paste / duplicate).
//!
//! Document format: `{ "nodes": { "<uuid>": <Node.data JSON> } }`. Output attr
//! **values** are derived and blanked on save (recomputed by cook); attr ids,
//! sources, the ordered `inputs`/`outputs` lists, and `parent`/`children` are
//! preserved so connections survive a round-trip. Transient index excluded.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::box_model::{BoxValue, RustBox};
use crate::error::{CoreError, CoreResult};
use crate::graph::NodeId;
use crate::graph_container::Graph;
use crate::node::{Node, box_del, box_put};
use crate::node_type::NodeKind;
use crate::registry::NodeTypeRegistry;
use crate::value_types::ValueTypes;

/// Current on-disk / clipboard format version.
pub const SUBNET_FORMAT_VERSION: u32 = 2;

/// Portable graph document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetFile {
    pub format_version: u32,
    pub id: Uuid,
    /// uuid-string → node `data` JSON (outputs blanked).
    pub nodes: HashMap<String, serde_json::Value>,
    /// Named bus slots (`BusRead` / `BusWrite`).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub bus_slots: HashMap<String, serde_json::Value>,
}

/// Legacy per-node serialized form, retained for `nodes-editor` paste helpers that
/// still pass it around. Holds just the `data` JSON of one node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedNode {
    pub id: NodeId,
    pub data: serde_json::Value,
}

/// View-mode marker kept for downstream API stability (now stored in `data.view.mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerializedViewMode {
    Minimal,
    Mid,
    Full,
}

/// Result of merging a fragment into a live graph.
#[derive(Debug, Clone)]
pub struct PasteResult {
    pub id_map: HashMap<NodeId, NodeId>,
    pub pasted_roots: HashSet<NodeId>,
}

/// Options for [`merge_subnet_file`].
pub struct SubnetMergeOptions<'a> {
    pub registry: &'a NodeTypeRegistry,
    pub value_types: &'a ValueTypes,
    /// When true, every pasted node gets a new UUID (clipboard paste).
    pub remap_ids: bool,
    /// Added to each node `view.pos` after remap.
    pub position_offset: Option<[f32; 2]>,
}

/// JSON for one node's `data`, with its output values kept or blanked **according to the node's
/// [`crate::NodeKind`]**, and every transient section (`view.dirty`, `compute`, `tmp`) stripped.
///
/// - A [`crate::NodeKind::Dataflow`] node's outputs are **blanked**. Its output is a value derived from
///   its inputs: recomputing it is free and gives the same answer, so storing it is waste. (This was the
///   behaviour for EVERY node, under the crate's assumption that cook is cheap.)
/// - A [`crate::NodeKind::Job`] node's outputs are **written to the file**. Its output is not a value to
///   recompute but the RECORD of an effect that already happened on disk — a path to forty minutes of
///   GPU work. Throwing it away would mean re-doing the reconstruction just to rediscover a path that is
///   sitting on disk; and because the node then comes back with all its outputs present, it loads CLEAN
///   (see [`node_from_data`]) and nothing re-runs when someone merely opens the graph.
///
/// The registry is the authority on the kind. It is deliberately NOT copied onto the node instance: a
/// fact that belongs to the TYPE, duplicated into every instance, is a fact that can go stale the day
/// the type changes, with nothing to reconcile the two.
fn serialize_node_data(node: &Node, registry: &NodeTypeRegistry) -> serde_json::Value {
    let mut data = node.data.clone();
    if registry.kind_of(node) != NodeKind::Job {
        for out in node.output_names() {
            box_put(&mut data, &format!("outputs.{out}.value"), BoxValue::Null);
        }
    }
    // view.dirty, compute, and tmp are all transient/runtime-only; omit them so a
    // freshly loaded graph always starts clean and doesn't carry stale cook state.
    // Absent sections are fine (`box_del` treats a missing path as a no-op).
    box_del(&mut data, "view.dirty");
    box_del(&mut data, "compute");
    box_del(&mut data, "tmp");
    data.to_json_value()
}

/// Serialize a whole graph.
///
/// The registry is required because whether a node's outputs are saved follows from its
/// [`crate::NodeKind`] (see [`serialize_node_data`]) — the TYPE is the authority, so the save path has
/// to ask it.
pub fn subnet_to_file(graph: &Graph, registry: &NodeTypeRegistry) -> SubnetFile {
    let nodes = graph
        .nodes
        .iter()
        .map(|(id, node)| (id.to_string(), serialize_node_data(node, registry)))
        .collect();
    SubnetFile {
        format_version: SUBNET_FORMAT_VERSION,
        id: graph.id,
        nodes,
        bus_slots: graph.bus.to_json_map(),
    }
}

/// Extract a subgraph induced by `node_ids` (see [`subnet_to_file`] for why the registry is needed).
pub fn extract_fragment(
    graph: &Graph,
    node_ids: &HashSet<NodeId>,
    registry: &NodeTypeRegistry,
) -> SubnetFile {
    let nodes = node_ids
        .iter()
        .filter_map(|id| {
            graph
                .nodes
                .get(id)
                .map(|node| (id.to_string(), serialize_node_data(node, registry)))
        })
        .collect();
    SubnetFile {
        format_version: SUBNET_FORMAT_VERSION,
        id: graph.id,
        nodes,
        bus_slots: HashMap::new(),
    }
}

pub fn subnet_to_json_pretty(graph: &Graph, registry: &NodeTypeRegistry) -> CoreResult<String> {
    serde_json::to_string_pretty(&subnet_to_file(graph, registry))
        .map_err(|e| CoreError::Other(e.to_string()))
}

pub fn subnet_from_json(json: &str) -> CoreResult<SubnetFile> {
    serde_json::from_str(json).map_err(|e| CoreError::Other(e.to_string()))
}

pub fn save_subnet_json(
    path: impl AsRef<Path>,
    graph: &Graph,
    registry: &NodeTypeRegistry,
) -> CoreResult<()> {
    let text = subnet_to_json_pretty(graph, registry)?;
    fs::write(path, text).map_err(|e| CoreError::Other(e.to_string()))
}

pub fn load_subnet_json(path: impl AsRef<Path>) -> CoreResult<SubnetFile> {
    let text = fs::read_to_string(path).map_err(|e| CoreError::Other(e.to_string()))?;
    subnet_from_json(&text)
}

/// Build a [`Node`] from saved `data` JSON, with its dirtiness **derived from the data itself**.
///
/// This used to stamp `set_dirty(true)` unconditionally, which was the faithful counterpart of
/// "blank every output on save": nothing was restored, so everything had to be recomputed. Once a
/// [`crate::NodeKind::Job`]'s outputs are persisted that is no longer true, and an unconditional dirty
/// flag would mean the editor re-runs a 40-minute reconstruction simply because someone opened the
/// graph that reconstruction produced.
///
/// So dirtiness is a FACT about the loaded data, not a stamp:
/// **a node is dirty unless every declared output came back with a value** ([`Node::outputs_complete`]) —
/// which also (deliberately) keeps every zero-output SINK dirty, since it carries no evidence it ever ran.
///
/// This is the only place the rule is applied; everywhere else `view.dirty` remains the single source of
/// truth (and a node that has never cooked — a freshly created one, whose outputs may already hold their
/// declared DEFAULTS — has no `view.dirty` at all and so reads as dirty via `Node::dirty`'s default).
fn node_from_data(id: NodeId, data_json: serde_json::Value) -> Node {
    let data = RustBox::from_json_value(data_json, Default::default());
    let mut node = Node { id, data };
    let dirty = !node.outputs_complete();
    node.set_dirty(dirty);
    node
}

/// Replace `target` entirely from a file (stable node IDs preserved).
pub fn load_subnet_from_file(
    target: &mut Graph,
    file: SubnetFile,
    _registry: &NodeTypeRegistry,
    _value_types: &ValueTypes,
) -> CoreResult<()> {
    target.id = file.id;
    target.nodes.clear();
    target.bus.clear();
    target.bus.load_json_map(file.bus_slots)?;
    for (id_str, data_json) in file.nodes {
        let id = NodeId::parse(&id_str)
            .ok_or_else(|| CoreError::Other(format!("bad node uuid {id_str:?}")))?;
        target.nodes.insert(id, node_from_data(id, data_json));
    }
    crate::name::normalize_subnet_names(target);
    target.rebuild_index();
    Ok(())
}

/// Fresh UUIDs for every node and rewrite source / parent / children refs.
fn remap_ids(file: &mut SubnetFile) -> HashMap<NodeId, NodeId> {
    let mut map: HashMap<NodeId, NodeId> = HashMap::new();
    let keys: Vec<String> = file.nodes.keys().cloned().collect();
    for k in &keys {
        if let Some(old) = NodeId::parse(k) {
            map.insert(old, NodeId::new());
        }
    }
    // Rebuild the map with new keys + patched internal references.
    let mut new_nodes: HashMap<String, serde_json::Value> = HashMap::new();
    for (old_str, mut data) in file.nodes.drain() {
        let Some(old) = NodeId::parse(&old_str) else {
            continue;
        };
        let new = map[&old];
        patch_refs(&mut data, &map);
        new_nodes.insert(new.to_string(), data);
    }
    file.nodes = new_nodes;
    map
}

/// Rewrite `parent`, `children[]`, and `attrs.*.source.node` uuid strings via `map`.
fn patch_refs(data: &mut serde_json::Value, map: &HashMap<NodeId, NodeId>) {
    let remap = |s: &str| -> Option<String> {
        NodeId::parse(s)
            .and_then(|id| map.get(&id))
            .map(|n| n.to_string())
    };
    if let Some(obj) = data.as_object_mut() {
        if let Some(p) = obj.get("parent").and_then(|v| v.as_str()).and_then(remap) {
            obj.insert("parent".into(), serde_json::Value::String(p));
        }
        if let Some(children) = obj.get_mut("children").and_then(|v| v.as_array_mut()) {
            for c in children.iter_mut() {
                if let Some(n) = c.as_str().and_then(remap) {
                    *c = serde_json::Value::String(n);
                }
            }
        }
        if let Some(attrs) = obj.get_mut("attrs").and_then(|v| v.as_object_mut()) {
            for attr in attrs.values_mut() {
                let Some(src) = attr.get_mut("source").and_then(|v| v.as_object_mut()) else {
                    continue;
                };
                if let Some(n) = src.get("node").and_then(|v| v.as_str()).and_then(remap) {
                    src.insert("node".into(), serde_json::Value::String(n));
                }
            }
        }
    }
}

fn offset_positions(file: &mut SubnetFile, delta: [f32; 2]) {
    for data in file.nodes.values_mut() {
        let Some(pos) = data.pointer_mut("/view/pos").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        if pos.len() == 2 {
            let x = pos[0].as_f64().unwrap_or(0.0) + delta[0] as f64;
            let y = pos[1].as_f64().unwrap_or(0.0) + delta[1] as f64;
            pos[0] = serde_json::json!(x);
            pos[1] = serde_json::json!(y);
        }
    }
}

/// Minimum `view.pos` among nodes in a fragment (for anchoring paste at cursor).
pub fn fragment_min_position(file: &SubnetFile) -> [f32; 2] {
    let mut min = [f32::MAX, f32::MAX];
    for data in file.nodes.values() {
        let Some(pos) = data.pointer("/view/pos").and_then(|v| v.as_array()) else {
            continue;
        };
        if pos.len() == 2 {
            min[0] = min[0].min(pos[0].as_f64().unwrap_or(0.0) as f32);
            min[1] = min[1].min(pos[1].as_f64().unwrap_or(0.0) as f32);
        }
    }
    if min[0] == f32::MAX { [0.0, 0.0] } else { min }
}

/// Assign fresh UUIDs to every node in the file (public for paste callers).
pub fn remap_node_ids(file: &mut SubnetFile) -> HashMap<NodeId, NodeId> {
    remap_ids(file)
}

/// Merge a file into `target`, optionally remapping IDs and shifting positions.
pub fn merge_subnet_file(
    target: &mut Graph,
    mut file: SubnetFile,
    options: SubnetMergeOptions<'_>,
) -> CoreResult<PasteResult> {
    let id_map = if options.remap_ids {
        remap_ids(&mut file)
    } else {
        HashMap::new()
    };
    if let Some(delta) = options.position_offset {
        offset_positions(&mut file, delta);
    }

    let mut pasted_roots = HashSet::new();
    for (id_str, data_json) in file.nodes {
        let id = NodeId::parse(&id_str)
            .ok_or_else(|| CoreError::Other(format!("bad node uuid {id_str:?}")))?;
        if target.nodes.contains_key(&id) && !options.remap_ids {
            return Err(CoreError::Other(format!(
                "node id collision on paste: {id}"
            )));
        }
        pasted_roots.insert(id);
        target.nodes.insert(id, node_from_data(id, data_json));
    }
    if options.remap_ids {
        crate::name::normalize_subnet_names(target);
    }
    target.rebuild_index();
    Ok(PasteResult {
        id_map,
        pasted_roots,
    })
}

/// Duplicate `node_ids` inside `graph` with a position offset (new UUIDs).
pub fn duplicate_selection(
    graph: &mut Graph,
    node_ids: &HashSet<NodeId>,
    offset: [f32; 2],
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
) -> CoreResult<PasteResult> {
    let fragment = extract_fragment(graph, node_ids, registry);
    merge_subnet_file(
        graph,
        fragment,
        SubnetMergeOptions {
            registry,
            value_types,
            remap_ids: true,
            position_offset: Some(offset),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Graph, NodeTypeRegistry, ValueTypes, register_builtins};

    // `register_builtins` registers the builtin VALUE types too (they are the `Builtins` library's
    // `NodeLibrary::value_types`) — a `register_builtin_types` call beside it would register each
    // twice, which now warns.
    fn fixtures() -> (NodeTypeRegistry, ValueTypes) {
        let mut types = ValueTypes::new();
        let mut reg = NodeTypeRegistry::new();
        register_builtins(&mut reg, &mut types);
        (reg, types)
    }

    #[test]
    fn roundtrip_stable_ids_and_wire() {
        let (reg, types) = fixtures();
        let mut graph = Graph::new();
        let a = graph
            .create_node("Float", [0.0, 0.0], &reg, &types)
            .unwrap();
        let b = graph
            .create_node("Add", [200.0, 0.0], &reg, &types)
            .unwrap();
        graph.connect_idx(a, 0, b, 0, &reg, &types).unwrap();
        graph.connect_idx(a, 0, b, 1, &reg, &types).unwrap();

        let json = subnet_to_json_pretty(&graph, &reg).unwrap();
        let file = subnet_from_json(&json).unwrap();
        let mut loaded = Graph::new();
        load_subnet_from_file(&mut loaded, file, &reg, &types).unwrap();

        assert_eq!(loaded.nodes.len(), 2);
        assert!(loaded.nodes.contains_key(&a));
        assert!(loaded.nodes.contains_key(&b));
        // Connection survived (consumer source resolves through the index).
        assert!(loaded.index.in_edge(b, "a").is_some());
        assert!(loaded.index.in_edge(b, "b").is_some());
    }

    #[test]
    fn roundtrip_parent_child() {
        let (reg, types) = fixtures();
        let mut graph = Graph::new();
        let parent = graph
            .create_node("Subnet", [0.0, 0.0], &reg, &types)
            .unwrap();
        let child = graph
            .create_node("Float", [0.0, 0.0], &reg, &types)
            .unwrap();
        graph.set_parent(child, Some(parent));

        let json = subnet_to_json_pretty(&graph, &reg).unwrap();
        let file = subnet_from_json(&json).unwrap();
        let mut loaded = Graph::new();
        load_subnet_from_file(&mut loaded, file, &reg, &types).unwrap();

        assert_eq!(loaded.nodes.get(&child).unwrap().parent(), Some(parent));
        assert_eq!(loaded.children_of(parent), vec![child]);
    }

    /// A stored `BoxValue::Object` attribute (the subnet bag boundary type) survives
    /// a JSON round-trip with all keys and value types intact.
    #[test]
    fn roundtrip_object_attr_value() {
        use crate::box_model::BoxValue;
        let (reg, types) = fixtures();
        let mut graph = Graph::new();
        let n = graph
            .create_node("MapInsert", [0.0, 0.0], &reg, &types)
            .unwrap();

        let mut obj = std::collections::BTreeMap::new();
        obj.insert("alpha".to_string(), BoxValue::F64(1.5));
        obj.insert("beta".to_string(), BoxValue::String("hi".to_string()));
        let value = BoxValue::obj(obj);
        graph
            .nodes
            .get_mut(&n)
            .unwrap()
            .set_input_value("map", value.clone());

        let json = subnet_to_json_pretty(&graph, &reg).unwrap();
        let file = subnet_from_json(&json).unwrap();
        let mut loaded = Graph::new();
        load_subnet_from_file(&mut loaded, file, &reg, &types).unwrap();

        let restored = loaded
            .nodes
            .get(&n)
            .unwrap()
            .input_value("map")
            .expect("'map' attr must survive round-trip");
        assert_eq!(
            restored, value,
            "Object attr value changed across JSON round-trip"
        );
    }

    #[test]
    fn paste_duplicate_new_ids() {
        let (reg, types) = fixtures();
        let mut graph = Graph::new();
        let a = graph
            .create_node("Float", [10.0, 10.0], &reg, &types)
            .unwrap();
        let mut sel = HashSet::new();
        sel.insert(a);
        let result = duplicate_selection(&mut graph, &sel, [40.0, 0.0], &reg, &types).unwrap();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(result.pasted_roots.len(), 1);
        assert!(!result.pasted_roots.contains(&a));
    }
}
