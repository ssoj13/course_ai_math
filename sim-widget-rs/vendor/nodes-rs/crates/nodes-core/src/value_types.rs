//! Value-type registry — the **single source of truth** for what a port type *is*.
//!
//! A type is a name (`"Float"`, `"Vec3"`, `"Color"`, a host's `"Recon"`, …) plus:
//! - a [`PinVisual`] for renderers, and
//! - a **coercion kernel** ([`ValueTypeInfo::coerce`]) saying which foreign values may become
//!   a value of this type.
//!
//! ## Why the coercion lives here (and not in a hardcoded `convert()` match)
//!
//! Until 2026-07-13 the conversion kernel was a free `convert::convert(v, to)` — a `match` on
//! the type NAME ending in `_ => None` — while connect-compatibility passed anything with
//! `from == to`. A HOST-registered custom type therefore **connected fine but had its value
//! silently dropped on every cook** (`resolve_inputs` → `convert` → `None` → warn → the
//! consumer kept its own literal). `"Image"` only worked because an arm was hardcoded for it
//! in this crate. Moving the kernel into the registry is what makes a custom port type
//! actually FLOW: a host registers its type once, with its own coercion rule, and cook +
//! connect both obey it.
//!
//! ## Coercion contract
//!
//! - `coerce: Some(f)` — `f(v)` returns `Some(converted)` when `v` can become this type, `None`
//!   when it cannot. This is the ONLY thing that makes a *cross-type* wire legal (see
//!   [`ValueTypes::compatible`]).
//! - `coerce: None` — **identity type**: values of this type pass through unchanged, and NO
//!   other type converts into it. This is the right default for an opaque host type (a mesh, a
//!   reconstruction handle) that is never synthesised from a `Float`.
//! - `"Any"` is special-cased before the lookup: it accepts (and forwards) everything.

use std::collections::HashMap;
use std::sync::Arc;

use crate::box_model::BoxValue;
use crate::node::boxvalue_to_display_string;
use crate::port::{PinVisual, PortDef};
use crate::values::builtin_default as default_value;

/// The wildcard type name: accepts and forwards any value, both at connect and at cook time.
pub const ANY_TYPE: &str = "Any";

/// Coercion kernel for ONE target type: `Some(v')` when `v` can become a value of this type,
/// `None` when no conversion is defined (the wire is rejected at connect; a pre-existing wire
/// keeps the consumer's stored literal and warns at cook).
pub type CoerceFn = Arc<dyn Fn(&BoxValue) -> Option<BoxValue> + Send + Sync>;

/// Per-type metadata: name, pin visual, prototype value, and how foreign values coerce into
/// this type.
#[derive(Clone)]
pub struct ValueTypeInfo {
    /// Type name as it appears in `PortDef::port_type` (`"Float"`, `"Recon"`, …). `Arc<str>`
    /// rather than `&'static str` so a host can register a type whose name is computed at
    /// runtime (loaded from a plugin manifest, a schema file, …).
    pub name: Arc<str>,
    /// Renderer-facing pin colour/shape.
    pub visual: PinVisual,
    /// The type's **prototype value**: the default a fresh port of this type holds, AND the
    /// probe [`ValueTypes::compatible`] runs through other types' coercion kernels.
    ///
    /// It must be a REPRESENTATIVE value of the type (`F64(0.0)` for `Float`, an empty buffer for
    /// an image-like type), not `Null` — a `Null` prototype makes every coercion kernel reject the
    /// type, so a host type with a real payload would be judged incompatible with everything even
    /// though its real values coerce fine. `Null` remains the honest default only for a type whose
    /// values genuinely are opaque/absent-until-cooked.
    pub default: BoxValue,
    /// `None` = identity type (see the module docs). `Some(f)` = custom coercion.
    pub coerce: Option<CoerceFn>,
}

impl ValueTypeInfo {
    /// An **identity** type: its own values flow unchanged; nothing else converts into it.
    /// The right choice for an opaque host payload type. Prototype value defaults to `Null` —
    /// give it a real one with [`Self::with_default`] if this type's values have a shape (see
    /// the [`Self::default`] field docs for why that matters).
    pub fn new(name: impl Into<Arc<str>>, visual: PinVisual) -> Self {
        Self {
            name: name.into(),
            visual,
            default: BoxValue::Null,
            coerce: None,
        }
    }

    /// Set the type's prototype/default value (see the [`Self::default`] field docs).
    pub fn with_default(mut self, default: BoxValue) -> Self {
        self.default = default;
        self
    }

    /// Attach a coercion kernel: `f(v)` decides whether a value of ANY other type may become a
    /// value of this one (and what it becomes).
    pub fn with_coerce(
        mut self,
        f: impl Fn(&BoxValue) -> Option<BoxValue> + Send + Sync + 'static,
    ) -> Self {
        self.coerce = Some(Arc::new(f));
        self
    }
}

/// Registry of value-type metadata. Cheap to clone (shared with renderers).
#[derive(Default, Clone)]
pub struct ValueTypes {
    by_name: HashMap<Arc<str>, ValueTypeInfo>,
}

impl ValueTypes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a type. Re-registering a name overrides its visual + coercion,
    /// which is how a host customises a builtin — and logs a `warn` when it replaces one, because
    /// a silent overwrite of a COERCION KERNEL is invisible-wrong-data territory: two libraries
    /// both declaring `"Image"` means the second one's rule decides which wires carry a value and
    /// which quietly drop it, with nothing in the log to say so.
    pub fn register(&mut self, info: ValueTypeInfo) {
        self.register_from(info, None);
    }

    /// [`Self::register`] with the name of the registering library / node type (`None` = a bare
    /// host call), used only to make the override warning say WHO replaced the type. Crate-internal:
    /// [`crate::NodeTypeRegistry::add_library`] threads [`crate::NodeLibrary::name`] through it.
    pub(crate) fn register_from(&mut self, info: ValueTypeInfo, src: Option<&str>) {
        let name = info.name.clone();
        if self.by_name.insert(name.clone(), info).is_some() {
            log::warn!(
                "value type '{name}' overridden{}: its pin visual, prototype default AND coercion \
                 kernel are REPLACED (wires of this type may now coerce differently)",
                src.map(|s| format!(" by library '{s}'"))
                    .unwrap_or_default(),
            );
        }
    }

    pub fn get(&self, name: &str) -> Option<&ValueTypeInfo> {
        self.by_name.get(name)
    }

    /// Names of all registered types (unordered).
    pub fn names(&self) -> impl Iterator<Item = &str> + '_ {
        self.by_name.keys().map(|k| &**k)
    }

    /// The prototype/default [`BoxValue`] a fresh port of type `name` holds — read from the
    /// REGISTERED type ([`ValueTypeInfo::default`]), so a host type gets its own default rather
    /// than the `Null` a hardcoded name-match would hand it. Unregistered → `Null`.
    pub fn make_default(&self, name: &str) -> BoxValue {
        self.get(name)
            .map(|i| i.default.clone())
            .unwrap_or(BoxValue::Null)
    }

    /// A [`PortDef`] of type `ty` carrying this registry's default for that type. The
    /// registry-aware counterpart of [`PortDef::typed`] (which can only know the BUILTIN
    /// defaults) — a host declaring a port of its OWN type should build it through here.
    pub fn port_def(&self, name: &str, ty: &str) -> PortDef {
        PortDef::new(name, ty, self.make_default(ty))
    }

    /// Convert `v` — a value produced by a port of type `from` — into type `to`, or `None` when no
    /// conversion applies.
    ///
    /// This is the ONE conversion entry point: cook's connected-input resolution
    /// ([`mod@crate::cook`]) and the `Convert` builtin both call it, and [`Self::compatible`] is a
    /// thin wrapper over the SAME decision — so what the editor allows and what the cook carries can
    /// never drift apart again.
    ///
    /// ## Why `from` is a parameter (it did not used to be)
    ///
    /// Without the SOURCE type, an identity target (`coerce: None`) had to accept any `v` it was
    /// handed: `convert(F64(3.0), "PointCloud")` returned `Some(F64(3.0))`, so wiring an `Any`
    /// output (a `Switch`, an expression, `Null`) into a custom `PointCloud` input handed that node
    /// a Float wearing its type's name. That is the same silent-wrong-data class the hardcoded
    /// `convert.rs` kernel was deleted to kill, one level down.
    ///
    /// ## The rule
    /// - `to == ANY` → pass through (a wildcard port takes anything).
    /// - `to` unregistered → `None` (fail loud; never forward into a type we know nothing about).
    /// - `to` has a coercion kernel → run it on `v` (the kernel IS the type's admission rule).
    /// - `to` is an IDENTITY type (`coerce: None`):
    ///   - `from == to` → pass through (its own values flow unchanged);
    ///   - `from == ANY` → the source is dynamically typed, so the name proves nothing and the only
    ///     honest check left is the VALUE: accept only if `v`'s [`BoxValue`] discriminant matches
    ///     the target's registered prototype ([`ValueTypeInfo::default`]). This is precisely why a
    ///     real prototype is demanded of every type (see that field's docs).
    ///   - otherwise → `None`.
    pub fn convert(&self, v: &BoxValue, from: &str, to: &str) -> Option<BoxValue> {
        if to == ANY_TYPE {
            return Some(v.clone());
        }
        let info = self.get(to)?; // unregistered target → no conversion (fail loud, never silent)
        match &info.coerce {
            Some(f) => f(v),
            None if from == to => Some(v.clone()), // identity type: its own values pass through
            // Dynamically-typed source: judge the VALUE against the target's prototype, since the
            // type NAME (`Any`) carries no information.
            None if from == ANY_TYPE && v.kind() == info.default.kind() => Some(v.clone()),
            None => None,
        }
    }

    /// Whether a wire `from` → `to` may carry a value — the SAME decision [`Self::convert`] makes,
    /// probed with `from`'s registered prototype ([`ValueTypeInfo::default`]).
    ///
    /// One rule, one authority: the editor can never accept a wire the cook would drop, nor refuse
    /// one the cook would carry. `Graph::connect` gates on this, and `resolve_inputs` re-checks it
    /// at cook time — which also covers wires restored from a saved graph, since deserialization
    /// never re-runs `connect`.
    ///
    /// ## The one place a `from == ANY` source is judged differently
    ///
    /// An `Any` output has NO value yet at connect time and its prototype (`Null`) describes
    /// nothing, so probing it would refuse every `Switch`/expression → typed-port wire in existence,
    /// including the `Any → Mat4` ones that work fine today. The honest answer at connect is
    /// "unknown", so the wire is ALLOWED and [`Self::convert`] is the enforcing gate at cook, where
    /// the real value exists: a `Float` riding an `Any` wire into a `PointCloud` port is refused
    /// THERE, and the node is left in `NodeStatus::Error` rather than fed a lie. This is the only
    /// asymmetry between the two, it is deliberate, and it never lets a wrong VALUE through — which
    /// is the property that matters.
    pub fn compatible(&self, from: &str, to: &str) -> bool {
        if from == to || to == ANY_TYPE {
            return true;
        }
        if self.get(to).is_none() {
            return false; // unregistered target: same verdict as `convert`
        }
        if from == ANY_TYPE {
            return true; // value unknown until cook; `convert` decides there
        }
        self.convert(&self.make_default(from), from, to).is_some()
    }
}

/// Register the builtin value types into `types` (see [`builtin_value_types`]).
///
/// Kept as a standalone call for a host that needs the builtin TYPES without the builtin NODES;
/// [`crate::Builtins`] declares the same list as its [`crate::NodeLibrary::value_types`], so
/// `add_library(&Builtins, ..)` already covers the normal path.
pub fn register_builtin_types(types: &mut ValueTypes) {
    for info in builtin_value_types() {
        types.register(info);
    }
}

/// The builtin value types: `Float`, `Int`, `Bool`, `String`, `Vec3`, `Vec4`, `Mat3`, `Mat4`,
/// `Color`, `Any`, `Map`, `Image` — each with its pin visual, its prototype/default value (from the
/// crate-private builtin seed table in `values.rs`) and the exact coercion semantics the old free
/// `convert::convert` kernel had.
pub fn builtin_value_types() -> Vec<ValueTypeInfo> {
    // `seed(ty, visual)` = a builtin type carrying its seed-table default; `.with_coerce(..)`
    // then attaches its kernel. The seed table is the ONLY name-match left, and it is now just a
    // literal table of builtin prototypes — it never decides anything about a HOST type.
    let seed = |ty: &'static str, visual: PinVisual| {
        ValueTypeInfo::new(ty, visual).with_default(default_value(ty))
    };

    vec![
        seed("Float", PinVisual::FLOAT).with_coerce(|v| v.as_f64().map(BoxValue::F64)),
        seed("Int", PinVisual::INT).with_coerce(|v| v.as_i64().map(BoxValue::I64)),
        seed("Bool", PinVisual::BOOL).with_coerce(|v| v.as_bool().map(BoxValue::Bool)),
        // Everything renders as a string; scalars stay unquoted (shared display helper).
        seed("String", PinVisual::STRING)
            .with_coerce(|v| Some(BoxValue::String(boxvalue_to_display_string(Some(v))))),
        seed("Vec3", PinVisual::VEC3).with_coerce(|v| to_vec3(v).map(BoxValue::Vec3)),
        seed("Vec4", PinVisual::VEC3).with_coerce(|v| to_vec4(v).map(BoxValue::Vec4)),
        seed("Color", PinVisual::COLOR).with_coerce(|v| to_vec4(v).map(BoxValue::Color)),
        seed("Mat3", PinVisual::ANY).with_coerce(|v| v.as_mat3().map(BoxValue::Mat3)),
        seed("Mat4", PinVisual::ANY).with_coerce(|v| v.as_mat4().map(BoxValue::Mat4)),
        // `Any` is short-circuited by `convert`/`compatible`; it is registered for its pin visual.
        seed(ANY_TYPE, PinVisual::ANY),
        // Map (bag): identity-only — a non-Object never silently becomes a bag. Teal diamond.
        seed("Map", PinVisual::MAP).with_coerce(|v| v.is_object().then(|| v.clone())),
        // Image: identity-only on a `Buffer` value (RGBA8, see builtins/image.rs) — a scalar never
        // silently becomes an image, but an Image→Image wire forwards the buffer instead of
        // dropping it. Its prototype stays `Null`: an image is ABSENT until a node cooks one.
        seed("Image", PinVisual::IMAGE).with_coerce(|v| v.as_buffer().is_some().then(|| v.clone())),
    ]
}

/// 3-tuple from Vec3, or Vec4/Color dropping the 4th component (alpha), or a length-3 numeric
/// array.
fn to_vec3(v: &BoxValue) -> Option<[f64; 3]> {
    match v {
        BoxValue::Vec3(a) => Some(*a),
        BoxValue::Vec4(a) | BoxValue::Color(a) => Some([a[0], a[1], a[2]]),
        BoxValue::Array(_) => v.as_vec3(),
        _ => None,
    }
}

/// 4-tuple from Vec4/Color (1:1), or Vec3 appending `1.0`, or a length-4 array.
fn to_vec4(v: &BoxValue) -> Option<[f64; 4]> {
    match v {
        BoxValue::Vec4(a) | BoxValue::Color(a) => Some(*a),
        BoxValue::Vec3(a) => Some([a[0], a[1], a[2], 1.0]),
        BoxValue::Array(_) => v.as_vec4(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtins() -> ValueTypes {
        let mut t = ValueTypes::new();
        register_builtin_types(&mut t);
        t
    }

    #[test]
    fn registers_visuals_and_defaults() {
        let t = builtins();
        assert!(t.get("Float").is_some());
        assert_eq!(t.make_default("Int"), BoxValue::I64(0));
        assert!(t.compatible("Int", "Float"));
        assert!(!t.compatible("Mat3", "Vec3"));
    }

    #[test]
    fn same_arity_identity() {
        let t = builtins();
        let v = BoxValue::Vec3([1.0, 2.0, 3.0]);
        assert_eq!(t.convert(&v, "Vec3", "Vec3"), Some(v.clone()));
        let c = BoxValue::Color([0.1, 0.2, 0.3, 0.5]);
        assert_eq!(t.convert(&c, "Color", "Color"), Some(c));
    }

    #[test]
    fn cross_arity_alpha_rules() {
        let t = builtins();
        // 4 -> 3 drops alpha.
        assert_eq!(
            t.convert(&BoxValue::Color([0.1, 0.2, 0.3, 0.9]), "Color", "Vec3"),
            Some(BoxValue::Vec3([0.1, 0.2, 0.3]))
        );
        // 3 -> 4 appends 1.0.
        assert_eq!(
            t.convert(&BoxValue::Vec3([0.1, 0.2, 0.3]), "Vec3", "Vec4"),
            Some(BoxValue::Vec4([0.1, 0.2, 0.3, 1.0]))
        );
        // Color <-> Vec4 1:1.
        assert_eq!(
            t.convert(&BoxValue::Vec4([1.0, 2.0, 3.0, 4.0]), "Vec4", "Color"),
            Some(BoxValue::Color([1.0, 2.0, 3.0, 4.0]))
        );
    }

    #[test]
    fn scalar_widening_and_string() {
        let t = builtins();
        assert_eq!(
            t.convert(&BoxValue::I64(3), "Int", "Float"),
            Some(BoxValue::F64(3.0))
        );
        assert_eq!(
            t.convert(&BoxValue::Bool(true), "Bool", "Int"),
            Some(BoxValue::I64(1))
        );
        assert_eq!(
            t.convert(&BoxValue::F64(2.5), "Float", "String"),
            Some(BoxValue::String("2.5".to_string()))
        );
    }

    #[test]
    fn compatibility_table() {
        let t = builtins();
        assert!(t.compatible("Vec3", "Color"));
        assert!(t.compatible("Color", "Vec3"));
        assert!(t.compatible("Int", "Float"));
        assert!(t.compatible("Any", "Mat4"));
        assert!(!t.compatible("Mat3", "Vec3"));
        assert!(!t.compatible("String", "Vec3"));
        // Map / Image stay identity-only.
        assert!(t.compatible("Map", "Map"));
        assert!(!t.compatible("Float", "Map"));
        assert!(!t.compatible("Float", "Image"));
    }

    /// A host-registered IDENTITY type: its own values flow, and nothing else converts into it.
    /// This is the §0 bug — before the kernel moved into the registry, `convert(v, "Recon")`
    /// hit the `_ => None` arm and dropped the value on every cook.
    #[test]
    fn custom_identity_type_flows_and_rejects_foreigners() {
        let mut t = builtins();
        t.register(ValueTypeInfo::new("Recon", PinVisual::ANY));

        let v = BoxValue::String("recon-handle".into());
        assert_eq!(
            t.convert(&v, "Recon", "Recon"),
            Some(v.clone()),
            "own value must flow"
        );
        assert!(t.compatible("Recon", "Recon"));
        assert!(!t.compatible("Float", "Recon"), "no silent Float→Recon");
        assert!(!t.compatible("Recon", "Float"), "no silent Recon→Float");
        assert!(t.compatible("Recon", "Any"), "Any accepts everything");
    }

    /// A host type's DEFAULT lives on the registered type, so `compatible` probes a REAL
    /// prototype of it.
    ///
    /// Before the default moved into `ValueTypeInfo`, `compatible` probed the hardcoded
    /// name-match `default_value(from)`, which handed every unknown (= every host) type `Null` —
    /// so `Recon → Mesh` was REFUSED even though every real `Recon` value (a buffer) coerces into
    /// `Mesh` perfectly. That is the mirror image of the value-drop bug the registry-owned kernel
    /// fixed, and it is what this test pins down.
    #[test]
    fn host_type_default_drives_compatibility() {
        let buf = || BoxValue::buffer(crate::box_model::DType::U8, vec![1], vec![0u8]);

        let mut t = builtins();
        // An opaque host payload type whose values are buffers — with a REAL prototype.
        t.register(ValueTypeInfo::new("Recon", PinVisual::ANY).with_default(buf()));
        // A host type that accepts anything buffer-shaped.
        t.register(
            ValueTypeInfo::new("Mesh", PinVisual::ANY)
                .with_default(buf())
                .with_coerce(|v| v.as_buffer().is_some().then(|| v.clone())),
        );

        assert!(
            t.compatible("Recon", "Mesh"),
            "a Recon value coerces into Mesh, so the wire must be allowed"
        );
        assert_eq!(
            t.convert(&buf(), "Recon", "Mesh"),
            Some(buf()),
            "and the value really flows"
        );
        // A type whose prototype does NOT fit is still refused — the gate stays honest.
        assert!(!t.compatible("Float", "Mesh"));
        // The registry, not a name-match, is the default authority.
        assert_eq!(t.make_default("Recon"), buf());
        assert_eq!(t.make_default("Float"), BoxValue::F64(0.0));
        assert_eq!(t.make_default("Unregistered"), BoxValue::Null);
        // A port of a host type is seeded from that registered default.
        assert_eq!(t.port_def("recon", "Recon").default, buf());
    }

    /// Locks the OVERRIDE semantics of a value-type name collision: the LAST registration wins,
    /// kernel included. Two libraries both declaring `"Image"` therefore silently swap which values
    /// a wire of that type carries — which is exactly why `register_from` now warns about it.
    #[test]
    fn re_registered_value_type_keeps_the_last_info() {
        let mut t = builtins();
        // The builtin `Image` coerces only buffers. Re-register the name as an identity type with a
        // scalar prototype — a plausible "my host's Image" collision.
        t.register(ValueTypeInfo::new("Image", PinVisual::ANY).with_default(BoxValue::F64(1.0)));

        let info = t.get("Image").expect("'Image' is registered");
        assert!(
            info.coerce.is_none(),
            "the LAST registration's coercion kernel wins"
        );
        assert_eq!(
            t.make_default("Image"),
            BoxValue::F64(1.0),
            "and its prototype default"
        );
        assert_eq!(
            t.names().filter(|n| *n == "Image").count(),
            1,
            "replaced, not appended"
        );
        // The swap is observable in behaviour: the builtin kernel rejected a Float, this one is an
        // IDENTITY type whose prototype is now a Float — so an `Image`-typed Float flows.
        assert_eq!(
            t.convert(&BoxValue::F64(1.0), "Image", "Image"),
            Some(BoxValue::F64(1.0))
        );
    }

    /// An unregistered target type converts to nothing (fail loud rather than silently forward).
    #[test]
    fn unregistered_target_never_converts() {
        let t = builtins();
        assert_eq!(t.convert(&BoxValue::F64(1.0), "Float", "Nonsense"), None);
        assert!(!t.compatible("Float", "Nonsense"));
    }

    /// THE BLOCKER: an identity type must not accept a foreign value just because the wire is
    /// dynamically typed.
    ///
    /// `convert` used to take only the TARGET, so an identity type (`coerce: None`) returned
    /// `Some(v)` for whatever it was handed. Wire an `Any` output (a `Switch`, an expression, the
    /// `Null` node) into a custom `PointCloud` input and the cook handed that node a `Float` wearing
    /// its own type's name — silent wrong data. Now the VALUE is judged against the target's
    /// registered prototype whenever the source type cannot vouch for it.
    #[test]
    fn any_source_into_identity_port_is_judged_by_value() {
        let cloud = || BoxValue::buffer(crate::box_model::DType::F32, vec![3], vec![0u8; 12]);

        let mut t = builtins();
        t.register(ValueTypeInfo::new("PointCloud", PinVisual::ANY).with_default(cloud()));

        // A Float riding an `Any` wire is NOT a point cloud — refused.
        assert_eq!(
            t.convert(&BoxValue::F64(3.0), ANY_TYPE, "PointCloud"),
            None,
            "a Float on an Any wire must never be handed to a PointCloud port"
        );
        // A value of the right SHAPE on the same dynamically-typed wire is accepted.
        assert_eq!(
            t.convert(&cloud(), ANY_TYPE, "PointCloud"),
            Some(cloud()),
            "a real point cloud on an Any wire must flow"
        );
        // The type's own values always flow.
        assert_eq!(
            t.convert(&cloud(), "PointCloud", "PointCloud"),
            Some(cloud())
        );
        // A DECLARED Float source is refused outright — at connect (compatible) and at cook.
        assert!(
            !t.compatible("Float", "PointCloud"),
            "Float -> PointCloud refused at connect"
        );
        assert_eq!(
            t.convert(&BoxValue::F64(3.0), "Float", "PointCloud"),
            None,
            "Float -> PointCloud refused at cook"
        );
    }

    /// `compatible` and `convert` are ONE rule: whatever the editor allows, the cook carries, and
    /// vice versa. The single deliberate asymmetry is an `Any` SOURCE, whose value does not exist
    /// yet at connect time — the wire is allowed and `convert` is the enforcing gate at cook.
    #[test]
    fn compatible_agrees_with_convert() {
        let t = builtins();
        let cases = [
            ("Int", "Float", BoxValue::I64(1)),
            ("Float", "Map", BoxValue::F64(1.0)),
            ("Vec3", "Color", BoxValue::Vec3([0.0; 3])),
            ("Mat3", "Vec3", BoxValue::Mat3([0.0; 9])),
            ("String", "Vec3", BoxValue::String("x".into())),
        ];
        for (from, to, v) in cases {
            assert_eq!(
                t.compatible(from, to),
                t.convert(&v, from, to).is_some(),
                "connect and cook disagree on {from} -> {to}"
            );
        }
        // The documented asymmetry, stated explicitly so a future change has to face it.
        assert!(
            t.compatible(ANY_TYPE, "Mat4"),
            "an Any source stays connectable…"
        );
        assert_eq!(
            t.convert(&BoxValue::F64(1.0), ANY_TYPE, "Mat4"),
            None,
            "…but the WRONG VALUE is still refused at cook"
        );
    }
}
