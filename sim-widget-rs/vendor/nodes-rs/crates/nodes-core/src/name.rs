//! Node `name` validation, auto-naming, and graph-wide normalization.
//!
//! A unique `name` per **parent-scope** is required for [`crate::address::OpPath`]
//! (the resolver [`crate::address`] keys on `(name, parent)`).
//! [`crate::graph_container::Graph::create_node`] allocates names via [`NameIndex`].
//!
//! Two name systems coexist on purpose:
//! - [`NameIndex`] — an eager, per-scope derived index owned by `Graph`. It powers the
//!   HOT path ([`crate::graph_container::Graph::create_node`]) in O(1) amortized and is
//!   reconstructed from the node map by
//!   [`crate::graph_container::Graph::rebuild_index`]. Its lifecycle mirrors the
//!   [`crate::index::AdjacencyIndex`]: any bulk path that leaves one stale leaves both
//!   stale, and both are rebuilt at the same site.
//! - [`name_taken`] / [`suggest_node_name`] / [`uniquify_name`] — node-scanning helpers
//!   for the COLD one-shot paths (load/merge/normalize/import) where the registry is not
//!   yet built. They are registry-independent so they stay correct mid-load while names
//!   are still being assigned. Never call them in a per-node hot loop.

use std::collections::{HashMap, HashSet};

use crate::error::{CoreError, CoreResult};
use crate::graph::NodeId;
use crate::graph_container::Graph;
use crate::node::Node;

/// Eager, per-scope name registry: the allocation authority for `create_node`.
///
/// `used` answers membership; `next` is a monotonic-per-rebuild suffix hint that keeps
/// allocation O(1) amortized (without it the upward probe for a free `{base}{n}` is
/// O(n) long → O(n²) over a batch). Correctness comes from the `used` membership check,
/// not from `next`; `next` is only a starting point and is re-seeded on every rebuild.
#[derive(Debug, Clone, Default)]
pub(crate) struct NameIndex {
    /// `parent-scope -> { live names in that scope }`.
    used: HashMap<Option<NodeId>, HashSet<String>>,
    /// `(scope, base) -> next suffix to try`. Hint only; re-seeded by [`Self::build`].
    next: HashMap<(Option<NodeId>, String), u32>,
}

impl NameIndex {
    /// Reconstruct per-scope name sets (and the `next` hints) from the node map. O(nodes).
    /// This is the SSOT reconstruction — every cold/bypass insert path is covered because
    /// it already calls [`Graph::rebuild_index`].
    pub(crate) fn build(nodes: &HashMap<NodeId, Node>) -> Self {
        let mut idx = Self::default();
        for node in nodes.values() {
            let name = node.name();
            if name.is_empty() {
                continue;
            }
            let scope = node.parent();
            idx.used.entry(scope).or_default().insert(name.to_string());
            // Seed the suffix hint so post-rebuild allocation stays O(1).
            if let Some((base, num)) = split_suffix(name) {
                let e = idx.next.entry((scope, base.to_string())).or_insert(1);
                *e = (*e).max(num.saturating_add(1));
            }
        }
        idx
    }

    /// Whether `name` is live in `scope`.
    pub(crate) fn contains(&self, scope: Option<NodeId>, name: &str) -> bool {
        self.used.get(&scope).is_some_and(|s| s.contains(name))
    }

    /// Register `name` in `scope`.
    pub(crate) fn insert(&mut self, scope: Option<NodeId>, name: &str) {
        self.used.entry(scope).or_default().insert(name.to_string());
    }

    /// Drop `name` from `scope` (no-op if absent).
    pub(crate) fn remove(&mut self, scope: Option<NodeId>, name: &str) {
        if let Some(s) = self.used.get_mut(&scope) {
            s.remove(name);
        }
    }

    /// Allocate and register a fresh unique `{base}{n}` in `scope`. O(1) amortized.
    pub(crate) fn alloc(&mut self, scope: Option<NodeId>, base: &str) -> String {
        let key = (scope, base.to_string());
        let mut n = self.next.get(&key).copied().unwrap_or(1).max(1);
        loop {
            let candidate = format!("{base}{n}");
            if !self.contains(scope, &candidate) {
                self.insert(scope, &candidate);
                self.next.insert(key, n.saturating_add(1));
                return candidate;
            }
            n += 1;
        }
    }

    /// Take `desired` if free in `scope`, else fall back to `{desired}{n}`. Registers and
    /// returns the final name. Used by reparent/import where a specific name is preferred.
    pub(crate) fn alloc_unique(&mut self, scope: Option<NodeId>, desired: &str) -> String {
        if !self.contains(scope, desired) {
            self.insert(scope, desired);
            return desired.to_string();
        }
        self.alloc(scope, desired)
    }

    /// Move `name` from `old` scope to `new` scope, uniquifying on collision. Returns the
    /// final name (may differ from `name` when the target scope already held it).
    pub(crate) fn reparent(
        &mut self,
        old: Option<NodeId>,
        new: Option<NodeId>,
        name: &str,
    ) -> String {
        self.remove(old, name);
        self.alloc_unique(new, name)
    }

    /// Debug-only invariant: no two nodes share a name within one parent-scope.
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_unique(nodes: &HashMap<NodeId, Node>) {
        let mut counts: HashMap<(Option<NodeId>, String), usize> = HashMap::new();
        for n in nodes.values() {
            let name = n.name();
            if name.is_empty() {
                continue;
            }
            *counts.entry((n.parent(), name.to_string())).or_default() += 1;
        }
        for ((scope, name), c) in counts {
            debug_assert!(
                c == 1,
                "duplicate node name {name:?} in scope {scope:?} ({c} nodes)"
            );
        }
    }
}

/// Split a trailing decimal suffix: `"add12"` → `Some(("add", 12))`, `"float"` → `None`.
/// An all-digit name (no alphabetic base) yields `None` (no useful base to seed).
fn split_suffix(name: &str) -> Option<(&str, u32)> {
    let cut = name
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, _)| i)?;
    if cut == 0 {
        return None; // entirely digits → no base
    }
    let (base, digits) = name.split_at(cut);
    digits.parse::<u32>().ok().map(|n| (base, n))
}

/// `{type_lower}` base for auto-names (`Float` → `float`). Empty type → `node`.
pub fn name_base(type_name: &str) -> String {
    let base = type_name
        .chars()
        .flat_map(|c| c.to_lowercase())
        .collect::<String>();
    if base.is_empty() {
        "node".to_string()
    } else {
        base
    }
}

/// Characters a PORT / ATTR / TYPE name may not contain, because box-rs's `parse_path` reads them
/// as path STRUCTURE rather than as part of a key.
///
/// The same rule [`validate_node_name`] already enforces for a NODE name (no `.`), extended to `[`
/// and `]` and applied to the OTHER namespace it was never applied to: the port/attr names a node
/// type declares. A node's data lives in a `RustBox` addressed by paths built from those names
/// (`inputs.{name}.value`, `compute.{name}`, …), so a port called `a.b` writes into a nested
/// `a → b` key that nothing reads back, and one containing `[` fails the write outright — either
/// way the author's value silently disappears at cook time.
pub const PATH_HOSTILE: [char; 3] = ['.', '[', ']'];

/// Validate ONE name a box path is built from — a node type's own name, a port name or an
/// [`crate::AttrDef`] key (see [`PATH_HOSTILE`]).
///
/// Deliberately LOOSER than [`validate_node_name`]: a node name is a Houdini identifier (used in op
/// paths), while a port name only has to survive being a single box-path segment, so existing names
/// with spaces or other punctuation keep working. `what` (`"input port"`, …) and `owner` (the type
/// name) only shape the error message.
///
/// Called for every name on a [`crate::NodeDef`] by [`crate::validate_def`], which the registry runs
/// before a type is allowed in.
pub fn validate_path_name(what: &str, owner: &str, name: &str) -> CoreResult<()> {
    if name.is_empty() {
        return Err(CoreError::InvalidName(format!(
            "node type {owner:?}: {what} name is empty (it would address the box root)"
        )));
    }
    if let Some(c) = name.chars().find(|c| PATH_HOSTILE.contains(c)) {
        return Err(CoreError::InvalidName(format!(
            "node type {owner:?}: {what} name {name:?} must not contain {c:?} — box-rs path syntax \
             reads it as STRUCTURE, so a write to {name:?} would land in the wrong key (or fail) \
             and the value would be LOST"
        )));
    }
    Ok(())
}

/// Houdini-style identifier: letter/underscore start, then alphanumeric/underscore; no `.`.
pub fn validate_node_name(name: &str) -> CoreResult<()> {
    let Some(first) = name.chars().next() else {
        return Err(CoreError::InvalidName("empty name".into()));
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(CoreError::InvalidName(format!(
            "name must start with [A-Za-z_]: {name:?}"
        )));
    }
    if name.contains('.') {
        return Err(CoreError::InvalidName(format!(
            "name must not contain '.': {name:?}"
        )));
    }
    for ch in name.chars().skip(1) {
        if !(ch.is_ascii_alphanumeric() || ch == '_') {
            return Err(CoreError::InvalidName(format!(
                "invalid character in name: {name:?}"
            )));
        }
    }
    Ok(())
}

/// Whether `name` is already used by another node **in `scope`** (optionally excluding
/// `except`). Node-scanning; for cold paths only (see module docs).
pub fn name_taken(
    graph: &Graph,
    scope: Option<NodeId>,
    name: &str,
    except: Option<NodeId>,
) -> bool {
    graph
        .nodes
        .values()
        .any(|n| n.parent() == scope && n.name() == name && except.is_none_or(|id| id != n.id))
}

/// `{type_lower}{n}` unique within `scope` (`Float` → `float1`, `float2`, …).
/// Node-scanning; for cold paths (`infer_node_name`/normalize). The hot create path uses
/// [`NameIndex::alloc`] instead.
pub fn suggest_node_name(graph: &Graph, scope: Option<NodeId>, type_name: &str) -> String {
    let base = name_base(type_name);
    let mut n = 1u32;
    loop {
        let candidate = format!("{base}{n}");
        if !name_taken(graph, scope, &candidate, None) {
            return candidate;
        }
        n += 1;
    }
}

/// Pick a unique name within `scope` starting from `desired` (`desired`, `desired1`, …).
/// Node-scanning; for cold paths.
pub fn uniquify_name(
    graph: &Graph,
    scope: Option<NodeId>,
    desired: &str,
    except: Option<NodeId>,
) -> String {
    let base = sanitize_name_base(desired);
    if !name_taken(graph, scope, &base, except) {
        return base;
    }
    let mut n = 1u32;
    loop {
        let candidate = format!("{base}{n}");
        if !name_taken(graph, scope, &candidate, except) {
            return candidate;
        }
        n += 1;
    }
}

/// Reduce an arbitrary label to a valid identifier base (spaces/dashes → `_`, drop other
/// punctuation, ensure a leading `[A-Za-z_]`). Exposed for callers that need to pre-sanitize
/// labels before their own uniqueness pass (e.g. the diagram importer).
pub fn sanitize_name_base(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if (ch.is_whitespace() || ch == '-') && !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    if out.is_empty() {
        return "node".to_string();
    }
    if !(out.as_bytes()[0].is_ascii_alphabetic() || out.as_bytes()[0] == b'_') {
        out.insert(0, '_');
    }
    out
}

/// Infer a valid unique name from a node's display name (or type), within the node's scope.
pub fn infer_node_name(graph: &Graph, node: &Node) -> String {
    let scope = node.parent();
    let from_display = sanitize_name_base(node.display_name());
    if !from_display.is_empty() && from_display != "node" {
        return uniquify_name(graph, scope, &from_display, Some(node.id));
    }
    suggest_node_name(graph, scope, node.type_name())
}

/// Ensure every node in `graph` has a valid unique `name` within its parent-scope.
/// Cold path (load/paste); the caller rebuilds the derived indices afterwards.
pub fn normalize_subnet_names(graph: &mut Graph) {
    let ids: Vec<NodeId> = graph.nodes.keys().copied().collect();
    for id in &ids {
        let needs_name = graph
            .nodes
            .get(id)
            .is_some_and(|n| n.name().is_empty() || validate_node_name(n.name()).is_err());
        if needs_name {
            let name = {
                let node = graph.nodes.get(id).unwrap();
                infer_node_name(graph, node)
            };
            if let Some(node) = graph.nodes.get_mut(id) {
                node.set_name(&name);
            }
        }
    }
    for id in &ids {
        let taken = graph
            .nodes
            .get(id)
            .is_some_and(|n| name_taken(graph, n.parent(), n.name(), Some(*id)));
        if taken {
            let (scope, cur) = {
                let node = graph.nodes.get(id).unwrap();
                (node.parent(), node.name().to_string())
            };
            let new_name = uniquify_name(graph, scope, &cur, Some(*id));
            if let Some(node) = graph.nodes.get_mut(id) {
                node.set_name(&new_name);
            }
        }
    }
}

/// Rename a node; enforces validation and per-scope uniqueness, and keeps the
/// [`NameIndex`] in sync (this targeted edit does not trigger a full rebuild).
pub fn rename_node(graph: &mut Graph, node_id: NodeId, new_name: &str) -> CoreResult<()> {
    validate_node_name(new_name)?;
    let scope = graph.nodes.get(&node_id).and_then(|n| n.parent());
    if name_taken(graph, scope, new_name, Some(node_id)) {
        return Err(CoreError::DuplicateName(new_name.to_string()));
    }
    let old = graph.nodes.get(&node_id).map(|n| n.name().to_string());
    let node = graph
        .nodes
        .get_mut(&node_id)
        .ok_or(CoreError::NodeNotFound(node_id.uuid()))?;
    node.set_name(new_name);
    if let Some(old) = old {
        graph.names.remove(scope, &old);
    }
    graph.names.insert(scope, new_name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Graph, NodeTypeRegistry, ValueTypes, register_builtins};

    #[test]
    fn validate_rejects_dot() {
        assert!(validate_node_name("box.1").is_err());
        assert!(validate_node_name("geo1").is_ok());
    }

    /// The SAME no-dot discipline, in the namespace it was never applied to: a PORT/ATTR name is a
    /// box-path segment, so `.`/`[`/`]` (or an empty name) would silently misfile every write to
    /// that port. Looser than a node name otherwise — a space stays legal.
    #[test]
    fn validate_path_name_rejects_path_structure() {
        assert!(validate_path_name("input port", "Add", "a.b").is_err());
        assert!(validate_path_name("input port", "Add", "x[0]").is_err());
        assert!(validate_path_name("attr", "Add", "").is_err());
        assert!(validate_path_name("input port", "Add", "a").is_ok());
        assert!(validate_path_name("input port", "Add", "max iter").is_ok());

        // The message names the type, the name and the offending character.
        let e = validate_path_name("input port", "Add", "a.b")
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("Add") && e.contains("a.b") && e.contains('.'),
            "{e}"
        );
    }

    #[test]
    fn suggest_increments() {
        // `register_builtins` registers the builtin value types too — registering them again would
        // be a (now warned) override of each.
        let mut types = ValueTypes::new();
        let mut reg = NodeTypeRegistry::new();
        register_builtins(&mut reg, &mut types);
        let mut graph = Graph::new();
        let _ = graph
            .create_node("Float", [0.0, 0.0], &reg, &types)
            .unwrap();
        // Scan-based suggest (cold path) agrees with the registry-allocated `float1`.
        let n2 = suggest_node_name(&graph, None, "Float");
        assert_eq!(n2, "float2");
    }

    #[test]
    fn split_suffix_cases() {
        assert_eq!(split_suffix("add12"), Some(("add", 12)));
        assert_eq!(split_suffix("float"), None);
        assert_eq!(split_suffix("123"), None);
        assert_eq!(split_suffix("a1b2"), Some(("a1b", 2)));
    }
}
