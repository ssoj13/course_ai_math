# egui-jobs-table

A standalone, generic **async-job / task-queue table** for egui 0.34.

## What it is

A reusable widget that renders a list of background jobs as a sortable,
filterable table. Each row carries:

- a coloured **status badge** (Queued / Running / Done / Failed / Cancelled),
- caller-supplied **data columns** (pre-formatted strings),
- an optional **progress bar** (shown for `Running` rows with a known fraction),
- per-row **action buttons** (Cancel / Retry / Open / Remove),
- a **selection checkbox**.

The toolbar above the table gives a free-text filter and a status dropdown.
Clicking a header sorts by that column (re-click flips direction).

## Why it exists / where it came from

Gathered from playa's `playa-jobs-ui` crate (`panel.rs`) and **decoupled** from
its host engine `playa-jobs-core` (`JobQueue`, `Job`, `JobState`). The goal is a
table any egui app can drop in without dragging in a concrete job queue, async
runtime, or network code.

## Model (flat, caller-owned)

The widget owns **no** job state. The host passes data in and gets actions out:

```rust
JobRow {
    id: u64,                 // stable key, echoed back in every action
    columns: Vec<String>,    // one pre-formatted cell per header column
    status: JobStatus,       // Queued | Running | Done | Failed | Cancelled
    progress: Option<f32>,   // 0.0..=1.0, only meaningful while Running
    actions: RowActions,     // which buttons this row offers
}

RowActions { cancel, retry, open, remove }   // four bools
// RowActions::for_status(status) derives a sensible default set.

JobsTableState {             // host-owned, persists across frames
    sort: Option<SortKey>,   // SortKey::Column(usize) | SortKey::Status
    sort_descending: bool,
    filter_text: String,
    status_filter: Option<JobStatus>,
    selected: Vec<u64>,      // kept sorted + deduped (serde-friendly)
}
```

### Entry point

```rust
let actions: Vec<JobAction> =
    JobsTable::new(headers)        // Vec<String>, left-to-right data columns
        .show(ui, &rows, &mut state);
```

`JobsTable` builders: `.without_toolbar()`, `.row_height(px)`.

### Action enum (returned for the host to apply)

```rust
enum JobAction {
    Cancel { id: u64 },
    Retry  { id: u64 },
    Open   { id: u64 },
    Remove { id: u64 },
    Select { id: u64, selected: bool },   // checkbox toggled
}
```

Sort and filter are handled **internally** in `JobsTableState` — there is no
`Sort` action; the host doesn't need to do anything for sorting. (Selection is
likewise tracked in `state.selected`; the `Select` action is emitted only so a
host that mirrors selection elsewhere can react.)

Selecting a row: the checkbox toggles it, and **clicking a data/status cell**
(list-style `selectable_label`, no text I-beam) toggles the same selection. The
action-button column is left non-selectable so its buttons never double as a
row select. (Full ctrl/shift range-select would need a batch `JobAction`
variant — deferred.)

## Gotchas

- **Columns are strings.** Sorting is lexicographic, case-insensitive, over the
  pre-formatted cell text. For numeric columns, format with zero-padding (or a
  fixed width) if you need numeric order. The widget never parses cells.
- **`headers` excludes the built-in columns.** The selection checkbox, the
  status badge, and the action/progress cell get their own columns
  automatically — don't list them in `headers`.
- **`columns` length should match `headers`.** Short vectors render blank cells;
  extra entries are ignored. No panic either way.
- **Progress only shows while `Running`.** `progress: None` on a running row
  renders a plain "Running…" label instead of a bar.
- **`serde` is optional.** Enable the `serde` feature to derive
  `Serialize`/`Deserialize` on `JobsTableState`, `JobStatus`, and `SortKey` for
  persisting view state across runs.

## What was dropped (vs. the playa source)

- **The Seedance submit dialog** (`dialog.rs`): provider-specific endpoints
  (text-to-video / image-to-video), per-second fal.ai cost pricing, the params
  builder, and the batch-prompt splitter. None of it is generic. A tiny generic
  "are you sure / simple form" modal was **not** added — it isn't trivial to do
  well and is out of scope for a table widget; add one separately if needed.
- **The opaque "Cost" sort column** and the footer cost/`USD` stats: those read
  `result["bytes"]` / a cost model out of `playa-jobs-core`. Replaced by generic
  sorting over the caller's own columns.
- **`serde_json::Value` job params / results**: the widget takes plain
  `Vec<String>` cells, so no JSON dependency.
- The richer 8-variant `JobState` was collapsed to 5 generic buckets
  (`Submitting`/`Downloading`/`Staging`/`AwaitingProvider` → `Running`,
  `Pending` → `Queued`, `Complete` → `Done`).

## Layout

- `src/egui_jobs_table.rs` — crate root: docs + public re-exports (the `[lib]
  path`).
- `src/table.rs` — the model (`JobRow`, `JobStatus`, `RowActions`,
  `JobAction`, `SortKey`, `JobsTableState`), the `JobsTable` renderer, pure
  filter/sort helpers, and unit tests.

## Build / verify

Standalone — the crate's `Cargo.toml` carries its own bare `[workspace]` table,
so it is **not** a member of the egui-widgets-rs root workspace and builds in
isolation:

```
cargo check                                          # zero errors / warnings
cargo clippy --all-targets -- -D warnings            # clippy clean
cargo clippy --all-targets --features serde -- -D warnings
cargo test                                           # 10 unit tests + 1 doctest
```

## TODO / notes

- [ ] (optional) bulk action bar (cancel/retry/remove the current selection) —
      the playa source had one in a bottom panel; selection state is already
      tracked here, so a host can build it, or it could be added behind a
      builder flag later.
- egui 0.34.3 self-reports `requires Rust 1.88`-incompatible (`1.92`) in its
  metadata, but compiles fine on the local toolchain. The crate keeps
  `rust-version = "1.88"` per the workspace convention; bump if a stricter
  toolchain check is enforced.
