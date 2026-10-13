# nodes-rs — Module structure (usd-view template)

**Reference:** `vfx-rs/crates/geo/vfx-usd/crates/usd-view`  
**Method:** copy folder layout and responsibilities, replace USD/`Engine` with `Subnet`/`GraphRenderer`. **Do not** copy `snarl/` or legacy `nodes-rs/src/snarl`.

**Graph model & paths:** [`NODES.md`](./NODES.md) (scene = root `Subnet`, Houdini-style op paths, `name` vs `NodeId`).

---

## Layer map

```text
vfx-usd                              nodes-rs
────────────────────────────────────────────────────────────
usd-core, Stage                        nodes-core (Subnet, RustBox)
usd-imaging::gl::Engine                nodes-view::GraphRenderer
  set_shared_wgpu_context                attach_wgpu(...)
  render → wgpu texture                  render → color_texture_view()
  pick_at_pixel                          pick_at_pixel / hit_test

panels/viewport/gpu_display.rs         nodes-view::present::GpuDisplay
panels/viewport/mod.rs (ui_viewport)   nodes-editor::panels::graph_viewport
usd-view app + launcher                nodes-editor::GraphEditorApp
register_native_texture                nodes-egui::GraphGpuBridge
```

**Public API = `GraphRenderer` + wgpu** (like `Engine`), not `ui_viewport`.

---

## Workspace

```text
mermaid-rs/crates/
  nodes-core/          # headless
  nodes-view/          # wgpu API
  nodes-egui/          # egui bridge only
  nodes-editor/        # copy usd-view/src skeleton
  nodes-rs/            # LEGACY — remove after migration
    docs/STRUCTURE.md
    docs/TARGET.md     # phase 0: from md.old/task.md (when written)
    ROADMAP.md
    # (no local task.md — use ../../../md/TODO.md)
```

---

## `nodes-core` (no wgpu, no egui)

```text
nodes-core/src/
  lib.rs
  rustbox/           mod.rs, path.rs, value.rs
  graph/             node.rs, subnet.rs, connection.rs, attr.rs
  cook/              solver.rs, executor.rs, node_types.rs, errors.rs
  layout/            static_layout.rs, energy.rs (stub)
  bus/               mod.rs (stub)
  serde/             mod.rs
  paste/             mod.rs
tests/
docs/TARGET.md
```

---

## `nodes-view` (wgpu — canonical API)

```text
nodes-view/src/
  lib.rs
  engine/
    mod.rs           # GraphRenderer (≈ Engine)
    params.rs        # RenderParams
    attach.rs        # attach_wgpu
    invalidate.rs    # Reload | SceneSwitch | DeviceDeath
  camera/
    mod.rs, flat.rs, spatial.rs (stub)
  render/
    mod.rs, frame.rs
    passes/          clear, grid, wires, nodes, pick_id, preview (stub)
    pipelines/       *.wgsl
    resources.rs
  pick/              mod.rs, ray.rs (stub)
  layout_cache.rs
  present/
    mod.rs
    gpu_display.rs   # adapt from usd-view panels/viewport/gpu_display.rs
  preview/           mod.rs (stub)
examples/winit_minimal.rs
```

```rust
// Public surface (mirror usd_imaging::gl::Engine)
pub struct GraphRenderer;
impl GraphRenderer {
    pub fn new(desc: GraphRendererDesc) -> Self;
    pub fn attach_wgpu(&mut self, adapter: &wgpu::Adapter, device: &wgpu::Device, queue: &wgpu::Queue);
    pub fn resize(&mut self, width: u32, height: u32);
    pub fn render(&mut self, params: &RenderParams) -> Result<(), RenderError>;
    pub fn color_texture_view(&self) -> Option<&wgpu::TextureView>;
    pub fn pick_at_pixel(&mut self, x: i32, y: i32, params: &RenderParams) -> Option<PickHit>;
}
```

Input: `nodes-view/src/input.rs` — pointer/scroll in **viewport pixels**, no egui types.

---

## `nodes-egui` (optional host)

```text
nodes-egui/src/
  lib.rs
  gpu_bridge.rs        # ViewportGpuState pattern → GraphGpuBridge
  viewport_panel.rs    # optional ui helper
```

---

## `nodes-editor` (copy `usd-view/src`)

### Copy matrix

| usd-view | nodes-editor | Notes |
|----------|--------------|-------|
| `launcher.rs` | `launcher.rs` | Wgpu eframe setup |
| `app/mod.rs` | `app/mod.rs` | `Engine` → `GraphRenderer` |
| `app/panels.rs` | `app/panels.rs` | |
| `app/actions.rs` | `app/actions.rs` | graph actions |
| `app/sync.rs` | `app/sync.rs` | cook + selection sync |
| `app/file_ops.rs` | `app/file_ops.rs` | JSON graph |
| `app/toolbar.rs` | `app/toolbar.rs` | |
| `data_model.rs` | `data_model.rs` | Root / View / Selection models |
| `camera/` | `camera/` | use `nodes_view::Camera` (3D ortho eye/target/up) |
| `dock.rs` | `dock.rs` | DockTab::Graph |
| `event_bus.rs` | `event_bus.rs` | copy |
| `events.rs` | `events.rs` | GraphLoaded, etc. |
| `keyboard.rs` | `keyboard.rs` | |
| `menus/*` | `menus/*` | trim USD items |
| `panels/viewport/mod.rs` | `panels/graph_viewport/mod.rs` | **main adapt** |
| `panels/viewport/context_menu.rs` | `panels/graph_viewport/context_menu.rs` | |
| `panels/viewport/gpu_display.rs` | → `nodes-view/present/` | not in editor |
| `panels/camera_controls.rs` | `panels/camera_controls.rs` | copy |
| `panels/hud.rs` | `panels/hud.rs` | copy |
| `panels/overlays.rs` | `panels/overlays.rs` | copy |
| `panels/pick.rs` | `panels/pick_fallback.rs` | CPU fallback |
| `panels/attr_editor.rs` | `panels/attr_editor.rs` | RustBox |
| `panels/prim_tree*.rs` | `panels/subnet_tree.rs` | navigation |
| — | `panels/tab_menu/` | from legacy nodes-rs |
| — | `panels/wire_interaction.rs` | new |
| — | `nodes/builtin/*.rs` | one file per node |
| `persistence.rs` | `persistence.rs` | |
| `status_bar.rs` | `status_bar.rs` | |
| `bounds.rs` | `bounds.rs` | fit graph bbox |
| `screenshot.rs` | `screenshot.rs` | |
| `file_watcher.rs` | `file_watcher.rs` | |

### Target tree

```text
nodes-editor/src/
  lib.rs
  launcher.rs
  app/               mod, panels, actions, sync, file_ops, toolbar
  data_model.rs
  camera/mod.rs
  dock.rs
  event_bus.rs
  events.rs
  keyboard.rs
  menus/
  panels/
    graph_viewport/    mod.rs, context_menu.rs, state.rs
    camera_controls.rs
    hud.rs
    overlays.rs
    attr_editor.rs
    tab_menu/
    subnet_tree.rs
    wire_interaction.rs
    preferences.rs
  nodes/builtin/
  persistence.rs
  status_bar.rs
  bounds.rs
  screenshot.rs
  file_watcher.rs
```

### `configure_wgpu_render_state` (same as usd-view)

```rust
pub fn configure_wgpu_render_state(&mut self, rs: egui_wgpu::RenderState) {
    self.renderer.attach_wgpu(&rs.adapter, &rs.device, &rs.queue, rs.target_format);
    self.gpu_bridge.set_render_state(rs.clone());
}
```

Per frame in `ui_graph_viewport`:

1. `ui.allocate_exact_size` → rect  
2. `renderer.render(w, h, &params)`  
3. `present::GpuDisplay::present_renderer_color(...)`  
4. `ui.image(gpu_bridge.texture_id(), ...)`

---

## mermaid-rs host

```text
src/widget/graph_viewport.rs    # nodes-view + nodes-egui
src/adapter/mermaid_to_subnet.rs
```

Delete: `Snarl`, `GraphViewer` SnarlViewer.

---

## Migration steps

| # | Action |
|---|--------|
| 1 | Add workspace members: nodes-core, nodes-view, nodes-egui, nodes-editor |
| 2 | Copy usd-view `launcher.rs`, `app/`, `dock.rs`, `data_model.rs` skeleton |
| 3 | Implement nodes-core from TARGET.md |
| 4 | Copy/adapt `gpu_display.rs` → nodes-view/present/ |
| 5 | Implement GraphRenderer (Engine shape) |
| 6 | Adapt `panels/viewport/mod.rs` → graph_viewport |
| 7 | Port tab_menu + builtin nodes from legacy nodes-rs |
| 8 | Delete nodes-rs/src/snarl |
| 9 | mermaid widget phase 6 |

---

## Dependency rules

```text
nodes-core   → (stdlib only + serde)
nodes-view   → nodes-core, wgpu
nodes-egui   → nodes-view, egui, egui-wgpu
nodes-editor → nodes-core, nodes-view, nodes-egui, eframe/wgpu
```

---

## Future stubs (create early)

| File | Feature |
|------|---------|
| `nodes-core/layout/energy.rs` | continuous layout |
| `nodes-core/bus/mod.rs` | Data Bus |
| `nodes-view/camera/spatial.rs` | space-sim fly |
| `nodes-view/pick/ray.rs` | 3D pick |
| `nodes-view/render/passes/preview.rs` | live 2D/3D in node |

---

*See ROADMAP.md for phases. Template: usd-view @ vfx-rs.*
