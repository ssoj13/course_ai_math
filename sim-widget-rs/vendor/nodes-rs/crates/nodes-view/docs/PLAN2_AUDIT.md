# plan2-node-editor-sdf-sensing — audit (2026-05-26)

Source: [`md.old/plan2-node-editor-sdf-sensing.md`](../../../md.old/plan2-node-editor-sdf-sensing.md)

| Item | Status | Evidence |
|------|--------|----------|
| Filled node body + title + pins via shape batch | Done | `shape_scene.rs`, `shape_sdf.wgsl` |
| Pin shapes match `PinShape` | Done | `pin_shape_to_sdf` |
| LMB pick uses `ShapeScene` / `PickIndex` | Done | `pick_index.rs`, `nodes-egui` viewport |
| Hover distinct from selection | Done | outline stroke in `shape_scene.rs` |
| Pick priority: pin > title > body | Done | `pick_index` tests `pin_beats_body_overlap` |
| Later body wins same layer | Done | `later_body_wins_same_layer` |
| Pin occlusion (socket behind node) | Done | `resolve_pick` + `pin_occluded_by_later_body` |
| No legacy per-node pick loops | Done | grep: no `pick_node_body` / `pick_pin_at` |
| Stress ~500 bodies | Done | `stress_grid_many_instances` |
| ~50k pick perf | Bench | `benches/pick_index.rs` (criterion) |
| Interactive winit example | Done | `examples/winit_interactive.rs` |
| Optional GPU `pick_id` debug | Deferred | not required for v1; CPU pick is canonical |
| Padding / `pick_padding` | Done | viewport passes padding into `pick_topmost` |

**Gaps (non-blocking v1):** GPU pick_id overlay, criterion gate in CI (run locally: `cargo bench -p nodes-view --bench pick_index`).
