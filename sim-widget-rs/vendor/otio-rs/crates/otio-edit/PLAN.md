# otio-edit — editing / command / undo layer over otio-core

> Author: Alex Joss <joss13@gmail.com>. Design conventions: see `../../CONVENTIONS.md`.
> This plan is the agreed output of a 3-agent design review (architecture,
> Rust/undo-correctness, edit-ops/widget). It governs Phases A (otio-core
> hardening) and B (this crate).

## Why
otio-core is the canonical timeline **document model** (lossless, interchange,
algorithms). It is deliberately NOT an editing engine: no undo, no high-level
edit operations (ripple/roll/slip/razor…), no change notification, no layout
cache. `otio-edit` is that engine — the layer a Rust+egui NLE timeline widget
(DaVinci/Premiere-class) drives. Keeping it a separate crate preserves the
"one model, single responsibility per layer" rule: the widget renders, otio-edit
edits + records undo, otio-core stores, otio-algo computes layout.

```
egui timeline widget  ──reads layout──►  otio-edit cached layout (per-track)
        │ commands (drag/trim/cut/insert)
        ▼
  otio-edit: Editor (owns Document privately) → Tx → journal → undo/redo
        │ primitive mutations only
        ▼
  otio-core Document  (arena; lossless; persist; interchange)
        │ media refs + time-mapping
        ▼
  playa (decode / playback)            otio-algo (range/transform/flatten)
```

## Design decisions (locked by the review)

### Undo model — journal of reversible primitives
The arena is already a closed set of reversible primitives. Each edit op records
the primitives it applies; undo reverse-iterates applying inverses. Chosen over
snapshot-subtree (a ripple on a 1000-clip track would clone the whole tail per
keystroke) and over op-log/event-sourcing (replay fights generational ids and
redo-after-divergence). Memory is O(edit size); a ChangeSet of touched NodeIds
falls out of the journal for free.

### Recording lives in an otio-edit `Tx` wrapper, NOT in otio-core mutators
`Editor` owns the `Document` **privately** and exposes mutation ONLY through a
`Tx` that does read-old-then-write and appends primitives to the journal. This is
encapsulation, not discipline: no edit op ever gets `&mut Document`, so there is
no "a command bypassed the journal and corrupted undo" path. otio-core mutators
stay pure (no optional journaling flag → no dual code paths).

### Generational-id stability — undo DETACHES, never frees
`NodeId` is a generational slotmap key: a freed-then-recreated node gets a *new*
id, which would invalidate journal entries and widget/Python handles. Therefore:
- **Undo of a create = detach (clear parent + unlink from parent's field), keep
  the node alive in the arena with its id stable.** Redo re-attaches the SAME id.
- **`gc()` never frees a node referenced by the undo/redo stacks.** GC roots =
  document root ∪ every NodeId named by any retained journal entry ("pinned").
- **gc runs only outside the undo horizon** (on save / explicit history flush),
  never mid-edit. Mid-edit only ever detaches. This is the #1 correctness rule —
  it eliminates the "floating bug" class where a handle silently points at a
  wrong/empty slot after undo.

### Field-order fidelity under undo
`Node.fields` is order-significant (lossless round-trip). A naive
`SetField{old}` breaks order when a command removes then re-adds a field (it
reappends at the end). The journal captures the field **index**; otio-core gains
`remove_field_at`/`insert_field_at` so undo restores a field at its original
position. Coalescing (a drag = many `SetField` on one `(node,key)`) keeps only
the FIRST old value; the final new value is implicit in the live document.

### Change propagation — `revision()` now, per-track ChangeSet derived from the journal
v1: a monotonic `revision()` on the Editor, bumped on every committed `Tx`; the
widget compares `last_seen` and rebuilds its cache when it changes. The set of
NodeIds touched by a committed Tx is exactly its journal entries, so a per-track
**dirty set** is derived for free — the layout cache invalidates only the tracks
that actually changed (not all tracks on any edit). No `RwLock`: `Document` is
already `Send + Sync + Clone`, the Editor is single-writer, and a background
render/export thread takes a cheap `Document::clone()` snapshot.

### Layout cache — per-track
`range_of_all_children` is O(n) per track. Cache a `Vec<(NodeId, TimeRange)>` per
track keyed by track NodeId, invalidated by that track's dirty bit (derived from
the journal). The Vec is start-sorted so the widget binary-searches the visible
`[t0,t1]` window and draws only on-screen clips (cull 1000→~tens). The dirty
state lives in otio-edit (a side table), NOT on otio-core `Node` (keep it pure).

## Phase A — otio-core hardening (prerequisite, in `crates/otio-core`)
Primitives the journal/gc need. Each lands with unit tests; the corpus
round-trip (12/12) and all existing diffs must stay green.

1. `detach_from_parent(&mut self, id) -> bool` — clear `parent` AND unlink the id
   from the parent's `Child`/`Children`/`ChildMap` field. The undo-safe inverse of
   a create/attach (keeps the node + id alive). Dedups the unlink logic shared
   with `remove_child`.
2. `remove_node(&mut self, id) -> Result<(), Error>` — **leaf-only**: errors
   unless `parent == None` AND the node has no child-type field with entries.
   Frees the slot. Only `gc` calls it (outside the undo horizon).
3. `remove_field_at(&mut self, node, index) -> Option<(String, Field)>` and
   `insert_field_at(&mut self, node, index, key, field)` — order-preserving field
   undo/redo.
4. `gc(&mut self, pinned: &HashSet<NodeId>) -> usize` — mark-sweep: free every node
   unreachable from `root` AND not in `pinned`; returns the count freed. Frees in
   leaf-first order so `remove_node`'s leaf contract holds.
5. Fix `set_child_in_map`: detach the evicted occupant (`parent = None`), guarded
   against self-replace. Same latent leak when `set_value`/`set_child` replace a
   child-link — document that the Tx layer must capture the old child.
6. NO `RwLock`/`SharedDoc`. `Document` stays plain (already `Send+Sync+Clone`);
   single-writer convention documented.

## Phase B — otio-edit crate

### Modules (lib root `src/otio_edit.rs`, `[lib] name = "otio_edit"`)
- `error.rs` — `EditError` (thiserror).
- `journal.rs` — `Prim` enum (CreateNode, AttachChild, DetachChild, SetField{node,key,old_index,old_field}, SetChildInMap{old}, SetRoot{old}), `Transaction{label, prims}`, inverse application.
- `tx.rs` — `Tx<'a>{ doc:&mut Document, prims:&mut Vec<Prim> }`: recording wrappers around the core mutators (read-old-then-write), `begin/commit/rollback`.
- `editor.rs` — `Editor{ doc, undo:Vec<Transaction>, redo:Vec<Transaction>, revision, dirty }`: owns the Document privately; `exec`, `undo`, `redo`, `revision()`, `dirty_tracks()`, `gc()`, `document()` (read-only borrow), `snapshot()` (clone for a bg thread).
- `ops.rs` — edit operations, each a function building a Transaction via Tx.
- `layout.rs` — per-track cached `Vec<(NodeId,TimeRange)>` + dirty invalidation.
- `snap.rs` — (v2) pure snapping helper.

### Edit ops — v1
All expressible from existing core mutators + `edit::set_source_range` + range
math; **no new arena primitive needed** beyond Phase A:
- field writes: `set_source_range`, `set_name`, `set_color`, `set_enabled`
- `trim_in(Δ)`: `sr' = (s+Δ, d-Δ)`, clamp `s+Δ ≥ avail.start`, `d-Δ > 0`
- `trim_out(Δ)`: `sr' = (s, d+Δ)`, clamp `d+Δ ≤ avail.end-s`, `> 0`
- `slip(Δ)`: `sr' = (s+Δ, d)`, clamp to media `available_range`
- `razor(track, T)`: split child at T → deep_clone + `set_source_range` (left `(s,off)`, right `(s+off, d-off)`) + `insert_child` after
- `ripple_delete(track, idx)`: `remove_child` (+ merge adjacent gaps)
- `lift(track, idx)`: replace child with `build::gap(child.duration())`
- `overwrite(track, P, T)`: trim straddling head/tail (mirror `flatten.rs:88-99`), remove fully-covered, `insert_child` P, pad trailing gap
- `insert(track, P, idx)`: `insert_child` (ripples downstream)
- tracks: `add_track`, `remove_track`, `reorder_track`
- markers: `add_marker` (`push_named_child`), `remove_marker`

Transition-awareness: edge ops refuse mid-transition trims (otio-algo already
raises `CannotTrimTransition`); razor/overwrite index math skips transitions when
accumulating start (as `range.rs` does).

Gap rules: never two adjacent Gaps (merge: sum durations); never a zero-duration
Gap (drop). lift leaves a gap; ripple_delete closes + merges; overwrite may split
a clip and/or pad a trailing gap; slide (v2) resizes bounding gaps.

### Edit ops — v2
`roll` (2× set_source_range on the shared edit point), `slide` (2× set_source_range
on bounding gaps), `move_clip` (remove + insert + gap fill), 3-point editing,
snapping, per-track ChangeSet-driven incremental relayout.

### Verification
- otio-core Phase A: unit tests per primitive + corpus 12/12 still lossless,
  existing diffs unaffected.
- otio-edit: per-op unit tests (apply → assert layout/JSON, undo → assert
  byte-identical to pre-state, redo → identical to post-state). A property-ish
  test: random op sequence, then full undo, must return to the exact starting JSON
  (the strongest correctness gate for the journal).

## Top risks (from the review) and mitigations
1. Generational-id instability across undo/redo → detach-not-delete + gc pinning.
2. A mutation bypassing `Tx` → Editor keeps `Document` private; only `Tx` mutates;
   algos that clone (flatten) run on throwaway docs, outside the editable session.
3. `set_child_in_map` / child-link replacement orphaning a node → Phase A detach
   fix + Tx captures the replaced child id as `{old}`.

## Status
- [x] Phase A: detach_from_parent, remove_node, remove_field_at/insert_field_at, gc, set_child_in_map fix.
- [x] Phase B scaffold: crate + modules + error + journal/tx/editor skeletons.
- [x] Phase B v1 ops + layout cache + undo/redo + tests.
- [x] Phase B v2: roll, slide, move_clip, three_point, snapping helper (`snap.rs`).
      Deferred: transition-aware overwrite (still refuses a transition in-window —
      v1 limitation carried over); per-track ChangeSet-driven incremental relayout
      (layout cache still invalidates on `revision()`).
