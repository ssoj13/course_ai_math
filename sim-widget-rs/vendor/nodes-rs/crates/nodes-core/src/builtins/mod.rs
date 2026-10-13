//! The builtin node library.
//!
//! Every builtin is an ordinary [`NodeType`] and the whole set is an ordinary [`NodeLibrary`]
//! ([`Builtins`]) — there is NO privileged in-crate registration path. A host adds the stock nodes
//! exactly the way it adds an external pack:
//!
//! ```
//! use nodes_core::{Builtins, NodeTypeRegistry, ValueTypes};
//! let mut registry = NodeTypeRegistry::new();
//! let mut value_types = ValueTypes::new();
//! registry.add_library(&Builtins, &mut value_types);   // == register_builtins(..)
//! assert!(registry.get("Add").is_some());
//! ```
//!
//! Each submodule exposes a `types() -> Vec<Arc<dyn NodeType>>` list; [`Builtins::types`]
//! concatenates them.

mod base;
mod bus;
mod color;
mod constants;
mod debug;
mod frame;
mod image;
mod logic;
mod map;
mod math;
mod note;
mod string;
mod subnet;
mod vec;

use std::sync::Arc;

/// The Subnet family constructor (`Subnet` + wired `SubnetInput`/`SubnetOutput`).
pub use subnet::create_subnet;

use crate::node_type::{NodeLibrary, NodeType};
use crate::registry::NodeTypeRegistry;
use crate::value_types::{ValueTypeInfo, ValueTypes, builtin_value_types};

/// The stock node library: math, constants, logic, string, vector, colour, map, bus, image I/O,
/// the Subnet family and the backdrop nodes — plus the builtin VALUE types they use (declared as
/// the library's [`NodeLibrary::value_types`], so `add_library(&Builtins, ..)` is all a host needs).
pub struct Builtins;

impl NodeLibrary for Builtins {
    fn name(&self) -> &str {
        "builtins"
    }

    fn types(&self) -> Vec<Arc<dyn NodeType>> {
        let mut all = Vec::new();
        all.extend(frame::types());
        all.extend(note::types());
        all.extend(base::types());
        all.extend(debug::types());
        all.extend(bus::types());
        all.extend(constants::types());
        all.extend(math::types());
        all.extend(image::types());
        all.extend(logic::types());
        all.extend(string::types());
        all.extend(vec::types());
        all.extend(color::types());
        all.extend(subnet::types());
        all.extend(map::types());
        all
    }

    fn value_types(&self) -> Vec<ValueTypeInfo> {
        builtin_value_types()
    }
}

/// A CURATED subset of the [`Builtins`], selected by CATEGORY NAME — for a domain editor that wants
/// only a few stock nodes (e.g. `["note", "subnet"]` = annotations + grouping) beside its OWN palette,
/// instead of the full math / logic / string / … set. It is an ordinary [`NodeLibrary`], so an
/// embedder adds it like any other pack; combine it with a bare (no-builtins) registry to get exactly
/// "my nodes + these categories".
///
/// Valid category keys (one per builtins submodule): `frame`, `note`, `base`, `debug`, `bus`,
/// `constants`, `math`, `image`, `logic`, `string`, `vec`, `color`, `subnet`, `map`. An unknown key
/// contributes nothing (no panic). The builtin VALUE types are brought along wholesale (cheap, and the
/// selected nodes need them).
pub struct BuiltinsSubset {
    /// The builtins category keys to include (see the type docs for the valid set).
    pub categories: &'static [&'static str],
}

impl BuiltinsSubset {
    /// The node types of ONE builtins category key (empty for an unknown key).
    fn category_types(key: &str) -> Vec<Arc<dyn NodeType>> {
        match key {
            "frame" => frame::types(),
            "note" => note::types(),
            "base" => base::types(),
            "debug" => debug::types(),
            "bus" => bus::types(),
            "constants" => constants::types(),
            "math" => math::types(),
            "image" => image::types(),
            "logic" => logic::types(),
            "string" => string::types(),
            "vec" => vec::types(),
            "color" => color::types(),
            "subnet" => subnet::types(),
            "map" => map::types(),
            _ => Vec::new(),
        }
    }
}

impl NodeLibrary for BuiltinsSubset {
    fn name(&self) -> &str {
        "builtins"
    }

    fn types(&self) -> Vec<Arc<dyn NodeType>> {
        self.categories
            .iter()
            .flat_map(|k| Self::category_types(k))
            .collect()
    }

    fn value_types(&self) -> Vec<ValueTypeInfo> {
        builtin_value_types()
    }
}

/// Add the [`Builtins`] library to `registry` (and its value types to `types`).
///
/// A thin alias for `registry.add_library(&Builtins, types)` — kept because every host and test
/// says "register the builtins", not "add the builtins library".
pub fn register_builtins(registry: &mut NodeTypeRegistry, types: &mut ValueTypes) {
    registry.add_library(&Builtins, types);
}
