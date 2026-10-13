//! **Builtin** prototype values, keyed by type name — the seed table
//! [`crate::value_types::register_builtin_types`] hands to each builtin [`crate::ValueTypeInfo`].
//!
//! This is deliberately NOT the type-default authority anymore. A type's default now lives ON the
//! registered type (`ValueTypeInfo::default`), and [`crate::ValueTypes::make_default`] reads it
//! from the registry. While this name-match WAS the authority, a host-registered type silently got
//! `Null` as its default — which made `ValueTypes::compatible` probe every coercion kernel with
//! `Null` and refuse wires the cook would have carried fine (the mirror image of the value-drop bug
//! the registry-owned kernel fixed). Keep this table for builtins only; never consult it for a type
//! name that did not come from this crate.

use crate::box_model::BoxValue;

/// Row-major 3x3 identity.
const IDENTITY3: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
/// Row-major 4x4 identity.
const IDENTITY4: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// Prototype value for a BUILTIN type name. Unknown / `"Any"` / `"Image"` → [`BoxValue::Null`].
/// Crate-private on purpose (see the module docs): outside this crate the authority is
/// [`crate::ValueTypes::make_default`].
pub(crate) fn builtin_default(ty: &str) -> BoxValue {
    match ty {
        "Float" => BoxValue::F64(0.0),
        "Int" => BoxValue::I64(0),
        "Bool" => BoxValue::Bool(false),
        "String" => BoxValue::String(String::new()),
        "Vec3" => BoxValue::Vec3([0.0; 3]),
        "Vec4" => BoxValue::Vec4([0.0; 4]),
        "Mat3" => BoxValue::Mat3(IDENTITY3),
        "Mat4" => BoxValue::Mat4(IDENTITY4),
        "Color" => BoxValue::Color([0.0, 0.0, 0.0, 1.0]),
        // Map is an Object (BTreeMap<String,BoxValue>); empty by default.
        "Map" => BoxValue::object(),
        _ => BoxValue::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_per_type() {
        assert_eq!(builtin_default("Float"), BoxValue::F64(0.0));
        assert_eq!(builtin_default("Int"), BoxValue::I64(0));
        assert_eq!(builtin_default("Bool"), BoxValue::Bool(false));
        assert_eq!(builtin_default("Vec3"), BoxValue::Vec3([0.0; 3]));
        assert_eq!(
            builtin_default("Color"),
            BoxValue::Color([0.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(builtin_default("Mat4"), BoxValue::Mat4(IDENTITY4));
        assert_eq!(builtin_default("Any"), BoxValue::Null);
        assert_eq!(builtin_default("Nonsense"), BoxValue::Null);
    }
}
