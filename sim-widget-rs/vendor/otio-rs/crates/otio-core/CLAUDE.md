# CLAUDE.md — `crates/otio-core`

> Dev conventions (Alex Joss, joss13@gmail.com): see `../../CONVENTIONS.md`.

## What
The OTIO object model + lossless `.otio` JSON round-trip (Phase 2 of otio-rs,
see `../../PLAN.md`). Depends on `otio-time` and `box-rs`.

- `Document` — generational arena (`slotmap`) of nodes + a root. `document.rs`.
- `Node { schema_name, schema_version, parent, fields }` — one serializable
  object, fields in source order.
- `Field = Value(serde_json::Value) | Child(NodeId) | Children(Vec<NodeId>) | ChildMap(Vec<(String, NodeId)>)`.
  `ChildMap` = a named map of child nodes (e.g. Clip.2 `media_references`).
- `NodeId` — stable generational key.
- Lib root `src/otio_core.rs` (`[lib] name = "otio_core"`).

## Core design decision — lossless by construction
Instead of 20 hand-written per-schema serializers (which drift), each node stores
its fields **verbatim**: a field is either a raw JSON `Value` (scalars, metadata,
and the inline value-type schemas RationalTime/TimeRange/Color/…) or a structural
`Child`/`Children` link. Serialization just re-emits `OTIO_SCHEMA` + the ordered
fields. Nothing is interpreted, so nothing is lost. Typed schema accessors are a
thin layer on top (future `otio-schema`).

- **Node vs value**: an `OTIO_SCHEMA` object is a node unless its schema is in
  `is_value_schema` (RationalTime, TimeRange, TimeTransform, Color, Box2d, V2d,
  **SerializableObjectRef**) — those stay inline. `SerializableObjectRef` is the
  inline ref-id schema (deserialization.cpp `SerializableObjectRef.1`), emitted
  for shared objects under OTIO_INSTANCING_SUPPORT; listing it here prevents
  misclassifying a ref as a phantom node. Unknown/plugin schemas are nodes (preserved).
- **Legacy aliases** (`canonical_schema`): `Sequence`->`Track`, `Filler`->`Gap`,
  `SerializeableCollection`->`SerializableCollection` (typeRegistry.cpp). Used by
  views/algos for dispatch; storage keeps the original name (lossless).
- **Child detection** (`classify`): a field whose value is a node-object → `Child`;
  a non-empty *object* whose values are all node-objects → `ChildMap` (e.g. Clip.2
  `media_references`); a non-empty *array* of node-objects → `Children`; everything
  else → `Value` (incl. empty `[]`/`{}`, so empty `effects`/`metadata` round-trip
  verbatim). Arm order matters: single-node before map (a node-object is also an
  Object).
- `serde_json` built with `preserve_order` so output key order matches the source;
  the round-trip gate compares JSON DOMs (order-independent) for true fidelity.

## Status
- [x] Model + JSON round-trip. `cargo test -p otio-core` → unit (2) + corpus (1) pass.
- [x] **Phase-2 gate**: `tests/corpus_roundtrip.rs` round-trips the official sample
      corpus → **12/12 files lossless**, DOM-equal. `cargo clippy` clean.
- [x] `box-rs` integration validated via `Node::metadata()` (snapshot view).
- [x] Verified by an independent agent vs the C++ reference. One real fix applied:
      `SerializableObjectRef` added to value schemas (was misclassified as a node).
      `ChildMap` added so Clip.2 `media_references` maps are navigable (lossless).

### Known limitation (documented skip, not silent)
- `big_int.otio` is excluded: it uses rapidjson's non-standard JSON extensions
  (`Inf`/`-Infinity`/`NaN` literals + int128/int256). `serde_json` can't parse
  these. → tracked as the "extended-JSON number fidelity" follow-up (PLAN.md §7).
  Options later: tolerant tokenizer for Inf/NaN + `arbitrary_precision` for big ints.

## Mutation
- **Field-level**: `Document::node_mut(id)`, `Node::set_value(key, Value)`
  (replace-in-place or append, order-preserving), `Node::remove_field(key)`,
  `Node::set_metadata(box)`. Value-field only (replacing a child-link via
  `set_value` would detach its nodes).
- **Structural** (on `Document`): `create_node(schema, ver)`, `set_root(id)`,
  `deep_clone(id)` (recursive, fresh ids, reparented), `set_child(parent, key,
  child)` (single-child fields like `tracks`/`media_reference`),
  `append_child`/`insert_child`/`remove_child` (the `children` list; creates the
  field if absent/empty, maintains parent pointers).
- Note: orphaned nodes (removed children, intermediate clones) linger in the
  arena until `gc` reclaims them — ids are generational so this is safe.

### Editing-layer primitives (Phase A, for `otio-edit`)
Added so the editor's undo journal + GC can keep node ids stable across undo/redo:
- `detach_from_parent(id) -> bool` — clear `id.parent` AND unlink it from the
  parent's `Child`/`Children`/`ChildMap` field, **keeping the node + id alive**.
  The undo-safe inverse of a create/attach (a true free would bump the slot
  generation and invalidate journal entries / language-binding handles). Shares
  an `unlink_child` helper with `remove_child`.
- `remove_node(id) -> Result<(), Error>` — **leaf-only** true removal (errors
  unless `parent == None` and no child-type field has entries). The only call
  site is `gc`; never an undo primitive.
- `remove_field_at(node, index) -> Option<(String, Field)>` /
  `insert_field_at(node, index, key, field)` — order-preserving field undo/redo
  (a plain re-add via `set_value` would append at the end, breaking lossless
  source order; the journal restores the captured index).
- `gc(pinned: &HashSet<NodeId>) -> usize` — mark-sweep. **Roots = document root ∪
  every pinned id**, so a pinned node *and its whole subtree* survive (the editor
  pins all undo/redo-referenced ids → an undo re-attaches a complete subtree).
  Frees everything unmarked. Run outside the undo horizon, never mid-edit.
- `set_child_in_map` now **detaches the evicted occupant** (clears its `parent`),
  guarded against self-replace — fixes a latent orphan/`parent`-staleness bug.
  (Same care needed when `set_value`/`set_child` replace a child-link: the
  `otio-edit` `Tx` captures the old child for undo.)

## Deviations from PLAN
- JSON read/write currently lives **in otio-core** (not a separate `otio-serde`),
  because it is intrinsic to the lossless model. A thin `otio-serde` can wrap it
  later for format-specific concerns (downgrade/upgrade, byte-exact float policy).

## Next
- `otio-schema`: typed accessors over `fields` (source_range as `TimeRange`,
  clip media refs, track kind, etc.) writing back through `box-rs` for metadata.
- Ref-id (`*`-shared-object) resolver for full parity (trees work today).
- Schema version up/down-grade (`CORE_VERSION_MAP`).
