//! `Debug` node: one input + matching output per inspector widget kind, for exercising/visually
//! testing every control the type-driven inspector can render (checkbox, int/float drag, slider,
//! line edit, combo, radio, vec3/vec4, color, mat3/mat4) and every pin shape
//! (circle/square/triangle/diamond) at once. Pure passthrough cook — each output echoes its
//! matching input.
//!
//! `Debug2` is the same node PLUS an `image`/`image_out` typed passthrough (13 ports/side): a
//! DELIBERATE stress fixture for debugging the node "look" (many ports + an image port). It is
//! seeded into the default scene so the rendering of a busy node can be eyeballed on launch.
//! Outputs are named with an `_out` suffix purely for legibility — inputs and outputs are separate
//! namespaces, so a same-named pair would be legal too.

use crate::prelude::*;

/// The two inspector/render stress fixtures.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    let debug = fn_node(
        NodeDef::new("Debug", "Debug")
            .with_inputs(debug_ports("", false))
            .with_outputs(debug_ports("_out", false))
            .with_schema(debug_schema()),
        |ctx| echo_cook(ctx.node),
    );

    let debug2 = fn_node(
        NodeDef::new("Debug2", "Debug2")
            .with_inputs(debug_ports("", true))
            .with_outputs(debug_ports("_out", true))
            .with_schema(debug_schema()),
        |ctx| echo_cook(ctx.node),
    );

    vec![debug, debug2]
}

/// One port per inspector widget kind, each name carrying `suffix` (`""` for the input side,
/// `"_out"` for the output side — the two sides are built from the SAME list, so a Debug node can
/// never grow an input without the matching output its echo cook writes). `image` adds the typed
/// Image port that distinguishes `Debug2`.
fn debug_ports(suffix: &str, image: bool) -> Vec<PortDef> {
    let n = |base: &str| format!("{base}{suffix}");
    let mut ports = vec![
        bool(&n("b"), false),
        int(&n("i"), 0),
        float(&n("f"), 0.0),
        float(&n("slider"), 5.0),
        string(&n("text")),
        string(&n("combo")),
        string(&n("radio")),
        vec3(&n("vec3")),
        PortDef::typed(&n("vec4"), "Vec4"),
        color(&n("color")),
        PortDef::typed(&n("mat3"), "Mat3"),
        PortDef::typed(&n("mat4"), "Mat4"),
    ];
    if image {
        ports.push(PortDef::typed(&n("image"), "Image"));
    }
    ports
}

/// The generic Debug/Debug2 cook: echo every input to its `<name>_out` output (pure passthrough).
/// Names are collected first so the immutable read doesn't overlap the `set_out` writes.
fn echo_cook(node: &mut crate::Node) -> Result<(), crate::CookError> {
    let names: Vec<String> = node.input_names().iter().map(|s| s.to_string()).collect();
    for name in names {
        let v = node.cook_value(&name).unwrap_or(BoxValue::Null);
        node.set_out(&format!("{name}_out"), v);
    }
    Ok(())
}

/// The shared inspector-widget schema for `Debug` and `Debug2` (one entry per widget kind). Image
/// ports are omitted deliberately -- Image has no inline widget, it's a plain typed wire.
fn debug_schema() -> Vec<AttrDef> {
    vec![
        AttrDef::new("b", "Bool", bv(false)).description("Checkbox."),
        AttrDef::new("i", "Int", iv(0)).description("Int drag value."),
        AttrDef::new("f", "Float", fv(0.0)).description("Float drag value."),
        AttrDef::new("slider", "Float", fv(5.0))
            .range(0.0, 10.0)
            .description("Float with a range — renders as a slider."),
        AttrDef::new("text", "String", sv("")).description("Free-form line edit."),
        AttrDef::new("combo", "String", sv("Alpha"))
            .choices(["Alpha", "Beta", "Gamma", "Delta", "Epsilon"])
            .description(">4 choices — renders as a combo box."),
        AttrDef::new("radio", "String", sv("A"))
            .choices(["A", "B", "C"])
            .description("≤4 choices — renders as a radio group."),
        AttrDef::new("vec3", "Vec3", vec3v([0.0, 0.0, 0.0])).description("3x DragValue row."),
        AttrDef::new("vec4", "Vec4", BoxValue::Vec4([0.0; 4])).description("4x DragValue row."),
        AttrDef::new("color", "Color", cv([1.0, 1.0, 1.0, 1.0])).description("Color picker."),
        AttrDef::new("mat3", "Mat3", BoxValue::Mat3(IDENTITY3)).description("3x3 DragValue grid."),
        AttrDef::new("mat4", "Mat4", BoxValue::Mat4(IDENTITY4)).description("4x4 DragValue grid."),
    ]
}

/// Row-major 3x3 identity (the `Mat3` prototype, for the schema's informational default).
const IDENTITY3: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
/// Row-major 4x4 identity (the `Mat4` prototype, for the schema's informational default).
const IDENTITY4: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];
