use crate::prelude::*;

/// Scalar math nodes. `Add` and `Clamp` carry inspector schema (tooltips / a slider range), so
/// they build their [`NodeDef`] explicitly instead of going through the terse `node!` macro.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    let add = fn_node(
        NodeDef::new("Add", "Math")
            .with_inputs(vec![float("a", 0.0), float("b", 0.0)])
            .with_outputs(vec![float("result", 0.0)])
            .with_schema(vec![
                AttrDef::new("a", "Float", fv(0.0)).description("First addend."),
                AttrDef::new("b", "Float", fv(0.0)).description("Second addend."),
            ]),
        |ctx| {
            set_float_output(
                ctx.node,
                "result",
                ctx.node.in_f64("a") + ctx.node.in_f64("b"),
            );
            Ok(())
        },
    );

    // Clamp `value` into `[lo, hi]`. The `value` param carries a soft 0..1 range in its schema so
    // the inspector renders it with a Slider (range is a UI hint only; out-of-range literals still
    // cook correctly).
    let clamp = fn_node(
        NodeDef::new("Clamp", "Math")
            .with_inputs(vec![
                float("value", 0.5),
                float("lo", 0.0),
                float("hi", 1.0),
            ])
            .with_outputs(vec![float("result", 0.0)])
            .with_schema(vec![
                AttrDef::new("value", "Float", fv(0.5))
                    .range(0.0, 1.0)
                    .description("Input value (slider hint: 0..1)."),
                AttrDef::new("lo", "Float", fv(0.0)).description("Lower bound."),
                AttrDef::new("hi", "Float", fv(1.0)).description("Upper bound."),
            ]),
        |ctx| {
            let v = ctx.node.in_f64("value");
            let lo = ctx.node.in_f64("lo");
            let hi = ctx.node.in_f64("hi");
            let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
            set_float_output(ctx.node, "result", v.clamp(lo, hi));
            Ok(())
        },
    );

    vec![
        add,
        clamp,
        crate::node!("Sub", "Math",
            [float("a", 0.0), float("b", 0.0)] => [float("result", 0.0)],
            |ctx| {
                set_float_output(ctx.node, "result", ctx.node.in_f64("a") - ctx.node.in_f64("b"));
                Ok(())
            }
        ),
        crate::node!("Mul", "Math",
            [float("a", 0.0), float("b", 1.0)] => [float("result", 0.0)],
            |ctx| {
                set_float_output(ctx.node, "result", ctx.node.in_f64("a") * ctx.node.in_f64("b"));
                Ok(())
            }
        ),
        crate::node!("Div", "Math",
            [float("a", 0.0), float("b", 1.0)] => [float("result", 0.0)],
            |ctx| {
                let b = ctx.node.in_f64("b");
                if b.abs() < 1e-12 {
                    Err(CookError::Math("division by zero".into()))
                } else {
                    set_float_output(ctx.node, "result", ctx.node.in_f64("a") / b);
                    Ok(())
                }
            }
        ),
        crate::node!("Negate", "Math",
            [float("value", 0.0)] => [float("result", 0.0)],
            |ctx| {
                set_float_output(ctx.node, "result", -ctx.node.in_f64("value"));
                Ok(())
            }
        ),
        crate::node!("Abs", "Math",
            [float("value", 0.0)] => [float("result", 0.0)],
            |ctx| {
                set_float_output(ctx.node, "result", ctx.node.in_f64("value").abs());
                Ok(())
            }
        ),
        crate::node!("Sin", "Math",
            [float("angle", 0.0)] => [float("result", 0.0)],
            |ctx| {
                set_float_output(ctx.node, "result", ctx.node.in_f64("angle").sin());
                Ok(())
            }
        ),
        crate::node!("Cos", "Math",
            [float("angle", 0.0)] => [float("result", 0.0)],
            |ctx| {
                set_float_output(ctx.node, "result", ctx.node.in_f64("angle").cos());
                Ok(())
            }
        ),
    ]
}
