use crate::chrome::FRAME_NODE_TYPE;
use crate::prelude::*;

/// The `Frame` backdrop: a bare, socket-less, user-resizable box. Purely visual (a translucent
/// group background); its cook is an inert passthrough.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    vec![fn_node(
        NodeDef::new(FRAME_NODE_TYPE, "Layout").with_display("Frame"),
        passthrough_cook,
    )]
}
