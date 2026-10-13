# CLAUDE.md — `crates/otio-edit`

> Dev conventions (Alex Joss, joss13@gmail.com): see `../../CONVENTIONS.md`.
> Full design + the 3-agent review it came from: see `PLAN.md` in this dir.

## What
The **editing engine** over the otio-core document model: high-level edit ops
(insert/ripple/razor/trim/…), a journal-of-primitives **undo/redo** with stable
node identity, a per-track **layout cache**, and a `revision()` change signal.
This is the layer a Rust+egui NLE timeline widget drives. otio-core stores +
serializes; otio-algo computes layout; otio-edit edits + records undo; the widget
renders. One responsibility per layer. Lib root `src/otio_edit.rs`.

## Architecture (locked by review — do not casually change)
- **`Editor`** owns the `Document` PRIVATELY; the only mutation path is
  `Editor::edit(label, |tx| …)`, which commits the recorded primitives as one
  undoable step (and rolls them back on `Err` → atomic ops). The widget reads
  `editor.document()`; a background render/export thread takes `editor.snapshot()`
  (the model is `Send + Sync + Clone`).
- **`Tx`** (`tx.rs`) is the recording wrapper: read-old-then-write, appends a
  `Prim` per change. Because `&mut Document` is reachable ONLY through a `Tx`,
  no code path can mutate without journaling → the "bypass corrupts undo" class
  is structurally impossible.
- **`journal.rs`**: `Prim` (Created, FieldSet{before,after}, ChildInserted,
  ChildRemoved, ChildSet{before,after}, MapChildSet{before,after}) — each stores
  BOTH directions so undo *and* redo are exact. `Transaction{label, prims}`.
- **Identity contract (the crux)**: undo NEVER frees a node. `Created` is a pure
  PIN MARKER (no-op undo/redo) — structural attach/detach is ALWAYS done by the
  attach prims (`ChildInserted`/`ChildSet`/`MapChildSet`), whose inverses detach a
  whole created/cloned subtree as a unit, keeping its internal links intact (an
  early bug detached per-created-node and tore cloned subtrees apart — caught by
  `deep_clone_subtree_round_trips_and_gc_pins_it`). `Editor::gc` pins every
  history-referenced id and, treating pinned ids as gc-roots, retains their whole
  subtrees. So a `NodeId` the widget/bindings hold stays valid across undo/redo.
  Freeing happens only in `gc`/`clear_history`, outside the undo horizon.
- **Single-point field traversal**: `Field::child_ids()` (otio-core) is the one
  place that enumerates a field's child links; gc-mark, remove_node's leaf check,
  and `record_created_subtree` route through it, and `unlink_child` is exhaustive
  — so a future `Field` variant is a compile error, not a silent dangling-id bug.
- **`layout.rs`**: `LayoutCache` memoizes per-track `range_of_all_children`,
  invalidated when `editor.revision()` advances (coarse v1; per-track dirty
  derived from the journal is the v2 refinement). Start-sorted → widget
  binary-searches the visible window.

## Status
- [x] Phase A (in otio-core): detach_from_parent, remove_node, remove_field_at/
  insert_field_at, gc(pinned), set_child_in_map detach fix. Unit-tested; corpus
  12/12 + all Python/adapter diffs still green.
- [x] Phase B core: journal/tx/editor (undo/redo/gc/revision), layout cache,
  minimal v1 ops (set_name, set_enabled, set_source_range, insert/append_child,
  ripple_delete). **8 tests green**: undo/redo round-trips to byte-identical JSON;
  field order preserved across remove→re-add→undo; Created+MapChildSet+deep_clone
  via Tx round-trip (incl. the empty-ChildMap and clone-subtree fixes); gc pins
  history-referenced subtrees then reclaims after clear_history; edit-after-undo
  clears redo; failed edit rolls back atomically; undo/redo past the ends are
  no-ops. Verified by a 3-agent adversarial review (journal/Phase-A/test-strength)
  which surfaced the two bugs now fixed + tested.
- [x] Phase B v1 completion: trim_in/out, slip, razor/split, lift, overwrite,
  add/remove/reorder track, add/remove marker; gap-merge in ripple_delete; per-op
  tests (23 total). Tx gained `build_gap`/`build_track`/`build_marker` (wrap
  `otio_schema::build::*` + record the created subtree) and
  `push_named_child`/`remove_named_child` (the `markers`/`effects` lists, which
  are a separate `Field::Children` from the `"children"` composition). journal.rs
  gained symmetric `NamedChildPushed`/`NamedChildRemoved` prims (undo↔redo exact,
  restoring an emptied list to canonical `[]`). EditError gained
  `From<otio_algo::AlgoError>` (maps `CannotTrimTransition`→`AcrossTransition`).
  Each op has the undo==byte-identical-JSON gate + transition / media-bounds
  refusal tests. overwrite covers: split-inside-a-clip (straddle both edges →
  head trim + cloned tail), span whole clips (remove), and past-track-end (pad a
  lead-in gap); it REFUSES a transition overlapping the window (no transition-aware
  overwrite in v1 — deferred). Verified by a 3-agent review (named-child prims /
  op math / test strength): fixed 1 real bug — `NamedChildPushed` undo now
  restores field ABSENCE (captures the pre-push `FieldState`), not an empty `[]`
  (the named-child analogue of the empty-ChildMap bug; regression-tested by
  `add_marker_undo_restores_absence_when_field_was_absent`); enforced the
  no-two-touching-gaps invariant in `lift`/`overwrite` too (not only
  `ripple_delete`); replaced a silent `let _ = push_named_child` with a
  debug-assert. **30 tests** (added overwrite-spanning, reorder-forward-move,
  cross-rate razor, 3-gap-merge, mid-list marker remove, lift-adjacent-gap). The
  op math was hand-traced correct (incl. overwrite spanning whole clips + razor
  rate-rescale); those paths now have regression locks.
- [x] Phase B v2: roll, slide, move_clip, three_point, snapping helper (`snap.rs`).
  **42 tests green** (12 new v2 tests). Each forward result is asserted (resulting
  layout starts/durations), then undo→byte-identical pre-state, redo→identical
  post-state; the pure `snap` has within/outside/tie-break unit tests.
  - **roll(left_clip, delta)**: A=left_clip, B=right sibling. `sr_A'=(sA, dA+delta)`,
    `sr_B'=(sB+delta, dB-delta)`. Clamps BOTH to media bounds (A's new end ≤ A.avail
    end; B's new start ≥ B.avail start; both new durs > 0). Refuses AcrossTransition
    if A/B is a transition (a transition between them IS the idx+1 child → caught).
    A Gap side skips the media-end/start check (gaps have no available_range) but
    still honours the dur>0 guard.
  - **slide(clip, delta)**: clip's own `source_range` is UNCHANGED; prev neighbor
    absorbs +delta on its tail, next neighbor absorbs -delta on its head. **Choice:**
    a Gap neighbor is RESIZED (refuse if it would go ≤0); a Clip neighbor rolls that
    shared edge (reuses roll's media-bounds math); a MISSING prev neighbor inserts a
    lead-in Gap for delta>0 (refuses delta<0 — no room before track start), a missing
    next neighbor is a no-op (nothing downstream to hold). `merge_adjacent_gaps` runs
    after (a lead-in gap can abut a pre-existing gap).
  - **move_clip(clip, to_index)**: lift+overwrite semantics (non-rippling NLE drag):
    lift the clip (leaves a same-dur Gap so downstream stays put), then overwrite it
    back in at the start-time of the child currently at `to_index` (clamped to track
    end). Reuses the lift inline + `overwrite_in_tx`. One atomic undoable step.
  - **three_point(track, src_clip, src_in, src_out, timeline_in)**: set
    `src_clip.source_range = (src_in, src_out-src_in)` (refuse if src_out≤src_in),
    then `overwrite_in_tx` at `timeline_in`. The timeline OUT is implied by the
    source window duration.
  - **snap.rs (pure, Editor-free)**: `snap(targets, candidate, threshold) ->
    RationalTime` returns the nearest target within `threshold` (distance in seconds,
    so mixed-rate edges order correctly), else `candidate`. Tie → FIRST target wins
    (with a `DEFAULT_EPSILON_S` tie tolerance so a float-ULP gap doesn't flip the
    pick); returns the target itself (rate preserved). `snap_targets(doc, track)`
    collects track-start(0) + each child's start_time + end_time_exclusive.
  - **Refactor**: `overwrite`'s body was extracted to `overwrite_in_tx(tx, …)` so
    `overwrite`/`move_clip`/`three_point` share ONE placement codepath (single
    point of responsibility).
  - **Deferred (honest):** transition-aware overwrite — still REFUSES a transition
    overlapping the window (the v1 limitation carries over; `overwrite_in_tx` is the
    one place to add it later). Per-track ChangeSet-driven incremental relayout — the
    layout cache still invalidates on `revision()` (coarse). Neither is faked.
  - **2-agent review (op math / test coverage):** the op math was hand-traced
    correct (no wrong-but-reversible layout bug). Fixed 1 real fidelity bug a
    coverage test caught — `roll` against a **Gap** B-neighbor was writing a
    non-canonical `source_range` start (`sB+δ`); a Gap has no media, so its start
    stays anchored at 0 (the timeline shift is carried by cumulative durations).
    47 tests now (+5: slide two-clip-neighbors, slide-at-track-start lead-in gap,
    move_clip backward, roll gap-neighbor, snap mixed-rate). Two spec-consistent
    caveats documented as TODOs: mixed source-rate roll/slide can emit a
    fractional-frame `source_range` (exact per the math, like the reference — no
    frame-grid snap); `three_point` does not validate that `(src_in, src_out)` fits
    the source media `available_range` (a playback concern, not a layout error).
- [x] Phase B v2.1 — clip-addressed verbs for the timeline widget (which addresses
  items by `NodeId`, not index): **`move_by(clip, delta)`** — free reposition by a
  time delta (lift + `overwrite_in_tx` at `current_start+delta`, clamped ≥0; leaves
  a same-dur gap, downstream stays put, neighbours NOT rolled — the "move a clip and
  leave a gap" gesture, vs `slide`'s "roll the neighbours"). **`lift_clip`** /
  **`ripple_delete_clip`** — thin wrappers that resolve `track_and_index` then call
  `lift`/`ripple_delete`, so the widget never juggles indices. Tests: move_by leaves
  the prev clip's `source_range` untouched + opens a gap; lift replaces the clip with
  a same-dur gap (footprint kept); ripple removes + closes up; ripple on a **Gap**
  closes it (the widget's Premiere-style Delete-on-a-gap). All undo/redo round-trip.
  **56 tests green.**

## Gotchas
- Ops must go through `Tx` — never reach for `editor`'s document mutably another
  way (there is no such path, by design; keep it that way).
- Edit math (positions, trims) reads via `tx.doc()` / otio-algo, then applies via
  `Tx`. Trimming into a `Transition` must error (`EditError::AcrossTransition`);
  slip/trim must clamp to media `available_range` (`OutOfMediaBounds`).
- `gc` is O(arena); call it on save/idle, not per-keystroke.

## Verify
`cargo test -p otio-edit` (undo/redo/gc gates) + `cargo test -p otio-core`
(Phase A primitives) + `cargo clippy -p otio-core -p otio-edit --all-targets -D warnings`.
The undo round-trip test (apply script → undo all → identical JSON) is the
strongest correctness gate; extend it as ops are added.
