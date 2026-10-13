//! Subnet node family: Subnet, SubnetInput, SubnetOutput.
//!
//! ## Data flow (pre-stage model)
//!
//! The cook driver (`cook::run_subnet_prestage`) runs BEFORE the node's own cook:
//!   1. Packs the Subnet's resolved inputs into a `BoxValue::Object` bag.
//!   2. Seeds SubnetInput's "bag" output with it.
//!   3. Topo-cooks all other children (recursing for nested Subnets).
//!   4. Reads SubnetOutput's "bag" input (via its source edge) → stashes in
//!      `node.cook_inputs["__subnet_out__"]`.
//!
//! Then the Subnet's own cook (`subnet_emit`) runs and exposes that stashed bag
//! as the "out" output.  SubnetInput and SubnetOutput cooks are no-ops; they are
//! seeded / read by the driver, not by their own closures.

use crate::box_model::BoxValue;
use crate::prelude::*;

/// The Subnet family: the compound itself plus its two boundary helpers.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    // SubnetInput: no inputs; one Map output "bag".
    // The driver seeds "bag" with the parent Subnet's resolved inputs packed as an Object.
    // Cook is a no-op — seeding is done externally by run_subnet_prestage.
    let input = fn_node(
        NodeDef::new("SubnetInput", "Subnet")
            .with_display("Subnet Input")
            .with_outputs(vec![map("bag")]),
        |_ctx| Ok(()),
    );

    // SubnetOutput: one Map input "bag"; no outputs.
    // The driver reads "bag" (via its source edge) to extract the inner subgraph result.
    // Cook is a no-op — the driver reads the value directly.
    let output = fn_node(
        NodeDef::new("SubnetOutput", "Subnet")
            .with_display("Subnet Output")
            .with_inputs(vec![map("bag")]),
        |_ctx| Ok(()),
    );

    // Subnet: one Any input "in", one Map output "out".
    // The pre-stage (run_subnet_prestage) packs inputs → bag, cooks children, then stashes the
    // result in `compute["__subnet_out__"]`; `subnet_emit` forwards that stash to output "out".
    // Category "Subnet" so it sits with its SubnetInput/SubnetOutput helpers in the TAB menu.
    let subnet = crate::node!("Subnet", "Subnet",
        [any("in")] => [map("out")],
        subnet_emit
    );

    vec![input, output, subnet]
}

/// Construct a `Subnet` compound at `view_pos`, pre-populated with a wired
/// `SubnetInput` → `SubnetOutput` pass-through pair INSIDE it (Houdini-style), so a fresh
/// subnet forwards its packed inputs to its output out of the box instead of being an empty
/// shell that cooks to an empty bag and warns "has children but no SubnetInput/SubnetOutput".
///
/// This is the Subnet family's constructor: the knowledge of which helper nodes a subnet needs
/// and how they're wired lives here, NOT in the generic [`crate::Graph::create_node`] primitive — that
/// stays able to make a *bare* subnet (paste, scripting, low-level tests build the interior by
/// hand). The two helpers are parented to the returned Subnet and placed at fixed positions in
/// the compound's OWN scope (`view_pos` is a parent-scope coordinate, irrelevant inside).
pub fn create_subnet(
    graph: &mut crate::Graph,
    view_pos: [f32; 2],
    registry: &NodeTypeRegistry,
    value_types: &crate::ValueTypes,
) -> crate::CoreResult<crate::NodeId> {
    let subnet_id = graph.create_node("Subnet", view_pos, registry, value_types)?;
    let in_id = graph.create_node("SubnetInput", [0.0, 0.0], registry, value_types)?;
    let out_id = graph.create_node("SubnetOutput", [420.0, 0.0], registry, value_types)?;
    graph.set_parent(in_id, Some(subnet_id));
    graph.set_parent(out_id, Some(subnet_id));
    // SubnetInput "bag" (output 0) → SubnetOutput "bag" (input 0): a straight pass-through.
    if let Err(e) = graph.connect_idx(in_id, 0, out_id, 0, registry, value_types) {
        log::warn!("subnet I/O auto-wire failed: {e}");
    }
    Ok(subnet_id)
}

/// Cook closure for Subnet: forward the pre-stage output bag to "out".
///
/// The bag was stashed by `run_subnet_prestage` as `cook_inputs["__subnet_out__"]`.
/// If the node has no children (prestage no-op) or the key is absent, emit an
/// empty Object so downstream always sees a valid Map.
fn subnet_emit(ctx: &mut crate::CookCtx) -> crate::CookResult<()> {
    let bag = ctx
        .node
        .cook_value("__subnet_out__")
        .filter(|v| v.is_object())
        .unwrap_or_else(BoxValue::object);
    ctx.node.set_out("out", bag);
    Ok(())
}
