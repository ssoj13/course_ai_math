//! Thin facade over [`crate::cook::cook`] / [`crate::cook::cook_dirty`].

use crate::cook::{cook, cook_dirty};
use crate::graph_container::Graph;
use crate::registry::NodeTypeRegistry;
use crate::value_types::ValueTypes;

/// Holds registry + value-type context for repeated cook calls.
pub struct Executor<'a> {
    pub registry: &'a NodeTypeRegistry,
    pub value_types: &'a ValueTypes,
}

impl<'a> Executor<'a> {
    pub fn new(registry: &'a NodeTypeRegistry, value_types: &'a ValueTypes) -> Self {
        Self {
            registry,
            value_types,
        }
    }

    pub fn cook_all(&self, graph: &mut Graph) {
        cook(graph, self.registry, self.value_types);
    }

    pub fn cook_dirty(&self, graph: &mut Graph) {
        cook_dirty(graph, self.registry, self.value_types);
    }
}
