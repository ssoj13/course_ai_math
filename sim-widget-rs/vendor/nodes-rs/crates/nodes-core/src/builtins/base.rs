use crate::prelude::*;

/// The closed set of target type names the `Convert` node (and its `to` choice
/// control) supports. Mirrors the conversion kernel's accepted `to` arguments.
pub const CONVERT_TYPES: &[&str] = &[
    "Float", "Int", "Bool", "String", "Vec3", "Vec4", "Mat3", "Mat4", "Color",
];

/// Base nodes: the two passthroughs and the explicit type converter.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    // Explicit type conversion node (uses the centralized conversion kernel — the value-type
    // registry). The output type starts as `Any`; the `to` attr names the target type and is a
    // first-class string attr (unified model) so the inspector can drive it with a choice control
    // and it round-trips like any other attr.
    let convert = fn_node(
        NodeDef::new("Convert", "Base")
            .with_inputs(vec![
                any("value"),
                PortDef::new("to", "String", sv("Float")),
            ])
            .with_outputs(vec![any("out")])
            .with_schema(vec![
                AttrDef::new("value", "Any", BoxValue::Null)
                    .description("Value to convert. Connect a source or leave null."),
                AttrDef::new("to", "String", sv("Float"))
                    .choices(CONVERT_TYPES.iter().copied())
                    .description("Target type for the conversion."),
            ]),
        |ctx| {
            // Read `to` via cook_value so a connection (or the stored literal) is seen.
            let to = ctx.node.in_str("to");
            let to = if to.is_empty() {
                "Float".to_string()
            } else {
                to
            };
            let input = ctx.node.cook_value("value").unwrap_or(BoxValue::Null);
            // The value-type registry owns the coercion kernel, so an explicit Convert obeys the
            // exact same rule a wire does — including for host-registered custom types.
            //
            // The `value` port is declared `Any`, so that IS the honest source type: for an
            // identity-typed target the kernel then judges the VALUE against that type's prototype,
            // exactly as a wire from an `Any` output would be judged (see `ValueTypes::convert`).
            // A conversion the registry refuses is an ERROR, not a `Null` output: emitting `Null`
            // would look like "absent data" to every downstream node and hide the real reason.
            let out = ctx.types.convert(&input, ANY_TYPE, &to).ok_or_else(|| {
                CookError::Custom(format!(
                    "cannot convert {:?} to '{to}': no coercion into that type accepts this value",
                    input.kind()
                ))
            })?;
            ctx.node.set_out("out", out);
            Ok(())
        },
    );

    vec![
        crate::node!("Null", "Base", [any("in")] => [any("out")], passthrough_cook),
        crate::node!("Display", "Base", [any("value")] => [any("out")], passthrough_cook),
        convert,
    ]
}
