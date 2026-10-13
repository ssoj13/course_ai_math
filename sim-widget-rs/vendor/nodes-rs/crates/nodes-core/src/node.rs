//! Node type definitions ([`NodeDef`]) and runtime instances ([`Node`]).
//!
//! [`NodeDef`] is registered once per type in [`crate::registry::NodeTypeRegistry`].
//! A [`Node`] stores **all** data — persistent and transient — in a single [`RustBox`]
//! (`data`), organized into four top-level sections.
//!
//! ## `data` layout (Maya-attribute-style, corrected 2026-07-05)
//!
//! ```text
//! type              : "Float"
//! name              : "float1"
//! display_name      : "Float"
//! inputs.<name>     : { id: <attr_uuid>, value: <BoxValue>,
//!                       source: null | { node: <node_uuid>, attr: <attr_uuid> } }
//! outputs.<name>    : { id: <attr_uuid>, value: <BoxValue> }
//! input_order       : [ <name>, ... ]   // declared/pin order (a BTreeMap can't
//! output_order      : [ <name>, ... ]   // preserve insertion order, so these are separate)
//! compute.<name>    : <BoxValue> — NEVER serialized. Resolved value for a connected
//!                     input during ONE cook pass (replaces an out-of-band Rust field).
//! tmp.<anything>    : NEVER serialized. General scratch space for transient-but-
//!                     inspectable state (future: cook timing, mesh/preview buffers) —
//!                     anything that doesn't need `compute`'s specific cook-override
//!                     semantics but still belongs in `data`, not a bespoke Rust field.
//! parent            : <node_uuid> | null
//! children          : [ <node_uuid>, ... ]
//! view              : { pos:[x,y], size:[w,h], mode, bypass, display, full_h,
//!                       orientation }
//! ```
//!
//! `view.orientation` (added 2026-07-05, see [`Self::orientation_pref`]) is an
//! explicit horizontal/vertical override; absent means "auto" (aspect-ratio
//! based, [`crate::chrome::pin_edge_layout`]) — the default for every node.
//!
//! **Everything that affects the node's computation is an `inputs.<name>` entry —
//! there is no separate "non-connectable config" concept** (the old `params.*` bare
//! fields were folded in here; see `builtins/bus.rs`/`builtins/map.rs`). Every input
//! can be driven by a connection (`source`) exactly like every other, Maya-attribute-
//! style — whether or not the current UI happens to draw a pin for it. `inputs` and
//! `outputs` are SEPARATE maps (not one shared bucket, which is what this comment
//! described before 2026-07-05): a name can be reused on both sides (e.g. a future
//! `mesh` -> `mesh` deform node) without collision, since each side has its own
//! namespace. `source` only ever appears on the input side — outputs are never
//! "connected from" anything, they're computed.
//!
//! ## Cook-time input resolution
//!
//! Before a cook closure runs, the cook driver populates `data.compute.<name>` with
//! the resolved values for connected inputs (pulled from the source node's output,
//! auto-converted). The `in_*` helpers consult `compute` first; the stored
//! `inputs.<name>.value` is NEVER mutated by cook (it holds the author's literal).
//! The driver clears `compute` after each node's cook completes.

use std::sync::{Arc, LazyLock};

use uuid::Uuid;

use crate::box_model::{BoxValue, PathKey, RustBox};
use crate::chrome::{PinEdgeLayout, resolve_orientation};
use crate::cook::NodeStatus;
use crate::graph::NodeId;
use crate::layout::PinSide;
use crate::value_types::ValueTypes;
use crate::view_mode::{NodeViewKind, NodeViewMode};

/// Optional explicit control override for the inspector. When absent, the
/// inspector infers the control from the attr's value type plus its
/// [`AttrDef::range`] / [`AttrDef::choices`] (see `nodes-editor` inspector). This
/// lets a type author force a specific widget when inference would pick wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiHint {
    /// Force a slider (numeric attrs with a [`AttrDef::range`]).
    Slider,
    /// Force a drop-down list (attrs with [`AttrDef::choices`]).
    ComboBox,
    /// Force a radio-button group (attrs with [`AttrDef::choices`]).
    Radio,
    /// Force a file-path field with a browse button (string attrs holding a filesystem path).
    FilePath,
}

/// Per-attribute UI/metadata declaration (design spec §3.4).
///
/// Schema is **UI metadata only**: it drives the type-driven inspector
/// (control choice, range, choices, tooltip). It does **not** seed attr values —
/// defaults still come from the node's ports (whose defaults come from the registered value type —
/// [`crate::ValueTypes::make_default`]) at instantiation. A type declares its schema in its own
/// [`NodeDef::with_schema`]; a host can re-skin someone else's via
/// [`crate::registry::NodeTypeRegistry::set_schema`].
#[derive(Debug, Clone)]
pub struct AttrDef {
    /// Attribute name this metadata applies to (matches an `attrs.<key>` entry).
    pub key: String,
    /// Declared value type (`"Float"`, `"String"`, …). Informational; the live
    /// value kind in `data` is authoritative for control selection.
    pub ty: String,
    /// Suggested default (informational; instantiation uses port defaults).
    pub default: BoxValue,
    /// Numeric soft range `(min, max)` → renders a slider.
    pub range: Option<(f64, f64)>,
    /// Closed set of string choices → renders a combo box / radio group.
    pub choices: Option<Vec<String>>,
    /// Human-readable description → renders a hover tooltip.
    pub description: Option<String>,
    /// Optional explicit control override.
    pub ui_hint: Option<UiHint>,
}

impl AttrDef {
    /// Minimal metadata for `key` of declared type `ty` (all hints empty).
    pub fn new(key: impl Into<String>, ty: impl Into<String>, default: BoxValue) -> Self {
        Self {
            key: key.into(),
            ty: ty.into(),
            default,
            range: None,
            choices: None,
            description: None,
            ui_hint: None,
        }
    }

    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.range = Some((min, max));
        self
    }

    pub fn choices<S: Into<String>>(mut self, choices: impl IntoIterator<Item = S>) -> Self {
        self.choices = Some(choices.into_iter().map(Into::into).collect());
        self
    }

    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub fn ui_hint(mut self, hint: UiHint) -> Self {
        self.ui_hint = Some(hint);
        self
    }
}

/// Immutable **shape** of a node type: identity, ports and inspector schema.
///
/// This is DATA ONLY — it carries no behaviour. The cook closure and the cook stage used to live
/// here too, which meant a type had two half-authorities (the def AND the registry entry holding
/// it). Behaviour now lives on [`crate::NodeType`], which produces this def once at registration;
/// the registry caches it and every data-driven consumer (node menu, inspector, `schema.rs`,
/// renderers) reads it back through `registry.get(type_name)`.
///
/// Names are `Arc<str>`, not `&'static str`: a type built at RUNTIME (from a plugin manifest, a
/// schema file, a generated pipeline) cannot produce a `&'static str` without leaking, and defs are
/// cloned into menus, so cheap sharing matters.
#[derive(Clone)]
pub struct NodeDef {
    pub type_name: Arc<str>,
    pub display_name: Arc<str>,
    pub category: Arc<str>,
    pub inputs: Vec<crate::port::PortDef>,
    pub outputs: Vec<crate::port::PortDef>,
    /// Per-attribute UI metadata (see [`AttrDef`]). Empty by default; set with
    /// [`NodeDef::with_schema`] when building the def, or attached post-registration via
    /// [`crate::registry::NodeTypeRegistry::set_schema`].
    pub schema: Vec<AttrDef>,
    /// Dataflow or Job (see [`crate::NodeKind`]) — **not set by the def's author**.
    ///
    /// The authority is [`crate::NodeType::kind`]; the registry stamps this copy onto the cached def
    /// when the type is registered, at the ONE place a type enters the map, so the two can never
    /// disagree. It lives here only because [`Node::from_def`] has a def and no registry, and it must
    /// know: a Job's outputs are seeded ABSENT (`Null`), never from a declared default.
    pub kind: crate::node_type::NodeKind,
}

impl NodeDef {
    /// A def with no ports and no schema: `type_name` doubles as the display name until
    /// [`Self::with_display`] overrides it. Chain [`Self::with_inputs`] / [`Self::with_outputs`] /
    /// [`Self::with_schema`] to fill it in.
    pub fn new(type_name: impl Into<Arc<str>>, category: impl Into<Arc<str>>) -> Self {
        let type_name = type_name.into();
        Self {
            display_name: type_name.clone(),
            type_name,
            category: category.into(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            schema: Vec::new(),
            // Overwritten from the TYPE at registration (see the field docs).
            kind: crate::node_type::NodeKind::Dataflow,
        }
    }

    /// Human-facing label (node title / menu entry) when it differs from the type name.
    pub fn with_display(mut self, display_name: impl Into<Arc<str>>) -> Self {
        self.display_name = display_name.into();
        self
    }

    /// Declared input ports, in pin order.
    pub fn with_inputs(mut self, inputs: Vec<crate::port::PortDef>) -> Self {
        self.inputs = inputs;
        self
    }

    /// Declared output ports, in pin order.
    pub fn with_outputs(mut self, outputs: Vec<crate::port::PortDef>) -> Self {
        self.outputs = outputs;
        self
    }

    /// Per-attribute inspector metadata (ranges, choices, tooltips, control overrides).
    pub fn with_schema(mut self, schema: Vec<AttrDef>) -> Self {
        self.schema = schema;
        self
    }

    /// UI metadata for attribute `key`, if the type declares schema for it.
    pub fn attr_meta(&self, key: &str) -> Option<&AttrDef> {
        self.schema.iter().find(|a| a.key == key)
    }

    /// Declared type of an input port by name (for connection compatibility check).
    pub fn input_type(&self, name: &str) -> Option<&str> {
        self.inputs
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.port_type.as_str())
    }

    /// Declared type of an output port by name.
    pub fn output_type(&self, name: &str) -> Option<&str> {
        self.outputs
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.port_type.as_str())
    }
}

/// Paths the cook driver writes on **every node on every tick**, built once.
///
/// A node receives its inputs once and cooks once per tick, so everything here
/// is written a fixed small number of times per node per tick — and parsing
/// `"tmp.status"` back into segments each time (a `String` per segment, plus
/// the segment array) cost more than the write it preceded.
///
/// Built from segments rather than parsed, so there is no failure case to
/// handle and no `unwrap` on a constant.
static P_STATUS: LazyLock<PathKey> = LazyLock::new(|| PathKey::from_keys(&["tmp", "status"]));
static P_COOKED_GEN: LazyLock<PathKey> =
    LazyLock::new(|| PathKey::from_keys(&["tmp", "cooked_gen"]));
static P_VIEW_DIRTY: LazyLock<PathKey> = LazyLock::new(|| PathKey::from_keys(&["view", "dirty"]));
static P_COMPUTE: LazyLock<PathKey> = LazyLock::new(|| PathKey::from_keys(&["compute"]));

/// A port path such as `inputs.<name>.value`, built without formatting a
/// string and parsing it back.
///
/// Deliberately still a `PathKey` handed to the box rather than a direct map
/// lookup: the box config rewrites a path-hostile key on write (`0` becomes
/// `x0`) and falls back to that form on read, so indexing the section map by
/// the raw port name would find nothing for exactly the port names that need
/// the transform most.
fn port_path(section: &str, name: &str, field: Option<&str>) -> PathKey {
    match field {
        Some(f) => PathKey::from_keys(&[section, name, f]),
        None => PathKey::from_keys(&[section, name]),
    }
}

/// A node in a [`crate::graph_container::Graph`]: identity plus a `data` box holding
/// everything, persistent and transient (see the module doc's four-section layout).
#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    /// All node data — `inputs`/`outputs` (persistent) and `compute`/`tmp` (transient,
    /// stripped on serialization).
    pub data: RustBox,
}

impl Node {
    /// Build a node from its type definition: writes `type`/`name`/`display_name`,
    /// one `inputs.<name>` entry per input port and one `outputs.<name>` entry per
    /// output port (fresh attr uuid + declared default value; inputs also get a null
    /// `source`), the `input_order`/`output_order` name lists, empty `children`, null
    /// `parent`, and a default `view`. Input and output port names are independent
    /// namespaces, so the same name (e.g. `mesh` -> `mesh`) is fine on both sides.
    pub fn from_def(id: NodeId, def: &NodeDef, name: &str, _value_types: &ValueTypes) -> Self {
        let mut data = RustBox::new();
        box_put(
            &mut data,
            "type",
            BoxValue::String(def.type_name.to_string()),
        );
        box_put(&mut data, "name", BoxValue::String(name.to_string()));
        box_put(
            &mut data,
            "display_name",
            BoxValue::String(def.display_name.to_string()),
        );

        let mut inputs = BoxValue::object();
        let mut input_order = Vec::with_capacity(def.inputs.len());
        if let Some(map) = inputs.as_object_mut() {
            for p in &def.inputs {
                map.insert(p.name.clone(), make_attr(p.default.clone()));
                input_order.push(BoxValue::String(p.name.clone()));
            }
        }
        // A JOB's outputs are seeded ABSENT (`Null`), never from the port's declared default.
        //
        // This is load-bearing, and subtle. A job's output is not a value anyone can author — it is the
        // RECORD of an effect (a path to 40 minutes of GPU work). If a job's output port were seeded
        // with its declared default (`PortDef::string("sparse")` → `""`), a job that had NEVER RUN would
        // look like it had a value, and then: it would round-trip as "complete" and load CLEAN (never
        // running), and its consumers would happily compute on an empty string instead of failing. The
        // absence of a value IS the signal that a job has not run, so it must be a true absence.
        //
        // Dataflow outputs keep their declared defaults — they are recomputed on the first cook anyway,
        // and a fresh node is dirty for the independent reason that it has no `view.dirty` yet.
        let job = def.kind == crate::node_type::NodeKind::Job;
        let mut outputs = BoxValue::object();
        let mut output_order = Vec::with_capacity(def.outputs.len());
        if let Some(map) = outputs.as_object_mut() {
            for p in &def.outputs {
                let seed = if job {
                    BoxValue::Null
                } else {
                    p.default.clone()
                };
                map.insert(p.name.clone(), make_attr(seed));
                output_order.push(BoxValue::String(p.name.clone()));
            }
        }
        box_put(&mut data, "inputs", inputs);
        box_put(&mut data, "outputs", outputs);
        box_put(&mut data, "input_order", BoxValue::array(input_order));
        box_put(&mut data, "output_order", BoxValue::array(output_order));
        box_put(&mut data, "parent", BoxValue::Null);
        box_put(&mut data, "children", BoxValue::array(Vec::new()));
        box_put(&mut data, "view", default_view());
        // Pre-initialize both transient sections as empty objects so nested
        // `compute.<name>` / `tmp.<name>` path-sets never need to auto-vivify a
        // missing parent object.
        box_put(&mut data, "compute", BoxValue::object());
        box_put(&mut data, "tmp", BoxValue::object());

        Self { id, data }
    }

    /// Write `value` at `path` in this node's `data` box — the ONE write path for every setter
    /// below (see [`box_put`] for why a failed write is logged rather than silently dropped).
    fn put(&mut self, path: &str, value: BoxValue) {
        box_put(&mut self.data, path, value);
    }

    /// [`Self::put`] at an already-parsed path, for the handful the driver
    /// writes on every node on every tick.
    fn put_key(&mut self, key: &PathKey, value: BoxValue) {
        if let Err(e) = self.data.set_path_key(key, value) {
            log::error!(
                "node data: write to '{}' failed ({e}); the value was DROPPED",
                key.format_dot()
            );
        }
    }

    // ---- identity / labels ---------------------------------------------------

    /// Type name (`data["type"]`). Empty string if missing.
    pub fn type_name(&self) -> &str {
        self.data.get_str("type").unwrap_or("")
    }

    /// Unique sibling name used in op paths (`data["name"]`).
    pub fn name(&self) -> &str {
        self.data.get_str("name").unwrap_or("")
    }

    pub fn set_name(&mut self, name: &str) {
        self.put("name", BoxValue::String(name.to_string()));
    }

    pub fn display_name(&self) -> &str {
        self.data.get_str("display_name").unwrap_or("")
    }

    pub fn set_display_name(&mut self, name: &str) {
        self.put("display_name", BoxValue::String(name.to_string()));
    }

    /// Free-text body of a sticky-note (`Note`) annotation node; empty for every other node type.
    /// Stored as a plain top-level string (`data.note_text`) so it round-trips through the generic
    /// JSON serializer for free (rhai needs the matching `note(..)` builder — see `script.rs`).
    /// Multi-line: newlines are preserved verbatim.
    pub fn note_text(&self) -> &str {
        self.data.get_str("note_text").unwrap_or("")
    }

    /// Set the sticky-note body text (see [`Node::note_text`]).
    pub fn set_note_text(&mut self, text: &str) {
        self.put("note_text", BoxValue::String(text.to_string()));
    }

    // ---- inputs / outputs (two independent value stores) ---------------------

    /// Ordered input names (declared/pin order).
    pub fn input_names(&self) -> Vec<String> {
        name_list(&self.data, "input_order")
    }

    /// Ordered output names (declared/pin order).
    pub fn output_names(&self) -> Vec<String> {
        name_list(&self.data, "output_order")
    }

    /// Add an input port to THIS INSTANCE, after the ones it was built with.
    ///
    /// A [`NodeDef`]'s ports are a property of the TYPE, so every instance of a type has the same
    /// set. That is right for most nodes and wrong for one shape: a node whose fan-in is decided
    /// per instance — a compositor's layer stack, a merge with N branches — where the port set IS
    /// the document. Without this such a host has to either declare a fixed maximum (and draw
    /// dead pins forever) or address inputs BY POSITION, which is exactly the fragility a named
    /// port set exists to remove.
    ///
    /// Returns `false` and changes nothing if `name` is already an input: ports are a SET, and a
    /// silent overwrite would drop the existing wire's `source` while leaving the wire drawn.
    ///
    /// The new port lands at the END of [`Self::input_names`], seeded exactly as `from_def` seeds
    /// one: a fresh attr uuid, the given default, and a null `source`. Removing ports is
    /// deliberately NOT offered here — a removal has to reckon with the wires that end on it, and
    /// that is the graph's business, not a single node's.
    pub fn add_input(&mut self, name: &str, default: BoxValue) -> bool {
        if self.input_names().iter().any(|n| n == name) {
            return false;
        }
        let attr = make_attr(default);
        // A failed write means the port does NOT exist, so `input_order` must not gain a name
        // pointing at nothing - that would draw a pin with no attr behind it.
        if self
            .data
            .set_path_key(&port_path("inputs", name, None), attr)
            .is_err()
        {
            return false;
        }
        let mut order: Vec<BoxValue> = self
            .input_names()
            .into_iter()
            .map(BoxValue::String)
            .collect();
        order.push(BoxValue::String(name.to_string()));
        self.put("input_order", BoxValue::Array(std::sync::Arc::new(order)));
        true
    }

    /// Stable attr uuid string for input `name`, if present.
    pub fn input_attr_id(&self, name: &str) -> Option<String> {
        self.data
            .get_path_key(&port_path("inputs", name, Some("id")))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }

    /// Stable attr uuid string for output `name`, if present.
    pub fn output_attr_id(&self, name: &str) -> Option<String> {
        self.data
            .get_path_key(&port_path("outputs", name, Some("id")))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }

    /// Output name whose stable id matches `attr_uuid` (used when resolving wire
    /// sources — wires always source FROM an output).
    pub fn attr_name_by_id(&self, attr_uuid: &str) -> Option<String> {
        let outputs = self.data.get_path("outputs")?.as_object()?;
        for (name, obj) in outputs {
            if obj
                .as_object()
                .and_then(|m| m.get("id"))
                .and_then(|v| v.as_str())
                == Some(attr_uuid)
            {
                return Some(name.clone());
            }
        }
        None
    }

    /// Stored (authored) value of input `name`. Never includes cook-time overrides.
    /// Returns `None` if the attr is absent.
    pub fn input_value(&self, name: &str) -> Option<BoxValue> {
        self.data
            .get_path_key(&port_path("inputs", name, Some("value")))
            .cloned()
    }

    /// Current (last-cooked) value of output `name`. Returns `None` if absent.
    pub fn output_value(&self, name: &str) -> Option<BoxValue> {
        self.data
            .get_path_key(&port_path("outputs", name, Some("value")))
            .cloned()
    }

    /// **Does this node's data show a finished cook?** — i.e. does every declared output carry a value?
    ///
    /// This is the FACT the loader derives dirtiness from (see `nodes_core::serialization`): a node
    /// restored with all of its outputs present has nothing to recompute, while one whose outputs were
    /// blanked on save (what happens to a [`crate::NodeKind::Dataflow`] node) must cook to get them
    /// back.
    ///
    /// A node with **no outputs at all** reports `false`: a sink (`BusWrite`, `SaveImage`) exists purely
    /// for its side effect and carries no evidence that it ever ran, so it can never be assumed done.
    /// (Reading it the other way — "no outputs are missing, so it is complete" — is vacuously true and
    /// would silently retire every sink in a loaded graph.)
    ///
    /// `Null` counts as no value: that is exactly what the serializer writes for a non-persisted output.
    pub fn outputs_complete(&self) -> bool {
        let names = self.output_names();
        if names.is_empty() {
            return false;
        }
        names
            .iter()
            .all(|name| !matches!(self.output_value(name), None | Some(BoxValue::Null)))
    }

    /// Effective value for use inside a cook closure: returns the transient `compute`
    /// override if present (connected input pulled from source), otherwise the stored
    /// authored value.
    pub fn cook_value(&self, name: &str) -> Option<BoxValue> {
        if let Some(v) = self.data.get_path_key(&port_path("compute", name, None)) {
            return Some(v.clone());
        }
        self.input_value(name)
    }

    /// Replace the transient `compute` overrides (resolved connected-input values
    /// for the upcoming cook pass). Never touches `inputs.*` (the stored literals).
    /// Replace the transient `compute` overrides (resolved connected-input values
    /// for the upcoming cook pass). Never touches `inputs.*` (the stored literals).
    ///
    /// Takes the map the box will store, so the driver's resolved inputs go in
    /// as they are rather than being re-collected into a different map type on
    /// every node on every tick.
    pub fn set_compute(&mut self, overrides: crate::box_model::BoxMap) {
        self.put_key(&P_COMPUTE, BoxValue::obj(overrides));
    }

    /// Clear the transient `compute` overrides after a cook pass completes.
    pub fn clear_compute(&mut self) {
        self.put_key(&P_COMPUTE, BoxValue::object());
    }

    /// Set a single `compute` override without disturbing any others already present
    /// (unlike [`Self::set_compute`], which replaces the whole map).
    pub fn insert_compute(&mut self, name: &str, value: BoxValue) {
        self.put_key(&port_path("compute", name, None), value);
    }

    /// Set input `name`'s **stored** (authored) value.
    ///
    /// If the attr does not yet exist in `inputs`, a complete attr object is created
    /// with a fresh id and `source: null`, so the new attr is connectable.
    pub fn set_input_value(&mut self, name: &str, value: BoxValue) {
        // Only write to the stored path; never touch `compute` here.
        if self.input_attr_id(name).is_some() {
            // Attr exists — update just the value sub-field.
            self.put_key(&port_path("inputs", name, Some("value")), value);
        } else {
            // Attr absent — insert a full attr object (with id + source: null)
            // so the attr is connectable.
            self.put_key(&port_path("inputs", name, None), make_attr(value));
        }
    }

    // ---- typed input reads (use cook_value so closures see resolved inputs) --

    pub fn in_f64(&self, name: &str) -> f64 {
        self.cook_value(name)
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    }

    pub fn in_i64(&self, name: &str) -> i64 {
        self.cook_value(name).and_then(|v| v.as_i64()).unwrap_or(0)
    }

    pub fn in_bool(&self, name: &str) -> bool {
        self.cook_value(name)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Read input as a string. Scalars produce unquoted text (not JSON-quoted).
    pub fn in_str(&self, name: &str) -> String {
        boxvalue_to_display_string(self.cook_value(name).as_ref())
    }

    pub fn in_vec3(&self, name: &str) -> [f64; 3] {
        self.cook_value(name)
            .and_then(|v| v.as_vec3())
            .unwrap_or([0.0; 3])
    }

    pub fn in_color(&self, name: &str) -> [f64; 4] {
        self.cook_value(name)
            .and_then(|v| v.as_color())
            .unwrap_or([0.0, 0.0, 0.0, 1.0])
    }

    /// Write an output's stored value (called by cook closures).
    pub fn set_out(&mut self, name: &str, value: BoxValue) {
        if self.output_attr_id(name).is_some() {
            self.put_key(&port_path("outputs", name, Some("value")), value);
        } else {
            self.put_key(&port_path("outputs", name, None), make_attr(value));
        }
    }

    // ---- connections (consumer-side source; always on the INPUT side — outputs
    // ---- are computed, never "connected from" anything) ---------------------

    /// Source feeding input `name`, as `(source_node, source_attr_uuid)`.
    pub fn source_of(&self, name: &str) -> Option<(NodeId, String)> {
        let src = self
            .data
            .get_path_key(&port_path("inputs", name, Some("source")))?;
        let obj = src.as_object()?;
        let node = obj.get("node").and_then(|v| v.as_str())?;
        let attr = obj.get("attr").and_then(|v| v.as_str())?;
        Some((NodeId::parse(node)?, attr.to_string()))
    }

    /// Whether input `name` is driven by a connection.
    pub fn is_connected(&self, name: &str) -> bool {
        self.source_of(name).is_some()
    }

    /// Set the source of input `name` to `(node, attr_uuid)` (`attr_uuid` is the
    /// SOURCE node's OUTPUT attr id — see [`Self::output_attr_id`]).
    pub fn set_source(&mut self, name: &str, node: NodeId, attr_uuid: &str) {
        let mut obj = BoxValue::object();
        if let Some(m) = obj.as_object_mut() {
            m.insert("node".into(), BoxValue::String(node.to_string()));
            m.insert("attr".into(), BoxValue::String(attr_uuid.to_string()));
        }
        self.put_key(&port_path("inputs", name, Some("source")), obj);
    }

    /// Clear the source of input `name`.
    pub fn clear_source(&mut self, name: &str) {
        self.put_key(&port_path("inputs", name, Some("source")), BoxValue::Null);
    }

    // ---- hierarchy -----------------------------------------------------------

    pub fn parent(&self) -> Option<NodeId> {
        self.data.get_str("parent").and_then(NodeId::parse)
    }

    pub fn set_parent(&mut self, parent: Option<NodeId>) {
        let v = match parent {
            Some(id) => BoxValue::String(id.to_string()),
            None => BoxValue::Null,
        };
        self.put("parent", v);
    }

    pub fn children(&self) -> Vec<NodeId> {
        name_list(&self.data, "children")
            .iter()
            .filter_map(|s| NodeId::parse(s))
            .collect()
    }

    pub fn add_child(&mut self, child: NodeId) {
        let mut kids = self.children();
        if !kids.contains(&child) {
            kids.push(child);
            self.write_children(&kids);
        }
    }

    /// Append many children with a SINGLE list rewrite (vs repeated [`Self::add_child`], which
    /// re-serializes the growing list each call). De-duplicates against existing children.
    pub fn add_children(&mut self, new_kids: &[NodeId]) {
        if new_kids.is_empty() {
            return;
        }
        let mut kids = self.children();
        let existing: std::collections::HashSet<NodeId> = kids.iter().copied().collect();
        let mut seen = existing;
        for &c in new_kids {
            if seen.insert(c) {
                kids.push(c);
            }
        }
        self.write_children(&kids);
    }

    pub fn remove_child(&mut self, child: NodeId) {
        let mut kids = self.children();
        kids.retain(|c| *c != child);
        self.write_children(&kids);
    }

    fn write_children(&mut self, kids: &[NodeId]) {
        let arr: Vec<BoxValue> = kids
            .iter()
            .map(|c| BoxValue::String(c.to_string()))
            .collect();
        self.put("children", BoxValue::array(arr));
    }

    // ---- view ----------------------------------------------------------------

    pub fn view_pos(&self) -> [f32; 2] {
        read_xy(&self.data, "view.pos").unwrap_or([0.0, 0.0])
    }

    pub fn set_view_pos(&mut self, pos: [f32; 2]) {
        write_xy(&mut self.data, "view.pos", pos);
    }

    pub fn view_size(&self) -> [f32; 2] {
        read_xy(&self.data, "view.size").unwrap_or([320.0, 180.0])
    }

    pub fn set_view_size(&mut self, size: [f32; 2]) {
        write_xy(&mut self.data, "view.size", size);
    }

    /// Remembered Full-mode body height, independent of `view.size`. `view.size[1]` is
    /// overwritten on every view-mode change (Minimal/Mid need a short height for their
    /// content) — without a separate memory, cycling through those modes and back to Full
    /// would permanently lose the original (or user-resized) Full-mode height, collapsing
    /// the node to bare content height. Falls back to the current `view.size[1]` for nodes
    /// saved before this field existed.
    pub fn full_view_height(&self) -> f32 {
        self.data
            .get_path("view.full_h")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .unwrap_or_else(|| self.view_size()[1])
    }

    pub fn set_full_view_height(&mut self, h: f32) {
        self.put("view.full_h", BoxValue::F64(h as f64));
    }

    /// In-plane node orientation in radians (`view.rotation`), default `0` (axis-aligned). Written
    /// by the live [`crate::force_sim::ForceSim`] when its `rotate_nodes` option is on so the card
    /// can "unfold" along its connection springs; renderers read it to draw a rotated body/pins.
    pub fn view_rotation(&self) -> f32 {
        self.data
            .get_path("view.rotation")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .unwrap_or(0.0)
    }

    /// Set the in-plane node orientation (radians). See [`Self::view_rotation`].
    pub fn set_view_rotation(&mut self, radians: f32) {
        self.put("view.rotation", BoxValue::F64(radians as f64));
    }

    pub fn view_mode(&self) -> NodeViewMode {
        match self.data.get_i64("view.mode").unwrap_or(2) {
            0 => NodeViewMode::Minimal,
            1 => NodeViewMode::Mid,
            _ => NodeViewMode::Full,
        }
    }

    /// What this node's body renders (plain / live 2D / live 3D). Default [`NodeViewKind::Flat2D`].
    pub fn view_kind(&self) -> NodeViewKind {
        NodeViewKind::from_i64(self.data.get_i64("view.view_kind").unwrap_or(0))
    }

    /// Set the node-body render kind (see [`NodeViewKind`]).
    pub fn set_view_kind(&mut self, kind: NodeViewKind) {
        self.put("view.view_kind", BoxValue::I64(kind.to_i64()));
    }

    /// Cycle the node-body render kind and return the new value (editor toggle).
    pub fn cycle_view_kind(&mut self) -> NodeViewKind {
        let next = self.view_kind().cycle();
        self.set_view_kind(next);
        next
    }

    pub fn set_view_mode(&mut self, mode: NodeViewMode) {
        let n = match mode {
            NodeViewMode::Minimal => 0,
            NodeViewMode::Mid => 1,
            NodeViewMode::Full => 2,
        };
        self.put("view.mode", BoxValue::I64(n));
    }

    /// Box path storing a port's display level for `side`, index `i`.
    fn port_level_path(side: PinSide, i: usize) -> String {
        let key = match side {
            PinSide::Input => "view.in_level",
            PinSide::Output => "view.out_level",
        };
        format!("{key}.{i}")
    }

    /// Per-port DISPLAY LEVEL: the minimum [`NodeViewMode::level`] at which this port draws a
    /// socket + label (0 = shown even collapsed, 1 = from Mid up, 2 = Full only). Unset ports
    /// use an index-based default — the FIRST port on a side is level 0 (so a collapsed node
    /// always shows its primary socket) and the rest are level 1 (Mid). Layout still positions
    /// every port regardless, so wires never dangle (see [`crate::displayed_pin_indices`]).
    pub fn port_display_level(&self, side: PinSide, i: usize) -> u8 {
        self.data
            .get_i64(&Self::port_level_path(side, i))
            .map(|v| v.clamp(0, 2) as u8)
            .unwrap_or(if i == 0 { 0 } else { 1 })
    }

    /// Set a port's [`Self::port_display_level`] (clamped to `0..=2`).
    pub fn set_port_display_level(&mut self, side: PinSide, i: usize, level: u8) {
        self.put(
            &Self::port_level_path(side, i),
            BoxValue::I64(level.min(2) as i64),
        );
    }

    /// Explicit orientation override (`view.orientation`: 0 = horizontal, 1 =
    /// vertical). `None` means "auto" — resolved from aspect ratio via
    /// [`Self::resolved_orientation`] — which is the default for every node
    /// unless deliberately overridden.
    pub fn orientation_pref(&self) -> Option<PinEdgeLayout> {
        match self.data.get_i64("view.orientation") {
            Some(0) => Some(PinEdgeLayout::Horizontal),
            Some(1) => Some(PinEdgeLayout::Vertical),
            _ => None,
        }
    }

    pub fn set_orientation_pref(&mut self, pref: Option<PinEdgeLayout>) {
        match pref {
            None => box_del(&mut self.data, "view.orientation"),
            Some(PinEdgeLayout::Horizontal) => self.put("view.orientation", BoxValue::I64(0)),
            Some(PinEdgeLayout::Vertical) => self.put("view.orientation", BoxValue::I64(1)),
        }
    }

    /// Effective orientation for a `[w, h]` view size: the explicit preference if
    /// set, else aspect-based auto-detection (see [`crate::chrome::pin_edge_layout`]).
    pub fn resolved_orientation(&self, w: f32, h: f32) -> PinEdgeLayout {
        resolve_orientation(self.orientation_pref(), w, h)
    }

    pub fn bypass(&self) -> bool {
        self.data.get_bool("view.bypass").unwrap_or(false)
    }

    pub fn set_bypass(&mut self, on: bool) {
        self.put("view.bypass", BoxValue::Bool(on));
    }

    pub fn display_flag(&self) -> bool {
        self.data.get_bool("view.display").unwrap_or(false)
    }

    pub fn set_display_flag(&mut self, on: bool) {
        self.put("view.display", BoxValue::Bool(on));
    }

    pub fn dirty(&self) -> bool {
        self.data.get_bool("view.dirty").unwrap_or(true)
    }

    pub fn set_dirty(&mut self, on: bool) {
        self.put_key(&P_VIEW_DIRTY, BoxValue::Bool(on));
    }

    /// Cook generation this node was last (re)computed at (see [`crate::Graph::cook_gen`]).
    ///
    /// Stamped by the executor in `eval_one` every time the node runs, stored in the transient
    /// `tmp.cooked_gen` (never serialized). `0` means "never cooked this session". Consumers
    /// (e.g. live node previews in `nodes-view`) cache the gen they last rendered and compare:
    /// a bump means the node's outputs are fresh and any derived GPU preview must regenerate.
    pub fn cooked_gen(&self) -> u64 {
        self.data.get_i64("tmp.cooked_gen").unwrap_or(0).max(0) as u64
    }

    /// Stamp the cook generation this node was just computed at (executor-only).
    pub fn set_cooked_gen(&mut self, generation: u64) {
        self.put_key(&P_COOKED_GEN, BoxValue::I64(generation as i64));
    }

    /// What the cook driver last did to this node ([`NodeStatus`]).
    ///
    /// Stored in the transient `tmp.status` section (never serialized — like `tmp.cooked_gen`), so
    /// a freshly loaded graph reports [`NodeStatus::Idle`] until it is cooked. Hosts read this to
    /// tint a node / show a spinner or an error badge without keeping a parallel side-table.
    pub fn status(&self) -> NodeStatus {
        match self.data.get_i64("tmp.status.kind").unwrap_or(0) {
            1 => NodeStatus::Cooking {
                progress: self
                    .data
                    .get_path("tmp.status.progress")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32,
                msg: self.status_msg(),
            },
            2 => NodeStatus::Done {
                elapsed_s: self
                    .data
                    .get_path("tmp.status.elapsed_s")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            },
            3 => NodeStatus::Error(self.status_msg()),
            _ => NodeStatus::Idle,
        }
    }

    fn status_msg(&self) -> String {
        self.data
            .get_str("tmp.status.msg")
            .unwrap_or("")
            .to_string()
    }

    /// Record the node's cook status (driver + [`crate::cook::CookCtx::phase`]).
    ///
    /// A failed write here would leave the node reporting `Idle` forever with no clue why, so the
    /// box errors are surfaced (`log::error`) rather than discarded — this is the `let _ =` trap
    /// this crate's notes warn about.
    pub(crate) fn set_status(&mut self, status: NodeStatus) {
        let (kind, msg, progress, elapsed) = match status {
            NodeStatus::Idle => (0, String::new(), 0.0, 0.0),
            NodeStatus::Cooking { progress, msg } => (1, msg, progress as f64, 0.0),
            NodeStatus::Done { elapsed_s } => (2, String::new(), 1.0, elapsed_s),
            NodeStatus::Error(msg) => (3, msg, 0.0, 0.0),
        };

        // ONE navigation for all four fields. This used to be four `set_path`
        // calls, each parsing `"tmp.status.<field>"` into segments and walking
        // the box from the root — and the driver calls this twice on every node
        // on every tick, so eight walks per node bought four scalars.
        //
        // The field names are written into the map directly rather than through
        // `set_path`, which is only safe because they are plain lowercase
        // identifiers: every key transform the box config can apply (camel
        // killer, conversion box) is the identity on them.
        let slot = match self.data.get_path_key_mut(&P_STATUS) {
            Ok(slot) => slot,
            Err(e) => {
                log::error!(
                    "node '{}': failed to record cook status: {e}",
                    self.data.get_str("name").unwrap_or("")
                );
                return;
            }
        };
        if !slot.is_object() {
            *slot = BoxValue::object();
        }
        let Some(m) = slot.as_object_mut() else {
            return;
        };
        m.insert("kind".into(), BoxValue::I64(kind));
        m.insert("msg".into(), BoxValue::String(msg));
        m.insert("progress".into(), BoxValue::F64(progress));
        m.insert("elapsed_s".into(), BoxValue::F64(elapsed));
    }
}

// ---- module-level helpers ---------------------------------------------------

/// Write `value` at `path` in a node's `data` box, SURFACING a failed write instead of dropping it.
///
/// This is the crate's single node-data write primitive: every `data.set_*` in `node.rs`,
/// `script.rs` and `serialization.rs` goes through it (they all used to be `let _ = …`, which is
/// the "never silently discard errors" rule broken ~58 times).
///
/// Why it matters, given that most writes use a LITERAL path (`"view.dirty"`, `"parent"`, …) that
/// cannot realistically fail: the DYNAMIC ones (`inputs.{name}.value`, `compute.{name}`, …) embed a
/// port/attr name that comes from a `NodeDef`, a saved graph or a rhai script. box-rs's `parse_path`
/// reads `.` and `[`/`]` as path STRUCTURE, so a port called `a.b` would write into a nested key
/// nobody reads back, and one containing `[` fails outright — in both cases the value vanished with
/// no trace. [`crate::validate_def`] now rejects such names at registration, and this logs anything
/// that still slips through (e.g. a hand-edited graph file).
///
/// It intentionally does NOT propagate: a `Result` on ~58 infallible-in-practice setters would be
/// pure churn, and a failed write is a bug to be seen in the log, not a case for a caller to branch
/// on. Same treatment as [`Node::set_status`], which established this convention.
pub(crate) fn box_put(data: &mut RustBox, path: &str, value: BoxValue) {
    if let Err(e) = data.set_path(path, value) {
        log::error!("node data: write to '{path}' failed ({e}); the value was DROPPED");
    }
}

/// Delete `path` from a node's `data` box. Deleting an absent path is a no-op (removal is
/// idempotent — `view.orientation` and the transient sections are legitimately optional); any OTHER
/// box error is surfaced. Counterpart of [`box_put`].
pub(crate) fn box_del(data: &mut RustBox, path: &str) {
    match data.remove_path(path) {
        Ok(_) | Err(crate::box_model::BoxError::PathNotFound(_)) => {}
        Err(e) => log::error!("node data: removal of '{path}' failed ({e})"),
    }
}

/// Produce a human-readable string from a `BoxValue` without JSON quoting for
/// scalars. Used by both [`Node::in_str`] and the `"String"` type's coercion closure in
/// [`crate::value_types::register_builtin_types`], so they are consistent.
pub(crate) fn boxvalue_to_display_string(v: Option<&BoxValue>) -> String {
    match v {
        Some(BoxValue::String(s)) => s.clone(),
        Some(BoxValue::F64(f)) => f.to_string(),
        Some(BoxValue::I64(i)) => i.to_string(),
        Some(BoxValue::Bool(b)) => b.to_string(),
        Some(other) => other.to_json_value().to_string(),
        None => String::new(),
    }
}

/// `{ id: <uuid>, value: <v>, source: null }`.
pub(crate) fn make_attr(value: BoxValue) -> BoxValue {
    let mut obj = BoxValue::object();
    if let Some(m) = obj.as_object_mut() {
        m.insert("id".into(), BoxValue::String(Uuid::new_v4().to_string()));
        m.insert("value".into(), value);
        m.insert("source".into(), BoxValue::Null);
    }
    obj
}

/// Default `view` object. Note: `dirty` is NOT stored here; it is runtime-only
/// and is excluded from serialization (blanked like output values).
fn default_view() -> BoxValue {
    let mut v = BoxValue::object();
    if let Some(m) = v.as_object_mut() {
        m.insert(
            "pos".into(),
            BoxValue::array(vec![BoxValue::F64(0.0), BoxValue::F64(0.0)]),
        );
        m.insert(
            "size".into(),
            BoxValue::array(vec![BoxValue::F64(320.0), BoxValue::F64(180.0)]),
        );
        m.insert("mode".into(), BoxValue::I64(2)); // Full
        m.insert("bypass".into(), BoxValue::Bool(false));
        m.insert("display".into(), BoxValue::Bool(false));
        // dirty is transient — not written here; set_dirty(true) is called after load.
    }
    v
}

/// Read an ordered array of strings at `path`.
fn name_list(data: &RustBox, path: &str) -> Vec<String> {
    data.get_path(path)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Read a 2-component `[f32;2]` array at `path`.
fn read_xy(data: &RustBox, path: &str) -> Option<[f32; 2]> {
    let arr = data.get_path(path)?.as_array()?;
    if arr.len() != 2 {
        return None;
    }
    Some([arr[0].as_f64()? as f32, arr[1].as_f64()? as f32])
}

fn write_xy(data: &mut RustBox, path: &str, xy: [f32; 2]) {
    box_put(
        data,
        path,
        BoxValue::array(vec![
            BoxValue::F64(xy[0] as f64),
            BoxValue::F64(xy[1] as f64),
        ]),
    );
}
