//! Serializable **node-type schema** — the DISPLAY subset of [`crate::node::NodeDef`].
//!
//! ## Why this exists
//!
//! A read-only node-editor host (e.g. the colmap-ui "Pipeline" DAG widget) renders node
//! TITLES and PORT NAMES by resolving each node's `type_name` against a
//! [`crate::registry::NodeTypeRegistry`] (see `nodes-view` `text_labels`). Such a host does
//! NOT own the compute crate that defines those types, so it cannot build the real registry —
//! it would pass an EMPTY one, and port labels degrade to generic `in`/`out`.
//!
//! The fix is to decouple the DISPLAY model from the compute crate: the crate that DOES own
//! the types serializes their display subset to JSON ([`NodeTypeRegistry::schemas_json`]); the
//! display-only host parses it back into a passthrough registry
//! ([`NodeTypeRegistry::from_schemas_json`]) that answers `get(type_name)` with the real
//! `display_name` / `category` / port names — enough to render, nothing to cook.
//!
//! The schema is DISPLAY-ONLY on purpose: no `CookFn`, no per-port default `BoxValue`, no
//! `AttrDef` inspector metadata — only what a diagram needs to label boxes and pins. A
//! dedicated [`PortSchema`] (name + type) is used rather than serializing [`crate::port::PortDef`]
//! directly, so the compute-only `default: BoxValue` field never leaks into the wire format.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::cook::{CookCtx, CookError, CookResult};
use crate::error::{CoreError, CoreResult};
use crate::node::NodeDef;
use crate::port::PortDef;
use crate::registry::NodeTypeRegistry;

/// The cook of every type in a [`NodeTypeRegistry::from_schemas_json`] registry: always an error.
///
/// A JSON schema describes a type's SHAPE, never its behaviour, so a stub has no result to produce.
/// Erroring here is what keeps a display-only registry from silently fabricating plausible-looking
/// output if anything ever drives a cook over it (see [`NodeTypeRegistry::from_schemas_json`]).
fn stub_cook(type_name: &str) -> CookResult<()> {
    Err(CookError::Custom(format!(
        "'{type_name}' is a display-only schema stub: this registry was built by \
         `from_schemas_json` and cannot cook"
    )))
}

/// Display subset of a [`PortDef`]: the pin's `name` (its label) and `port_type` (its pin
/// colour / connection type). The compute-only `default` value is intentionally omitted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortSchema {
    pub name: String,
    pub port_type: String,
}

impl PortSchema {
    fn from_port(p: &PortDef) -> Self {
        Self {
            name: p.name.clone(),
            port_type: p.port_type.clone(),
        }
    }
}

/// Display subset of a [`NodeDef`] — everything a read-only diagram needs to label a node box
/// and its pins, and nothing it does not (no cook fn, no port defaults, no inspector schema).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeSchema {
    pub type_name: String,
    pub display_name: String,
    pub category: String,
    pub inputs: Vec<PortSchema>,
    pub outputs: Vec<PortSchema>,
}

impl NodeSchema {
    fn from_def(def: &NodeDef) -> Self {
        Self {
            type_name: def.type_name.to_string(),
            display_name: def.display_name.to_string(),
            category: def.category.to_string(),
            inputs: def.inputs.iter().map(PortSchema::from_port).collect(),
            outputs: def.outputs.iter().map(PortSchema::from_port).collect(),
        }
    }
}

impl NodeTypeRegistry {
    /// Serialize every registered type's DISPLAY subset (see [`NodeSchema`]) to a JSON array.
    ///
    /// Output is sorted by `type_name` so the file is deterministic (diff-friendly, and stable
    /// across the non-deterministic internal `HashMap` iteration order). Serialization of these
    /// all-`String` structs cannot fail in practice; on the impossible error path it logs and
    /// returns an empty array so a best-effort writer never aborts its caller.
    pub fn schemas_json(&self) -> String {
        let mut schemas: Vec<NodeSchema> = self
            .type_names()
            .filter_map(|t| self.get(t))
            .map(NodeSchema::from_def)
            .collect();
        schemas.sort_by(|a, b| a.type_name.cmp(&b.type_name));
        serde_json::to_string_pretty(&schemas).unwrap_or_else(|e| {
            log::warn!("schemas_json: serialize failed: {e}");
            "[]".to_string()
        })
    }

    /// Parse a [`schemas_json`](Self::schemas_json) array into a DISPLAY-ONLY registry.
    ///
    /// Each schema becomes a [`NodeDef`] whose ports carry the real names/types, wrapped in an
    /// inert [`crate::fn_node`] type. `get(type_name)` then returns full display info
    /// (`display_name`, `category`, ordered input/output port names), which is all a read-only
    /// diagram consumes.
    ///
    /// **A stub CANNOT cook**: its cook always returns [`CookError::Custom`] naming the type (see
    /// [`stub_cook`]). A schema carries no behaviour, so any cook of this registry would have to
    /// FABRICATE a result — a passthrough cook (what this used to install) reported
    /// `NodeStatus::Done`, stamped a fresh `cooked_gen` and published input[0] as output[0], so an
    /// `Add` stub would emit `result = a` and every downstream consumer would silently read wrong
    /// data. Failing loudly costs nothing (a display host never cooks) and turns a silent-wrong-data
    /// path into a visible error.
    ///
    /// Port `default` values are [`crate::box_model::BoxValue::Null`]: a display registry never
    /// instantiates nodes (the host loads pre-built nodes from a graph file), so defaults are
    /// irrelevant to it — which is also why these types are inserted WITHOUT a `ValueTypes` (they
    /// declare none; the names came from JSON, not from any type we own).
    pub fn from_schemas_json(json: &str) -> CoreResult<NodeTypeRegistry> {
        let schemas: Vec<NodeSchema> = serde_json::from_str(json)
            .map_err(|e| CoreError::Other(format!("schemas_json parse failed: {e}")))?;
        let mut reg = NodeTypeRegistry::new();
        for s in schemas {
            let to_ports = |ps: Vec<PortSchema>| -> Vec<PortDef> {
                ps.into_iter()
                    .map(|p| PortDef::new(&p.name, &p.port_type, crate::box_model::BoxValue::Null))
                    .collect()
            };
            // Captured by the cook closure so the error names the type the host tried to cook.
            let ty: Arc<str> = Arc::from(s.type_name.as_str());
            let def = NodeDef::new(s.type_name, s.category)
                .with_display(s.display_name)
                .with_inputs(to_ports(s.inputs))
                .with_outputs(to_ports(s.outputs));
            reg.insert(crate::node_type::fn_node(
                def,
                move |_ctx: &mut CookCtx<'_>| stub_cook(&ty),
            ));
        }
        Ok(reg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::register_builtins;

    /// Round-trip: a fully-populated registry → JSON → display-only registry must preserve
    /// each type's `display_name` and its input/output port NAMES (the labels the diagram draws).
    #[test]
    fn schema_json_round_trip_preserves_display_and_ports() {
        let mut src = NodeTypeRegistry::new();
        let mut types = crate::ValueTypes::new();
        register_builtins(&mut src, &mut types);

        let json = src.schemas_json();
        let round = NodeTypeRegistry::from_schemas_json(&json).expect("parse schemas_json");

        // Every source type survives with identical display name + port names.
        for t in src.type_names() {
            let orig = src.get(t).expect("orig def");
            let back = round
                .get(t)
                .unwrap_or_else(|| panic!("type '{t}' missing after round-trip"));
            assert_eq!(
                orig.display_name, back.display_name,
                "display_name for '{t}'"
            );
            assert_eq!(orig.category, back.category, "category for '{t}'");
            let names = |ports: &[PortDef]| -> Vec<String> {
                ports.iter().map(|p| p.name.clone()).collect()
            };
            assert_eq!(
                names(&orig.inputs),
                names(&back.inputs),
                "input names for '{t}'"
            );
            assert_eq!(
                names(&orig.outputs),
                names(&back.outputs),
                "output names for '{t}'"
            );
            let types = |ports: &[PortDef]| -> Vec<String> {
                ports.iter().map(|p| p.port_type.clone()).collect()
            };
            assert_eq!(
                types(&orig.inputs),
                types(&back.inputs),
                "input types for '{t}'"
            );
            assert_eq!(
                types(&orig.outputs),
                types(&back.outputs),
                "output types for '{t}'"
            );
        }
    }

    /// A display-only stub must FAIL to cook, not fabricate a result.
    ///
    /// The stub cook used to be `passthrough_cook`, which copied input[0] → output[0]: cooking an
    /// `Add` stub reported `Done` and published `result = a` — wrong data, silently, with a fresh
    /// `cooked_gen` telling every consumer it was current. The type must end in `NodeStatus::Error`
    /// and write NOTHING.
    #[test]
    fn schema_stub_errors_on_cook_and_writes_no_output() {
        let mut src = NodeTypeRegistry::new();
        let mut types = crate::ValueTypes::new();
        register_builtins(&mut src, &mut types);

        let reg = NodeTypeRegistry::from_schemas_json(&src.schemas_json()).expect("parse");
        let mut graph = crate::Graph::new();
        let id = graph
            .create_node("Add", [0.0, 0.0], &reg, &types)
            .expect("create Add stub");
        // A value the passthrough cook would happily have republished as `result`.
        graph
            .nodes
            .get_mut(&id)
            .expect("node")
            .set_input_value("a", crate::BoxValue::F64(7.0));

        crate::cook::cook(&mut graph, &reg, &types);

        let node = &graph.nodes[&id];
        match node.status() {
            crate::NodeStatus::Error(msg) => {
                assert!(
                    msg.contains("Add"),
                    "the error must name the stub type, got: {msg}"
                );
                assert!(
                    msg.contains("display-only"),
                    "error must explain why, got: {msg}"
                );
            }
            other => panic!("a stub cook must error, got {other:?}"),
        }
        assert_ne!(
            node.output_value("result"),
            Some(crate::BoxValue::F64(7.0)),
            "a stub must never fabricate an output value"
        );
        assert_eq!(
            node.cooked_gen(),
            0,
            "a failed stub must not advertise fresh output"
        );
    }
}
