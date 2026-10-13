//! Rhai script serialization for [`Graph`] (design spec §3.6).
//!
//! A saved graph can be emitted as a **deterministic** Rhai script that rebuilds
//! it through a small graph-building API, and reloaded by running that script on
//! an engine whose registered functions reconstruct the [`Graph`].
//!
//! ## Identity preservation
//!
//! The script is identity-preserving: it emits explicit node uuids, attribute
//! uuids, attribute values, sources, the ordered `inputs`/`outputs` lists,
//! `parent`/`children` refs, per-node `params`, and graph-level bus slots.
//! Reconstructing from the script therefore produces a graph whose JSON
//! serialization (see [`crate::serialization`]) is **identical** to the
//! original — the Rhai round-trip matches the JSON round-trip.
//!
//! ## Builder API surface (the script functions)
//!
//! ```text
//! node(id, type, name, display_name, px, py)    // create a node skeleton
//! view(id, w, h, mode, bypass, display, full_h) // node view box / flags
//! attr_in(id, name, attr_id, json_value)         // add an INPUT attr (id + value)
//! attr_out(id, name, attr_id, json_value)        // add an OUTPUT attr (id + value)
//! input(id, name)                               // append name to input_order
//! output(id, name)                               // append name to output_order
//! set_source(id, name, src_node, src_attr_id)   // wire: consumer input <- source
//! parent(child_id, parent_id)                   // hierarchy edge
//! bus(name, json_value)                         // graph-level bus slot
//! ```
//!
//! `json_value` arguments are JSON strings using the same tagged form as the JSON
//! document (`{"$vec3":[..]}`, `{"$color":[..]}`, ...); they are parsed back via
//! [`BoxValue::from_json_value`], so `Vec3`/`Vec4`/`Mat*`/`Color` round-trip
//! losslessly.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rhai::{Engine, EvalAltResult};
use thiserror::Error;
use uuid::Uuid;

use crate::box_model::{BoxValue, RustBox};
use crate::graph::NodeId;
use crate::graph_container::Graph;
use crate::node::{Node, box_put};

/// Errors raised while emitting or evaluating a graph script.
#[derive(Debug, Error)]
pub enum ScriptError {
    /// The Rhai source failed to compile.
    #[error("rhai parse error: {0}")]
    Parse(String),
    /// The Rhai script failed at runtime (bad uuid, malformed JSON value, ...).
    #[error("rhai eval error: {0}")]
    Eval(String),
}

// ---- emit (to_rhai) ----------------------------------------------------------

/// Emit a deterministic Rhai script that rebuilds `graph`.
///
/// Determinism: nodes are visited sorted by uuid string; within a node,
/// attributes are emitted in stable key order (`attrs` is a `BTreeMap`, already
/// sorted), and the ordered `inputs`/`outputs` lists preserve port order. Bus
/// slots are emitted sorted by key. Two calls on the same graph produce
/// byte-identical output.
pub fn to_rhai(graph: &Graph) -> String {
    let mut out = String::new();
    out.push_str("// graph rebuilt via the nodes-core Rhai builder API\n");

    // Stable node order: sort by uuid string.
    let mut ids: Vec<&NodeId> = graph.nodes.keys().collect();
    ids.sort_by_key(|id| id.to_string());

    // Pass 1: declare nodes, their views, attrs, params, and ordered port lists.
    for id in &ids {
        let node = &graph.nodes[*id];
        emit_node(&mut out, node);
    }

    // Pass 2: wires + hierarchy (after every node exists, so refs resolve on load).
    for id in &ids {
        let node = &graph.nodes[*id];
        emit_links(&mut out, node);
    }

    // Pass 3: graph-level bus slots, sorted by key for determinism.
    emit_bus(&mut out, graph);

    out
}

fn emit_node(out: &mut String, node: &Node) {
    let id = node.id.to_string();
    let ty = node.type_name();
    let name = node.name();
    let display = node.display_name();
    let pos = node.view_pos();
    out.push_str(&format!(
        "node({}, {}, {}, {}, {}, {});\n",
        rstr(&id),
        rstr(ty),
        rstr(name),
        rstr(display),
        rnum(pos[0] as f64),
        rnum(pos[1] as f64),
    ));

    // View box + flags (kept lossless so JSON re-serialization matches). `full_h` is the
    // separate remembered Full-mode body height (see `Node::full_view_height`) — distinct
    // from `size[1]`, which is overwritten per view-mode change.
    let size = node.view_size();
    let mode = node.data.get_i64("view.mode").unwrap_or(2);
    let bypass = node.bypass();
    let display_flag = node.display_flag();
    let full_h = node.full_view_height();
    out.push_str(&format!(
        "view({}, {}, {}, {}, {}, {}, {});\n",
        rstr(&id),
        rnum(size[0] as f64),
        rnum(size[1] as f64),
        mode,
        bypass,
        display_flag,
        rnum(full_h as f64),
    ));

    // Node-body render kind, emitted only when non-default so the common case stays terse and
    // pre-existing scripts (no `view_kind` call) load as Flat2D. Kept a separate optional call
    // rather than an 8th `view(..)` arg to avoid breaking the fixed `view(..)` signature.
    let vk = node.view_kind();
    if vk != crate::NodeViewKind::Flat2D {
        out.push_str(&format!("view_kind({}, {});\n", rstr(&id), vk.to_i64()));
    }

    // Sticky-note body text, emitted only when non-empty (JSON is generic; rhai needs this matching
    // `note(..)` builder or the text silently drops on a rhai round-trip — see the module-doc caveat).
    let note = node.note_text();
    if !note.is_empty() {
        out.push_str(&format!("note({}, {});\n", rstr(&id), rstr(note)));
    }

    // Input/output attrs in stable key order (BTreeMap iteration is sorted) —
    // separate maps, separate builder calls (see module doc); `attr_in`/`attr_out`
    // mirror the two independent namespaces `Node::from_def` writes.
    if let Some(attrs) = node.data.get_path("inputs").and_then(BoxValue::as_object) {
        for (attr_name, attr_obj) in attrs {
            let Some(map) = attr_obj.as_object() else {
                continue;
            };
            let attr_id = map.get("id").and_then(BoxValue::as_str).unwrap_or("");
            let value = map.get("value").cloned().unwrap_or(BoxValue::Null);
            let json = value.to_json_value().to_string();
            out.push_str(&format!(
                "attr_in({}, {}, {}, {});\n",
                rstr(&id),
                rstr(attr_name),
                rstr(attr_id),
                rstr(&json),
            ));
        }
    }
    if let Some(attrs) = node.data.get_path("outputs").and_then(BoxValue::as_object) {
        for (attr_name, attr_obj) in attrs {
            let Some(map) = attr_obj.as_object() else {
                continue;
            };
            let attr_id = map.get("id").and_then(BoxValue::as_str).unwrap_or("");
            let value = map.get("value").cloned().unwrap_or(BoxValue::Null);
            let json = value.to_json_value().to_string();
            out.push_str(&format!(
                "attr_out({}, {}, {}, {});\n",
                rstr(&id),
                rstr(attr_name),
                rstr(attr_id),
                rstr(&json),
            ));
        }
    }

    // Ordered port-name lists (`input_order`/`output_order` — separate from the
    // `inputs`/`outputs` value maps above, since a BTreeMap can't preserve
    // declaration order).
    for input in node.input_names() {
        out.push_str(&format!("input({}, {});\n", rstr(&id), rstr(&input)));
    }
    for output in node.output_names() {
        out.push_str(&format!("output({}, {});\n", rstr(&id), rstr(&output)));
    }
}

fn emit_links(out: &mut String, node: &Node) {
    let id = node.id.to_string();
    // Wires: consumer input <- (source node, source attr uuid). Stable order via
    // the ordered inputs list.
    for input in node.input_names() {
        if let Some((src_node, src_attr)) = node.source_of(&input) {
            out.push_str(&format!(
                "set_source({}, {}, {}, {});\n",
                rstr(&id),
                rstr(&input),
                rstr(&src_node.to_string()),
                rstr(&src_attr),
            ));
        }
    }
    // Hierarchy: emit the parent edge from the child (children lists are rebuilt
    // from this on load, matching `Graph::set_parent`).
    if let Some(parent) = node.parent() {
        out.push_str(&format!(
            "parent({}, {});\n",
            rstr(&id),
            rstr(&parent.to_string()),
        ));
    }
}

fn emit_bus(out: &mut String, graph: &Graph) {
    if graph.bus.is_empty() {
        return;
    }
    // Collect + sort keys for determinism (DataBus uses HashMap internally).
    let mut keys: Vec<&str> = graph.bus.keys().collect();
    keys.sort_unstable();
    for key in keys {
        let value = graph.bus.get(key).cloned().unwrap_or(BoxValue::Null);
        let json = value.to_json_value().to_string();
        out.push_str(&format!("bus({}, {});\n", rstr(key), rstr(&json)));
    }
}

/// Quote a string as a Rhai string literal.
fn rstr(s: &str) -> String {
    let mut q = String::with_capacity(s.len() + 2);
    q.push('"');
    for ch in s.chars() {
        match ch {
            '"' => q.push_str("\\\""),
            '\\' => q.push_str("\\\\"),
            '\n' => q.push_str("\\n"),
            '\r' => q.push_str("\\r"),
            '\t' => q.push_str("\\t"),
            c => q.push(c),
        }
    }
    q.push('"');
    q
}

/// Format an `f64` so it always round-trips and is parsed as a float literal by
/// Rhai. Non-finite values (`NaN`, `±inf`) are clamped to `0.0` — they cannot
/// appear in valid node positions or sizes and Rhai has no literal for them.
fn rnum(v: f64) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    if v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        // {:?} on f64 emits a round-trippable repr (e.g. "1.5", "3.14").
        format!("{v:?}")
    }
}

// ---- load (from_rhai) --------------------------------------------------------

/// Run `src` on an engine whose registered functions build a [`Graph`], and
/// return the constructed graph.
pub fn from_rhai(src: &str) -> Result<Graph, ScriptError> {
    let graph = Rc::new(RefCell::new(Graph::new()));
    let mut engine = Engine::new();
    register_builder(&mut engine, &graph);

    let result = engine
        .run(src)
        .map_err(|e| ScriptError::Eval(e.to_string()));

    // Drop the engine so the only remaining strong ref to the graph is `graph`,
    // allowing `try_unwrap` to recover it without cloning.
    drop(engine);
    result?;

    let mut g = Rc::try_unwrap(graph)
        .map(RefCell::into_inner)
        .unwrap_or_else(|rc| std::mem::take(&mut *rc.borrow_mut()));
    // Rebuild the transient index, exactly like the JSON loader does.
    g.rebuild_index();
    Ok(g)
}

/// Register the builder API on `engine`, each function mutating the shared graph.
fn register_builder(engine: &mut Engine, graph: &Rc<RefCell<Graph>>) {
    // node(id, type, name, display_name, px, py)
    {
        let g = graph.clone();
        engine.register_fn(
            "node",
            move |id: &str,
                  ty: &str,
                  name: &str,
                  display: &str,
                  px: f64,
                  py: f64|
                  -> Result<(), Box<EvalAltResult>> {
                let node_id = parse_id(id)?;
                let mut node = empty_node(node_id, ty, name, display);
                node.set_view_pos([px as f32, py as f32]);
                g.borrow_mut().nodes.insert(node_id, node);
                Ok(())
            },
        );
    }
    // view(id, w, h, mode, bypass, display, full_h)
    {
        let g = graph.clone();
        engine.register_fn(
            "view",
            move |id: &str,
                  w: f64,
                  h: f64,
                  mode: i64,
                  bypass: bool,
                  display: bool,
                  full_h: f64|
                  -> Result<(), Box<EvalAltResult>> {
                let node_id = parse_id(id)?;
                let mut gb = g.borrow_mut();
                let node = gb.nodes.get_mut(&node_id).ok_or_else(|| missing(id))?;
                node.set_view_size([w as f32, h as f32]);
                box_put(&mut node.data, "view.mode", BoxValue::I64(mode));
                node.set_bypass(bypass);
                node.set_display_flag(display);
                node.set_full_view_height(full_h as f32);
                Ok(())
            },
        );
    }
    // note(id, text) — sticky-note body text (Note annotation nodes). Optional, like view_kind.
    {
        let g = graph.clone();
        engine.register_fn(
            "note",
            move |id: &str, text: &str| -> Result<(), Box<EvalAltResult>> {
                let node_id = parse_id(id)?;
                let mut gb = g.borrow_mut();
                let node = gb.nodes.get_mut(&node_id).ok_or_else(|| missing(id))?;
                node.set_note_text(text);
                Ok(())
            },
        );
    }
    // view_kind(id, kind) — optional node-body render kind (0=Flat2D, 1=Live2D, 2=Live3D).
    {
        let g = graph.clone();
        engine.register_fn(
            "view_kind",
            move |id: &str, kind: i64| -> Result<(), Box<EvalAltResult>> {
                let node_id = parse_id(id)?;
                let mut gb = g.borrow_mut();
                let node = gb.nodes.get_mut(&node_id).ok_or_else(|| missing(id))?;
                node.set_view_kind(crate::NodeViewKind::from_i64(kind));
                Ok(())
            },
        );
    }
    // attr_in(id, name, attr_id, json_value) / attr_out(...) — separate namespaces,
    // see module doc.
    register_attr_setter(engine, graph, "attr_in", "inputs");
    register_attr_setter(engine, graph, "attr_out", "outputs");
    // input(id, name) — append to input_order.
    register_port_appender(engine, graph, "input", "input_order");
    // output(id, name) — append to output_order.
    register_port_appender(engine, graph, "output", "output_order");
    // set_source(id, name, src_node, src_attr_id)
    {
        let g = graph.clone();
        engine.register_fn(
            "set_source",
            move |id: &str,
                  name: &str,
                  src_node: &str,
                  src_attr: &str|
                  -> Result<(), Box<EvalAltResult>> {
                let node_id = parse_id(id)?;
                let src = parse_id(src_node)?;
                let mut gb = g.borrow_mut();
                let node = gb.nodes.get_mut(&node_id).ok_or_else(|| missing(id))?;
                node.set_source(name, src, src_attr);
                Ok(())
            },
        );
    }
    // parent(child_id, parent_id)
    {
        let g = graph.clone();
        engine.register_fn(
            "parent",
            move |child: &str, parent: &str| -> Result<(), Box<EvalAltResult>> {
                let child_id = parse_id(child)?;
                let parent_id = parse_id(parent)?;
                let mut gb = g.borrow_mut();
                // Set the child's parent ref and mirror it onto the parent's children
                // list (keeps both sides consistent, like Graph::set_parent).
                if let Some(node) = gb.nodes.get_mut(&child_id) {
                    node.set_parent(Some(parent_id));
                } else {
                    return Err(missing(child));
                }
                if let Some(pnode) = gb.nodes.get_mut(&parent_id) {
                    pnode.add_child(child_id);
                } else {
                    return Err(missing(parent));
                }
                Ok(())
            },
        );
    }
    // bus(name, json_value) — restore one graph-level bus slot.
    {
        let g = graph.clone();
        engine.register_fn(
            "bus",
            move |name: &str, json: &str| -> Result<(), Box<EvalAltResult>> {
                let jv: serde_json::Value = serde_json::from_str(json)
                    .map_err(|e| err(format!("bad bus json for slot {name:?}: {e}")))?;
                let value = BoxValue::from_json_value(jv);
                g.borrow_mut().bus.set(name, value);
                Ok(())
            },
        );
    }
}

/// Register an `attr_in`/`attr_out` builder that writes one attr object into
/// `data.<bucket>.<name>` (`bucket` is `"inputs"` or `"outputs"`), preserving the
/// attr's own uuid (fresh-uuid construction would break source refs / JSON
/// identity).
fn register_attr_setter(
    engine: &mut Engine,
    graph: &Rc<RefCell<Graph>>,
    fn_name: &'static str,
    bucket: &'static str,
) {
    let g = graph.clone();
    engine.register_fn(
        fn_name,
        move |id: &str, name: &str, attr_id: &str, json: &str| -> Result<(), Box<EvalAltResult>> {
            let node_id = parse_id(id)?;
            let jv: serde_json::Value = serde_json::from_str(json)
                .map_err(|e| err(format!("bad attr json for {id}.{name}: {e}")))?;
            let value = BoxValue::from_json_value(jv);
            let mut gb = g.borrow_mut();
            let node = gb.nodes.get_mut(&node_id).ok_or_else(|| missing(id))?;
            let mut obj = BoxValue::object();
            if let Some(m) = obj.as_object_mut() {
                m.insert("id".into(), BoxValue::String(attr_id.to_string()));
                m.insert("value".into(), value);
                m.insert("source".into(), BoxValue::Null);
            }
            // `name` comes from the SCRIPT, so this is one of the dynamic paths that used to lose a
            // value in silence when the name was path-hostile (`a.b`, `x[0]`) — `box_put` logs it.
            box_put(&mut node.data, &format!("{bucket}.{name}"), obj);
            Ok(())
        },
    );
}

/// Register an `input`/`output` builder that appends `name` to the named ordered
/// list on the node's `data`.
fn register_port_appender(
    engine: &mut Engine,
    graph: &Rc<RefCell<Graph>>,
    fn_name: &'static str,
    list_path: &'static str,
) {
    let g = graph.clone();
    engine.register_fn(
        fn_name,
        move |id: &str, name: &str| -> Result<(), Box<EvalAltResult>> {
            let node_id = parse_id(id)?;
            let mut gb = g.borrow_mut();
            let node = gb.nodes.get_mut(&node_id).ok_or_else(|| missing(id))?;
            let mut list = node
                .data
                .get_path(list_path)
                .and_then(BoxValue::as_array)
                .cloned()
                .unwrap_or_default();
            list.push(BoxValue::String(name.to_string()));
            box_put(&mut node.data, list_path, BoxValue::array(list));
            Ok(())
        },
    );
}

/// Build a bare node skeleton (no attrs/ports/params yet — those arrive via
/// builder calls). Mirrors the canonical `data` layout so the JSON round-trip
/// matches.
fn empty_node(id: NodeId, ty: &str, name: &str, display: &str) -> Node {
    let mut data = RustBox::new();
    box_put(&mut data, "type", BoxValue::String(ty.to_string()));
    box_put(&mut data, "name", BoxValue::String(name.to_string()));
    box_put(
        &mut data,
        "display_name",
        BoxValue::String(display.to_string()),
    );
    box_put(&mut data, "inputs", BoxValue::object());
    box_put(&mut data, "outputs", BoxValue::object());
    box_put(&mut data, "input_order", BoxValue::array(Vec::new()));
    box_put(&mut data, "output_order", BoxValue::array(Vec::new()));
    box_put(&mut data, "compute", BoxValue::object());
    box_put(&mut data, "tmp", BoxValue::object());
    box_put(&mut data, "parent", BoxValue::Null);
    box_put(&mut data, "children", BoxValue::array(Vec::new()));
    // A default view; the subsequent `view(..)` call overwrites pos/size/flags.
    let mut view = BoxValue::object();
    if let Some(m) = view.as_object_mut() {
        let mut tree: BTreeMap<String, BoxValue> = BTreeMap::new();
        tree.insert(
            "pos".into(),
            BoxValue::array(vec![BoxValue::F64(0.0), BoxValue::F64(0.0)]),
        );
        tree.insert(
            "size".into(),
            BoxValue::array(vec![BoxValue::F64(320.0), BoxValue::F64(180.0)]),
        );
        tree.insert("mode".into(), BoxValue::I64(2));
        tree.insert("bypass".into(), BoxValue::Bool(false));
        tree.insert("display".into(), BoxValue::Bool(false));
        *m = tree;
    }
    box_put(&mut data, "view", view);

    Node { id, data }
}

fn parse_id(s: &str) -> Result<NodeId, Box<EvalAltResult>> {
    Uuid::parse_str(s)
        .map(NodeId)
        .map_err(|e| err(format!("bad uuid {s:?}: {e}")))
}

fn missing(id: &str) -> Box<EvalAltResult> {
    err(format!("unknown node id {id:?}"))
}

fn err(msg: String) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        msg.into(),
        rhai::Position::NONE,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Graph, NodeTypeRegistry, ValueTypes, register_builtins, subnet_to_json_pretty};

    // `register_builtins` registers the builtin VALUE types too (the `Builtins` library declares
    // them), so a separate `register_builtin_types` call would register each one twice.
    fn fixtures() -> (NodeTypeRegistry, ValueTypes) {
        let mut types = ValueTypes::new();
        let mut reg = NodeTypeRegistry::new();
        register_builtins(&mut reg, &mut types);
        (reg, types)
    }

    /// Build a representative graph covering all serialized data paths:
    /// - Float with a literal F64 value
    /// - Add node wired from Float (×2 inputs), carrying Vec3 + Color extra attrs
    /// - Convert node with its `to` attr set (exercises a string-choice attr)
    /// - BusWrite node with its `slot` input set (exercises the bus node type)
    /// - Parent/child pair (Subnet containing Float)
    /// - A seeded bus slot (exercises graph-level bus serialization)
    ///
    /// Returns `(graph, add_id, subnet_id)`.
    fn build_graph() -> (Graph, NodeId, NodeId) {
        let (reg, types) = fixtures();
        let mut g = Graph::new();

        // Float node: explicit value.
        let float = g.create_node("Float", [10.0, 20.0], &reg, &types).unwrap();
        g.nodes
            .get_mut(&float)
            .unwrap()
            .set_input_value("value", BoxValue::F64(3.5));

        // Add node: wired from Float, plus typed-value attrs to prove lossless tags.
        let add = g.create_node("Add", [200.0, 0.0], &reg, &types).unwrap();
        g.connect_idx(float, 0, add, 0, &reg, &types).unwrap();
        g.connect_idx(float, 0, add, 1, &reg, &types).unwrap();
        g.nodes
            .get_mut(&add)
            .unwrap()
            .set_input_value("dir", BoxValue::Vec3([1.0, 2.0, 3.0]));
        g.nodes
            .get_mut(&add)
            .unwrap()
            .set_input_value("tint", BoxValue::Color([0.1, 0.2, 0.3, 0.4]));

        // Convert node with its `to` attr set — exercises a string-choice attr.
        let conv = g
            .create_node("Convert", [400.0, 0.0], &reg, &types)
            .unwrap();
        g.nodes
            .get_mut(&conv)
            .unwrap()
            .set_input_value("to", BoxValue::String("Vec3".to_string()));

        // BusWrite node with `slot` input set — exercises the bus slot name.
        let bw = g
            .create_node("BusWrite", [600.0, 0.0], &reg, &types)
            .unwrap();
        g.nodes
            .get_mut(&bw)
            .unwrap()
            .set_input_value("slot", BoxValue::String("myslot".to_string()));

        // Parent/child pair.
        let subnet = g.create_node("Subnet", [0.0, 300.0], &reg, &types).unwrap();
        g.set_parent(float, Some(subnet));

        // Graph-level bus slot — exercises the top-level bus_slots field.
        g.bus.set("someslot", BoxValue::F64(42.0));

        (g, add, subnet)
    }

    /// Normalize a `SubnetFile` JSON document for comparison: remove the random
    /// top-level `id` field (which differs between `Graph::new()` calls) and keep
    /// everything else — `nodes` (with params) and `bus_slots`.
    fn strip_graph_id(doc: &serde_json::Value) -> serde_json::Value {
        let mut v = doc.clone();
        if let Some(obj) = v.as_object_mut() {
            obj.remove("id");
        }
        v
    }

    #[test]
    fn rhai_roundtrip_matches_json() {
        let (g, _add, _subnet) = build_graph();

        // The serializer needs the registry to know each node's kind (a Job's outputs are saved, a
        // Dataflow node's are blanked) — see `serialization::serialize_node_data`.
        let (reg, _types) = crate::default_registry();

        // Reference: the JSON document of the original graph.
        let json_before = subnet_to_json_pretty(&g, &reg).unwrap();

        // Rhai round-trip.
        let script = to_rhai(&g);
        let reloaded = from_rhai(&script).unwrap();

        // The reloaded graph must serialize identically through the JSON path.
        let json_after = subnet_to_json_pretty(&reloaded, &reg).unwrap();

        let before: serde_json::Value = serde_json::from_str(&json_before).unwrap();
        let after: serde_json::Value = serde_json::from_str(&json_after).unwrap();

        // Compare the full document minus the random top-level `id` — this covers
        // nodes (including params), bus_slots, and format_version.
        assert_eq!(
            strip_graph_id(&before),
            strip_graph_id(&after),
            "Rhai round-trip must reproduce the full document (nodes+params+bus_slots)"
        );
    }

    #[test]
    fn rhai_output_is_deterministic() {
        let (g, _a, _b) = build_graph();
        let a = to_rhai(&g);
        let b = to_rhai(&g);
        assert_eq!(a, b, "to_rhai must be deterministic across calls");
    }

    #[test]
    fn rhai_preserves_wire_and_parent() {
        let (g, add, subnet) = build_graph();
        let script = to_rhai(&g);
        let reloaded = from_rhai(&script).unwrap();

        // Wires resolve through the rebuilt index.
        assert!(reloaded.index.in_edge(add, "a").is_some());
        assert!(reloaded.index.in_edge(add, "b").is_some());

        // Parent/child survived.
        let child = g
            .nodes
            .iter()
            .find(|(_, n)| n.parent() == Some(subnet))
            .map(|(id, _)| *id)
            .unwrap();
        assert_eq!(reloaded.nodes.get(&child).unwrap().parent(), Some(subnet));
        assert_eq!(reloaded.children_of(subnet), vec![child]);
    }

    #[test]
    fn rhai_preserves_params_and_bus() {
        let (g, _add, _subnet) = build_graph();
        let script = to_rhai(&g);
        let reloaded = from_rhai(&script).unwrap();

        // Convert node: the `to` attr must survive.
        let conv = reloaded
            .nodes
            .values()
            .find(|n| n.type_name() == "Convert")
            .expect("Convert node must be present");
        assert_eq!(
            conv.input_value("to")
                .and_then(|v| v.as_str().map(str::to_string)),
            Some("Vec3".to_string()),
            "Convert `to` attr must round-trip"
        );

        // BusWrite node: `slot` input must survive.
        let bw = reloaded
            .nodes
            .values()
            .find(|n| n.type_name() == "BusWrite")
            .expect("BusWrite node must be present");
        assert_eq!(
            bw.input_value("slot")
                .and_then(|v| v.as_str().map(str::to_string)),
            Some("myslot".to_string()),
            "BusWrite `slot` input must round-trip"
        );

        // Graph-level bus slot must survive.
        assert_eq!(
            reloaded.bus.get("someslot"),
            Some(&BoxValue::F64(42.0)),
            "bus slot 'someslot' must round-trip"
        );
    }

    #[test]
    fn rhai_preserves_view_kind() {
        // A non-default NodeViewKind must survive the rhai round-trip via the optional
        // `view_kind(id, kind)` builder (JSON is generic; rhai needs the matching emit — this
        // guards that pairing, per the crate CLAUDE note that rhai is NOT generic).
        let (mut g, add, _subnet) = build_graph();
        g.nodes
            .get_mut(&add)
            .unwrap()
            .set_view_kind(crate::NodeViewKind::Live2D);

        let script = to_rhai(&g);
        let reloaded = from_rhai(&script).unwrap();
        assert_eq!(
            reloaded.nodes.get(&add).map(|n| n.view_kind()),
            Some(crate::NodeViewKind::Live2D),
            "non-default view_kind must round-trip through rhai"
        );
        // Untouched nodes stay Flat2D (no spurious emit for the default).
        if let Some(other) = g.nodes.keys().find(|id| **id != add).copied() {
            assert_eq!(
                reloaded.nodes.get(&other).map(|n| n.view_kind()),
                Some(crate::NodeViewKind::Flat2D),
                "default view_kind stays Flat2D across the round-trip"
            );
        }
    }

    #[test]
    fn rhai_preserves_note_text() {
        // A sticky-note's multi-line body text must survive the rhai round-trip via the optional
        // `note(id, text)` builder (JSON is generic; rhai needs the matching emit). Same guard as
        // `rhai_preserves_view_kind` — the crate CLAUDE note warns rhai is NOT generic.
        let (mut g, add, _subnet) = build_graph();
        let body = "line one\nline two\twith tab";
        g.nodes.get_mut(&add).unwrap().set_note_text(body);

        let script = to_rhai(&g);
        let reloaded = from_rhai(&script).unwrap();
        assert_eq!(
            reloaded.nodes.get(&add).map(|n| n.note_text()),
            Some(body),
            "multi-line note_text must round-trip through rhai verbatim"
        );
        // Untouched nodes carry no note text (no spurious emit for the empty default).
        if let Some(other) = g.nodes.keys().find(|id| **id != add).copied() {
            assert_eq!(
                reloaded.nodes.get(&other).map(|n| n.note_text()),
                Some(""),
                "empty note_text stays empty across the round-trip"
            );
        }
    }

    #[test]
    fn rnum_clamps_nonfinite() {
        // Non-finite values must produce valid Rhai float literals (parseable as 0.0).
        let nan = rnum(f64::NAN);
        let inf = rnum(f64::INFINITY);
        let ninf = rnum(f64::NEG_INFINITY);
        assert_eq!(nan, "0.0");
        assert_eq!(inf, "0.0");
        assert_eq!(ninf, "0.0");
        // Finite values must not be clamped.
        assert_eq!(rnum(1.0), "1.0");
        assert_eq!(rnum(1.5), "1.5");
    }
}
