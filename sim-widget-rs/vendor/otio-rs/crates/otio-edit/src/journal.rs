//! The undo journal: a list of reversible primitive mutations.
//!
//! Every edit operation records the primitives it applies into a
//! [`Transaction`]; undo reverse-iterates applying each primitive's inverse,
//! redo re-applies forward. Each primitive captures BOTH its before- and
//! after-state so it is reversible in either direction without re-running the
//! op. Node ids are never freed by undo (see [`Prim::Created`]) — they are
//! detached and kept alive so the same `NodeId` survives undo/redo, which is the
//! identity contract the widget and language bindings rely on (see PLAN.md).

use otio_core::{Document, Field, NodeId};

/// The reversible state of one field (its index + value), or absence.
///
/// `None` = the field is not present; `Some((index, field))` = present at that
/// position. Restoring a field to its captured index preserves the lossless
/// source order (a plain re-add would append at the end).
pub type FieldState = Option<(usize, Field)>;

/// One reversible primitive mutation.
#[derive(Debug, Clone)]
pub enum Prim {
    /// A node was created in the arena. This primitive is a PIN MARKER only: it
    /// keeps the `NodeId` alive against `gc` (and reachable as a gc-root with its
    /// subtree) while it sits in the undo/redo stacks. Both undo and redo are
    /// no-ops structurally — attachment/detachment is ALWAYS performed by the
    /// attach primitives (`ChildInserted`/`ChildSet`/`MapChildSet`), whose
    /// inverses detach the whole created subtree as a unit (keeping its internal
    /// links intact). Detaching per-created-node here would instead tear apart a
    /// cloned subtree's internal structure, which redo could not rebuild.
    Created(NodeId),

    /// A *value* field `key` on `node` changed from `before` to `after`.
    ///
    /// Invariant: `FieldSet` is only ever produced for value fields (via
    /// `Tx::set_value` -> `Node::set_value`, which replaces in place or appends).
    /// `before`/`after` are captured by index AFTER the write, so the index is
    /// always current. Do NOT reuse `FieldSet` to rearrange a structural
    /// (`Child`/`Children`/`ChildMap`) field — mixing field insertion/removal
    /// with structural reorder in one transaction could restore a wrong index.
    FieldSet {
        /// The node whose field changed.
        node: NodeId,
        /// The field key.
        key: String,
        /// State before the edit (for undo).
        before: FieldState,
        /// State after the edit (for redo).
        after: FieldState,
    },

    /// A child was inserted into `parent`'s `children` list at `index`.
    ChildInserted {
        /// The composition.
        parent: NodeId,
        /// Insertion index.
        index: usize,
        /// The inserted child.
        child: NodeId,
    },

    /// A child was removed from `parent`'s `children` list at `index`.
    ChildRemoved {
        /// The composition.
        parent: NodeId,
        /// The index the child occupied.
        index: usize,
        /// The removed child.
        child: NodeId,
    },

    /// A single-child field `key` on `parent` changed occupant.
    ChildSet {
        /// The parent.
        parent: NodeId,
        /// The single-child field key (e.g. `media_reference`).
        key: String,
        /// Prior occupant (for undo).
        before: Option<NodeId>,
        /// New occupant (for redo).
        after: Option<NodeId>,
    },

    /// A named entry in `parent`'s child-map `map_key` changed occupant.
    MapChildSet {
        /// The parent.
        parent: NodeId,
        /// The map field key (e.g. `media_references`).
        map_key: String,
        /// The entry name (e.g. `DEFAULT_MEDIA`).
        name: String,
        /// Prior occupant (for undo).
        before: Option<NodeId>,
        /// New occupant (for redo).
        after: Option<NodeId>,
    },

    /// A child was pushed onto a NAMED child-list field `key` (e.g. `markers`,
    /// `effects`) at `index`. This is the general analogue of `ChildInserted`,
    /// which is hard-wired to the `"children"` composition list; `markers` lives
    /// in a separate `Field::Children` named `markers`, so it needs its own prim.
    /// Symmetric to [`Prim::NamedChildRemoved`]: undo removes the entry, redo
    /// re-pushes it at the same index (keeping the node + its id alive between).
    NamedChildPushed {
        /// The item carrying the named list.
        parent: NodeId,
        /// The named child-list field key (e.g. `markers`).
        key: String,
        /// The index the child occupies in the list.
        index: usize,
        /// The pushed child.
        child: NodeId,
        /// The field's EXACT state before the push, so undo restores it
        /// precisely — including ABSENCE (a push that created the field from a
        /// node that had no `markers` key must, on undo, remove the field, not
        /// leave an empty `[]`; same class as the `MapChildSet` empty-map fix).
        before: FieldState,
    },

    /// A child was removed from a NAMED child-list field `key` at `index` (the
    /// inverse direction of [`Prim::NamedChildPushed`]).
    NamedChildRemoved {
        /// The item carrying the named list.
        parent: NodeId,
        /// The named child-list field key (e.g. `markers`).
        key: String,
        /// The index the child occupied in the list.
        index: usize,
        /// The removed child.
        child: NodeId,
    },
}

/// The current children of `parent`'s named child-list field `key` (empty if the
/// field is absent or holds an empty `[]` value rather than a `Children` list).
fn named_children(doc: &Document, parent: NodeId, key: &str) -> Vec<NodeId> {
    doc.node(parent)
        .map(|n| n.children(key).to_vec())
        .unwrap_or_default()
}

/// Insert `child` into `parent`'s named child-list field `key` at `index`
/// (clamped), reparenting it. Shared by the redo of `NamedChildPushed` and the
/// undo of `NamedChildRemoved` — it restores the *exact* captured position.
///
/// Built only from public otio-core APIs (no peeking at `Node` internals):
/// `push_named_child` appends, so to land at `index` we detach the tail
/// `[index..]`, append `child`, then re-append the saved tail. For the common
/// append case (`index == len`) this collapses to a single `push_named_child`.
fn insert_named_child(doc: &mut Document, parent: NodeId, key: &str, index: usize, child: NodeId) {
    let cur = named_children(doc, parent, key);
    let at = index.min(cur.len());
    // Detach the tail that must follow `child`, in reverse so each detach leaves
    // the earlier ones in place.
    let tail: Vec<NodeId> = cur[at..].to_vec();
    for &c in tail.iter().rev() {
        doc.detach_from_parent(c);
    }
    // `push_named_child` only errors if `key` holds a non-array `Value`, which
    // cannot occur during well-formed journal replay (the field is always a
    // `Children` list or absent here). Surface a corrupted-journal invariant in
    // dev rather than silently dropping the error.
    for &c in std::iter::once(&child).chain(tail.iter()) {
        if doc.push_named_child(parent, key, c).is_err() {
            debug_assert!(false, "insert_named_child: push failed (journal corruption)");
        }
    }
}

/// Remove the child at `index` from `parent`'s named child-list field `key`,
/// detaching it (parent cleared). If the list becomes empty the field is reset to
/// an empty `[]` value (the lossless canonical form for an empty
/// `markers`/`effects`), restoring the pre-edit shape so undo is byte-identical.
/// Shared by the undo of `NamedChildPushed` and the redo of `NamedChildRemoved`.
fn remove_named_child(doc: &mut Document, parent: NodeId, key: &str, index: usize) {
    let cur = named_children(doc, parent, key);
    let Some(&child) = cur.get(index) else {
        return;
    };
    doc.detach_from_parent(child);
    // `detach_from_parent` leaves an empty `Children` list behind; an empty named
    // list must serialize as `[]` (a `Value`), so normalize the canonical shape.
    if cur.len() == 1
        && let Some(n) = doc.node_mut(parent)
    {
        n.set_value(key, serde_json::Value::Array(Vec::new()));
    }
}

/// Restore field `key` on `node` to `state` (remove-if-present then insert at
/// the captured index). Order-preserving; the single point of field-state
/// reconstruction shared by undo and redo.
fn set_field_state(doc: &mut Document, node: NodeId, key: &str, state: &FieldState) {
    if let Some(i) = doc
        .node(node)
        .and_then(|n| n.fields().iter().position(|(k, _)| k == key))
    {
        doc.remove_field_at(node, i);
    }
    if let Some((idx, field)) = state {
        doc.insert_field_at(node, *idx, key.to_string(), field.clone());
    }
}

/// Set a single-child field `key` on `parent` to `occupant` (or remove the field
/// and detach the current child when `occupant` is `None`).
fn set_child_state(doc: &mut Document, parent: NodeId, key: &str, occupant: Option<NodeId>) {
    match occupant {
        Some(child) => {
            let _ = doc.set_child(parent, key, child);
        }
        None => {
            // Detach whatever single child currently occupies `key`, then drop it.
            if let Some(cur) = doc.node(parent).and_then(|n| n.child(key)) {
                doc.detach_from_parent(cur);
            }
            if let Some(i) = doc
                .node(parent)
                .and_then(|n| n.fields().iter().position(|(k, _)| k == key))
            {
                doc.remove_field_at(parent, i);
            }
        }
    }
}

/// Set the map entry `(map_key,name)` on `parent` to `occupant` (or remove the
/// entry and detach the current child when `occupant` is `None`).
fn set_map_child_state(
    doc: &mut Document,
    parent: NodeId,
    map_key: &str,
    name: &str,
    occupant: Option<NodeId>,
) {
    match occupant {
        Some(child) => {
            let _ = doc.set_child_in_map(parent, map_key, name, child);
        }
        None => {
            // Detaching removes the (name) entry from the map and clears the
            // child's parent.
            if let Some(cur) = doc.node(parent).and_then(|n| n.child_in_map(map_key, name)) {
                doc.detach_from_parent(cur);
            }
            // If the map is now empty, the field was created by the edit — drop
            // it to restore the original absence. An empty ChildMap never comes
            // from a parsed source (empty `{}` classifies as a Value), so an
            // empty one here can only be an editing artifact. Mirrors the
            // single-child `set_child_state(None)` which drops the field too.
            let now_empty = doc
                .node(parent)
                .map(|n| n.child_map(map_key).is_empty())
                .unwrap_or(false);
            if now_empty
                && let Some(i) = doc
                    .node(parent)
                    .and_then(|n| n.fields().iter().position(|(k, _)| k == map_key))
            {
                doc.remove_field_at(parent, i);
            }
        }
    }
}

impl Prim {
    /// Re-apply this primitive forward (used by redo).
    pub fn redo(&self, doc: &mut Document) {
        match self {
            // The node still exists (undo never freed it); nothing to do.
            Prim::Created(_) => {}
            Prim::FieldSet {
                node, key, after, ..
            } => set_field_state(doc, *node, key, after),
            Prim::ChildInserted {
                parent,
                index,
                child,
            } => {
                let _ = doc.insert_child(*parent, *index, *child);
            }
            Prim::ChildRemoved { parent, index, .. } => {
                doc.remove_child(*parent, *index);
            }
            Prim::ChildSet {
                parent, key, after, ..
            } => set_child_state(doc, *parent, key, *after),
            Prim::MapChildSet {
                parent,
                map_key,
                name,
                after,
                ..
            } => set_map_child_state(doc, *parent, map_key, name, *after),
            Prim::NamedChildPushed {
                parent,
                key,
                index,
                child,
                ..
            } => insert_named_child(doc, *parent, key, *index, *child),
            Prim::NamedChildRemoved {
                parent,
                key,
                index,
                ..
            } => remove_named_child(doc, *parent, key, *index),
        }
    }

    /// Apply this primitive's inverse (used by undo).
    pub fn undo(&self, doc: &mut Document) {
        match self {
            // Pin marker only — the node stays in the arena; its attach prim's
            // inverse detaches the whole subtree as a unit.
            Prim::Created(_) => {}
            Prim::FieldSet {
                node, key, before, ..
            } => set_field_state(doc, *node, key, before),
            Prim::ChildInserted { parent, index, .. } => {
                doc.remove_child(*parent, *index);
            }
            Prim::ChildRemoved {
                parent,
                index,
                child,
            } => {
                let _ = doc.insert_child(*parent, *index, *child);
            }
            Prim::ChildSet {
                parent,
                key,
                before,
                ..
            } => set_child_state(doc, *parent, key, *before),
            Prim::MapChildSet {
                parent,
                map_key,
                name,
                before,
                ..
            } => set_map_child_state(doc, *parent, map_key, name, *before),
            // Inverse of a push: detach the pushed child, then restore the field
            // to its EXACT pre-push shape (absent / `[]` / prior list) so the
            // round-trip is byte-identical even when the push created the field.
            Prim::NamedChildPushed {
                parent,
                key,
                child,
                before,
                ..
            } => {
                doc.detach_from_parent(*child);
                set_field_state(doc, *parent, key, before);
            }
            Prim::NamedChildRemoved {
                parent,
                key,
                index,
                child,
            } => insert_named_child(doc, *parent, key, *index, *child),
        }
    }

    /// Every `NodeId` this primitive references, for `gc` pinning.
    pub fn pinned_ids(&self, out: &mut Vec<NodeId>) {
        match self {
            Prim::Created(id) => out.push(*id),
            Prim::FieldSet { node, .. } => out.push(*node),
            Prim::ChildInserted { parent, child, .. }
            | Prim::ChildRemoved { parent, child, .. } => {
                out.push(*parent);
                out.push(*child);
            }
            Prim::ChildSet {
                parent,
                before,
                after,
                ..
            } => {
                out.push(*parent);
                out.extend(before.iter().chain(after.iter()).copied());
            }
            Prim::MapChildSet {
                parent,
                before,
                after,
                ..
            } => {
                out.push(*parent);
                out.extend(before.iter().chain(after.iter()).copied());
            }
            Prim::NamedChildPushed { parent, child, .. }
            | Prim::NamedChildRemoved { parent, child, .. } => {
                out.push(*parent);
                out.push(*child);
            }
        }
    }
}

/// A named, atomic group of primitives — one undo step.
#[derive(Debug, Clone)]
pub struct Transaction {
    /// Human-readable label (e.g. `"ripple delete"`), shown in undo history.
    pub label: String,
    /// The primitives, in application order.
    pub prims: Vec<Prim>,
}

impl Transaction {
    /// A new empty transaction with `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            prims: Vec::new(),
        }
    }

    /// Whether the transaction recorded no changes.
    pub fn is_empty(&self) -> bool {
        self.prims.is_empty()
    }

    /// Undo every primitive in reverse order.
    pub fn undo(&self, doc: &mut Document) {
        for prim in self.prims.iter().rev() {
            prim.undo(doc);
        }
    }

    /// Redo every primitive in forward order.
    pub fn redo(&self, doc: &mut Document) {
        for prim in &self.prims {
            prim.redo(doc);
        }
    }

    /// Append this transaction's pinned ids to `out`.
    pub fn pinned_ids(&self, out: &mut Vec<NodeId>) {
        for prim in &self.prims {
            prim.pinned_ids(out);
        }
    }
}
