//! Map bag nodes: MapGet and MapInsert.
//!
//! These operate on `BoxValue::Object` (the "bag" type, port type `"Map"`).
//! The key to read/write is a regular `key` input (String, default `""`) —
//! connectable like any other input, mirroring how BusRead/BusWrite treat `slot`.
//!
//! ## MapGet
//! Reads the `key` input from the incoming Map; outputs the value at that key
//! (or Null if the key is absent or the map input is not an Object).
//!
//! ## MapInsert
//! Clones the incoming Map (or starts from an empty Object if the input is not
//! an Object), inserts `key` → input "value", and outputs the result.

use crate::prelude::*;

pub fn types() -> Vec<Arc<dyn NodeType>> {
    let get = fn_node(
        NodeDef::new("MapGet", "Map")
            .with_display("Map Get")
            .with_inputs(vec![map("map"), string("key")])
            .with_outputs(vec![any("value")]),
        |ctx| {
            let key = ctx.node.in_str("key");
            let result = ctx
                .node
                .cook_value("map")
                .as_ref()
                .and_then(|v| v.as_object())
                .and_then(|obj| obj.get(&key).cloned())
                .unwrap_or(BoxValue::Null);
            ctx.node.set_out("value", result);
            Ok(())
        },
    );

    let insert = fn_node(
        NodeDef::new("MapInsert", "Map")
            .with_display("Map Insert")
            .with_inputs(vec![map("map"), any("value"), string("key")])
            .with_outputs(vec![map("out")]),
        |ctx| {
            let key = ctx.node.in_str("key");
            let val = ctx.node.cook_value("value").unwrap_or(BoxValue::Null);

            // Start from the incoming map, or an empty Object if not a valid Map.
            // Kept as a `BoxValue` rather than unpacked into an owned `BTreeMap`:
            // the interior is shared, so `as_object_mut` copies only when the
            // incoming map is still referenced elsewhere.
            let mut out = ctx
                .node
                .cook_value("map")
                .filter(BoxValue::is_object)
                .unwrap_or_else(BoxValue::object);
            if let Some(entries) = out.as_object_mut() {
                entries.insert(key, val);
            }
            ctx.node.set_out("out", out);
            Ok(())
        },
    );

    vec![get, insert]
}
