use crate::prelude::*;

/// String nodes.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    vec![
        crate::node!("Concat", "String",
            [string("a"), string("b")] => [string("result")],
            |ctx| {
                let result = format!("{}{}", ctx.node.in_str("a"), ctx.node.in_str("b"));
                ctx.node.set_out("result", sv(result));
                Ok(())
            }
        ),
        crate::node!("StringLen", "String",
            [string("text")] => [int("length", 0)],
            |ctx| {
                let len = ctx.node.in_str("text").len() as i64;
                ctx.node.set_out("length", iv(len));
                Ok(())
            }
        ),
    ]
}
