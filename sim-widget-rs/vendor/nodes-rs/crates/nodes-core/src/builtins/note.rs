use crate::chrome::NOTE_NODE_TYPE;
use crate::prelude::*;

/// The sticky-note annotation type. Like `Frame` it is a bare passthrough with no sockets — its
/// purpose is purely visual (a resizable board comment). The free-text body lives in
/// `Node::note_text` (a plain string field, generically serialized); the editor renders + edits it.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    vec![fn_node(
        NodeDef::new(NOTE_NODE_TYPE, "Layout").with_display("Note"),
        passthrough_cook,
    )]
}
