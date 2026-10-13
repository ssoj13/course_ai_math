//! Catalog of node types: `type_name` → [`RegEntry`] (cached [`NodeDef`] + the [`NodeType`]).
//!
//! Populated by adding LIBRARIES ([`NodeTypeRegistry::add_library`]) — the builtins are one
//! ([`crate::builtins::Builtins`]) and an external compute crate exports its own the same way, so
//! there is no privileged in-crate registration path. Passed into [`crate::Graph::create_node`] and
//! the cook driver.
//!
//! ## One authority per type
//!
//! A type's SHAPE (`def`) is produced ONCE by [`NodeType::def`] and cached here; its BEHAVIOUR is
//! the `Arc<dyn NodeType>` beside it. Before, the cook closure lived ON the def while the def lived
//! in the registry — two half-authorities for one type, and no place at all for a type's own state.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{CoreError, CoreResult};
use crate::name::validate_path_name;
use crate::node::{AttrDef, NodeDef};
use crate::node_type::{NodeKind, NodeLibrary, NodeType};
use crate::value_types::ValueTypes;

/// Validate a [`NodeDef`] before it is registered: its type name, every input/output port name and
/// every [`AttrDef`] key must survive as a single box-path segment.
///
/// The rule itself lives with the crate's other name rules, in [`crate::name`]
/// ([`validate_path_name`] / [`crate::name::PATH_HOSTILE`]) — the same no-`.` discipline
/// [`crate::validate_node_name`] has always enforced for NODE names, finally applied to the
/// port/attr namespace where it was missing.
///
/// [`NodeTypeRegistry::add`] / [`NodeTypeRegistry::add_library`] run this themselves and REFUSE a
/// bad type (logging why). It is public so a host can pre-check a library it is about to add —
/// `validate_library(&MyNodes)?` — and get the error as a value instead of a log line.
pub fn validate_def(def: &NodeDef) -> CoreResult<()> {
    let owner = &*def.type_name;
    validate_path_name("type", owner, owner)?;
    for p in &def.inputs {
        validate_path_name("input port", owner, &p.name)?;
    }
    for p in &def.outputs {
        validate_path_name("output port", owner, &p.name)?;
    }
    for a in &def.schema {
        validate_path_name("attr", owner, &a.key)?;
    }
    Ok(())
}

/// [`validate_def`] for every type in a library — what a host calls to fail fast on a bad node pack
/// instead of discovering at runtime that one of its types never registered.
pub fn validate_library(lib: &dyn NodeLibrary) -> CoreResult<()> {
    for ty in lib.types() {
        validate_def(&ty.def())?;
    }
    Ok(())
}

/// Give every port whose default is `Null` the PROTOTYPE of its registered value type.
///
/// A [`NodeType::def`] is built with no registry in hand — `PortDef::typed("cloud", "PointCloud")`
/// cannot know what a `PointCloud` looks like, so it yields `Null`. And a `Null` prototype is
/// exactly what makes the type checks mis-judge: [`ValueTypes::compatible`] probes it, and
/// [`ValueTypes::convert`] matches an `Any`-sourced value against it, so a `Null`-defaulted host
/// type would be judged incompatible with everything and accept nothing. The author was expected to
/// hand-spell the prototype and the docs called it a footgun.
///
/// It does not have to be: a type DECLARES its own value types ([`NodeType::value_types`]), and
/// they are registered a few lines above this call — so the prototype IS known by the time the def
/// is cached, and the registry can simply fill it in. Turns a documented footgun into an
/// impossibility.
///
/// Only `Null` defaults are touched: an author who spelled a real default keeps it, and a type whose
/// prototype is HONESTLY `Null` (`Image` — absent until a node cooks one) is unaffected.
fn reseed_port_defaults(def: &mut NodeDef, types: &ValueTypes) {
    let reseed = |p: &mut crate::port::PortDef| {
        if matches!(p.default, crate::box_model::BoxValue::Null) {
            let proto = types.make_default(&p.port_type);
            if !matches!(proto, crate::box_model::BoxValue::Null) {
                p.default = proto;
            }
        }
    };
    def.inputs.iter_mut().for_each(reseed);
    def.outputs.iter_mut().for_each(reseed);
}

/// Registration gate: `true` = the def may enter the registry. A def that fails [`validate_def`] is
/// REFUSED and the reason logged (naming the type, the offending name and the character).
///
/// Refusing rather than returning a `Result` is deliberate: `add`/`add_library` stay infallible, so
/// the ergonomic `registry.add(Arc::new(MyNode), &mut types)` keeps working and no host call site
/// has to change. A refused type simply is not in the registry — it never appears in the node menu
/// and `create_node` fails loudly with `UnknownNodeType`, which is exactly the outcome a broken def
/// deserves. A host that wants the error as a VALUE calls [`validate_def`] / [`validate_library`]
/// first.
fn accept(def: &NodeDef, src: Option<&str>) -> bool {
    match validate_def(def) {
        Ok(()) => true,
        Err(e) => {
            log::error!(
                "REFUSING to register node type{}: {e}",
                src.map(|s| format!(" from library '{s}'"))
                    .unwrap_or_default(),
            );
            false
        }
    }
}

/// One registered node type: its cached shape and its behaviour.
#[derive(Clone)]
pub struct RegEntry {
    /// The type's shape, cached from [`NodeType::def`] at registration (which is why that method
    /// must be pure — see its docs). EVERY data-driven consumer reads this copy.
    pub def: NodeDef,
    /// The type's behaviour: what the cook driver calls, and where a type's own state lives.
    pub ty: Arc<dyn NodeType>,
}

/// Catalog of creatable node types.
#[derive(Default, Clone)]
pub struct NodeTypeRegistry {
    entries: HashMap<Arc<str>, RegEntry>,
}

impl NodeTypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one node type, and register the custom value types it declares
    /// ([`NodeType::value_types`]) into `types` — so a host that adds a node with a `"Recon"` port
    /// gets the `"Recon"` value type (its coercion rule + pin visual) without knowing it exists.
    ///
    /// Re-adding a name REPLACES the entry (that is how a host overrides a builtin) — and logs a
    /// `warn` naming what was replaced, because an intentional override and an accidental name
    /// clash between two libraries are indistinguishable to a silent `insert`, while the accident
    /// swaps a type's BEHAVIOUR under every graph that already uses that name.
    ///
    /// A type whose [`NodeDef`] fails [`validate_def`] (a path-hostile type/port/attr name) is
    /// REFUSED, with a `log::error!` naming it — see [`accept`].
    pub fn add(&mut self, ty: Arc<dyn NodeType>, types: &mut ValueTypes) {
        self.add_from(ty, types, None);
    }

    /// Register a whole [`NodeLibrary`]: its library-wide value types, then each of its types
    /// (each contributing its own value types via [`Self::add`]).
    ///
    /// This is the ONE call a host makes to plug an external node pack in:
    /// `registry.add_library(&MyNodes, &mut value_types)`.
    ///
    /// The library's [`NodeLibrary::name`] is threaded into the override warnings, so a collision
    /// report says WHICH pack replaced the node type / value type (`"colmap" replaced "Image"`).
    pub fn add_library(&mut self, lib: &dyn NodeLibrary, types: &mut ValueTypes) {
        let src = lib.name();
        for info in lib.value_types() {
            types.register_from(info, Some(src));
        }
        for ty in lib.types() {
            self.add_from(ty, types, Some(src));
        }
    }

    /// [`Self::add`] with the registering library's name (`None` = a standalone type), used for the
    /// override warning. A type whose [`NodeDef`] fails [`validate_def`] is refused BEFORE its value
    /// types are registered, so a rejected type leaves nothing of itself behind.
    fn add_from(&mut self, ty: Arc<dyn NodeType>, types: &mut ValueTypes, src: Option<&str>) {
        let mut def = ty.def();
        if !accept(&def, src) {
            return;
        }
        for info in ty.value_types() {
            types.register_from(info, src);
        }
        reseed_port_defaults(&mut def, types);
        self.put_entry(def, ty, src);
    }

    /// Insert a type WITHOUT touching a `ValueTypes` — crate-internal, for the display-only
    /// registry built from serialized schemas (`schema.rs`), whose types are inert stubs that
    /// declare no value types at all. Validated exactly like [`Self::add`]: a schema file is
    /// EXACTLY the untrusted source a path-hostile port name arrives from.
    pub(crate) fn insert(&mut self, ty: Arc<dyn NodeType>) {
        self.insert_from(ty, None);
    }

    /// [`Self::insert`] with the registering library's name.
    fn insert_from(&mut self, ty: Arc<dyn NodeType>, src: Option<&str>) {
        let def = ty.def();
        if !accept(&def, src) {
            return;
        }
        self.put_entry(def, ty, src);
    }

    /// The one place a node type enters the map (its `def` already validated). Replacing an existing
    /// name is ALLOWED (a host overriding a builtin is a supported use) but never silent — see
    /// [`Self::add`].
    fn put_entry(&mut self, mut def: NodeDef, ty: Arc<dyn NodeType>, src: Option<&str>) {
        // The TYPE is the authority on its kind; the cached def carries a copy purely so that
        // `Node::from_def` (which has a def and no registry) can seed a Job's outputs correctly — see
        // `NodeDef::kind`. Stamping it HERE, at the one place a type enters the map, is what keeps the
        // two from ever disagreeing: a def cannot be registered with a kind its type did not declare.
        def.kind = ty.kind();
        let key = def.type_name.clone();
        if let Some(prev) = self.entries.insert(key.clone(), RegEntry { def, ty }) {
            log::warn!(
                "node type '{key}' overridden{}: the previous definition (display '{}', category \
                 '{}') is REPLACED, including its cook behaviour",
                src.map(|s| format!(" by library '{s}'"))
                    .unwrap_or_default(),
                prev.def.display_name,
                prev.def.category,
            );
        }
    }

    /// The cached [`NodeDef`] for `name` — ports, display name, category, inspector schema. This is
    /// what every data-driven consumer (node menu, inspector, layout, renderers) reads.
    pub fn get(&self, name: &str) -> Option<&NodeDef> {
        self.entries.get(name).map(|e| &e.def)
    }

    /// The BEHAVIOUR registered for `name` — what the cook driver invokes.
    pub fn node_type(&self, name: &str) -> Option<&Arc<dyn NodeType>> {
        self.entries.get(name).map(|e| &e.ty)
    }

    /// Both halves at once (shape + behaviour).
    pub fn entry(&self, name: &str) -> Option<&RegEntry> {
        self.entries.get(name)
    }

    /// Is type `name` a dataflow function or a JOB (see [`crate::NodeKind`])? An UNKNOWN type reports
    /// [`crate::NodeKind::Dataflow`] — the safe default: the cook driver skips jobs, and an
    /// unregistered type has no cook to run anyway, so nothing can be executed by accident.
    ///
    /// The TYPE is the authority here; the kind is deliberately not copied onto node instances, so a
    /// saved graph can never carry a stale answer that disagrees with the type's own definition.
    pub fn kind(&self, name: &str) -> NodeKind {
        self.node_type(name).map(|ty| ty.kind()).unwrap_or_default()
    }

    /// The kind of a node INSTANCE, resolved through its type name (see [`Self::kind`]).
    pub fn kind_of(&self, node: &crate::Node) -> NodeKind {
        self.kind(node.type_name())
    }

    /// [`Self::get`], as a `Result` — for call sites that must fail loudly on an unknown type.
    pub fn require(&self, name: &str) -> CoreResult<&NodeDef> {
        self.get(name)
            .ok_or_else(|| CoreError::UnknownNodeType(name.to_string()))
    }

    /// Attach (or replace) the per-attribute UI [`AttrDef`] schema of an already-registered type.
    ///
    /// A type normally declares its schema in its own `def()` ([`NodeDef::with_schema`]); this is
    /// the escape hatch for a HOST that wants to re-skin someone else's type (add a tooltip, turn a
    /// float into a slider) without reimplementing it. No-op (`false`) if `type_name` is unknown.
    pub fn set_schema(&mut self, type_name: &str, schema: Vec<AttrDef>) -> bool {
        // Guard a footgun: a `choices` set renders as a ComboBox/RadioButton that writes a String
        // value, so it is only valid on a String attr. Attaching choices to a numeric attr would
        // silently write a String into a numeric slot and read back as 0.
        #[cfg(debug_assertions)]
        for a in &schema {
            debug_assert!(
                a.choices.is_none() || a.ty == "String",
                "AttrDef '{}.{}': `choices` is only supported on a String attr (got ty={})",
                type_name,
                a.key,
                a.ty
            );
        }
        if let Some(entry) = self.entries.get_mut(type_name) {
            entry.def.schema = schema;
            true
        } else {
            false
        }
    }

    /// Every registered type name (unordered).
    pub fn type_names(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.keys().map(|k| &**k)
    }

    /// Defs grouped by category, each group sorted by display name — the node-menu source.
    pub fn by_category(&self) -> HashMap<&str, Vec<&NodeDef>> {
        let mut map: HashMap<&str, Vec<&NodeDef>> = HashMap::new();
        for entry in self.entries.values() {
            map.entry(&entry.def.category).or_default().push(&entry.def);
        }
        for v in map.values_mut() {
            v.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::passthrough_cook;
    use crate::node_type::fn_node;
    use crate::port::PortDef;

    /// A def with one input port, for the name-validation tests.
    fn def_with_input(ty: &str, port: &str) -> NodeDef {
        NodeDef::new(ty, "Test")
            .with_display(ty)
            .with_inputs(vec![PortDef::float(port, 0.0)])
    }

    /// A path-hostile PORT name must be caught at registration, not silently lose every write to
    /// that port at cook time (box-rs would read the `.` in `a.b` as path structure and drop the
    /// value into a nested key nobody reads).
    #[test]
    fn path_hostile_port_name_is_refused_at_registration() {
        let mut types = ValueTypes::new();
        let mut reg = NodeTypeRegistry::new();

        reg.add(
            fn_node(def_with_input("Bad", "a.b"), passthrough_cook),
            &mut types,
        );
        assert!(
            reg.get("Bad").is_none(),
            "a type with a path-hostile port must NOT register"
        );

        // The error names the type, the port and the offending character.
        let err = validate_def(&def_with_input("Bad", "a.b"))
            .expect_err("'a.b' must be rejected")
            .to_string();
        assert!(err.contains("Bad"), "names the type: {err}");
        assert!(err.contains("a.b"), "names the port: {err}");
        assert!(err.contains('\''), "quotes the character: {err}");

        // A clean def of the same shape still registers — the gate is not over-eager.
        reg.add(
            fn_node(def_with_input("Good", "a"), passthrough_cook),
            &mut types,
        );
        assert!(reg.get("Good").is_some());
    }

    /// The same gate covers the TYPE name (a map key + every log message), the OUTPUT ports, and an
    /// empty name (which would address the box root).
    #[test]
    fn validate_def_rejects_hostile_type_output_and_empty_names() {
        assert!(
            validate_def(&def_with_input("My[Node]", "a")).is_err(),
            "type name"
        );
        assert!(
            validate_def(&def_with_input("Ok", "")).is_err(),
            "empty port name"
        );
        let bad_out = NodeDef::new("Ok", "Test").with_outputs(vec![PortDef::float("x[0]", 0.0)]);
        assert!(validate_def(&bad_out).is_err(), "output port name");
        assert!(
            validate_def(&def_with_input("Ok", "a")).is_ok(),
            "a clean def passes"
        );
    }

    /// Every builtin passes the gate — the validator is a real guard, not a rule the crate's own
    /// types break.
    #[test]
    fn builtins_library_validates() {
        validate_library(&crate::builtins::Builtins).expect("the builtins must be path-safe");
    }

    /// A port of a CUSTOM type gets its prototype from the type the node itself declares — the
    /// author never has to hand-spell it, and can no longer leave the `Null` that makes every type
    /// check mis-judge (see `reseed_port_defaults`).
    #[test]
    fn port_default_is_reseeded_from_the_types_own_value_type() {
        use crate::box_model::{BoxValue, DType};
        use crate::port::PinVisual;
        use crate::value_types::ValueTypeInfo;

        let cloud = || BoxValue::buffer(DType::F32, vec![3], vec![0u8; 12]);

        // A type whose port is typed `PointCloud` and — as any real `def()` must — could only give it
        // a `Null` default, but which DECLARES the `PointCloud` value type with a real prototype.
        struct CloudNode;
        impl NodeType for CloudNode {
            fn def(&self) -> NodeDef {
                NodeDef::new("MakeCloud", "Test")
                    .with_outputs(vec![crate::port::PortDef::typed("cloud", "PointCloud")])
            }
            fn value_types(&self) -> Vec<ValueTypeInfo> {
                vec![
                    ValueTypeInfo::new("PointCloud", PinVisual::ANY)
                        .with_default(BoxValue::buffer(DType::F32, vec![3], vec![0u8; 12])),
                ]
            }
            fn cook(&self, _ctx: &mut crate::cook::CookCtx<'_>) -> crate::cook::CookResult<()> {
                Ok(())
            }
        }

        // Before registration the def can only say `Null` — that is the footgun.
        assert_eq!(CloudNode.def().outputs[0].default, BoxValue::Null);

        let mut types = ValueTypes::new();
        let mut reg = NodeTypeRegistry::new();
        reg.add(Arc::new(CloudNode), &mut types);

        let def = reg.get("MakeCloud").expect("registered");
        assert_eq!(
            def.outputs[0].default,
            cloud(),
            "the cached def must carry the value type's real prototype, not Null"
        );
    }

    /// Locks the documented OVERRIDE semantics: registering a name twice REPLACES the entry (last
    /// write wins) — a host overriding a builtin is supported, so the collision must not be
    /// rejected. It is merely no longer silent (see `insert_from`'s `log::warn!`).
    #[test]
    fn re_registered_type_name_keeps_the_last_definition() {
        let mut types = ValueTypes::new();
        let mut reg = NodeTypeRegistry::new();

        reg.add(
            fn_node(
                NodeDef::new("Dup", "First").with_display("First"),
                passthrough_cook,
            ),
            &mut types,
        );
        reg.add(
            fn_node(
                NodeDef::new("Dup", "Second").with_display("Second"),
                passthrough_cook,
            ),
            &mut types,
        );

        let def = reg.get("Dup").expect("'Dup' is registered");
        assert_eq!(&*def.display_name, "Second", "the LAST registration wins");
        assert_eq!(&*def.category, "Second");
        assert_eq!(
            reg.type_names().count(),
            1,
            "an override replaces, it does not append"
        );
    }
}
