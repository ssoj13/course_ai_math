# GraphViewportConfig matrix

Shared viewport: `nodes_egui::GraphViewportState::ui(host, config, extras)`.

| Capability | `EDITOR` | `DIAGRAM` | `DIAGRAM_EDIT` |
|------------|----------|-----------|----------------|
| Pan / zoom / fit (H, A, F) | yes | yes | yes |
| Node drag (geometry) | yes | yes | yes |
| Marquee select | yes | yes | yes |
| Wiring | yes | no | yes |
| Delete selection | yes | no | yes |
| Auto-layout (L) | yes | no | yes |
| Resize handles | yes | no | no |
| View mode toggle (M) | yes | no | no |
| Inline widgets | yes | no | no |
| Orbit HUD | yes | no | no |
| `on_frame_end` / diagram sync | no | geometry only | full topology |
| Cook after frame | host (`commit_frame`) | host | host |

Constants: `GraphViewportConfig::EDITOR`, `::DIAGRAM`, `::DIAGRAM_EDIT` in `graph_viewport/config.rs`.

## Pick order

GPU/CPU pick uses `PickIndex::pick_topmost`: higher `PickLayer` wins (Pin > Title > Body); within a layer, later draw order wins.

## Hosts

- **nodes-editor** — `EDITOR` + `WireDropActions` (tab menu link search).
- **mermaid-rs `DiagramViewer`** — `DIAGRAM` or `DIAGRAM_EDIT` via `DiagramSession`.
