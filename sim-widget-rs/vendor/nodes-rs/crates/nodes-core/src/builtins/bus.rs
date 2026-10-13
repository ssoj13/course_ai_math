//! `BusWrite` / `BusRead` — ORDINARY node types that read and write the graph's `DataBus`
//! through `ctx.bus`.
//!
//! They used to be **special-cased inside the cook driver** (which held `&mut Graph` and so could
//! reach the bus, while a cook closure could not) — and the "bus readers run last" ordering keyed
//! off the literal type name `"BusRead"`. Both were dead ends for host node types: nobody outside
//! this crate could write a node that touches the bus, and nobody could opt into the late pass. Now
//! the bus is on the [`crate::CookCtx`] and the ordering is a declared
//! [`CookStage::Late`] on the TYPE ([`crate::NodeType::stage`]) — a host bus-reader is just
//! another node type.
//!
//! Semantics: `BusWrite` publishes its resolved `value` into slot `slot` during its own cook (so
//! only nodes cooked LATER observe it), and `BusRead` — deferred to the late stage — reads slot
//! `slot` into its output. The slot name is a regular `slot` input (String, default `"data"`),
//! connectable like any other input.

use crate::prelude::*;

pub fn types() -> Vec<Arc<dyn NodeType>> {
    let write = fn_node(
        NodeDef::new("BusWrite", "Base")
            .with_display("Bus Write")
            .with_inputs(vec![
                any("value"),
                PortDef::new("slot", "String", sv("data")),
            ]),
        |ctx| {
            let slot = ctx.node.in_str("slot");
            // The FIRST declared input ("value") is the payload; `slot` names the target.
            // NO value (a hand-built / older-JSON node whose `value` attr is absent) => NO write:
            // writing `Null` would DESTROY whatever another node had already published to that
            // slot. A bus write only ever publishes a value the node actually has.
            let value = ctx
                .node
                .input_names()
                .first()
                .and_then(|name| ctx.node.cook_value(name));
            match value {
                Some(v) => ctx.bus.set(slot, v),
                None => log::warn!(
                    "BusWrite '{}': no 'value' input to publish — slot '{slot}' left untouched",
                    ctx.node.name()
                ),
            }
            Ok(())
        },
    );

    // Deferred (`Late`): must observe every `BusWrite` of the main pass, in every scope.
    let read = fn_node_staged(
        NodeDef::new("BusRead", "Base")
            .with_display("Bus Read")
            .with_inputs(vec![PortDef::new("slot", "String", sv("data"))])
            .with_outputs(vec![any("value")]),
        CookStage::Late,
        Vec::new(),
        |ctx| {
            let slot = ctx.node.in_str("slot");
            let value = ctx.bus.get(&slot).cloned().unwrap_or(BoxValue::Null);
            let outputs = ctx.node.output_names();
            if let Some(out) = outputs.first() {
                ctx.node.set_out(out, value);
            }
            Ok(())
        },
    );

    vec![write, read]
}
