//! Houdini-style op paths and address resolution over the flat graph.
//!
//! [`OpPath`] / [`OpAddress`] identify nodes and their data for CH-style tools.
//! Hierarchy is flat: a path segment descends into a node's children (nodes whose
//! `parent` is that node). Tails resolve against the unified `data` box.

use std::fmt;

use crate::box_model::PathKey;
use crate::error::{CoreError, CoreResult};
use crate::graph::NodeId;
use crate::graph_container::Graph;
use crate::name::validate_node_name;
use crate::node::Node;
use crate::registry::NodeTypeRegistry;

/// Absolute node path: `/geo1/box1` → segments `["geo1", "box1"]`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpPath {
    pub segments: Vec<String>,
}

impl OpPath {
    pub fn root() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    pub fn from_segments(segments: Vec<String>) -> CoreResult<Self> {
        for seg in &segments {
            validate_node_name(seg)?;
        }
        Ok(Self { segments })
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn format(&self) -> String {
        if self.segments.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", self.segments.join("/"))
        }
    }
}

impl fmt::Display for OpPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format())
    }
}

/// What an address points at beyond the node path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressTail {
    Node,
    /// A `data` path (e.g. `view.pos` or `attrs.value.value`).
    Param(PathKey),
    Input(PortRef),
    Output(PortRef),
    /// `@slot` — bus on the focus graph.
    BusSlot(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortRef {
    Index(usize),
    Name(String),
}

/// Full address: op path + optional tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpAddress {
    pub path: OpPath,
    pub tail: AddressTail,
}

/// Editor navigation context for relative paths (v1 API surface).
#[derive(Debug, Clone)]
pub struct EditFocus {
    pub path: OpPath,
}

impl EditFocus {
    pub fn at_root() -> Self {
        Self {
            path: OpPath::root(),
        }
    }
}

/// Resolved node location (absolute path).
#[derive(Debug, Clone)]
pub struct ResolvedLocation {
    pub path: OpPath,
    pub node_id: NodeId,
}

/// What an [`OpAddress`] resolves to within a [`Graph`].
pub enum ResolvedTarget<'a> {
    Node {
        location: ResolvedLocation,
        node: &'a Node,
    },
    Param {
        location: ResolvedLocation,
        node: &'a Node,
        path: PathKey,
    },
    Input {
        location: ResolvedLocation,
        node: &'a Node,
        index: usize,
        name: String,
    },
    Output {
        location: ResolvedLocation,
        node: &'a Node,
        index: usize,
        name: String,
    },
    BusSlot {
        slot: String,
        graph: &'a Graph,
    },
}

/// Reserved first segments (stripped on resolve); future virtual contexts.
pub const VIRTUAL_CONTEXT_OBJ: &str = "obj";
pub const VIRTUAL_CONTEXT_OUT: &str = "out";
pub const VIRTUAL_CONTEXT_MAT: &str = "mat";

const VIRTUAL_CONTEXT_PREFIXES: &[&str] = &[
    VIRTUAL_CONTEXT_OBJ,
    VIRTUAL_CONTEXT_OUT,
    VIRTUAL_CONTEXT_MAT,
];

pub fn parse_address(input: &str) -> CoreResult<OpAddress> {
    let s = input.trim();
    if s.is_empty() {
        return Err(CoreError::InvalidPath("empty address".into()));
    }
    if let Some(slot) = s.strip_prefix('@') {
        if slot.is_empty() {
            return Err(CoreError::InvalidPath("empty bus slot".into()));
        }
        validate_bus_slot(slot)?;
        return Ok(OpAddress {
            path: OpPath::root(),
            tail: AddressTail::BusSlot(slot.to_string()),
        });
    }

    let mut body = s;
    if body.starts_with('/') {
        body = &body[1..];
    }

    let mut segments: Vec<String> = body
        .split('/')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();

    let tail = if let Some(last) = segments.last().cloned() {
        if let Some(dot) = last.find('.') {
            if dot == 0 {
                return Err(CoreError::InvalidPath(
                    "parm tail cannot start with '.' on its own segment".into(),
                ));
            }
            let parm_str = &last[dot + 1..];
            if parm_str.is_empty() {
                return Err(CoreError::InvalidPath("empty parm tail".into()));
            }
            segments.pop();
            segments.push(last[..dot].to_string());
            parse_parm_tail(parm_str)?
        } else {
            AddressTail::Node
        }
    } else {
        AddressTail::Node
    };

    for seg in &segments {
        validate_node_name(seg)?;
    }

    Ok(OpAddress {
        path: OpPath { segments },
        tail,
    })
}

/// Parse `/geo1/box1` (node-only).
pub fn parse_op_path(input: &str) -> CoreResult<OpPath> {
    let addr = parse_address(input)?;
    if !matches!(addr.tail, AddressTail::Node) {
        return Err(CoreError::InvalidPath(
            "expected node path without parm tail".into(),
        ));
    }
    Ok(addr.path)
}

fn parse_parm_tail(parm_str: &str) -> CoreResult<AddressTail> {
    let path = crate::box_model::RustBox::path_key(parm_str)
        .map_err(|e| CoreError::InvalidPath(e.to_string()))?;
    let segs = path.segments();
    if let Some(crate::box_model::Segment::Key(key)) = segs.first() {
        if key == "inputs" {
            return Ok(AddressTail::Input(port_ref_from_rest(&segs[1..])?));
        }
        if key == "outputs" {
            return Ok(AddressTail::Output(port_ref_from_rest(&segs[1..])?));
        }
    }
    Ok(AddressTail::Param(path))
}

fn port_ref_from_rest(segs: &[crate::box_model::Segment]) -> CoreResult<PortRef> {
    match segs {
        [crate::box_model::Segment::Index(i)] => Ok(PortRef::Index(*i)),
        [crate::box_model::Segment::Key(name)] => Ok(PortRef::Name(name.clone())),
        [] => Err(CoreError::InvalidPath("missing port index or name".into())),
        _ => Err(CoreError::InvalidPath("invalid port reference".into())),
    }
}

fn validate_bus_slot(slot: &str) -> CoreResult<()> {
    if slot.is_empty() || slot.contains(['/', '.', '@']) {
        return Err(CoreError::InvalidPath(format!(
            "invalid bus slot: {slot:?}"
        )));
    }
    Ok(())
}

fn strip_virtual_context(mut segments: Vec<String>) -> Vec<String> {
    while segments
        .first()
        .is_some_and(|s| VIRTUAL_CONTEXT_PREFIXES.contains(&s.as_str()))
    {
        segments.remove(0);
    }
    segments
}

/// Find a child of `parent` (or a root node when `parent` is `None`) by name.
fn child_by_name(graph: &Graph, parent: Option<NodeId>, name: &str) -> Option<NodeId> {
    graph
        .nodes
        .iter()
        .find(|(_, n)| n.name() == name && n.parent() == parent)
        .map(|(id, _)| *id)
}

/// Resolve absolute node path from `graph` root, descending by `children`.
pub fn resolve_op(graph: &Graph, path: &OpPath) -> CoreResult<ResolvedLocation> {
    if path.is_empty() {
        return Err(CoreError::PathNotFound("/".into()));
    }
    let segments = strip_virtual_context(path.segments.clone());
    if segments.is_empty() {
        return Err(CoreError::PathNotFound(path.format()));
    }

    let mut parent: Option<NodeId> = None;
    let mut node_id = None;
    for (i, seg) in segments.iter().enumerate() {
        let id = child_by_name(graph, parent, seg)
            .ok_or_else(|| CoreError::PathNotFound(path.format()))?;
        if i + 1 == segments.len() {
            node_id = Some(id);
            break;
        }
        parent = Some(id);
    }

    let node_id = node_id.expect("last segment resolves node");
    Ok(ResolvedLocation {
        path: OpPath {
            segments: path.segments.clone(),
        },
        node_id,
    })
}

/// Resolve a full address against a graph.
pub fn resolve_address<'a>(
    graph: &'a Graph,
    focus_graph: &'a Graph,
    registry: &'a NodeTypeRegistry,
    addr: &OpAddress,
) -> CoreResult<ResolvedTarget<'a>> {
    match &addr.tail {
        AddressTail::BusSlot(slot) => Ok(ResolvedTarget::BusSlot {
            slot: slot.clone(),
            graph: focus_graph,
        }),
        _ => {
            let loc = resolve_op(graph, &addr.path)?;
            let node = graph
                .nodes
                .get(&loc.node_id)
                .ok_or(CoreError::NodeNotFound(loc.node_id.uuid()))?;
            let _def = registry
                .get(node.type_name())
                .ok_or_else(|| CoreError::UnknownNodeType(node.type_name().to_string()))?;
            match &addr.tail {
                AddressTail::Node => Ok(ResolvedTarget::Node {
                    location: loc,
                    node,
                }),
                AddressTail::Param(path) => Ok(ResolvedTarget::Param {
                    location: loc,
                    node,
                    path: path.clone(),
                }),
                AddressTail::Input(port_ref) => {
                    let (index, name) = resolve_port(&node.input_names(), port_ref)?;
                    Ok(ResolvedTarget::Input {
                        location: loc,
                        node,
                        index,
                        name,
                    })
                }
                AddressTail::Output(port_ref) => {
                    let (index, name) = resolve_port(&node.output_names(), port_ref)?;
                    Ok(ResolvedTarget::Output {
                        location: loc,
                        node,
                        index,
                        name,
                    })
                }
                AddressTail::BusSlot(_) => unreachable!(),
            }
        }
    }
}

fn resolve_port(ports: &[String], port_ref: &PortRef) -> CoreResult<(usize, String)> {
    match port_ref {
        PortRef::Index(i) => ports
            .get(*i)
            .map(|p| (*i, p.clone()))
            .ok_or_else(|| CoreError::InvalidPath(format!("port index {i} out of range"))),
        PortRef::Name(name) => ports
            .iter()
            .position(|p| p == name)
            .map(|i| (i, name.clone()))
            .ok_or_else(|| CoreError::InvalidPath(format!("unknown port {name:?}"))),
    }
}

/// Build absolute op path for `node_id` by walking `parent` links upward.
pub fn op_path_of(graph: &Graph, node_id: NodeId) -> CoreResult<OpPath> {
    if !graph.nodes.contains_key(&node_id) {
        return Err(CoreError::NodeNotFound(node_id.uuid()));
    }
    let mut segments = Vec::new();
    let mut cur = Some(node_id);
    while let Some(id) = cur {
        let node = graph
            .nodes
            .get(&id)
            .ok_or(CoreError::NodeNotFound(id.uuid()))?;
        segments.push(node.name().to_string());
        cur = node.parent();
    }
    segments.reverse();
    Ok(OpPath { segments })
}

/// Relative resolve: `../peer`, `child`, or absolute `/…`.
pub fn resolve_relative<'a>(
    graph: &'a Graph,
    focus: &EditFocus,
    focus_graph: &'a Graph,
    registry: &'a NodeTypeRegistry,
    input: &str,
) -> CoreResult<ResolvedTarget<'a>> {
    let s = input.trim();
    if s.starts_with('@') || s.starts_with('/') {
        return resolve_address(graph, focus_graph, registry, &parse_address(s)?);
    }
    if let Some(rest) = s.strip_prefix("../") {
        if focus.path.is_empty() {
            return Err(CoreError::InvalidPath("no parent graph".into()));
        }
        let mut parent_segs = focus.path.segments.clone();
        parent_segs.pop();
        validate_node_name(rest)?;
        parent_segs.push(rest.to_string());
        return resolve_address(
            graph,
            focus_graph,
            registry,
            &OpAddress {
                path: OpPath {
                    segments: parent_segs,
                },
                tail: AddressTail::Node,
            },
        );
    }
    let mut combined = focus.path.segments.clone();
    for seg in s.split('/').filter(|p| !p.is_empty()) {
        validate_node_name(seg)?;
        combined.push(seg.to_string());
    }
    resolve_address(
        graph,
        focus_graph,
        registry,
        &OpAddress {
            path: OpPath { segments: combined },
            tail: AddressTail::Node,
        },
    )
}
