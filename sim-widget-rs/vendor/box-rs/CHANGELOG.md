# Changelog

All notable changes to `box-rs` are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); this crate is pre-1.0.

## [0.3.0] — 2026-08-06

### Changed
- **`Array` and `Object` hold their interior behind an `Arc`, copy-on-write.**
  Cloning a `BoxValue` is now a refcount bump instead of a deep copy of the
  tree: measured **10 ns regardless of size**, where a 1296-leaf tree used to
  cost proportionally more than a 9-leaf one (`tests/clone_cost.rs` asserts the
  ratio, so a regression to deep copying fails the suite). Mutation goes through
  `Arc::make_mut`, so the observable semantics are unchanged — a clone is still
  an independent value, the copy just happens on the first write instead of on
  every clone. This is what makes a dataflow graph affordable: a payload that
  travels a wire, lands in a bus slot and is read back used to be deep-copied at
  every hop.
- `deep_freeze_clone` is now `value.clone()`. With copy-on-write interiors a
  plain clone already gives the "nothing can mutate this behind your back"
  guarantee the deep walk existed to provide; the function is kept as the name
  for that guarantee.
- `PartialEq` is hand-written rather than derived, with an `Arc::ptr_eq`
  shortcut. Equality is still **by content** — the pointer check is only a fast
  path for the case sharing makes common (`equality_is_by_content_not_by_pointer`
  locks this).

### Added
- `BoxValue::obj(map)` / `BoxValue::array(items)` constructors, `BoxMap` /
  `BoxList` interior aliases, and `From<BoxMap>` / `From<BoxList>` /
  `FromIterator` impls. Prefer these over naming the variant: they are the one
  place that changes if the representation moves again.

### Migration
- **BREAKING:** constructing `BoxValue::Object(map)` / `BoxValue::Array(vec)`
  now needs `BoxValue::obj(map)` / `BoxValue::array(vec)` (or `.into()`).
  *Matching* on those variants is unaffected — the binding derefs to the same
  `&BTreeMap` / `&Vec` as before, so `as_object`, `as_object_mut`, iteration and
  indexing all keep working untouched. In this tree that was ~30 lines, nearly
  all of them in `nodes-rs`.

## [0.2.0] — 2026-07-03

### Added
- **`BoxValue::Buffer { dtype, shape, data }`** — a compact typed binary buffer for
  bulk numeric payloads (point clouds, keypoints, descriptors, …) that would be
  absurd as `Array(Vec<BoxValue>)`. Bytes are opaque + immutable, `Arc`-shared
  (clone/freeze do NOT deep-copy). New `DType` enum (mirrors the safetensors dtype
  set) and `BufData` newtype (its `Debug` prints `[N bytes]`, not the bytes).
  Accessors: `BoxValue::buffer(..)`, `as_bytes`, `as_buffer`, `buffer_arc`.
- JSON backend round-trips a buffer as `{"$buffer":{"dtype","shape","data":[u8,…]}}`
  (native u8 array — no base64 dep; verbose, for small buffers / interop).
- **`safetensors` feature** + `container` module: `save_safetensors` / `load_safetensors`
  serialize a whole box as ONE `.safetensors` file — buffers become real tensors
  (typed, aligned, tool-inspectable), the rest of the tree rides as JSON in
  `__metadata__["box"]`. The compact binary backend for scene/checkpoint files.

### Changed
- **BREAKING (semver):** adding a `BoxValue`/`ValueKind` variant breaks *exhaustive*
  matches. In-crate matches updated. Downstream consumers that match without a `_`
  arm must add a `Buffer` arm before bumping their pinned rev — see README Consumers
  (notably `nodes-rs/.../inspector_wgpu.rs`). No `#[non_exhaustive]` (would force a
  `_` into every consumer today).
- Optional dep `safetensors` (off by default) keeps the base crate dependency-light.

## [0.1.0] — 2026-06-19
Initial standalone release. Extracted from `mermaid-rs/crates/rust-box` into its
own private repo (`ssoj13/box-rs`) for reuse across the `cglibs` projects via
git-ref.

### Notes
- Package renamed `rust-box` → **`box-rs`**; the crate links as **`box_rs`**
  (bare `box` is a reserved keyword). Downstream imports use `box_rs::*`.
- No functional changes vs the in-tree `rust-box`: same `RustBox` / `BoxValue`
  model, typed aggregate values (Vec3/Vec4/Mat3/Mat4/Color), dot/bracket/attr
  path access, smart merge, structural diff/subtract, JSON interop.
- Dependencies: serde, serde_json, thiserror, smallvec (no internal deps).
- 18 unit tests pass.
