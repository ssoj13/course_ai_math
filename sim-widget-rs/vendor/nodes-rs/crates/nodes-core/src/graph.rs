//! Stable identifiers for nodes.
//!
//! [`NodeId`] is a serde-transparent newtype over [`uuid::Uuid`]. The graph stores
//! nodes in a flat `HashMap<Uuid, Node>` (see [`crate::graph_container::Graph`]);
//! edges live in each consumer node's `attrs.<input>.source` (no explicit wire list).

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable node identity. Transparent over [`Uuid`] for serde + map keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub Uuid);

impl NodeId {
    /// Fresh random (v4) id.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Underlying uuid.
    pub fn uuid(self) -> Uuid {
        self.0
    }

    /// Parse from a uuid string (used when reading `source`/`parent` refs).
    pub fn parse(s: &str) -> Option<Self> {
        Uuid::parse_str(s).ok().map(Self)
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for NodeId {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}

impl From<NodeId> for Uuid {
    fn from(id: NodeId) -> Self {
        id.0
    }
}

/// Subnets are ordinary nodes now (flat hierarchy via `parent`/`children`); the
/// historical `SubnetId` is kept as an alias for callers that still name it.
pub type SubnetId = Uuid;
