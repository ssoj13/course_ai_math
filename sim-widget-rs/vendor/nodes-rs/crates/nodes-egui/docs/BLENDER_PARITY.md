# Blender node-editor parity (P9)

> **Scope note (added 2026-07-05):** this checklist describes the **`nodes-egui`** viewport
> specifically (this crate's own `TabMenu`/pick/marquee implementation, consumed by
> `mermaid-rs` and `vfx-rs`). It does **not** describe `nodes-editor`'s standalone
> `wgpu_app.rs` shell — per `nodes-editor/CLAUDE.md`, that is "a separate, older egui-based
> viewport (not this app)" from `nodes-editor`'s point of view. The two crates can have
> different feature sets; do not use this file as a status reference for `nodes-editor`.
> (`nodes-editor` itself has undo/redo, delete, copy/paste/duplicate, and save/load as of
> 2026-07-05 — see `report.md` at the repo root.)

Reference: [`md.old/plan3-blender-node-editor-parity.md`](../../../md.old/plan3-blender-node-editor-parity.md)

## P0 checklist (target feel)

| Feature | Status | Notes |
|---------|--------|-------|
| Pan / zoom | Done | `Camera`, zoom-to-cursor, MMB pan |
| Node drag | Done | title-bar drag |
| Wire drag | Done | pin drag + preview |
| Pick priority | Done | pin > title > body + occlusion |
| Marquee select | Done | empty-space drag |
| Delete | Done | Delete / Backspace |
| Tab spawn menu | Done | `TabMenu` |
| Draw order | Done | pick layer + instance index |
| Pin shapes (SDF) | Done | `shape_sdf.wgsl` |
| Inline socket values | Partial | `TypeWidget` on some types |
| Zoom-aware pick radius | Done | `pick_padding_world` |

## Deferred (post-v1)

- GPU `pick_id` debug buffer
- Link search menu on empty wire drop (spawn filter)
- Panel collapse / geometry-nodes panels
- 6-DOF spatial graph navigation
