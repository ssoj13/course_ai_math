//! [`Document`] — the arena of schema [`Node`]s, plus `.otio` JSON I/O.

use std::collections::HashSet;

use serde_json::{Map, Value};
use slotmap::{new_key_type, SlotMap};

use crate::error::Error;
use crate::schema::{is_value_schema, parse_schema};

/// Maximum structural nesting depth for the recursive arena walks (`build_node`,
/// `emit`, `deep_clone`, `adopt_rec`). Real OTIO trees are shallow
/// (timeline→stack→track→clip→media-ref ≈ 5 levels); 1024 is far beyond any
/// legitimate document yet bounds native stack growth, so a pathologically
/// nested or corrupt input fails with an error instead of aborting the whole
/// process via stack overflow.
const MAX_DEPTH: usize = 1024;

/// Field keys whose value is an opaque dictionary (`AnyDictionary`) that may
/// legally embed nested `SerializableObject`s — plugin `metadata` and generator
/// `parameters`. Their contents must NOT be promoted to structural children:
/// promotion flips `Node::metadata()` to empty and makes embedded objects look
/// like real children to clone / gc / detach. Such fields stay verbatim
/// [`Field::Value`] and re-emit byte-identically, so the JSON round-trip is
/// unchanged. See [`Document::classify`].
const OPAQUE_KEYS: &[&str] = &["metadata", "parameters"];

new_key_type! {
    /// A stable handle to a [`Node`] within a [`Document`].
    ///
    /// Generational: deleting a node does not invalidate other handles, and a
    /// reused slot gets a fresh generation (no ABA). This is the identity used
    /// for undo/redo, cross-references, and (later) language bindings.
    pub struct NodeId;
}

/// One field of a serializable object, preserving source order.
#[derive(Debug, Clone)]
pub enum Field {
    /// A verbatim JSON value: scalars, metadata dicts, and OTIO value-type
    /// schemas (`RationalTime`, `TimeRange`, …) that are kept inline.
    Value(Value),
    /// A structural link to a single child node (e.g. `tracks`, `media_reference`).
    Child(NodeId),
    /// A structural link to an ordered list of child nodes (e.g. `children`,
    /// `effects`, `markers`).
    Children(Vec<NodeId>),
    /// A structural link to a *named map* of child nodes (e.g. the Clip.2
    /// `media_references` map keyed by reference name).
    ChildMap(Vec<(String, NodeId)>),
}

impl Field {
    /// The child node ids this field links to (empty for a `Value`).
    ///
    /// The single point of "what does this field reference" — every structural
    /// traversal (gc mark, unlink, detach, leaf check, clone, adopt, emit) routes
    /// through here so adding a future `Field` variant is one compile error, not
    /// a silent dangling-id hazard scattered across call sites.
    pub fn child_ids(&self) -> Vec<NodeId> {
        match self {
            Field::Value(_) => Vec::new(),
            Field::Child(c) => vec![*c],
            Field::Children(ids) => ids.clone(),
            Field::ChildMap(entries) => entries.iter().map(|(_, c)| *c).collect(),
        }
    }
}

/// A single serializable object: its schema, parent, and ordered fields.
#[derive(Debug, Clone)]
pub struct Node {
    schema_name: String,
    schema_version: u32,
    parent: Option<NodeId>,
    fields: Vec<(String, Field)>,
}

impl Node {
    /// The schema name (e.g. `"Clip"`).
    pub fn schema_name(&self) -> &str {
        &self.schema_name
    }

    /// The schema version (e.g. `2` for `Clip.2`).
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// This node's parent, if any.
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// The ordered fields of this node.
    pub fn fields(&self) -> &[(String, Field)] {
        &self.fields
    }

    /// Look up a field by key.
    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, f)| f)
    }

    /// The child node id linked under `key`, if that field is a single child.
    pub fn child(&self, key: &str) -> Option<NodeId> {
        match self.field(key) {
            Some(Field::Child(id)) => Some(*id),
            _ => None,
        }
    }

    /// The child node ids linked under `key`, if that field is a child list.
    pub fn children(&self, key: &str) -> &[NodeId] {
        match self.field(key) {
            Some(Field::Children(ids)) => ids,
            _ => &[],
        }
    }

    /// The named child map under `key` (e.g. `media_references`), if present.
    pub fn child_map(&self, key: &str) -> &[(String, NodeId)] {
        match self.field(key) {
            Some(Field::ChildMap(m)) => m,
            _ => &[],
        }
    }

    /// The child node linked under `key`'s map entry `name`, if present.
    pub fn child_in_map(&self, key: &str, name: &str) -> Option<NodeId> {
        self.child_map(key)
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, id)| *id)
    }

    /// This node's `metadata` field as a (general-purpose) box-rs container.
    ///
    /// Returns an empty box when absent. The box is a detached snapshot — mutate
    /// and write it back via the typed accessor layer (not yet wired here).
    pub fn metadata(&self) -> box_rs::RustBox {
        match self.field("metadata") {
            Some(Field::Value(v)) => {
                box_rs::RustBox::from_json_value(v.clone(), box_rs::BoxConfig::default())
            }
            _ => box_rs::RustBox::new(),
        }
    }

    // --- mutation ----------------------------------------------------------

    /// Set (or append) a plain-value field, preserving existing field order.
    ///
    /// Replaces an existing field of the same key; appends if new. Intended for
    /// value fields (scalars, value-type schemas, metadata). Replacing a field
    /// that currently holds child links would detach them (the child nodes remain
    /// in the arena but become unreferenced) — use this for value fields only.
    pub fn set_value(&mut self, key: &str, value: Value) {
        if let Some(slot) = self.fields.iter_mut().find(|(k, _)| k == key) {
            slot.1 = Field::Value(value);
        } else {
            self.fields.push((key.to_string(), Field::Value(value)));
        }
    }

    /// Remove a field by key. Returns whether it was present.
    pub fn remove_field(&mut self, key: &str) -> bool {
        let before = self.fields.len();
        self.fields.retain(|(k, _)| k != key);
        self.fields.len() != before
    }

    /// Replace the `metadata` field from a box-rs container.
    ///
    /// Note: uses box-rs's JSON form. Do not place box-rs typed aggregates
    /// (Vec3/Color/…) in OTIO metadata until the plain-JSON bridge lands — they
    /// would serialize with non-standard `$`-tags. OTIO-sourced metadata is fine.
    pub fn set_metadata(&mut self, meta: &box_rs::RustBox) {
        self.set_value("metadata", meta.to_json_value());
    }
}

/// A parsed OTIO document: an arena of nodes rooted at one serializable object.
#[derive(Debug, Clone, Default)]
pub struct Document {
    nodes: SlotMap<NodeId, Node>,
    root: Option<NodeId>,
}

impl Document {
    /// Create an empty document.
    pub fn new() -> Self {
        Document::default()
    }

    /// The root node id, if the document is non-empty.
    pub fn root_id(&self) -> Option<NodeId> {
        self.root
    }

    /// The root node, if the document is non-empty.
    pub fn root(&self) -> Option<&Node> {
        self.root.and_then(|id| self.nodes.get(id))
    }

    /// Borrow a node by id.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Mutably borrow a node by id.
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }

    // --- structural construction ------------------------------------------

    /// Create a new empty node with the given schema, returning its id.
    ///
    /// The node has no parent and no fields; wire it in with [`set_child`],
    /// [`append_child`], or [`set_root`].
    ///
    /// [`set_child`]: Self::set_child
    /// [`append_child`]: Self::append_child
    /// [`set_root`]: Self::set_root
    pub fn create_node(&mut self, schema_name: &str, schema_version: u32) -> NodeId {
        self.nodes.insert(Node {
            schema_name: schema_name.to_string(),
            schema_version,
            parent: None,
            fields: Vec::new(),
        })
    }

    /// Set the document root.
    pub fn set_root(&mut self, id: NodeId) {
        self.root = Some(id);
    }

    /// Deep-clone a node and its entire subtree, returning the new root id.
    ///
    /// Child links are followed and cloned recursively with fresh ids; verbatim
    /// value fields are copied as-is. The clone is detached (its top node has no
    /// parent) until wired in.
    pub fn deep_clone(&mut self, id: NodeId) -> Option<NodeId> {
        self.deep_clone_rec(id, 0)
    }

    /// Depth-bounded worker for [`Document::deep_clone`].
    ///
    /// Returns `None` past [`MAX_DEPTH`] (the existing failure channel — callers
    /// already map `None` to their own error), so a programmatically built tree
    /// deeper than the cap fails cleanly instead of overflowing the stack.
    fn deep_clone_rec(&mut self, id: NodeId, depth: usize) -> Option<NodeId> {
        if depth >= MAX_DEPTH {
            return None;
        }
        // Snapshot the source first so we can mutate the arena while cloning.
        let (schema_name, schema_version, fields_src) = {
            let src = self.nodes.get(id)?;
            (src.schema_name.clone(), src.schema_version, src.fields.clone())
        };
        let new_id = self.create_node(&schema_name, schema_version);

        let mut new_fields = Vec::with_capacity(fields_src.len());
        for (key, field) in fields_src {
            let cloned = match field {
                Field::Value(v) => Field::Value(v),
                Field::Child(cid) => {
                    let nc = self.deep_clone_rec(cid, depth + 1)?;
                    self.nodes[nc].parent = Some(new_id);
                    Field::Child(nc)
                }
                Field::Children(ids) => {
                    let mut out = Vec::with_capacity(ids.len());
                    for cid in ids {
                        let nc = self.deep_clone_rec(cid, depth + 1)?;
                        self.nodes[nc].parent = Some(new_id);
                        out.push(nc);
                    }
                    Field::Children(out)
                }
                Field::ChildMap(entries) => {
                    let mut out = Vec::with_capacity(entries.len());
                    for (name, cid) in entries {
                        let nc = self.deep_clone_rec(cid, depth + 1)?;
                        self.nodes[nc].parent = Some(new_id);
                        out.push((name, nc));
                    }
                    Field::ChildMap(out)
                }
            };
            new_fields.push((key, cloned));
        }
        self.nodes[new_id].fields = new_fields;
        Some(new_id)
    }

    /// Deep-copy a subtree from another document into this one.
    ///
    /// Copies `src_root` and all its descendants from `src` into `self` with
    /// fresh ids, returning the new root id together with a **transfer map**
    /// `[(src_id, dst_id), …]` covering every node copied (root first, then in
    /// field/child order). The new root is detached (no parent).
    ///
    /// This is the cross-arena counterpart of [`Document::deep_clone`]. It is
    /// the primitive behind the Python bindings' "adopt on compose": a
    /// free-floating object built in its own staging document is migrated into
    /// the composition's document, and the transfer map lets the binding layer
    /// re-point every live handle (the object *and* its descendants — media
    /// references, markers, effects) to its new id, preserving Python identity.
    ///
    /// `src` is only read; the caller drops or detaches the source subtree.
    /// Returns `None` if `src_root` does not resolve in `src`.
    pub fn adopt_from(
        &mut self,
        src: &Document,
        src_root: NodeId,
    ) -> Option<(NodeId, Vec<(NodeId, NodeId)>)> {
        let mut map = Vec::new();
        let new_root = self.adopt_rec(src, src_root, None, &mut map, 0)?;
        Some((new_root, map))
    }

    /// Recursive worker for [`Document::adopt_from`]: copies `sid` from `src`
    /// under `parent`, recording `(sid, new_id)` in `map`.
    ///
    /// Returns `None` past [`MAX_DEPTH`] (the existing failure channel) so a
    /// pathologically deep source subtree fails cleanly instead of overflowing
    /// the stack.
    fn adopt_rec(
        &mut self,
        src: &Document,
        sid: NodeId,
        parent: Option<NodeId>,
        map: &mut Vec<(NodeId, NodeId)>,
        depth: usize,
    ) -> Option<NodeId> {
        if depth >= MAX_DEPTH {
            return None;
        }
        let snode = src.nodes.get(sid)?;
        let new_id = self.nodes.insert(Node {
            schema_name: snode.schema_name.clone(),
            schema_version: snode.schema_version,
            parent,
            fields: Vec::new(),
        });
        map.push((sid, new_id));

        let mut new_fields = Vec::with_capacity(snode.fields.len());
        for (key, field) in &snode.fields {
            let cloned = match field {
                Field::Value(v) => Field::Value(v.clone()),
                Field::Child(cid) => {
                    Field::Child(self.adopt_rec(src, *cid, Some(new_id), map, depth + 1)?)
                }
                Field::Children(ids) => {
                    let mut out = Vec::with_capacity(ids.len());
                    for cid in ids {
                        out.push(self.adopt_rec(src, *cid, Some(new_id), map, depth + 1)?);
                    }
                    Field::Children(out)
                }
                Field::ChildMap(entries) => {
                    let mut out = Vec::with_capacity(entries.len());
                    for (name, cid) in entries {
                        out.push((
                            name.clone(),
                            self.adopt_rec(src, *cid, Some(new_id), map, depth + 1)?,
                        ));
                    }
                    Field::ChildMap(out)
                }
            };
            new_fields.push((key.clone(), cloned));
        }
        self.nodes[new_id].fields = new_fields;
        Some(new_id)
    }

    /// Set a single-child field (e.g. `tracks`, `media_reference`) on `parent`.
    ///
    /// Reparents `child` to `parent`. Replaces an existing field of the same key;
    /// any node previously linked under `key` (other than `child` itself) is
    /// detached (`parent` cleared) so it does not linger as a phantom child of
    /// `parent` — the same integrity guarantee [`Document::set_child_in_map`] and
    /// [`Document::remove_child`] give, routed through one shared overwrite path.
    /// Rejects reparenting `child` onto its own descendant (would form a cycle).
    pub fn set_child(&mut self, parent: NodeId, key: &str, child: NodeId) -> Result<(), Error> {
        self.require(parent)?;
        self.require(child)?;
        self.reject_cycle(parent, child)?;
        self.nodes[child].parent = Some(parent);
        self.replace_structural_field(parent, key, Field::Child(child));
        Ok(())
    }

    /// Append a child to `parent`'s `children` list (composition convention).
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), Error> {
        let len = self.children_len(parent);
        self.insert_child(parent, len, child)
    }

    /// Append `child` to the named child-list field `key` (e.g. `effects`,
    /// `markers`), creating it from absent or an empty array. Reparents `child`.
    pub fn push_named_child(
        &mut self,
        parent: NodeId,
        key: &str,
        child: NodeId,
    ) -> Result<(), Error> {
        self.require(parent)?;
        self.require(child)?;
        self.reject_cycle(parent, child)?;
        self.nodes[child].parent = Some(parent);
        let node = &mut self.nodes[parent];
        match node.fields.iter_mut().find(|(k, _)| k == key) {
            Some((_, Field::Children(ids))) => ids.push(child),
            Some((_, field @ Field::Value(_))) => {
                let empty = matches!(field, Field::Value(v) if v.as_array().is_some_and(|a| a.is_empty()));
                if !empty {
                    return Err(Error::Structural(format!(
                        "`{key}` field holds a non-array value"
                    )));
                }
                *field = Field::Children(vec![child]);
            }
            Some(_) => {
                return Err(Error::Structural(format!(
                    "`{key}` field has an unexpected shape"
                )))
            }
            None => node
                .fields
                .push((key.to_string(), Field::Children(vec![child]))),
        }
        Ok(())
    }

    /// Insert a child at `index` in `parent`'s `children` list (clamped).
    ///
    /// Reparents `child`. Creates the `children` field if absent or empty.
    pub fn insert_child(
        &mut self,
        parent: NodeId,
        index: usize,
        child: NodeId,
    ) -> Result<(), Error> {
        self.require(parent)?;
        self.require(child)?;
        self.reject_cycle(parent, child)?;
        self.nodes[child].parent = Some(parent);
        let node = &mut self.nodes[parent];
        match node.fields.iter_mut().find(|(k, _)| k == "children") {
            Some((_, Field::Children(ids))) => {
                ids.insert(index.min(ids.len()), child);
            }
            Some((_, field @ Field::Value(_))) => {
                // Only valid when the existing value is an empty array.
                let is_empty_array = matches!(field, Field::Value(v) if v.as_array().is_some_and(|a| a.is_empty()));
                if !is_empty_array {
                    return Err(Error::Structural(
                        "`children` field holds a non-array value".into(),
                    ));
                }
                *field = Field::Children(vec![child]);
            }
            Some(_) => {
                return Err(Error::Structural(
                    "`children` field has an unexpected shape".into(),
                ));
            }
            None => node
                .fields
                .push(("children".to_string(), Field::Children(vec![child]))),
        }
        Ok(())
    }

    /// Set (or replace) a named entry in `parent`'s child-map field `map_key`
    /// (e.g. a Clip's `media_references` -> `DEFAULT_MEDIA`). Reparents `child`.
    ///
    /// If an existing occupant is replaced, that evicted node is detached
    /// (`parent` cleared) so it does not linger as a phantom child of `parent` —
    /// this keeps `gc` reachability and the undo journal's old-child capture
    /// correct. Replacing an entry with the same `child` is a no-op (no detach).
    pub fn set_child_in_map(
        &mut self,
        parent: NodeId,
        map_key: &str,
        name: &str,
        child: NodeId,
    ) -> Result<(), Error> {
        self.require(parent)?;
        self.require(child)?;
        self.reject_cycle(parent, child)?;
        self.nodes[child].parent = Some(parent);
        // Capture every displaced child under the borrow, detach them after so
        // none is left as a phantom child (stale `parent`) of `parent`.
        let evicted: Vec<NodeId> = {
            let node = &mut self.nodes[parent];
            match node.fields.iter_mut().find(|(k, _)| k == map_key) {
                Some((_, Field::ChildMap(entries))) => {
                    match entries.iter_mut().find(|(n, _)| n == name) {
                        Some(entry) => {
                            let prev = entry.1;
                            entry.1 = child;
                            // Self-replace is a no-op (keep `child` attached).
                            if prev != child { vec![prev] } else { vec![] }
                        }
                        None => {
                            entries.push((name.to_string(), child));
                            vec![]
                        }
                    }
                }
                Some((_, field)) => {
                    // Overwriting a non-`ChildMap` field: detach every child it
                    // linked (other than the one we are installing).
                    let displaced = field
                        .child_ids()
                        .into_iter()
                        .filter(|c| *c != child)
                        .collect();
                    *field = Field::ChildMap(vec![(name.to_string(), child)]);
                    displaced
                }
                None => {
                    node.fields.push((
                        map_key.to_string(),
                        Field::ChildMap(vec![(name.to_string(), child)]),
                    ));
                    vec![]
                }
            }
        };
        self.detach_evicted(&evicted);
        Ok(())
    }

    /// Clear `parent` on every evicted node id so a displaced child is never
    /// left as a phantom child (stale `parent`) of its former holder.
    ///
    /// Single source of truth for the "detach what we displaced" step shared by
    /// [`Document::set_child`] (via [`Document::replace_structural_field`]) and
    /// [`Document::set_child_in_map`], so the two cannot drift apart.
    fn detach_evicted(&mut self, evicted: &[NodeId]) {
        for prev in evicted {
            if let Some(n) = self.nodes.get_mut(*prev) {
                n.parent = None;
            }
        }
    }

    /// Replace (or create) `parent`'s field `key` with `new_field`, detaching any
    /// node previously linked under `key` that `new_field` does not also link.
    ///
    /// The shared whole-field overwrite path: it guarantees a displaced child's
    /// `parent` is cleared, which keeps `gc` reachability and the undo journal's
    /// old-child capture correct. (`set_child_in_map` does its own per-entry
    /// merge for an existing `ChildMap`, but still routes the parent-clearing
    /// through [`Document::detach_evicted`].)
    fn replace_structural_field(&mut self, parent: NodeId, key: &str, new_field: Field) {
        let keep = new_field.child_ids();
        let evicted: Vec<NodeId> = {
            let node = &mut self.nodes[parent];
            match node.fields.iter_mut().find(|(k, _)| k == key) {
                Some((_, field)) => {
                    let evicted = field
                        .child_ids()
                        .into_iter()
                        .filter(|c| !keep.contains(c))
                        .collect();
                    *field = new_field;
                    evicted
                }
                None => {
                    node.fields.push((key.to_string(), new_field));
                    Vec::new()
                }
            }
        };
        self.detach_evicted(&evicted);
    }

    /// Remove the child at `index` from `parent`'s `children` list.
    ///
    /// Returns the removed node id (now detached, parent cleared), or `None`.
    pub fn remove_child(&mut self, parent: NodeId, index: usize) -> Option<NodeId> {
        let removed = {
            let node = self.nodes.get_mut(parent)?;
            match node.fields.iter_mut().find(|(k, _)| k == "children") {
                Some((_, Field::Children(ids))) if index < ids.len() => ids.remove(index),
                _ => return None,
            }
        };
        if let Some(n) = self.nodes.get_mut(removed) {
            n.parent = None;
        }
        Some(removed)
    }

    /// Unlink `child` from any of `parent`'s structural fields (`Child`,
    /// `Children`, `ChildMap`), without touching `child.parent`.
    ///
    /// A `Child` field that held exactly `child` is dropped entirely; `Children`
    /// / `ChildMap` lose the matching entry. Returns whether a link was removed.
    /// Shared by [`Document::detach_from_parent`].
    fn unlink_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        let Some(p) = self.nodes.get_mut(parent) else {
            return false;
        };
        let mut found = false;
        // Exhaustive match (no wildcard) so a future `Field` variant is a
        // compile error here rather than a silently un-unlinked link.
        p.fields.retain_mut(|(_, field)| match field {
            Field::Child(c) if *c == child => {
                found = true;
                false // drop the single-child field
            }
            Field::Child(_) => true,
            Field::Children(ids) => {
                let before = ids.len();
                ids.retain(|c| *c != child);
                found |= ids.len() != before;
                true
            }
            Field::ChildMap(entries) => {
                let before = entries.len();
                entries.retain(|(_, c)| *c != child);
                found |= entries.len() != before;
                true
            }
            Field::Value(_) => true,
        });
        found
    }

    /// Detach `id` from its parent: clear `id.parent` and unlink it from the
    /// parent's field that referenced it. The node and its `NodeId` stay alive.
    ///
    /// This is the undo-safe inverse of a create-and-attach: it removes the link
    /// without freeing the slot, so the same `NodeId` survives undo/redo (a true
    /// removal would bump the slot generation and invalidate journal entries and
    /// language-binding handles). Returns `false` if `id` is unknown or had no
    /// parent. Reachability cleanup of the now-orphaned node is [`Document::gc`].
    pub fn detach_from_parent(&mut self, id: NodeId) -> bool {
        let Some(parent) = self.nodes.get(id).and_then(|n| n.parent) else {
            return false;
        };
        let unlinked = self.unlink_child(parent, id);
        if let Some(n) = self.nodes.get_mut(id) {
            n.parent = None;
        }
        unlinked
    }

    /// Permanently free a **detached leaf** node, releasing its arena slot.
    ///
    /// Errors unless `id` has no parent and no child-type field with entries —
    /// freeing a node that is still linked would leave dangling `NodeId`s in some
    /// parent's child list. Callers free a subtree bottom-up after detaching, or
    /// rely on [`Document::gc`]. This is the only true node removal; it is run
    /// outside the undo horizon (see PLAN.md), never as an undo primitive.
    pub fn remove_node(&mut self, id: NodeId) -> Result<(), Error> {
        let node = self
            .nodes
            .get(id)
            .ok_or_else(|| Error::Structural("remove_node: node id not found".into()))?;
        if node.parent.is_some() {
            return Err(Error::Structural(
                "remove_node: node still has a parent".into(),
            ));
        }
        let has_children = node
            .fields
            .iter()
            .any(|(_, f)| !f.child_ids().is_empty());
        if has_children {
            return Err(Error::Structural(
                "remove_node: node still has children".into(),
            ));
        }
        self.nodes.remove(id);
        if self.root == Some(id) {
            self.root = None;
        }
        Ok(())
    }

    /// Remove and return the field at `index` (key + value), preserving the
    /// order of the survivors. `None` if `node`/`index` is out of range.
    ///
    /// Paired with [`Document::insert_field_at`] for order-preserving field
    /// undo/redo: `Node::set_value` re-appends a re-added field at the end, which
    /// would break the lossless source order — the journal restores the original
    /// index instead.
    pub fn remove_field_at(&mut self, node: NodeId, index: usize) -> Option<(String, Field)> {
        let n = self.nodes.get_mut(node)?;
        (index < n.fields.len()).then(|| n.fields.remove(index))
    }

    /// Insert a field at `index` (clamped to the field count). Returns `false`
    /// if `node` is unknown. See [`Document::remove_field_at`].
    pub fn insert_field_at(
        &mut self,
        node: NodeId,
        index: usize,
        key: String,
        field: Field,
    ) -> bool {
        let Some(n) = self.nodes.get_mut(node) else {
            return false;
        };
        let idx = index.min(n.fields.len());
        n.fields.insert(idx, (key, field));
        true
    }

    /// Mark-and-sweep garbage collection: free every node that is neither
    /// reachable from `root` nor `pinned`, returning the number freed.
    ///
    /// `pinned` is the set of `NodeId`s the caller must keep alive even though
    /// they are currently detached — for the editor that is every id referenced
    /// by the undo/redo stacks, so an undo can re-attach a detached node with its
    /// id intact. **Pinned nodes act as additional GC roots**: a pinned node AND
    /// its whole subtree (media references, markers, effects) are retained, so an
    /// undo re-attaches a structurally complete subtree. Run outside the undo
    /// horizon (on save / history flush), never mid-edit.
    pub fn gc(&mut self, pinned: &HashSet<NodeId>) -> usize {
        // Mark phase: reachable from the document root OR from any pinned node.
        let mut reachable: HashSet<NodeId> = HashSet::new();
        let mut stack: Vec<NodeId> = self.root.into_iter().collect();
        stack.extend(pinned.iter().copied());
        while let Some(id) = stack.pop() {
            if !reachable.insert(id) {
                continue;
            }
            if let Some(n) = self.nodes.get(id) {
                for (_, field) in &n.fields {
                    stack.extend(field.child_ids());
                }
            }
        }
        // Sweep phase: free everything unmarked.
        let doomed: Vec<NodeId> = self
            .nodes
            .keys()
            .filter(|id| !reachable.contains(id))
            .collect();
        for id in &doomed {
            self.nodes.remove(*id);
        }
        doomed.len()
    }

    /// The number of children in `parent`'s `children` list (0 if absent/empty).
    fn children_len(&self, parent: NodeId) -> usize {
        self.nodes
            .get(parent)
            .map(|n| n.children("children").len())
            .unwrap_or(0)
    }

    /// Error unless `id` resolves.
    fn require(&self, id: NodeId) -> Result<(), Error> {
        if self.nodes.contains_key(id) {
            Ok(())
        } else {
            Err(Error::Structural("node id not found".into()))
        }
    }

    /// Whether `ancestor` is `node` itself or appears on `node`'s parent chain.
    ///
    /// Walks `parent` links upward, bounded by [`MAX_DEPTH`] so a pre-existing
    /// corrupt cycle cannot loop forever — if the cap is hit it conservatively
    /// reports `true`, refusing the operation rather than spinning.
    fn is_ancestor(&self, ancestor: NodeId, node: NodeId) -> bool {
        let mut cur = Some(node);
        let mut steps = 0;
        while let Some(id) = cur {
            if id == ancestor {
                return true;
            }
            if steps >= MAX_DEPTH {
                return true;
            }
            steps += 1;
            cur = self.nodes.get(id).and_then(|n| n.parent);
        }
        false
    }

    /// Reject reparenting `child` under `parent` when `child` is `parent` or an
    /// ancestor of `parent` — that would close a cycle and make the recursive
    /// walks (`emit`, `deep_clone`) loop without end. This cheap guard at every
    /// reparenting entry point is what keeps the arena a forest by construction.
    fn reject_cycle(&self, parent: NodeId, child: NodeId) -> Result<(), Error> {
        if self.is_ancestor(child, parent) {
            return Err(Error::Structural(
                "reparenting would create a cycle (child is an ancestor of parent)".into(),
            ));
        }
        Ok(())
    }

    /// The number of nodes in the arena.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the arena is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Parse an OTIO document from a JSON string.
    pub fn from_json_str(s: &str) -> Result<Document, Error> {
        let value: Value = serde_json::from_str(s)?;
        let obj = value.as_object().ok_or(Error::NotAnObject)?;
        let mut doc = Document::new();
        let root = doc.build_node(obj, None, 0)?;
        doc.root = Some(root);
        Ok(doc)
    }

    /// Serialize this document to a pretty-printed JSON string.
    pub fn to_json_string(&self) -> Result<String, Error> {
        let root = self.root.ok_or(Error::Empty)?;
        let value = self.emit(root, 0)?;
        Ok(serde_json::to_string_pretty(&value)?)
    }

    /// Serialize an arbitrary node and its subtree to a pretty-printed JSON
    /// string (the node is emitted as the top-level object).
    ///
    /// This backs the Python `serialize_json_to_string(obj)`, which serializes
    /// any serializable object — not only a document's root.
    pub fn to_json_string_at(&self, id: NodeId) -> Result<String, Error> {
        if !self.nodes.contains_key(id) {
            return Err(Error::Structural("node id not found".into()));
        }
        let value = self.emit(id, 0)?;
        Ok(serde_json::to_string_pretty(&value)?)
    }

    /// Recursively build a node from a JSON object, returning its id.
    ///
    /// `depth` bounds nesting at [`MAX_DEPTH`] so a hostile or corrupt file with
    /// pathological nesting fails with an error rather than overflowing the
    /// native stack.
    fn build_node(
        &mut self,
        obj: &Map<String, Value>,
        parent: Option<NodeId>,
        depth: usize,
    ) -> Result<NodeId, Error> {
        if depth >= MAX_DEPTH {
            return Err(Error::Structural(format!(
                "document nesting exceeds the {MAX_DEPTH}-level depth cap"
            )));
        }
        let tag = obj
            .get("OTIO_SCHEMA")
            .and_then(|v| v.as_str())
            .ok_or(Error::MissingSchema)?;
        let (name, version) = parse_schema(tag)?;

        let id = self.nodes.insert(Node {
            schema_name: name,
            schema_version: version,
            parent,
            fields: Vec::new(),
        });

        let mut fields = Vec::with_capacity(obj.len());
        for (key, val) in obj {
            if key == "OTIO_SCHEMA" {
                continue;
            }
            let field = self.classify(val, id, key, depth)?;
            fields.push((key.clone(), field));
        }
        self.nodes[id].fields = fields;
        Ok(id)
    }

    /// Decide whether a field value is a structural child link or a plain value.
    ///
    /// `key` is the field name and `depth` the holder node's depth. Fields in
    /// [`OPAQUE_KEYS`] (`metadata`, `parameters`) stay verbatim [`Field::Value`]
    /// even if they embed `SerializableObject`s: those are opaque dictionary
    /// contents, not structural children, and promoting them would corrupt
    /// `Node::metadata()` and the clone / gc / detach traversals.
    fn classify(
        &mut self,
        value: &Value,
        parent: NodeId,
        key: &str,
        depth: usize,
    ) -> Result<Field, Error> {
        if OPAQUE_KEYS.contains(&key) {
            return Ok(Field::Value(value.clone()));
        }
        match value {
            Value::Object(o) if is_node_object(o) => {
                Ok(Field::Child(self.build_node(o, Some(parent), depth + 1)?))
            }
            // A map whose values are all node-objects (e.g. Clip.2 media_references).
            // Checked after the single-node case so a node-object isn't treated as a map.
            Value::Object(o)
                if !o.is_empty()
                    && o.values().all(|v| v.as_object().is_some_and(is_node_object)) =>
            {
                let mut entries = Vec::with_capacity(o.len());
                for (k, v) in o {
                    let child = v.as_object().expect("checked by all()");
                    entries.push((k.clone(), self.build_node(child, Some(parent), depth + 1)?));
                }
                Ok(Field::ChildMap(entries))
            }
            Value::Array(a)
                if !a.is_empty()
                    && a.iter().all(|e| e.as_object().is_some_and(is_node_object)) =>
            {
                let mut ids = Vec::with_capacity(a.len());
                for e in a {
                    let o = e.as_object().expect("checked by all()");
                    ids.push(self.build_node(o, Some(parent), depth + 1)?);
                }
                Ok(Field::Children(ids))
            }
            _ => Ok(Field::Value(value.clone())),
        }
    }

    /// Recursively re-emit a node as JSON, restoring `OTIO_SCHEMA` and order.
    ///
    /// `depth` bounds nesting at [`MAX_DEPTH`]: cycle prevention at the
    /// reparenting entry points already keeps the arena a forest, but a
    /// programmatically built tree deeper than the cap would still overflow the
    /// stack — this errors via the writer `Result` instead.
    fn emit(&self, id: NodeId, depth: usize) -> Result<Value, Error> {
        if depth >= MAX_DEPTH {
            return Err(Error::Structural(format!(
                "serialization nesting exceeds the {MAX_DEPTH}-level depth cap"
            )));
        }
        let node = &self.nodes[id];
        let mut map = Map::new();
        map.insert(
            "OTIO_SCHEMA".to_string(),
            Value::String(format!("{}.{}", node.schema_name, node.schema_version)),
        );
        for (key, field) in &node.fields {
            let value = match field {
                Field::Value(v) => v.clone(),
                Field::Child(cid) => self.emit(*cid, depth + 1)?,
                Field::Children(ids) => {
                    let mut arr = Vec::with_capacity(ids.len());
                    for i in ids {
                        arr.push(self.emit(*i, depth + 1)?);
                    }
                    Value::Array(arr)
                }
                Field::ChildMap(entries) => {
                    let mut m = Map::new();
                    for (k, id) in entries {
                        m.insert(k.clone(), self.emit(*id, depth + 1)?);
                    }
                    Value::Object(m)
                }
            };
            map.insert(key.clone(), value);
        }
        Ok(Value::Object(map))
    }
}

/// Whether a JSON object is a `SerializableObject` (a tree node), i.e. it has an
/// `OTIO_SCHEMA` tag whose schema is not one of the inline value types.
fn is_node_object(o: &Map<String, Value>) -> bool {
    o.get("OTIO_SCHEMA")
        .and_then(|v| v.as_str())
        .is_some_and(|tag| {
            let name = tag.rsplit_once('.').map_or(tag, |(n, _)| n);
            !is_value_schema(name)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &str = r#"{
      "OTIO_SCHEMA": "Timeline.1",
      "metadata": {},
      "name": "t",
      "tracks": {
        "OTIO_SCHEMA": "Stack.1",
        "children": [
          {
            "OTIO_SCHEMA": "Track.1",
            "children": [],
            "kind": "Video",
            "metadata": {},
            "name": "T1",
            "source_range": null
          }
        ],
        "metadata": {},
        "name": "tracks"
      }
    }"#;

    #[test]
    fn builds_tree_and_navigates() {
        let doc = Document::from_json_str(SIMPLE).unwrap();
        let root = doc.root().unwrap();
        assert_eq!(root.schema_name(), "Timeline");
        assert_eq!(root.schema_version(), 1);
        let stack = doc.node(root.child("tracks").unwrap()).unwrap();
        assert_eq!(stack.schema_name(), "Stack");
        assert_eq!(stack.children("children").len(), 1);
        let track = doc.node(stack.children("children")[0]).unwrap();
        assert_eq!(track.schema_name(), "Track");
    }

    #[test]
    fn roundtrip_is_lossless() {
        let v0: Value = serde_json::from_str(SIMPLE).unwrap();
        let doc = Document::from_json_str(SIMPLE).unwrap();
        let out = doc.to_json_string().unwrap();
        let v1: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v0, v1);
    }

    #[test]
    fn build_timeline_from_scratch() {
        let mut doc = Document::new();
        let tl = doc.create_node("Timeline", 1);
        doc.set_root(tl);
        let stack = doc.create_node("Stack", 1);
        doc.set_child(tl, "tracks", stack).unwrap();
        let track = doc.create_node("Track", 1);
        doc.node_mut(track).unwrap().set_value("kind", serde_json::json!("Video"));
        doc.append_child(stack, track).unwrap();
        let clip = doc.create_node("Clip", 1);
        doc.node_mut(clip).unwrap().set_value("name", serde_json::json!("C1"));
        doc.append_child(track, clip).unwrap();

        // Structure is navigable and parent pointers are wired.
        assert_eq!(doc.root().unwrap().schema_name(), "Timeline");
        let s = doc.node(tl).unwrap().child("tracks").unwrap();
        let t = doc.node(s).unwrap().children("children")[0];
        let c = doc.node(t).unwrap().children("children")[0];
        assert_eq!(doc.node(c).unwrap().parent(), Some(t));
        assert_eq!(doc.node(t).unwrap().parent(), Some(s));

        // It serializes and re-parses without loss of structure.
        let out = doc.to_json_string().unwrap();
        let doc2 = Document::from_json_str(&out).unwrap();
        assert_eq!(doc2.root().unwrap().schema_name(), "Timeline");
    }

    #[test]
    fn deep_clone_is_independent() {
        let mut doc = Document::from_json_str(SIMPLE).unwrap();
        let root = doc.root_id().unwrap();
        let clone = doc.deep_clone(root).unwrap();
        assert_ne!(clone, root);
        assert_eq!(doc.node(clone).unwrap().schema_name(), "Timeline");

        // Mutating the clone must not touch the original.
        doc.node_mut(clone).unwrap().set_value("name", serde_json::json!("Clone"));
        let Some(Field::Value(v)) = doc.node(root).unwrap().field("name") else {
            panic!("original name field missing");
        };
        assert_eq!(v.as_str(), Some("t"));
    }

    #[test]
    fn adopt_from_copies_subtree_with_transfer_map() {
        // Source: a small subtree in its own staging document.
        let src = Document::from_json_str(SIMPLE).unwrap();
        let src_root = src.root_id().unwrap();
        let src_count = src.len();

        // Target: an empty document adopts the whole subtree.
        let mut dst = Document::new();
        let (new_root, map) = dst.adopt_from(&src, src_root).unwrap();

        // Every source node appears in the map exactly once, root first.
        assert_eq!(map.len(), src_count);
        assert_eq!(map[0], (src_root, new_root));
        assert_eq!(dst.len(), src_count);

        // The new root is detached and parent pointers are rewired locally.
        assert_eq!(dst.node(new_root).unwrap().parent(), None);
        let stack = dst.node(new_root).unwrap().child("tracks").unwrap();
        assert_eq!(dst.node(stack).unwrap().parent(), Some(new_root));

        // Identical JSON, proving a faithful copy across the arena boundary.
        dst.set_root(new_root);
        let a: Value = serde_json::from_str(&src.to_json_string().unwrap()).unwrap();
        let b: Value = serde_json::from_str(&dst.to_json_string().unwrap()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn detach_unlinks_and_keeps_id_alive() {
        let mut doc = Document::from_json_str(SIMPLE).unwrap();
        let stack = doc.root().unwrap().child("tracks").unwrap();
        let track = doc.node(stack).unwrap().children("children")[0];

        // Detaching unlinks the track from the stack but keeps the node + id.
        assert!(doc.detach_from_parent(track));
        assert_eq!(doc.node(stack).unwrap().children("children").len(), 0);
        assert!(doc.node(track).is_some());
        assert_eq!(doc.node(track).unwrap().parent(), None);
        // Detaching again is a no-op (no parent left).
        assert!(!doc.detach_from_parent(track));
    }

    #[test]
    fn remove_node_is_leaf_only() {
        let mut doc = Document::from_json_str(SIMPLE).unwrap();
        let root = doc.root_id().unwrap();
        let stack = doc.node(root).unwrap().child("tracks").unwrap();

        // The stack still has a parent -> refused.
        assert!(doc.remove_node(stack).is_err());
        // The root has a child (tracks) -> refused even though it has no parent.
        assert!(doc.remove_node(root).is_err());

        // Detach a leaf track, then it can be freed; the slot is gone.
        let track = doc.node(stack).unwrap().children("children")[0];
        assert!(doc.detach_from_parent(track));
        assert!(doc.remove_node(track).is_ok());
        assert!(doc.node(track).is_none());
    }

    #[test]
    fn field_at_helpers_preserve_order() {
        let mut doc = Document::new();
        let n = doc.create_node("Clip", 2);
        doc.node_mut(n).unwrap().set_value("a", serde_json::json!(1));
        doc.node_mut(n).unwrap().set_value("b", serde_json::json!(2));
        doc.node_mut(n).unwrap().set_value("c", serde_json::json!(3));

        // Remove the middle field, capture it, then restore at the same index.
        let (key, field) = doc.remove_field_at(n, 1).unwrap();
        assert_eq!(key, "b");
        let keys: Vec<_> = doc
            .node(n)
            .unwrap()
            .fields()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(keys, ["a", "c"]);

        assert!(doc.insert_field_at(n, 1, key, field));
        let keys: Vec<_> = doc
            .node(n)
            .unwrap()
            .fields()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(keys, ["a", "b", "c"]); // original order restored
    }

    #[test]
    fn gc_frees_orphans_keeps_reachable_and_pinned() {
        let mut doc = Document::from_json_str(SIMPLE).unwrap();
        let before = doc.len();
        let stack = doc.root().unwrap().child("tracks").unwrap();
        let track = doc.node(stack).unwrap().children("children")[0];

        // Detach the track: now an orphan (unreachable from root), still present.
        doc.detach_from_parent(track);
        assert_eq!(doc.len(), before);

        // gc with the orphan pinned: nothing freed (the editor pins undo refs).
        let mut pinned = HashSet::new();
        pinned.insert(track);
        assert_eq!(doc.gc(&pinned), 0);
        assert!(doc.node(track).is_some());

        // gc with nothing pinned: the orphan is swept.
        assert_eq!(doc.gc(&HashSet::new()), 1);
        assert!(doc.node(track).is_none());
        // Reachable nodes are untouched.
        assert!(doc.root().is_some());
        assert!(doc.node(stack).is_some());
    }

    #[test]
    fn gc_retains_pinned_node_subtree() {
        // Pinning a node must keep its whole subtree alive (pinned ids are GC
        // roots): a detached clip's media reference survives because the clip is
        // pinned, even though the media ref id itself is not in `pinned`.
        let mut doc = Document::new();
        let root = doc.create_node("Stack", 1);
        doc.set_root(root);
        let clip = doc.create_node("Clip", 2);
        let mref = doc.create_node("MissingReference", 1);
        doc.set_child_in_map(clip, "media_references", "DEFAULT_MEDIA", mref)
            .unwrap();
        doc.append_child(root, clip).unwrap();

        doc.detach_from_parent(clip); // orphan the clip subtree
        let mut pinned = HashSet::new();
        pinned.insert(clip); // pin only the clip, not its media ref
        assert_eq!(doc.gc(&pinned), 0, "pinned clip + its media ref must survive");
        assert!(doc.node(clip).is_some());
        assert!(doc.node(mref).is_some());

        // Unpinned: the whole orphan subtree is swept (clip + media ref).
        assert_eq!(doc.gc(&HashSet::new()), 2);
        assert!(doc.node(clip).is_none());
        assert!(doc.node(mref).is_none());
    }

    #[test]
    fn set_child_in_map_detaches_evicted() {
        let mut doc = Document::new();
        let clip = doc.create_node("Clip", 2);
        doc.set_root(clip);
        let ref_a = doc.create_node("MissingReference", 1);
        let ref_b = doc.create_node("ExternalReference", 1);

        doc.set_child_in_map(clip, "media_references", "DEFAULT_MEDIA", ref_a)
            .unwrap();
        assert_eq!(doc.node(ref_a).unwrap().parent(), Some(clip));

        // Replacing the entry detaches the evicted occupant.
        doc.set_child_in_map(clip, "media_references", "DEFAULT_MEDIA", ref_b)
            .unwrap();
        assert_eq!(doc.node(ref_b).unwrap().parent(), Some(clip));
        assert_eq!(doc.node(ref_a).unwrap().parent(), None);

        // Re-setting the same child is a no-op (stays attached).
        doc.set_child_in_map(clip, "media_references", "DEFAULT_MEDIA", ref_b)
            .unwrap();
        assert_eq!(doc.node(ref_b).unwrap().parent(), Some(clip));
    }

    #[test]
    fn set_child_replace_detaches_evicted() {
        // Overwriting a single-child key must clear the displaced node's parent,
        // else it lingers as a phantom child (the CLASS E integrity fix). Mirror
        // of `set_child_in_map_detaches_evicted` for the single-child path.
        let mut doc = Document::new();
        let parent = doc.create_node("Timeline", 1);
        doc.set_root(parent);
        let a = doc.create_node("Stack", 1);
        let b = doc.create_node("Stack", 1);

        doc.set_child(parent, "tracks", a).unwrap();
        assert_eq!(doc.node(a).unwrap().parent(), Some(parent));

        // Replace a with b under the same key: a is evicted -> detached.
        doc.set_child(parent, "tracks", b).unwrap();
        assert_eq!(doc.node(b).unwrap().parent(), Some(parent));
        assert_eq!(doc.node(a).unwrap().parent(), None);
        assert_eq!(doc.node(parent).unwrap().child("tracks"), Some(b));

        // Re-setting the same child is a no-op (stays attached, not self-detached).
        doc.set_child(parent, "tracks", b).unwrap();
        assert_eq!(doc.node(b).unwrap().parent(), Some(parent));
    }

    #[test]
    fn set_child_rejects_cycle() {
        // a -> b (a is b's ancestor). Reparenting a (or b) under b closes a cycle
        // that would make emit/deep_clone recurse forever -> rejected at source.
        let mut doc = Document::new();
        let a = doc.create_node("Stack", 1);
        doc.set_root(a);
        let b = doc.create_node("Stack", 1);
        doc.set_child(a, "child", b).unwrap();

        // child==parent and ancestor-under-descendant are both refused.
        assert!(matches!(doc.set_child(b, "self", b), Err(Error::Structural(_))));
        assert!(matches!(doc.set_child(b, "up", a), Err(Error::Structural(_))));
        // Guard had no side effects: links unchanged.
        assert_eq!(doc.node(a).unwrap().child("child"), Some(b));
        assert_eq!(doc.node(b).unwrap().parent(), Some(a));

        // A normal non-cyclic replace still succeeds (guard not over-eager).
        let c = doc.create_node("Clip", 1);
        doc.set_child(a, "child", c).unwrap();
        assert_eq!(doc.node(a).unwrap().child("child"), Some(c));
        assert_eq!(doc.node(c).unwrap().parent(), Some(a));
        assert_eq!(doc.node(b).unwrap().parent(), None); // b evicted cleanly
    }

    #[test]
    fn classify_keeps_opaque_metadata_verbatim() {
        // `metadata` holding a nested node-object must stay a verbatim Value, not
        // be promoted to a structural child — else metadata() returns empty and
        // the embedded object is treated as a real child (CLASS E fix).
        const SRC: &str = r#"{
          "OTIO_SCHEMA": "Clip.1",
          "metadata": { "fake": { "OTIO_SCHEMA": "Marker.1", "name": "inside" } },
          "name": "c"
        }"#;
        let doc = Document::from_json_str(SRC).unwrap();
        let clip = doc.root_id().unwrap();

        // Stored verbatim, not promoted to Child / ChildMap / Children.
        assert!(matches!(
            doc.node(clip).unwrap().field("metadata"),
            Some(Field::Value(_))
        ));
        // metadata() still returns the dict (not empty).
        let meta = doc.node(clip).unwrap().metadata().to_json_value();
        assert!(meta.as_object().is_some_and(|m| !m.is_empty()));

        // The whole doc still round-trips losslessly (DOM-equal).
        let v0: Value = serde_json::from_str(SRC).unwrap();
        let v1: Value = serde_json::from_str(&doc.to_json_string().unwrap()).unwrap();
        assert_eq!(v0, v1);
    }

    #[test]
    fn deep_tree_clones_and_emits_under_cap() {
        // A legitimately deep (but sub-MAX_DEPTH) chain must clone + emit cleanly:
        // the depth cap must not false-trip and the cycle guard's ancestor-walk
        // must not reject ordinary appends. (Over-MAX_DEPTH abort is NOT tested
        // here — see `.bughunt/test_core.md`.)
        let mut doc = Document::new();
        let root = doc.create_node("Stack", 1);
        doc.set_root(root);
        let mut cur = root;
        for _ in 0..256 {
            let n = doc.create_node("Stack", 1);
            doc.set_child(cur, "child", n).unwrap();
            cur = n;
        }
        let clone = doc.deep_clone(root).unwrap(); // recursive clone, no overflow
        assert_ne!(clone, root);
        assert!(doc.to_json_string().is_ok()); // emit succeeds under the cap
    }
}
