//! [`Tx`] — the recording mutation wrapper.
//!
//! Every document change inside an edit op goes through a `Tx`, which performs
//! read-old-then-write and appends a [`Prim`] to the journal. Because the
//! [`Editor`](crate::editor::Editor) hands out `&mut Document` ONLY via a `Tx`,
//! there is no code path that mutates the document without recording undo — the
//! "a command bypassed the journal" corruption class is structurally impossible.
//!
//! `Tx` exposes read access (`doc()`) for the range math edit ops need, and a
//! small set of recording mutators mirroring the otio-core structural API.

use otio_core::{Document, NodeId};
use otio_schema::build;
use otio_time::TimeRange;
use serde_json::Value;

use crate::error::EditError;
use crate::journal::{FieldState, Prim};

/// Capture the current `(index, field)` state of `key` on `node`, or `None`.
fn capture_field(doc: &Document, node: NodeId, key: &str) -> FieldState {
    let n = doc.node(node)?;
    let i = n.fields().iter().position(|(k, _)| k == key)?;
    Some((i, n.fields()[i].1.clone()))
}

/// A recording transaction over a borrowed document.
pub struct Tx<'a> {
    doc: &'a mut Document,
    prims: Vec<Prim>,
}

impl<'a> Tx<'a> {
    /// Begin recording against `doc`.
    pub fn new(doc: &'a mut Document) -> Self {
        Self {
            doc,
            prims: Vec::new(),
        }
    }

    /// Read-only view of the document (for range/layout math during an op).
    pub fn doc(&self) -> &Document {
        self.doc
    }

    /// Consume the transaction, returning the recorded primitives.
    pub fn into_prims(self) -> Vec<Prim> {
        self.prims
    }

    /// Create a node, recording it for undo (detach-on-undo, pinned vs gc).
    pub fn create_node(&mut self, schema: &str, version: u32) -> NodeId {
        let id = self.doc.create_node(schema, version);
        self.prims.push(Prim::Created(id));
        id
    }

    /// Deep-clone a subtree, recording a `Created` for the clone root AND every
    /// cloned descendant so the whole subtree is pinned against `gc` across
    /// undo/redo (used by e.g. razor, which clones a clip with its media refs).
    pub fn deep_clone(&mut self, id: NodeId) -> Result<NodeId, EditError> {
        let new_id = self.doc.deep_clone(id).ok_or(EditError::NodeNotFound)?;
        self.record_created_subtree(new_id);
        Ok(new_id)
    }

    /// Build a canonical `Gap` spanning `range` via the schema builder, recording
    /// the created node for undo/gc. A gap built this way carries the full default
    /// field set (`name`/`metadata`/`effects`/`markers`/`enabled`/`color`/
    /// `source_range`) the reference emits — matching `otio_schema::build::gap`.
    pub fn build_gap(&mut self, range: TimeRange) -> NodeId {
        let id = build::gap(self.doc, range);
        self.record_created_subtree(id);
        id
    }

    /// Build a canonical empty `Track` of `kind` (`"Video"`/`"Audio"`) via the
    /// schema builder, recording it for undo/gc.
    pub fn build_track(&mut self, name: &str, kind: &str) -> NodeId {
        let id = build::track(self.doc, name, kind);
        self.record_created_subtree(id);
        id
    }

    /// Build a canonical `Marker.2` via the schema builder, recording it for
    /// undo/gc. `color` is the OTIO marker-color enum (e.g. `"RED"`); `raw_color`
    /// is stored under `metadata.cmx_3600.color` (pass the same string when there
    /// is no distinct raw value).
    pub fn build_marker(
        &mut self,
        name: &str,
        marked_range: TimeRange,
        color: &str,
        raw_color: &str,
    ) -> NodeId {
        let id = build::marker(self.doc, name, marked_range, color, raw_color);
        self.record_created_subtree(id);
        id
    }

    /// Append `child` to `parent`'s NAMED child-list field `key` (e.g. `markers`,
    /// `effects`), recording a [`Prim::NamedChildPushed`] at the real end index so
    /// the inverse removes the right slot. Distinct from `append_child`, which is
    /// hard-wired to the `"children"` composition list.
    pub fn push_named_child(
        &mut self,
        parent: NodeId,
        key: &str,
        child: NodeId,
    ) -> Result<(), EditError> {
        let index = self
            .doc
            .node(parent)
            .map(|n| n.children(key).len())
            .ok_or(EditError::NodeNotFound)?;
        // Capture the field's exact shape BEFORE the push so undo can restore
        // absence (not just an empty `[]`) when this push creates the field.
        let before = capture_field(self.doc, parent, key);
        self.doc.push_named_child(parent, key, child)?;
        self.prims.push(Prim::NamedChildPushed {
            parent,
            key: key.to_string(),
            index,
            child,
            before,
        });
        Ok(())
    }

    /// Remove the child at `index` from `parent`'s NAMED child-list field `key`,
    /// recording a [`Prim::NamedChildRemoved`] so undo re-inserts it at the same
    /// position. Returns the removed (now detached) child id.
    pub fn remove_named_child(
        &mut self,
        parent: NodeId,
        key: &str,
        index: usize,
    ) -> Result<NodeId, EditError> {
        let children = self
            .doc
            .node(parent)
            .map(|n| n.children(key).to_vec())
            .ok_or(EditError::NodeNotFound)?;
        let child = *children.get(index).ok_or(EditError::IndexOutOfRange {
            index,
            len: children.len(),
        })?;
        // Detach the specific child; if the list empties, reset to canonical `[]`.
        self.doc.detach_from_parent(child);
        if children.len() == 1
            && let Some(n) = self.doc.node_mut(parent)
        {
            n.set_value(key, Value::Array(Vec::new()));
        }
        self.prims.push(Prim::NamedChildRemoved {
            parent,
            key: key.to_string(),
            index,
            child,
        });
        Ok(child)
    }

    /// DFS-record a `Created` primitive for `id` and all its structural
    /// descendants (so every freshly cloned node id is pinned).
    fn record_created_subtree(&mut self, id: NodeId) {
        self.prims.push(Prim::Created(id));
        let children: Vec<NodeId> = match self.doc.node(id) {
            Some(n) => n.fields().iter().flat_map(|(_, f)| f.child_ids()).collect(),
            None => Vec::new(),
        };
        for c in children {
            self.record_created_subtree(c);
        }
    }

    /// Set a value field, recording before/after for reversible undo/redo.
    pub fn set_value(&mut self, node: NodeId, key: &str, value: Value) -> Result<(), EditError> {
        let before = capture_field(self.doc, node, key);
        self.doc
            .node_mut(node)
            .ok_or(EditError::NodeNotFound)?
            .set_value(key, value);
        let after = capture_field(self.doc, node, key);
        self.prims.push(Prim::FieldSet {
            node,
            key: key.to_string(),
            before,
            after,
        });
        Ok(())
    }

    /// Insert `child` into `parent`'s `children` at `index` (clamped), recording
    /// the real clamped index so the inverse removes the right slot.
    pub fn insert_child(
        &mut self,
        parent: NodeId,
        index: usize,
        child: NodeId,
    ) -> Result<(), EditError> {
        let len = self
            .doc
            .node(parent)
            .map(|n| n.children("children").len())
            .ok_or(EditError::NodeNotFound)?;
        let real = index.min(len);
        self.doc.insert_child(parent, real, child)?;
        self.prims.push(Prim::ChildInserted {
            parent,
            index: real,
            child,
        });
        Ok(())
    }

    /// Append `child` to `parent`'s `children`.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), EditError> {
        let len = self
            .doc
            .node(parent)
            .map(|n| n.children("children").len())
            .ok_or(EditError::NodeNotFound)?;
        self.insert_child(parent, len, child)
    }

    /// Remove the child at `index` from `parent`'s `children`, recording it.
    pub fn remove_child(&mut self, parent: NodeId, index: usize) -> Result<NodeId, EditError> {
        let child = self
            .doc
            .remove_child(parent, index)
            .ok_or(EditError::IndexOutOfRange {
                index,
                len: self
                    .doc
                    .node(parent)
                    .map(|n| n.children("children").len())
                    .unwrap_or(0),
            })?;
        self.prims.push(Prim::ChildRemoved {
            parent,
            index,
            child,
        });
        Ok(child)
    }

    /// Set a single-child field `key` on `parent`, recording the prior occupant.
    pub fn set_child(
        &mut self,
        parent: NodeId,
        key: &str,
        child: NodeId,
    ) -> Result<(), EditError> {
        let before = self.doc.node(parent).and_then(|n| n.child(key));
        self.doc.set_child(parent, key, child)?;
        self.prims.push(Prim::ChildSet {
            parent,
            key: key.to_string(),
            before,
            after: Some(child),
        });
        Ok(())
    }

    /// Set a named map entry on `parent`, recording the prior occupant.
    pub fn set_child_in_map(
        &mut self,
        parent: NodeId,
        map_key: &str,
        name: &str,
        child: NodeId,
    ) -> Result<(), EditError> {
        let before = self.doc.node(parent).and_then(|n| n.child_in_map(map_key, name));
        self.doc.set_child_in_map(parent, map_key, name, child)?;
        self.prims.push(Prim::MapChildSet {
            parent,
            map_key: map_key.to_string(),
            name: name.to_string(),
            before,
            after: Some(child),
        });
        Ok(())
    }
}
