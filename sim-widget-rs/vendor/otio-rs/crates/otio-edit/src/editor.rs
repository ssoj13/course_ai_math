//! [`Editor`] — owns the document and the undo/redo history.
//!
//! The Editor is the single entry point for editing. It holds the `Document`
//! **privately** and mutates it only through [`Editor::edit`], which runs a
//! closure against a [`Tx`] and commits the recorded primitives as one undo
//! step. Read access for the widget is `document()`; a background render/export
//! thread takes a cheap `snapshot()` (the model is `Clone`).

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use otio_core::{Document, NodeId};

use crate::error::EditError;
use crate::journal::Transaction;
use crate::tx::Tx;

/// Hands each new [`Editor`] a DISTINCT starting revision. A `Document` allocates `NodeId`s
/// deterministically, so two freshly-built editors reuse the same ids; a downstream layout cache
/// keyed on `(NodeId, revision)` ([`crate::LayoutCache`]) would then serve one editor's layout for
/// another's identical track if both started at revision 0 — a rebuilt timeline (the host swaps the
/// whole `Editor`) showing the PREVIOUS document's clips (often none). Each editor reserves a wide
/// band of revisions (`epoch << 20`, ~1M edits of headroom) so its own edits can never collide with
/// another editor's band.
static EDITOR_EPOCH: AtomicU64 = AtomicU64::new(1);

/// A timeline editor: a document plus its undo/redo stacks.
pub struct Editor {
    doc: Document,
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    revision: u64,
}

impl Editor {
    /// Wrap an existing document (e.g. one read from `.otio`).
    pub fn new(doc: Document) -> Self {
        Self {
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
            // A UNIQUE band per editor, not 0 — see [`EDITOR_EPOCH`]. Without this a rebuilt
            // timeline shared a revision with the editor it replaced and rendered its stale layout.
            revision: EDITOR_EPOCH.fetch_add(1, Ordering::Relaxed) << 20,
        }
    }

    /// Start from an empty document.
    pub fn empty() -> Self {
        Self::new(Document::new())
    }

    /// Read-only access to the model (for the widget / layout / serialization).
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// A detached clone of the document for a background thread (render/export).
    /// `Document` is `Send + Sync`, so the snapshot can cross threads freely.
    pub fn snapshot(&self) -> Document {
        self.doc.clone()
    }

    /// A counter bumped on every committed edit, undo and redo. The widget compares it to know when
    /// to rebuild its layout. It also STARTS in a distinct per-editor band (see [`EDITOR_EPOCH`]),
    /// so the value additionally identifies WHICH editor produced a given layout — a cache keyed on
    /// it can never confuse two freshly-built editors that share `NodeId`s.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether there is anything to undo / redo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The label of the next undo step, if any (for menu text).
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|t| t.label.as_str())
    }

    /// Run an edit operation as ONE atomic, undoable transaction.
    ///
    /// `f` mutates the document via the recording [`Tx`]. On success the recorded
    /// primitives (if any) become a new undo step and the redo stack is cleared.
    /// On error every primitive recorded so far is rolled back (reverse-applied),
    /// so a partially-applied compound op never leaves the document inconsistent.
    pub fn edit<F>(&mut self, label: &str, f: F) -> Result<(), EditError>
    where
        F: FnOnce(&mut Tx) -> Result<(), EditError>,
    {
        let mut tx = Tx::new(&mut self.doc);
        let result = f(&mut tx);
        let prims = tx.into_prims();
        let txn = Transaction {
            label: label.to_string(),
            prims,
        };
        match result {
            Ok(()) => {
                if !txn.is_empty() {
                    self.undo.push(txn);
                    self.redo.clear();
                    self.revision += 1;
                }
                Ok(())
            }
            Err(e) => {
                // Atomic failure: reverse the partial work.
                txn.undo(&mut self.doc);
                Err(e)
            }
        }
    }

    /// Undo the most recent edit. Returns whether anything was undone.
    pub fn undo(&mut self) -> bool {
        let Some(txn) = self.undo.pop() else {
            return false;
        };
        txn.undo(&mut self.doc);
        self.redo.push(txn);
        self.revision += 1;
        true
    }

    /// Redo the most recently undone edit. Returns whether anything was redone.
    pub fn redo(&mut self) -> bool {
        let Some(txn) = self.redo.pop() else {
            return false;
        };
        txn.redo(&mut self.doc);
        self.undo.push(txn);
        self.revision += 1;
        true
    }

    /// Every `NodeId` referenced by the undo/redo history — the set `gc` must
    /// keep alive so an undo can re-attach a detached node with its id intact.
    fn pinned(&self) -> HashSet<NodeId> {
        let mut ids = Vec::new();
        for txn in self.undo.iter().chain(self.redo.iter()) {
            txn.pinned_ids(&mut ids);
        }
        ids.into_iter().collect()
    }

    /// Reclaim orphaned arena nodes that the history does not pin. Run outside
    /// the undo horizon (on save / idle), never mid-edit. Returns the count freed.
    pub fn gc(&mut self) -> usize {
        let pinned = self.pinned();
        self.doc.gc(&pinned)
    }

    /// Drop all undo/redo history (e.g. after saving) and reclaim now-unpinned
    /// orphans. Returns the count freed.
    pub fn clear_history(&mut self) -> usize {
        self.undo.clear();
        self.redo.clear();
        self.doc.gc(&HashSet::new())
    }
}
