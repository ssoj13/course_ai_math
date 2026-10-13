# graph-layout-rs

A **generic, dependency-free node-graph layout engine**: feed it an abstract graph
(nodes with sizes + directed edges), get node positions out. No `glam`, no `wgpu`,
no `nodes-core` — it works on plain `[f32; 2]` coordinates and opaque `u64` node
ids, so any host (node editor, mermaid, future apps) can drive it through a tiny
adapter.

The crate links as **`graph_layout`** (package `graph-layout`).

## Use

```toml
[dependencies]
graph-layout = { git = "ssh://git@github.com/ssoj13/graph-layout-rs.git", branch = "main", package = "graph-layout" }
```

```rust
use graph_layout::*; // abstract graph in -> positions out
```

## What's inside

- `graph_layout.rs` — the public engine: an abstract graph (node sizes + directed
  edges) and a `LayoutEngine` trait; returns one `[f32; 2]` position per node id.
- `sugiyama.rs` — a layered (Sugiyama-style) DAG layout implementation.

GPU-accelerated force-directed layout that implements the same `LayoutEngine`
trait lives separately in `nodes-rs` (`graph-layout-gpu`), so this crate stays
dependency-free.

## Build

```sh
cargo check
cargo test
```

## Provenance

Extracted from `mermaid-rs` (then `nodes-rs`) on 2026-06-19 into its own private
repo for reuse. Consumers: `nodes-core`, `graph-layout-gpu`, `nodes-editor` (all
in `nodes-rs`). See `CLAUDE.md` for design notes.

## License

MIT.
