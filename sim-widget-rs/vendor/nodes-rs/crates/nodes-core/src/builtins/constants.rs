use crate::prelude::*;

/// Constant nodes: one authored literal, forwarded to the output.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    vec![
        crate::node!("Float", "Constants",
            [float("value", 0.0)] => [float("out", 0.0)],
            passthrough_cook
        ),
        crate::node!("Int", "Constants",
            [int("value", 0)] => [int("out", 0)],
            passthrough_cook
        ),
        crate::node!("Bool", "Constants",
            [bool("value", false)] => [bool("out", false)],
            passthrough_cook
        ),
        crate::node!("String", "Constants",
            [string("value")] => [string("out")],
            passthrough_cook
        ),
    ]
}
