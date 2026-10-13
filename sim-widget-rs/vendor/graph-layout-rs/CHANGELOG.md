# Changelog

Pre-1.0; format loosely follows [Keep a Changelog](https://keepachangelog.com/).

## [0.1.0] — 2026-06-19
Initial standalone release. Extracted from `nodes-rs` (originally `mermaid-rs`)
into its own private repo (`ssoj13/graph-layout-rs`) for reuse via git-ref.

### Notes
- No functional changes: the engine is intentionally **dependency-free** (plain
  `[f32; 2]` coords + `u64` node ids), exposing the `LayoutEngine` trait + a
  Sugiyama (layered DAG) implementation.
- The GPU force-directed implementation of the same trait (`graph-layout-gpu`)
  stays in `nodes-rs` so this crate pulls in no `wgpu`.
- Consumers (in `nodes-rs`): `nodes-core`, `graph-layout-gpu`, `nodes-editor`.
