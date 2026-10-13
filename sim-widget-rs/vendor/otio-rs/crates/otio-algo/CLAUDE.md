# CLAUDE.md — `crates/otio-algo`

> Dev conventions (Alex Joss, joss13@gmail.com): see `../../CONVENTIONS.md`.

## What
Range / layout algorithms over the otio-core model (Phase 2 of otio-rs, see
`../../PLAN.md`). Read-only; turns an OTIO tree into the positional data a timeline
UI consumes. Faithful ports of `track.cpp`/`stack.cpp`/`item.cpp`.

Public API (`range.rs`):
- `duration(doc, id)` — item/transition duration.
- `trimmed_range(doc, id)` — `source_range` or `available_range`.
- `available_range(doc, id)` — Clip→media range; Track→sum of item durations +
  head/tail transition offsets; Stack→longest child.
- `range_of_child_at_index(doc, parent, index)` — a child's range in parent time
  (negative indexing supported; transitions don't advance, shift back by in_offset).
- `range_of_all_children(doc, parent) -> Vec<(NodeId, TimeRange)>` — the full
  layout (the widget's main entry point).
- `flatten_stack(doc, stack) -> NodeId` — collapse a stack of tracks into one
  "Flattened" `Track` (top wins; lower tracks fill gaps). Mutates `doc` (clones).
- `track_trimmed_to_range(doc, track, range) -> NodeId` — clone a track trimmed
  to a range (drop outside children, trim partial ones). `flatten.rs`.
- `transformed_time(doc, from, time, to)` / `transformed_time_range` — map a
  time/range between item coordinate spaces (walk the parent chain). `transform.rs`.
- `range_of_child(doc, ancestor, child)` / `index_of_child` — a child's range in
  an ancestor's space. `transform.rs`.

Lib roots: `src/otio_algo.rs` (`[lib] name = "otio_algo"`); modules `range.rs`,
`flatten.rs`, `transform.rs`.

## Key reference facts (verified, not guessed)
- `Composable::overlapping()` is true ONLY for `Transition`; transitions have
  `duration = in_offset + out_offset` and are centered on the cut.
- `Gap` has no `available_range` override → relies on its (always-present)
  `source_range` (Item::available_range is NOT_IMPLEMENTED).
- All `optional<T>` writers emit `null` (never omit) — relevant to setters, not here.
- **Legacy schema aliases** (otio-core `canonical_schema`): `Sequence`→`Track`,
  `Filler`→`Gap`, `SerializeableCollection`→`SerializableCollection`. Dispatch
  canonicalizes; storage stays verbatim.

## Status / verification
- [x] `cargo test -p otio-algo` → 3 pass (exact `simple_cut` layout, transition
      smoke, `Sequence`-alias regression). clippy clean.
- [x] **Differential gates GREEN** vs Python OTIO over the corpus:
  - `range_of_child_at_index` → **12 files, 152 child ranges, 0 mismatches**
    (`examples/algo_dump.rs` + `tests/diff_algo.py`).
  - `flatten_stack` → **12 files, 66 flat children, 0 mismatches**
    (`examples/flatten_dump.rs` + `tests/diff_flatten.py`). NB: this gate
    compares *child layout only*. The output `Track` and padding `Gap` are built
    via `otio_schema::build::{track,gap}` so they carry the full canonical
    default fields (`kind`/`enabled`/`markers`/`effects`/`metadata`/`color`/…)
    the reference emits; that top-level field fidelity is gated by the otio-py
    full-DOM test `crates/otio-py/tests/diff_pyapi_algorithms.py`.
  - `transformed_time` (clip start -> top stack) → **12 files, 94 clips, 0
    mismatches**, incl. nested stacks (`examples/transform_dump.rs` +
    `tests/diff_transform.py`).
  - All skip big_int.otio. Build the examples then run the three `diff_*.py`.

### Found via the differential
- `Sequence.1` is the legacy name for `Track` (typeRegistry.cpp:88). A real timeline
  sample uses it; without aliasing, `range_of_all_children` returned `Err`/empty.
  Fixed centrally via `otio_core::canonical_schema`.

## TODO (next)
- [x] `flatten_stack` + `track_trimmed_to_range`.
- [x] `transformed_time` / `transformed_time_range` + `range_of_child`.
- [ ] `trimmed_range_of_child` + `trim_child_range` (trim child ranges to the
      composition's source_range; composition.cpp).
- [ ] Arena compaction to reclaim orphaned clones left by flatten.
