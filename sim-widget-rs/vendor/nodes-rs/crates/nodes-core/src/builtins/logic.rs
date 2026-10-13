use crate::prelude::*;

/// Comparison + branch nodes.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    vec![
        crate::node!("Compare", "Logic",
            [float("a", 0.0), float("b", 0.0)]
            => [bool("gt", false), bool("eq", false), bool("lt", false)],
            |ctx| {
                let a = ctx.node.in_f64("a");
                let b = ctx.node.in_f64("b");
                ctx.node.set_out("gt", bv(a > b));
                ctx.node.set_out("eq", bv((a - b).abs() < 1e-10));
                ctx.node.set_out("lt", bv(a < b));
                Ok(())
            }
        ),
        crate::node!("Switch", "Logic",
            [bool("cond", false), any("true_val"), any("false_val")] => [any("out")],
            |ctx| {
                let out = if ctx.node.in_bool("cond") {
                    ctx.node.cook_value("true_val").unwrap_or(BoxValue::Null)
                } else {
                    ctx.node.cook_value("false_val").unwrap_or(BoxValue::Null)
                };
                ctx.node.set_out("out", out);
                Ok(())
            }
        ),
    ]
}
