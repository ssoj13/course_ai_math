//! The graph + type-tables trio that every consumer of a graph needs together.
//!
//! Reading or writing a graph is never just topology: interpreting ANY port needs the node-type
//! table ([`NodeTypeRegistry`]) and the port/value type table ([`ValueTypes`]) alongside the
//! [`Subnet`]. Three crates (`nodes-editor`, `nodes-chrome`, `nodes-view`) each used to restate
//! that trio as their own local struct; they all share these types now so the grouping — and its
//! reasoning — lives in exactly one place.

use crate::{NodeTypeRegistry, Subnet, ValueTypes};

/// A read-only graph plus the two type tables needed to interpret it: the topology (`subnet`),
/// the node-type table (`registry`), and the port/value type table (`value_types`).
///
/// Grouped because these three always travel together — resolving any port's type or value needs
/// all three, and no caller ever varies one independently, so passing them positionally only
/// added noise.
///
/// Named `GraphRead` (not `GraphView`) because [`crate::cook::GraphView`] is an unrelated,
/// already root-exported type: the cook-time view of a node's NEIGHBOURS (`nodes` + `index`).
/// `GraphRead`/[`GraphEdit`] also reads as the read-vs-write pair it is.
#[derive(Clone, Copy)]
pub struct GraphRead<'a> {
    pub subnet: &'a Subnet,
    pub registry: &'a NodeTypeRegistry,
    pub value_types: &'a ValueTypes,
}

/// The same trio, but with the graph open for EDITING (spawning/rewiring nodes).
///
/// Grouped for the same reason as [`GraphRead`]: an edit cannot use one without the others — a
/// spawn looks its type up in `registry`, types its ports against `value_types`, and writes the
/// result into `subnet`. Not `Copy`: it holds a unique borrow of the graph.
pub struct GraphEdit<'a> {
    pub subnet: &'a mut Subnet,
    pub registry: &'a NodeTypeRegistry,
    pub value_types: &'a ValueTypes,
}

impl<'a> GraphEdit<'a> {
    /// Reborrow as a read-only [`GraphRead`] (so edit code can call read-only helpers).
    pub fn view(&self) -> GraphRead<'_> {
        GraphRead {
            subnet: self.subnet,
            registry: self.registry,
            value_types: self.value_types,
        }
    }
}
