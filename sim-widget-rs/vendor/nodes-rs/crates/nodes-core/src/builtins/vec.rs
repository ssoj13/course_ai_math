use crate::prelude::*;

/// Vector pack/unpack nodes.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    vec![
        crate::node!("MakeVec3", "Vector",
            [float("x", 0.0), float("y", 0.0), float("z", 0.0)] => [vec3("vec")],
            |ctx| {
                let v = vec3v([
                    ctx.node.in_f64("x"),
                    ctx.node.in_f64("y"),
                    ctx.node.in_f64("z"),
                ]);
                ctx.node.set_out("vec", v);
                Ok(())
            }
        ),
        crate::node!("SplitVec3", "Vector",
            [vec3("vec")] => [float("x", 0.0), float("y", 0.0), float("z", 0.0)],
            |ctx| {
                let v = ctx.node.in_vec3("vec");
                ctx.node.set_out("x", fv(v[0]));
                ctx.node.set_out("y", fv(v[1]));
                ctx.node.set_out("z", fv(v[2]));
                Ok(())
            }
        ),
    ]
}
