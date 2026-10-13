use crate::prelude::*;

/// Colour pack/unpack nodes.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    vec![
        crate::node!("MakeColor", "Color",
            [float("r", 0.0), float("g", 0.0), float("b", 0.0), float("a", 1.0)]
            => [color("color")],
            |ctx| {
                let rgba = cv([
                    ctx.node.in_f64("r"),
                    ctx.node.in_f64("g"),
                    ctx.node.in_f64("b"),
                    ctx.node.in_f64("a"),
                ]);
                ctx.node.set_out("color", rgba);
                Ok(())
            }
        ),
        crate::node!("SplitColor", "Color",
            [color("color")]
            => [float("r", 0.0), float("g", 0.0), float("b", 0.0), float("a", 1.0)],
            |ctx| {
                let c = ctx.node.in_color("color");
                ctx.node.set_out("r", fv(c[0]));
                ctx.node.set_out("g", fv(c[1]));
                ctx.node.set_out("b", fv(c[2]));
                ctx.node.set_out("a", fv(c[3]));
                Ok(())
            }
        ),
    ]
}
