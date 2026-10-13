//! Backwards-compatible alias. The graph container lives in
//! [`crate::graph_container::Graph`]; `Subnet` is kept as a type alias to limit
//! churn in downstream crates (ported in a later phase). Hierarchy is now flat
//! (per-node `parent`/`children`), not nested child subnets.

pub use crate::graph_container::Graph as Subnet;
